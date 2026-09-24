//! The type checker: reads a compiled script and the annotations the fork kept
//! beside it, and reports what does not fit. `docs/10-types.md` is the design.
//!
//! Nothing here runs the program. Types are erased, so the checker's only
//! output is findings: an error where a program contradicts its own
//! annotations, a warning where a name could not be given a type at all and
//! checking stops.
//!
//! The approach is the usual bidirectional one. [`Checker::infer`] works out
//! what type an expression has; [`Checker::check_expr`] is handed the type an
//! expression is expected to have, and pushes it inward where that decides
//! something: a whole-number literal where an `int` is wanted, a string literal
//! where a literal union is, the fields of a map literal, the parameters of a
//! closure handed to `filter`.

use std::collections::HashMap;

use rhai::{
    ASTFlags, Annotation, AnnotationKind, Dynamic, Expr, FnPtr, Position, ScriptFuncDef, Shared,
    Stmt, StmtBlock, AST,
};

use crate::types::{parse_type, Field, Type};

/// One thing the checker has to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// The sentence an author reads.
    pub message: String,
    /// 1-based line within the script it was checked as, when known.
    pub line: Option<usize>,
    /// An error when the program contradicts itself, a warning when checking
    /// had to stop for want of a type.
    pub is_error: bool,
    /// Found in the template, so [`Finding::line`] is a line of the file, not
    /// of the script.
    pub template: bool,
}

/// A template, as the checker sees it: what each expression in it must be.
/// The runtime builds this from the markup, since it is the one that knows
/// what an attribute of an element takes and what an event hands over.
#[derive(Clone, Debug)]
pub enum Tpl {
    /// A `{{ }}` or a bound attribute. `want` is what it must be, `None` for
    /// anything at all.
    Expr { src: String, want: Option<Type>, line: usize, what: String },
    /// A handler's statements, run with `event` bound to `event`.
    Handler { src: String, event: Type, line: usize, what: String },
    /// `r-model`: `src` names what the input writes `writes` into, and shows
    /// as `shows`.
    Model { src: String, writes: Type, shows: Type, line: usize, what: String },
    /// A value that is not an expression, as `label="Save"` passes a string,
    /// handed to something that takes `want`.
    Given { given: Type, want: Type, line: usize, what: String },
    /// `r-for="var in src"`: the body sees `var` as one element of `src`.
    For { var: String, src: String, line: usize, body: Vec<Tpl> },
    /// An `r-if` chain. Each branch is its condition, `None` for `r-else`, its
    /// line, and its subtree, which sees its condition true and every earlier
    /// one false.
    If { branches: Vec<(Option<String>, usize, Vec<Tpl>)> },
}

/// What the checker is told about the world outside the script.
#[derive(Clone, Debug, Default)]
pub struct Context {
    /// Types declared in other files and brought in with `use`, as their name
    /// and their text.
    pub imported_types: Vec<(String, String)>,
    /// The other types declared beside an imported one, as name, text and the
    /// `use` path that would bring each in. An imported type may be built from
    /// them, so they resolve, but this file may not name one it did not import.
    pub support_types: Vec<(String, String, String)>,
    /// Names in scope that the script does not declare, with their types: a
    /// component's props when it is checked on its own, and the like.
    pub provided: Vec<(String, Type)>,
    /// `let`s that are placeholders standing in for a value set elsewhere (a
    /// component's `computed`, run later). Bound to the type given, or `any`,
    /// and not checked.
    pub placeholders: Vec<(String, Option<Type>)>,
    /// `host::` functions with a signature, by name.
    pub host: Vec<(String, Type)>,
    /// Names some caller has in scope when it calls a function: an `r-for`'s
    /// variable, a handler's own `let`s. Under the fork a plain call runs in
    /// its caller's scope, so a function can read these, and a typed one may
    /// not: see `docs/10-types.md`, "A typed function cannot read its caller's
    /// locals".
    pub caller_names: std::collections::HashSet<String>,
    /// The template of the file being checked. See [`Tpl`].
    pub template: Vec<Tpl>,
    /// Report nothing past this script line. The document's script has the
    /// components' functions appended to it, and a finding in one of those is
    /// the component's to report when it is checked.
    pub own_lines: Option<usize>,
}

/// Check `ast` against its own annotations and the [`Context`]. `compile`
/// turns a template's expression or handler into an AST, the way the runtime
/// will when it runs it.
pub fn check(ast: &AST, cx: &Context, compile: &dyn Fn(&str) -> Option<AST>) -> Vec<Finding> {
    let mut checker = Checker::new(ast, cx, compile);
    checker.run();
    let mut findings = checker.findings;
    findings.sort_by_key(|f| f.line);
    findings.dedup();
    findings
}

/// The state of a function's checking.
#[derive(Clone, Debug)]
enum FnState {
    Unchecked,
    /// Being checked right now, so a call reached from inside its own body is
    /// recursion.
    Checking,
    /// Checked, and this is what it returns.
    Done(Type),
}

#[derive(Clone, Debug)]
struct FnInfo {
    def: Shared<ScriptFuncDef>,
    /// Each parameter's declared type, `None` where it has none.
    params: Vec<(String, Option<Type>)>,
    /// `): T`, when written.
    result: Option<Type>,
    state: FnState,
}

impl FnInfo {
    fn is_anonymous(&self) -> bool {
        self.def.name.starts_with("anon$")
    }
}

struct Checker<'a> {
    ast: &'a AST,
    cx: &'a Context,
    compile: &'a dyn Fn(&str) -> Option<AST>,
    /// While a template piece is checked: the file line it starts on, and what
    /// it is (`:disabled` on <button>), which prefixes what is said about it.
    in_template: Option<(usize, String)>,
    /// Declared and imported types, by name.
    types: HashMap<String, Type>,
    /// Types that resolve here without being nameable here, with the `use`
    /// path that would make them so.
    hidden: HashMap<String, String>,
    fns: HashMap<(String, usize), FnInfo>,
    /// The top-level `let`s, which are the document's signals.
    globals: HashMap<String, Type>,
    /// Local scopes, innermost last.
    scopes: Vec<HashMap<String, Type>>,
    /// What conditions have established, frame by frame. See `fact`.
    facts: Vec<HashMap<String, Type>>,
    /// The names each function writes, itself or through what it calls.
    writes: HashMap<(String, usize), std::collections::HashSet<String>>,
    /// A `let`'s declared type, by the position of its name.
    declared: HashMap<Position, Type>,
    /// What each `return` in the function being checked hands back.
    returns: Vec<Vec<Type>>,
    /// The named function whose body is being checked, whether it is typed,
    /// and where it is: what a read of a caller's local is reported against.
    in_fn: Vec<(String, bool)>,
    /// Every name declared anywhere in the script: a `let`, a parameter or a
    /// loop variable. One a function cannot see itself is some caller's.
    script_names: std::collections::HashSet<String>,
    /// The declared result of each function being checked, innermost last,
    /// with the function's name; `None` for one whose result is inferred.
    results: Vec<Option<(String, Type)>>,
    findings: Vec<Finding>,
    /// While positive, findings are counted rather than kept: used to try a
    /// value against each member of a union.
    quiet: usize,
    quiet_errors: usize,
}

/// How deep a type is unfolded before giving up, so a recursive type such as
/// `type Tree = { kids: Tree[] }` cannot send assignability round forever.
const MAX_DEPTH: usize = 24;

impl<'a> Checker<'a> {
    fn new(ast: &'a AST, cx: &'a Context, compile: &'a dyn Fn(&str) -> Option<AST>) -> Self {
        Checker {
            ast,
            cx,
            compile,
            in_template: None,
            types: HashMap::new(),
            hidden: HashMap::new(),
            fns: HashMap::new(),
            globals: HashMap::new(),
            scopes: Vec::new(),
            facts: vec![HashMap::new()],
            writes: HashMap::new(),
            declared: HashMap::new(),
            returns: Vec::new(),
            results: Vec::new(),
            in_fn: Vec::new(),
            script_names: crate::declared_in(ast),
            findings: Vec::new(),
            quiet: 0,
            quiet_errors: 0,
        }
    }

    // ----- Reporting -------------------------------------------------------

    fn report(&mut self, pos: Position, message: String, is_error: bool) {
        if self.quiet > 0 {
            if is_error {
                self.quiet_errors += 1;
            }
            return;
        }
        if let Some((start, what)) = &self.in_template {
            let line = Some(start + pos.line().unwrap_or(1) - 1);
            let message = format!("{what}: {message}");
            self.findings.push(Finding { message, line, is_error, template: true });
            return;
        }
        let line = pos.line();
        if let (Some(line), Some(own)) = (line, self.cx.own_lines) {
            if line > own {
                return;
            }
        }
        self.findings.push(Finding { message, line, is_error, template: false });
    }

    fn error(&mut self, pos: Position, message: String) {
        self.report(pos, message, true);
    }

    fn warn(&mut self, pos: Position, message: String) {
        self.report(pos, message, false);
    }

    // ----- Setup -----------------------------------------------------------

    fn run(&mut self) {
        let annotations: Vec<Annotation> = self.ast.annotations().to_vec();

        // Types first, since everything else names them.
        for (name, text, path) in &self.cx.support_types {
            if let Ok(ty) = parse_type(text) {
                self.types.insert(name.clone(), ty);
                self.hidden.insert(name.clone(), path.clone());
            }
        }
        for (name, text) in &self.cx.imported_types {
            self.hidden.remove(name);
            if let Ok(ty) = parse_type(text) {
                self.types.insert(name.clone(), ty);
            }
        }
        for a in annotations.iter().filter(|a| a.kind == AnnotationKind::Type) {
            match parse_type(&a.ty) {
                Ok(ty) => {
                    if self.types.contains_key(a.name.as_str())
                        && !self.hidden.contains_key(a.name.as_str())
                        && !self.cx.imported_types.iter().any(|(n, _)| n == a.name.as_str())
                    {
                        self.error(a.pos, format!("the type `{}` is declared twice", a.name));
                    }
                    if !a.name.starts_with(|c: char| c.is_uppercase()) {
                        self.error(
                            a.pos,
                            format!(
                                "a declared type's name starts with a capital letter: `type {}{}`",
                                a.name[..1].to_uppercase(),
                                &a.name[1..]
                            ),
                        );
                    }
                    self.hidden.remove(a.name.as_str());
                    self.types.insert(a.name.to_string(), ty);
                }
                Err(e) => self.error(a.ty_pos, format!("`{}` is not a type: {}", a.ty, e.message)),
            }
        }

        // Every annotation, parsed, with its names checked against the types
        // that exist.
        let mut parsed: Vec<(Annotation, Type)> = Vec::new();
        for a in annotations {
            match parse_type(&a.ty) {
                Ok(ty) => {
                    self.check_names_exist(&ty, a.ty_pos);
                    parsed.push((a, ty));
                }
                Err(e) if a.kind != AnnotationKind::Type => {
                    self.error(a.ty_pos, format!("`{}` is not a type: {}", a.ty, e.message))
                }
                Err(_) => {}
            }
        }
        for (name, ty) in &self.cx.provided {
            self.globals.insert(name.clone(), ty.clone());
        }

        for (a, ty) in &parsed {
            if a.kind == AnnotationKind::Var {
                self.declared.insert(a.pos, ty.clone());
            }
        }

        let defs: Vec<Shared<ScriptFuncDef>> = self.ast.iter_fn_def().cloned().collect();
        for def in defs {
            let arity = def.params.len();
            let key = (def.name.to_string(), arity);
            let params = def
                .params
                .iter()
                .map(|p| {
                    let ty = parsed.iter().find_map(|(a, ty)| match &a.kind {
                        AnnotationKind::Param { function, arity: n }
                            if function == &def.name && *n == arity && a.name == *p =>
                        {
                            Some(ty.clone())
                        }
                        _ => None,
                    });
                    (p.to_string(), ty)
                })
                .collect();
            let result = parsed.iter().find_map(|(a, ty)| match &a.kind {
                AnnotationKind::Result { arity: n } if a.name == def.name && *n == arity => {
                    Some(ty.clone())
                }
                _ => None,
            });
            self.fns.insert(key, FnInfo { def, params, result, state: FnState::Unchecked });
        }

        // The top level, in order: its `let`s are the signals every function
        // reads.
        let statements: Vec<Stmt> = self.ast.statements().to_vec();
        for stmt in &statements {
            self.check_stmt(stmt);
        }

        // Then every named function nothing has reached yet.
        let mut named: Vec<(String, usize)> = self
            .fns
            .iter()
            .filter(|(_, f)| !f.is_anonymous())
            .map(|(k, _)| k.clone())
            .collect();
        named.sort();
        for key in named {
            self.function_result(&key.0, key.1);
        }

        // Last, the template: every function it can call is checked by now,
        // so nothing found in one is reported against a template line.
        let cx = self.cx;
        for item in &cx.template {
            self.template_item(item);
        }
    }

    // ----- Templates -------------------------------------------------------

    /// Run `f` as the template piece `what`, written on file line `line`.
    fn in_piece<T>(&mut self, line: usize, what: &str, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = self.in_template.replace((line, what.to_string()));
        let out = f(self);
        self.in_template = saved;
        out
    }

    /// Compile a piece of the template, making its arrows known. `None` when
    /// it does not compile, which the runtime reports in its own words.
    fn compile_piece(&mut self, src: &str) -> Option<AST> {
        let ast = (self.compile)(src)?;
        for def in ast.iter_fn_def() {
            let arity = def.params.len();
            let params = def
                .params
                .iter()
                .map(|p| {
                    let ty = ast.annotations().iter().find_map(|a| match &a.kind {
                        AnnotationKind::Param { function, arity: n }
                            if function == &def.name && *n == arity && a.name == *p =>
                        {
                            parse_type(&a.ty).ok()
                        }
                        _ => None,
                    });
                    (p.to_string(), ty)
                })
                .collect();
            let info = FnInfo { def: def.clone(), params, result: None, state: FnState::Unchecked };
            self.fns.entry((def.name.to_string(), arity)).or_insert(info);
        }
        Some(ast)
    }

    /// The one expression a piece is, when it is one.
    fn piece_expr(ast: &AST) -> Option<Expr> {
        match ast.statements() {
            [Stmt::Expr(e)] => Some((**e).clone()),
            _ => None,
        }
    }

    fn template_item(&mut self, item: &Tpl) {
        match item {
            Tpl::Expr { src, want, line, what } => {
                let Some(ast) = self.compile_piece(src) else { return };
                self.in_piece(*line, what, |c| match (Self::piece_expr(&ast), want) {
                    (Some(e), Some(want)) => c.check_expr(&e, want),
                    (Some(e), None) => {
                        c.infer(&e);
                    }
                    (None, _) => {
                        c.with_scope(|c| c.check_statements(ast.statements()));
                    }
                });
            }
            Tpl::Handler { src, event, line, what } => {
                let Some(ast) = self.compile_piece(src) else { return };
                self.in_piece(*line, what, |c| {
                    c.with_scope(|c| {
                        c.bind("event", event.clone());
                        c.check_statements(ast.statements());
                    })
                });
            }
            Tpl::Model { src, writes, shows, line, what } => {
                let Some(ast) = self.compile_piece(src) else { return };
                let Some(e) = Self::piece_expr(&ast) else { return };
                self.in_piece(*line, what, |c| {
                    let held = c.infer(&e);
                    if c.resolve(&held) == Type::Any {
                        return;
                    }
                    let name = src.trim();
                    if !c.assignable(writes, &held) {
                        let message = format!(
                            "the field writes {} into `{name}`, which holds {}",
                            c.show(writes),
                            c.show(&held)
                        );
                        c.error(e.position(), message);
                    } else if !c.assignable(&held, shows) {
                        let message = format!(
                            "the field shows {}, and `{name}` holds {}",
                            c.show(shows),
                            c.show(&held)
                        );
                        c.error(e.position(), message);
                    }
                });
            }
            Tpl::Given { given, want, line, what } => {
                self.in_piece(*line, what, |c| {
                    if !c.assignable(given, want) {
                        c.mismatch_ty(Position::NONE, given, want);
                    }
                });
            }
            Tpl::For { var, src, line, body } => {
                let Some(ast) = self.compile_piece(src) else { return };
                let Some(e) = Self::piece_expr(&ast) else { return };
                let element = self.in_piece(*line, "`r-for`", |c| {
                    let ty = c.infer(&e);
                    c.element_of(&e, &ty)
                });
                self.with_scope(|c| {
                    c.bind(var, element);
                    for item in body {
                        c.template_item(item);
                    }
                });
            }
            Tpl::If { branches } => {
                // What every earlier branch not being taken established.
                let mut earlier: Vec<(String, Type)> = Vec::new();
                for (cond, line, body) in branches {
                    let cond = cond.as_deref().and_then(|src| {
                        let ast = self.compile_piece(src)?;
                        Self::piece_expr(&ast)
                    });
                    let facts = match &cond {
                        Some(e) => self.with_facts(earlier.clone(), |c| {
                            c.in_piece(*line, "the condition", |c| {
                                c.infer(e);
                            });
                            c.facts_of(e, true)
                        }),
                        None => Vec::new(),
                    };
                    let mut here = earlier.clone();
                    here.extend(facts);
                    self.with_facts(here, |c| {
                        for item in body {
                            c.template_item(item);
                        }
                    });
                    match &cond {
                        Some(e) => {
                            let not = self.with_facts(earlier.clone(), |c| c.facts_of(e, false));
                            earlier.extend(not);
                        }
                        None => break,
                    }
                }
            }
        }
    }

    /// Report a name in `ty` that is no declared type.
    fn check_names_exist(&mut self, ty: &Type, pos: Position) {
        let mut names = Vec::new();
        named_in(ty, &mut names);
        for name in names {
            if let Some(path) = self.hidden.get(&name) {
                let message = format!("`{name}` is not imported here; bring it in with `use {path};`");
                self.error(pos, message);
            } else if !self.types.contains_key(&name) {
                let known: Vec<&str> = self.types.keys().map(String::as_str).collect();
                let hint = near_miss(&name, &known)
                    .map(|n| format!("; did you mean `{n}`?"))
                    .unwrap_or_else(|| {
                        format!(
                            ". Declare it with `type {name} = …;`, or bring it in with \
                             `use types::{name};`"
                        )
                    });
                self.error(pos, format!("there is no type `{name}`{hint}"));
            }
        }
    }

    // ----- Types -----------------------------------------------------------

    /// `ty` with a declared name replaced by what it stands for, at the top
    /// only. A name that stands for nothing is `any`: it was reported where it
    /// was written.
    fn resolve(&self, ty: &Type) -> Type {
        let mut ty = ty.clone();
        for _ in 0..MAX_DEPTH {
            match ty {
                Type::Named(ref name) => match self.types.get(name) {
                    Some(t) => ty = t.clone(),
                    None => return Type::Any,
                },
                _ => return ty,
            }
        }
        Type::Any
    }

    /// Whether a value of type `from` may go where a `to` is wanted.
    fn assignable(&self, from: &Type, to: &Type) -> bool {
        self.assignable_at(from, to, 0)
    }

    fn assignable_at(&self, from: &Type, to: &Type, depth: usize) -> bool {
        if depth > MAX_DEPTH || from == to {
            return true;
        }
        let from = self.resolve(from);
        let to = self.resolve(to);
        let d = depth + 1;
        match (&from, &to) {
            (Type::Any, _) | (_, Type::Any) => true,
            // Every member of a union has to fit.
            (Type::Union(members), _) => members.iter().all(|m| self.assignable_at(m, &to, d)),
            (_, Type::Union(members)) => members.iter().any(|m| self.assignable_at(&from, m, d)),
            (Type::Int, Type::Number) => true,
            (Type::Literal(_), Type::String) => true,
            (Type::Literal(a), Type::Literal(b)) => a == b,
            (Type::Array(a), Type::Array(b)) => self.assignable_at(a, b, d),
            (Type::Record(have), Type::Record(want)) => want.iter().all(|w| {
                match have.iter().find(|h| h.name == w.name) {
                    // An optional field may be missing, and a field that may be
                    // missing only fits one that may be.
                    Some(h) => (w.optional || !h.optional) && self.assignable_at(&h.ty, &w.ty, d),
                    None => w.optional,
                }
            }),
            (Type::Record(have), Type::Dict(value)) => {
                have.iter().all(|h| self.assignable_at(&h.ty, value, d))
            }
            (Type::Dict(a), Type::Dict(b)) => self.assignable_at(a, b, d),
            (Type::Function(fp, fr), Type::Function(tp, tr)) => {
                fp.len() <= tp.len()
                    && fp.iter().zip(tp).all(|(f, t)| self.assignable_at(t, f, d))
                    && (matches!(**tr, Type::Null) || self.assignable_at(fr, tr, d))
            }
            _ => false,
        }
    }

    /// A type as a message shows it: by its declared name where it has one.
    fn show(&self, ty: &Type) -> String {
        format!("`{ty}`")
    }

    // ----- Scope -----------------------------------------------------------

    fn bind(&mut self, name: &str, ty: Type) {
        match self.scopes.last_mut() {
            Some(scope) => {
                scope.insert(name.to_string(), ty);
            }
            None => {
                self.globals.insert(name.to_string(), ty);
            }
        }
    }

    fn lookup(&self, name: &str) -> Option<Type> {
        if let Some(t) = self.fact(name) {
            return Some(t);
        }
        for scope in self.scopes.iter().rev() {
            if let Some(t) = scope.get(name) {
                return Some(t.clone());
            }
        }
        self.globals.get(name).cloned()
    }

    fn with_scope<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.push(HashMap::new());
        self.facts.push(HashMap::new());
        let out = f(self);
        self.facts.pop();
        self.scopes.pop();
        out
    }

    // ----- Narrowing -------------------------------------------------------
    //
    // A fact is what a condition established about a path (`t`, `t.note`,
    // `m[k]`) for the region it guards: its type there. Facts live in frames
    // that open and close with blocks, so a fact never outlives its region.

    /// What is known about `path` here, if anything.
    fn fact(&self, path: &str) -> Option<Type> {
        self.facts.iter().rev().find_map(|frame| frame.get(path).cloned())
    }

    /// Run `f` with `facts` known.
    fn with_facts<T>(&mut self, facts: Vec<(String, Type)>, f: impl FnOnce(&mut Self) -> T) -> T {
        self.facts.push(facts.into_iter().collect());
        let out = f(self);
        self.facts.pop();
        out
    }

    /// Know `facts` for the rest of the current block: an early return has
    /// ruled the other case out.
    fn add_facts(&mut self, facts: Vec<(String, Type)>) {
        if self.facts.is_empty() {
            self.facts.push(HashMap::new());
        }
        let frame = self.facts.last_mut().unwrap();
        frame.extend(facts);
    }

    /// Forget everything known about `root` and what hangs off it, because
    /// something has written it.
    fn forget(&mut self, root: &str) {
        let dot = format!("{root}.");
        let bracket = format!("{root}[");
        for frame in &mut self.facts {
            frame.retain(|path, _| path != root && !path.starts_with(&dot) && !path.starts_with(&bracket));
        }
    }

    /// `t` without `null`, keeping a declared name where there was nothing to
    /// take out.
    fn non_null(&self, t: &Type) -> Type {
        match self.resolve(t) {
            Type::Union(_) | Type::Null => without_null(&self.resolve(t)),
            _ => t.clone(),
        }
    }

    /// What `cond` being `truth` establishes, as facts.
    fn facts_of(&mut self, cond: &Expr, truth: bool) -> Vec<(String, Type)> {
        match cond {
            Expr::FnCall(call, _) if call.name == "!" && call.args.len() == 1 => {
                self.facts_of(&call.args[0], !truth)
            }
            // All of an `&&` held, or none of an `||` did.
            Expr::And(items, _) if truth => self.facts_of_all(items, true),
            Expr::Or(items, _) if !truth => self.facts_of_all(items, false),
            Expr::FnCall(call, _)
                if matches!(call.name.as_str(), "==" | "!=" | "===" | "!==") && call.args.len() == 2 =>
            {
                let equal = matches!(call.name.as_str(), "==" | "===") == truth;
                let (a, b) = (&call.args[0], &call.args[1]);
                match self.facts_of_equality(a, b, equal) {
                    Some(facts) => facts,
                    None => self.facts_of_equality(b, a, equal).unwrap_or_default(),
                }
            }
            // `"note" in t` and `k in m`, which rhai writes `t.contains("note")`.
            Expr::FnCall(call, _) if call.name == "contains" && call.args.len() == 2 && truth => {
                self.facts_of_in(&call.args[0], &call.args[1])
            }
            // Truthiness: a path that is truthy is not `null`.
            _ => {
                let Some(path) = path_of(cond) else { return Vec::new() };
                let t = self.infer_quietly(cond);
                if truth {
                    vec![(path, self.non_null(&t))]
                } else if falsy_only_when_null(&without_null(&self.resolve(&t))) && t != Type::Any {
                    vec![(path, Type::Null)]
                } else {
                    Vec::new()
                }
            }
        }
    }

    fn facts_of_all(&mut self, items: &[Expr], truth: bool) -> Vec<(String, Type)> {
        let mut out: Vec<(String, Type)> = Vec::new();
        for item in items {
            let facts = self.with_facts(out.clone(), |c| c.facts_of(item, truth));
            out.extend(facts);
        }
        out
    }

    /// `side == other` (or `!=`, as `equal` says) where `side` is a path and
    /// `other` a literal or `null`. `None` when `side` is not something a fact
    /// can be kept about.
    fn facts_of_equality(&mut self, side: &Expr, other: &Expr, equal: bool) -> Option<Vec<(String, Type)>> {
        // `type_of(x) == "string"`.
        if let (Expr::FnCall(call, _), Expr::StringConstant(tag, _)) = (side, other) {
            if call.name == "type_of" && call.args.len() == 1 {
                let path = path_of(&call.args[0])?;
                let t = self.infer_quietly(&call.args[0]);
                let t = self.resolve(&t);
                let members = match t {
                    Type::Union(m) => m,
                    other => vec![other],
                };
                let kept: Vec<Type> =
                    members.into_iter().filter(|m| is_type_of(m, tag, &self.types) == equal).collect();
                return Some(if kept.is_empty() { Vec::new() } else { vec![(path, Type::union(kept))] });
            }
        }
        let path = path_of(side)?;
        let t = self.infer_quietly(side);
        let is_null = matches!(other, Expr::Unit(_))
            || matches!(other, Expr::Custom(c, _) if c.tokens.first().is_some_and(|t| t.as_str() == "null"));
        if is_null {
            return Some(vec![(path, if equal { Type::Null } else { self.non_null(&t) })]);
        }
        let Expr::StringConstant(s, _) = other else { return Some(Vec::new()) };
        let s = s.to_string();
        let mut out = Vec::new();
        let resolved = self.resolve(&t);
        let members = match &resolved {
            Type::Union(m) => m.clone(),
            other => vec![other.clone()],
        };
        let narrowed: Vec<Type> = if equal {
            members
                .iter()
                .filter_map(|m| match m {
                    Type::Literal(x) if *x == s => Some(m.clone()),
                    Type::String | Type::Any => Some(Type::Literal(s.clone())),
                    _ => None,
                })
                .collect()
        } else {
            members.iter().filter(|m| **m != Type::Literal(s.clone())).cloned().collect()
        };
        if !narrowed.is_empty() && (equal || narrowed.len() < members.len()) {
            out.push((path.clone(), Type::union(narrowed)));
        }
        // A discriminant: `load.state == "done"` narrows `load` too.
        if let Some(cut) = path.rfind('.') {
            let (parent, field) = (&path[..cut], &path[cut + 1..]);
            if let Some(Type::Union(members)) = self.type_at(parent).map(|t| self.resolve(&t)) {
                let lit = Type::Literal(s.clone());
                let kept: Vec<Type> = members
                    .iter()
                    .filter(|m| match self.resolve(m) {
                        Type::Record(fields) => match fields.iter().find(|f| f.name == field) {
                            Some(f) if equal => self.assignable(&lit, &f.ty),
                            Some(f) => self.resolve(&f.ty) != lit,
                            None => !equal,
                        },
                        Type::Null => !equal,
                        _ => true,
                    })
                    .cloned()
                    .collect();
                if !kept.is_empty() && kept.len() < members.len() {
                    out.push((parent.to_string(), Type::union(kept)));
                }
            }
        }
        Some(out)
    }

    /// `key in map` held: the key is there.
    fn facts_of_in(&mut self, map: &Expr, key: &Expr) -> Vec<(String, Type)> {
        let Some(base) = path_of(map) else { return Vec::new() };
        let t = self.infer_quietly(map);
        let t = self.resolve(&t);
        match key {
            Expr::StringConstant(k, _) => {
                let path = format!("{base}.{k}");
                let mut out = Vec::new();
                if let Some(field) = self.type_at(&path) {
                    out.push((path, self.non_null(&field)));
                }
                // Only the members of a union that have the field.
                if let Type::Union(members) = &t {
                    let kept: Vec<Type> = members
                        .iter()
                        .filter(|m| match self.resolve(m) {
                            Type::Record(fields) => fields.iter().any(|f| f.name == k.as_str()),
                            Type::Dict(_) | Type::Any => true,
                            _ => false,
                        })
                        .cloned()
                        .collect();
                    if !kept.is_empty() && kept.len() < members.len() {
                        out.push((base, Type::union(kept)));
                    }
                }
                out
            }
            Expr::Variable(v, ..) if v.2.is_empty() => match without_null(&t) {
                Type::Dict(value) => vec![(format!("{base}[{}]", v.1), *value)],
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// The names a function writes that are not its own: assignments to
    /// anything it did not declare, and whatever the functions it calls write.
    fn writes_of(&mut self, name: &str, arity: usize) -> std::collections::HashSet<String> {
        use std::collections::HashSet;
        let key = (name.to_string(), arity);
        if let Some(w) = self.writes.get(&key) {
            return w.clone();
        }
        // Recursion ends here, with what is known so far.
        self.writes.insert(key.clone(), HashSet::new());
        let Some(info) = self.fns.get(&key).cloned() else { return HashSet::new() };
        let mut assigned: HashSet<String> = HashSet::new();
        let mut locals: HashSet<String> = info.params.iter().map(|(p, _)| p.clone()).collect();
        let mut calls: Vec<(String, usize)> = Vec::new();
        for stmt in info.def.body.statements() {
            stmt.walk(&mut Vec::new(), &mut |path| {
                match path.last() {
                    Some(rhai::ASTNode::Stmt(Stmt::Assignment(x))) => {
                        if let Some(root) = path_of(&x.1.lhs).map(|p| root_of(&p).to_string()) {
                            assigned.insert(root);
                        }
                    }
                    Some(rhai::ASTNode::Stmt(Stmt::Var(x, ..))) => {
                        locals.insert(x.0.name.to_string());
                    }
                    Some(rhai::ASTNode::Stmt(Stmt::FnCall(call, _)))
                    | Some(rhai::ASTNode::Expr(Expr::FnCall(call, _)))
                        if call.namespace.is_empty() =>
                    {
                        calls.push((call.name.to_string(), call.args.len()));
                    }
                    _ => {}
                }
                true
            });
        }
        let mut out: HashSet<String> = assigned.difference(&locals).cloned().collect();
        for (callee, n) in calls {
            if self.fns.contains_key(&(callee.clone(), n)) {
                out.extend(self.writes_of(&callee, n));
            }
        }
        self.writes.insert(key, out.clone());
        out
    }

    /// The type at a path, from what is known and the records along it, with
    /// nothing reported. `None` when some step of it cannot be followed.
    fn type_at(&self, path: &str) -> Option<Type> {
        if let Some(t) = self.fact(path) {
            return Some(t);
        }
        let Some(cut) = path.rfind(['.', '[']) else { return self.lookup(path) };
        let (parent, rest) = path.split_at(cut);
        let parent = self.type_at(parent)?;
        let parent = without_null(&self.resolve(&parent));
        let field = rest.trim_start_matches(['.', '[']).trim_end_matches(']');
        let field_type = |t: &Type| match self.resolve(t) {
            Type::Record(fields) => fields.iter().find(|f| f.name == field).map(|f| {
                if f.optional {
                    f.ty.clone().optional()
                } else {
                    f.ty.clone()
                }
            }),
            Type::Dict(v) => Some((*v).clone().optional()),
            _ => None,
        };
        match parent {
            Type::Union(members) => {
                let found: Vec<Type> = members.iter().filter_map(field_type).collect();
                (!found.is_empty()).then(|| Type::union(found))
            }
            other => field_type(&other),
        }
    }

    // ----- Statements ------------------------------------------------------

    /// Check a block, returning the type of the value it ends with.
    fn check_block(&mut self, block: &StmtBlock) -> Type {
        self.with_scope(|c| c.check_statements(block.statements()))
    }

    fn check_statements(&mut self, statements: &[Stmt]) -> Type {
        let mut last = Type::Null;
        for stmt in statements {
            last = self.check_stmt(stmt);
            self.after(stmt);
        }
        last
    }

    /// What a statement settles for the ones after it. An `if` that leaves
    /// settles its condition: after `if t == null { return; }`, `t` is not
    /// `null`.
    fn after(&mut self, stmt: &Stmt) {
        if let Stmt::If(x, _) = stmt {
            let (body, branch) = (exits(x.body.statements()), exits(x.branch.statements()));
            if body && !branch {
                let facts = self.facts_of(&x.expr, false);
                self.add_facts(facts);
            } else if branch && !body {
                let facts = self.facts_of(&x.expr, true);
                self.add_facts(facts);
            }
        }
    }

    /// Statements whose last one is the value of a function declared to
    /// return `want`: that value is checked against it, reaching into each
    /// branch of an `if` and each arm of a `switch`.
    fn check_statements_for(&mut self, statements: &[Stmt], name: &str, want: &Type) {
        let Some((last, before)) = statements.split_last() else {
            if !self.assignable(&Type::Null, want) {
                let message =
                    format!("`{name}` is declared to return {}, and it returns nothing", self.show(want));
                self.error(Position::NONE, message);
            }
            return;
        };
        for stmt in before {
            self.check_stmt(stmt);
            self.after(stmt);
        }
        match last {
            Stmt::Expr(e) => self.check_result(e, name, want),
            Stmt::If(x, _) if !x.branch.statements().is_empty() => {
                self.infer(&x.expr);
                let yes = self.facts_of(&x.expr, true);
                let no = self.facts_of(&x.expr, false);
                self.with_facts(yes, |c| {
                    c.with_scope(|c| c.check_statements_for(x.body.statements(), name, want))
                });
                self.with_facts(no, |c| {
                    c.with_scope(|c| c.check_statements_for(x.branch.statements(), name, want))
                });
            }
            Stmt::Block(b) => self.with_scope(|c| c.check_statements_for(b.statements(), name, want)),
            Stmt::Switch(x, pos) => {
                self.check_switch(&x.0, &x.1, *pos, Some((name, want)));
            }
            // A `return` checks itself.
            Stmt::Return(..) | Stmt::BreakLoop(..) => {
                self.check_stmt(last);
            }
            other => {
                let got = self.check_stmt(other);
                if !exits(std::slice::from_ref(other)) && !self.assignable(&got, want) {
                    self.result_mismatch(other.position(), name, want, &got);
                }
            }
        }
    }

    /// One value a function hands back, checked against its declared result.
    fn check_result(&mut self, e: &Expr, name: &str, want: &Type) {
        // A block used as the value, which is how a `switch` or an `if` in
        // expression position arrives: its own last statement is the value.
        if let Expr::Stmt(block) = e {
            if self.closure_of(e).is_none() {
                self.with_scope(|c| c.check_statements_for(block.statements(), name, want));
                return;
            }
        }
        let before = self.findings.len();
        self.check_expr(e, want);
        if self.findings.len() == before + 1 {
            let got = self.infer_quietly(e);
            let last = self.findings.last_mut().unwrap();
            if last.is_error && last.message.starts_with("this is ") {
                last.message = format!("`{name}` is declared to return `{want}`, and this is `{got}`");
            }
        }
    }

    fn result_mismatch(&mut self, pos: Position, name: &str, want: &Type, got: &Type) {
        let message =
            format!("`{name}` is declared to return {}, and this is {}", self.show(want), self.show(got));
        self.error(pos, message);
    }

    /// Check a statement, returning the type of its value (`null` for one that
    /// has none).
    fn check_stmt(&mut self, stmt: &Stmt) -> Type {
        match stmt {
            Stmt::Var(x, ..) => {
                let (ident, value, _) = &**x;
                self.check_let(ident.name.as_str(), ident.pos, value);
                Type::Null
            }
            Stmt::Assignment(x) => {
                let (op, bin) = &**x;
                // What was known about the target no longer holds once it is
                // written; what it holds now is its declared type.
                if let Some(path) = path_of(&bin.lhs) {
                    self.forget(&path);
                }
                let target = self.infer(&bin.lhs);
                match op.get_op_assignment_info() {
                    None => {
                        let before = self.findings.len();
                        self.check_expr(&bin.rhs, &target);
                        // Name the variable, and say how to let it hold both.
                        if let Expr::Variable(v, ..) = &bin.lhs {
                            if self.findings.len() == before + 1 {
                                let got = self.infer_quietly(&bin.rhs);
                                let last = self.findings.last_mut().unwrap();
                                if last.is_error && last.message.starts_with("this is ") {
                                    let both = Type::union([target.clone(), widen(&got)]);
                                    last.message = format!(
                                        "`{0}` holds `{1}`, so it cannot be given `{2}` here. If it \
                                         may hold either, say so: `let {0}: {3} = …`",
                                        v.1,
                                        target,
                                        widen(&got),
                                        both
                                    );
                                }
                            }
                        }
                    }
                    Some((.., op_syntax)) => {
                        let rhs = self.infer(&bin.rhs);
                        // `count += 1`, and `count++`, which is written that way.
                        let rhs = self.int_beside(&rhs, &bin.rhs, &target);
                        let result = self.binary(op_syntax, &target, &rhs, &bin.rhs, op.position());
                        if !self.assignable(&result, &target) {
                            let message = format!(
                                "`{}` makes {}, which does not fit {}",
                                op.get_op_assignment_info().map_or("", |i| i.3),
                                self.show(&result),
                                self.show(&target)
                            );
                            self.error(op.position(), message);
                        }
                    }
                }
                Type::Null
            }
            Stmt::If(x, _) => {
                self.infer(&x.expr);
                let yes = self.facts_of(&x.expr, true);
                let no = self.facts_of(&x.expr, false);
                let a = self.with_facts(yes, |c| c.check_block(&x.body));
                let b = self.with_facts(no, |c| c.check_block(&x.branch));
                Type::union([a, b])
            }
            Stmt::Switch(x, pos) => self.check_switch(&x.0, &x.1, *pos, None),
            Stmt::While(x, _) => {
                self.infer(&x.expr);
                let yes = self.facts_of(&x.expr, true);
                self.with_facts(yes, |c| c.check_block(&x.body));
                Type::Null
            }
            Stmt::Do(x, ..) => {
                self.infer(&x.expr);
                self.check_block(&x.body);
                Type::Null
            }
            Stmt::For(x, _) => {
                let (var, counter, flow) = &**x;
                let iterable = self.infer(&flow.expr);
                let item = self.element_of(&flow.expr, &iterable);
                // `for k in keys(m)`: every `k` is a key `m` has.
                let keyed = match &flow.expr {
                    Expr::FnCall(call, _) if call.name == "keys" && call.args.len() == 1 => {
                        path_of(&call.args[0]).zip(Some(call.args[0].clone()))
                    }
                    _ => None,
                };
                let mut facts = Vec::new();
                if let Some((base, map)) = keyed {
                    let t = self.infer_quietly(&map);
                    if let Type::Dict(value) = without_null(&self.resolve(&t)) {
                        facts.push((format!("{base}[{}]", var.name), *value));
                    }
                }
                self.with_scope(|c| {
                    c.add_facts(facts);
                    c.bind(var.name.as_str(), item);
                    if let Some(counter) = counter {
                        c.bind(counter.name.as_str(), Type::Int);
                    }
                    c.check_statements(flow.body.statements());
                });
                Type::Null
            }
            Stmt::FnCall(call, pos) => self.call(call, *pos, None),
            Stmt::Block(block) => self.check_block(block),
            Stmt::TryCatch(x, _) => {
                self.check_block(&x.body);
                self.with_scope(|c| {
                    if let Expr::Variable(v, ..) = &x.expr {
                        c.bind(v.1.as_str(), Type::Any);
                    }
                    c.check_statements(x.branch.statements());
                });
                Type::Null
            }
            Stmt::Expr(e) => self.infer(e),
            Stmt::Return(value, flags, pos) => {
                // `throw` is a `return` with a flag, and what it throws is not
                // a result.
                let thrown = flags.contains(ASTFlags::BREAK);
                if let (false, Some(Some((name, want)))) = (thrown, self.results.last().cloned()) {
                    match value {
                        Some(v) => self.check_result(v, &name, &want),
                        None if !self.assignable(&Type::Null, &want) => {
                            let message = format!(
                                "`{name}` is declared to return {}, and this returns nothing",
                                self.show(&want)
                            );
                            self.error(*pos, message);
                        }
                        None => {}
                    }
                    return Type::Null;
                }
                let ty = value.as_ref().map_or(Type::Null, |v| self.infer(v));
                if let Some(returns) = self.returns.last_mut() {
                    returns.push(ty);
                }
                Type::Null
            }
            Stmt::BreakLoop(value, ..) => {
                if let Some(v) = value {
                    self.infer(v);
                }
                Type::Null
            }
            _ => Type::Null,
        }
    }

    /// A `switch`: each arm runs knowing which case matched, and one on a
    /// literal union with no `_` arm must handle every member.
    fn check_switch(
        &mut self,
        subject: &Expr,
        cases: &rhai::SwitchCasesCollection,
        pos: Position,
        want: Option<(&str, &Type)>,
    ) -> Type {
        let subject_type = self.infer(subject);
        let path = path_of(subject);
        // The parser keeps only each case value's hash, so the members are
        // hashed the same way to see which arm, if any, each one reaches.
        let members = literal_members(&without_null(&self.resolve(&subject_type)))
            .unwrap_or_default()
            .into_iter()
            .map(|quoted| quoted.trim_matches('"').to_string())
            .collect::<Vec<_>>();
        let mut arm_members: HashMap<usize, Vec<String>> = HashMap::new();
        let mut unhandled = Vec::new();
        for m in &members {
            match cases.cases.get(&case_hash(m)) {
                Some(arms) => {
                    for arm in arms.iter() {
                        arm_members.entry(*arm).or_default().push(m.clone());
                    }
                }
                None => unhandled.push(m.clone()),
            }
        }
        if cases.def_case.is_none() && !unhandled.is_empty() && !members.is_empty() {
            let listed: Vec<String> = unhandled.iter().map(|m| format!("{m:?}")).collect();
            self.error(
                pos,
                format!(
                    "this `switch` does not handle {}, and has no `_` arm for {}",
                    listed.join(", "),
                    if unhandled.len() == 1 { "it" } else { "them" }
                ),
            );
        }

        let mut out = Vec::new();
        for (i, case) in cases.expressions.iter().enumerate() {
            let matched = if cases.def_case == Some(i) { Some(&unhandled) } else { arm_members.get(&i) };
            let mut facts = Vec::new();
            if let (Some(path), Some(lits)) = (&path, matched) {
                if !lits.is_empty() {
                    let narrowed = Type::union(lits.iter().map(|l| Type::Literal(l.clone())));
                    facts.push((path.clone(), narrowed));
                    // A discriminant narrows the value it belongs to.
                    if let Some(cut) = path.rfind('.') {
                        let (parent, field) = (&path[..cut], &path[cut + 1..]);
                        if let Some(Type::Union(all)) = self.type_at(parent).map(|t| self.resolve(&t)) {
                            let kept: Vec<Type> = all
                                .iter()
                                .filter(|m| match self.resolve(m) {
                                    Type::Record(fields) => fields.iter().any(|f| {
                                        f.name == field
                                            && lits.iter().any(|l| self.assignable(&Type::Literal(l.clone()), &f.ty))
                                    }),
                                    _ => false,
                                })
                                .cloned()
                                .collect();
                            if !kept.is_empty() {
                                facts.push((parent.to_string(), Type::union(kept)));
                            }
                        }
                    }
                }
            }
            let arm = self.with_facts(facts, |c| {
                c.infer(&case.lhs);
                match want {
                    // Each arm is a value the function returns.
                    Some((name, want)) => {
                        c.check_result(&case.rhs, name, want);
                        want.clone()
                    }
                    None => c.infer(&case.rhs),
                }
            });
            out.push(arm);
        }
        if out.is_empty() {
            Type::Null
        } else {
            Type::union(out)
        }
    }

    fn check_let(&mut self, name: &str, pos: Position, value: &Expr) {
        if let Some((_, ty)) = self.cx.placeholders.iter().find(|(n, _)| n == name) {
            let ty = ty.clone().unwrap_or(Type::Any);
            self.bind(name, ty);
            return;
        }
        if let Some(ty) = self.declared.get(&pos).cloned() {
            self.check_expr(value, &ty);
            self.bind(name, ty);
            return;
        }
        let ty = widen(&self.infer(value));
        let why = uninformative(value).filter(|why| match why {
            // A `host::` function with a signature says what it returns.
            Blank::Host(f) => !self.cx.host.iter().any(|(n, _)| n == f),
            _ => true,
        });
        if let Some(why) = why {
            let hint = match why {
                Blank::EmptyList => format!(
                    "`{name}` starts as an empty list, which says nothing about what it will \
                     hold, so nothing done with it is checked. Say what it holds: \
                     `let {name}: T[] = …`"
                ),
                Blank::EmptyMap => format!(
                    "`{name}` starts as `{{}}`, which says nothing about what it will hold, so \
                     nothing done with it is checked. Say what it holds: \
                     `let {name}: {{ … }} = …`"
                ),
                Blank::Null => format!(
                    "`{name}` starts as `null`, which says nothing about what it will hold, so \
                     nothing done with it is checked. Say what it holds: `let {name}: T? = …`"
                ),
                Blank::Host(f) => format!(
                    "`host::{f}` has no signature, so what `{name}` holds is not checked. Say \
                     what it returns: `let {name}: T = …`"
                ),
            };
            self.warn(pos, hint);
            self.bind(name, Type::Any);
            return;
        }
        self.bind(name, ty);
    }

    // ----- Expressions -----------------------------------------------------

    /// Check `e` where a `want` is expected, reporting it if it does not fit.
    fn check_expr(&mut self, e: &Expr, want: &Type) {
        let resolved = self.resolve(want);
        if resolved == Type::Any {
            self.infer(e);
            return;
        }
        match e {
            Expr::IntegerConstant(..) if self.accepts_int(&resolved) => {}
            Expr::FnCall(call, _) if call.name == "signal" && call.args.len() == 1 => {
                self.check_expr(&call.args[0], want);
            }
            // A negative whole number is written as `-` applied to one.
            Expr::FnCall(call, _)
                if call.name == "-"
                    && call.args.len() == 1
                    && matches!(call.args[0], Expr::IntegerConstant(..))
                    && self.accepts_int(&resolved) => {}
            Expr::Array(items, pos) => match self.array_member(&resolved) {
                Some(item) => {
                    for i in items.iter() {
                        self.check_expr(i, &item);
                    }
                }
                None => self.mismatch(e, *pos, want),
            },
            Expr::Map(x, pos) => self.check_map(&x.0, *pos, want, &resolved),
            _ if self.closure_of(e).is_some() => {
                let expected = match &resolved {
                    Type::Function(p, r) => Some((p.clone(), (**r).clone())),
                    _ => None,
                };
                let got = self.closure(e, expected);
                if !self.assignable(&got, want) {
                    self.mismatch_ty(e.position(), &got, want);
                }
            }
            Expr::Stmt(block) if self.closure_of(e).is_none() => {
                // A block used as a value: its last statement is the value.
                let got = self.check_block(block);
                if !self.assignable(&got, want) {
                    self.mismatch_ty(e.position(), &got, want);
                }
            }
            _ => {
                let got = self.infer(e);
                if !self.assignable(&got, want) {
                    self.mismatch_ty(e.position(), &got, want);
                }
            }
        }
    }

    fn mismatch(&mut self, e: &Expr, pos: Position, want: &Type) {
        let got = self.infer(e);
        self.mismatch_ty(pos, &got, want);
    }

    fn mismatch_ty(&mut self, pos: Position, got: &Type, want: &Type) {
        // A literal union is a closed list, so the useful answer is the list,
        // and the member that was probably meant.
        if let (Type::Literal(value), Some(members)) = (got, literal_members(&self.resolve(want))) {
            let bare: Vec<&str> = members.iter().map(|m| m.trim_matches('"')).collect();
            let hint = near_miss(value, &bare).map(|m| format!("; did you mean \"{m}\"?")).unwrap_or_default();
            let message = format!(
                "{value:?} is not {}, which is one of {}{hint}",
                self.show(want),
                members.join(", ")
            );
            self.error(pos, message);
            return;
        }
        let message = format!("this is {}, where {} is expected", self.show(got), self.show(want));
        self.error(pos, message);
    }

    /// A whole-number literal beside an `int` counts as one, so `count + 1`
    /// and `count += 1` keep `count` an `int`. Anything else is left as it is.
    fn int_beside(&self, ty: &Type, e: &Expr, other: &Type) -> Type {
        if matches!(e, Expr::IntegerConstant(..)) && self.resolve(other) == Type::Int {
            Type::Int
        } else {
            ty.clone()
        }
    }

    /// Whether a whole-number literal fits `ty`.
    fn accepts_int(&self, ty: &Type) -> bool {
        match self.resolve(ty) {
            Type::Int | Type::Number | Type::Any => true,
            Type::Union(members) => members.iter().any(|m| self.accepts_int(m)),
            _ => false,
        }
    }

    /// The element type an array literal takes where `ty` is wanted.
    fn array_member(&self, ty: &Type) -> Option<Type> {
        match self.resolve(ty) {
            Type::Array(item) => Some(*item),
            Type::Any => Some(Type::Any),
            Type::Union(members) => members.iter().find_map(|m| self.array_member(m)),
            _ => None,
        }
    }

    /// A map literal where `want` is expected.
    fn check_map(&mut self, fields: &[(rhai::Ident, Expr)], pos: Position, want: &Type, resolved: &Type) {
        match resolved {
            Type::Record(expected) => {
                for (ident, value) in fields {
                    match expected.iter().find(|f| f.name == ident.name.as_str()) {
                        Some(field) => {
                            let ty = if field.optional { field.ty.clone().optional() } else { field.ty.clone() };
                            self.check_expr(value, &ty);
                        }
                        None => {
                            self.infer(value);
                            let names: Vec<&str> = expected.iter().map(|f| f.name.as_str()).collect();
                            let hint = near_miss(ident.name.as_str(), &names)
                                .map(|n| format!("; did you mean `{n}`?"))
                                .unwrap_or_default();
                            let message = format!(
                                "{} has no field `{}`{hint}",
                                self.show(want),
                                ident.name
                            );
                            self.error(ident.pos, message);
                        }
                    }
                }
                let missing: Vec<&Field> = expected
                    .iter()
                    .filter(|f| !f.optional && !fields.iter().any(|(i, _)| i.name.as_str() == f.name))
                    .collect();
                if !missing.is_empty() {
                    let names: Vec<String> = missing.iter().map(|f| format!("`{}`", f.name)).collect();
                    let message = format!(
                        "this is missing {} {}, which {} requires",
                        if names.len() == 1 { "the field" } else { "the fields" },
                        names.join(", "),
                        self.show(want)
                    );
                    self.error(pos, message);
                }
            }
            Type::Dict(value) => {
                for (_, v) in fields {
                    self.check_expr(v, value);
                }
            }
            Type::Union(members) => {
                // The first member it fits without a word said. None fitting
                // is one error naming the union, not one per member.
                for member in members {
                    if self.fits_quietly(|c| c.check_map(fields, pos, member, &c.resolve(member))) {
                        self.check_map(fields, pos, member, &self.resolve(member));
                        return;
                    }
                }
                let got = self.infer_map(fields);
                self.mismatch_ty(pos, &got, want);
            }
            _ => {
                let got = self.infer_map(fields);
                if !self.assignable(&got, want) {
                    self.mismatch_ty(pos, &got, want);
                }
            }
        }
    }

    /// Whether `f` runs without an error, saying nothing either way.
    fn fits_quietly(&mut self, f: impl FnOnce(&mut Self)) -> bool {
        let before = self.quiet_errors;
        self.quiet += 1;
        f(self);
        self.quiet -= 1;
        let fits = self.quiet_errors == before;
        self.quiet_errors = before;
        fits
    }

    fn infer_map(&mut self, fields: &[(rhai::Ident, Expr)]) -> Type {
        Type::Record(
            fields
                .iter()
                .map(|(ident, value)| Field {
                    name: ident.name.to_string(),
                    optional: false,
                    ty: widen(&self.infer(value)),
                })
                .collect(),
        )
    }

    /// The type of `e`.
    fn infer(&mut self, e: &Expr) -> Type {
        if self.closure_of(e).is_some() {
            return self.closure(e, None);
        }
        match e {
            Expr::DynamicConstant(d, _) => dynamic_type(d),
            Expr::BoolConstant(..) => Type::Bool,
            Expr::IntegerConstant(..) | Expr::FloatConstant(..) => Type::Number,
            Expr::CharConstant(..) => Type::String,
            Expr::StringConstant(s, _) => Type::Literal(s.to_string()),
            Expr::InterpolatedString(parts, _) => {
                for p in parts.iter() {
                    self.infer(p);
                }
                Type::String
            }
            Expr::Array(items, _) => {
                let members: Vec<Type> = items.iter().map(|i| widen(&self.infer(i))).collect();
                if members.is_empty() {
                    Type::Array(Box::new(Type::Any))
                } else {
                    Type::Array(Box::new(Type::union(members)))
                }
            }
            Expr::Map(x, _) => self.infer_map(&x.0),
            Expr::Unit(_) => Type::Null,
            Expr::Variable(v, ..) => {
                if !v.2.is_empty() {
                    return Type::Any;
                }
                let name = v.1.as_str();
                match self.lookup(name) {
                    Some(t) => t,
                    None => {
                        self.caller_local(name, e.position());
                        Type::Any
                    }
                }
            }
            Expr::Stmt(block) => self.check_block(block),
            Expr::FnCall(call, pos) => self.call(call, *pos, None),
            Expr::Dot(x, flags, _) | Expr::Index(x, flags, _) => {
                let base = self.infer(&x.lhs);
                let index = matches!(e, Expr::Index(..));
                let path = path_of(&x.lhs);
                let (ty, short) = self.chain(base, index, *flags, &x.rhs, path);
                if short {
                    ty.optional()
                } else {
                    ty
                }
            }
            // Each operand runs knowing the ones before it held (`&&`) or
            // failed (`||`): `t != null && t.done` reads `t` as not `null`.
            Expr::And(items, _) | Expr::Or(items, _) => {
                let truth = matches!(e, Expr::And(..));
                let mut known: Vec<(String, Type)> = Vec::new();
                for i in items.iter() {
                    self.with_facts(known.clone(), |c| c.infer(i));
                    let more = self.with_facts(known.clone(), |c| c.facts_of(i, truth));
                    known.extend(more);
                }
                Type::Bool
            }
            Expr::Coalesce(items, _) => {
                let n = items.len();
                let mut out = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    let t = self.infer(item);
                    out.push(if i + 1 < n { without_null(&self.resolve(&t)) } else { t });
                }
                Type::union(out)
            }
            Expr::Custom(c, _) => {
                for input in c.inputs.iter() {
                    self.infer(input);
                }
                if c.tokens.first().is_some_and(|t| t.as_str() == "null") {
                    Type::Null
                } else {
                    Type::Any
                }
            }
            _ => Type::Any,
        }
    }

    /// A chain step and the rest of the chain after it. `index` says whether
    /// the step is `[ ]` rather than `.`, and `flags` whether it was written
    /// `?.` or `?[`. The second half of the answer is whether any step was
    /// optional, which makes the whole chain possibly `null`.
    fn chain(
        &mut self,
        base: Type,
        index: bool,
        flags: ASTFlags,
        rhs: &Expr,
        path: Option<String>,
    ) -> (Type, bool) {
        let optional = flags.contains(ASTFlags::NEGATED);
        match rhs {
            Expr::Dot(x, inner, _) | Expr::Index(x, inner, _) => {
                let (step, here) = self.step(&base, index, optional, &x.lhs, path);
                let next_index = matches!(rhs, Expr::Index(..));
                let (ty, short) = self.chain(step, next_index, *inner, &x.rhs, here);
                (ty, short || optional)
            }
            _ => (self.step(&base, index, optional, rhs, path).0, optional),
        }
    }

    /// One step of a chain: `.name`, `.method(…)` or `[index]` applied to a
    /// `base` that sits at `path`. Returns the step's type and its own path.
    fn step(
        &mut self,
        base: &Type,
        index: bool,
        optional: bool,
        what: &Expr,
        path: Option<String>,
    ) -> (Type, Option<String>) {
        let here = path.clone().and_then(|p| path_step(p, index, what));
        // What a condition established about this step wins: `t.note` inside
        // `if t?.note != null` is present, whatever the record says. A fact
        // that it may be `null` says it may be absent, so the rules for
        // reading an absent one still apply, and only the type is taken.
        let known = here.as_deref().and_then(|p| self.fact(p));
        if let Some(known) = &known {
            if !may_be_null(&self.resolve(known)) {
                if index {
                    self.infer(what);
                }
                return (known.clone(), here);
            }
        }
        // Messages name the type as written (`Task`), not what it expands to.
        let shown = if optional { without_null(base) } else { base.clone() };
        let base = self.resolve(&shown);
        let read = Read { optional, path: path.as_deref() };
        let ty = if index {
            let key = self.infer(what);
            self.index(&base, &shown, &key, what, read)
        } else {
            match what {
                Expr::Property(p, pos) => self.property(&base, &shown, p.2.as_str(), *pos, read),
                Expr::MethodCall(call, pos) => self.method(&base, call, *pos),
                // A chain whose first step is a variable used as a property,
                // which rhai writes for `a.b` inside some shapes.
                Expr::Variable(v, ..) => {
                    self.property(&base, &shown, v.1.as_str(), what.position(), read)
                }
                other => {
                    self.infer(other);
                    Type::Any
                }
            }
        };
        (known.unwrap_or(ty), here)
    }

    fn property(&mut self, base: &Type, shown: &Type, name: &str, pos: Position, read: Read) -> Type {
        match base {
            Type::Any => Type::Any,
            Type::Array(_) | Type::String | Type::Literal(_) if name == "length" => Type::Int,
            Type::Record(fields) => match fields.iter().find(|f| f.name == name) {
                Some(f) if f.optional => {
                    // The field may not be there, and strict map properties
                    // make reading an absent one raise: `?.` is how a program
                    // says it knows. Narrowing is the other way, handled above.
                    if !read.optional {
                        let whole = read.path.map_or_else(|| "…".to_string(), str::to_string);
                        self.error(
                            pos,
                            format!(
                                "`{name}` may be absent from {}, so read it as `{whole}?.{name}`, \
                                 or check it first: `if {whole}?.{name} != null {{ … }}`",
                                self.show(shown)
                            ),
                        );
                    }
                    f.ty.clone().optional()
                }
                Some(f) => f.ty.clone(),
                None => {
                    let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
                    let hint =
                        near_miss(name, &names).map(|n| format!("; did you mean `{n}`?")).unwrap_or_default();
                    self.error(pos, format!("{} has no field `{name}`{hint}", self.show(shown)));
                    Type::Any
                }
            },
            Type::Dict(value) => {
                // Any key of a dictionary may be missing, as an optional field
                // may, and reading a missing one raises the same way.
                if !read.optional {
                    let whole = read.path.map_or_else(|| "…".to_string(), str::to_string);
                    self.error(
                        pos,
                        format!(
                            "{} may have no `{name}`, so read it as `{whole}?.{name}`, or check \
                             first: `if \"{name}\" in {whole} {{ … }}`",
                            self.show(shown)
                        ),
                    );
                }
                (**value).clone()
            }
            Type::Union(members) => {
                let members = members.clone();
                // Reported once for the union rather than once per member.
                let before = self.findings.len();
                let out: Vec<Type> = members
                    .iter()
                    .filter(|m| !matches!(m, Type::Null))
                    .map(|m| {
                        let m = self.resolve(m);
                        self.property(&m, &m, name, pos, read)
                    })
                    .collect();
                if self.findings.len() > before + 1 {
                    self.findings.truncate(before + 1);
                }
                Type::union(out)
            }
            // `.length` on a map, or a field on a number: rhai raises, and so
            // does this.
            Type::Number | Type::Int | Type::Bool | Type::String | Type::Literal(_) | Type::Array(_) => {
                self.error(pos, format!("{} has no property `{name}`", self.show(shown)));
                Type::Any
            }
            Type::Null => {
                self.error(
                    pos,
                    format!("this is `null` here, so it has no `{name}`; read it with `?.` if it may be absent"),
                );
                Type::Any
            }
            _ => Type::Any,
        }
    }

    fn index(&mut self, base: &Type, shown: &Type, key: &Type, what: &Expr, read: Read) -> Type {
        match base {
            Type::Array(item) => {
                if !self.assignable(key, &Type::Number) {
                    let message = format!("a list is indexed by a number, and this is {}", self.show(key));
                    self.error(what.position(), message);
                }
                (**item).clone()
            }
            Type::Dict(value) => {
                // A key may be missing, and `[` raises on one that is: `?[`
                // reads it as possibly `null`, and `k in m` rules it out.
                if !read.optional {
                    let whole = read.path.map_or_else(|| "…".to_string(), str::to_string);
                    let k = match what {
                        Expr::Variable(v, ..) => v.1.to_string(),
                        Expr::StringConstant(s, _) => format!("{s:?}"),
                        _ => "k".to_string(),
                    };
                    self.error(
                        what.position(),
                        format!(
                            "{} may have no such key, so read it as `{whole}?[{k}]`, or check \
                             first: `if {k} in {whole} {{ … }}`",
                            self.show(shown)
                        ),
                    );
                }
                (**value).clone()
            }
            Type::Record(fields) => match self.resolve(key) {
                Type::Literal(k) => match fields.iter().find(|f| f.name == k) {
                    Some(f) if f.optional => f.ty.clone().optional(),
                    Some(f) => f.ty.clone(),
                    None => {
                        self.error(what.position(), format!("{} has no field `{k}`", self.show(shown)));
                        Type::Any
                    }
                },
                // Every member of a literal union a field: a lookup table.
                Type::Union(keys) if keys.iter().all(|k| matches!(k, Type::Literal(_))) => {
                    let mut out = Vec::new();
                    for k in keys {
                        if let Type::Literal(k) = k {
                            match fields.iter().find(|f| f.name == *k) {
                                Some(f) => out.push(f.ty.clone()),
                                None => {
                                    self.error(
                                        what.position(),
                                        format!("{} has no field `{k}`", self.show(shown)),
                                    );
                                    return Type::Any;
                                }
                            }
                        }
                    }
                    Type::union(out)
                }
                _ => Type::Any,
            },
            _ => Type::Any,
        }
    }

    /// The type a `for` loop hands its variable, iterating `ty`.
    fn element_of(&self, e: &Expr, ty: &Type) -> Type {
        if let Expr::FnCall(call, _) = e {
            if matches!(call.name.as_str(), ".." | "..=" | "range") {
                return Type::Int;
            }
        }
        match self.resolve(ty) {
            Type::Array(item) => *item,
            Type::String | Type::Literal(_) => Type::String,
            _ => Type::Any,
        }
    }

    // ----- Operators -------------------------------------------------------

    fn binary(&mut self, op: &str, a: &Type, b: &Type, at: &Expr, pos: Position) -> Type {
        let a = self.resolve(a);
        let b = self.resolve(b);
        if a == Type::Any || b == Type::Any {
            return match op {
                "==" | "!=" | "<" | ">" | "<=" | ">=" | "===" | "!==" => Type::Bool,
                _ => Type::Any,
            };
        }
        let number = |t: &Type| matches!(t, Type::Number | Type::Int);
        let text = |t: &Type| matches!(t, Type::String | Type::Literal(_));
        match op {
            "+" if text(&a) || text(&b) => Type::String,
            "+" if matches!(a, Type::Array(_)) && matches!(b, Type::Array(_)) => {
                let (Type::Array(x), Type::Array(y)) = (&a, &b) else { unreachable!() };
                Type::Array(Box::new(Type::union([(**x).clone(), (**y).clone()])))
            }
            "+" | "-" | "*" | "%" if a == Type::Int && b == Type::Int => Type::Int,
            "+" | "-" | "*" | "/" | "%" | "**" if number(&a) && number(&b) => Type::Number,
            "==" | "!=" | "===" | "!==" => Type::Bool,
            "<" | ">" | "<=" | ">=" => Type::Bool,
            "&" | "|" | "^" | "<<" | ">>" => Type::Number,
            _ => {
                let message = format!(
                    "`{op}` cannot be applied to {} and {}",
                    self.show(&a),
                    self.show(&b)
                );
                self.error(if pos.is_none() { at.position() } else { pos }, message);
                Type::Any
            }
        }
    }

    // ----- Calls -----------------------------------------------------------

    /// A call, by name. `receiver` is unused for a free call; methods go
    /// through [`Self::method`].
    fn call(&mut self, call: &rhai::FnCallExpr, pos: Position, _receiver: Option<Type>) -> Type {
        let name = call.name.as_str();
        let args = &call.args;

        if !call.namespace.is_empty() {
            let space = call.namespace.to_string();
            for a in args.iter() {
                self.infer(a);
            }
            if space.trim_end_matches("::") == "host" {
                if let Some((_, Type::Function(_, result))) = self.cx.host.iter().find(|(n, _)| n == name) {
                    return (**result).clone();
                }
            }
            return Type::Any;
        }

        // Operators come through here too, with a name like `+`.
        if call.op_token.is_some() || is_operator(name) {
            return match args.len() {
                1 => {
                    let t = self.infer(&args[0]);
                    match name {
                        "!" => Type::Bool,
                        "-" | "+" => match self.resolve(&t) {
                            Type::Int => Type::Int,
                            _ => Type::Number,
                        },
                        _ => Type::Any,
                    }
                }
                2 => {
                    if name == "contains" {
                        // `k in m`, which rhai writes as `m.contains(k)`.
                        self.infer(&args[0]);
                        self.infer(&args[1]);
                        return Type::Bool;
                    }
                    let a = self.infer(&args[0]);
                    let b = self.infer(&args[1]);
                    if matches!(name, ".." | "..=") {
                        return Type::Any;
                    }
                    // `count + 1` stays an `int` when `count` is one.
                    let a = self.int_beside(&a, &args[0], &b);
                    let b = self.int_beside(&b, &args[1], &a);
                    if matches!(name, "==" | "!=" | "===" | "!==") {
                        self.check_comparable(&args[0], &a, &args[1], &b, pos);
                    }
                    self.binary(name, &a, &b, &args[1], pos)
                }
                _ => Type::Any,
            };
        }

        match (name, args.len()) {
            ("signal", 1) => {
                let t = self.infer(&args[0]);
                // Numbers are coerced to floats on the way through.
                return match t {
                    Type::Int => Type::Number,
                    other => other,
                };
            }
            ("Number" | "parseInt" | "parseFloat" | "parse_int" | "parse_float" | "to_float", _)
            | ("max" | "min" | "abs" | "floor" | "ceil" | "round" | "sqrt", _) => {
                self.infer_all(args);
                return Type::Number;
            }
            ("to_int", _) => {
                self.infer_all(args);
                return Type::Int;
            }
            ("String" | "type_of" | "path_for" | "to_string", _) => {
                self.infer_all(args);
                return Type::String;
            }
            ("isNaN" | "is_def_var" | "is_def_fn", _) => {
                self.infer_all(args);
                return Type::Bool;
            }
            ("keys", 1) => {
                self.infer_all(args);
                return Type::Array(Box::new(Type::String));
            }
            ("values", 1) => {
                let m = self.infer(&args[0]);
                return Type::Array(Box::new(match self.resolve(&m) {
                    Type::Dict(v) => *v,
                    Type::Record(fields) => Type::union(fields.into_iter().map(|f| f.ty)),
                    _ => Type::Any,
                }));
            }
            ("print" | "debug" | "emit" | "navigate" | "replace" | "back" | "forward" | "blur"
            | "clearInterval", _) => {
                self.infer_all(args);
                return Type::Null;
            }
            ("__interval", _) => {
                self.infer_all(args);
                return Type::Number;
            }
            _ => {}
        }

        // A function of the script's own.
        if self.fns.contains_key(&(name.to_string(), args.len())) {
            return self.call_script_fn(name, args, pos);
        }
        // An arrow kept in a variable, called by name.
        if let Some(Type::Function(params, result)) = self.lookup(name).map(|t| self.resolve(&t)) {
            for (a, p) in args.iter().zip(params.iter()) {
                self.check_expr(a, p);
            }
            return *result;
        }
        self.infer_all(args);
        Type::Any
    }

    /// A name a function body read that is none of its own and no document
    /// name: it can only come from whoever called it, a template (an `r-for`,
    /// a handler's `let`) or another function (its `let`s and parameters).
    fn caller_local(&mut self, name: &str, pos: Position) {
        let Some((function, typed)) = self.in_fn.last().cloned() else { return };
        if !self.cx.caller_names.contains(name) && !self.script_names.contains(name) {
            // Declared nowhere at all: the undefined-name check says so.
            return;
        }
        // An untyped function keeps the fork's behaviour: it runs in its
        // caller's scope and may read what the caller has.
        if typed {
            self.error(
                pos,
                format!(
                    "`{function}` reads `{name}`, which only whoever calls it has. A function \
                     whose parameters are all typed, or that has none, sees its own names and the \
                     document's, not its caller's: pass `{name}` in as a parameter"
                ),
            );
        }
    }

    /// Infer each argument of a call whose signature is not known. A closure
    /// among them gets `any` parameters without a word: whatever it is handed
    /// is already unchecked, and that was reported where it began.
    /// A comparison that can never be true is almost always a typo: `filter ==
    /// "al"` for `"all"`, or a number compared with text. `null` is always
    /// allowed on either side, since checking for it is how a program is
    /// careful.
    fn check_comparable(&mut self, left: &Expr, a: &Type, right: &Expr, b: &Type, pos: Position) {
        if self.overlaps(a, b) {
            return;
        }
        for (side, other, t) in [(left, right, a), (right, left, b)] {
            if let Expr::StringConstant(s, _) = other {
                if let Some(members) = literal_members(&self.resolve(t)) {
                    let bare: Vec<&str> = members.iter().map(|m| m.trim_matches('"')).collect();
                    let hint =
                        near_miss(s, &bare).map(|m| format!("; did you mean \"{m}\"?")).unwrap_or_default();
                    let who = path_of(side).map_or_else(|| "this".to_string(), |p| format!("`{p}`"));
                    let message = format!(
                        "{who} is {}, which is one of {}, so it is never {s:?}{hint}",
                        self.show(t),
                        members.join(", ")
                    );
                    self.error(pos, message);
                    return;
                }
            }
        }
        let message =
            format!("this compares {} with {}, which are never equal", self.show(a), self.show(b));
        self.error(pos, message);
    }

    /// Whether a value of type `a` can ever equal one of type `b`.
    fn overlaps(&self, a: &Type, b: &Type) -> bool {
        let a = self.resolve(a);
        let b = self.resolve(b);
        match (&a, &b) {
            (Type::Any, _) | (_, Type::Any) | (Type::Null, _) | (_, Type::Null) => true,
            (Type::Union(m), _) => m.iter().any(|x| self.overlaps(x, &b)),
            (_, Type::Union(m)) => m.iter().any(|x| self.overlaps(&a, x)),
            (Type::Literal(x), Type::Literal(y)) => x == y,
            (Type::Literal(_) | Type::String, Type::Literal(_) | Type::String) => true,
            (Type::Number | Type::Int, Type::Number | Type::Int) => true,
            (Type::Bool, Type::Bool) => true,
            (Type::Record(_) | Type::Dict(_), Type::Record(_) | Type::Dict(_)) => true,
            (Type::Array(_), Type::Array(_)) | (Type::Function(..), Type::Function(..)) => true,
            (Type::Named(_), _) | (_, Type::Named(_)) => true,
            _ => false,
        }
    }

    fn infer_all(&mut self, args: &[Expr]) {
        for a in args {
            if self.closure_of(a).is_some() {
                self.closure(a, Some((Vec::new(), Type::Any)));
            } else {
                self.infer(a);
            }
        }
    }

    /// The type of `e`, saying nothing about it.
    fn infer_quietly(&mut self, e: &Expr) -> Type {
        let errors = self.quiet_errors;
        self.quiet += 1;
        let t = self.infer(e);
        self.quiet -= 1;
        self.quiet_errors = errors;
        t
    }

    fn call_script_fn(&mut self, name: &str, args: &[Expr], pos: Position) -> Type {
        let key = (name.to_string(), args.len());
        let params: Vec<(String, Option<Type>)> = self.fns[&key].params.clone();
        for (arg, (param, ty)) in args.iter().zip(params.iter()) {
            match ty {
                Some(ty) => {
                    let before = self.findings.len();
                    self.check_expr(arg, ty);
                    // Say which parameter the value was for.
                    if self.findings.len() == before + 1 {
                        let got = self.infer_quietly(arg);
                        let last = self.findings.last_mut().unwrap();
                        if last.is_error && last.message.starts_with("this is ") {
                            last.message =
                                format!("`{name}` takes `{ty}` as `{param}`, and this is `{got}`");
                        }
                    }
                }
                None => {
                    self.infer(arg);
                }
            }
        }
        let _ = pos;
        let result = self.function_result(name, args.len());
        // Whatever the function writes may no longer be what a condition
        // established: a fact about a signal dies at a call that writes it.
        for written in self.writes_of(name, args.len()) {
            self.forget(&written);
        }
        result
    }

    /// What a named function returns, checking its body the first time it is
    /// asked.
    fn function_result(&mut self, name: &str, arity: usize) -> Type {
        let key = (name.to_string(), arity);
        let Some(info) = self.fns.get(&key).cloned() else { return Type::Any };
        match info.state {
            FnState::Done(t) => return info.result.unwrap_or(t),
            FnState::Checking => {
                // Recursion. A declared result answers it; without one there is
                // no answer to be had, and the author is told to write one.
                return match info.result {
                    Some(t) => t,
                    None => {
                        let at = info.def.body.position();
                        self.error(
                            at,
                            format!(
                                "`{name}` calls itself, so its result cannot be worked out from \
                                 its body. Write it: `fn {name}(…): T`"
                            ),
                        );
                        Type::Any
                    }
                };
            }
            FnState::Unchecked => {}
        }
        if let Some(f) = self.fns.get_mut(&key) {
            f.state = FnState::Checking;
        }

        // Unannotated parameters: `any`, and a warning, once, where the function
        // is written.
        let at = info.def.body.position();
        let untyped: Vec<&str> =
            info.params.iter().filter(|(_, t)| t.is_none()).map(|(p, _)| p.as_str()).collect();
        if !untyped.is_empty() && !info.is_anonymous() {
            let listed: Vec<String> = untyped.iter().map(|p| format!("`{p}`")).collect();
            let signature: Vec<String> = info
                .params
                .iter()
                .map(|(p, t)| match t {
                    Some(t) => format!("{p}: {t}"),
                    None => format!("{p}: T"),
                })
                .collect();
            self.warn(
                at,
                format!(
                    "{} {} no type, so what `{name}` is given there is not checked. Annotate \
                     {}: `fn {name}({})`",
                    listed.join(", "),
                    if untyped.len() == 1 { "has" } else { "have" },
                    if untyped.len() == 1 { "it" } else { "them" },
                    signature.join(", ")
                ),
            );
        }

        // The body runs with its parameters in scope and nothing of whoever
        // called it: a script's own locals are not visible here.
        let saved = std::mem::take(&mut self.scopes);
        let saved_facts = std::mem::replace(&mut self.facts, vec![HashMap::new()]);
        // Typed means every parameter annotated, which a function with none is.
        let typed = info.params.iter().all(|(_, t)| t.is_some());
        self.in_fn.push((name.to_string(), typed));
        self.scopes.push(
            info.params.iter().map(|(p, t)| (p.clone(), t.clone().unwrap_or(Type::Any))).collect(),
        );
        let inferred = match &info.result {
            // Declared: every value it hands back is checked against it.
            Some(declared) => {
                self.results.push(Some((name.to_string(), declared.clone())));
                self.check_statements_for(info.def.body.statements(), name, declared);
                self.results.pop();
                declared.clone()
            }
            None => {
                self.results.push(None);
                self.returns.push(Vec::new());
                let last = self.check_statements(info.def.body.statements());
                let mut results = self.returns.pop().unwrap_or_default();
                self.results.pop();
                results.push(last);
                widen(&Type::union(results))
            }
        };
        self.in_fn.pop();
        self.scopes = saved;
        self.facts = saved_facts;
        if let Some(f) = self.fns.get_mut(&key) {
            f.state = FnState::Done(inferred.clone());
        }
        info.result.unwrap_or(inferred)
    }

    // ----- Closures --------------------------------------------------------

    /// The function an expression is a closure of, and how many captured
    /// variables its definition puts ahead of the declared parameters.
    fn closure_of(&self, e: &Expr) -> Option<(Shared<ScriptFuncDef>, usize)> {
        let (ptr, captured) = match e {
            Expr::DynamicConstant(d, _) => (d.clone().try_cast::<FnPtr>()?, 0),
            // A capturing closure is a block that shares the captured names and
            // curries them in: `{ share …; curry(fn, a, b) }`.
            Expr::Stmt(block) => {
                let statements = block.statements();
                let [Stmt::Share(..), Stmt::Expr(call)] = statements else { return None };
                let Expr::FnCall(call, _) = &**call else { return None };
                let Some(Expr::DynamicConstant(d, _)) = call.args.first() else { return None };
                (d.clone().try_cast::<FnPtr>()?, call.args.len() - 1)
            }
            _ => return None,
        };
        let name = ptr.fn_name();
        self.fns.values().find(|f| f.def.name == name).map(|f| (f.def.clone(), captured))
    }

    /// Check a closure's body and return its function type. `expected` is the
    /// shape the place it is used in wants, which is where an unannotated
    /// parameter gets its type from.
    fn closure(&mut self, e: &Expr, expected: Option<(Vec<Type>, Type)>) -> Type {
        let Some((def, captured)) = self.closure_of(e) else { return Type::Any };
        let key = (def.name.to_string(), def.params.len());
        let declared: Vec<(String, Option<Type>)> =
            self.fns.get(&key).map(|f| f.params.clone()).unwrap_or_default();
        let own: Vec<(String, Option<Type>)> = declared.into_iter().skip(captured).collect();

        let mut params = Vec::new();
        for (i, (name, ty)) in own.iter().enumerate() {
            let from_context = expected.as_ref().and_then(|(p, _)| p.get(i).cloned());
            let ty = match (ty, from_context) {
                (Some(t), _) => t.clone(),
                (None, Some(t)) => t,
                (None, None) => {
                    if expected.is_none() {
                        self.warn(
                            e.position(),
                            format!(
                                "`{name}` has no type and nothing here says what it will be \
                                 given, so it is not checked. Say what it takes: `({name}: T) => …`"
                            ),
                        );
                    }
                    Type::Any
                }
            };
            params.push((name.clone(), ty));
        }

        // The body sees the scopes around it, which is where its captured
        // names come from, plus its own parameters.
        self.scopes.push(params.iter().cloned().collect());
        self.returns.push(Vec::new());
        let last = self.check_statements(def.body.statements());
        let mut results = self.returns.pop().unwrap_or_default();
        self.scopes.pop();
        results.push(last);
        let result = Type::union(results);

        if let Some((_, want)) = &expected {
            if !matches!(self.resolve(want), Type::Any | Type::Null) && !self.assignable(&result, want) {
                let message = format!(
                    "this function returns {}, where {} is expected",
                    self.show(&result),
                    self.show(want)
                );
                self.error(e.position(), message);
            }
        }
        Type::Function(params.into_iter().map(|(_, t)| t).collect(), Box::new(widen(&result)))
    }

    // ----- Methods ---------------------------------------------------------

    fn method(&mut self, base: &Type, call: &rhai::FnCallExpr, _pos: Position) -> Type {
        let name = call.name.as_str();
        let args = &call.args;
        match base {
            Type::Array(item) => {
                let item = (**item).clone();
                let callback = |ret: Type| Some((vec![item.clone(), Type::Int], ret));
                match name {
                    "map" if args.len() == 1 => {
                        let f = self.callback(&args[0], callback(Type::Any));
                        return Type::Array(Box::new(f));
                    }
                    "filter" if args.len() == 1 => {
                        self.callback(&args[0], callback(Type::Any));
                        return Type::Array(Box::new(item));
                    }
                    "find" if args.len() == 1 => {
                        self.callback(&args[0], callback(Type::Any));
                        return item.optional();
                    }
                    "findIndex" if args.len() == 1 => {
                        self.callback(&args[0], callback(Type::Any));
                        return Type::Int;
                    }
                    "some" | "every" if args.len() == 1 => {
                        self.callback(&args[0], callback(Type::Any));
                        return Type::Bool;
                    }
                    "forEach" if args.len() == 1 => {
                        self.callback(&args[0], callback(Type::Null));
                        return Type::Null;
                    }
                    "sort" if args.len() == 1 => {
                        self.callback(&args[0], Some((vec![item.clone(), item.clone()], Type::Number)));
                        return Type::Array(Box::new(item));
                    }
                    "reduce" if args.len() == 2 => {
                        let acc = widen(&self.infer(&args[1]));
                        self.callback(&args[0], Some((vec![acc.clone(), item, Type::Int], acc.clone())));
                        return acc;
                    }
                    "push" | "append" if args.len() == 1 => {
                        self.check_expr(&args[0], &item);
                        return Type::Null;
                    }
                    "pop" | "shift" => return item.optional(),
                    "includes" | "contains" => {
                        self.infer_all(args);
                        return Type::Bool;
                    }
                    "indexOf" | "index_of" | "len" => {
                        self.infer_all(args);
                        return Type::Int;
                    }
                    "slice" | "reverse" | "extract" => {
                        self.infer_all(args);
                        return Type::Array(Box::new(item));
                    }
                    "join" => {
                        self.infer_all(args);
                        return Type::String;
                    }
                    _ => {}
                }
            }
            Type::String | Type::Literal(_) => match name {
                "split" => {
                    self.infer_all(args);
                    return Type::Array(Box::new(Type::String));
                }
                "trim" | "toLowerCase" | "toUpperCase" | "to_lower" | "to_upper" | "repeat"
                | "charAt" | "slice" | "substring" | "sub_string" | "to_string" => {
                    self.infer_all(args);
                    return Type::String;
                }
                "startsWith" | "endsWith" | "includes" | "contains" | "starts_with" | "ends_with" => {
                    self.infer_all(args);
                    return Type::Bool;
                }
                "indexOf" | "index_of" | "len" => {
                    self.infer_all(args);
                    return Type::Int;
                }
                _ => {}
            },
            Type::Number | Type::Int => match name {
                "to_int" => return Type::Int,
                "to_float" | "floor" | "ceil" | "round" | "abs" | "sqrt" => return Type::Number,
                "to_string" => return Type::String,
                _ => {}
            },
            Type::Record(_) | Type::Dict(_) => match name {
                "keys" => return Type::Array(Box::new(Type::String)),
                "len" => return Type::Int,
                "contains" => {
                    self.infer_all(args);
                    return Type::Bool;
                }
                _ => {}
            },
            _ => {}
        }
        self.infer_all(args);
        Type::Any
    }

    /// A callback argument, checked against the shape the method hands it.
    /// Returns what the callback returns.
    fn callback(&mut self, arg: &Expr, shape: Option<(Vec<Type>, Type)>) -> Type {
        if self.closure_of(arg).is_some() {
            match self.closure(arg, shape) {
                Type::Function(_, result) => *result,
                _ => Type::Any,
            }
        } else {
            let t = self.infer(arg);
            match self.resolve(&t) {
                Type::Function(_, result) => *result,
                _ => Type::Any,
            }
        }
    }
}

/// The hash a `switch` keys a string case by, computed the way the parser
/// computes it.
fn case_hash(value: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let value = Dynamic::from(rhai::ImmutableString::from(value));
    let mut hasher = rhai::get_hasher();
    value.hash(&mut hasher);
    hasher.finish()
}

/// Whether `null` is among what `t` may be.
fn may_be_null(t: &Type) -> bool {
    match t {
        Type::Null | Type::Any => true,
        Type::Union(m) => m.iter().any(may_be_null),
        _ => false,
    }
}

/// The name a path starts from: `t` for `t.note`, `m` for `m[k]`.
fn root_of(path: &str) -> &str {
    path.split(['.', '[']).next().unwrap_or(path)
}

/// Whether a value of type `t` (with `null` already taken out) can only be
/// falsy by being `null`: records, lists, dictionaries and functions are
/// always truthy, while `""`, `0` and `false` are falsy values of their own.
fn falsy_only_when_null(t: &Type) -> bool {
    match t {
        Type::Record(_) | Type::Array(_) | Type::Dict(_) | Type::Function(..) => true,
        Type::Union(m) => m.iter().all(falsy_only_when_null),
        _ => false,
    }
}

/// Whether `type_of` answers `tag` for a value of type `t`. The names are
/// rhai's own: `"f64"` and `"i64"` for numbers, `"()"` for `null`.
fn is_type_of(t: &Type, tag: &str, types: &HashMap<String, Type>) -> bool {
    let t = match t {
        Type::Named(n) => types.get(n).cloned().unwrap_or(Type::Any),
        other => other.clone(),
    };
    match tag {
        "string" => matches!(t, Type::String | Type::Literal(_)),
        "bool" => t == Type::Bool,
        "array" => matches!(t, Type::Array(_)),
        "map" => matches!(t, Type::Record(_) | Type::Dict(_)),
        "()" => t == Type::Null,
        "f64" | "i64" => matches!(t, Type::Number | Type::Int),
        _ => false,
    }
}

/// Whether a block ends by leaving: a `return`, a `throw`, a `break` or a
/// `continue`, or an `if` both of whose branches do.
fn exits(statements: &[Stmt]) -> bool {
    match statements.last() {
        Some(Stmt::Return(..) | Stmt::BreakLoop(..)) => true,
        Some(Stmt::If(x, _)) => exits(x.body.statements()) && exits(x.branch.statements()),
        Some(Stmt::Block(b)) => exits(b.statements()),
        _ => false,
    }
}

/// How a chain step was written, for the rules that depend on it.
#[derive(Clone, Copy)]
struct Read<'p> {
    /// Written `?.` or `?[`.
    optional: bool,
    /// The path of what is being read from, for a message to quote.
    path: Option<&'p str>,
}

/// The path an expression reads, as `t`, `t.note` or `m[k]`, when it is one a
/// fact can be remembered about: a name, then fields, string keys and keys
/// held in names. A method call anywhere in it makes it no path at all.
fn path_of(e: &Expr) -> Option<String> {
    match e {
        Expr::Variable(v, ..) if v.2.is_empty() => Some(v.1.to_string()),
        Expr::Dot(x, ..) | Expr::Index(x, ..) => {
            let root = path_of(&x.lhs)?;
            path_rest(root, matches!(e, Expr::Index(..)), &x.rhs)
        }
        _ => None,
    }
}

fn path_rest(prefix: String, index: bool, rhs: &Expr) -> Option<String> {
    match rhs {
        Expr::Dot(x, ..) | Expr::Index(x, ..) => {
            let here = path_step(prefix, index, &x.lhs)?;
            path_rest(here, matches!(rhs, Expr::Index(..)), &x.rhs)
        }
        other => path_step(prefix, index, other),
    }
}

/// `prefix` extended by one step. A string key is the same path as the field
/// of that name, so `m["a"]` and `m.a` share their facts.
fn path_step(prefix: String, index: bool, what: &Expr) -> Option<String> {
    match (index, what) {
        (true, Expr::StringConstant(s, _)) => Some(format!("{prefix}.{s}")),
        (true, Expr::Variable(v, ..)) if v.2.is_empty() => Some(format!("{prefix}[{}]", v.1)),
        (false, Expr::Property(p, _)) => Some(format!("{prefix}.{}", p.2)),
        (false, Expr::Variable(v, ..)) => Some(format!("{prefix}.{}", v.1)),
        _ => None,
    }
}

/// Why a starting value says nothing about a name's type.
enum Blank {
    EmptyList,
    EmptyMap,
    Null,
    Host(String),
}

/// A starting value that says nothing about what a name will hold, looking
/// through `signal(…)`.
fn uninformative(e: &Expr) -> Option<Blank> {
    match e {
        Expr::FnCall(call, _) if call.name == "signal" && call.args.len() == 1 && call.namespace.is_empty() => {
            uninformative(&call.args[0])
        }
        Expr::Array(items, _) if items.is_empty() => Some(Blank::EmptyList),
        Expr::Map(x, _) if x.0.is_empty() => Some(Blank::EmptyMap),
        Expr::Unit(_) => Some(Blank::Null),
        Expr::Custom(c, _) if c.tokens.first().is_some_and(|t| t.as_str() == "null") => Some(Blank::Null),
        Expr::FnCall(call, _) if !call.namespace.is_empty() => Some(Blank::Host(call.name.to_string())),
        _ => None,
    }
}

/// A literal type as the name it would be bound to holds it: `"all"` is a
/// `string` once it is in a variable nobody annotated.
fn widen(ty: &Type) -> Type {
    match ty {
        Type::Literal(_) => Type::String,
        Type::Array(item) => Type::Array(Box::new(widen(item))),
        Type::Record(fields) => Type::Record(
            fields.iter().map(|f| Field { name: f.name.clone(), optional: f.optional, ty: widen(&f.ty) }).collect(),
        ),
        Type::Union(members) => Type::union(members.iter().map(widen)),
        other => other.clone(),
    }
}

/// The members of a union of string literals, quoted, or `None` if `ty` is
/// anything else.
fn literal_members(ty: &Type) -> Option<Vec<String>> {
    match ty {
        Type::Literal(s) => Some(vec![format!("{s:?}")]),
        Type::Union(members) => members
            .iter()
            .map(|m| match m {
                Type::Literal(s) => Some(format!("{s:?}")),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// `ty` without `null` in it.
fn without_null(ty: &Type) -> Type {
    match ty {
        Type::Union(members) => Type::union(members.iter().filter(|m| **m != Type::Null).cloned()),
        Type::Null => Type::Any,
        other => other.clone(),
    }
}

/// The declared names a type mentions.
fn named_in(ty: &Type, out: &mut Vec<String>) {
    match ty {
        Type::Named(n) => out.push(n.clone()),
        Type::Array(t) | Type::Dict(t) => named_in(t, out),
        Type::Record(fields) => fields.iter().for_each(|f| named_in(&f.ty, out)),
        Type::Union(members) => members.iter().for_each(|m| named_in(m, out)),
        Type::Function(params, result) => {
            params.iter().for_each(|p| named_in(p, out));
            named_in(result, out);
        }
        _ => {}
    }
}

/// The type of a constant the parser folded, which is rare with the optimizer
/// off.
fn dynamic_type(d: &Dynamic) -> Type {
    if d.is::<bool>() {
        Type::Bool
    } else if d.is::<rhai::INT>() || d.is::<rhai::FLOAT>() {
        Type::Number
    } else if d.is_string() {
        Type::String
    } else if d.is_unit() {
        Type::Null
    } else {
        Type::Any
    }
}

fn is_operator(name: &str) -> bool {
    matches!(
        name,
        "+" | "-" | "*" | "/" | "%" | "**" | "==" | "!=" | "<" | ">" | "<=" | ">=" | "===" | "!=="
            | "!" | "&" | "|" | "^" | "<<" | ">>" | ".." | "..="
    )
}

/// The known name closest to `name`, if one is close enough to be a typo.
fn near_miss<'n>(name: &str, known: &[&'n str]) -> Option<&'n str> {
    // A short name gets one edit, anything longer two: `titel` is `title`.
    let limit = if name.len() <= 3 { 1 } else { 2 };
    known
        .iter()
        .map(|k| (edit_distance(name, k), *k))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, _)| *d)
        .map(|(_, k)| k)
}

/// Edits between two names, where swapping two neighbouring letters is one.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Builder;

    const TASK: &str = "type Task = { id: int, title: string, done: bool, note?: string };\n";

    fn findings(src: &str) -> Vec<Finding> {
        findings_with(src, &Context::default())
    }

    fn findings_with(src: &str, cx: &Context) -> Vec<Finding> {
        let engine = Builder::new().build(src).unwrap_or_else(|e| panic!("{src}\n{e:?}"));
        engine.check_types(cx)
    }

    fn errors(src: &str) -> Vec<String> {
        findings(src).into_iter().filter(|f| f.is_error).map(|f| f.message).collect()
    }

    fn warnings(src: &str) -> Vec<String> {
        findings(src).into_iter().filter(|f| !f.is_error).map(|f| f.message).collect()
    }

    fn one_error(src: &str, part: &str) {
        let e = errors(src);
        assert!(e.iter().any(|m| m.contains(part)), "wanted an error with {part:?} in\n{src}\ngot {e:?}");
    }

    #[test]
    fn a_program_that_agrees_with_itself_says_nothing() {
        let src = format!(
            "{TASK}\
             type Filter = \"all\" | \"open\" | \"done\";\n\
             let tasks: Task[] = signal([{{ id: 1, title: \"a\", done: false }}]);\n\
             let filter: Filter = signal(\"all\");\n\
             let count: int = 0;\n\
             let n = signal(0);\n\
             fn visible(): Task[] {{ tasks.filter(t => !t.done) }}\n\
             fn label(t: Task, i: int): string {{ `${{i}}. ${{t.title}}` }}\n\
             fn add(title: string) {{ tasks = tasks + [{{ id: tasks.length, title: title, done: false }}]; }}\n\
             fn bump() {{ count = count + 1; n = n / 2; filter = \"open\"; }}\n\
             let first = label(tasks[0], 1);\n\
             let total = tasks.reduce((sum, t) => sum + t.id, 0);\n"
        );
        let f = findings(&src);
        assert!(f.is_empty(), "{f:#?}");
    }

    #[test]
    fn a_value_that_does_not_fit_its_annotation() {
        one_error("let n: int = \"x\";", "where `int` is expected");
        one_error("let n: int = 1.5;", "`number`, where `int` is expected");
        one_error(
            "let n: int = 3; fn f() { n = n / 2; }",
            "`n` holds `int`, so it cannot be given `number` here. If it may hold either, say so: `let n: number = …`",
        );
        one_error("let s: string = 1;", "where `string` is expected");
        one_error("let b: bool = \"yes\";", "where `bool` is expected");
        one_error("let xs: int[] = [1, \"a\"];", "where `int` is expected");
        assert!(errors("let n: int = 3; let m: int = -2; let x: number = n;").is_empty());
        // A plain number signal halves without complaint: literals are numbers.
        assert!(errors("let n = signal(0); fn f() { n = n / 2; }").is_empty());
    }

    #[test]
    fn a_map_literal_is_held_to_its_record() {
        let src = format!("{TASK}let t: Task = {{ id: 1, titel: \"x\", done: false }};");
        one_error(&src, "no field `titel`; did you mean `title`?");
        one_error(&src, "missing the field `title`");
        let ok = format!("{TASK}let t: Task = {{ id: 1, title: \"x\", done: false }};");
        assert!(errors(&ok).is_empty(), "{:?}", errors(&ok));
    }

    #[test]
    fn a_misspelt_field_is_named() {
        let src = format!("{TASK}let t: Task = {{ id: 1, title: \"x\", done: false }};\nfn f() {{ t.titel }}");
        one_error(&src, "`Task` has no field `titel`; did you mean `title`?");
    }

    #[test]
    fn a_closure_parameter_takes_its_type_from_where_it_is_used() {
        let src = format!("{TASK}let tasks: Task[] = [];\nfn f() {{ tasks.filter(t => t.titel) }}");
        one_error(&src, "no field `titel`");
        let src = format!("{TASK}let tasks: Task[] = [];\nlet ids: string[] = tasks.map(t => t.id);");
        one_error(&src, "where `string[]` is expected");
    }

    #[test]
    fn a_call_is_checked_against_its_parameters() {
        let src = format!("{TASK}fn label(t: Task): string {{ t.title }}\nfn f() {{ label(5) }}");
        one_error(&src, "`label` takes `Task` as `t`, and this is `number`");
        let src = format!("{TASK}let tasks: Task[] = [];\nfn f() {{ tasks.push(5); }}");
        one_error(&src, "where `Task` is expected");
    }

    #[test]
    fn a_result_is_checked_against_its_declaration() {
        one_error("fn f(): int { \"x\" }", "`f` is declared to return `int`, and this is `\"x\"`");
        one_error("fn f(n: number) { if n > 0 { f(n - 1) } else { 0 } }", "`f` calls itself");
        assert!(errors("fn f(n: number): number { if n > 0 { f(n - 1) } else { 0 } }").is_empty());
    }

    #[test]
    fn a_literal_union_takes_only_its_members() {
        let t = "type F = \"a\" | \"b\";\n";
        one_error(&format!("{t}let f: F = \"c\";"), "\"c\" is not `F`");
        one_error(&format!("{t}let f: F = \"bb\";"), "did you mean \"b\"?");
        one_error(
            &format!("{t}let f: F = \"a\"; fn g() {{ f = \"z\"; }}"),
            "\"z\" is not `F`, which is one of \"a\", \"b\"",
        );
        assert!(errors(&format!("{t}let f: F = \"a\"; fn g() {{ f = \"b\"; }}")).is_empty());
        // An unannotated string is a string, and a string is not an `F`.
        one_error(&format!("{t}let s = \"a\";\nlet f: F = s;"), "`string`, where `F` is expected");
    }

    #[test]
    fn a_dictionary_holds_one_type() {
        one_error("let m: { [string]: bool } = { a: true, b: 1 };", "where `bool` is expected");
        assert!(errors("let m: { [string]: bool } = { a: true, b: false };").is_empty());
    }

    #[test]
    fn a_name_that_is_no_type() {
        let src = format!("{TASK}let x: Tsk = 1;");
        one_error(&src, "there is no type `Tsk`; did you mean `Task`?");
        one_error("type task = { id: int };", "starts with a capital letter");
        one_error("let x: Nope = 1;", "Declare it with `type Nope");
    }

    #[test]
    fn what_cannot_be_inferred_is_warned_and_not_an_error() {
        let src = "let tasks = signal([]);\nlet user = signal(());\nlet m = signal({});\n\
                   fn f(x) { x }\nlet add = (a, b) => a + b;";
        let f = findings(src);
        assert!(f.iter().all(|f| !f.is_error), "{f:#?}");
        let w = warnings(src);
        assert!(w.iter().any(|m| m.contains("`tasks` starts as an empty list")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("`user` starts as `null`")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("`m` starts as `{}`")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("`x` has no type") && m.contains("fn f(x: T)")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("`a` has no type")), "{w:?}");
        // Annotated, each of them is quiet.
        let quiet = "type U = { name: string };\nlet tasks: string[] = signal([]);\n\
                     let user: U? = signal(());\nfn f(x: int) { x }\n\
                     let add = (a: number, b: number) => a + b;";
        assert!(findings(quiet).is_empty(), "{:#?}", findings(quiet));
    }

    #[test]
    fn an_any_turns_checking_off_and_says_nothing_more() {
        let src = "let tasks = signal([]);\nfn f() { tasks[0].whatever.at.all + 1 }";
        assert!(errors(src).is_empty(), "{:?}", errors(src));
    }

    #[test]
    fn host_functions_with_a_signature_are_typed() {
        let mut b = Builder::new();
        b.host_number("level", || 50.0);
        let engine = b.build("let a: string = host::level();").expect("builds");
        let f = engine.check_types(&Context::default());
        assert!(f.iter().any(|f| f.message.contains("`number`, where `string` is expected")), "{f:?}");
    }

    #[test]
    fn findings_past_the_own_lines_are_not_reported() {
        let cx = Context { own_lines: Some(1), ..Context::default() };
        let f = findings_with("let a: int = 1;\nlet b: int = \"x\";", &cx);
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn an_imported_type_is_known() {
        let cx = Context {
            imported_types: vec![("Task".into(), "{ id: int, title: string }".into())],
            ..Context::default()
        };
        let f = findings_with("let t: Task = { id: 1, titel: \"x\" };", &cx);
        assert!(f.iter().any(|f| f.message.contains("did you mean `title`")), "{f:?}");
    }

    const NOTE: &str = "type T = { id: int, note?: string };\nlet t: T = { id: 1 };\n";

    fn clean(src: &str) {
        let e = errors(src);
        assert!(e.is_empty(), "expected no errors in\n{src}\ngot {e:?}");
    }

    #[test]
    fn an_optional_field_is_read_with_a_question_mark() {
        one_error(&format!("{NOTE}fn f() {{ t.note }}"), "`note` may be absent from `T`, so read it as `t?.note`");
        clean(&format!("{NOTE}fn f(): string? {{ t?.note }}"));
        clean(&format!("{NOTE}fn f(): string {{ t?.note ?? \"none\" }}"));
    }

    #[test]
    fn a_checked_field_is_read_plainly_inside_the_check() {
        clean(&format!("{NOTE}fn f(): string {{ if t?.note != null {{ t.note }} else {{ \"\" }} }}"));
        clean(&format!("{NOTE}fn f(): string {{ if \"note\" in t {{ t.note }} else {{ \"\" }} }}"));
        clean(&format!("{NOTE}fn f(): string {{ if t?.note {{ t.note }} else {{ \"\" }} }}"));
        clean(&format!("{NOTE}fn f(): bool {{ t?.note != null && t.note.length > 0 }}"));
        clean(&format!("{NOTE}fn f(): string {{ if t?.note == null {{ return \"\"; }} t.note }}"));
        // Outside the region, the rule is back.
        one_error(&format!("{NOTE}fn f() {{ if t?.note != null {{ }} t.note }}"), "may be absent");
        one_error(&format!("{NOTE}fn f() {{ if t?.note == null {{ t.note }} }}"), "may be absent");
    }

    const LOAD: &str = "type Load =\n  | { state: \"idle\" }\n  | { state: \"done\", rows: int[] };\n\
                        let load: Load = { state: \"idle\" };\n";

    #[test]
    fn a_discriminant_narrows_its_union() {
        clean(&format!("{LOAD}fn f(): int {{ if load.state == \"done\" {{ load.rows.length }} else {{ 0 }} }}"));
        one_error(&format!("{LOAD}fn f() {{ load.rows }}"), "no field `rows`");
        clean(&format!(
            "{LOAD}fn f(): int {{ switch load.state {{ \"idle\" => 0, \"done\" => load.rows.length }} }}"
        ));
        clean(&format!("{LOAD}fn f(): int {{ if load.state != \"done\" {{ return 0; }} load.rows.length }}"));
    }

    #[test]
    fn a_switch_on_a_literal_union_handles_every_member() {
        let f = "type F = \"a\" | \"b\" | \"c\";\nlet f: F = \"a\";\n";
        one_error(&format!("{f}fn g() {{ switch f {{ \"a\" => 1, \"b\" => 2 }} }}"), "does not handle \"c\"");
        clean(&format!("{f}fn g() {{ switch f {{ \"a\" => 1, \"b\" => 2, \"c\" => 3 }} }}"));
        clean(&format!("{f}fn g() {{ switch f {{ \"a\" => 1, _ => 0 }} }}"));
    }

    #[test]
    fn a_fact_about_a_signal_dies_at_a_call_that_writes_it() {
        let reset = "fn reset() { load = { state: \"idle\" }; }\nfn later() { reset(); }\n";
        one_error(
            &format!("{LOAD}{reset}fn f() {{ if load.state == \"done\" {{ later(); load.rows }} }}"),
            "no field `rows`",
        );
        clean(&format!("{LOAD}{reset}fn f() {{ if load.state == \"done\" {{ print(1); load.rows }} }}"));
    }

    #[test]
    fn a_comparison_that_can_never_be_true() {
        let f = "type F = \"all\" | \"open\";\nlet f: F = \"all\";\n";
        one_error(&format!("{f}fn g() {{ f == \"al\" }}"), "`f` is `F`, which is one of \"all\", \"open\", so it is never \"al\"; did you mean \"all\"?");
        one_error("let n = 1;\nfn g() { n == \"1\" }", "compares `number` with `\"1\"`");
        clean(&format!("{f}fn g() {{ f == \"open\" || f != null }}"));
    }

    #[test]
    fn a_dictionary_key_may_be_missing() {
        let m = "let m: { [string]: bool } = { a: true };\n";
        one_error(&format!("{m}fn f(k: string) {{ m[k] }}"), "read it as `m?[k]`");
        one_error(&format!("{m}fn f() {{ m.a }}"), "read it as `m?.a`");
        clean(&format!("{m}fn f(k: string): bool? {{ m?[k] }}"));
        clean(&format!("{m}fn f(k: string): bool {{ if k in m {{ m[k] }} else {{ false }} }}"));
        clean(&format!("{m}fn f(): bool {{ if \"a\" in m {{ m.a }} else {{ false }} }}"));
        clean(&format!("{m}fn f() {{ for k in keys(m) {{ print(m[k]); }} }}"));
    }

    #[test]
    fn type_of_narrows() {
        let v = "let v: string | { n: int } = \"x\";\n";
        clean(&format!("{v}fn f(): int {{ if type_of(v) == \"map\" {{ v.n }} else {{ 0 }} }}"));
        one_error(&format!("{v}fn f() {{ v.n }}"), "no property `n`");
    }

    #[test]
    fn a_typed_function_cannot_read_its_callers_locals() {
        let cx = Context {
            caller_names: ["item".to_string()].into_iter().collect(),
            ..Default::default()
        };
        let errors_in = |src: &str| -> Vec<String> {
            findings_with(src, &cx).into_iter().filter(|f| f.is_error).map(|f| f.message).collect()
        };
        let head = "let picked = signal(\"\");\n";
        // No parameters counts as typed.
        let e = errors_in(&format!("{head}fn pick() {{ picked = item; }}"));
        assert!(e.iter().any(|m| m.contains("`pick` reads `item`")), "{e:?}");
        let e = errors_in(&format!("{head}fn pick(x: string) {{ picked = x + item; }}"));
        assert!(e.iter().any(|m| m.contains("`pick` reads `item`")), "{e:?}");
        // Another function's `let` is a caller's local too, with no template
        // involved.
        let e = errors(&format!(
            "{head}fn outer() {{ let helper = \"h\"; inner(); }}\nfn inner() {{ picked = helper; }}"
        ));
        assert!(e.iter().any(|m| m.contains("`inner` reads `helper`")), "{e:?}");
        // An untyped one keeps the fork's behaviour.
        let e = errors_in(&format!("{head}fn pick(x) {{ picked = x + item; }}"));
        assert!(!e.iter().any(|m| m.contains("reads `item`")), "{e:?}");
        // Its own names, the document's, and an arrow's parameters are its to read.
        let e = errors_in(&format!(
            "{head}fn pick(item: string) {{ picked = item; }}\n\
             fn first(): string {{ let item = \"a\"; [item].map(x => x + picked)[0] }}"
        ));
        assert!(e.is_empty(), "{e:?}");
    }

    /// What the template pieces in `template` produce against `script`.
    fn template_findings(script: &str, template: Vec<Tpl>) -> Vec<Finding> {
        let cx = Context { template, ..Default::default() };
        findings_with(script, &cx).into_iter().filter(|f| f.template).collect()
    }

    fn ty(text: &str) -> Type {
        parse_type(text).unwrap()
    }

    fn expr(src: &str, want: Option<&str>) -> Tpl {
        Tpl::Expr { src: src.into(), want: want.map(ty), line: 7, what: "`x`".into() }
    }

    #[test]
    fn a_loop_variable_is_an_element_of_its_list() {
        let script = format!("{TASK}let tasks: Task[] = signal([]);");
        let body = vec![expr("t.titel", None)];
        let f = template_findings(&script, vec![Tpl::For { var: "t".into(), src: "tasks".into(), line: 5, body }]);
        assert!(f.iter().any(|f| f.message.contains("titel") && f.is_error), "{f:?}");
        let body = vec![expr("t.title", Some("string"))];
        let f = template_findings(&script, vec![Tpl::For { var: "t".into(), src: "tasks".into(), line: 5, body }]);
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn a_binding_is_checked_against_what_its_attribute_takes() {
        let f = template_findings("let count = signal(0);", vec![expr("count", Some("bool"))]);
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].message.starts_with("`x`: "), "says which attribute: {f:?}");
        assert_eq!(f[0].line, Some(7), "on the template's line");
        let class = "string | string[] | { [string]: bool }";
        let script = format!("{TASK}let t: Task = {{ id: 1, title: \"a\", done: false }};");
        let f = template_findings(&script, vec![expr("{ active: t }", Some(class))]);
        assert!(f.iter().any(|f| f.is_error), "a map of classes takes bools: {f:?}");
        let f = template_findings(&script, vec![expr("{ active: t.done }", Some(class))]);
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn an_r_if_narrows_its_subtree() {
        let script = format!("{TASK}let t: Task = signal({{ id: 1, title: \"a\", done: false }});");
        let f = template_findings(&script, vec![expr("t.note", None)]);
        assert!(f.iter().any(|f| f.is_error), "outside a check, `note` may be absent: {f:?}");
        let read = |line: usize| Tpl::Expr { src: "t.note".into(), want: None, line, what: "`x`".into() };
        let chain = |first: &str| Tpl::If {
            branches: vec![(Some(first.to_string()), 3, vec![read(3)]), (None, 4, vec![read(4)])],
        };
        let f = template_findings(&script, vec![chain("t?.note != null")]);
        assert_eq!(f.len(), 1, "the r-if branch is clean, the r-else is not: {f:?}");
        assert_eq!(f[0].line, Some(4));
        let f = template_findings(&script, vec![chain("t?.note == null")]);
        assert_eq!(f.len(), 1, "and the other way round: {f:?}");
        assert_eq!(f[0].line, Some(3));
    }

    #[test]
    fn r_model_names_something_of_the_field_s_value_type() {
        let model = |src: &str, value: &str| Tpl::Model {
            src: src.into(),
            writes: ty(value),
            shows: ty(value),
            line: 2,
            what: "`r-model`".into(),
        };
        let script = "let n = signal(0);\nlet on = signal(false);\nlet name = signal(\"\");";
        let f = template_findings(script, vec![model("n", "string")]);
        assert!(f.iter().any(|f| f.message.contains("writes `string` into `n`")), "{f:?}");
        let f = template_findings(script, vec![model("on", "bool"), model("name", "string"), model("n", "number")]);
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn a_handler_s_event_has_its_event_s_type() {
        let swipe = ty("{ direction: \"left\" | \"right\" | \"up\" | \"down\" }");
        let handler = |src: &str| Tpl::Handler { src: src.into(), event: swipe.clone(), line: 9, what: "`@swipe`".into() };
        let script = "let n = signal(0);";
        let f = template_findings(script, vec![handler("if event.direction == \"lefty\" { n = 1; }")]);
        assert!(f.iter().any(|f| f.message.contains("did you mean \"left\"")), "{f:?}");
        let f = template_findings(script, vec![handler("if event.direction == \"left\" { n = 1; }")]);
        assert!(f.is_empty(), "{f:?}");
        let f = template_findings(script, vec![handler("n = \"x\"")]);
        assert!(f.iter().any(|f| f.is_error), "a handler's write is checked too: {f:?}");
    }

    #[test]
    fn a_plain_attribute_passes_its_text() {
        let given = |text: &str| Tpl::Given {
            given: Type::Literal(text.into()),
            want: ty("\"primary\" | \"quiet\""),
            line: 4,
            what: "`kind` on <btn>".into(),
        };
        let f = template_findings("", vec![given("big")]);
        assert!(f.iter().any(|f| f.is_error && f.message.contains("`kind` on <btn>")), "{f:?}");
        assert!(template_findings("", vec![given("quiet")]).is_empty());
    }

    #[test]
    fn findings_carry_their_line() {
        let f = findings("let a = 1;\n\nlet n: int = \"x\";");
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].line, Some(3));
    }
}
