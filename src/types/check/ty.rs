//! The types the checker works with (docs/docs/types.md, section 2).

use std::fmt;
use std::rc::Rc;

/// A declared interface, class or type alias: an index into the
/// checker's declarations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeclId(pub usize);

#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    /// Dynamic: anything, checked when it enters a typed place.
    Any,
    /// Anything, but it must be narrowed before it is used as a type.
    Unknown,
    Never,
    Void,
    Undefined,
    Null,
    Number,
    String,
    Boolean,
    /// A string literal type, `"a"`.
    StrLit(Rc<str>),
    Array(Rc<Ty>),
    /// `[A, B]`: an array with these elements, in order.
    Tuple(Rc<[Ty]>),
    /// An object type written out: `{ x: number; y?: string }`.
    Object(Rc<ObjTy>),
    /// An interface, a type alias, or an instance of a class.
    Named(DeclId),
    /// A class itself: what `new` is applied to.
    ClassValue(DeclId),
    Function(Rc<FnTy>),
    Union(Rc<[Ty]>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prop {
    pub name: String,
    pub ty: Ty,
    pub optional: bool,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ObjTy {
    pub props: Vec<Prop>,
    /// `[key: string]: T`
    pub index: Option<Ty>,
}

impl ObjTy {
    pub fn get(&self, name: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.name == name)
    }

    /// Add `prop`, replacing one of the same name.
    pub fn set(&mut self, prop: Prop) {
        match self.props.iter_mut().find(|p| p.name == prop.name) {
            Some(existing) => *existing = prop,
            None => self.props.push(prop),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Ty,
    /// `x?: T`, or a parameter with a default value.
    pub optional: bool,
    /// `...xs: T[]`; `ty` is the element type.
    pub rest: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FnTy {
    pub params: Vec<Param>,
    pub ret: Ty,
}

impl FnTy {
    /// How many arguments a call must pass.
    pub fn required(&self) -> usize {
        self.params
            .iter()
            .take_while(|p| !p.optional && !p.rest)
            .count()
    }

    /// The type of the parameter that argument `i` goes to.
    pub fn param(&self, i: usize) -> Option<&Ty> {
        match self.params.get(i) {
            Some(p) => Some(&p.ty),
            None => self.params.last().filter(|p| p.rest).map(|p| &p.ty),
        }
    }
}

impl Ty {
    pub fn array(elem: Ty) -> Ty {
        Ty::Array(Rc::new(elem))
    }

    pub fn function(params: Vec<Param>, ret: Ty) -> Ty {
        Ty::Function(Rc::new(FnTy { params, ret }))
    }

    /// The union of `types`: nested unions flattened, duplicates removed,
    /// and `any` absorbing everything. A string literal is dropped when
    /// `string` is a member.
    pub fn union(types: impl IntoIterator<Item = Ty>) -> Ty {
        let mut members: Vec<Ty> = Vec::new();
        let mut stack: Vec<Ty> = types.into_iter().collect();
        stack.reverse();
        while let Some(t) = stack.pop() {
            match t {
                Ty::Any => return Ty::Any,
                Ty::Never => {}
                Ty::Union(inner) => stack.extend(inner.iter().rev().cloned()),
                t => {
                    if !members.contains(&t) {
                        members.push(t);
                    }
                }
            }
        }
        if members.contains(&Ty::Unknown) {
            return Ty::Unknown;
        }
        if members.contains(&Ty::String) {
            members.retain(|t| !matches!(t, Ty::StrLit(_)));
        }
        match members.len() {
            0 => Ty::Never,
            1 => members.pop().unwrap(),
            _ => Ty::Union(members.into()),
        }
    }

    /// The members of a union, or the type itself.
    pub fn members(&self) -> &[Ty] {
        match self {
            Ty::Union(members) => members,
            other => std::slice::from_ref(other),
        }
    }

    /// `any` or `unknown`: nothing is known about the value.
    pub fn is_dynamic(&self) -> bool {
        matches!(self, Ty::Any | Ty::Unknown)
    }

    pub fn is_nullish(&self) -> bool {
        matches!(self, Ty::Null | Ty::Undefined | Ty::Void)
    }

    /// The elements of a tuple as one type: what an element at an
    /// unknown position is.
    pub fn tuple_element(elems: &[Ty]) -> Ty {
        Ty::union(elems.iter().cloned())
    }

    /// The type of a literal as a variable holds it: `"a"` is a `string`.
    pub fn widened(&self) -> Ty {
        match self {
            Ty::StrLit(_) => Ty::String,
            Ty::Union(members) => Ty::union(members.iter().map(Ty::widened)),
            other => other.clone(),
        }
    }
}

/// How to print a type: declarations are printed by name.
pub struct Show<'a> {
    pub ty: &'a Ty,
    pub names: &'a dyn Fn(DeclId) -> String,
}

impl fmt::Display for Show<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let show = |ty| Show {
            ty,
            names: self.names,
        };
        match self.ty {
            Ty::Any => write!(f, "any"),
            Ty::Unknown => write!(f, "unknown"),
            Ty::Never => write!(f, "never"),
            Ty::Void => write!(f, "void"),
            Ty::Undefined => write!(f, "undefined"),
            Ty::Null => write!(f, "null"),
            Ty::Number => write!(f, "number"),
            Ty::String => write!(f, "string"),
            Ty::Boolean => write!(f, "boolean"),
            Ty::StrLit(s) => write!(f, "\"{s}\""),
            Ty::Array(elem) => match elem.as_ref() {
                Ty::Union(_) | Ty::Function(_) => write!(f, "({})[]", show(elem)),
                _ => write!(f, "{}[]", show(elem)),
            },
            Ty::Tuple(elems) => {
                write!(f, "[")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", show(e))?;
                }
                write!(f, "]")
            }
            Ty::Object(obj) => {
                if obj.props.is_empty() && obj.index.is_none() {
                    return write!(f, "{{}}");
                }
                write!(f, "{{ ")?;
                for p in &obj.props {
                    let mark = if p.optional { "?" } else { "" };
                    write!(f, "{}{}: {}; ", p.name, mark, show(&p.ty))?;
                }
                if let Some(index) = &obj.index {
                    write!(f, "[key: string]: {}; ", show(index))?;
                }
                write!(f, "}}")
            }
            Ty::Named(id) => write!(f, "{}", (self.names)(*id)),
            Ty::ClassValue(id) => write!(f, "typeof {}", (self.names)(*id)),
            Ty::Function(func) => {
                write!(f, "(")?;
                for (i, p) in func.params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    if p.rest {
                        write!(f, "...{}: {}[]", p.name, show(&p.ty))?;
                    } else {
                        let mark = if p.optional { "?" } else { "" };
                        write!(f, "{}{}: {}", p.name, mark, show(&p.ty))?;
                    }
                }
                write!(f, ") => {}", show(&func.ret))
            }
            Ty::Union(members) => {
                for (i, m) in members.iter().enumerate() {
                    if i > 0 {
                        write!(f, " | ")?;
                    }
                    match m {
                        Ty::Function(_) => write!(f, "({})", show(m))?,
                        _ => write!(f, "{}", show(m))?,
                    }
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unions_flatten_and_absorb() {
        let u = Ty::union([Ty::Number, Ty::union([Ty::Null, Ty::Number])]);
        assert_eq!(u, Ty::Union(vec![Ty::Number, Ty::Null].into()));
        assert_eq!(Ty::union([Ty::Number, Ty::Any]), Ty::Any);
        assert_eq!(Ty::union([Ty::Number, Ty::Never]), Ty::Number);
        assert_eq!(Ty::union([]), Ty::Never);
        assert_eq!(Ty::union([Ty::StrLit("a".into()), Ty::String]), Ty::String);
    }

    #[test]
    fn printing() {
        let names = |_: DeclId| "Point".to_string();
        let show = |ty: &Ty| Show { ty, names: &names }.to_string();
        assert_eq!(show(&Ty::union([Ty::Number, Ty::Null])), "number | null");
        assert_eq!(show(&Ty::array(Ty::Named(DeclId(0)))), "Point[]");
        assert_eq!(
            show(&Ty::array(Ty::union([Ty::Number, Ty::String]))),
            "(number | string)[]"
        );
        let f = Ty::function(
            vec![Param {
                name: "a".into(),
                ty: Ty::Number,
                optional: true,
                rest: false,
            }],
            Ty::Void,
        );
        assert_eq!(show(&f), "(a?: number) => void");
    }
}
