//! A checked script, lowered to the typed IR.
//!
//! Step 4.2 of `docs/11-next.md`. The input is a file's whole script as Rux's
//! parser reads it (its `computed`, `prop` and lifecycle declarations
//! included), and the [`Record`] the checker kept while checking it. The
//! output is a [`Unit`], which `rux_ir::verify` holds to the IR's rules.
//!
//! Lowering decides nothing the checker did not: every type comes from the
//! record. What it adds is resolution (each name to its slot), the choice of
//! operator by operand type, and the conversions the types call for.

use std::collections::HashMap;

use rux_ir::ir::{self, *};
use rux_ir::table::Table;
use rux_syntax::ast::{self as ast, ExprKind as E, StmtKind as S, TemplatePart};
use rux_syntax::Span;

use crate::check::{type_of_expr, Record, Types};
use crate::types::Type;

/// The functions the language gives every file, by name. A call to a name
/// that is none of these, and no function or value the file declares, is
/// one the IR cannot resolve.
const BUILTINS: &[&str] = &[
    "print", "debug", "emit", "navigate", "replace", "back", "forward", "blur", "query", "setInterval",
    "clearInterval", "__interval", "Ok", "Err", "parseInt", "parseFloat", "Number", "String", "isNaN", "intDiv",
    "toFloat", "trunc", "floor", "ceil", "round", "abs", "min", "max", "sqrt", "sin", "cos", "tan", "atan2", "exp",
    "ln", "log", "log10", "keys", "values", "range", "path_for", "pathFor", "type_of", "to_string", "to_int",
    "to_float", "parse_int", "parse_float", "is_def_var", "is_def_fn",
];

/// Lower `script`, whose text is `src`, as the checker recorded it.
/// `provided` are the names the runtime supplies, with their types.
pub fn lower(script: &ast::Script, src: &str, record: &Record, provided: &[(String, Type)]) -> Unit {
    let mut types: Vec<(String, Vec<String>, Type)> =
        record.types.iter().map(|(n, (p, t))| (n.clone(), p.clone(), t.clone())).collect();
    types.sort_by(|a, b| a.0.cmp(&b.0));
    let table = Table::new(&types);
    let mut l = Lower {
        table,
        unit: Unit { types, ..Unit::default() },
        globals: HashMap::new(),
        fns: HashMap::new(),
        types: &record.script,
        src,
        piece: None,
        frames: Vec::new(),
        tparams: Vec::new(),
    };
    l.declare(script, record, provided);
    l.top_level(script, record);
    for (i, piece) in record.pieces.iter().enumerate() {
        l.types = &piece.types;
        l.src = &piece.src;
        l.piece = Some(i);
        let body = l.isolated(|l| {
            for (name, ty) in &piece.given {
                l.local(name, ty.clone());
            }
            l.block(&piece.script.stmts)
        });
        let given = piece.given.len() as u32;
        l.unit.pieces.push(Piece {
            src: piece.src.clone(),
            line: piece.line,
            what: piece.what.clone(),
            kind: piece.kind,
            body,
            given,
        });
    }
    l.unit
}

/// Text the runtime hands in to run, lowered: see [`lower_piece`].
#[derive(Clone, Debug)]
pub struct Lowered {
    pub body: Body,
    /// The locals its top level declared, by name, in the order declared: a
    /// component's script declares its instance's state this way.
    pub top: Vec<(String, LocalId)>,
    /// What it says that the IR has no node for.
    pub unsupported: Vec<Unsupported>,
}

/// `script`, text the runtime runs in `unit`'s file (a binding, a handler, an
/// effect's body, a component's script), lowered without a checker record:
/// its own expressions are `any`, and every operation on one is decided when
/// it runs. The unit's functions it calls are typed as they always were.
///
/// `given` are the names the runtime hands in with it, by value: an
/// instance's state, an `r-for` row, `event`. They are the body's first
/// locals, in that order. With `globals` false, the unit's state is not
/// visible, which is how a component's script runs, apart from the document.
/// A name nothing declares is added to `unit.outer` and looked up where it
/// runs.
///
/// A top-level `fn` the unit already has is left out: the runtime puts a
/// component's functions into the document's script, and hands in the
/// component's script, functions and all, to make an instance's state.
pub fn lower_piece(unit: &mut Unit, script: &ast::Script, src: &str, given: &[String], globals: bool) -> Lowered {
    let empty = Types::default();
    let table = Table::new(&unit.types);
    let mut l = Lower {
        table,
        unit: Unit {
            globals: unit.globals.clone(),
            outer: std::mem::take(&mut unit.outer),
            ..Unit::default()
        },
        globals: if globals {
            unit.globals.iter().enumerate().map(|(i, g)| (g.name.clone(), GlobalId(i as u32))).collect()
        } else {
            HashMap::new()
        },
        fns: unit
            .fns
            .iter()
            .enumerate()
            .map(|(i, f)| ((f.name.clone(), f.params as usize), FnId(i as u32)))
            .collect(),
        types: &empty,
        src,
        piece: None,
        frames: vec![Frame { locals: Vec::new(), scopes: vec![HashMap::new()], captures: None }],
        tparams: Vec::new(),
    };
    for name in given {
        l.local(name, Type::Any);
    }
    let own = |def: &ast::FnDecl| unit.fns.iter().any(|f| f.name == def.name.name && f.params as usize == def.params.len());
    let mut stmts = Vec::new();
    let mut ty = None;
    let last = script.stmts.len();
    for (i, s) in script.stmts.iter().enumerate() {
        if let S::Fn(def) = &s.kind {
            if own(def) {
                continue;
            }
        }
        let lowered = l.stmt(s, i + 1 == last);
        if i + 1 == last {
            if let [Stmt { kind: StmtKind::Expr(e), .. }] = &lowered[..] {
                ty = Some(e.ty.clone());
            }
        }
        stmts.extend(lowered);
    }
    let frame = l.frames.pop().expect("the piece's frame");
    let mut top: Vec<(String, LocalId)> = frame.scopes[0]
        .iter()
        .filter(|(_, id)| id.0 as usize >= given.len())
        .map(|(n, id)| (n.clone(), *id))
        .collect();
    top.sort_by_key(|(_, id)| *id);
    unit.outer = std::mem::take(&mut l.unit.outer);
    Lowered {
        body: Body { locals: frame.locals, block: Block { stmts, ty } },
        top,
        unsupported: l.unit.unsupported,
    }
}

struct Frame {
    locals: Vec<Local>,
    scopes: Vec<HashMap<String, LocalId>>,
    /// A closure's captures, by name, in the order its body numbers them.
    /// `None` for a frame that sees nothing around it but the file's names.
    captures: Option<Vec<(String, Root)>>,
}

struct Lower<'a> {
    table: Table,
    unit: Unit,
    globals: HashMap<String, GlobalId>,
    fns: HashMap<(String, usize), FnId>,
    /// What the checker kept for the parse being lowered.
    types: &'a Types,
    /// The text of the parse being lowered.
    src: &'a str,
    /// The template piece being lowered, `None` for the script.
    piece: Option<usize>,
    frames: Vec<Frame>,
    /// The type parameters of the function being lowered.
    tparams: Vec<String>,
}

fn at(span: Span) -> At {
    At { start: span.start, end: span.end }
}

impl<'a> Lower<'a> {
    // ----- The file's own names ----------------------------------------------

    fn declare(&mut self, script: &ast::Script, record: &Record, provided: &[(String, Type)]) {
        let decl = |span: Span| record.script.decl.get(&span.start).cloned().unwrap_or(Type::Any);
        for stmt in &script.stmts {
            match &stmt.kind {
                S::Fn(def) => {
                    let key = (def.name.name.clone(), def.params.len());
                    let id = FnId(self.fns.len() as u32);
                    self.fns.entry(key).or_insert(id);
                }
                S::Let { name, value, .. } => {
                    let signal = matches!(&value, Some(ast::Expr { kind: E::Call { callee, .. }, .. })
                        if callee.len() == 1 && callee[0].name == "signal");
                    let kind = if signal { GlobalKind::Signal } else { GlobalKind::Let };
                    self.global(&name.name, decl(name.span), kind);
                }
                S::Computed { name, .. } => self.global(&name.name, decl(name.span), GlobalKind::Computed),
                S::Prop(decls) => {
                    for d in decls {
                        self.global(&d.name.name, decl(d.name.span), GlobalKind::Prop);
                    }
                }
                _ => {}
            }
        }
        for (name, ty) in provided {
            if !self.globals.contains_key(name) {
                self.global(name, ty.clone(), GlobalKind::Provided);
            }
        }
    }

    fn global(&mut self, name: &str, ty: Type, kind: GlobalKind) {
        let id = GlobalId(self.unit.globals.len() as u32);
        self.globals.insert(name.to_string(), id);
        self.unit.globals.push(Global { name: name.to_string(), ty, kind });
    }

    fn top_level(&mut self, script: &ast::Script, record: &Record) {
        // Functions first, in the order their ids were given.
        let mut defs: Vec<&ast::FnDecl> = script
            .stmts
            .iter()
            .filter_map(|s| match &s.kind {
                S::Fn(def) => Some(def),
                _ => None,
            })
            .collect();
        defs.dedup_by_key(|d| (d.name.name.clone(), d.params.len()));
        for def in defs {
            let key = (def.name.name.clone(), def.params.len());
            if self.fns.get(&key).map(|id| id.0 as usize) != Some(self.unit.fns.len()) {
                continue;
            }
            let (_, result) = record.fns.get(&key).cloned().unwrap_or((Vec::new(), Type::Any));
            self.tparams = def.type_params.iter().map(|p| p.name.clone()).collect();
            let body = self.isolated(|l| {
                for p in &def.params {
                    let ty = l.decl(p.name.span);
                    l.bind(&p.name.name, ty);
                }
                l.block(&def.body.stmts)
            });
            self.unit.fns.push(Func {
                name: def.name.name.clone(),
                type_params: std::mem::take(&mut self.tparams),
                params: def.params.len() as u32,
                result,
                body,
                at: at(def.name.span),
            });
        }

        // Then everything else, in order: the init frame holds the top level.
        let mut init = Vec::new();
        self.frames.push(Frame { locals: Vec::new(), scopes: vec![HashMap::new()], captures: None });
        for stmt in &script.stmts {
            match &stmt.kind {
                S::Fn(_) | S::Type { .. } | S::Use(_) | S::Empty => {}
                S::Let { name, value, .. } => {
                    let Some(value) = value else { continue };
                    let global = self.globals[&name.name];
                    let value = self.expr(unwrap_signal(value));
                    init.push(Stmt { kind: StmtKind::Init { global, value }, at: at(stmt.span) });
                }
                S::Prop(decls) => {
                    for d in decls {
                        if let Some(default) = &d.default {
                            let global = self.globals[&d.name.name];
                            let value = self.expr(default);
                            init.push(Stmt { kind: StmtKind::Init { global, value }, at: at(stmt.span) });
                        }
                    }
                }
                S::Computed { name, value, .. } => {
                    let global = self.globals[&name.name];
                    let value = self.isolated(|l| {
                        let e = l.expr(value);
                        let ty = Some(e.ty.clone());
                        Block { stmts: vec![Stmt { kind: StmtKind::Expr(e), at: at(stmt.span) }], ty }
                    });
                    self.unit.computeds.push(Computed { global, value });
                }
                S::Lifecycle { kind, body } => {
                    let b = self.isolated(|l| l.block(&body.stmts));
                    match kind {
                        ast::Lifecycle::Effect => self.unit.effects.push(b),
                        ast::Lifecycle::Mounted => self.unit.mounted.push(b),
                        ast::Lifecycle::Unmounted => self.unit.unmounted.push(b),
                    }
                }
                _ => init.extend(self.stmt(stmt, false)),
            }
        }
        let frame = self.frames.pop().expect("the init frame");
        self.unit.init = Body { locals: frame.locals, block: Block { stmts: init, ty: None } };
    }

    // ----- Frames and names --------------------------------------------------

    /// Run `f` in a frame of its own that sees only the file's names.
    fn isolated(&mut self, f: impl FnOnce(&mut Self) -> Block) -> Body {
        self.frames.push(Frame { locals: Vec::new(), scopes: vec![HashMap::new()], captures: None });
        let block = f(self);
        let frame = self.frames.pop().expect("a frame");
        Body { locals: frame.locals, block }
    }

    fn frame(&mut self) -> &mut Frame {
        self.frames.last_mut().expect("a frame")
    }

    /// A new slot in this frame, not yet visible by name.
    fn local(&mut self, name: &str, ty: Type) -> LocalId {
        let frame = self.frame();
        let id = LocalId(frame.locals.len() as u32);
        frame.locals.push(Local { name: name.to_string(), ty });
        frame.scopes.last_mut().expect("a scope").insert(name.to_string(), id);
        id
    }

    fn bind(&mut self, name: &str, ty: Type) -> LocalId {
        self.local(name, ty)
    }

    fn scoped<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.frame().scopes.push(HashMap::new());
        let out = f(self);
        self.frame().scopes.pop();
        out
    }

    /// The type the checker bound the name written at `span` to.
    fn decl(&self, span: Span) -> Type {
        self.types.decl.get(&span.start).cloned().unwrap_or(Type::Any)
    }

    /// Where the name `name` lives, seen from the innermost frame.
    fn resolve(&mut self, name: &str) -> Option<Root> {
        let depth = self.frames.len();
        self.find(depth - 1, name).or_else(|| self.globals.get(name).map(|g| Root::Global(*g)))
    }

    fn find(&mut self, depth: usize, name: &str) -> Option<Root> {
        let frame = &self.frames[depth];
        for scope in frame.scopes.iter().rev() {
            if let Some(id) = scope.get(name) {
                return Some(Root::Local(*id));
            }
        }
        let captures = frame.captures.as_ref()?;
        if let Some(i) = captures.iter().position(|(n, _)| n == name) {
            return Some(Root::Capture(i as u32));
        }
        if depth == 0 {
            return None;
        }
        match self.find(depth - 1, name)? {
            Root::Global(g) => Some(Root::Global(g)),
            Root::Outer(n) => Some(Root::Outer(n)),
            outer => {
                let captures = self.frames[depth].captures.as_mut().expect("a closure");
                captures.push((name.to_string(), outer));
                Some(Root::Capture(captures.len() as u32 - 1))
            }
        }
    }

    /// The type of what `r` names, seen from the innermost frame.
    fn root_ty(&self, r: Root) -> Type {
        self.root_ty_at(self.frames.len() - 1, r)
    }

    fn root_ty_at(&self, depth: usize, r: Root) -> Type {
        let frame = &self.frames[depth];
        match r {
            Root::Local(id) => frame.locals.get(id.0 as usize).map_or(Type::Any, |l| l.ty.clone()),
            Root::Global(g) => self.unit.globals[g.0 as usize].ty.clone(),
            Root::Outer(_) => Type::Any,
            // What it captured, in the frame around it.
            Root::Capture(i) => match frame.captures.as_ref().and_then(|c| c.get(i as usize)) {
                Some((_, outer)) if depth > 0 => self.root_ty_at(depth - 1, *outer),
                _ => Type::Any,
            },
        }
    }

    /// The index of `name` in the unit's outer names, added if new.
    fn outer(&mut self, name: &str) -> u32 {
        match self.unit.outer.iter().position(|n| n == name) {
            Some(i) => i as u32,
            None => {
                self.unit.outer.push(name.to_string());
                self.unit.outer.len() as u32 - 1
            }
        }
    }

    fn unsupported(&mut self, what: impl Into<String>, span: Span) {
        self.unit.unsupported.push(Unsupported { what: what.into(), at: at(span), piece: self.piece });
    }

    // ----- Types -------------------------------------------------------------

    fn ty(&self, e: &ast::Expr) -> Type {
        self.types.of.get(&e.id.0).cloned().unwrap_or(Type::Any)
    }

    fn resolved(&self, t: &Type) -> Type {
        self.table.resolve(t)
    }

    /// `x` made to fit `want`: an `int` widened where only a `float` will do,
    /// an `any` checked where something known is wanted.
    fn convert(&self, x: ir::Expr, want: Option<&Type>) -> ir::Expr {
        let Some(want) = want else { return x };
        let (got, w) = (self.resolved(&x.ty), self.resolved(want));
        let at = x.at;
        if got == Type::Int && wants_float(&w) {
            return ir::Expr::new(ExprKind::Widen(Box::new(x)), Type::Float, at);
        }
        if got == Type::Any && !may_be_any(&w) && !matches!(w, Type::Null | Type::Void) {
            return ir::Expr::new(ExprKind::Check(Box::new(x)), want.clone(), at);
        }
        x
    }

    /// `x` as a `float`, for an operator that works on two.
    fn as_float(&self, x: ir::Expr) -> ir::Expr {
        if self.resolved(&x.ty) == Type::Int {
            let at = x.at;
            ir::Expr::new(ExprKind::Widen(Box::new(x)), Type::Float, at)
        } else {
            x
        }
    }

    // ----- Statements --------------------------------------------------------

    /// Statements in a scope of their own. The last, when it gives a value
    /// (an expression, an `if`, a `switch`, a block), is the block's value.
    fn block(&mut self, stmts: &[ast::Stmt]) -> Block {
        self.scoped(|l| {
            let mut out = Vec::new();
            let mut ty = None;
            for (i, s) in stmts.iter().enumerate() {
                let last = i + 1 == stmts.len();
                let lowered = l.stmt(s, last);
                if last {
                    if let [Stmt { kind: StmtKind::Expr(e), .. }] = &lowered[..] {
                        ty = Some(e.ty.clone());
                    }
                }
                out.extend(lowered);
            }
            Block { stmts: out, ty }
        })
    }

    fn one(kind: StmtKind, span: Span) -> Vec<Stmt> {
        vec![Stmt { kind, at: at(span) }]
    }

    /// A statement. `value` when it ends a block whose value may be used, so
    /// an `if` or a `switch` there is lowered as the expression it is.
    fn stmt(&mut self, s: &ast::Stmt, value: bool) -> Vec<Stmt> {
        let sp = s.span;
        match &s.kind {
            S::Empty | S::Type { .. } | S::Use(_) => Vec::new(),
            S::Expr(e) => Self::one(StmtKind::Expr(self.expr(e)), sp),
            S::Let { name, value: v, .. } => {
                let ty = self.decl(name.span);
                let v = v.as_ref().map(|v| self.expr(v));
                let local = self.bind(&name.name, ty);
                Self::one(StmtKind::Let { local, value: v }, sp)
            }
            S::Assign { target, op, value: v } => match self.assign(target, op, v) {
                Some(kind) => Self::one(kind, sp),
                None => Vec::new(),
            },
            S::Step { target, up } => {
                let Some(place) = self.place(target) else { return Vec::new() };
                let float = self.resolved(&place.ty) == Type::Float;
                let (op, one) = match (float, up) {
                    (false, true) => (BinOp::AddInt, ExprKind::Int(1)),
                    (false, false) => (BinOp::SubInt, ExprKind::Int(1)),
                    (true, true) => (BinOp::AddFloat, ExprKind::Float(1.0)),
                    (true, false) => (BinOp::SubFloat, ExprKind::Float(1.0)),
                };
                let op = if self.resolved(&place.ty) == Type::Any { BinOp::Dyn(if *up { "+" } else { "-" }) } else { op };
                let one_ty = if float { Type::Float } else { Type::Int };
                let value = ir::Expr::new(one, one_ty, at(target.span));
                Self::one(StmtKind::Assign { place, op: Some(op), value }, sp)
            }
            S::If(x) if value => {
                let e = self.if_expr(x, None, sp);
                Self::one(StmtKind::Expr(e), sp)
            }
            S::If(x) => {
                let cond = self.expr(&x.cond);
                let then = self.block(&x.then.stmts);
                let otherwise = x.otherwise.as_deref().map(|o| self.else_block(o));
                Self::one(StmtKind::If { cond, then, otherwise }, sp)
            }
            S::Switch(sw) => {
                let e = self.switch(sw, None, sp);
                Self::one(StmtKind::Expr(e), sp)
            }
            S::Block(b) => {
                let b = self.block(&b.stmts);
                let ty = b.ty.clone().unwrap_or(Type::Null);
                Self::one(StmtKind::Expr(ir::Expr::new(ExprKind::Block(b), ty, at(sp))), sp)
            }
            S::While { cond: Some(cond), body } => {
                let cond = self.expr(cond);
                let body = self.block(&body.stmts);
                Self::one(StmtKind::While { cond, body }, sp)
            }
            S::While { cond: None, body } => {
                self.unsupported("a `loop`", sp);
                self.block(&body.stmts);
                Vec::new()
            }
            S::Do { body, cond, .. } => {
                self.unsupported("a `do` loop", sp);
                self.block(&body.stmts);
                self.expr(cond);
                Vec::new()
            }
            S::For { var, counter, iter, body } => self.for_loop(var, counter.as_ref(), iter, body, sp),
            S::Break(None) => Self::one(StmtKind::Break, sp),
            S::Break(Some(v)) => {
                self.unsupported("a `break` with a value", sp);
                self.expr(v);
                Self::one(StmtKind::Break, sp)
            }
            S::Continue => Self::one(StmtKind::Continue, sp),
            S::Return(v) => {
                let v = v.as_ref().map(|v| self.expr(v));
                Self::one(StmtKind::Return(v), sp)
            }
            S::Throw(v) => {
                let v = match v {
                    Some(v) => self.expr(v),
                    None => ir::Expr::new(ExprKind::None, Type::Null, at(sp)),
                };
                Self::one(StmtKind::Throw(v), sp)
            }
            S::Try { body, var, catch } => {
                let body = self.block(&body.stmts);
                let (var, catch) = self.scoped(|l| {
                    let var = var.as_ref().map(|v| {
                        let ty = l.decl(v.span);
                        l.bind(&v.name, ty)
                    });
                    (var, l.block(&catch.stmts))
                });
                Self::one(StmtKind::Try { body, var, catch }, sp)
            }
            S::Fn(def) => {
                self.unsupported("a `fn` inside a block", def.name.span);
                Vec::new()
            }
            S::Import { .. } => {
                self.unsupported("an `import … as` of rhai's", sp);
                Vec::new()
            }
            S::Export(_) => {
                self.unsupported("an `export`", sp);
                Vec::new()
            }
            S::Computed { .. } | S::Lifecycle { .. } | S::Prop(_) => {
                self.unsupported("a declaration below the top level", sp);
                Vec::new()
            }
        }
    }

    /// The `else` of an `if`: a block, or another `if`.
    fn else_block(&mut self, o: &ast::Stmt) -> Block {
        match &o.kind {
            S::Block(b) => self.block(&b.stmts),
            _ => self.block(std::slice::from_ref(o)),
        }
    }

    fn if_expr(&mut self, x: &ast::If, ty: Option<Type>, sp: Span) -> ir::Expr {
        let cond = self.expr(&x.cond);
        let then = self.block(&x.then.stmts);
        let otherwise = x.otherwise.as_deref().map(|o| self.else_block(o));
        let ty = ty.unwrap_or_else(|| {
            let a = then.ty.clone().unwrap_or(Type::Null);
            let b = otherwise.as_ref().and_then(|o| o.ty.clone()).unwrap_or(Type::Null);
            Type::union([a, b])
        });
        ir::Expr::new(ExprKind::If { cond: Box::new(cond), then, otherwise }, ty, at(sp))
    }

    fn switch(&mut self, sw: &ast::Switch, ty: Option<Type>, sp: Span) -> ir::Expr {
        let value = self.expr(&sw.value);
        let mut arms = Vec::new();
        for arm in &sw.arms {
            let patterns = arm.patterns.iter().map(|p| self.expr(p)).collect();
            let guard = arm.guard.as_ref().map(|g| self.expr(g));
            let body = self.block(std::slice::from_ref(&*arm.body));
            arms.push(Arm { patterns, guard, body });
        }
        let ty = ty.unwrap_or_else(|| {
            Type::union(arms.iter().map(|a: &Arm| a.body.ty.clone().unwrap_or(Type::Null)))
        });
        ir::Expr::new(ExprKind::Match { value: Box::new(value), arms }, ty, at(sp))
    }

    fn for_loop(
        &mut self,
        var: &ast::Ident,
        counter: Option<&ast::Ident>,
        iter: &ast::Expr,
        body: &ast::Block,
        sp: Span,
    ) -> Vec<Stmt> {
        // `for i in a..b`: a counting loop, not a range made and walked.
        if let E::Binary { op: op @ (".." | "..="), lhs, rhs } = &iter.kind {
            if counter.is_none() {
                let from = self.expr(lhs);
                let to = self.expr(rhs);
                return self.scoped(|l| {
                    let ty = l.decl(var.span);
                    let var = l.bind(&var.name, ty);
                    let body = l.block(&body.stmts);
                    Self::one(StmtKind::ForRange { var, from, to, inclusive: *op == "..=", body }, sp)
                });
            }
        }
        let iter_ir = self.expr(iter);
        let over = match self.resolved(&iter_ir.ty) {
            Type::Array(_) => Iterable::Array,
            Type::String | Type::Literal(_) => Iterable::Chars,
            _ => Iterable::Dyn,
        };
        self.scoped(|l| {
            let ty = l.decl(var.span);
            let v = l.bind(&var.name, ty);
            let c = counter.map(|c| {
                let ty = l.decl(c.span);
                l.bind(&c.name, ty)
            });
            let body = l.block(&body.stmts);
            Self::one(StmtKind::ForEach { var: v, counter: c, iter: iter_ir, over, body }, sp)
        })
    }

    fn place(&mut self, target: &ast::Expr) -> Option<Place> {
        let ty = self.ty(target);
        let mut steps = Vec::new();
        let mut e = target;
        loop {
            match &e.kind {
                E::Var(name) => {
                    let root = match self.resolve(name) {
                        Some(root) => root,
                        None => Root::Outer(self.outer(name)),
                    };
                    steps.reverse();
                    return Some(Place { root, steps, ty });
                }
                E::Field { base, name, .. } => {
                    steps.push(PlaceStep::Field(name.name.clone()));
                    e = base;
                }
                E::Index { base, index, .. } => {
                    let i = self.expr(index);
                    steps.push(PlaceStep::Index(i));
                    e = base;
                }
                _ => {
                    self.unsupported("a write to something that is not a name, a field or an index", e.span);
                    return None;
                }
            }
        }
    }

    fn assign(&mut self, target: &ast::Expr, op: &str, value: &ast::Expr) -> Option<StmtKind> {
        let place = self.place(target)?;
        let v = self.expr(value);
        if op == "=" {
            return Some(StmtKind::Assign { place, op: None, value: v });
        }
        let symbol = op.trim_end_matches('=');
        let held = ir::Expr::new(ExprKind::None, place.ty.clone(), at(target.span));
        let (op, _, v) = self.operator(symbol, held, v, target.span);
        let op = match op {
            Some(op) => op,
            None => return None,
        };
        Some(StmtKind::Assign { place, op: Some(op), value: v })
    }

    // ----- Expressions -------------------------------------------------------

    fn expr(&mut self, e: &ast::Expr) -> ir::Expr {
        let x = self.expr_here(e);
        let want = self.types.want.get(&e.id.0).cloned();
        self.convert(x, want.as_ref())
    }

    fn expr_here(&mut self, e: &ast::Expr) -> ir::Expr {
        let ty = self.ty(e);
        let a = at(e.span);
        let kind = match &e.kind {
            E::Unit | E::Null => ExprKind::None,
            E::Int(n) => ExprKind::Int(*n),
            E::Float(f) => ExprKind::Float(*f),
            E::Str(s) => ExprKind::Str(s.clone()),
            E::Char(c) => ExprKind::Str(c.to_string()),
            E::Bool(b) => ExprKind::Bool(*b),
            E::Template(parts) => {
                let mut out = Vec::new();
                for p in parts {
                    match p {
                        TemplatePart::Text(t) => out.push(ir::Expr::new(ExprKind::Str(t.clone()), Type::String, a)),
                        TemplatePart::Code(b) => {
                            let b = self.block(&b.stmts);
                            let t = b.ty.clone().unwrap_or(Type::Null);
                            out.push(ir::Expr::new(ExprKind::Block(b), t, a));
                        }
                    }
                }
                ExprKind::Template(out)
            }
            E::Array(items) => ExprKind::Array(items.iter().map(|i| self.expr(i)).collect()),
            E::Map { entries, .. } => {
                ExprKind::Map(entries.iter().map(|en| (en.key.name.clone(), self.expr(&en.value))).collect())
            }
            E::Var(name) => match self.resolve(name) {
                Some(Root::Local(id)) => ExprKind::Local(id),
                Some(Root::Capture(i)) => ExprKind::Capture(i),
                Some(Root::Global(g)) => ExprKind::Global(g),
                Some(Root::Outer(n)) => ExprKind::Outer(n),
                None => ExprKind::Outer(self.outer(name)),
            },
            E::Path(_) => {
                self.unsupported("a module path used as a value", e.span);
                ExprKind::None
            }
            E::This => {
                self.unsupported("`this`", e.span);
                ExprKind::None
            }
            E::Call { callee, args, bang } => return self.call(e, callee, args, *bang, ty),
            E::Field { .. } | E::Method { .. } | E::Index { .. } => {
                let x = self.step(e);
                if has_optional(e) {
                    return ir::Expr::new(ExprKind::Chain(Box::new(x)), ty, a);
                }
                return x;
            }
            E::Unary { op, expr } => {
                let x = self.expr(expr);
                let t = self.resolved(&x.ty);
                let op = match (*op, &t) {
                    ("!", _) => UnOp::Not,
                    ("-", Type::Int) => UnOp::NegInt,
                    ("-", Type::Float) => UnOp::NegFloat,
                    ("-", _) => UnOp::NegDyn,
                    ("+", Type::Int | Type::Float) => return x,
                    _ => UnOp::PlusDyn,
                };
                ExprKind::Unary { op, expr: Box::new(x) }
            }
            E::Binary { op, lhs, rhs } => return self.binary(e, op, lhs, rhs, ty),
            E::Is { expr, ty: t } => {
                let x = self.expr(expr);
                // A type that cannot be read fits nothing, as the fork's `is`
                // answered for one, rather than everything.
                let tested = type_of_expr(t)
                    .map(|t| t.with_params(&self.tparams))
                    .unwrap_or_else(|_| Type::Named(rux_syntax::print::ty(t)));
                ExprKind::Is { expr: Box::new(x), ty: tested }
            }
            E::Closure { params, body, .. } => {
                self.frames.push(Frame { locals: Vec::new(), scopes: vec![HashMap::new()], captures: Some(Vec::new()) });
                for p in params {
                    let t = self.decl(p.name.span);
                    self.bind(&p.name.name, t);
                }
                let block = match &body.kind {
                    S::Block(b) => self.block(&b.stmts),
                    _ => self.block(std::slice::from_ref(&**body)),
                };
                let frame = self.frames.pop().expect("the closure's frame");
                let captures = frame.captures.unwrap_or_default().into_iter().map(|(_, r)| r).collect();
                let body = Body { locals: frame.locals, block };
                ExprKind::Closure(std::rc::Rc::new(Closure { params: params.len() as u32, body, captures }))
            }
            E::Interval { args, body } => {
                let args = args.iter().map(|x| self.expr(x)).collect();
                let inner = body.span.start as usize + 1..(body.span.end as usize).saturating_sub(1);
                let text = self.src.get(inner).unwrap_or_default().replace('\r', "");
                let body = self.isolated(|l| l.block(&body.stmts));
                ExprKind::Interval { args, body: Box::new(body), text }
            }
            E::Stmt(s) => {
                return match &s.kind {
                    S::If(x) => self.if_expr(x, Some(ty), e.span),
                    S::Switch(sw) => self.switch(sw, Some(ty), e.span),
                    S::Block(b) => {
                        let b = self.block(&b.stmts);
                        ir::Expr::new(ExprKind::Block(b), ty, a)
                    }
                    _ => {
                        let stmts = self.scoped(|l| l.stmt(s, false));
                        ir::Expr::new(ExprKind::Block(Block { stmts, ty: None }), ty, a)
                    }
                };
            }
        };
        ir::Expr::new(kind, ty, a)
    }

    /// One step of a chain, its base lowered as part of the same chain.
    fn step(&mut self, e: &ast::Expr) -> ir::Expr {
        let ty = self.ty(e);
        let a = at(e.span);
        let base_of = |l: &mut Self, base: &ast::Expr| {
            if is_step(base) {
                l.step(base)
            } else {
                l.expr(base)
            }
        };
        let kind = match &e.kind {
            E::Field { base, name, optional } => {
                ExprKind::Field { base: Box::new(base_of(self, base)), name: name.name.clone(), optional: *optional }
            }
            E::Index { base, index, optional } => {
                let b = base_of(self, base);
                let i = self.expr(index);
                ExprKind::Index { base: Box::new(b), index: Box::new(i), optional: *optional }
            }
            E::Method { recv, name, args, optional } => {
                let r = base_of(self, recv);
                let base = if *optional { without_null(&self.resolved(&r.ty)) } else { self.resolved(&r.ty) };
                let on = match &base {
                    Type::Array(_) => MethodOn::Array,
                    Type::String | Type::Literal(_) => MethodOn::String,
                    Type::Int => MethodOn::Int,
                    Type::Float => MethodOn::Float,
                    Type::Record(_) | Type::Dict(_) => MethodOn::Map,
                    _ if name.name == "unwrap" => MethodOn::Result,
                    _ => MethodOn::Dyn,
                };
                // A `Result` unfolds to a union of records; `unwrap` is its own.
                let on = if name.name == "unwrap" && args.is_empty() { MethodOn::Result } else { on };
                let args = args.iter().map(|x| self.expr(x)).collect();
                let method = Method { on, name: name.name.clone() };
                ExprKind::Method { recv: Box::new(r), method, args, optional: *optional }
            }
            _ => return self.expr(e),
        };
        ir::Expr::new(kind, ty, a)
    }

    fn call(&mut self, e: &ast::Expr, callee: &[ast::Ident], args: &[ast::Expr], bang: bool, ty: Type) -> ir::Expr {
        let a = at(e.span);
        if bang {
            self.unsupported("a `f!()` call", e.span);
        }
        let name = callee.last().map_or("", |c| c.name.as_str());
        if callee.len() == 1 && name == "signal" && args.len() == 1 {
            return self.expr(&args[0]);
        }
        let callee = if callee.len() == 2 && callee[0].name == "host" {
            Callee::Host(name.to_string())
        } else if callee.len() > 1 {
            let path: Vec<&str> = callee.iter().map(|c| c.name.as_str()).collect();
            self.unsupported("a call through a module path", e.span);
            Callee::Dyn(path.join("::"))
        } else if let Some(id) = self.fns.get(&(name.to_string(), args.len())) {
            Callee::Fn(*id)
        } else if BUILTINS.contains(&name) {
            // A function's name is looked up before a variable's, so a
            // built-in wins over state of the same name: `query(".card")`
            // with the router's `query` signal in scope.
            Callee::Builtin(name.to_string())
        } else if let Some(root) = self.resolve(name) {
            let t = self.root_ty(root);
            let kind = match root {
                Root::Local(id) => ExprKind::Local(id),
                Root::Capture(i) => ExprKind::Capture(i),
                Root::Global(g) => ExprKind::Global(g),
                Root::Outer(n) => ExprKind::Outer(n),
            };
            Callee::Value(Box::new(ir::Expr::new(kind, t, a)))
        } else {
            // Found where it runs: an arrow a caller holds, or a function
            // no file this one can see declares.
            Callee::Dyn(name.to_string())
        };
        let args = args.iter().map(|x| self.expr(x)).collect();
        ir::Expr::new(ExprKind::Call { callee, args }, ty, a)
    }

    fn binary(&mut self, e: &ast::Expr, op: &str, lhs: &ast::Expr, rhs: &ast::Expr, ty: Type) -> ir::Expr {
        let a = at(e.span);
        match op {
            "&&" | "||" => {
                let (l, r) = (self.expr(lhs), self.expr(rhs));
                return ir::Expr::new(ExprKind::Logic { and: op == "&&", lhs: Box::new(l), rhs: Box::new(r) }, ty, a);
            }
            "??" => {
                let (l, r) = (self.expr(lhs), self.expr(rhs));
                return ir::Expr::new(ExprKind::Coalesce { lhs: Box::new(l), rhs: Box::new(r) }, ty, a);
            }
            _ => {}
        }
        let (l, r) = (self.expr(lhs), self.expr(rhs));
        match self.operator(op, l, r, e.span) {
            (Some(op), l, r) => ir::Expr::new(ExprKind::Binary { op, lhs: Box::new(l), rhs: Box::new(r) }, ty, a),
            (None, _, _) => ir::Expr::new(ExprKind::None, ty, a),
        }
    }

    /// The operator `op` on `l` and `r`, chosen by their types, with the
    /// operands converted as it needs them. `None` for one the IR has no node
    /// for.
    fn operator(&mut self, op: &str, l: ir::Expr, r: ir::Expr, span: Span) -> (Option<BinOp>, ir::Expr, ir::Expr) {
        let (a, b) = (self.resolved(&l.ty), self.resolved(&r.ty));
        let text = |t: &Type| matches!(t, Type::String | Type::Literal(_));
        let int = |t: &Type| *t == Type::Int;
        let number = |t: &Type| matches!(t, Type::Int | Type::Float);
        let dynamic = may_be_any(&a) || may_be_any(&b);
        let chosen = match op {
            "==" | "===" => Some(BinOp::Eq),
            "!=" | "!==" => Some(BinOp::Ne),
            "in" => Some(BinOp::In),
            "!in" => Some(BinOp::NotIn),
            ".." => Some(BinOp::Range),
            "..=" => Some(BinOp::RangeInclusive),
            "&" | "|" | "^" | "<<" | ">>" => {
                self.unsupported(format!("the bitwise operator `{op}`"), span);
                return (None, l, r);
            }
            _ if dynamic => Some(BinOp::Dyn(static_op(op))),
            "+" if text(&a) || text(&b) => Some(BinOp::Concat),
            "+" if matches!(a, Type::Array(_)) && matches!(b, Type::Array(_)) => Some(BinOp::ConcatArray),
            "+" if int(&a) && int(&b) => Some(BinOp::AddInt),
            "-" if int(&a) && int(&b) => Some(BinOp::SubInt),
            "*" if int(&a) && int(&b) => Some(BinOp::MulInt),
            "%" if int(&a) && int(&b) => Some(BinOp::RemInt),
            "**" if int(&a) && int(&b) && matches!(r.kind, ExprKind::Int(n) if n >= 0) => Some(BinOp::PowInt),
            "+" if number(&a) && number(&b) => Some(BinOp::AddFloat),
            "-" if number(&a) && number(&b) => Some(BinOp::SubFloat),
            "*" if number(&a) && number(&b) => Some(BinOp::MulFloat),
            "/" if number(&a) && number(&b) => Some(BinOp::Div),
            "%" if number(&a) && number(&b) => Some(BinOp::RemFloat),
            "**" if number(&a) && number(&b) => Some(BinOp::PowFloat),
            // Typed only between two numbers or two texts; anything else, a
            // value that may be `none` among them, is compared as it runs.
            "<" | "<=" | ">" | ">=" if !(number(&a) && number(&b)) && !(text(&a) && text(&b)) => {
                Some(BinOp::Dyn(static_op(op)))
            }
            "<" => Some(BinOp::Lt),
            "<=" => Some(BinOp::Le),
            ">" => Some(BinOp::Gt),
            ">=" => Some(BinOp::Ge),
            _ => Some(BinOp::Dyn(static_op(op))),
        };
        let Some(chosen) = chosen else { return (None, l, r) };
        let floats = matches!(
            chosen,
            BinOp::AddFloat | BinOp::SubFloat | BinOp::MulFloat | BinOp::Div | BinOp::RemFloat | BinOp::PowFloat
        ) || (matches!(chosen, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge) && a != b && number(&a) && number(&b));
        if floats {
            (Some(chosen), self.as_float(l), self.as_float(r))
        } else {
            (Some(chosen), l, r)
        }
    }
}

/// `signal(v)` is `v`: what makes a top-level `let` state is the `let`
/// being a signal global, not a call.
fn unwrap_signal(e: &ast::Expr) -> &ast::Expr {
    match &e.kind {
        E::Call { callee, args, .. } if callee.len() == 1 && callee[0].name == "signal" && args.len() == 1 => &args[0],
        _ => e,
    }
}

fn is_step(e: &ast::Expr) -> bool {
    matches!(e.kind, E::Field { .. } | E::Method { .. } | E::Index { .. })
}

/// Whether a chain ending at `e` has a `?.` or `?[` in it.
fn has_optional(e: &ast::Expr) -> bool {
    match &e.kind {
        E::Field { base, optional, .. } | E::Index { base, optional, .. } => *optional || has_optional(base),
        E::Method { recv, optional, .. } => *optional || has_optional(recv),
        _ => false,
    }
}

fn wants_float(w: &Type) -> bool {
    match w {
        Type::Float => true,
        Type::Union(m) => m.contains(&Type::Float) && !m.contains(&Type::Int),
        _ => false,
    }
}

fn may_be_any(t: &Type) -> bool {
    match t {
        Type::Any | Type::Param(_) => true,
        Type::Union(m) => m.iter().any(may_be_any),
        _ => false,
    }
}

fn without_null(t: &Type) -> Type {
    match t {
        Type::Union(m) => Type::union(m.iter().filter(|x| **x != Type::Null).cloned()),
        other => other.clone(),
    }
}

/// An operator's text, for [`BinOp::Dyn`].
fn static_op(op: &str) -> &'static str {
    match op {
        "+" => "+",
        "-" => "-",
        "*" => "*",
        "/" => "/",
        "%" => "%",
        "**" => "**",
        "<" => "<",
        "<=" => "<=",
        ">" => ">",
        ">=" => ">=",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{check_typed, Context};

    /// `src` as a whole script, checked, lowered and verified: its IR as text.
    fn ir(src: &str) -> String {
        ir_with(src, &[])
    }

    fn ir_with(src: &str, provided: &[(String, Type)]) -> String {
        let script = rux_syntax::parse(src, rux_syntax::Options { declarations: true })
            .unwrap_or_else(|e| panic!("{src}\n{e}"));
        let cx = Context { provided: provided.to_vec(), ..Context::default() };
        let (findings, record) = check_typed(&script, src, &cx);
        let errors: Vec<&String> = findings.iter().filter(|f| f.is_error).map(|f| &f.message).collect();
        assert!(errors.is_empty(), "{errors:#?}");
        let unit = lower(&script, src, &record, provided);
        let problems = rux_ir::verify::unit(&unit);
        let text = rux_ir::print::unit(&unit);
        assert!(problems.is_empty(), "{problems:#?}\n{text}");
        text
    }

    #[test]
    fn a_small_file_lowers_to_what_it_means() {
        let text = ir("let n = signal(0);\n\
                       let total: float = signal(1);\n\
                       computed doubled = n * 2;\n\
                       fn bump(by: int) { n += by; total = total + n; }\n\
                       effect { print(`n is ${n}`); }\n");
        let want = "\
global 0 n: int signal
global 1 total: float signal
global 2 doubled: int computed
init
  (init @n (int 0))
  (init @total (widen (int 1)))
computed @doubled
  (mul.int @n:int (int 2)):int
effect
  (call print (template \"n is \" (block { @n:int }):int):string):none
fn 0 bump(by#0: int): none
  (assign @n: int add.int by#0:int)
  (assign @total: float (add.float @total:float (widen @n:int)):float)
";
        assert_eq!(text, want);
    }

    #[test]
    fn every_conversion_is_written() {
        // An `int` where only a `float` will do is widened; `/` widens both
        // sides; an `any` going into typed code is checked.
        let text = ir_with(
            "fn half(x: int): float { x / 2 }\n\
             fn scale(f: float): float { f }\n\
             fn run(): float { let id: int = params.id; scale(id) + half(3) }\n",
            &[("params".to_string(), Type::Any)],
        );
        assert!(text.contains("(div (widen x#0:int) (widen (int 2))):float"), "{text}");
        assert!(text.contains("(let id#0: int (check (field @params:any id):any):int)"), "{text}");
        assert!(text.contains("(call fn:scale (widen id#0:int)):float"), "{text}");
    }

    #[test]
    fn a_closure_captures_what_it_reads_of_the_frame_around_it() {
        let text = ir("type Task = { id: int, done: bool };\n\
                       let tasks: Task[] = signal([]);\n\
                       fn count(min: int): int { let seen = 0; tasks.filter(t => t.id > min).length }\n");
        assert!(text.contains("(closure (t#0: Task) [min#0] { (gt (field t#0:Task id):int ^0:int):bool }"), "{text}");
    }

    #[test]
    fn a_chain_with_a_question_mark_is_one_chain() {
        let text = ir("type Task = { id: int, note?: string };\n\
                       let tasks: Task[] = signal([]);\n\
                       fn note(): int? { tasks?[0]?.note?.length }\n");
        assert!(text.contains("(chain (field? (field? (index? @tasks:Task[] (int 0)):Task note):string? length):int?):int?"), "{text}");
    }

    #[test]
    fn a_comparison_with_a_side_that_may_be_none_is_decided_when_it_runs() {
        let text = ir("fn f(x: int?): bool { x > 0 }\n");
        assert!(text.contains("(dyn> x#0:int? (int 0)):bool"), "{text}");
    }

    #[test]
    fn a_name_no_file_declares_is_looked_up_where_it_runs() {
        let script = rux_syntax::parse("fn f() { caller + 1 }", rux_syntax::Options { declarations: true }).unwrap();
        let (_, record) = check_typed(&script, "fn f() { caller + 1 }", &Context::default());
        let unit = lower(&script, "fn f() { caller + 1 }", &record, &[]);
        assert!(unit.unsupported.is_empty(), "{:?}", unit.unsupported);
        assert_eq!(unit.outer, ["caller"]);
    }
}
