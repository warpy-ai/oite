//! Narrowing: what a condition says about the type of a variable or a
//! property when it is true, and when it is false.

use super::*;

/// What a condition establishes: (path, type) pairs for when it holds and
/// for when it does not.
#[derive(Default)]
pub(super) struct Facts {
    pub then: Vec<(String, Ty)>,
    pub otherwise: Vec<(String, Ty)>,
}

impl Facts {
    fn swapped(self) -> Facts {
        Facts {
            then: self.otherwise,
            otherwise: self.then,
        }
    }
}

/// `x`, `x.y.z` or `this.x` as a path: what narrowing is recorded under.
pub(super) fn path_of(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(id) => Some(id.sym.to_string()),
        Expr::This(_) => Some("this".to_string()),
        Expr::Paren(p) => path_of(&p.expr),
        Expr::TsNonNull(e) => path_of(&e.expr),
        Expr::Member(m) => {
            let MemberProp::Ident(name) = &m.prop else {
                return None;
            };
            Some(format!("{}.{}", path_of(&m.obj)?, name.sym))
        }
        _ => None,
    }
}

fn is_null(expr: &Expr) -> bool {
    matches!(expr, Expr::Lit(Lit::Null(_)))
}

fn is_undefined(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(id) if id.sym.as_ref() == "undefined")
}

fn str_lit(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Lit(Lit::Str(s)) => Some(s.value.to_string_lossy().into_owned()),
        _ => None,
    }
}

/// `typeof <operand>`
fn typeof_operand(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::Unary(u) if u.op == UnaryOp::TypeOf => Some(&u.arg),
        _ => None,
    }
}

impl Checker {
    /// Apply what was established: each path has its type from here.
    pub(super) fn apply(&mut self, facts: &[(String, Ty)]) {
        for (path, ty) in facts {
            self.narrow(path, ty.clone());
        }
    }

    /// The members of `ty` that pass `keep`, as a type; `None` if that
    /// changes nothing.
    fn filtered(&self, ty: &Ty, keep: impl Fn(&Ty) -> bool) -> Option<Ty> {
        let expanded = self.expand(ty);
        let members: Vec<Ty> = expanded.members().iter().map(|m| self.expand(m)).collect();
        let kept: Vec<Ty> = members.iter().filter(|m| keep(m)).cloned().collect();
        (kept.len() != members.len()).then(|| Ty::union(kept))
    }

    /// One fact about `expr`, if it is a path and `narrowed` is a type.
    fn fact(expr: &Expr, narrowed: Option<Ty>) -> Vec<(String, Ty)> {
        match (path_of(expr), narrowed) {
            (Some(path), Some(ty)) => vec![(path, ty)],
            _ => Vec::new(),
        }
    }

    /// What `cond` establishes. It only looks at variables and property
    /// paths, so it reports nothing itself.
    pub(super) fn facts(&mut self, cond: &Expr) -> Facts {
        match cond {
            Expr::Paren(p) => self.facts(&p.expr),
            Expr::Unary(u) if u.op == UnaryOp::Bang => self.facts(&u.arg).swapped(),
            Expr::Bin(bin) => match bin.op {
                BinaryOp::LogicalAnd => {
                    let left = self.facts(&bin.left);
                    self.push_scope();
                    self.apply(&left.then);
                    let right = self.facts(&bin.right);
                    self.scopes.pop();
                    let mut then = left.then;
                    then.extend(right.then);
                    Facts {
                        then,
                        otherwise: Vec::new(),
                    }
                }
                BinaryOp::LogicalOr => {
                    let left = self.facts(&bin.left);
                    self.push_scope();
                    self.apply(&left.otherwise);
                    let right = self.facts(&bin.right);
                    self.scopes.pop();
                    let mut otherwise = left.otherwise;
                    otherwise.extend(right.otherwise);
                    Facts {
                        then: Vec::new(),
                        otherwise,
                    }
                }
                BinaryOp::EqEqEq | BinaryOp::EqEq => {
                    self.equality(&bin.left, &bin.right, bin.op == BinaryOp::EqEq)
                }
                BinaryOp::NotEqEq | BinaryOp::NotEq => self
                    .equality(&bin.left, &bin.right, bin.op == BinaryOp::NotEq)
                    .swapped(),
                BinaryOp::InstanceOf => self.instance_of(&bin.left, &bin.right),
                _ => Facts::default(),
            },
            Expr::Call(call) => self.is_array_call(call),
            _ => self.truthiness(cond),
        }
    }

    /// `a === b` (or `a == b` when `loose`).
    fn equality(&mut self, a: &Expr, b: &Expr, loose: bool) -> Facts {
        // `typeof x === "kind"`, either way round.
        for (operand, kind) in [(a, b), (b, a)] {
            if let (Some(subject), Some(kind)) = (typeof_operand(operand), str_lit(kind)) {
                return self.typeof_facts(subject, &kind);
            }
        }
        // `x === null`, `x === undefined`, either way round.
        for (subject, other) in [(a, b), (b, a)] {
            let nullish: &[Ty] = if is_null(other) && loose || is_undefined(other) && loose {
                &[Ty::Null, Ty::Undefined, Ty::Void]
            } else if is_null(other) {
                &[Ty::Null]
            } else if is_undefined(other) {
                &[Ty::Undefined, Ty::Void]
            } else {
                continue;
            };
            if path_of(subject).is_none() {
                continue;
            }
            let current = self.expr(subject);
            if current.is_dynamic() {
                return Facts::default();
            }
            return Facts {
                then: Self::fact(subject, self.filtered(&current, |m| nullish.contains(m))),
                otherwise: Self::fact(subject, self.filtered(&current, |m| !nullish.contains(m))),
            };
        }
        Facts::default()
    }

    /// `typeof subject === kind`
    fn typeof_facts(&mut self, subject: &Expr, kind: &str) -> Facts {
        if path_of(subject).is_none() {
            return Facts::default();
        }
        let current = self.expr(subject);
        if current.is_dynamic() {
            // Nothing is known: the test says what it is.
            let primitive = match kind {
                "string" => Ty::String,
                "number" => Ty::Number,
                "boolean" => Ty::Boolean,
                "undefined" => Ty::Undefined,
                _ => return Facts::default(),
            };
            return Facts {
                then: Self::fact(subject, Some(primitive)),
                otherwise: Vec::new(),
            };
        }
        let is_kind = |m: &Ty| match kind {
            "string" => matches!(m, Ty::String | Ty::StrLit(_)),
            "number" => matches!(m, Ty::Number),
            "boolean" => matches!(m, Ty::Boolean),
            "undefined" => matches!(m, Ty::Undefined | Ty::Void),
            "function" => matches!(m, Ty::Function(_) | Ty::ClassValue(_)),
            "object" => matches!(m, Ty::Null | Ty::Array(_) | Ty::Object(_) | Ty::Named(_)),
            _ => false,
        };
        // A member whose kind is not known stays on both sides.
        let unknown = |m: &Ty| m.is_dynamic();
        Facts {
            then: Self::fact(
                subject,
                self.filtered(&current, |m| is_kind(m) || unknown(m)),
            ),
            otherwise: Self::fact(
                subject,
                self.filtered(&current, |m| !is_kind(m) || unknown(m)),
            ),
        }
    }

    /// `subject instanceof Class`
    fn instance_of(&mut self, subject: &Expr, class: &Expr) -> Facts {
        if path_of(subject).is_none() {
            return Facts::default();
        }
        let Ty::ClassValue(id) = self.expr(class) else {
            return Facts::default();
        };
        let current = self.expr(subject);
        if matches!(current, Ty::Unknown) {
            return Facts {
                then: Self::fact(subject, Some(Ty::Named(id))),
                otherwise: Vec::new(),
            };
        }
        if current.is_dynamic() {
            return Facts::default();
        }
        let instance = |m: &Ty| match m {
            Ty::Named(other) => self.class(*other).is_some() && self.is_subclass(*other, id),
            _ => false,
        };
        // A class that `id` extends may be an instance of `id` or not.
        let maybe = |m: &Ty| match m {
            Ty::Named(other) => {
                self.class(*other).is_none() || (*other != id && self.is_subclass(id, *other))
            }
            Ty::Any | Ty::Unknown | Ty::Object(_) => true,
            _ => false,
        };
        Facts {
            then: Self::fact(
                subject,
                self.filtered(&current, |m| instance(m) || maybe(m)),
            ),
            otherwise: Self::fact(
                subject,
                self.filtered(&current, |m| !instance(m) || maybe(m)),
            ),
        }
    }

    /// `Array.isArray(subject)`
    fn is_array_call(&mut self, call: &CallExpr) -> Facts {
        let Callee::Expr(callee) = &call.callee else {
            return Facts::default();
        };
        let is_array = matches!(callee.as_ref(), Expr::Member(m)
            if matches!(m.obj.as_ref(), Expr::Ident(o) if o.sym.as_ref() == "Array")
                && matches!(&m.prop, MemberProp::Ident(p) if p.sym.as_ref() == "isArray"));
        let [arg] = call.args.as_slice() else {
            return Facts::default();
        };
        if !is_array || arg.spread.is_some() || path_of(&arg.expr).is_none() {
            return Facts::default();
        }
        let subject = arg.expr.as_ref();
        let current = self.expr(subject);
        if current.is_dynamic() {
            return Facts {
                then: Self::fact(subject, Some(Ty::array(Ty::Any))),
                otherwise: Vec::new(),
            };
        }
        Facts {
            then: Self::fact(
                subject,
                self.filtered(&current, |m| matches!(m, Ty::Array(_)) || m.is_dynamic()),
            ),
            otherwise: Self::fact(
                subject,
                self.filtered(&current, |m| !matches!(m, Ty::Array(_))),
            ),
        }
    }

    /// `subject` used as a condition: when it holds the value is not
    /// `null` or `undefined`; when it does not, the value is not an
    /// object (objects are always truthy).
    fn truthiness(&mut self, subject: &Expr) -> Facts {
        if path_of(subject).is_none() {
            return Facts::default();
        }
        let current = self.expr(subject);
        if current.is_dynamic() {
            return Facts::default();
        }
        let always_truthy = |m: &Ty| {
            matches!(
                m,
                Ty::Array(_) | Ty::Object(_) | Ty::Named(_) | Ty::Function(_) | Ty::ClassValue(_)
            )
        };
        Facts {
            then: Self::fact(subject, self.filtered(&current, |m| !m.is_nullish())),
            otherwise: Self::fact(subject, self.filtered(&current, |m| !always_truthy(m))),
        }
    }
}
