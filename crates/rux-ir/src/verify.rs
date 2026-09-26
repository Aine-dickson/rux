//! The IR's own rules, checked: what a backend may take for granted.
//!
//! A problem here is a defect in whatever built the IR, never an author's:
//! the checker has already said everything an author needs to hear. So the
//! messages talk about the IR, and a debug build treats any of them as a bug.

use crate::ir::*;
use crate::table::Table;
use crate::types::Type;

/// Every rule `unit` breaks, one sentence each, naming where.
pub fn unit(u: &Unit) -> Vec<String> {
    let mut v = Verifier { u, table: Table::new(&u.types), problems: Vec::new(), frames: Vec::new(), place: String::new() };
    v.unit();
    v.problems
}

struct Frame<'u> {
    locals: &'u [Local],
    /// A closure's captures, when this frame is a closure's.
    captures: Option<&'u [Root]>,
    loops: usize,
    /// What a `return` here must fit. `None` where a `return` says nothing
    /// checkable: a handler, an effect, a closure.
    result: Option<&'u Type>,
    /// An `async fn`'s body, the one frame an `await` may be in.
    is_async: bool,
}

struct Verifier<'u> {
    u: &'u Unit,
    table: Table,
    problems: Vec<String>,
    frames: Vec<Frame<'u>>,
    /// Where in the unit the verifier is, for the message.
    place: String,
}

impl<'u> Verifier<'u> {
    fn problem(&mut self, e: Option<&Expr>, message: String) {
        let at = e.map(|e| format!(" at {}..{}", e.at.start, e.at.end)).unwrap_or_default();
        self.problems.push(format!("{}{at}: {message}", self.place));
    }

    fn unit(&mut self) {
        let u = self.u;
        self.place = "init".into();
        self.body(&u.init, None);
        for c in &u.computeds {
            self.place = format!("computed #{}", c.global.0);
            if c.global.0 as usize >= u.globals.len() {
                self.problem(None, "no such global".into());
            }
            self.body(&c.value, None);
        }
        for (label, bodies) in [("effect", &u.effects), ("mounted", &u.mounted), ("unmounted", &u.unmounted)] {
            for b in bodies {
                self.place = label.into();
                self.body(b, None);
            }
        }
        for f in &u.fns {
            self.place = format!("fn {}", f.name);
            if f.params as usize > f.body.locals.len() {
                self.problem(None, "more parameters than locals".into());
            }
            self.frames.push(Frame { locals: &f.body.locals, captures: None, loops: 0, result: Some(&f.result), is_async: f.is_async });
            self.block(&f.body.block);
            self.frames.pop();
        }
        for p in &u.pieces {
            self.place = format!("piece line {} ({})", p.line, p.what);
            if p.given as usize > p.body.locals.len() {
                self.problem(None, "more given names than locals".into());
            }
            self.body(&p.body, None);
        }
    }

    fn body(&mut self, b: &'u Body, result: Option<&'u Type>) {
        self.frames.push(Frame { locals: &b.locals, captures: None, loops: 0, result, is_async: false });
        self.block(&b.block);
        self.frames.pop();
    }

    fn frame(&mut self) -> &mut Frame<'u> {
        self.frames.last_mut().expect("a frame")
    }

    fn local_ty(&mut self, id: LocalId, e: Option<&Expr>) -> Option<&'u Type> {
        let locals = self.frames.last()?.locals;
        match locals.get(id.0 as usize) {
            Some(l) => Some(&l.ty),
            None => {
                self.problem(e, format!("local #{} is not in a frame of {}", id.0, locals.len()));
                None
            }
        }
    }

    fn root_ty(&mut self, r: Root, e: Option<&Expr>) -> Option<Type> {
        match r {
            Root::Local(id) => self.local_ty(id, e).cloned(),
            Root::Capture(i) => {
                let Some(caps) = self.frames.last().and_then(|f| f.captures) else {
                    self.problem(e, format!("capture ^{i} outside a closure"));
                    return None;
                };
                let Some(&outer) = caps.get(i as usize) else {
                    self.problem(e, format!("capture ^{i} of {}", caps.len()));
                    return None;
                };
                // What it captured, read in the frame around the closure.
                let frame = self.frames.pop().expect("a frame");
                let ty = self.root_ty(outer, e);
                self.frames.push(frame);
                ty
            }
            Root::Global(g) => match self.u.globals.get(g.0 as usize) {
                Some(g) => Some(g.ty.clone()),
                None => {
                    self.problem(e, format!("global #{} of {}", g.0, self.u.globals.len()));
                    None
                }
            },
            Root::Outer(n) => {
                if n as usize >= self.u.outer.len() {
                    self.problem(e, format!("outer name #{n} of {}", self.u.outer.len()));
                }
                Some(Type::Any)
            }
        }
    }

    fn fits(&mut self, what: &str, e: &Expr, want: &Type) {
        if !self.table.assignable(&e.ty, want) {
            self.problem(Some(e), format!("{what} is `{}`, which does not fit `{want}`", e.ty));
        }
    }

    fn block(&mut self, b: &'u Block) {
        for s in &b.stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &'u Stmt) {
        match &s.kind {
            StmtKind::Expr(e) => self.expr(e),
            StmtKind::Let { local, value } => {
                let ty = self.local_ty(*local, None).cloned();
                if let Some(v) = value {
                    self.expr(v);
                    if let Some(ty) = ty {
                        self.fits("a `let`'s value", v, &ty);
                    }
                }
            }
            StmtKind::Init { global, value } => {
                self.expr(value);
                if let Some(ty) = self.root_ty(Root::Global(*global), Some(value)) {
                    self.fits("a global's first value", value, &ty);
                }
            }
            StmtKind::Assign { place, op, value } => {
                self.root_ty(place.root, Some(value));
                for step in &place.steps {
                    if let PlaceStep::Index(i) = step {
                        self.expr(i);
                    }
                }
                self.expr(value);
                if op.is_none() {
                    self.fits("an assigned value", value, &place.ty);
                }
            }
            StmtKind::If { cond, then, otherwise } => {
                self.expr(cond);
                self.block(then);
                if let Some(o) = otherwise {
                    self.block(o);
                }
            }
            StmtKind::While { cond, body } => {
                self.expr(cond);
                self.in_loop(body);
            }
            StmtKind::ForRange { var, from, to, body, .. } => {
                self.local_ty(*var, None);
                for e in [from, to] {
                    self.expr(e);
                    if !self.table.is(&e.ty, &Type::Int) && self.table.resolve(&e.ty) != Type::Any {
                        self.problem(Some(e), format!("a range bound is `{}`, not `int`", e.ty));
                    }
                }
                self.in_loop(body);
            }
            StmtKind::ForEach { var, counter, iter, body, .. } => {
                self.local_ty(*var, None);
                if let Some(c) = counter {
                    self.local_ty(*c, None);
                }
                self.expr(iter);
                self.in_loop(body);
            }
            StmtKind::Break | StmtKind::Continue => {
                if self.frame().loops == 0 {
                    self.problem(None, "`break` or `continue` outside a loop".into());
                }
            }
            StmtKind::Return(v) => {
                if let Some(v) = v {
                    self.expr(v);
                    if let Some(want) = self.frames.last().and_then(|f| f.result) {
                        if !matches!(want, Type::Void) {
                            self.fits("a returned value", v, want);
                        }
                    }
                }
            }
            StmtKind::Throw(v) => self.expr(v),
            StmtKind::Try { body, var, catch } => {
                self.block(body);
                if let Some(v) = var {
                    self.local_ty(*v, None);
                }
                self.block(catch);
            }
        }
    }

    fn in_loop(&mut self, body: &'u Block) {
        self.frame().loops += 1;
        self.block(body);
        self.frame().loops -= 1;
    }

    /// `e`'s type, unfolded.
    fn kind(&self, e: &Expr) -> Type {
        self.table.resolve(&e.ty)
    }

    fn expr(&mut self, e: &'u Expr) {
        match &e.kind {
            ExprKind::None | ExprKind::Bool(_) | ExprKind::Str(_) => {}
            ExprKind::Int(_) => {
                if !self.table.is(&e.ty, &Type::Int) {
                    self.problem(Some(e), format!("an int literal typed `{}`", e.ty));
                }
            }
            ExprKind::Float(_) => {
                if !self.table.is(&e.ty, &Type::Float) {
                    self.problem(Some(e), format!("a float literal typed `{}`", e.ty));
                }
            }
            ExprKind::Template(parts) | ExprKind::Array(parts) => {
                for p in parts {
                    self.expr(p);
                }
            }
            ExprKind::Map(entries) => {
                for (_, v) in entries {
                    self.expr(v);
                }
            }
            ExprKind::Local(id) => {
                if let Some(ty) = self.local_ty(*id, Some(e)).cloned() {
                    self.fits("a read of a local", e, &ty);
                }
            }
            ExprKind::Capture(i) => {
                self.root_ty(Root::Capture(*i), Some(e));
            }
            ExprKind::Global(g) => {
                if let Some(ty) = self.root_ty(Root::Global(*g), Some(e)) {
                    self.fits("a read of a global", e, &ty);
                }
            }
            ExprKind::Outer(n) => {
                self.root_ty(Root::Outer(*n), Some(e));
            }
            ExprKind::Call { callee, args } => self.call(e, callee, args, false),
            ExprKind::Await(call) => {
                if !self.frames.last().is_some_and(|f| f.is_async) {
                    self.problem(Some(e), "an `await` outside an `async fn`'s own body".into());
                }
                match &call.kind {
                    ExprKind::Call { callee: callee @ (Callee::Fn(_) | Callee::Host(_)), args } => {
                        self.call(call, callee, args, true);
                    }
                    _ => {
                        self.problem(Some(e), "an `await` of something that is not a call to an `async fn` or a host function".into());
                        self.expr(call);
                    }
                }
            }
            ExprKind::Start { func, args } => {
                match self.u.fns.get(func.0 as usize) {
                    Some(f) if !f.is_async => self.problem(Some(e), format!("a start of `{}`, which is not `async`", f.name)),
                    Some(f) if f.params as usize != args.len() => {
                        let n = f.params;
                        self.problem(Some(e), format!("`{}` takes {n} arguments, and is given {}", f.name, args.len()));
                    }
                    Some(_) => {}
                    None => self.problem(Some(e), format!("fn #{} of {}", func.0, self.u.fns.len())),
                }
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::Method { recv, args, .. } => {
                self.expr(recv);
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::Field { base, .. } | ExprKind::Chain(base) => self.expr(base),
            ExprKind::Index { base, index, .. } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::Unary { op, expr } => {
                self.expr(expr);
                let want = match op {
                    UnOp::NegInt => Some(Type::Int),
                    UnOp::NegFloat => Some(Type::Float),
                    _ => None,
                };
                if let Some(want) = want {
                    if !self.table.is(&expr.ty, &want) {
                        self.problem(Some(e), format!("{op:?} on `{}`", expr.ty));
                    }
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                self.expr(lhs);
                self.expr(rhs);
                self.binary(e, *op, lhs, rhs);
            }
            ExprKind::Logic { lhs, rhs, .. } | ExprKind::Coalesce { lhs, rhs } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Is { expr, .. } => self.expr(expr),
            ExprKind::Widen(inner) => {
                self.expr(inner);
                if !self.table.is(&inner.ty, &Type::Int) || !self.table.is(&e.ty, &Type::Float) {
                    self.problem(Some(e), format!("a widening from `{}` to `{}`", inner.ty, e.ty));
                }
            }
            ExprKind::Check(inner) => {
                self.expr(inner);
                if !may_be_any(&self.kind(inner)) {
                    self.problem(Some(e), format!("a check on `{}`, which is known", inner.ty));
                }
            }
            ExprKind::If { cond, then, otherwise } => {
                self.expr(cond);
                self.block(then);
                if let Some(o) = otherwise {
                    self.block(o);
                }
            }
            ExprKind::Match { value, arms } => {
                self.expr(value);
                for arm in arms {
                    for p in &arm.patterns {
                        self.expr(p);
                    }
                    if let Some(g) = &arm.guard {
                        self.expr(g);
                    }
                    self.block(&arm.body);
                }
            }
            ExprKind::Block(b) => self.block(b),
            ExprKind::Closure(c) => {
                for r in &c.captures {
                    self.root_ty(*r, Some(e));
                }
                if c.params as usize > c.body.locals.len() {
                    self.problem(Some(e), "a closure with more parameters than locals".into());
                }
                self.frames.push(Frame {
                    locals: &c.body.locals,
                    captures: Some(&c.captures),
                    loops: 0,
                    result: None,
                    is_async: false,
                });
                self.block(&c.body.block);
                self.frames.pop();
            }
            ExprKind::Interval { args, body, .. } => {
                for a in args {
                    self.expr(a);
                }
                self.body(body, None);
            }
        }
    }

    /// A call. A call to an `async fn` is awaited (`awaited`) or is a
    /// [`ExprKind::Start`], never a plain call.
    fn call(&mut self, e: &'u Expr, callee: &'u Callee, args: &'u [Expr], awaited: bool) {
        match callee {
            Callee::Fn(f) => match self.u.fns.get(f.0 as usize) {
                Some(func) if func.is_async && !awaited => {
                    self.problem(Some(e), format!("a plain call of `{}`, which is `async`", func.name));
                }
                Some(func) if func.params as usize != args.len() => {
                    let n = func.params;
                    self.problem(Some(e), format!("`{}` takes {n} arguments, and is given {}", func.name, args.len()));
                }
                Some(_) => {}
                None => self.problem(Some(e), format!("fn #{} of {}", f.0, self.u.fns.len())),
            },
            Callee::Value(v) => self.expr(v),
            Callee::Builtin(_) | Callee::Host(_) | Callee::Dyn(_) => {}
        }
        for a in args {
            self.expr(a);
        }
    }

    fn binary(&mut self, e: &Expr, op: BinOp, lhs: &Expr, rhs: &Expr) {
        let (a, b) = (self.kind(lhs), self.kind(rhs));
        let both = |t: Type| a == t && b == t;
        let text = |t: &Type| matches!(t, Type::String | Type::Literal(_));
        let ok = match op {
            BinOp::AddInt | BinOp::SubInt | BinOp::MulInt | BinOp::RemInt | BinOp::PowInt => both(Type::Int),
            BinOp::AddFloat | BinOp::SubFloat | BinOp::MulFloat | BinOp::Div | BinOp::RemFloat | BinOp::PowFloat => {
                both(Type::Float)
            }
            BinOp::Concat => text(&a) || text(&b),
            BinOp::ConcatArray => matches!(a, Type::Array(_)) && matches!(b, Type::Array(_)),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                (matches!(a, Type::Int | Type::Float) && matches!(b, Type::Int | Type::Float)) || (text(&a) && text(&b))
            }
            // Decided when it runs: an `any`, or a comparison with a side
            // that may be `none`.
            BinOp::Dyn(_) => true,
            BinOp::Eq | BinOp::Ne | BinOp::In | BinOp::NotIn | BinOp::Range | BinOp::RangeInclusive => true,
        };
        if !ok {
            self.problem(Some(e), format!("`{}` on `{}` and `{}`", op.name(), lhs.ty, rhs.ty));
        }
    }
}

/// Whether a value of `ty` may be one whose type is decided when it runs.
fn may_be_any(ty: &Type) -> bool {
    match ty {
        Type::Any | Type::Param(_) => true,
        Type::Union(members) => members.iter().any(may_be_any),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(n: i64) -> Expr {
        Expr::new(ExprKind::Int(n), Type::Int, At::default())
    }

    fn one_fn(result: Type, locals: Vec<Local>, stmts: Vec<StmtKind>) -> Unit {
        let stmts = stmts.into_iter().map(|kind| Stmt { kind, at: At::default() }).collect();
        Unit {
            fns: vec![Func {
                name: "f".into(),
                type_params: Vec::new(),
                params: 0,
                result,
                is_async: false,
                body: Body { locals, block: Block { stmts, ty: None } },
                at: At::default(),
            }],
            ..Unit::default()
        }
    }

    #[test]
    fn a_well_typed_unit_breaks_no_rule() {
        let add = Expr::new(
            ExprKind::Binary { op: BinOp::AddInt, lhs: Box::new(int(1)), rhs: Box::new(int(2)) },
            Type::Int,
            At::default(),
        );
        let widened = Expr::new(ExprKind::Widen(Box::new(add)), Type::Float, At::default());
        let x = Local { name: "x".into(), ty: Type::Float };
        let u = one_fn(
            Type::Float,
            vec![x],
            vec![
                StmtKind::Let { local: LocalId(0), value: Some(widened) },
                StmtKind::Return(Some(Expr::new(ExprKind::Local(LocalId(0)), Type::Float, At::default()))),
            ],
        );
        assert_eq!(unit(&u), Vec::<String>::new());
        assert!(crate::print::unit(&u).contains("(let x#0: float (widen (add.int (int 1) (int 2)):int))"));
    }

    #[test]
    fn what_a_backend_cannot_take_for_granted_is_named() {
        let mixed = Expr::new(
            ExprKind::Binary {
                op: BinOp::AddInt,
                lhs: Box::new(int(1)),
                rhs: Box::new(Expr::new(ExprKind::Float(0.5), Type::Float, At::default())),
            },
            Type::Int,
            At::default(),
        );
        let u = one_fn(
            Type::Int,
            Vec::new(),
            vec![StmtKind::Expr(mixed), StmtKind::Break, StmtKind::Let { local: LocalId(3), value: None }],
        );
        let problems = unit(&u);
        assert_eq!(problems.len(), 3, "{problems:#?}");
        assert!(problems[0].contains("`add.int` on `int` and `float`"), "{problems:#?}");
        assert!(problems[1].contains("outside a loop"), "{problems:#?}");
        assert!(problems[2].contains("local #3"), "{problems:#?}");
    }
}
