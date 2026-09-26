//! Script modules, linked into the files that import them: step 7 of
//! `docs/11-next.md`, "Modules".
//!
//! A module's names reach an importing file under their **linked** name,
//! [`qualified`]: `items` in `stores/cart.rux` is `stores/cart::items` in
//! every file that imports it. [`resolve`] rewrites a file before it is
//! checked and lowered, so `cart.items`, `cart.add(p)` and a picked `add(p)`
//! all become plain names, and the checker, the IR and the interpreter need
//! nothing new to read them. The runtime tracks a read or a write by name, so
//! a binding that reads `cart.items` follows the module's signal whichever
//! name each importer gave the module.
//!
//! What can be told from the text alone is refused here: a module used as a
//! value, a name it does not export, a write from outside it, and an imported
//! name declared again in the file.

use std::collections::{HashMap, HashSet};

use rux_syntax::ast::{Expr, ExprKind, Ident, Script, Stmt, StmtKind};
use rux_syntax::visit::{walk_stmts, Node};
use rux_syntax::Span;

use crate::types::Type;

/// What an imported name stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A whole module: `use stores::cart;` makes `cart` this.
    Module(String),
    /// One thing a module exports: `use stores::cart::{add};` makes `add`
    /// this, by module and name.
    Member(String, String),
}

/// A name a file imports, and where the import is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alias {
    pub local: String,
    pub target: Target,
    /// The line of the file the import is on, for a message about it.
    pub line: usize,
}

/// The name `member` of `module` has once linked: `stores/cart::items`.
pub fn qualified(module: &str, member: &str) -> String {
    format!("{module}::{member}")
}

/// `stores/cart::items` as `(stores/cart, items)`; `None` for a name that
/// is not linked.
pub fn split(name: &str) -> Option<(&str, &str)> {
    name.rsplit_once("::").filter(|(m, _)| !m.is_empty())
}

/// How a linked name reads in a message: `` `items` of stores/cart ``.
pub fn shown(name: &str) -> String {
    match split(name) {
        Some((module, member)) => format!("`{member}` of {module}"),
        None => format!("`{name}`"),
    }
}

/// What a module exports, as an importing file's checker sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Export {
    pub name: String,
    pub kind: ExportKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExportKind {
    /// `export fn`: its parameters and result, and whether it is `async`.
    Fn { params: Vec<Type>, result: Type, is_async: bool },
    /// `export let x = signal(…)`: state that importers read and follow.
    Signal(Type),
    /// `export let x = …`: a value that is never written.
    Value(Type),
    /// `export type`.
    Type,
}

/// What a document's text is linked against: the names it imports and what
/// each module exports. The interpreter keeps one, since a handler or a
/// binding reaches it as text and is linked when it is first compiled.
#[derive(Clone, Debug, Default)]
pub struct Linking {
    pub aliases: Vec<Alias>,
    pub exports: HashMap<String, Vec<Export>>,
    /// Each component's own names, by the component's key: what its code is
    /// linked against instead of the document's.
    pub scopes: HashMap<String, Scope>,
}

/// A component's own names: step 7.4 of `docs/11-next.md`. A component's
/// functions are linked under `name` (`components/badge::inc`), and its code
/// sees them, what it imports, and nothing of the document's.
#[derive(Clone, Debug, Default)]
pub struct Scope {
    /// `components/badge`.
    pub name: String,
    pub aliases: Vec<Alias>,
}

impl Linking {
    /// What code in `scope` (a component's key; `None` for the document)
    /// imports.
    pub fn aliases_in(&self, scope: Option<&str>) -> &[Alias] {
        match scope.and_then(|k| self.scopes.get(k)) {
            Some(s) => &s.aliases,
            None if scope.is_some() => &[],
            None => &self.aliases,
        }
    }

    /// The linked-name prefix of a component's own functions.
    pub fn own_prefix(&self, scope: Option<&str>) -> Option<&str> {
        scope.and_then(|k| self.scopes.get(k)).map(|s| s.name.as_str())
    }

    /// Every export, under its linked name, for [`crate::check::Context::linked`].
    pub fn linked(&self) -> Vec<Export> {
        let mut out: Vec<Export> = self
            .exports
            .iter()
            .flat_map(|(m, xs)| xs.iter().map(move |x| Export { name: qualified(m, &x.name), kind: x.kind.clone() }))
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

/// What `script`, a module checked into `record`, exports.
pub fn exports_of(script: &Script, record: &crate::check::Record) -> Vec<Export> {
    script
        .exports
        .iter()
        .filter_map(|x| {
            let stmt = script.stmts.get(x.stmt)?;
            let kind = match &stmt.kind {
                StmtKind::Fn(def) => {
                    let (params, result) = record
                        .fns
                        .get(&(def.name.name.clone(), def.params.len()))
                        .cloned()
                        .unwrap_or_else(|| (vec![Type::Any; def.params.len()], Type::Any));
                    ExportKind::Fn { params, result, is_async: def.is_async }
                }
                StmtKind::Let { name, value, .. } => {
                    let ty = record.script.decl.get(&name.span.start).cloned().unwrap_or(Type::Any);
                    let signal = matches!(value, Some(Expr { kind: ExprKind::Call { callee, .. }, .. })
                        if callee.len() == 1 && callee[0].name == "signal");
                    if signal {
                        ExportKind::Signal(ty)
                    } else {
                        ExportKind::Value(ty)
                    }
                }
                StmtKind::Type { .. } => ExportKind::Type,
                _ => return None,
            };
            Some(Export { name: x.name.name.clone(), kind })
        })
        .collect()
}

/// A problem [`resolve`] found, at `span` of the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub span: Span,
    pub message: String,
}

/// `problems` in `src`, as the checker's errors: what the load reports.
pub fn findings(problems: &[Problem], src: &str) -> Vec<crate::check::Finding> {
    let lines = rux_syntax::LineIndex::new(src);
    problems
        .iter()
        .map(|p| crate::check::Finding {
            message: p.message.clone(),
            line: Some(lines.line_col(src, p.span.start as usize).0),
            is_error: true,
            template: false,
        })
        .collect()
}

/// Rewrite `script` so every use of an imported name is its linked name, and
/// say what cannot be. `exports` is what each module exports, by module name.
pub fn resolve(script: &mut Script, aliases: &[Alias], exports: &HashMap<String, Vec<Export>>) -> Vec<Problem> {
    let mut problems = Vec::new();
    if aliases.is_empty() {
        return problems;
    }
    let by_local: HashMap<&str, &Alias> = aliases.iter().map(|a| (a.local.as_str(), a)).collect();

    // An import's name means the import everywhere in the file, so nothing in
    // the file may declare it again: that is what lets the rewrite below be
    // read off the text.
    walk_stmts(&script.stmts, &mut |node| {
        for ident in declared(node) {
            if let Some(alias) = by_local.get(ident.name.as_str()) {
                problems.push(Problem {
                    span: ident.span,
                    message: format!(
                        "`{0}` is what line {1} imports, so it cannot be declared again here; \
                         rename one of them, as `use … as other` does for the import",
                        ident.name, alias.line
                    ),
                });
            }
        }
        true
    });

    let exported = |module: &str, member: &str| -> Option<&Export> {
        exports.get(module)?.iter().find(|e| e.name == member)
    };
    let values_of = |module: &str| -> String {
        let names: Vec<String> = exports
            .get(module)
            .into_iter()
            .flatten()
            .filter(|e| e.kind != ExportKind::Type)
            .map(|e| format!("`{}`", e.name))
            .collect();
        if names.is_empty() {
            "it exports nothing that is not a type".to_string()
        } else {
            format!("it exports {}", names.join(", "))
        }
    };
    let module_of = |local: &str| match by_local.get(local).map(|a| &a.target) {
        Some(Target::Module(m)) => Some(m.clone()),
        _ => None,
    };
    let member_of = |local: &str| match by_local.get(local).map(|a| &a.target) {
        Some(Target::Member(m, n)) => Some((m.clone(), n.clone())),
        _ => None,
    };

    rux_syntax::visit_mut::exprs_mut(&mut script.stmts, &mut |e| {
        let span = e.span;
        match &mut e.kind {
            // `cart.items`
            ExprKind::Field { base, name, optional: false } => {
                let ExprKind::Var(local) = &base.kind else { return true };
                let Some(module) = module_of(local) else { return true };
                match exported(&module, &name.name) {
                    Some(x) if x.kind != ExportKind::Type => {
                        e.kind = ExprKind::Var(qualified(&module, &name.name));
                    }
                    _ => {
                        problems.push(Problem {
                            span: name.span,
                            message: format!("{module} does not export `{}`: {}", name.name, values_of(&module)),
                        });
                        // Its `cart` is not a second problem.
                        return false;
                    }
                }
                true
            }
            // `cart.add(p)`
            ExprKind::Method { recv, name, args, optional: false } => {
                let ExprKind::Var(local) = &recv.kind else { return true };
                let Some(module) = module_of(local) else { return true };
                match exported(&module, &name.name) {
                    Some(Export { kind: ExportKind::Fn { .. }, .. }) => {
                        let callee = vec![Ident { name: qualified(&module, &name.name), span: name.span }];
                        let args = std::mem::take(args);
                        e.kind = ExprKind::Call { callee, args, bang: false };
                    }
                    Some(Export { kind: ExportKind::Type, .. }) | None => problems.push(Problem {
                        span: name.span,
                        message: format!("{module} does not export `{}`: {}", name.name, values_of(&module)),
                    }),
                    // `cart.items.push(p)` is a method on a field, not this;
                    // `cart.items(p)` calls something that is not a function.
                    Some(_) => problems.push(Problem {
                        span: name.span,
                        message: format!("`{local}.{}` is not a function of {module}", name.name),
                    }),
                }
                // Linked, or refused: either way its `cart` is done with.
                matches!(e.kind, ExprKind::Call { .. })
            }
            ExprKind::Field { base, name, optional: true } | ExprKind::Method { recv: base, name, optional: true, .. } => {
                if let ExprKind::Var(local) = &base.kind {
                    if module_of(local).is_some() {
                        problems.push(Problem {
                            span,
                            message: format!(
                                "`{local}` is a module, always there, so `?.` has nothing to skip: write `{local}.{}`",
                                name.name
                            ),
                        });
                    }
                }
                // Skipped, so its `cart` is not reported a second time.
                false
            }
            ExprKind::Var(local) => {
                if let Some((module, member)) = member_of(local) {
                    *local = qualified(&module, &member);
                } else if let Some(module) = module_of(local) {
                    problems.push(Problem {
                        span,
                        message: format!(
                            "`{local}` is the module {module}, not a value: name what you want from it, as `{local}.name`"
                        ),
                    });
                }
                true
            }
            ExprKind::Call { callee, .. } if callee.len() == 1 => {
                let local = callee[0].name.clone();
                if let Some((module, member)) = member_of(&local) {
                    callee[0].name = qualified(&module, &member);
                } else if let Some(module) = module_of(&local) {
                    problems.push(Problem {
                        span: callee[0].span,
                        message: format!(
                            "`{local}` is the module {module}, not a function: call one of its functions, as `{local}.name()`"
                        ),
                    });
                }
                true
            }
            _ => true,
        }
    });

    // What is linked in is changed only inside the module it comes from.
    let foreign: HashSet<String> = exports
        .iter()
        .flat_map(|(m, xs)| xs.iter().map(move |x| qualified(m, &x.name)))
        .collect();
    let mut writes = Vec::new();
    walk_stmts(&script.stmts, &mut |node| {
        match node {
            Node::Stmt(Stmt { kind: StmtKind::Assign { target, .. } | StmtKind::Step { target, .. }, .. }) => {
                if let Some(root) = root_name(target).filter(|r| foreign.contains(*r)) {
                    writes.push((target.span, root.to_string()));
                }
            }
            Node::Expr(Expr { kind: ExprKind::Method { recv, name, .. }, span, .. })
                if crate::interp::stdlib::mutates(&name.name) =>
            {
                if let Some(root) = root_name(recv).filter(|r| foreign.contains(*r)) {
                    writes.push((*span, root.to_string()));
                }
            }
            _ => {}
        }
        true
    });
    for (span, root) in writes {
        let (module, _) = split(&root).expect("a linked name");
        problems.push(Problem {
            span,
            message: format!(
                "{} belongs to {module}, and only {module} changes it: export a function from it that does",
                shown(&root)
            ),
        });
    }
    problems
}

/// The name a place is rooted at: `a` for `a`, `a.b[0]` and `a.b.c`.
fn root_name(e: &Expr) -> Option<&str> {
    match &e.kind {
        ExprKind::Var(v) => Some(v),
        ExprKind::Field { base, .. } | ExprKind::Index { base, .. } => root_name(base),
        _ => None,
    }
}

/// The names `node` declares: a `let`, a function and its parameters, a
/// closure's parameters, a loop's variables, what `catch` binds, a
/// `computed` or a `prop`.
fn declared<'a>(node: Node<'a>) -> Vec<&'a Ident> {
    match node {
        Node::Stmt(s) => match &s.kind {
            StmtKind::Let { name, .. } | StmtKind::Computed { name, .. } => vec![name],
            StmtKind::Fn(def) => std::iter::once(&def.name).chain(def.params.iter().map(|p| &p.name)).collect(),
            StmtKind::For { var, counter, .. } => std::iter::once(var).chain(counter.iter()).collect(),
            StmtKind::Try { var, .. } => var.iter().collect(),
            StmtKind::Prop(decls) => decls.iter().map(|d| &d.name).collect(),
            _ => Vec::new(),
        },
        Node::Expr(e) => match &e.kind {
            ExprKind::Closure { params, .. } => params.iter().map(|p| &p.name).collect(),
            _ => Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exports() -> HashMap<String, Vec<Export>> {
        let f = ExportKind::Fn { params: vec![Type::Any], result: Type::Null, is_async: false };
        HashMap::from([(
            "stores/cart".to_string(),
            vec![
                Export { name: "items".into(), kind: ExportKind::Signal(Type::Any) },
                Export { name: "add".into(), kind: f },
                Export { name: "Item".into(), kind: ExportKind::Type },
            ],
        )])
    }

    fn aliases() -> Vec<Alias> {
        vec![
            Alias { local: "cart".into(), target: Target::Module("stores/cart".into()), line: 2 },
            Alias { local: "put".into(), target: Target::Member("stores/cart".into(), "add".into()), line: 3 },
        ]
    }

    /// The script after [`resolve`], printed, and what it said.
    fn run(src: &str) -> (String, Vec<String>) {
        let opts = rux_syntax::Options { declarations: true };
        let mut script = rux_syntax::parse(src, opts).unwrap_or_else(|e| panic!("{src}: {e}"));
        let problems = resolve(&mut script, &aliases(), &exports());
        let mut names = Vec::new();
        rux_syntax::visit::exprs(&script, &mut |e| {
            match &e.kind {
                ExprKind::Var(v) => names.push(v.clone()),
                ExprKind::Call { callee, .. } => names.push(format!("{}()", callee[0].name)),
                _ => {}
            }
            true
        });
        (names.join(" "), problems.into_iter().map(|p| p.message).collect())
    }

    #[test]
    fn a_modules_names_become_their_linked_names() {
        let (names, problems) = run("fn f(p: int) { cart.add(p); put(p); let n = cart.items.length; }");
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(names, "stores/cart::add() p stores/cart::add() p stores/cart::items");
    }

    #[test]
    fn what_cannot_be_linked_is_said() {
        let said = |src: &str| run(src).1.join("\n");
        assert!(said("let x = cart;").contains("is the module stores/cart, not a value"));
        assert!(said("cart();").contains("not a function"));
        assert!(said("let x = cart.secret;").contains("does not export `secret`: it exports `items`, `add`"));
        assert!(said("cart.Item();").contains("does not export `Item`"));
        assert!(said("cart.items(1);").contains("not a function of stores/cart"));
        assert!(said("let x = cart?.items;").contains("`?.` has nothing to skip"));
        assert!(said("fn f(cart: int) { }").contains("`cart` is what line 2 imports"));
        assert!(said("let put = 1;").contains("`put` is what line 3 imports"));
    }

    #[test]
    fn only_the_module_changes_what_it_exports() {
        let said = |src: &str| run(src).1.join("\n");
        for write in ["cart.items = [];", "cart.items[0] = 1;", "cart.items.push(1);", "cart.items.length++;"] {
            assert!(said(write).contains("`items` of stores/cart belongs to stores/cart"), "{write}: {}", said(write));
        }
        // Reading it, and calling a method that reads, are fine.
        assert_eq!(run("let n = cart.items.filter(x => x > 1);").1, Vec::<String>::new());
    }
}
