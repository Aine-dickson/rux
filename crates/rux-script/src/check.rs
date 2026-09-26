//! The type checker: reads a script as Rux's parser gives it, annotations
//! and all, and reports what does not fit. `docs/10-types.md` is the design,
//! and `docs/11-next.md` step 3 the plan this belongs to.
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

use std::collections::{HashMap, HashSet};

use rux_syntax::ast::*;
use rux_syntax::print;
use rux_syntax::visit::{walk_stmts, Node};
use rux_syntax::{LineIndex, Options, Span};

use crate::types::{parse_decl, parse_type, Field, Type};

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

/// What the checker worked out a name, a field or a function to be, for an
/// editor's hover and completion. See `docs/10-types.md`, "Editor".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    /// A line of the script, or of the file when [`Seen::template`] is set.
    pub line: usize,
    /// 1-based column within the line. `None` in a template piece, whose
    /// columns count from the piece and not from the file, and for what is
    /// placed at a function's line rather than where it is written.
    pub column: Option<usize>,
    pub template: bool,
    /// `value` for a name read, `field` for a property, `let` where a name is
    /// declared, `param` for a parameter, `fn` for a function.
    pub kind: &'static str,
    /// As written: `note` for `t.note`.
    pub name: String,
    /// The whole chain it ends, `t.note`, when it is one a condition could
    /// talk about. What completion after `t.` looks up.
    pub path: Option<String>,
    /// The type as written (`Task`), or a function's signature.
    pub ty: String,
    /// The fields a `.` after it may read: name, type, and whether the field
    /// may be absent.
    pub fields: Vec<(String, String, bool)>,
    /// Whether the value may be `null`, so a field is read with `?.`.
    pub nullable: bool,
}

/// The type an unannotated parameter was handed at every call the checker
/// saw, for the editor's quick-fix. Never used to check anything: decision 11
/// in `docs/10-types.md` is that a parameter is not inferred from its calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Guess {
    /// The script line the function is written on.
    pub line: usize,
    pub function: String,
    pub param: String,
    pub ty: String,
}

/// Everything [`check_recording`] saw.
#[derive(Clone, Debug, Default)]
pub struct Table {
    pub seen: Vec<Seen>,
    pub guesses: Vec<Guess>,
}

/// What the checker settled about each expression of one parse, for the
/// typed IR (step 4 of `docs/11-next.md`). Keyed by [`ExprId`]'s number,
/// except [`Types::decl`], which is keyed by where the name is written.
#[derive(Clone, Debug, Default)]
pub struct Types {
    /// The type each expression has where it stands, narrowing applied: `t`
    /// inside `if t != none` is not `none`. The first answer given outside a
    /// trial is the one kept; a trial (a union member tried quietly) keeps
    /// nothing.
    pub of: HashMap<u32, Type>,
    /// Where an expression was checked against a type it had to fit, that
    /// type. Where the two differ, a conversion happens: an `int` widened to a
    /// `float`, an `any` checked on its way into typed code.
    pub want: HashMap<u32, Type>,
    /// The type a declared name is bound to (a `let`, a parameter, a loop
    /// variable, a closure's parameter), by the start of its name's span.
    pub decl: HashMap<u32, Type>,
}

/// A template piece as the checker read it.
#[derive(Clone, Debug)]
pub struct Piece {
    pub src: String,
    /// The file line it is written on.
    pub line: usize,
    /// What it is, as the checker names it in a finding: `:disabled on <button>`.
    pub what: String,
    pub script: Script,
    pub types: Types,
}

/// Everything the checker settled, for the typed IR: [`check_typed`].
#[derive(Clone, Debug, Default)]
pub struct Record {
    /// The script's own expressions.
    pub script: Types,
    /// Every template piece, in the order the checker reached them.
    pub pieces: Vec<Piece>,
    /// Each function's parameters and result as checked, by name and arity.
    /// A result that was not written is the one inferred from the body.
    pub fns: HashMap<(String, usize), (Vec<Type>, Type)>,
    /// Every type the script can name, declared, imported or built in, with
    /// its type parameters.
    pub types: HashMap<String, (Vec<String>, Type)>,
}

/// [`check`], also keeping what it settled about every expression. What the
/// typed IR is built from.
pub fn check_typed(script: &Script, src: &str, cx: &Context) -> (Vec<Finding>, Record) {
    let mut checker = Checker::new(script, src, cx);
    checker.typed = true;
    checker.run();
    let mut record = std::mem::take(&mut checker.rec);
    record.script = std::mem::take(&mut checker.cur);
    for (key, info) in &checker.fns {
        let params = info.params.iter().map(|(_, t)| t.clone().unwrap_or(Type::Any)).collect();
        let result = match (&info.result, &info.state) {
            (Some(t), _) | (None, FnState::Done(t)) => t.clone(),
            _ => Type::Any,
        };
        record.fns.insert(key.clone(), (params, result));
    }
    for (name, ty) in &checker.types {
        let params = checker.type_params.get(name).cloned().unwrap_or_default();
        record.types.insert(name.clone(), (params, ty.clone()));
    }
    let mut findings = checker.findings;
    findings.sort_by_key(|f| f.line);
    findings.dedup();
    (findings, record)
}

/// Check `script`, whose text is `src`, against its own annotations and the
/// [`Context`].
pub fn check(script: &Script, src: &str, cx: &Context) -> Vec<Finding> {
    check_recording(script, src, cx, false).0
}

/// [`check`], and when `record` is set, what it worked out along the way. An
/// ordinary load does not record: nobody is there to read it.
pub fn check_recording(script: &Script, src: &str, cx: &Context, record: bool) -> (Vec<Finding>, Table) {
    let mut checker = Checker::new(script, src, cx);
    checker.record = record;
    checker.run();
    let table = if record { checker.table() } else { Table::default() };
    let mut findings = checker.findings;
    findings.sort_by_key(|f| f.line);
    findings.dedup();
    (findings, table)
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
struct FnInfo<'a> {
    def: &'a FnDecl,
    /// `fn first<T>`: its type parameters, which its annotations read as
    /// [`Type::Param`]s.
    tparams: Vec<String>,
    /// Each parameter's declared type, `None` where it has none.
    params: Vec<(String, Option<Type>)>,
    /// `): T`, when written.
    result: Option<Type>,
    state: FnState,
}

/// Where something is, for a report: a span of the source being checked, or
/// [`NOWHERE`] for something with no place of its own.
type Pos = Span;

/// A report with no line.
const NOWHERE: Pos = Span { start: u32::MAX, end: u32::MAX };

/// The text being checked, the script or a template piece, and its lines.
struct Source {
    text: String,
    lines: LineIndex,
}

impl Source {
    fn new(text: &str) -> Self {
        Source { text: text.to_string(), lines: LineIndex::new(text) }
    }
}

struct Checker<'a> {
    script: &'a Script,
    cx: &'a Context,
    /// The text spans point into: the script, or the template piece being
    /// checked.
    source: Source,
    /// While a template piece is checked: the file line it starts on, and what
    /// it is (`:disabled` on <button>), which prefixes what is said about it.
    in_template: Option<(usize, String)>,
    /// Declared and imported types, by name.
    types: HashMap<String, Type>,
    /// The type parameters of each generic one of [`Checker::types`], whose
    /// body reads them as [`Type::Param`]s.
    type_params: HashMap<String, Vec<String>>,
    /// The type parameters of the function whose body is being checked.
    tparams: Vec<String>,
    /// Types that resolve here without being nameable here, with the `use`
    /// path that would make them so.
    hidden: HashMap<String, String>,
    fns: HashMap<(String, usize), FnInfo<'a>>,
    /// The top-level `let`s, which are the document's signals.
    globals: HashMap<String, Type>,
    /// Local scopes, innermost last.
    scopes: Vec<HashMap<String, Type>>,
    /// What conditions have established, frame by frame. See `fact`.
    facts: Vec<HashMap<String, Type>>,
    /// The names each function writes, itself or through what it calls.
    writes: HashMap<(String, usize), HashSet<String>>,
    /// What each `return` in the function being checked hands back.
    returns: Vec<Vec<Type>>,
    /// The named function whose body is being checked, and whether it is
    /// typed: what a read of a caller's local is reported against.
    in_fn: Vec<(String, bool)>,
    /// Every name declared anywhere in the script: a `let`, a parameter or a
    /// loop variable. One a function cannot see itself is some caller's.
    script_names: HashSet<String>,
    /// The declared result of each function being checked, innermost last,
    /// with the function's name; `None` for one whose result is inferred.
    results: Vec<Option<(String, Type)>>,
    findings: Vec<Finding>,
    /// While positive, findings are counted rather than kept: used to try a
    /// value against each member of a union.
    quiet: usize,
    quiet_errors: usize,
    /// Whether to keep what is seen, for [`check_recording`].
    record: bool,
    seen: Vec<Seen>,
    /// What each unannotated parameter was handed, by function and position.
    given: HashMap<(String, usize), Vec<(usize, Type)>>,
    /// Whether to keep what each expression is, for [`check_typed`].
    typed: bool,
    /// What is kept, for the parse being checked: the script's, or the
    /// template piece's while one is.
    cur: Types,
    rec: Record,
    /// While positive, nothing is kept: a text is being read again for what
    /// it establishes, outside the piece it belongs to.
    rec_off: usize,
}

/// The id of an expression the checker makes up itself, as `x++` is checked
/// as `x += 1`: nothing is kept about one.
const SYNTHETIC: u32 = u32::MAX;

/// How deep a type is unfolded before giving up, so a recursive type such as
/// `type Tree = { kids: Tree[] }` cannot send assignability round forever.
const MAX_DEPTH: usize = 24;

impl<'a> Checker<'a> {
    fn new(script: &'a Script, src: &str, cx: &'a Context) -> Self {
        Checker {
            script,
            cx,
            source: Source::new(src),
            in_template: None,
            // `Result` is declared everywhere.
            types: HashMap::from([("Result".to_string(), crate::types::result_decl().1)]),
            type_params: HashMap::from([("Result".to_string(), crate::types::result_decl().0)]),
            tparams: Vec::new(),
            hidden: HashMap::new(),
            fns: HashMap::new(),
            globals: HashMap::new(),
            scopes: Vec::new(),
            facts: vec![HashMap::new()],
            writes: HashMap::new(),
            returns: Vec::new(),
            results: Vec::new(),
            in_fn: Vec::new(),
            script_names: declared_names(&script.stmts),
            findings: Vec::new(),
            quiet: 0,
            quiet_errors: 0,
            record: false,
            seen: Vec::new(),
            given: HashMap::new(),
            typed: false,
            cur: Types::default(),
            rec: Record::default(),
            rec_off: 0,
        }
    }

    // ----- Places ----------------------------------------------------------

    /// The 1-based line of `pos` in the text being checked.
    fn line_of(&self, pos: Pos) -> Option<usize> {
        if pos == NOWHERE || pos.start as usize > self.source.text.len() {
            return None;
        }
        Some(self.source.lines.line_col(&self.source.text, pos.start as usize).0)
    }

    fn column_of(&self, pos: Pos) -> Option<usize> {
        if pos == NOWHERE || pos.start as usize > self.source.text.len() {
            return None;
        }
        Some(self.source.lines.line_col(&self.source.text, pos.start as usize).1)
    }

    /// Where a report about `e` goes. An operator is placed at the operator,
    /// and a chain at its first step, which is where an author reads it
    /// starting when it runs over several lines.
    fn at(&self, e: &Expr) -> Pos {
        match &e.kind {
            ExprKind::Field { .. } | ExprKind::Method { .. } | ExprKind::Index { .. } => {
                let mut first = e;
                while let Some(base) = step_base(first).filter(|b| is_step(b)) {
                    first = base;
                }
                step_at(first)
            }
            ExprKind::Binary { op, lhs, rhs } => self.op_at(lhs.span, rhs.span, op),
            ExprKind::Is { expr, ty } => self.op_at(expr.span, ty.span, "is"),
            ExprKind::Call { callee, .. } => callee.first().map_or(e.span, |c| c.span),
            _ => e.span,
        }
    }

    /// Where `op` is written between the spans `before` and `after`.
    fn op_at(&self, before: Span, after: Span, op: &str) -> Pos {
        let (from, to) = (before.end as usize, after.start as usize);
        match self.source.text.get(from..to).and_then(|gap| gap.find(op)) {
            Some(i) => Span::new(from + i, from + i + op.len()),
            None => Span::at(from.min(self.source.text.len())),
        }
    }

    // ----- Recording, for the editor --------------------------------------

    /// Keep that `name` at `pos` is `ty`. `path` is the chain it ends.
    /// Whether it was kept.
    fn saw(&mut self, pos: Pos, kind: &'static str, name: &str, path: Option<&str>, ty: &Type) -> bool {
        let Some(l) = self.line_of(pos) else { return false };
        let column = self.column_of(pos);
        self.keep(l, column, kind, name, path, ty)
    }

    fn keep(
        &mut self,
        l: usize,
        column: Option<usize>,
        kind: &'static str,
        name: &str,
        path: Option<&str>,
        ty: &Type,
    ) -> bool {
        if !self.record || self.quiet > 0 {
            return false;
        }
        let (line, column, template) = match &self.in_template {
            Some((start, _)) => (start + l - 1, None, true),
            None => {
                if self.cx.own_lines.is_some_and(|own| l > own) {
                    return false;
                }
                (l, column, false)
            }
        };
        let resolved = self.resolve(ty);
        let nullable = !matches!(resolved, Type::Any) && may_be_null(&resolved);
        let fields = self
            .fields_of(&self.resolve(&without_null(&resolved)))
            .into_iter()
            .map(|f| (f.name, f.ty.to_string(), f.optional))
            .collect();
        self.seen.push(Seen {
            line,
            column,
            template,
            kind,
            name: name.to_string(),
            path: path.map(str::to_string),
            ty: ty.to_string(),
            fields,
            nullable,
        });
        true
    }

    /// The fields a `.` may read on a value of `ty`, already resolved: a
    /// record's own, or those every member of a union of records has.
    fn fields_of(&self, ty: &Type) -> Vec<Field> {
        match ty {
            Type::Record(fields) => fields.clone(),
            Type::Union(members) => {
                let records: Vec<Vec<Field>> = members
                    .iter()
                    .map(|m| match self.resolve(m) {
                        Type::Record(f) => Some(f),
                        _ => None,
                    })
                    .collect::<Option<_>>()
                    .unwrap_or_default();
                let Some((first, rest)) = records.split_first() else { return Vec::new() };
                first
                    .iter()
                    .filter_map(|f| {
                        let mut tys = vec![f.ty.clone()];
                        let mut optional = f.optional;
                        for r in rest {
                            let other = r.iter().find(|o| o.name == f.name)?;
                            tys.push(other.ty.clone());
                            optional |= other.optional;
                        }
                        Some(Field { name: f.name.clone(), optional, ty: Type::union(tys) })
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// `fn label(t: Task, i: int): string`, as far as it is known.
    fn signature(&self, name: &str, arity: usize) -> Option<String> {
        let info = self.fns.get(&(name.to_string(), arity))?;
        let params: Vec<String> = info
            .params
            .iter()
            .map(|(p, t)| match t {
                Some(t) => format!("{p}: {t}"),
                None => p.clone(),
            })
            .collect();
        let result = match (&info.result, &info.state) {
            (Some(t), _) | (None, FnState::Done(t)) => format!(": {t}"),
            _ => String::new(),
        };
        let tparams = if info.tparams.is_empty() { String::new() } else { format!("<{}>", info.tparams.join(", ")) };
        Some(format!("fn {name}{tparams}({}){result}", params.join(", ")))
    }

    /// What was kept, with every function and parameter placed at the line
    /// its function is written on, and each unannotated parameter's guess.
    fn table(&mut self) -> Table {
        let mut guesses = Vec::new();
        let mut named: Vec<(String, usize)> = self.fns.keys().cloned().collect();
        named.sort();
        for key in named {
            let info = self.fns[&key].clone();
            let Some(line) = self.line_of(info.def.body.span) else { continue };
            if self.cx.own_lines.is_some_and(|own| line > own) {
                continue;
            }
            // Placed on the line, not at a column, as they were when the
            // fork kept no position for a function's name or its parameters.
            if let Some(sig) = self.signature(&key.0, key.1) {
                if self.keep(line, None, "fn", &key.0, None, &Type::Any) {
                    let last = self.seen.last_mut().unwrap();
                    last.ty = sig;
                    last.fields.clear();
                    last.nullable = false;
                }
            }
            for (i, (param, ty)) in info.params.iter().enumerate() {
                self.keep(line, None, "param", param, Some(param), ty.as_ref().unwrap_or(&Type::Any));
                if ty.is_some() {
                    continue;
                }
                let handed: Vec<Type> = self
                    .given
                    .get(&key)
                    .map(|g| g.iter().filter(|(n, _)| *n == i).map(|(_, t)| t.clone()).collect())
                    .unwrap_or_default();
                if handed.is_empty() || handed.iter().any(|t| self.resolve(t) == Type::Any) {
                    continue;
                }
                guesses.push(Guess {
                    line,
                    function: key.0.clone(),
                    param: param.clone(),
                    ty: widen(&Type::union(handed)).to_string(),
                });
            }
        }
        let mut seen = std::mem::take(&mut self.seen);
        // The same expression is inferred more than once on some paths; the
        // first answer is the one given where it stands.
        let mut kept = HashSet::new();
        seen.retain(|s| kept.insert((s.template, s.line, s.column, s.kind, s.name.clone(), s.path.clone())));
        Table { seen, guesses }
    }

    // ----- Reporting -------------------------------------------------------

    fn report(&mut self, pos: Pos, message: String, is_error: bool) {
        if self.quiet > 0 {
            if is_error {
                self.quiet_errors += 1;
            }
            return;
        }
        let line = self.line_of(pos);
        if let Some((start, what)) = &self.in_template {
            let line = Some(start + line.unwrap_or(1) - 1);
            let message = format!("{what}: {message}");
            self.findings.push(Finding { message, line, is_error, template: true });
            return;
        }
        if let (Some(line), Some(own)) = (line, self.cx.own_lines) {
            if line > own {
                return;
            }
        }
        self.findings.push(Finding { message, line, is_error, template: false });
    }

    fn error(&mut self, pos: Pos, message: String) {
        self.report(pos, message, true);
    }

    fn warn(&mut self, pos: Pos, message: String) {
        self.report(pos, message, false);
    }

    // ----- Setup -----------------------------------------------------------

    fn run(&mut self) {
        let script = self.script;

        // Types first, since everything else names them.
        for (name, text, path) in &self.cx.support_types {
            if let Ok((params, ty)) = parse_decl(text) {
                self.types.insert(name.clone(), ty);
                self.type_params.insert(name.clone(), params);
                self.hidden.insert(name.clone(), path.clone());
            }
        }
        for (name, text) in &self.cx.imported_types {
            self.hidden.remove(name);
            if let Ok((params, ty)) = parse_decl(text) {
                self.types.insert(name.clone(), ty);
                self.type_params.insert(name.clone(), params);
            }
        }
        for stmt in &script.stmts {
            let StmtKind::Type { name, params, ty } = &stmt.kind else { continue };
            self.old_spellings_in(ty);
            let params: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
            match type_of_expr(ty).map(|t| t.with_params(&params)) {
                Ok(t) => {
                    let n = name.name.as_str();
                    if self.types.contains_key(n)
                        && !self.hidden.contains_key(n)
                        && !self.cx.imported_types.iter().any(|(i, _)| i == n)
                    {
                        self.error(name.span, format!("the type `{n}` is declared twice"));
                    }
                    if !n.starts_with(|c: char| c.is_uppercase()) {
                        self.error(
                            name.span,
                            format!(
                                "a declared type's name starts with a capital letter: `type {}{}`",
                                n[..1].to_uppercase(),
                                &n[1..]
                            ),
                        );
                    }
                    self.hidden.remove(n);
                    self.types.insert(n.to_string(), t);
                    self.type_params.insert(n.to_string(), params);
                }
                Err(e) => {
                    let text = ty.span.text(&self.source.text).to_string();
                    self.error(ty.span, format!("`{text}` is not a type: {e}"));
                }
            }
        }

        // A generic type's uses of other types, which only its body says.
        for stmt in &script.stmts {
            let StmtKind::Type { name, params, ty } = &stmt.kind else { continue };
            if params.is_empty() {
                continue;
            }
            if let Some(body) = self.types.get(&name.name).cloned() {
                self.check_names_exist(&body, ty.span);
            }
        }

        // Every annotation, with its names checked against the types that
        // exist. A generic function's own type parameters are types inside it.
        for stmt in &script.stmts {
            let mut written: Vec<&TypeExpr> = Vec::new();
            annotations_in(std::slice::from_ref(stmt), &mut written);
            let tparams = match &stmt.kind {
                StmtKind::Fn(def) => def.type_params.iter().map(|p| p.name.clone()).collect(),
                _ => Vec::new(),
            };
            for ty in written {
                self.old_spellings_in(ty);
                match type_of_expr(ty) {
                    Ok(t) => self.check_names_exist(&t.with_params(&tparams), ty.span),
                    Err(e) => {
                        let text = ty.span.text(&self.source.text).to_string();
                        self.error(ty.span, format!("`{text}` is not a type: {e}"));
                    }
                }
            }
        }
        for (name, ty) in &self.cx.provided {
            self.globals.insert(name.clone(), ty.clone());
        }

        for stmt in &script.stmts {
            let StmtKind::Fn(def) = &stmt.kind else { continue };
            let tparams: Vec<String> = def.type_params.iter().map(|p| p.name.clone()).collect();
            let read = |t: &TypeExpr| type_of_expr(t).ok().map(|t| t.with_params(&tparams));
            let params = def.params.iter().map(|p| (p.name.name.clone(), p.ty.as_ref().and_then(read))).collect();
            let result = def.result.as_ref().and_then(read);
            let key = (def.name.name.clone(), def.params.len());
            self.fns.insert(key, FnInfo { def, tparams, params, result, state: FnState::Unchecked });
        }

        // The top level, in order: its `let`s are the signals every function
        // reads.
        for stmt in &script.stmts {
            self.check_stmt(stmt);
        }

        // Then every function nothing has reached yet.
        let mut named: Vec<(String, usize)> = self.fns.keys().cloned().collect();
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

    /// Run `f` with `src` as the text spans point into, keeping nothing of
    /// what it finds: the text has been read as a piece already.
    fn in_source<T>(&mut self, src: &str, f: impl FnOnce(&mut Self) -> T) -> T {
        self.rec_off += 1;
        let out = self.with_source(src, f);
        self.rec_off -= 1;
        out
    }

    fn with_source<T>(&mut self, src: &str, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = std::mem::replace(&mut self.source, Source::new(src));
        let out = f(self);
        self.source = saved;
        out
    }

    /// Run `f` as the template piece `what`, written on file line `line`.
    fn in_piece<T>(&mut self, src: &str, line: usize, what: &str, f: impl FnOnce(&mut Self) -> T) -> T {
        self.in_piece_of(src, None, line, what, f)
    }

    /// [`Checker::in_piece`] for the piece `script` parsed from `src`, which is
    /// kept for the typed IR with what was settled about it.
    fn in_piece_of<T>(
        &mut self,
        src: &str,
        script: Option<&Script>,
        line: usize,
        what: &str,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let saved = self.in_template.replace((line, what.to_string()));
        let outer = std::mem::take(&mut self.cur);
        let out = self.with_source(src, f);
        let types = std::mem::replace(&mut self.cur, outer);
        if let (true, Some(script)) = (self.typed, script) {
            self.rec.pieces.push(Piece {
                src: src.to_string(),
                line,
                what: what.to_string(),
                script: script.clone(),
                types,
            });
        }
        self.in_template = saved;
        out
    }

    // ----- Keeping types, for the IR ---------------------------------------

    fn keeping(&self) -> bool {
        self.typed && self.quiet == 0 && self.rec_off == 0
    }

    /// Keep that `e` is `ty` where it stands, unless it was said already.
    fn note(&mut self, e: &Expr, ty: &Type) {
        if self.keeping() && e.id.0 != SYNTHETIC {
            self.cur.of.entry(e.id.0).or_insert_with(|| ty.clone());
        }
    }

    /// Keep that `e` had to fit `want`.
    fn note_want(&mut self, e: &Expr, want: &Type) {
        if self.keeping() && e.id.0 != SYNTHETIC {
            self.cur.want.entry(e.id.0).or_insert_with(|| want.clone());
        }
    }

    /// Keep that the name written at `at` is bound to `ty`.
    fn note_decl(&mut self, at: Span, ty: &Type) {
        if self.keeping() {
            self.cur.decl.entry(at.start).or_insert_with(|| ty.clone());
        }
    }

    /// A piece of the template, parsed. `None` when it does not parse, which
    /// the runtime reports in its own words.
    fn parse_piece(src: &str) -> Option<Script> {
        rux_syntax::parse(src, Options::default()).ok()
    }

    /// The one expression a piece is, when it is one.
    fn piece_expr(script: &Script) -> Option<&Expr> {
        match &script.stmts[..] {
            [Stmt { kind: StmtKind::Expr(e), .. }] => Some(e),
            _ => None,
        }
    }

    fn template_item(&mut self, item: &Tpl) {
        match item {
            Tpl::Expr { src, want, line, what } => {
                let Some(piece) = Self::parse_piece(src) else { return };
                self.in_piece_of(src, Some(&piece), *line, what, |c| match (Self::piece_expr(&piece), want) {
                    (Some(e), Some(want)) => c.check_expr(e, want),
                    (Some(e), None) => {
                        c.infer(e);
                    }
                    (None, _) => {
                        c.with_scope(|c| c.check_statements(&piece.stmts));
                    }
                });
            }
            Tpl::Handler { src, event, line, what } => {
                let Some(piece) = Self::parse_piece(src) else { return };
                self.in_piece_of(src, Some(&piece), *line, what, |c| {
                    c.with_scope(|c| {
                        c.bind("event", event.clone());
                        c.check_statements(&piece.stmts);
                    })
                });
            }
            Tpl::Model { src, writes, shows, line, what } => {
                let Some(piece) = Self::parse_piece(src) else { return };
                let Some(e) = Self::piece_expr(&piece) else { return };
                self.in_piece_of(src, Some(&piece), *line, what, |c| {
                    let held = c.infer(e);
                    if c.resolve(&held) == Type::Any {
                        return;
                    }
                    let name = src.trim();
                    // A number field bound to an `int` writes what was typed
                    // cut to a whole number.
                    let int_field = *writes == Type::Float && c.resolve(&held) == Type::Int;
                    if !int_field && !c.assignable(writes, &held) {
                        let message = format!(
                            "the field writes {} into `{name}`, which holds {}",
                            c.show(writes),
                            c.show(&held)
                        );
                        c.error(c.at(e), message);
                    } else if !c.assignable(&held, shows) {
                        let message = format!(
                            "the field shows {}, and `{name}` holds {}",
                            c.show(shows),
                            c.show(&held)
                        );
                        c.error(c.at(e), message);
                    }
                });
            }
            Tpl::Given { given, want, line, what } => {
                self.in_piece("", *line, what, |c| {
                    if !c.assignable(given, want) {
                        c.mismatch_ty(NOWHERE, given, want);
                    }
                });
            }
            Tpl::For { var, src, line, body } => {
                let Some(piece) = Self::parse_piece(src) else { return };
                let Some(e) = Self::piece_expr(&piece) else { return };
                let element = self.in_piece_of(src, Some(&piece), *line, "`r-for`", |c| {
                    let ty = c.infer(e);
                    c.element_of(e, &ty)
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
                    let piece = cond.as_deref().and_then(|src| Some((src, Self::parse_piece(src)?)));
                    let cond = piece.as_ref().and_then(|(src, p)| Some((*src, Self::piece_expr(p)?)));
                    let facts = match cond {
                        Some((src, e)) => self.with_facts(earlier.clone(), |c| {
                            let script = piece.as_ref().map(|(_, p)| p);
                            c.in_piece_of(src, script, *line, "the condition", |c| {
                                c.infer(e);
                            });
                            c.in_source(src, |c| c.facts_of(e, true))
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
                    match cond {
                        Some((src, e)) => {
                            let not = self.with_facts(earlier.clone(), |c| c.in_source(src, |c| c.facts_of(e, false)));
                            earlier.extend(not);
                        }
                        None => break,
                    }
                }
            }
        }
    }

    /// `null` and `()` where `none` is meant: both still read as `none`, and
    /// `rux fmt` rewrites them. Anything else at `pos` says nothing.
    fn old_spelling(&mut self, pos: Pos) {
        let written = self.source.text.get(pos.range()).unwrap_or_default().to_string();
        if written == "null" || written == "()" {
            self.warn(pos, format!("`{written}` is written `none` now; `rux fmt` rewrites it"));
        }
    }

    /// [`Self::old_spelling`] for each `null` in a type as written.
    fn old_spellings_in(&mut self, ty: &TypeExpr) {
        match &ty.kind {
            TypeKind::Null => self.old_spelling(ty.span),
            TypeKind::Array(t) | TypeKind::Optional(t) | TypeKind::Paren(t) => self.old_spellings_in(t),
            TypeKind::Dict { value, .. } => self.old_spellings_in(value),
            TypeKind::Union(members) | TypeKind::Generic { args: members, .. } => {
                members.iter().for_each(|m| self.old_spellings_in(m))
            }
            TypeKind::Record(fields) => fields.iter().for_each(|f| self.old_spellings_in(&f.ty)),
            TypeKind::Function(params, result) => {
                params.iter().for_each(|p| self.old_spellings_in(p));
                self.old_spellings_in(result);
            }
            TypeKind::Name(_) | TypeKind::Literal(_) => {}
        }
    }

    /// Report a name in `ty` that is no declared type.
    fn check_names_exist(&mut self, ty: &Type, pos: Pos) {
        let mut names = Vec::new();
        named_in(ty, &mut names);
        for (name, given) in names {
            let params = self.type_params.get(&name).map_or(0, Vec::len);
            if self.types.contains_key(&name) && !self.hidden.contains_key(&name) && given != params {
                let message = match (params, given) {
                    (0, _) => format!("`{name}` takes no type arguments"),
                    (_, 0) => format!(
                        "`{name}` is written with what it holds: `{name}<{}>`",
                        self.type_params[&name].join(", ")
                    ),
                    _ => format!(
                        "`{name}` takes {params} type argument{}: `{name}<{}>`",
                        if params == 1 { "" } else { "s" },
                        self.type_params[&name].join(", ")
                    ),
                };
                self.error(pos, message);
                continue;
            }
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
                // A generic one named without its arguments was reported.
                Type::Named(ref name) if self.type_params.get(name).is_some_and(|p| !p.is_empty()) => {
                    return Type::Any
                }
                Type::Named(ref name) => match self.types.get(name) {
                    Some(t) => ty = t.clone(),
                    None => return Type::Any,
                },
                Type::Generic(ref name, ref args) => match (self.types.get(name), self.type_params.get(name)) {
                    (Some(body), Some(params)) if params.len() == args.len() => {
                        let given = params.iter().cloned().zip(args.iter().cloned()).collect();
                        ty = body.substitute(&given);
                    }
                    _ => return Type::Any,
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
            // An `int` widens to a `float`; never the other way.
            (Type::Int, Type::Float) => true,
            (Type::BoolLit(_), Type::Bool) => true,
            // `return none;` and `return;` in a `void` function.
            (Type::Null, Type::Void) => true,
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
                    && (matches!(**tr, Type::Null | Type::Void) || self.assignable_at(fr, tr, d))
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
        match &cond.kind {
            ExprKind::Unary { op: "!", expr } => self.facts_of(expr, !truth),
            // All of an `&&` held, or none of an `||` did.
            ExprKind::Binary { op: "&&", .. } if truth => self.facts_of_all(&operands(cond, "&&"), true),
            ExprKind::Binary { op: "||", .. } if !truth => self.facts_of_all(&operands(cond, "||"), false),
            ExprKind::Binary { op: op @ ("==" | "!=" | "===" | "!=="), lhs, rhs } => {
                let equal = matches!(*op, "==" | "===") == truth;
                match self.facts_of_equality(lhs, rhs, equal) {
                    Some(facts) => facts,
                    None => self.facts_of_equality(rhs, lhs, equal).unwrap_or_default(),
                }
            }
            // `x is T`: a `T` where it held; where it did not, a union loses
            // the members that are all `T`.
            ExprKind::Is { expr, ty } => {
                let Some(path) = path_of(expr) else { return Vec::new() };
                let Ok(ty) = self.annotation(ty) else { return Vec::new() };
                if truth {
                    return vec![(path, ty)];
                }
                let t = self.infer_quietly(expr);
                match self.resolve(&t) {
                    Type::Union(members) => {
                        let kept: Vec<Type> =
                            members.iter().filter(|m| !self.assignable(m, &ty)).cloned().collect();
                        if kept.is_empty() || kept.len() == members.len() {
                            Vec::new()
                        } else {
                            vec![(path, Type::union(kept))]
                        }
                    }
                    _ => Vec::new(),
                }
            }
            // `"note" in t` and `k in m`.
            ExprKind::Binary { op: "in", lhs, rhs } if truth => self.facts_of_in(rhs, lhs),
            ExprKind::Binary { op: "!in", lhs, rhs } if !truth => self.facts_of_in(rhs, lhs),
            ExprKind::Binary { op: "!in", .. } => Vec::new(),
            // Truthiness: a path that is truthy is not `null`.
            _ => {
                let Some(path) = path_of(cond) else { return Vec::new() };
                let t = self.infer_quietly(cond);
                let mut out = if truth {
                    vec![(path.clone(), self.non_null(&t))]
                } else if falsy_only_when_null(&without_null(&self.resolve(&t))) && t != Type::Any {
                    vec![(path.clone(), Type::Null)]
                } else {
                    Vec::new()
                };
                out.extend(self.facts_of_flag(&path, truth));
                out
            }
        }
    }

    fn facts_of_all(&mut self, items: &[&Expr], truth: bool) -> Vec<(String, Type)> {
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
        if let (ExprKind::Call { callee, args, .. }, ExprKind::Str(tag)) = (&side.kind, &other.kind) {
            if is_named(callee, "type_of") && args.len() == 1 {
                let path = path_of(&args[0])?;
                let t = self.infer_quietly(&args[0]);
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
        if matches!(other.kind, ExprKind::Unit | ExprKind::Null) {
            return Some(vec![(path, if equal { Type::Null } else { self.non_null(&t) })]);
        }
        let ExprKind::Str(s) = &other.kind else { return Some(Vec::new()) };
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

    /// `if r.ok`: a union whose members say `ok: true` or `ok: false` keeps
    /// the ones that agree, which is how a `Result` is read.
    fn facts_of_flag(&mut self, path: &str, truth: bool) -> Vec<(String, Type)> {
        let Some(cut) = path.rfind('.') else { return Vec::new() };
        let (parent, field) = (&path[..cut], &path[cut + 1..]);
        let Some(Type::Union(members)) = self.type_at(parent).map(|t| self.resolve(&t)) else { return Vec::new() };
        let kept: Vec<Type> = members
            .iter()
            .filter(|m| match self.resolve(m) {
                Type::Record(fields) => {
                    !matches!(fields.iter().find(|f| f.name == field).map(|f| self.resolve(&f.ty)), Some(Type::BoolLit(b)) if b != truth)
                }
                Type::Null => !truth,
                _ => true,
            })
            .cloned()
            .collect();
        if !kept.is_empty() && kept.len() < members.len() {
            vec![(parent.to_string(), Type::union(kept))]
        } else {
            Vec::new()
        }
    }

    /// `key in map` held: the key is there.
    fn facts_of_in(&mut self, map: &Expr, key: &Expr) -> Vec<(String, Type)> {
        let Some(base) = path_of(map) else { return Vec::new() };
        let t = self.infer_quietly(map);
        let t = self.resolve(&t);
        match &key.kind {
            ExprKind::Str(k) => {
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
            ExprKind::Var(v) => match without_null(&t) {
                Type::Dict(value) => vec![(format!("{base}[{v}]"), *value)],
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// The names a function writes that are not its own: assignments to
    /// anything it did not declare, and whatever the functions it calls write.
    /// A closure's body is its own and is not counted.
    fn writes_of(&mut self, name: &str, arity: usize) -> HashSet<String> {
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
        walk_stmts(&info.def.body.stmts, &mut |node| {
            match node {
                Node::Stmt(Stmt { kind: StmtKind::Assign { target, .. } | StmtKind::Step { target, .. }, .. }) => {
                    if let Some(root) = path_of(target).map(|p| root_of(&p).to_string()) {
                        assigned.insert(root);
                    }
                }
                Node::Stmt(Stmt { kind: StmtKind::Let { name, .. }, .. }) => {
                    locals.insert(name.name.clone());
                }
                Node::Expr(Expr { kind: ExprKind::Call { callee, args, .. }, .. }) if callee.len() == 1 => {
                    calls.push((callee[0].name.clone(), args.len()));
                }
                Node::Expr(Expr { kind: ExprKind::Closure { .. } | ExprKind::Interval { .. }, .. }) => return false,
                _ => {}
            }
            true
        });
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
    fn check_block(&mut self, block: &Block) -> Type {
        self.with_scope(|c| c.check_statements(&block.stmts))
    }

    fn check_statements(&mut self, statements: &[Stmt]) -> Type {
        let mut last = Type::Null;
        for stmt in statements {
            last = self.check_stmt(stmt);
            self.after(stmt);
        }
        last
    }

    /// A statement used as a value, as a block of that one statement.
    fn stmt_value(&mut self, s: &Stmt) -> Type {
        self.with_scope(|c| c.check_statements(std::slice::from_ref(s)))
    }

    /// What a statement settles for the ones after it. An `if` that leaves
    /// settles its condition: after `if t == null { return; }`, `t` is not
    /// `null`.
    fn after(&mut self, stmt: &Stmt) {
        if let StmtKind::If(x) = &stmt.kind {
            let (body, branch) = (exits(&x.then.stmts), exits(else_stmts(x)));
            if body && !branch {
                let facts = self.facts_of(&x.cond, false);
                self.add_facts(facts);
            } else if branch && !body {
                let facts = self.facts_of(&x.cond, true);
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
                self.error(NOWHERE, message);
            }
            return;
        };
        for stmt in before {
            self.check_stmt(stmt);
            self.after(stmt);
        }
        match &last.kind {
            StmtKind::Expr(e) => self.check_result(e, name, want),
            StmtKind::If(x) if !else_stmts(x).is_empty() => {
                self.infer(&x.cond);
                let yes = self.facts_of(&x.cond, true);
                let no = self.facts_of(&x.cond, false);
                self.with_facts(yes, |c| c.with_scope(|c| c.check_statements_for(&x.then.stmts, name, want)));
                self.with_facts(no, |c| c.with_scope(|c| c.check_statements_for(else_stmts(x), name, want)));
            }
            StmtKind::Block(b) => self.with_scope(|c| c.check_statements_for(&b.stmts, name, want)),
            StmtKind::Switch(sw) => {
                self.check_switch(sw, last.span, Some((name, want)));
            }
            // A `return` checks itself.
            StmtKind::Return(_) | StmtKind::Throw(_) | StmtKind::Break(_) | StmtKind::Continue => {
                self.check_stmt(last);
            }
            _ => {
                let got = self.check_stmt(last);
                if !exits(std::slice::from_ref(last)) && !self.assignable(&got, want) {
                    self.result_mismatch(last.span, name, want, &got);
                }
            }
        }
    }

    /// One value a function hands back, checked against its declared result.
    fn check_result(&mut self, e: &Expr, name: &str, want: &Type) {
        // A block used as the value, which is how a `switch` or an `if` in
        // expression position arrives: its own last statement is the value.
        if let ExprKind::Stmt(s) = &e.kind {
            self.with_scope(|c| c.check_statements_for(std::slice::from_ref(&**s), name, want));
            return;
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

    fn result_mismatch(&mut self, pos: Pos, name: &str, want: &Type, got: &Type) {
        let message =
            format!("`{name}` is declared to return {}, and this is {}", self.show(want), self.show(got));
        self.error(pos, message);
    }

    /// Check a statement, returning the type of its value (`null` for one that
    /// has none).
    fn check_stmt(&mut self, stmt: &Stmt) -> Type {
        match &stmt.kind {
            StmtKind::Let { name, ty, value, .. } => {
                let unit;
                let value = match value {
                    Some(v) => v,
                    None => {
                        unit = Expr { id: ExprId(SYNTHETIC), kind: ExprKind::Unit, span: name.span };
                        &unit
                    }
                };
                self.check_let(name, ty.as_ref(), value);
                Type::Null
            }
            StmtKind::Assign { target, op, value } => {
                let op_at = self.op_at(target.span, value.span, op);
                self.assign(target, op, value, op_at);
                Type::Null
            }
            // `count++`, which is `count += 1`.
            StmtKind::Step { target, up } => {
                let one = Expr { id: ExprId(SYNTHETIC), kind: ExprKind::Int(1), span: target.span };
                let op_at = Span::at(target.span.end as usize);
                self.assign(target, if *up { "+=" } else { "-=" }, &one, op_at);
                Type::Null
            }
            StmtKind::If(x) => {
                self.infer(&x.cond);
                let yes = self.facts_of(&x.cond, true);
                let no = self.facts_of(&x.cond, false);
                let a = self.with_facts(yes, |c| c.check_block(&x.then));
                let b = self.with_facts(no, |c| c.with_scope(|c| c.check_statements(else_stmts(x))));
                Type::union([a, b])
            }
            StmtKind::Switch(sw) => self.check_switch(sw, stmt.span, None),
            StmtKind::While { cond, body } => {
                let yes = match cond {
                    Some(cond) => {
                        self.infer(cond);
                        self.facts_of(cond, true)
                    }
                    None => Vec::new(),
                };
                self.with_facts(yes, |c| c.check_block(body));
                Type::Null
            }
            StmtKind::Do { body, cond, .. } => {
                self.infer(cond);
                self.check_block(body);
                Type::Null
            }
            StmtKind::For { var, counter, iter, body } => {
                let iterable = self.infer(iter);
                let item = self.element_of(iter, &iterable);
                // `for k in keys(m)`: every `k` is a key `m` has.
                let keyed = match &iter.kind {
                    ExprKind::Call { callee, args, .. } if is_named(callee, "keys") && args.len() == 1 => {
                        path_of(&args[0]).map(|p| (p, &args[0]))
                    }
                    _ => None,
                };
                let mut facts = Vec::new();
                if let Some((base, map)) = keyed {
                    let t = self.infer_quietly(map);
                    if let Type::Dict(value) = without_null(&self.resolve(&t)) {
                        facts.push((format!("{base}[{}]", var.name), *value));
                    }
                }
                self.with_scope(|c| {
                    c.add_facts(facts);
                    c.note_decl(var.span, &item);
                    c.bind(&var.name, item);
                    if let Some(counter) = counter {
                        c.note_decl(counter.span, &Type::Int);
                        c.bind(&counter.name, Type::Int);
                    }
                    c.check_statements(&body.stmts);
                });
                Type::Null
            }
            StmtKind::Block(block) => self.check_block(block),
            StmtKind::Try { body, var, catch } => {
                self.check_block(body);
                self.with_scope(|c| {
                    if let Some(v) = var {
                        c.note_decl(v.span, &Type::Any);
                        c.bind(&v.name, Type::Any);
                    }
                    c.check_statements(&catch.stmts);
                });
                Type::Null
            }
            StmtKind::Expr(e) => self.infer(e),
            StmtKind::Return(value) => {
                if let Some(Some((name, want))) = self.results.last().cloned() {
                    match value {
                        Some(v) => self.check_result(v, &name, &want),
                        None if !self.assignable(&Type::Null, &want) => {
                            let message = format!(
                                "`{name}` is declared to return {}, and this returns nothing",
                                self.show(&want)
                            );
                            self.error(stmt.span, message);
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
            // What is thrown is counted with what is returned, as it was
            // when a `throw` was a flagged `return`.
            StmtKind::Throw(value) => {
                let ty = value.as_ref().map_or(Type::Null, |v| self.infer(v));
                if let Some(returns) = self.returns.last_mut() {
                    returns.push(ty);
                }
                Type::Null
            }
            StmtKind::Break(value) => {
                if let Some(v) = value {
                    self.infer(v);
                }
                Type::Null
            }
            StmtKind::Export(Export::Let(inner)) => self.check_stmt(inner),
            // A file's own declarations, which reach the checker when it is
            // handed the whole file (`check_typed`). The runtime's script has
            // them taken out already.
            StmtKind::Computed { name, ty, value } => {
                self.check_let(name, ty.as_ref(), value);
                Type::Null
            }
            StmtKind::Lifecycle { body, .. } => {
                self.check_block(body);
                Type::Null
            }
            StmtKind::Prop(decls) => {
                for d in decls {
                    match &d.default {
                        Some(default) => self.check_let(&d.name, d.ty.as_ref(), default),
                        None => {
                            let ty = d.ty.as_ref().and_then(|t| self.annotation(t).ok()).unwrap_or(Type::Any);
                            self.note_decl(d.name.span, &ty);
                            self.bind(&d.name.name, ty);
                        }
                    }
                }
                Type::Null
            }
            StmtKind::Empty
            | StmtKind::Continue
            | StmtKind::Fn(_)
            | StmtKind::Type { .. }
            | StmtKind::Import { .. }
            | StmtKind::Export(Export::Name { .. })
            | StmtKind::Use(_) => Type::Null,
        }
    }

    /// `target op value`, `op` being `=` or one of `+=` and the rest.
    fn assign(&mut self, target: &Expr, op: &str, value: &Expr, op_at: Pos) {
        // What was known about the target no longer holds once it is
        // written; what it holds now is its declared type.
        if let Some(path) = path_of(target) {
            self.forget(&path);
        }
        let held = self.infer(target);
        if op == "=" {
            let before = self.findings.len();
            self.check_expr(value, &held);
            // Name the variable, and say how to let it hold both.
            if let ExprKind::Var(v) = &target.kind {
                if self.findings.len() == before + 1 {
                    let got = widen(&self.infer_quietly(value));
                    // The narrowest type that holds both: an `int[]` given a
                    // `float[]` wants `float[]`, not a union.
                    let both = if self.assignable(&held, &got) {
                        got.clone()
                    } else {
                        Type::union([held.clone(), got.clone()])
                    };
                    let last = self.findings.last_mut().unwrap();
                    if last.is_error && last.message.starts_with("this is ") {
                        last.message = format!(
                            "`{0}` holds `{1}`, so it cannot be given `{2}` here. If it \
                             may hold either, say so: `let {0}: {3} = …`",
                            v,
                            held,
                            got,
                            both
                        );
                    }
                }
            }
            return;
        }
        let rhs = self.infer(value);
        let operator = op.trim_end_matches('=');
        let result = self.binary(operator, &held, &rhs, op_at);
        if !self.assignable(&result, &held) {
            let message = format!("`{op}` makes {}, which does not fit {}", self.show(&result), self.show(&held));
            self.error(op_at, message);
        }
    }

    /// A `switch`: each arm runs knowing which case matched, and one on a
    /// literal union with no `_` arm must handle every member.
    fn check_switch(&mut self, sw: &Switch, pos: Pos, want: Option<(&str, &Type)>) -> Type {
        let subject_type = self.infer(&sw.value);
        let path = path_of(&sw.value);
        let members = literal_members(&without_null(&self.resolve(&subject_type)))
            .unwrap_or_default()
            .into_iter()
            .map(|quoted| quoted.trim_matches('"').to_string())
            .collect::<Vec<_>>();
        let default = sw.arms.iter().position(|a| a.patterns.is_empty());
        let mut arm_members: HashMap<usize, Vec<String>> = HashMap::new();
        let mut unhandled = Vec::new();
        for m in &members {
            let arms: Vec<usize> = sw
                .arms
                .iter()
                .enumerate()
                .filter(|(_, a)| a.patterns.iter().any(|p| matches!(&p.kind, ExprKind::Str(s) if s == m)))
                .map(|(i, _)| i)
                .collect();
            if arms.is_empty() {
                unhandled.push(m.clone());
            }
            for arm in arms {
                arm_members.entry(arm).or_default().push(m.clone());
            }
        }
        if default.is_none() && !unhandled.is_empty() && !members.is_empty() {
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

        // A case's values are constants, typed for the IR's sake.
        for p in sw.arms.iter().flat_map(|a| &a.patterns) {
            self.infer(p);
        }
        let mut out = Vec::new();
        for (i, arm) in sw.arms.iter().enumerate() {
            let matched = if default == Some(i) { Some(&unhandled) } else { arm_members.get(&i) };
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
            let value = self.with_facts(facts, |c| {
                if let Some(guard) = &arm.guard {
                    c.infer(guard);
                }
                match (want, &arm.body.kind) {
                    // Each arm is a value the function returns.
                    (Some((name, want)), StmtKind::Expr(e)) => {
                        c.check_result(e, name, want);
                        want.clone()
                    }
                    (Some((name, want)), _) => {
                        c.with_scope(|c| c.check_statements_for(std::slice::from_ref(&*arm.body), name, want));
                        want.clone()
                    }
                    (None, StmtKind::Expr(e)) => c.infer(e),
                    (None, _) => c.stmt_value(&arm.body),
                }
            });
            out.push(value);
        }
        if out.is_empty() {
            Type::Null
        } else {
            Type::union(out)
        }
    }

    fn check_let(&mut self, ident: &Ident, ty: Option<&TypeExpr>, value: &Expr) {
        let (name, pos) = (ident.name.as_str(), ident.span);
        if let Some((_, ty)) = self.cx.placeholders.iter().find(|(n, _)| n == name) {
            let ty = ty.clone().unwrap_or(Type::Any);
            self.saw(pos, "let", name, Some(name), &ty);
            self.note_decl(pos, &ty);
            self.bind(name, ty);
            return;
        }
        if let Some(ty) = ty.and_then(|t| self.annotation(t).ok()) {
            self.check_expr(value, &ty);
            self.saw(pos, "let", name, Some(name), &ty);
            self.note_decl(pos, &ty);
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
                    "`{name}` starts as `none`, which says nothing about what it will hold, so \
                     nothing done with it is checked. Say what it holds: `let {name}: T? = …`"
                ),
                Blank::Host(f) => format!(
                    "`host::{f}` has no signature, so what `{name}` holds is not checked. Say \
                     what it returns: `let {name}: T = …`"
                ),
            };
            self.warn(pos, hint);
            self.saw(pos, "let", name, Some(name), &Type::Any);
            self.note_decl(pos, &Type::Any);
            self.bind(name, Type::Any);
            return;
        }
        self.saw(pos, "let", name, Some(name), &ty);
        self.note_decl(pos, &ty);
        self.bind(name, ty);
    }

    // ----- Expressions -----------------------------------------------------

    /// Check `e` where a `want` is expected, reporting it if it does not fit.
    fn check_expr(&mut self, e: &Expr, want: &Type) {
        self.note_want(e, want);
        let resolved = self.resolve(want);
        if resolved == Type::Any {
            self.infer(e);
            return;
        }
        match &e.kind {
            ExprKind::Int(_) if self.accepts_int(&resolved) => self.note(e, &Type::Int),
            ExprKind::Call { callee, args, .. } if is_named(callee, "signal") && args.len() == 1 => {
                self.check_expr(&args[0], want);
                self.note(e, want);
            }
            // A negative whole number written as `-` applied to one.
            ExprKind::Unary { op: "-", expr } if matches!(expr.kind, ExprKind::Int(_)) && self.accepts_int(&resolved) => {
                self.note(expr, &Type::Int);
                self.note(e, &Type::Int);
            }
            ExprKind::Array(items) => match self.array_member(&resolved) {
                Some(item) => {
                    for i in items.iter() {
                        self.check_expr(i, &item);
                    }
                    self.note(e, &Type::Array(Box::new(item)));
                }
                None => self.mismatch(e, e.span, want),
            },
            ExprKind::Map { entries, .. } => {
                self.check_map(entries, e.span, want, &resolved);
                self.note(e, want);
            }
            ExprKind::Closure { .. } => {
                let expected = match &resolved {
                    Type::Function(p, r) => Some((p.clone(), (**r).clone())),
                    _ => None,
                };
                let got = self.closure(e, expected);
                self.note(e, &got);
                if !self.assignable(&got, want) {
                    self.mismatch_ty(self.at(e), &got, want);
                }
            }
            ExprKind::Stmt(s) => {
                // A block used as a value: its last statement is the value.
                let got = self.stmt_value(s);
                self.note(e, &got);
                if !self.assignable(&got, want) {
                    self.mismatch_ty(self.at(e), &got, want);
                }
            }
            _ => {
                let got = self.infer(e);
                if self.assignable(&got, want) {
                    return;
                }
                // The one way two `int`s make a `float`, and the fix for it.
                if matches!(e.kind, ExprKind::Binary { op: "/", .. })
                    && self.resolve(&got) == Type::Float
                    && self.accepts_int(&resolved)
                {
                    let message = format!(
                        "`/` always makes a `float`, where {} is expected; `intDiv(a, b)` divides \
                         to an `int`",
                        self.show(want)
                    );
                    self.error(self.at(e), message);
                    return;
                }
                self.mismatch_ty(self.at(e), &got, want);
            }
        }
    }

    fn mismatch(&mut self, e: &Expr, pos: Pos, want: &Type) {
        let got = self.infer(e);
        self.mismatch_ty(pos, &got, want);
    }

    fn mismatch_ty(&mut self, pos: Pos, got: &Type, want: &Type) {
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

    /// Whether a whole-number literal fits `ty`.
    fn accepts_int(&self, ty: &Type) -> bool {
        match self.resolve(ty) {
            Type::Int | Type::Float | Type::Any => true,
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
    fn check_map(&mut self, entries: &[MapEntry], pos: Pos, want: &Type, resolved: &Type) {
        match resolved {
            Type::Record(expected) => {
                for entry in entries {
                    let key = entry.key.name.as_str();
                    match expected.iter().find(|f| f.name == key) {
                        Some(field) => {
                            let ty = if field.optional { field.ty.clone().optional() } else { field.ty.clone() };
                            self.check_expr(&entry.value, &ty);
                        }
                        None => {
                            self.infer(&entry.value);
                            let names: Vec<&str> = expected.iter().map(|f| f.name.as_str()).collect();
                            let hint =
                                near_miss(key, &names).map(|n| format!("; did you mean `{n}`?")).unwrap_or_default();
                            let message = format!("{} has no field `{key}`{hint}", self.show(want));
                            self.error(entry.key.span, message);
                        }
                    }
                }
                let missing: Vec<&Field> = expected
                    .iter()
                    .filter(|f| !f.optional && !entries.iter().any(|e| e.key.name == f.name))
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
                for entry in entries {
                    self.check_expr(&entry.value, value);
                }
            }
            Type::Union(members) => {
                // The first member it fits without a word said. None fitting
                // is one error naming the union, not one per member.
                for member in members {
                    if self.fits_quietly(|c| c.check_map(entries, pos, member, &c.resolve(member))) {
                        self.check_map(entries, pos, member, &self.resolve(member));
                        return;
                    }
                }
                let got = self.infer_map(entries);
                self.mismatch_ty(pos, &got, want);
            }
            _ => {
                let got = self.infer_map(entries);
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

    fn infer_map(&mut self, entries: &[MapEntry]) -> Type {
        Type::Record(
            entries
                .iter()
                .map(|entry| Field {
                    name: entry.key.name.clone(),
                    optional: false,
                    ty: widen(&self.infer(&entry.value)),
                })
                .collect(),
        )
    }

    /// The type of `e`.
    fn infer(&mut self, e: &Expr) -> Type {
        let ty = self.infer_here(e);
        self.note(e, &ty);
        ty
    }

    fn infer_here(&mut self, e: &Expr) -> Type {
        match &e.kind {
            ExprKind::Closure { .. } => self.closure(e, None),
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Int(_) => Type::Int,
            ExprKind::Float(_) => Type::Float,
            ExprKind::Char(_) => Type::String,
            ExprKind::Str(s) => Type::Literal(s.clone()),
            ExprKind::Template(parts) => {
                for p in parts {
                    if let TemplatePart::Code(block) = p {
                        self.check_block(block);
                    }
                }
                Type::String
            }
            ExprKind::Array(items) => {
                let members: Vec<Type> = items.iter().map(|i| widen(&self.infer(i))).collect();
                if members.is_empty() {
                    Type::Array(Box::new(Type::Any))
                } else {
                    Type::Array(Box::new(Type::union(members)))
                }
            }
            ExprKind::Map { entries, .. } => self.infer_map(entries),
            ExprKind::Unit | ExprKind::Null => {
                self.old_spelling(e.span);
                Type::Null
            }
            ExprKind::Var(name) => match self.lookup(name) {
                Some(t) => {
                    self.saw(e.span, "value", name, Some(name), &t);
                    t
                }
                None => {
                    self.caller_local(name, e.span);
                    Type::Any
                }
            },
            ExprKind::Path(_) | ExprKind::This => Type::Any,
            ExprKind::Stmt(s) => self.stmt_value(s),
            ExprKind::Call { callee, args, .. } => self.call(callee, args),
            ExprKind::Field { .. } | ExprKind::Method { .. } | ExprKind::Index { .. } => {
                let (ty, _, short) = self.chain(e);
                if short {
                    ty.optional()
                } else {
                    ty
                }
            }
            ExprKind::Unary { op, expr } => {
                let t = self.infer(expr);
                match *op {
                    "!" => Type::Bool,
                    "-" | "+" => match self.resolve(&t) {
                        Type::Int => Type::Int,
                        Type::Any => Type::Any,
                        _ => Type::Float,
                    },
                    _ => Type::Any,
                }
            }
            ExprKind::Binary { op, lhs, rhs } => self.operator(e, op, lhs, rhs),
            ExprKind::Is { expr, ty } => {
                self.infer(expr);
                match self.annotation(ty) {
                    // At run time there is no `T` to test against.
                    Ok(t) if t.has_param() => self.error(
                        ty.span,
                        format!("`is` cannot test for `{t}`: a type parameter is not known when the program runs"),
                    ),
                    Ok(t) => self.check_names_exist(&t, ty.span),
                    Err(err) => {
                        let text = print::ty(ty);
                        self.error(ty.span, format!("`{text}` is not a type: {err}"));
                    }
                }
                Type::Bool
            }
            // The body runs later, as its own script: checked as that script,
            // with the file's names and none of the locals around it, and
            // knowing nothing a condition here established.
            ExprKind::Interval { args, body } => {
                self.infer_all(args);
                let scopes = std::mem::take(&mut self.scopes);
                let facts = std::mem::replace(&mut self.facts, vec![HashMap::new()]);
                self.check_block(body);
                self.scopes = scopes;
                self.facts = facts;
                Type::Int
            }
        }
    }

    /// A binary operator.
    fn operator(&mut self, e: &Expr, op: &str, lhs: &Expr, rhs: &Expr) -> Type {
        let pos = self.op_at(lhs.span, rhs.span, op);
        match op {
            // Each operand runs knowing the ones before it held (`&&`) or
            // failed (`||`): `t != null && t.done` reads `t` as not `null`.
            "&&" | "||" => {
                let truth = op == "&&";
                let mut known: Vec<(String, Type)> = Vec::new();
                for i in operands(e, op) {
                    self.with_facts(known.clone(), |c| c.infer(i));
                    let more = self.with_facts(known.clone(), |c| c.facts_of(i, truth));
                    known.extend(more);
                }
                // `a && b && c` is read as one list, so the `a && b` inside it
                // is never inferred on its own.
                let mut inner = vec![lhs, rhs];
                while let Some(x) = inner.pop() {
                    if let ExprKind::Binary { op: o, lhs, rhs } = &x.kind {
                        if *o == op {
                            self.note(x, &Type::Bool);
                            inner.extend([&**lhs, &**rhs]);
                        }
                    }
                }
                Type::Bool
            }
            "??" => {
                let items = operands(e, op);
                let n = items.len();
                let mut out = Vec::new();
                let mut raw = Vec::new();
                for (i, item) in items.into_iter().enumerate() {
                    let t = self.infer(item);
                    raw.push(t.clone());
                    out.push(if i + 1 < n { without_null(&self.resolve(&t)) } else { t });
                }
                // The `a ?? b` inside `a ?? b ?? c`, which is read as one
                // list: each is what its own operands make.
                let ids: Vec<u32> = operands(e, op).iter().map(|o| o.id.0).collect();
                let mut inner = vec![lhs, rhs];
                while let Some(x) = inner.pop() {
                    let ExprKind::Binary { op: "??", lhs: l, rhs: r } = &x.kind else { continue };
                    let own = operands(x, op);
                    let at = |o: &Expr| ids.iter().position(|i| *i == o.id.0).map(|i| raw[i].clone()).unwrap_or(Type::Any);
                    let mut parts: Vec<Type> =
                        own[..own.len() - 1].iter().map(|o| without_null(&self.resolve(&at(o)))).collect();
                    parts.push(at(own[own.len() - 1]));
                    self.note(x, &Type::union(parts));
                    inner.extend([&**l, &**r]);
                }
                Type::union(out)
            }
            "in" | "!in" => {
                self.infer(rhs);
                self.infer(lhs);
                Type::Bool
            }
            _ => {
                let a = self.infer(lhs);
                let b = self.infer(rhs);
                if matches!(op, ".." | "..=") {
                    return Type::Any;
                }
                if matches!(op, "==" | "!=" | "===" | "!==") {
                    self.check_comparable(lhs, &a, rhs, &b, pos);
                }
                // `2 ** 3` is an `int`: a power of an `int` to a whole
                // number written out, and not below zero, is one.
                if op == "**" && matches!(rhs.kind, ExprKind::Int(n) if n >= 0) && self.resolve(&a) == Type::Int {
                    return Type::Int;
                }
                self.binary(op, &a, &b, pos)
            }
        }
    }

    /// A chain of steps, `.name`, `.method(…)` and `[index]`, ending at `e`.
    /// Returns the last step's type, its path, and whether any step was
    /// written `?.` or `?[`, which makes the whole chain possibly `null`.
    fn chain(&mut self, e: &Expr) -> (Type, Option<String>, bool) {
        let (base, optional) = match &e.kind {
            ExprKind::Field { base, optional, .. } | ExprKind::Index { base, optional, .. } => (&**base, *optional),
            ExprKind::Method { recv, optional, .. } => (&**recv, *optional),
            _ => return (self.infer(e), path_of(e), false),
        };
        let (base_ty, path, short) = if is_step(base) {
            let (ty, path, short) = self.chain(base);
            self.note(base, &ty);
            (ty, path, short)
        } else {
            (self.infer(base), path_of(base), false)
        };
        let (ty, here) = self.step(&base_ty, optional, e, path);
        (ty, here, short || optional)
    }

    /// One step of a chain: `e` is `.name`, `.method(…)` or `[index]` applied
    /// to a `base` that sits at `path`. Returns the step's type and its own
    /// path.
    fn step(&mut self, base: &Type, optional: bool, e: &Expr, path: Option<String>) -> (Type, Option<String>) {
        let here = path.clone().and_then(|p| path_step(p, e));
        // What a condition established about this step wins: `t.note` inside
        // `if t?.note != null` is present, whatever the record says. A fact
        // that it may be `null` says it may be absent, so the rules for
        // reading an absent one still apply, and only the type is taken.
        let known = here.as_deref().and_then(|p| self.fact(p));
        if let Some(known) = &known {
            if !may_be_null(&self.resolve(known)) {
                match &e.kind {
                    ExprKind::Index { index, .. } => {
                        self.infer(index);
                    }
                    ExprKind::Field { name, .. } => {
                        self.saw(name.span, "field", &name.name, here.as_deref(), known);
                    }
                    _ => {}
                }
                return (known.clone(), here);
            }
        }
        // Messages name the type as written (`Task`), not what it expands to.
        let base_written = base;
        let shown = if optional { without_null(base) } else { base.clone() };
        let base = self.resolve(&shown);
        let read = Read { optional, path: path.as_deref() };
        let ty = match &e.kind {
            ExprKind::Index { index, .. } => {
                let key = self.infer(index);
                self.index(&base, &shown, &key, index, read)
            }
            // `a?.b` on an `a` that may be `null` may be `null` itself.
            ExprKind::Field { name, .. } if optional && may_be_null(&self.resolve(base_written)) => {
                self.property(&base, &shown, &name.name, name.span, read).optional()
            }
            ExprKind::Field { name, .. } => self.property(&base, &shown, &name.name, name.span, read),
            // A `Result`'s value, or the error thrown.
            ExprKind::Method { name, args, .. } if name.name == "unwrap" && args.is_empty() => match &shown {
                Type::Generic(n, a) if n == "Result" && a.len() == 2 => a[0].clone(),
                _ => self.method(&base, &name.name, args),
            },
            ExprKind::Method { name, args, .. } => self.method(&base, &name.name, args),
            _ => Type::Any,
        };
        let ty = known.unwrap_or(ty);
        if let ExprKind::Field { name, .. } = &e.kind {
            self.saw(name.span, "field", &name.name, here.as_deref(), &ty);
        }
        (ty, here)
    }

    fn property(&mut self, base: &Type, shown: &Type, name: &str, pos: Pos, read: Read) -> Type {
        match base {
            Type::Any => Type::Any,
            Type::Param(p) => {
                self.error(pos, format!("`{p}` is a type parameter, so nothing is known of a `{name}` on it"));
                Type::Any
            }
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
                                 or check it first: `if {whole}?.{name} != none {{ … }}`",
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
                // A value that may be `null` has no fields when it is, and
                // reading one raises: `?.` or a check first, as for an
                // optional field.
                if members.contains(&Type::Null) && !read.optional {
                    let whole = read.path.map_or_else(|| "…".to_string(), str::to_string);
                    self.error(
                        pos,
                        format!(
                            "`{whole}` may be `none`, so read it as `{whole}?.{name}`, or check it \
                             first: `if {whole} != none {{ … }}`"
                        ),
                    );
                }
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
            Type::Float
            | Type::Int
            | Type::Bool
            | Type::BoolLit(_)
            | Type::Void
            | Type::String
            | Type::Literal(_)
            | Type::Array(_) => {
                self.error(pos, format!("{} has no property `{name}`", self.show(shown)));
                Type::Any
            }
            Type::Null => {
                self.error(
                    pos,
                    format!("this is `none` here, so it has no `{name}`; read it with `?.` if it may be absent"),
                );
                Type::Any
            }
            _ => Type::Any,
        }
    }

    fn index(&mut self, base: &Type, shown: &Type, key: &Type, what: &Expr, read: Read) -> Type {
        match base {
            Type::Array(item) => {
                if self.resolve(key) == Type::Float {
                    let message = "a list is indexed by an `int`, and this is a `float`; `.trunc()` makes one of it";
                    self.error(self.at(what), message.to_string());
                } else if !self.assignable(key, &Type::Int) {
                    let message = format!("a list is indexed by an `int`, and this is {}", self.show(key));
                    self.error(self.at(what), message);
                }
                (**item).clone()
            }
            Type::Dict(value) => {
                // A key may be missing, and `[` raises on one that is: `?[`
                // reads it as possibly `null`, and `k in m` rules it out.
                if !read.optional {
                    let whole = read.path.map_or_else(|| "…".to_string(), str::to_string);
                    let k = match &what.kind {
                        ExprKind::Var(v) => v.clone(),
                        ExprKind::Str(s) => format!("{s:?}"),
                        _ => "k".to_string(),
                    };
                    self.error(
                        self.at(what),
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
                        self.error(self.at(what), format!("{} has no field `{k}`", self.show(shown)));
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
                                    self.error(self.at(what), format!("{} has no field `{k}`", self.show(shown)));
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
        match &e.kind {
            ExprKind::Binary { op: ".." | "..=", .. } => return Type::Int,
            ExprKind::Call { callee, .. } if is_named(callee, "range") => return Type::Int,
            _ => {}
        }
        match self.resolve(ty) {
            Type::Array(item) => *item,
            Type::String | Type::Literal(_) => Type::String,
            _ => Type::Any,
        }
    }

    // ----- Operators -------------------------------------------------------

    fn binary(&mut self, op: &str, a: &Type, b: &Type, pos: Pos) -> Type {
        let a = self.resolve(a);
        let b = self.resolve(b);
        if a == Type::Any || b == Type::Any {
            return match op {
                "==" | "!=" | "<" | ">" | "<=" | ">=" | "===" | "!==" => Type::Bool,
                _ => Type::Any,
            };
        }
        let number = |t: &Type| matches!(t, Type::Float | Type::Int);
        let text = |t: &Type| matches!(t, Type::String | Type::Literal(_));
        match op {
            "+" if text(&a) || text(&b) => Type::String,
            "+" if matches!(a, Type::Array(_)) && matches!(b, Type::Array(_)) => {
                let (Type::Array(x), Type::Array(y)) = (&a, &b) else { unreachable!() };
                Type::Array(Box::new(Type::union([(**x).clone(), (**y).clone()])))
            }
            // Two `int`s make an `int`, except by `/`, which always makes a
            // `float`: `7 / 2` is `3.5`, and `intDiv(7, 2)` is `3`.
            "+" | "-" | "*" | "%" if a == Type::Int && b == Type::Int => Type::Int,
            "+" | "-" | "*" | "/" | "%" | "**" if number(&a) && number(&b) => Type::Float,
            "==" | "!=" | "===" | "!==" => Type::Bool,
            "<" | ">" | "<=" | ">=" => Type::Bool,
            "&" | "|" | "^" | "<<" | ">>" => Type::Int,
            _ => {
                let message = format!(
                    "`{op}` cannot be applied to {} and {}",
                    self.show(&a),
                    self.show(&b)
                );
                self.error(pos, message);
                Type::Any
            }
        }
    }

    // ----- Calls -----------------------------------------------------------

    /// A call, by name: `f(args)` or `a::b(args)`.
    fn call(&mut self, callee: &[Ident], args: &[Expr]) -> Type {
        let pos = callee.first().map_or(NOWHERE, |c| c.span);
        let Some(last) = callee.last() else { return Type::Any };
        let name = last.name.as_str();

        if callee.len() > 1 {
            for a in args {
                self.infer(a);
            }
            if callee.len() == 2 && callee[0].name == "host" {
                if let Some((_, Type::Function(_, result))) = self.cx.host.iter().find(|(n, _)| n == name) {
                    return (**result).clone();
                }
            }
            return Type::Any;
        }

        match (name, args.len()) {
            ("signal", 1) => return self.infer(&args[0]),
            // The two halves of a `Result`; the other half is whatever the
            // place it goes wants.
            ("Ok", 1) => {
                let t = widen(&self.infer(&args[0]));
                return Type::Generic("Result".into(), vec![t, Type::Any]);
            }
            ("Err", 1) => {
                let t = widen(&self.infer(&args[0]));
                return Type::Generic("Result".into(), vec![Type::Any, t]);
            }
            // Text to a number gives `none` where JavaScript gives `NaN`.
            ("parseInt", 1) => {
                self.infer_all(args);
                return Type::Int.optional();
            }
            ("parseFloat", 1) => {
                self.infer_all(args);
                return Type::Float.optional();
            }
            // `int` in and out, and a `float` as soon as one comes in.
            ("max" | "min" | "abs", _) => {
                let mut all_int = !args.is_empty();
                for a in args {
                    let t = self.infer(a);
                    all_int &= self.resolve(&t) == Type::Int;
                }
                return if all_int { Type::Int } else { Type::Float };
            }
            ("Number" | "parse_float" | "to_float" | "toFloat" | "sqrt", _) => {
                self.infer_all(args);
                return Type::Float;
            }
            ("to_int" | "parse_int" | "trunc" | "floor" | "ceil" | "round" | "intDiv", _) => {
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
                return Type::Int;
            }
            _ => {}
        }

        // A function of the script's own.
        if self.fns.contains_key(&(name.to_string(), args.len())) {
            let result = self.call_script_fn(name, args);
            if let Some(sig) = self.signature(name, args.len()) {
                if self.saw(pos, "fn", name, None, &Type::Any) {
                    self.seen.last_mut().unwrap().ty = sig;
                }
            }
            return result;
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
    fn caller_local(&mut self, name: &str, pos: Pos) {
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

    /// A comparison that can never be true is almost always a typo: `filter ==
    /// "al"` for `"all"`, or a number compared with text. `null` is always
    /// allowed on either side, since checking for it is how a program is
    /// careful.
    fn check_comparable(&mut self, left: &Expr, a: &Type, right: &Expr, b: &Type, pos: Pos) {
        if self.overlaps(a, b) {
            return;
        }
        for (side, other, t) in [(left, right, a), (right, left, b)] {
            if let ExprKind::Str(s) = &other.kind {
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
            (Type::Param(_), _) | (_, Type::Param(_)) => true,
            (Type::Union(m), _) => m.iter().any(|x| self.overlaps(x, &b)),
            (_, Type::Union(m)) => m.iter().any(|x| self.overlaps(&a, x)),
            (Type::Literal(x), Type::Literal(y)) => x == y,
            (Type::Literal(_) | Type::String, Type::Literal(_) | Type::String) => true,
            (Type::Float | Type::Int, Type::Float | Type::Int) => true,
            (Type::Bool | Type::BoolLit(_), Type::Bool) | (Type::Bool, Type::BoolLit(_)) => true,
            (Type::BoolLit(a), Type::BoolLit(b)) => a == b,
            (Type::Record(_) | Type::Dict(_), Type::Record(_) | Type::Dict(_)) => true,
            (Type::Array(_), Type::Array(_)) | (Type::Function(..), Type::Function(..)) => true,
            (Type::Named(_), _) | (_, Type::Named(_)) => true,
            _ => false,
        }
    }

    /// Infer each argument of a call whose signature is not known. A closure
    /// among them gets `any` parameters without a word: whatever it is handed
    /// is already unchecked, and that was reported where it began.
    fn infer_all(&mut self, args: &[Expr]) {
        for a in args {
            if matches!(a.kind, ExprKind::Closure { .. }) {
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

    fn call_script_fn(&mut self, name: &str, args: &[Expr]) -> Type {
        let key = (name.to_string(), args.len());
        let mut params: Vec<(String, Option<Type>)> = self.fns[&key].params.clone();
        // A generic function's type parameters are what its arguments make
        // them, and the call is then checked as an ordinary one.
        let given = if self.fns[&key].tparams.is_empty() {
            None
        } else {
            let tparams = self.fns[&key].tparams.clone();
            let given = self.type_arguments(&tparams, &params, args);
            for (_, ty) in params.iter_mut() {
                *ty = ty.as_ref().map(|t| t.substitute(&given));
            }
            Some(given)
        };
        for (i, (arg, (param, ty))) in args.iter().zip(params.iter()).enumerate() {
            match ty {
                Some(ty) => {
                    let before = self.findings.len();
                    self.check_expr(arg, ty);
                    // Say which parameter the value was for.
                    if self.findings.len() == before + 1 {
                        let got = self.infer_quietly(arg);
                        let last = self.findings.last_mut().unwrap();
                        if last.is_error && last.message.starts_with("this is ") {
                            last.message = format!("`{name}` takes `{ty}` as `{param}`, and this is `{got}`");
                        }
                    }
                }
                None => {
                    let t = self.infer(arg);
                    if self.record && self.quiet == 0 {
                        self.given.entry(key.clone()).or_default().push((i, widen(&t)));
                    }
                }
            }
        }
        let mut result = self.function_result(name, args.len());
        if let Some(given) = given {
            result = result.substitute(&given);
        }
        // Whatever the function writes may no longer be what a condition
        // established: a fact about a signal dies at a call that writes it.
        for written in self.writes_of(name, args.len()) {
            self.forget(&written);
        }
        result
    }

    /// What each type parameter of a generic function is at a call, from its
    /// arguments: the plain ones first, then the closures, which are handed
    /// what the plain ones settled (`map(items, t => t.id)`). One nothing
    /// settles is `any`.
    fn type_arguments(&mut self, tparams: &[String], params: &[(String, Option<Type>)], args: &[Expr]) -> HashMap<String, Type> {
        let mut given: HashMap<String, Type> = HashMap::new();
        let is_closure = |a: &Expr| matches!(a.kind, ExprKind::Closure { .. });
        for closures in [false, true] {
            for (arg, (_, ty)) in args.iter().zip(params) {
                let Some(ty) = ty.as_ref().filter(|t| t.has_param()) else { continue };
                if is_closure(arg) != closures {
                    continue;
                }
                let got = if closures {
                    let shape = match ty.substitute(&given) {
                        Type::Function(p, r) => Some((p, *r)),
                        _ => None,
                    };
                    let errors = self.quiet_errors;
                    self.quiet += 1;
                    let t = self.closure(arg, shape.map(|(p, _)| (p, Type::Any)));
                    self.quiet -= 1;
                    self.quiet_errors = errors;
                    t
                } else {
                    self.infer_quietly(arg)
                };
                self.unify(ty, &widen(&got), &mut given, 0);
            }
        }
        for p in tparams {
            given.entry(p.clone()).or_insert(Type::Any);
        }
        given
    }

    /// Match `pattern`, which has type parameters in it, against `actual`,
    /// and note in `given` what each parameter must be. Where two arguments
    /// say different things, the wider one stands and the check that follows
    /// reports the other.
    fn unify(&self, pattern: &Type, actual: &Type, given: &mut HashMap<String, Type>, depth: usize) {
        if depth > MAX_DEPTH || !pattern.has_param() {
            return;
        }
        let d = depth + 1;
        // A parameter takes the type as written, `Task` and not its fields;
        // the structure is compared unfolded.
        let written = actual;
        let actual = self.resolve(actual);
        if actual == Type::Any {
            return;
        }
        match (pattern, &actual) {
            (Type::Param(p), _) => match given.get(p) {
                None => {
                    given.insert(p.clone(), written.clone());
                }
                Some(before) if self.assignable(before, written) && !self.assignable(written, before) => {
                    given.insert(p.clone(), written.clone());
                }
                Some(_) => {}
            },
            (Type::Array(p), Type::Array(a)) | (Type::Dict(p), Type::Dict(a)) => self.unify(p, a, given, d),
            (Type::Dict(p), Type::Record(fields)) => {
                self.unify(p, &Type::union(fields.iter().map(|f| f.ty.clone())), given, d)
            }
            (Type::Record(want), Type::Record(have)) => {
                for w in want {
                    if let Some(h) = have.iter().find(|h| h.name == w.name) {
                        self.unify(&w.ty, &h.ty, given, d);
                    }
                }
            }
            (Type::Function(pp, pr), Type::Function(ap, ar)) => {
                for (p, a) in pp.iter().zip(ap) {
                    self.unify(p, a, given, d);
                }
                self.unify(pr, ar, given, d);
            }
            // `T?` given a `string?` is a `string`: what the pattern names
            // itself is taken out, and the one member with a parameter in it
            // gets the rest.
            (Type::Union(members), _) => {
                let (generic, fixed): (Vec<&Type>, Vec<&Type>) = members.iter().partition(|m| m.has_param());
                let [one] = generic[..] else { return };
                let rest = match &actual {
                    Type::Union(have) => Type::union(
                        have.iter().filter(|h| !fixed.iter().any(|f| self.assignable(h, f))).cloned(),
                    ),
                    other if fixed.iter().any(|f| self.assignable(other, f)) => return,
                    other => other.clone(),
                };
                self.unify(one, &rest, given, d);
            }
            // A declared generic resolves to its body, so compare bodies.
            (Type::Generic(..), _) => {
                let body = self.resolve(pattern);
                if body != Type::Any {
                    self.unify(&body, &actual, given, d);
                }
            }
            _ => {}
        }
    }

    /// What a named function returns, checking its body the first time it is
    /// asked.
    fn function_result(&mut self, name: &str, arity: usize) -> Type {
        let key = (name.to_string(), arity);
        let Some(info) = self.fns.get(&key).cloned() else { return Type::Any };
        let at = info.def.body.span;
        match info.state {
            FnState::Done(t) => return info.result.unwrap_or(t),
            FnState::Checking => {
                // Recursion. A declared result answers it; without one there is
                // no answer to be had, and the author is told to write one.
                return match info.result {
                    Some(t) => t,
                    None => {
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
        let untyped: Vec<&str> =
            info.params.iter().filter(|(_, t)| t.is_none()).map(|(p, _)| p.as_str()).collect();
        if !untyped.is_empty() {
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
        let saved_tparams = std::mem::replace(&mut self.tparams, info.tparams.clone());
        // Typed means every parameter annotated, which a function with none is.
        let typed = info.params.iter().all(|(_, t)| t.is_some());
        self.in_fn.push((name.to_string(), typed));
        for (p, (_, t)) in info.def.params.iter().zip(&info.params) {
            self.note_decl(p.name.span, t.as_ref().unwrap_or(&Type::Any));
        }
        self.scopes.push(
            info.params.iter().map(|(p, t)| (p.clone(), t.clone().unwrap_or(Type::Any))).collect(),
        );
        let inferred = match &info.result {
            // `: void`: what its last statement makes is not a result, and a
            // `return` with a value is an error.
            Some(Type::Void) => {
                self.results.push(Some((name.to_string(), Type::Void)));
                self.check_statements(&info.def.body.stmts);
                self.results.pop();
                Type::Void
            }
            // Declared: every value it hands back is checked against it.
            Some(declared) => {
                self.results.push(Some((name.to_string(), declared.clone())));
                self.check_statements_for(&info.def.body.stmts, name, declared);
                self.results.pop();
                declared.clone()
            }
            None => {
                self.results.push(None);
                self.returns.push(Vec::new());
                let last = self.check_statements(&info.def.body.stmts);
                let mut results = self.returns.pop().unwrap_or_default();
                self.results.pop();
                results.push(last);
                widen(&Type::union(results))
            }
        };
        self.in_fn.pop();
        self.scopes = saved;
        self.facts = saved_facts;
        self.tparams = saved_tparams;
        if let Some(f) = self.fns.get_mut(&key) {
            f.state = FnState::Done(inferred.clone());
        }
        info.result.unwrap_or(inferred)
    }

    // ----- Closures --------------------------------------------------------

    /// Check a closure's body and return its function type. `expected` is the
    /// shape the place it is used in wants, which is where an unannotated
    /// parameter gets its type from.
    fn closure(&mut self, e: &Expr, expected: Option<(Vec<Type>, Type)>) -> Type {
        let ExprKind::Closure { params: written, body, .. } = &e.kind else { return Type::Any };
        let mut params = Vec::new();
        for (i, p) in written.iter().enumerate() {
            let name = &p.name.name;
            let declared = p.ty.as_ref().and_then(|t| self.annotation(t).ok());
            let from_context = expected.as_ref().and_then(|(p, _)| p.get(i).cloned());
            let ty = match (declared, from_context) {
                (Some(t), _) => t,
                (None, Some(t)) => t,
                (None, None) => {
                    if expected.is_none() {
                        self.warn(
                            e.span,
                            format!(
                                "`{name}` has no type and nothing here says what it will be \
                                 given, so it is not checked. Say what it takes: `({name}: T) => …`"
                            ),
                        );
                    }
                    Type::Any
                }
            };
            self.note_decl(p.name.span, &ty);
            params.push((name.clone(), ty));
        }

        // The body sees the scopes around it, which is where its captured
        // names come from, plus its own parameters.
        self.scopes.push(params.iter().cloned().collect());
        self.returns.push(Vec::new());
        let last = match &body.kind {
            StmtKind::Block(b) => self.check_statements(&b.stmts),
            _ => self.check_statements(std::slice::from_ref(&**body)),
        };
        let mut results = self.returns.pop().unwrap_or_default();
        self.scopes.pop();
        results.push(last);
        let result = Type::union(results);

        if let Some((_, want)) = &expected {
            if !matches!(self.resolve(want), Type::Any | Type::Null | Type::Void) && !self.assignable(&result, want) {
                let message = format!(
                    "this function returns {}, where {} is expected",
                    self.show(&result),
                    self.show(want)
                );
                self.error(e.span, message);
            }
        }
        let ty = Type::Function(params.into_iter().map(|(_, t)| t).collect(), Box::new(widen(&result)));
        self.note(e, &ty);
        ty
    }

    // ----- Methods ---------------------------------------------------------

    fn method(&mut self, base: &Type, name: &str, args: &[Expr]) -> Type {
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
                        self.callback(&args[0], Some((vec![item.clone(), item.clone()], Type::Float)));
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
            Type::Float | Type::Int => match name {
                "to_int" | "trunc" | "floor" | "ceil" | "round" => return Type::Int,
                "to_float" | "toFloat" | "sqrt" => return Type::Float,
                "abs" => return base.clone(),
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
        let t = if matches!(arg.kind, ExprKind::Closure { .. }) { self.closure(arg, shape) } else { self.infer(arg) };
        match self.resolve(&t) {
            Type::Function(_, result) => *result,
            _ => Type::Any,
        }
    }
}

/// A type as written, as the checker's [`Type`].
impl Checker<'_> {
    /// An annotation inside the body being checked, which reads the
    /// function's type parameters as [`Type::Param`]s.
    fn annotation(&self, ty: &TypeExpr) -> Result<Type, String> {
        type_of_expr(ty).map(|t| t.with_params(&self.tparams))
    }
}

fn type_of_expr(ty: &TypeExpr) -> Result<Type, String> {
    parse_type(&print::ty(ty)).map_err(|e| e.message)
}

/// Every annotation in `stmts`, in the order written: `let`s, parameters,
/// results and closures' parameters, not `type` declarations.
fn annotations_in<'s>(stmts: &'s [Stmt], out: &mut Vec<&'s TypeExpr>) {
    walk_stmts(stmts, &mut |node| {
        match node {
            Node::Stmt(Stmt { kind: StmtKind::Let { ty: Some(t), .. }, .. }) => out.push(t),
            Node::Stmt(Stmt { kind: StmtKind::Fn(def), .. }) => {
                out.extend(def.params.iter().filter_map(|p| p.ty.as_ref()));
                out.extend(def.result.iter());
            }
            Node::Expr(Expr { kind: ExprKind::Closure { params, .. }, .. }) => {
                out.extend(params.iter().filter_map(|p| p.ty.as_ref()));
            }
            _ => {}
        }
        true
    });
}

/// Every name `stmts` declare: parameters, closures' parameters, `let`s and
/// loop variables, flat. See `declared_in` in the crate root, which says the
/// same of the fork's AST.
fn declared_names(stmts: &[Stmt]) -> HashSet<String> {
    let mut names = HashSet::new();
    walk_stmts(stmts, &mut |node| {
        match node {
            Node::Stmt(Stmt { kind: StmtKind::Let { name, .. }, .. }) => {
                names.insert(name.name.clone());
            }
            Node::Stmt(Stmt { kind: StmtKind::For { var, counter, .. }, .. }) => {
                names.insert(var.name.clone());
                if let Some(c) = counter {
                    names.insert(c.name.clone());
                }
            }
            Node::Stmt(Stmt { kind: StmtKind::Fn(def), .. }) => {
                names.extend(def.params.iter().map(|p| p.name.name.clone()));
            }
            Node::Expr(Expr { kind: ExprKind::Closure { params, .. }, .. }) => {
                names.extend(params.iter().map(|p| p.name.name.clone()));
            }
            _ => {}
        }
        true
    });
    names
}

/// Whether a call's callee is the one plain name `name`.
fn is_named(callee: &[Ident], name: &str) -> bool {
    matches!(callee, [one] if one.name == name)
}

/// The operands of a run of one operator, left to right: `a && b && c`.
fn operands<'e>(e: &'e Expr, op: &str) -> Vec<&'e Expr> {
    fn go<'e>(e: &'e Expr, op: &str, out: &mut Vec<&'e Expr>) {
        match &e.kind {
            ExprKind::Binary { op: o, lhs, rhs } if *o == op => {
                go(lhs, op, out);
                go(rhs, op, out);
            }
            _ => out.push(e),
        }
    }
    let mut out = Vec::new();
    go(e, op, &mut out);
    out
}

/// Whether `e` is a step of a chain.
fn is_step(e: &Expr) -> bool {
    matches!(e.kind, ExprKind::Field { .. } | ExprKind::Method { .. } | ExprKind::Index { .. })
}

/// What a chain step is applied to.
fn step_base(e: &Expr) -> Option<&Expr> {
    match &e.kind {
        ExprKind::Field { base, .. } | ExprKind::Index { base, .. } => Some(base),
        ExprKind::Method { recv, .. } => Some(recv),
        _ => None,
    }
}

/// Where a chain step is written: its name, or what it indexes by.
fn step_at(e: &Expr) -> Span {
    match &e.kind {
        ExprKind::Field { name, .. } | ExprKind::Method { name, .. } => name.span,
        ExprKind::Index { index, .. } => index.span,
        _ => e.span,
    }
}

/// The statements of an `if`'s `else`, none when it has none; an `else if`
/// is one statement, the inner `if`.
fn else_stmts(x: &If) -> &[Stmt] {
    match x.otherwise.as_deref() {
        None => &[],
        Some(Stmt { kind: StmtKind::Block(b), .. }) => &b.stmts,
        Some(other) => std::slice::from_ref(other),
    }
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
        "bool" => matches!(t, Type::Bool | Type::BoolLit(_)),
        "array" => matches!(t, Type::Array(_)),
        "map" => matches!(t, Type::Record(_) | Type::Dict(_)),
        "()" => t == Type::Null,
        "f64" | "i64" => matches!(t, Type::Float | Type::Int),
        _ => false,
    }
}

/// Whether a block ends by leaving: a `return`, a `throw`, a `break` or a
/// `continue`, or an `if` both of whose branches do.
fn exits(statements: &[Stmt]) -> bool {
    match statements.last().map(|s| &s.kind) {
        Some(StmtKind::Return(_) | StmtKind::Throw(_) | StmtKind::Break(_) | StmtKind::Continue) => true,
        Some(StmtKind::If(x)) => exits(&x.then.stmts) && exits(else_stmts(x)),
        Some(StmtKind::Block(b)) => exits(&b.stmts),
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
    match &e.kind {
        ExprKind::Var(name) => Some(name.clone()),
        ExprKind::Field { base, .. } | ExprKind::Index { base, .. } => path_step(path_of(base)?, e),
        _ => None,
    }
}

/// `prefix` extended by the step `e`. A string key is the same path as the
/// field of that name, so `m["a"]` and `m.a` share their facts.
fn path_step(prefix: String, e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::Field { name, .. } => Some(format!("{prefix}.{}", name.name)),
        ExprKind::Index { index, .. } => match &index.kind {
            ExprKind::Str(s) => Some(format!("{prefix}.{s}")),
            ExprKind::Var(v) => Some(format!("{prefix}[{v}]")),
            _ => None,
        },
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
    match &e.kind {
        ExprKind::Call { callee, args, .. } if is_named(callee, "signal") && args.len() == 1 => {
            uninformative(&args[0])
        }
        ExprKind::Array(items) if items.is_empty() => Some(Blank::EmptyList),
        ExprKind::Map { entries, .. } if entries.is_empty() => Some(Blank::EmptyMap),
        ExprKind::Unit | ExprKind::Null => Some(Blank::Null),
        ExprKind::Call { callee, .. } if callee.len() > 1 => {
            Some(Blank::Host(callee.last().map(|c| c.name.clone()).unwrap_or_default()))
        }
        _ => None,
    }
}

/// A literal type as the name it would be bound to holds it: `"all"` is a
/// `string` once it is in a variable nobody annotated.
fn widen(ty: &Type) -> Type {
    match ty {
        Type::Literal(_) => Type::String,
        Type::BoolLit(_) => Type::Bool,
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
/// Every declared type `ty` names, with how many type arguments it was given.
fn named_in(ty: &Type, out: &mut Vec<(String, usize)>) {
    match ty {
        Type::Named(n) => out.push((n.clone(), 0)),
        Type::Generic(n, args) => {
            out.push((n.clone(), args.len()));
            args.iter().for_each(|a| named_in(a, out));
        }
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

/// The expressions of `script` that [`Types::of`] says nothing about, as their
/// line in `src` and their text. What the typed IR cannot be built from until
/// it is empty: see step 4 of `docs/11-next.md`.
pub fn untyped(script: &Script, src: &str, types: &Types) -> Vec<(usize, String)> {
    let lines = LineIndex::new(src);
    let mut out = Vec::new();
    rux_syntax::visit::exprs(script, &mut |e| {
        if !types.of.contains_key(&e.id.0) {
            let line = lines.line_col(src, e.span.start as usize).0;
            let text: String = e.span.text(src).chars().take(60).collect();
            out.push((line, text));
        }
        true
    });
    out
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

    /// What [`check_typed`] leaves without a type in `src`.
    fn untyped_in(src: &str) -> Vec<(usize, String)> {
        let engine = Builder::new().build(src).unwrap_or_else(|e| panic!("{src}\n{e:?}"));
        let (_, record) = engine.check_types_typed(None, &Context::default());
        let (script, text) = engine.parsed();
        untyped(script, text, &record.script)
    }

    #[test]
    fn every_expression_is_given_a_type() {
        let src = format!(
            "{TASK}\
             let tasks: Task[] = signal([]);\n\
             let n = signal(0);\n\
             let total: float = 1;\n\
             let label = `n is ${{n + 1}}`;\n\
             fn first<T>(items: T[]): T? {{ items?[0] }}\n\
             fn open(): Task[] {{ tasks.filter(t => !t.done) }}\n\
             fn bump() {{\n\
               n++;\n\
               n += 2;\n\
               let t = first(tasks);\n\
               if t != none && t.note?.length > 0 {{ print(t.title); }}\n\
               for i in 0..n {{ total = total + i.toFloat(); }}\n\
               let s = switch n {{ 1 => \"one\", _ => \"many\" }};\n\
               let m = {{ a: 1, b: [1, 2.5] }};\n\
               let k = if n > 2 {{ -1 }} else {{ m.a }};\n\
               let x = tasks.map(t => t.id).reduce((a, b) => a + b, 0);\n\
               let y = n is int;\n\
               let r = parseInt(\"3\") ?? 0;\n\
             }}\n"
        );
        let missing = untyped_in(&src);
        assert!(missing.is_empty(), "{missing:#?}");
    }

    #[test]
    fn a_program_that_agrees_with_itself_says_nothing() {
        let src = format!(
            "{TASK}\
             type Filter = \"all\" | \"open\" | \"done\";\n\
             let tasks: Task[] = signal([{{ id: 1, title: \"a\", done: false }}]);\n\
             let filter: Filter = signal(\"all\");\n\
             let count: int = 0;\n\
             let n: float = signal(0);\n\
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
        one_error("let n: int = 1.5;", "`float`, where `int` is expected");
        one_error(
            "let n: int = 3; fn f() { n = n + 0.5; }",
            "`n` holds `int`, so it cannot be given `float` here. If it may hold either, say so: `let n: float = …`",
        );
        one_error("let s: string = 1;", "where `string` is expected");
        one_error("let b: bool = \"yes\";", "where `bool` is expected");
        one_error("let xs: int[] = [1, \"a\"];", "where `int` is expected");
        assert!(errors("let n: int = 3; let m: int = -2; let x: float = n;").is_empty());
    }

    /// `int` and `float`, step 3 of `docs/11-next.md`.
    #[test]
    fn an_int_is_whole_and_widens_to_a_float() {
        // A whole-number literal is an `int`, and so is what it starts.
        one_error("let n = signal(0); fn f() { n = n / 2; }", "`/` always makes a `float`, where `int` is expected; `intDiv(a, b)`");
        one_error("let n = signal(0); fn f() { n /= 2; }", "`/=` makes `float`, which does not fit `int`");
        one_error("let n = 1; fn f() { n = 0.5; }", "`n` holds `int`, so it cannot be given `float` here");
        for quiet in [
            "let n = signal(0); fn f() { n = intDiv(n, 2); n += 1; n = n * 3 % 4; n++; n = -n; }",
            "let n: float = signal(0); fn f() { n = n / 2; n = n + 1; n += 0.5; }",
            "let x = signal(0.5); fn f() { x = x * 2; x = 3; }",
            "let n = 2 ** 3; let m: int = n;",
            "let n: int = 7; let f: float = n; let g: float = n + 0.5;",
            "let n: int = max(1, 2) + abs(-3) + 4.5.trunc() + 2.5.round() + 1.5.floor() + 0.5.ceil();",
            "let xs = [1, 2]; let i = 1; let a = xs[i]; let b = xs[1];",
            "let x: float = 1.5; let n: int = x.trunc(); let back: float = n.toFloat();",
            "fn half(x: float): float { x / 2 }\nlet h = half(3);",
        ] {
            assert!(errors(quiet).is_empty(), "{quiet}\n{:?}", errors(quiet));
        }
        one_error("let m: int = max(1, 2.5);", "`float`, where `int` is expected");
        one_error("fn f(): int { 2 ** -1 }", "this is `float`");
        one_error("let xs = [1, 2]; fn f() { xs[1.5] }", "a list is indexed by an `int`, and this is a `float`; `.trunc()`");
        one_error("let xs = [1, 2]; fn f() { xs[\"1\"] }", "a list is indexed by an `int`");
        // Text to a number may not be one.
        one_error("let n: int = parseInt(\"4\");", "`int?`, where `int` is expected");
        assert!(errors("let n: int = parseInt(\"4\") ?? 0; let x: float = parseFloat(\"1.5\") ?? 0.0;").is_empty());
        // `number` is retired, with the way out.
        one_error("let n: number = 1;", "there is no type `number` now");
    }

    /// Generics, step 3.4 of `docs/11-next.md`.
    #[test]
    fn a_generic_function_takes_its_types_from_its_arguments() {
        let src = format!(
            "{TASK}fn first<T>(items: T[]): T? {{ items?[0] }}\n\
             fn pluck<T, U>(items: T[], f: (T) => U): U[] {{ items.map(f) }}\n\
             let tasks: Task[] = [];\n\
             let t: Task? = first(tasks);\n\
             let ids: int[] = pluck(tasks, t => t.id);\n\
             let n: int? = first([1, 2]);\n"
        );
        assert!(findings(&src).is_empty(), "{:#?}", findings(&src));
        one_error(&format!("{src}let s: string? = first(tasks);"), "this is `Task?`, where `string?` is expected");
        one_error(&format!("{src}let u: string[] = pluck(tasks, t => t.id);"), "this is `int[]`, where `string[]` is expected");
        // The closure's parameter is a `Task`, from the list before it.
        one_error(&format!("{src}fn g() {{ pluck(tasks, t => t.titel) }}"), "`Task` has no field `titel`");
        // Two arguments that disagree about `T`.
        one_error(
            &format!("{src}fn pair<T>(a: T, b: T): T[] {{ [a, b] }}\nlet p = pair(1, \"x\");"),
            "`pair` takes `int` as `b`, and this is `\"x\"`",
        );
        let t = table(&src);
        assert_eq!(seen(&t, "fn", "first")[0].ty, "fn first<T>(items: T[]): T?");
    }

    #[test]
    fn a_type_parameter_is_opaque_inside_its_function() {
        one_error("fn f<T>(x: T): int { x.length }", "`T` is a type parameter, so nothing is known of a `length` on it");
        one_error("fn f<T>(x: T): int { x }", "`f` is declared to return `int`, and this is `T`");
        one_error("fn f<T>(x: T): T { x + 1 }", "cannot be applied to `T` and `int`");
        one_error("fn f<T>(x: any): bool { x is T }", "`is` cannot test for `T`");
        assert!(errors("fn same<T>(a: T, b: T): bool { a == b }\nfn wrap<T>(x: T): { value: T } { { value: x } }").is_empty());
        // A type parameter is a type only inside its own function.
        one_error("fn f<T>(x: T) { x }\nfn g(y: T) { y }", "there is no type `T`");
    }

    #[test]
    fn a_generic_type_takes_its_arguments() {
        let page = "type Page<T> = { items: T[], next: string? };\n";
        assert!(errors(&format!("{page}let p: Page<int> = {{ items: [1], next: none }};")).is_empty());
        one_error(&format!("{page}let p: Page<int> = {{ items: [\"a\"], next: none }};"), "where `int` is expected");
        one_error(&format!("{page}let p: Page = {{ items: [], next: none }};"), "`Page` is written with what it holds: `Page<T>`");
        one_error(&format!("{page}let p: Page<int, int> = {{ items: [], next: none }};"), "`Page` takes 1 type argument: `Page<T>`");
        one_error("type Two = { a: int };\nlet p: Two<int> = { a: 1 };", "`Two` takes no type arguments");
        one_error(&format!("{page}fn f(p: Page<int>): string {{ p.items[0] }}"), "this is `int`");
        // A recursive one, and the built-in spellings.
        let clean = "type Tree<T> = { value: T, kids: Tree<T>[] };\n\
                     let t: Tree<int> = { value: 1, kids: [{ value: 2, kids: [] }] };\n\
                     let a: Array<int> = [1];\nlet m: Map<string, bool> = { a: true };\nlet o: Option<string> = none;";
        assert!(errors(clean).is_empty(), "{:?}", errors(clean));
        // Imported, as another file writes it.
        let cx = Context {
            imported_types: vec![("Page".into(), "<T> { items: T[] }".into())],
            ..Context::default()
        };
        let f = findings_with("let p: Page<string> = { items: [1] };", &cx);
        assert!(f.iter().any(|f| f.message.contains("where `string` is expected")), "{f:?}");
    }

    /// `Result`, `void` and `is`, step 3.5 of `docs/11-next.md`.
    #[test]
    fn a_result_is_read_by_its_ok() {
        let age = "fn parseAge(s: string): Result<int, string> {\n\
                     let n = parseInt(s);\n\
                     if n == none { return Err(\"not a number\"); }\n\
                     if n < 0 { return Err(\"negative\"); }\n\
                     Ok(n)\n}\n\
                   let age: int = 0;\nlet problem: string = \"\";\n";
        let src = format!("{age}fn read(s: string) {{ let r = parseAge(s); if r.ok {{ age = r.value; }} else {{ problem = r.error; }} }}");
        assert!(findings(&src).is_empty(), "{:#?}", findings(&src));
        // Unread, either field may be missing.
        one_error(&format!("{age}fn read(s: string) {{ age = parseAge(s).value; }}"), "no field `value`");
        one_error(&format!("{age}fn read(s: string) {{ let r = parseAge(s); if !r.ok {{ age = r.value; }} }}"), "no field `value`");
        // The halves are held to what the function declared.
        one_error("fn f(): Result<int, string> { Ok(\"x\") }", "`f` is declared to return `Result<int, string>`");
        one_error("fn f(): Result<int, string> { Err(1) }", "`f` is declared to return `Result<int, string>`");
        assert!(errors(&format!("{age}let n: int = parseAge(\"4\").unwrap();")).is_empty());
        one_error(&format!("{age}let s: string = parseAge(\"4\").unwrap();"), "this is `int`, where `string` is expected");
        // A generic function passes a `Result` through.
        assert!(errors("fn keep<T>(r: Result<T, string>): Result<T, string> { r }\nlet r: Result<int, string> = keep(Ok(1));").is_empty());
    }

    #[test]
    fn void_returns_nothing() {
        assert!(errors("let n: int = 0;\nfn bump(): void { n += 1; }\nfn early(): void { if n > 3 { return; } n = 0; }").is_empty());
        one_error("fn f(): void { return 1; }", "`f` is declared to return `void`, and this is `int`");
        one_error("fn f(): void { }\nlet x: int = f();", "this is `void`, where `int` is expected");
        // A callback that returns nothing takes any function.
        assert!(errors("fn each(f: (int) => void) { f(1); }\nfn g() { each(n => n + 1); }").is_empty());
        assert!(errors("let a: string? = none; let b: void? = none;").is_empty());
    }

    #[test]
    fn is_binds_as_a_comparison_on_both_sides() {
        // `a == b is int` is `a == (b is int)`: a `bool` compared with `a`.
        one_error("let a = 1; let b = 2; fn f() { a == b is int }", "compares `int` with `bool`");
        assert!(errors("let a = true; let b = 2; fn f(): bool { a == b is int }").is_empty());
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
        one_error(&src, "`label` takes `Task` as `t`, and this is `int`");
        let src = format!("{TASK}let tasks: Task[] = [];\nfn f() {{ tasks.push(5); }}");
        one_error(&src, "where `Task` is expected");
    }

    #[test]
    fn a_result_is_checked_against_its_declaration() {
        one_error("fn f(): int { \"x\" }", "`f` is declared to return `int`, and this is `\"x\"`");
        one_error("fn f(n: int) { if n > 0 { f(n - 1) } else { 0 } }", "`f` calls itself");
        assert!(errors("fn f(n: int): int { if n > 0 { f(n - 1) } else { 0 } }").is_empty());
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
        let src = "let tasks = signal([]);\nlet user = signal(none);\nlet m = signal({});\n\
                   fn f(x) { x }\nlet add = (a, b) => a + b;";
        let f = findings(src);
        assert!(f.iter().all(|f| !f.is_error), "{f:#?}");
        let w = warnings(src);
        assert!(w.iter().any(|m| m.contains("`tasks` starts as an empty list")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("`user` starts as `none`")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("`m` starts as `{}`")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("`x` has no type") && m.contains("fn f(x: T)")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("`a` has no type")), "{w:?}");
        // Annotated, each of them is quiet.
        let quiet = "type U = { name: string };\nlet tasks: string[] = signal([]);\n\
                     let user: U? = signal(none);\nfn f(x: int) { x }\n\
                     let add = (a: float, b: float) => a + b;";
        assert!(findings(quiet).is_empty(), "{:#?}", findings(quiet));
    }

    /// `null` and `()` still mean `none`, with a word about the spelling.
    #[test]
    fn the_old_spellings_of_none_are_read_and_named() {
        let w = warnings("let a: string? = null;
let b: int | null = ();");
        assert_eq!(w.iter().filter(|m| m.contains("`null` is written `none` now")).count(), 2, "{w:?}");
        assert!(w.iter().any(|m| m.contains("`()` is written `none` now")), "{w:?}");
        assert!(errors("let a: string? = null;
let b: int | null = ();").is_empty());
        assert!(findings("let a: string? = none;
let b: int? = none;").is_empty());
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
        assert!(f.iter().any(|f| f.message.contains("`float`, where `string` is expected")), "{f:?}");
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

    /// The user's call, 2026-09-24: a value that may be `none` is read the way
    /// an optional field is, with `?.` or after a check.
    #[test]
    fn a_value_that_may_be_null_is_read_with_a_question_mark() {
        let sel = format!("{TASK}let sel: Task? = signal(none);\n");
        one_error(&format!("{sel}fn f() {{ sel.title }}"), "`sel` may be `none`, so read it as `sel?.title`");
        clean(&format!("{sel}fn f(): string? {{ sel?.title }}"));
        clean(&format!("{sel}fn f(): string {{ if sel != none {{ sel.title }} else {{ \"\" }} }}"));
        clean(&format!("{sel}fn f(): string {{ if sel == none {{ return \"\"; }} sel.title }}"));
        one_error(&format!("{sel}fn f(): string {{ sel?.title }}"), "string");
    }

    #[test]
    fn a_checked_field_is_read_plainly_inside_the_check() {
        clean(&format!("{NOTE}fn f(): string {{ if t?.note != none {{ t.note }} else {{ \"\" }} }}"));
        clean(&format!("{NOTE}fn f(): string {{ if \"note\" in t {{ t.note }} else {{ \"\" }} }}"));
        clean(&format!("{NOTE}fn f(): string {{ if t?.note {{ t.note }} else {{ \"\" }} }}"));
        clean(&format!("{NOTE}fn f(): bool {{ t?.note != none && t.note.length > 0 }}"));
        clean(&format!("{NOTE}fn f(): string {{ if t?.note == none {{ return \"\"; }} t.note }}"));
        // Outside the region, the rule is back.
        one_error(&format!("{NOTE}fn f() {{ if t?.note != none {{ }} t.note }}"), "may be absent");
        one_error(&format!("{NOTE}fn f() {{ if t?.note == none {{ t.note }} }}"), "may be absent");
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
        one_error("let n = 1;\nfn g() { n == \"1\" }", "compares `int` with `\"1\"`");
        clean(&format!("{f}fn g() {{ f == \"open\" || f != none }}"));
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
    fn is_narrows_both_ways() {
        let raw = format!("{TASK}let raw: any = 1;\nlet held: Task[] = [];\n");
        // Where it held, `raw` is a Task; `.titel` is then a finding.
        clean(&format!("{raw}fn f() {{ if raw is Task {{ held.push(raw); raw.title }} }}"));
        one_error(&format!("{raw}fn f() {{ if raw is Task {{ raw.titel }} }}"), "did you mean `title`?");
        let v = "let v: string | { n: int } = \"x\";\n";
        clean(&format!("{v}fn f(): int {{ if v is string {{ 0 }} else {{ v.n }} }}"));
        clean(&format!("{v}fn f(): int {{ if !(v is {{ n: int }}) {{ v.len() }} else {{ v.n }} }}"));
        let b: bool = errors(&format!("{v}let b: bool = v is string;")).is_empty();
        assert!(b, "`is` answers a bool");
    }

    #[test]
    fn is_names_a_type_that_exists() {
        one_error("let x = 1; let b = x is Tsak;", "there is no type `Tsak`");
        one_error(&format!("{TASK}let x = 1; let b = x is Tsak;"), "did you mean `Task`?");
        one_error("let x = 1; let b = x is boolean;", "is not a type");
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
        let f = template_findings(&script, vec![chain("t?.note != none")]);
        assert_eq!(f.len(), 1, "the r-if branch is clean, the r-else is not: {f:?}");
        assert_eq!(f[0].line, Some(4));
        let f = template_findings(&script, vec![chain("t?.note == none")]);
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
        let f = template_findings(script, vec![model("on", "bool"), model("name", "string"), model("n", "float")]);
        assert!(f.is_empty(), "a number field may write into an `int`, cut to a whole number: {f:?}");
        let f = template_findings("let n: string? = none;", vec![model("n", "float")]);
        assert!(f.iter().any(|f| f.message.contains("writes `float` into `n`")), "{f:?}");
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

    fn table(src: &str) -> Table {
        let engine = Builder::new().build(src).unwrap_or_else(|e| panic!("{src}
{e:?}"));
        engine.check_types_recording(&Context::default(), true).1
    }

    fn seen<'t>(t: &'t Table, kind: &str, name: &str) -> Vec<&'t Seen> {
        t.seen.iter().filter(|s| s.kind == kind && s.name == name).collect()
    }

    #[test]
    fn an_ordinary_check_records_nothing() {
        let engine = Builder::new().build(&format!("{TASK}let n = 1;")).unwrap();
        let (_, t) = engine.check_types_recording(&Context::default(), false);
        assert!(t.seen.is_empty() && t.guesses.is_empty());
    }

    #[test]
    fn a_name_is_recorded_where_it_is_read_with_its_fields() {
        let t = table(&format!("{TASK}let sel: Task? = signal(none);
fn f(): string {{ sel?.title ?? \"\" }}"));
        let read = seen(&t, "value", "sel");
        assert_eq!(read.len(), 1, "{:?}", t.seen);
        let read = read[0];
        assert_eq!((read.line, read.ty.as_str(), read.nullable), (3, "Task?", true));
        assert!(read.column.is_some());
        let names: Vec<&str> = read.fields.iter().map(|f| f.0.as_str()).collect();
        assert_eq!(names, ["id", "title", "done", "note"]);
        assert!(read.fields.iter().any(|f| f.0 == "note" && f.2), "note is optional");
        let title = seen(&t, "field", "title");
        assert_eq!(title[0].path.as_deref(), Some("sel.title"));
        // Read through `?.` on a value that may be none, so it may be none too.
        assert_eq!(title[0].ty, "string?");
        assert_eq!(seen(&t, "let", "sel")[0].line, 2);
    }

    #[test]
    fn a_narrowed_name_is_recorded_as_narrowed() {
        let t = table(&format!("{TASK}let sel: Task? = signal(none);
fn f(): string {{ if sel != none {{ sel.title }} else {{ \"\" }} }}"));
        let tys: Vec<&str> = seen(&t, "value", "sel").iter().map(|s| s.ty.as_str()).collect();
        assert!(tys.contains(&"Task"), "{tys:?}");
    }

    #[test]
    fn a_function_is_recorded_with_its_signature() {
        let t = table(&format!("{TASK}fn label(t: Task, i: int): string {{ t.title }}
fn count() {{ 1 }}
fn g() {{ label(#{{ id: 1, title: \"a\", done: false }}, 0) }}"));
        let call = seen(&t, "fn", "label");
        assert!(call.iter().any(|s| s.line == 4 && s.column.is_some()), "{call:?}");
        assert!(call.iter().all(|s| s.ty == "fn label(t: Task, i: int): string"), "{call:?}");
        assert_eq!(seen(&t, "fn", "count")[0].ty, "fn count(): int");
        let param = seen(&t, "param", "t");
        assert_eq!((param[0].line, param[0].ty.as_str(), param[0].column), (2, "Task", None));
    }

    #[test]
    fn an_unannotated_parameter_gets_a_guess_from_its_calls() {
        let t = table("fn shout(s, n) { s }
fn a() { shout(\"hi\", 1) }
fn b() { shout(\"yo\", 2) }");
        assert_eq!(t.guesses.len(), 2, "{:?}", t.guesses);
        assert_eq!((t.guesses[0].param.as_str(), t.guesses[0].ty.as_str(), t.guesses[0].line), ("s", "string", 1));
        assert_eq!((t.guesses[1].param.as_str(), t.guesses[1].ty.as_str()), ("n", "int"));
        // Handed something unknown, or never called: no guess.
        let t = table("let x = signal([]);
fn shout(s) { s }
fn a() { shout(x[0]) }
fn lone(q) { q }");
        assert!(t.guesses.is_empty(), "{:?}", t.guesses);
    }
}
