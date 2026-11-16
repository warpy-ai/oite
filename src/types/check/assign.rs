//! Assignability: whether a value of one type may go where another type
//! is declared.

use super::*;

/// The members of an object-like type, and whether the list is complete.
pub(super) struct Members {
    pub obj: ObjTy,
    /// False when the value may have members the checker does not know
    /// (a class instance, whose constructor may add any property).
    pub closed: bool,
}

impl Checker {
    /// `ty` with type aliases replaced by what they stand for.
    pub(super) fn expand(&self, ty: &Ty) -> Ty {
        let mut ty = ty.clone();
        for _ in 0..32 {
            match &ty {
                Ty::Named(id) => match &self.decls[id.0] {
                    TypeDecl::Alias { ty: target, .. } => ty = target.clone(),
                    TypeDecl::Pending(_) => return Ty::Any,
                    _ => return ty,
                },
                _ => return ty,
            }
        }
        Ty::Any
    }

    /// Whether class `a` is `b` or extends it.
    pub(super) fn is_subclass(&self, a: DeclId, b: DeclId) -> bool {
        let mut cur = Some(a);
        for _ in 0..64 {
            match cur {
                Some(id) if id == b => return true,
                Some(id) => cur = self.class(id).and_then(|c| c.parent),
                None => return false,
            }
        }
        false
    }

    /// The declared member `name` of an instance of class `id`, looking
    /// through its parent classes.
    pub(super) fn class_member(&self, id: DeclId, name: &str) -> Option<Prop> {
        let mut cur = Some(id);
        for _ in 0..64 {
            let class = self.class(cur?)?;
            if let Some(prop) = class.instance.get(name) {
                return Some(prop.clone());
            }
            cur = class.parent;
        }
        None
    }

    /// The members of `ty` (already expanded), if it is object-like.
    pub(super) fn members(&self, ty: &Ty) -> Option<Members> {
        match ty {
            Ty::Object(obj) => Some(Members {
                obj: (**obj).clone(),
                closed: true,
            }),
            Ty::Named(id) => match &self.decls[id.0] {
                TypeDecl::Interface { obj, .. } => Some(Members {
                    obj: obj.clone(),
                    closed: true,
                }),
                TypeDecl::Class(_) => {
                    // Parents first, so a subclass's member replaces its
                    // parent's.
                    let mut chain = Vec::new();
                    let mut cur = Some(*id);
                    while let Some(c) = cur.and_then(|c| self.class(c)) {
                        chain.push(c);
                        cur = c.parent;
                        if chain.len() > 64 {
                            break;
                        }
                    }
                    let mut obj = ObjTy::default();
                    for class in chain.iter().rev() {
                        for prop in &class.instance.props {
                            obj.set(prop.clone());
                        }
                    }
                    Some(Members { obj, closed: false })
                }
                _ => None,
            },
            Ty::Array(_) | Ty::Tuple(_) | Ty::Function(_) | Ty::ClassValue(_) => Some(Members {
                obj: ObjTy::default(),
                closed: false,
            }),
            _ => None,
        }
    }

    /// Whether a value of type `src` may be used where `dst` is declared.
    pub(super) fn assignable(&self, src: &Ty, dst: &Ty) -> bool {
        self.assignable_at(src, dst, 0)
    }

    fn assignable_at(&self, src: &Ty, dst: &Ty, depth: usize) -> bool {
        // Recursive types: assume the best past a reasonable depth.
        if depth > 12 {
            return true;
        }
        let src = self.expand(src);
        let dst = self.expand(dst);
        if src == dst {
            return true;
        }
        let next = depth + 1;
        match (&src, &dst) {
            (_, Ty::Any | Ty::Unknown) => true,
            (Ty::Any | Ty::Never, _) => true,
            (Ty::Unknown, _) => false,
            (Ty::Union(members), _) => members.iter().all(|m| self.assignable_at(m, &dst, next)),
            (_, Ty::Union(members)) => members.iter().any(|m| self.assignable_at(&src, m, next)),
            (Ty::StrLit(_), Ty::String) => true,
            (Ty::Undefined, Ty::Void) | (Ty::Void, Ty::Undefined) => true,
            (Ty::Array(a), Ty::Array(b)) => self.assignable_at(a, b, next),
            (Ty::Tuple(a), Ty::Tuple(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .zip(b.iter())
                        .all(|(x, y)| self.assignable_at(x, y, next))
            }
            (Ty::Tuple(a), Ty::Array(b)) => a.iter().all(|x| self.assignable_at(x, b, next)),
            // An array literal has the type of an array. It may be a
            // tuple if every element could be one of the tuple's.
            (Ty::Array(a), Ty::Tuple(b)) => self.assignable_at(a, &Ty::tuple_element(b), next),
            (Ty::Function(a), Ty::Function(b)) => self.fn_assignable(a, b, next),
            (Ty::ClassValue(_), Ty::Function(_)) => true,
            // Class types are nominal: an instance of the class or of a
            // subclass.
            (Ty::Named(a), Ty::Named(b)) if self.class(*b).is_some() => {
                self.class(*a).is_some() && self.is_subclass(*a, *b)
            }
            (_, Ty::Named(b)) if self.class(*b).is_some() => false,
            _ => self.structurally_assignable(&src, &dst, next),
        }
    }

    /// `src` to an object type or interface `dst`: every property `dst`
    /// requires is in `src` with an assignable type.
    fn structurally_assignable(&self, src: &Ty, dst: &Ty, depth: usize) -> bool {
        let Some(want) = self.members(dst) else {
            return false;
        };
        if !matches!(dst, Ty::Object(_) | Ty::Named(_)) {
            return false;
        }
        let Some(have) = self.members(src) else {
            // `{}` and `object` take any value that is not null or
            // undefined; nothing else takes a primitive.
            return want.obj.props.is_empty() && want.obj.index.is_none() && !src.is_nullish();
        };
        for prop in &want.obj.props {
            match have.obj.get(&prop.name) {
                Some(found) => {
                    if found.optional && !prop.optional {
                        return false;
                    }
                    if !self.assignable_at(&found.ty, &prop.ty, depth) {
                        return false;
                    }
                }
                None if prop.optional || !have.closed => {}
                None => match &have.obj.index {
                    Some(index) if self.assignable_at(index, &prop.ty, depth) => {}
                    _ => return false,
                },
            }
        }
        if let Some(index) = &want.obj.index {
            for prop in &have.obj.props {
                if !self.assignable_at(&prop.ty, index, depth) {
                    return false;
                }
            }
        }
        true
    }

    /// A function `a` where a function `b` is expected: `a` must not
    /// require more arguments than `b` is called with, their parameters
    /// must be compatible one way or the other, and `a`'s result must do
    /// for `b`'s.
    fn fn_assignable(&self, a: &FnTy, b: &FnTy, depth: usize) -> bool {
        let b_takes_any_number = b.params.last().is_some_and(|p| p.rest);
        if a.required() > b.params.len() && !b_takes_any_number {
            return false;
        }
        for (i, pa) in a.params.iter().enumerate() {
            let Some(pb) = b.param(i) else { break };
            if !self.assignable_at(pb, &pa.ty, depth) && !self.assignable_at(&pa.ty, pb, depth) {
                return false;
            }
        }
        matches!(self.expand(&b.ret), Ty::Void) || self.assignable_at(&a.ret, &b.ret, depth)
    }
}
