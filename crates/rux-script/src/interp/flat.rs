//! An `async fn`'s body, in a form that can stop at an `await` and go on
//! later.
//!
//! Step 6 of `docs/11-next.md`. The tree walker keeps its place on Rust's own
//! stack, so it cannot leave a function half run. An `async fn` is therefore
//! run from a list of [`Op`]s instead, whose place is a number: stopping is
//! keeping that number and the frame, and going on is starting from it again.
//!
//! Only the part of the body an `await` is in becomes ops. A statement with no
//! `await` in it is one [`Op::Run`], which the tree walker runs as it runs
//! everything else, so an `async fn` is as fast as any other where it does
//! not wait.
//!
//! Two passes. Hoisting rewrites the body so that every `await` is a
//! statement of its own, `let t = await call(…)` with the call's arguments
//! already worked out: an `await` inside an expression moves out in front of
//! it, and what the expression read before it is kept in a temporary first,
//! so the order things happen in does not change. `a && await b()` and the
//! other forms that decide whether their right side runs become an `if`.
//! Compiling then turns the statements an `await` is inside (an `if`, a
//! loop, a `try`) into jumps.

use std::rc::Rc;

use rux_ir::ir::*;

use crate::interp::stdlib;
use crate::types::Type;

/// A compiled `async fn`.
#[derive(Debug)]
pub struct Flat {
    pub ops: Vec<Op>,
    /// The function's locals, then the temporaries hoisting and compiling
    /// added.
    pub locals: Vec<Local>,
}

/// Where a loop a [`Op::Run`] is inside goes on a `break` or a `continue`,
/// and how many `try`s were open at its start.
#[derive(Clone, Copy, Debug)]
pub struct LoopAt {
    pub brk: usize,
    pub cont: usize,
    pub tries: usize,
}

#[derive(Debug)]
pub enum Op {
    /// A statement with no `await` in it, run by the tree walker. A `break`
    /// or `continue` it ends in goes to `lp`.
    Run { stmt: Stmt, lp: Option<LoopAt> },
    /// `into = await call`: work out the arguments, and wait for the call.
    Await { into: Option<LocalId>, call: Expr, at: At },
    /// Go on at `otherwise` when `cond` is falsy.
    Test { cond: Expr, otherwise: usize, at: At },
    Jump(usize),
    /// `for var in iter`: its items into `list`, from `idx` 0.
    EachStart { iter: Expr, list: LocalId, idx: LocalId, at: At },
    /// The next item into `var`, or on to `exit` when there is none.
    EachNext { list: LocalId, idx: LocalId, var: LocalId, counter: Option<LocalId>, exit: usize },
    RangeStart { from: Expr, to: Expr, inclusive: bool, cur: LocalId, end: LocalId, at: At },
    RangeNext { cur: LocalId, end: LocalId, var: LocalId, exit: usize },
    /// A `try` opens: an error until the matching [`Op::TryExit`] goes to
    /// `catch`, with the error in `var`.
    TryEnter { catch: usize, var: Option<LocalId> },
    TryExit,
    Return { value: Option<Expr>, at: At },
    /// One arm of a `switch` on `value`: on to `next` unless it matches.
    Arm { value: LocalId, patterns: Vec<Expr>, guard: Option<Expr>, next: usize, at: At },
}

/// Compile `f`, an `async fn`. An `Err` names what cannot be compiled yet.
///
/// With `force`, every `if`, loop, `try` and `switch` statement becomes ops
/// whether an `await` is in it or not: how debug builds under `RUX_FLAT=1`
/// run every function through these ops, to prove them on the whole test
/// suite (see [`everything`]).
pub fn build(f: &Func, force: bool) -> Result<Flat, String> {
    let mut body = f.body.block.clone();
    // The body's value is what the function gives, as a `return` would.
    if body.ty.is_some() {
        if let Some(Stmt { kind: StmtKind::Expr(_), .. }) = body.stmts.last() {
            let Some(Stmt { kind: StmtKind::Expr(e), at }) = body.stmts.pop() else { unreachable!() };
            body.stmts.push(Stmt { kind: StmtKind::Return(Some(e)), at });
        }
    }
    let mut h = Hoist { locals: f.body.locals.clone(), problem: None };
    let stmts = h.block(body).stmts;
    if let Some(p) = h.problem {
        return Err(p);
    }
    Ok(compile(stmts, h.locals, force))
}

/// Whether `RUX_FLAT=1` asks, in a debug build, for every function to run
/// from ops.
pub fn everything() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    cfg!(debug_assertions) && *ON.get_or_init(|| std::env::var("RUX_FLAT").is_ok_and(|v| v == "1"))
}

// ----- Finding an `await` ------------------------------------------------------

/// Whether `e` waits: has an `await` in it, not counting a closure's or an
/// interval's body, which run on their own.
pub fn has_await(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Await(_) => true,
        ExprKind::None
        | ExprKind::Bool(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Str(_)
        | ExprKind::Local(_)
        | ExprKind::Capture(_)
        | ExprKind::Global(_)
        | ExprKind::Outer(_)
        | ExprKind::Closure(_)
        | ExprKind::Interval { .. } => false,
        ExprKind::Template(xs) | ExprKind::Array(xs) => xs.iter().any(has_await),
        ExprKind::Map(entries) => entries.iter().any(|(_, v)| has_await(v)),
        ExprKind::Call { callee, args } => {
            matches!(callee, Callee::Value(f) if has_await(f)) || args.iter().any(has_await)
        }
        ExprKind::Start { args, .. } => args.iter().any(has_await),
        ExprKind::Method { recv, args, .. } => has_await(recv) || args.iter().any(has_await),
        ExprKind::Field { base, .. } | ExprKind::Chain(base) => has_await(base),
        ExprKind::Index { base, index, .. } => has_await(base) || has_await(index),
        ExprKind::Unary { expr, .. }
        | ExprKind::Is { expr, .. }
        | ExprKind::Widen(expr)
        | ExprKind::Check(expr) => has_await(expr),
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Logic { lhs, rhs, .. } | ExprKind::Coalesce { lhs, rhs } => {
            has_await(lhs) || has_await(rhs)
        }
        ExprKind::If { cond, then, otherwise } => {
            has_await(cond) || block_has_await(then) || otherwise.as_ref().is_some_and(block_has_await)
        }
        ExprKind::Match { value, arms } => {
            has_await(value)
                || arms.iter().any(|a| {
                    a.patterns.iter().any(has_await) || a.guard.as_ref().is_some_and(has_await) || block_has_await(&a.body)
                })
        }
        ExprKind::Block(b) => block_has_await(b),
    }
}

fn block_has_await(b: &Block) -> bool {
    b.stmts.iter().any(stmt_has_await)
}

fn stmt_has_await(s: &Stmt) -> bool {
    match &s.kind {
        StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::Init { value: e, .. } => has_await(e),
        StmtKind::Let { value, .. } | StmtKind::Return(value) => value.as_ref().is_some_and(has_await),
        StmtKind::Assign { place, value, .. } => {
            has_await(value) || place.steps.iter().any(|s| matches!(s, PlaceStep::Index(i) if has_await(i)))
        }
        StmtKind::If { cond, then, otherwise } => {
            has_await(cond) || block_has_await(then) || otherwise.as_ref().is_some_and(block_has_await)
        }
        StmtKind::While { cond, body } => has_await(cond) || block_has_await(body),
        StmtKind::ForRange { from, to, body, .. } => has_await(from) || has_await(to) || block_has_await(body),
        StmtKind::ForEach { iter, body, .. } => has_await(iter) || block_has_await(body),
        StmtKind::Try { body, catch, .. } => block_has_await(body) || block_has_await(catch),
        StmtKind::Break | StmtKind::Continue => false,
    }
}

// ----- Hoisting ----------------------------------------------------------------

struct Hoist {
    locals: Vec<Local>,
    /// The first thing found that cannot be compiled yet.
    problem: Option<String>,
}

fn stmt(kind: StmtKind, at: At) -> Stmt {
    Stmt { kind, at }
}

impl Hoist {
    fn temp(&mut self, ty: Type) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(Local { name: format!("%{}", id.0), ty });
        id
    }

    fn not_yet(&mut self, what: &str) {
        if self.problem.is_none() {
            self.problem = Some(format!("an `await` {what} is not supported yet"));
        }
    }

    fn block(&mut self, b: Block) -> Block {
        let mut out = Vec::with_capacity(b.stmts.len());
        for s in b.stmts {
            self.stmt(s, &mut out);
        }
        Block { stmts: out, ty: b.ty }
    }

    /// `b`, with its value written to `t` rather than being the block's.
    fn valued(&mut self, b: Block, t: LocalId) -> Block {
        let mut stmts = b.stmts;
        let value = match (b.ty.is_some(), stmts.last()) {
            (true, Some(Stmt { kind: StmtKind::Expr(_), .. })) => stmts.pop(),
            _ => None,
        };
        let mut out = Vec::new();
        for s in stmts {
            self.stmt(s, &mut out);
        }
        if let Some(Stmt { kind: StmtKind::Expr(e), at }) = value {
            let e = self.expr(e, &mut out);
            out.push(stmt(StmtKind::Let { local: t, value: Some(e) }, at));
        }
        Block { stmts: out, ty: None }
    }

    fn stmt(&mut self, s: Stmt, out: &mut Vec<Stmt>) {
        if !stmt_has_await(&s) {
            out.push(s);
            return;
        }
        let at = s.at;
        match s.kind {
            StmtKind::Expr(e) => match e.kind {
                ExprKind::Await(call) => {
                    let call = self.call(*call, out);
                    let e = Expr { kind: ExprKind::Await(Box::new(call)), ..e };
                    out.push(stmt(StmtKind::Expr(e), at));
                }
                ExprKind::If { cond, then, otherwise } => {
                    let cond = self.expr(*cond, out);
                    let then = self.block(Block { ty: None, ..then });
                    let otherwise = otherwise.map(|o| self.block(Block { ty: None, ..o }));
                    out.push(stmt(StmtKind::If { cond, then, otherwise }, at));
                }
                ExprKind::Block(b) => {
                    for s in b.stmts {
                        self.stmt(s, out);
                    }
                }
                ExprKind::Match { value, arms } => {
                    let value = self.expr(*value, out);
                    let arms = self.arms(arms, None);
                    let e = Expr { kind: ExprKind::Match { value: Box::new(value), arms }, ..e };
                    out.push(stmt(StmtKind::Expr(e), at));
                }
                kind => {
                    let e = self.expr(Expr { kind, ..e }, out);
                    if !matches!(e.kind, ExprKind::Local(_)) {
                        out.push(stmt(StmtKind::Expr(e), at));
                    }
                }
            },
            StmtKind::Let { local, value: Some(v) } => {
                let v = match v.kind {
                    ExprKind::Await(call) => {
                        let call = self.call(*call, out);
                        Expr { kind: ExprKind::Await(Box::new(call)), ..v }
                    }
                    kind => self.expr(Expr { kind, ..v }, out),
                };
                out.push(stmt(StmtKind::Let { local, value: Some(v) }, at));
            }
            StmtKind::Assign { place, op, value } => {
                // The place's indexes are worked out before the value, as
                // the tree walker works them out.
                let Place { root, steps, ty } = place;
                let mut indexes = Vec::new();
                let mut shape = Vec::new();
                for s in steps {
                    match s {
                        PlaceStep::Field(n) => shape.push(Some(n)),
                        PlaceStep::Index(i) => {
                            shape.push(None);
                            indexes.push(i);
                        }
                    }
                }
                indexes.push(value);
                let mut done = self.operands(indexes, out).into_iter();
                let steps = shape
                    .into_iter()
                    .map(|s| match s {
                        Some(n) => PlaceStep::Field(n),
                        None => PlaceStep::Index(done.next().expect("an index")),
                    })
                    .collect();
                let value = done.next().expect("the value");
                out.push(stmt(StmtKind::Assign { place: Place { root, steps, ty }, op, value }, at));
            }
            StmtKind::If { cond, then, otherwise } => {
                let cond = self.expr(cond, out);
                let then = self.block(then);
                let otherwise = otherwise.map(|o| self.block(o));
                out.push(stmt(StmtKind::If { cond, then, otherwise }, at));
            }
            StmtKind::While { cond, body } => {
                if has_await(&cond) {
                    // `while c { … }` as `while true { if !c { break } … }`,
                    // so the waiting in `c` happens on every turn.
                    let mut first = Vec::new();
                    let c = self.expr(cond, &mut first);
                    let not = Expr::new(ExprKind::Unary { op: UnOp::Not, expr: Box::new(c) }, Type::Bool, at);
                    first.push(stmt(
                        StmtKind::If { cond: not, then: Block { stmts: vec![stmt(StmtKind::Break, at)], ty: None }, otherwise: None },
                        at,
                    ));
                    first.extend(self.block(body).stmts);
                    let yes = Expr::new(ExprKind::Bool(true), Type::Bool, at);
                    out.push(stmt(StmtKind::While { cond: yes, body: Block { stmts: first, ty: None } }, at));
                } else {
                    let body = self.block(body);
                    out.push(stmt(StmtKind::While { cond, body }, at));
                }
            }
            StmtKind::ForRange { var, from, to, inclusive, body } => {
                let mut ends = self.operands(vec![from, to], out).into_iter();
                let (from, to) = (ends.next().expect("from"), ends.next().expect("to"));
                let body = self.block(body);
                out.push(stmt(StmtKind::ForRange { var, from, to, inclusive, body }, at));
            }
            StmtKind::ForEach { var, counter, iter, over, body } => {
                let iter = self.expr(iter, out);
                let body = self.block(body);
                out.push(stmt(StmtKind::ForEach { var, counter, iter, over, body }, at));
            }
            StmtKind::Return(Some(v)) => {
                let v = self.expr(v, out);
                out.push(stmt(StmtKind::Return(Some(v)), at));
            }
            StmtKind::Throw(v) => {
                let v = self.expr(v, out);
                out.push(stmt(StmtKind::Throw(v), at));
            }
            StmtKind::Try { body, var, catch } => {
                let body = self.block(body);
                let catch = self.block(catch);
                out.push(stmt(StmtKind::Try { body, var, catch }, at));
            }
            StmtKind::Init { global, value } => {
                let value = self.expr(value, out);
                out.push(stmt(StmtKind::Init { global, value }, at));
            }
            kind @ (StmtKind::Let { value: None, .. } | StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue) => {
                out.push(stmt(kind, at))
            }
        }
    }

    /// A `switch`'s arms, each body writing its value to `into` when given.
    fn arms(&mut self, arms: Vec<Arm>, into: Option<LocalId>) -> Vec<Arm> {
        arms.into_iter()
            .map(|a| {
                if a.patterns.iter().any(has_await) || a.guard.as_ref().is_some_and(has_await) {
                    self.not_yet("in a `switch` pattern or guard");
                }
                let body = match into {
                    Some(t) => self.valued(a.body, t),
                    None => self.block(Block { ty: None, ..a.body }),
                };
                Arm { patterns: a.patterns, guard: a.guard, body }
            })
            .collect()
    }

    /// `call`, an awaited call, with its arguments worked out in front of it.
    fn call(&mut self, call: Expr, out: &mut Vec<Stmt>) -> Expr {
        match call.kind {
            ExprKind::Call { callee, args } => {
                let args = self.operands(args, out);
                Expr { kind: ExprKind::Call { callee, args }, ..call }
            }
            _ => call,
        }
    }

    /// `items`, evaluated in order, with no `await` left in any: each before
    /// the last one that waits is kept in a temporary first, since what it
    /// reads could change while the later one waits.
    fn operands(&mut self, items: Vec<Expr>, out: &mut Vec<Stmt>) -> Vec<Expr> {
        let Some(last) = items.iter().rposition(has_await) else { return items };
        items
            .into_iter()
            .enumerate()
            .map(|(i, x)| {
                if i > last {
                    return x;
                }
                let x = self.expr(x, out);
                if i < last { self.spill(x, out) } else { x }
            })
            .collect()
    }

    /// `x` kept in a temporary, unless nothing can change it.
    fn spill(&mut self, x: Expr, out: &mut Vec<Stmt>) -> Expr {
        if matches!(
            x.kind,
            ExprKind::None | ExprKind::Bool(_) | ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Str(_) | ExprKind::Local(_)
        ) {
            return x;
        }
        let t = self.temp(x.ty.clone());
        let (ty, at) = (x.ty.clone(), x.at);
        out.push(stmt(StmtKind::Let { local: t, value: Some(x) }, at));
        Expr::new(ExprKind::Local(t), ty, at)
    }

    /// `e` with every `await` in it moved out in front, into `out`.
    fn expr(&mut self, e: Expr, out: &mut Vec<Stmt>) -> Expr {
        if !has_await(&e) {
            return e;
        }
        let Expr { ty, kind, at } = e;
        let local = |t: LocalId, ty: Type| Expr::new(ExprKind::Local(t), ty, at);
        let kind = match kind {
            ExprKind::Await(call) => {
                let call = self.call(*call, out);
                let t = self.temp(ty.clone());
                let waited = Expr::new(ExprKind::Await(Box::new(call)), ty.clone(), at);
                out.push(stmt(StmtKind::Let { local: t, value: Some(waited) }, at));
                return local(t, ty);
            }
            ExprKind::Template(parts) => ExprKind::Template(self.operands(parts, out)),
            ExprKind::Array(items) => ExprKind::Array(self.operands(items, out)),
            ExprKind::Map(entries) => {
                let (keys, values): (Vec<String>, Vec<Expr>) = entries.into_iter().unzip();
                ExprKind::Map(keys.into_iter().zip(self.operands(values, out)).collect())
            }
            ExprKind::Call { callee: Callee::Value(f), args } => {
                let mut all = vec![*f];
                all.extend(args);
                let mut all = self.operands(all, out);
                let f = all.remove(0);
                ExprKind::Call { callee: Callee::Value(Box::new(f)), args: all }
            }
            ExprKind::Call { callee, args } => ExprKind::Call { callee, args: self.operands(args, out) },
            ExprKind::Start { func, args } => ExprKind::Start { func, args: self.operands(args, out) },
            ExprKind::Method { recv, method, args, optional } => {
                if chain_has_optional(&recv) && args.iter().any(has_await) {
                    self.not_yet("after a `?.` in the same chain");
                }
                if stdlib::mutates(&method.name) && !optional && !has_await(&recv) {
                    // The receiver is the place the method changes: it stays
                    // a place, and only the arguments move.
                    ExprKind::Method { recv, method, args: self.operands(args, out), optional }
                } else {
                    let mut all = vec![*recv];
                    all.extend(args);
                    let mut all = self.operands(all, out);
                    let recv = all.remove(0);
                    ExprKind::Method { recv: Box::new(recv), method, args: all, optional }
                }
            }
            ExprKind::Field { base, name, optional } => {
                ExprKind::Field { base: Box::new(self.expr(*base, out)), name, optional }
            }
            ExprKind::Index { base, index, optional } => {
                if chain_has_optional(&base) && has_await(&index) {
                    self.not_yet("after a `?.` in the same chain");
                }
                let mut both = self.operands(vec![*base, *index], out).into_iter();
                let (base, index) = (both.next().expect("base"), both.next().expect("index"));
                ExprKind::Index { base: Box::new(base), index: Box::new(index), optional }
            }
            ExprKind::Chain(inner) => ExprKind::Chain(Box::new(self.expr(*inner, out))),
            ExprKind::Unary { op, expr } => ExprKind::Unary { op, expr: Box::new(self.expr(*expr, out)) },
            ExprKind::Is { expr, ty: tested } => ExprKind::Is { expr: Box::new(self.expr(*expr, out)), ty: tested },
            ExprKind::Widen(x) => ExprKind::Widen(Box::new(self.expr(*x, out))),
            ExprKind::Check(x) => ExprKind::Check(Box::new(self.expr(*x, out))),
            ExprKind::Binary { op, lhs, rhs } => {
                let mut both = self.operands(vec![*lhs, *rhs], out).into_iter();
                let (lhs, rhs) = (both.next().expect("lhs"), both.next().expect("rhs"));
                ExprKind::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }
            }
            ExprKind::Logic { and, lhs, rhs } => {
                // `t = bool(lhs); if t (or !t) { t = bool(rhs) }`: the right
                // side, and its waiting, only when it decides.
                let lhs = self.expr(*lhs, out);
                let t = self.temp(Type::Bool);
                let as_bool = |x: Expr, and: bool| {
                    let flag = Expr::new(ExprKind::Bool(and), Type::Bool, at);
                    Expr::new(ExprKind::Logic { and, lhs: Box::new(x), rhs: Box::new(flag) }, Type::Bool, at)
                };
                out.push(stmt(StmtKind::Let { local: t, value: Some(as_bool(lhs, and)) }, at));
                let mut then = Vec::new();
                let rhs = self.expr(*rhs, &mut then);
                then.push(stmt(StmtKind::Let { local: t, value: Some(as_bool(rhs, true)) }, at));
                let read = local(t, Type::Bool);
                let cond = if and {
                    read
                } else {
                    Expr::new(ExprKind::Unary { op: UnOp::Not, expr: Box::new(read) }, Type::Bool, at)
                };
                out.push(stmt(StmtKind::If { cond, then: Block { stmts: then, ty: None }, otherwise: None }, at));
                return local(t, Type::Bool);
            }
            ExprKind::Coalesce { lhs, rhs } => {
                let lhs = self.expr(*lhs, out);
                let t = self.temp(ty.clone());
                out.push(stmt(StmtKind::Let { local: t, value: Some(lhs) }, at));
                let mut then = Vec::new();
                let rhs = self.expr(*rhs, &mut then);
                then.push(stmt(StmtKind::Let { local: t, value: Some(rhs) }, at));
                let none = Expr::new(ExprKind::None, Type::Null, at);
                let is_none = Expr::new(
                    ExprKind::Binary { op: BinOp::Eq, lhs: Box::new(local(t, ty.clone())), rhs: Box::new(none) },
                    Type::Bool,
                    at,
                );
                out.push(stmt(StmtKind::If { cond: is_none, then: Block { stmts: then, ty: None }, otherwise: None }, at));
                return local(t, ty);
            }
            ExprKind::If { cond, then, otherwise } => {
                let cond = self.expr(*cond, out);
                let t = self.temp(ty.clone());
                out.push(stmt(StmtKind::Let { local: t, value: None }, at));
                let then = self.valued(then, t);
                let otherwise = otherwise.map(|o| self.valued(o, t));
                out.push(stmt(StmtKind::If { cond, then, otherwise }, at));
                return local(t, ty);
            }
            ExprKind::Match { value, arms } => {
                let value = self.expr(*value, out);
                let t = self.temp(ty.clone());
                out.push(stmt(StmtKind::Let { local: t, value: None }, at));
                let arms = self.arms(arms, Some(t));
                let m = Expr::new(ExprKind::Match { value: Box::new(value), arms }, Type::Null, at);
                out.push(stmt(StmtKind::Expr(m), at));
                return local(t, ty);
            }
            ExprKind::Block(b) => {
                let t = self.temp(ty.clone());
                out.push(stmt(StmtKind::Let { local: t, value: None }, at));
                out.extend(self.valued(b, t).stmts);
                return local(t, ty);
            }
            other => other,
        };
        Expr { ty, kind, at }
    }
}

/// Whether a chain below `e` has a `?.` or `?[` in it.
fn chain_has_optional(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Field { base, optional, .. } | ExprKind::Index { base, optional, .. } => {
            *optional || chain_has_optional(base)
        }
        ExprKind::Method { recv, optional, .. } => *optional || chain_has_optional(recv),
        ExprKind::Chain(inner) => chain_has_optional(inner),
        _ => false,
    }
}

// ----- Compiling ---------------------------------------------------------------

/// A jump target not placed yet: the index of a label.
struct Compiler {
    ops: Vec<Op>,
    /// Each label's op, once placed.
    labels: Vec<usize>,
    loops: Vec<LoopAt>,
    tries: usize,
    locals: Vec<Local>,
    /// Compile every control statement, not only those that wait.
    force: bool,
}

const UNPLACED: usize = usize::MAX;

fn compile(stmts: Vec<Stmt>, locals: Vec<Local>, force: bool) -> Flat {
    let mut c = Compiler { ops: Vec::new(), labels: Vec::new(), loops: Vec::new(), tries: 0, locals, force };
    for s in stmts {
        c.stmt(s);
    }
    // Falling off the end gives `none`.
    c.ops.push(Op::Return { value: None, at: At::default() });
    // Labels to places.
    let place = |l: usize, labels: &[usize]| labels[l];
    let labels = std::mem::take(&mut c.labels);
    for op in &mut c.ops {
        match op {
            Op::Run { lp: Some(lp), .. } => {
                lp.brk = place(lp.brk, &labels);
                lp.cont = place(lp.cont, &labels);
            }
            Op::Test { otherwise: to, .. }
            | Op::Jump(to)
            | Op::EachNext { exit: to, .. }
            | Op::RangeNext { exit: to, .. }
            | Op::TryEnter { catch: to, .. }
            | Op::Arm { next: to, .. } => *to = place(*to, &labels),
            _ => {}
        }
    }
    Flat { ops: c.ops, locals: c.locals }
}

impl Compiler {
    fn label(&mut self) -> usize {
        self.labels.push(UNPLACED);
        self.labels.len() - 1
    }

    fn place(&mut self, l: usize) {
        self.labels[l] = self.ops.len();
    }

    fn temp(&mut self, ty: Type) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(Local { name: format!("%{}", id.0), ty });
        id
    }

    fn stmts(&mut self, stmts: Vec<Stmt>) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn in_loop(&mut self, brk: usize, cont: usize, body: Block) {
        self.loops.push(LoopAt { brk, cont, tries: self.tries });
        self.stmts(body.stmts);
        self.loops.pop();
    }

    fn stmt(&mut self, s: Stmt) {
        let control = matches!(
            &s.kind,
            StmtKind::If { .. }
                | StmtKind::While { .. }
                | StmtKind::ForEach { .. }
                | StmtKind::ForRange { .. }
                | StmtKind::Try { .. }
                | StmtKind::Return(_)
                | StmtKind::Expr(Expr { kind: ExprKind::Match { .. }, .. })
        );
        if !(self.force && control) && !stmt_has_await(&s) {
            let lp = self.loops.last().copied();
            self.ops.push(Op::Run { stmt: s, lp });
            return;
        }
        let at = s.at;
        match s.kind {
            StmtKind::Let { local, value: Some(Expr { kind: ExprKind::Await(call), .. }) } => {
                self.ops.push(Op::Await { into: Some(local), call: *call, at });
            }
            StmtKind::Expr(Expr { kind: ExprKind::Await(call), .. }) => {
                self.ops.push(Op::Await { into: None, call: *call, at });
            }
            StmtKind::Expr(Expr { kind: ExprKind::Match { value, arms }, .. }) => {
                let v = self.temp(value.ty.clone());
                self.ops.push(Op::Run { stmt: stmt(StmtKind::Let { local: v, value: Some(*value) }, at), lp: None });
                let end = self.label();
                for arm in arms {
                    let next = self.label();
                    self.ops.push(Op::Arm { value: v, patterns: arm.patterns, guard: arm.guard, next, at });
                    self.stmts(arm.body.stmts);
                    self.ops.push(Op::Jump(end));
                    self.place(next);
                }
                self.place(end);
            }
            StmtKind::If { cond, then, otherwise } => {
                let other = self.label();
                self.ops.push(Op::Test { cond, otherwise: other, at });
                self.stmts(then.stmts);
                match otherwise {
                    Some(o) => {
                        let end = self.label();
                        self.ops.push(Op::Jump(end));
                        self.place(other);
                        self.stmts(o.stmts);
                        self.place(end);
                    }
                    None => self.place(other),
                }
            }
            StmtKind::While { cond, body } => {
                let (top, exit) = (self.label(), self.label());
                self.place(top);
                self.ops.push(Op::Test { cond, otherwise: exit, at });
                self.in_loop(exit, top, body);
                self.ops.push(Op::Jump(top));
                self.place(exit);
            }
            StmtKind::ForEach { var, counter, iter, body, .. } => {
                let (list, idx) = (self.temp(Type::Any), self.temp(Type::Int));
                self.ops.push(Op::EachStart { iter, list, idx, at });
                let (next, exit) = (self.label(), self.label());
                self.place(next);
                self.ops.push(Op::EachNext { list, idx, var, counter, exit });
                self.in_loop(exit, next, body);
                self.ops.push(Op::Jump(next));
                self.place(exit);
            }
            StmtKind::ForRange { var, from, to, inclusive, body } => {
                let (cur, end) = (self.temp(Type::Int), self.temp(Type::Int));
                self.ops.push(Op::RangeStart { from, to, inclusive, cur, end, at });
                let (next, exit) = (self.label(), self.label());
                self.place(next);
                self.ops.push(Op::RangeNext { cur, end, var, exit });
                self.in_loop(exit, next, body);
                self.ops.push(Op::Jump(next));
                self.place(exit);
            }
            StmtKind::Try { body, var, catch } => {
                let (handler, end) = (self.label(), self.label());
                self.ops.push(Op::TryEnter { catch: handler, var });
                self.tries += 1;
                self.stmts(body.stmts);
                self.tries -= 1;
                self.ops.push(Op::TryExit);
                self.ops.push(Op::Jump(end));
                self.place(handler);
                self.stmts(catch.stmts);
                self.place(end);
            }
            StmtKind::Return(value) => self.ops.push(Op::Return { value, at }),
            // Hoisting leaves every other statement with an `await` in it
            // as one of the above.
            kind => {
                let lp = self.loops.last().copied();
                self.ops.push(Op::Run { stmt: stmt(kind, at), lp });
            }
        }
    }
}

pub type Code = Rc<Flat>;
