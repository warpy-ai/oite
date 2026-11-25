//! Which top-level declarations of the rolls a program uses.
//!
//! The prelude links `@rolls/std` and the rolls it imports into every
//! program: some 700 functions, of which a program calls a few. A roll's
//! top-level declaration that nothing the program runs names is left out of
//! the compiled program, on every backend.
//!
//! What runs: every item of the program's own modules (the entry and what it
//! imports by relative path), and every item of a roll that does something
//! when it runs (`Array.prototype.map = ...`). A declaration that only
//! creates its binding (a function, a class with no static code,
//! `const x = <literal or function>`) runs for that binding alone, so it is
//! kept only if a kept item names it.
//!
//! Names are matched as identifiers, whatever scope an identifier is really
//! in: any identifier in a kept item that could name a binding (as the
//! module's own top-level name, an import, or a name of the prelude) keeps
//! it. `ns.x`, `ns` a namespace import, keeps the export `x`; any other use
//! of `ns` keeps every export. Nothing reaches a binding of a linked module
//! another way: its name is the linker's (`@rolls/array::map`), the runtime
//! looks up only its own globals by name, and there is no `eval` or
//! `globalThis`.

use std::collections::{HashMap, HashSet};

use swc_common::Spanned;
use swc_ecma_ast::*;
use swc_ecma_visit::{Visit, VisitWith};

use super::modules::ModuleLink;
use super::scope::{SpanKey, key};

/// A module of a linked program, as the reachability sees it.
pub(super) struct Linked<'a> {
    pub program: &'a Program,
    pub link: &'a ModuleLink,
    /// A roll the program imports: its declarations are left out when
    /// unused.
    pub roll: bool,
}

/// A top-level item that makes code.
struct Item {
    module: usize,
    span: SpanKey,
    /// The global names a declaration binds; `None` for an item that runs
    /// whether or not anything names it.
    binds: Option<Vec<String>>,
    /// The global names it may refer to.
    refs: Vec<String>,
}

/// The items (by span) of each module that the program does not use.
pub(super) fn unused_declarations(modules: &[Linked]) -> Vec<HashSet<SpanKey>> {
    let mut items = Vec::new();
    for (m, linked) in modules.iter().enumerate() {
        let module = match linked.program {
            Program::Module(module) => module,
            // A script (no `import` or `export`) runs as a whole.
            Program::Script(script) => {
                let decls = script.body.iter().filter_map(|stmt| stmt.as_decl());
                let names = Names::new(linked.link, decls, []);
                let mut refs = Refs::default();
                script.visit_with(&mut refs);
                items.push(Item {
                    module: m,
                    span: key(script.span),
                    binds: None,
                    refs: names.resolve(&refs),
                });
                continue;
            }
        };
        let names = Names::of(module, linked.link);
        for item in &module.body {
            if makes_no_code(item) {
                continue;
            }
            let binds = linked
                .roll
                .then(|| pure_declaration(item))
                .flatten()
                .map(|locals| {
                    locals
                        .into_iter()
                        .map(|l| format!("{}{}", linked.link.prefix, l))
                        .collect()
                });
            let mut refs = Refs::default();
            item.visit_with(&mut refs);
            items.push(Item {
                module: m,
                span: key(item.span()),
                binds,
                refs: names.resolve(&refs),
            });
        }
    }

    let mut declared: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, item) in items.iter().enumerate() {
        for global in item.binds.iter().flatten() {
            declared.entry(global).or_default().push(i);
        }
    }
    let mut kept: Vec<bool> = items.iter().map(|item| item.binds.is_none()).collect();
    let mut work: Vec<usize> = (0..items.len()).filter(|&i| kept[i]).collect();
    let mut named: HashSet<&str> = HashSet::new();
    while let Some(i) = work.pop() {
        for global in &items[i].refs {
            if !named.insert(global) {
                continue;
            }
            for &j in declared.get(global.as_str()).into_iter().flatten() {
                if !kept[j] {
                    kept[j] = true;
                    work.push(j);
                }
            }
        }
    }

    let mut unused = vec![HashSet::new(); modules.len()];
    for (item, kept) in items.iter().zip(kept) {
        if !kept {
            unused[item.module].insert(item.span);
        }
    }
    unused
}

/// Imports, re-exports, types and ambient declarations: no code.
fn makes_no_code(item: &ModuleItem) -> bool {
    match item {
        ModuleItem::ModuleDecl(
            ModuleDecl::Import(_)
            | ModuleDecl::ExportNamed(_)
            | ModuleDecl::ExportAll(_)
            | ModuleDecl::ExportDefaultDecl(ExportDefaultDecl {
                decl: DefaultDecl::TsInterfaceDecl(_),
                ..
            }),
        ) => true,
        ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl { decl, .. }))
        | ModuleItem::Stmt(Stmt::Decl(decl)) => {
            matches!(decl, Decl::TsInterface(_) | Decl::TsTypeAlias(_))
                || super::scope::is_ambient(decl)
        }
        _ => false,
    }
}

/// The names a top-level item binds, if running it does nothing else.
fn pure_declaration(item: &ModuleItem) -> Option<Vec<String>> {
    let decl = match item {
        ModuleItem::Stmt(Stmt::Decl(decl))
        | ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl { decl, .. })) => decl,
        ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(e)) => {
            let own = match &e.decl {
                DefaultDecl::Fn(f) => f.ident.as_ref(),
                DefaultDecl::Class(c) if pure_class(&c.class) => c.ident.as_ref(),
                _ => return None,
            };
            return Some(
                std::iter::once("default".to_string())
                    .chain(own.map(|id| id.sym.to_string()))
                    .collect(),
            );
        }
        ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(e)) => {
            return pure_expr(&e.expr).then(|| vec!["default".to_string()]);
        }
        _ => return None,
    };
    match decl {
        Decl::Fn(f) => Some(vec![f.ident.sym.to_string()]),
        Decl::Class(c) if pure_class(&c.class) => Some(vec![c.ident.sym.to_string()]),
        Decl::Var(v) => v
            .decls
            .iter()
            .map(|d| match (&d.name, &d.init) {
                (Pat::Ident(id), init) if init.as_deref().is_none_or(pure_expr) => {
                    Some(id.id.sym.to_string())
                }
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// Whether evaluating `e` only creates a value.
fn pure_expr(e: &Expr) -> bool {
    match e {
        Expr::Lit(_) | Expr::Fn(_) | Expr::Arrow(_) => true,
        Expr::Class(c) => pure_class(&c.class),
        Expr::Tpl(t) => t.exprs.is_empty(),
        Expr::Paren(p) => pure_expr(&p.expr),
        Expr::Unary(u) => {
            matches!(
                u.op,
                UnaryOp::Minus | UnaryOp::Plus | UnaryOp::Bang | UnaryOp::Tilde
            ) && pure_expr(&u.arg)
        }
        Expr::Array(a) => a
            .elems
            .iter()
            .flatten()
            .all(|el| el.spread.is_none() && pure_expr(&el.expr)),
        Expr::Object(o) => o.props.iter().all(|p| match p {
            PropOrSpread::Prop(p) => match p.as_ref() {
                Prop::KeyValue(kv) => pure_key(&kv.key) && pure_expr(&kv.value),
                Prop::Method(m) => pure_key(&m.key),
                Prop::Getter(g) => pure_key(&g.key),
                Prop::Setter(s) => pure_key(&s.key),
                Prop::Shorthand(_) | Prop::Assign(_) => false,
            },
            PropOrSpread::Spread(_) => false,
        }),
        Expr::TsAs(e) => pure_expr(&e.expr),
        Expr::TsSatisfies(e) => pure_expr(&e.expr),
        Expr::TsNonNull(e) => pure_expr(&e.expr),
        Expr::TsTypeAssertion(e) => pure_expr(&e.expr),
        Expr::TsConstAssertion(e) => pure_expr(&e.expr),
        _ => false,
    }
}

fn pure_key(key: &PropName) -> bool {
    !matches!(key, PropName::Computed(_))
}

/// Whether defining the class runs none of its code: no decorators, no
/// computed keys, no static initializers or blocks. Its parent is read,
/// and kept through its name.
fn pure_class(class: &Class) -> bool {
    #[derive(Default)]
    struct Decorated(bool);
    impl Visit for Decorated {
        fn visit_decorator(&mut self, _: &Decorator) {
            self.0 = true;
        }
    }
    let mut decorated = Decorated::default();
    class.visit_with(&mut decorated);
    if decorated.0 {
        return false;
    }
    if class
        .super_class
        .as_deref()
        .is_some_and(|s| !matches!(s, Expr::Ident(_)))
    {
        return false;
    }
    class.body.iter().all(|member| match member {
        ClassMember::Constructor(c) => pure_key(&c.key),
        ClassMember::Method(m) => pure_key(&m.key),
        ClassMember::PrivateMethod(_)
        | ClassMember::TsIndexSignature(_)
        | ClassMember::Empty(_) => true,
        ClassMember::ClassProp(p) => {
            pure_key(&p.key) && (!p.is_static || p.value.as_deref().is_none_or(pure_expr))
        }
        ClassMember::PrivateProp(p) => !p.is_static || p.value.as_deref().is_none_or(pure_expr),
        ClassMember::StaticBlock(_) | ClassMember::AutoAccessor(_) => false,
    })
}

/// The identifiers an item uses: `ns.x` as a member, every other one as a
/// name. Types make no code, so their names do not count.
#[derive(Default)]
struct Refs {
    names: HashSet<String>,
    members: HashSet<(String, String)>,
}

impl Visit for Refs {
    fn visit_ident(&mut self, ident: &Ident) {
        self.names.insert(ident.sym.to_string());
    }

    fn visit_expr(&mut self, expr: &Expr) {
        if let Expr::Member(m) = expr
            && let (Expr::Ident(obj), MemberProp::Ident(prop)) = (m.obj.as_ref(), &m.prop)
        {
            self.members
                .insert((obj.sym.to_string(), prop.sym.to_string()));
            return;
        }
        expr.visit_children_with(self);
    }

    fn visit_ts_type(&mut self, _: &TsType) {}
}

/// How a module's identifiers name globals.
struct Names<'a> {
    link: &'a ModuleLink,
    /// The module's own top-level bindings.
    top: HashSet<String>,
}

impl<'a> Names<'a> {
    fn of(module: &Module, link: &'a ModuleLink) -> Self {
        let decls = module.body.iter().filter_map(|item| match item {
            ModuleItem::Stmt(Stmt::Decl(decl))
            | ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl { decl, .. })) => Some(decl),
            _ => None,
        });
        // `export default function f` binds `f` too.
        let defaults = module.body.iter().filter_map(|item| match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(e)) => match &e.decl {
                DefaultDecl::Fn(f) => f.ident.as_ref(),
                DefaultDecl::Class(c) => c.ident.as_ref(),
                DefaultDecl::TsInterfaceDecl(_) => None,
            },
            _ => None,
        });
        Self::new(link, decls, defaults)
    }

    fn new<'d>(
        link: &'a ModuleLink,
        decls: impl Iterator<Item = &'d Decl>,
        defaults: impl IntoIterator<Item = &'d Ident>,
    ) -> Self {
        let mut top: HashSet<String> = decls.flat_map(super::modules::declared_names).collect();
        top.extend(defaults.into_iter().map(|id| id.sym.to_string()));
        Self { link, top }
    }

    /// Every global the identifiers in `refs` may name.
    fn resolve(&self, refs: &Refs) -> Vec<String> {
        let mut out = Vec::new();
        for name in &refs.names {
            self.name(name, &mut out);
        }
        for (obj, prop) in &refs.members {
            match self.link.namespaces.get(obj) {
                Some(members) => match members.binary_search_by(|(n, _)| n.cmp(prop)) {
                    Ok(i) => out.push(members[i].1.clone()),
                    Err(_) => self.name(obj, &mut out),
                },
                None => self.name(obj, &mut out),
            }
        }
        out
    }

    fn name(&self, name: &str, out: &mut Vec<String>) {
        if let Some(global) = self.link.imports.get(name) {
            out.push(global.clone());
        } else if let Some(members) = self.link.namespaces.get(name) {
            out.extend(members.iter().map(|(_, global)| global.clone()));
        } else if self.top.contains(name) {
            out.push(format!("{}{}", self.link.prefix, name));
        } else if let Some(global) = self.link.prelude.get(name) {
            out.push(global.clone());
        }
    }
}
