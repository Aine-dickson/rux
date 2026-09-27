//! Typed bodies: step 10, tracks (a) and (b) of `docs/11-next.md`.
//!
//! A function whose parameters, locals and result are all `int`, `float` or
//! `bool`, and whose body does only what those can (arithmetic, comparison,
//! logic, `if`, `while`, a `for` over a range, calls to other such
//! functions), is written a second time as Rust over `i64`, `f64` and `bool`:
//! `t{id}`. Its `f{id}` calls it when the arguments it is handed are those
//! kinds, and runs as before when they are not, so a value the checker could
//! not vouch for (an argument from `any`, a warning the interpreter ran past)
//! meets the code it always met.
//!
//! Nothing but the arguments enters from the dynamic world, so nothing else
//! needs checking. A call from one typed function to another is a Rust call,
//! counted toward the depth a run may nest to as any call is. Steps are
//! ticked where the interpreter ticks them, failures are placed where it
//! places them, and where Rust's own operation stops (an `int` overflow, `%`
//! by 0) the interpreter's operator runs on the same values, so the failure
//! is its own, word for word.

use std::fmt::Write as _;

use rux_ir::ir::{BinOp, Block, Callee, Expr, ExprKind, Func, Root, StmtKind, UnOp, Unit};
use rux_ir::types::Type;

/// What a typed value is in Rust.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum K {
    I,
    F,
    B,
}

impl K {
    fn of(t: &Type) -> Option<K> {
        match t {
            Type::Int => Some(K::I),
            Type::Float => Some(K::F),
            Type::Bool => Some(K::B),
            _ => None,
        }
    }

    pub(crate) fn rust(self) -> &'static str {
        match self {
            K::I => "i64",
            K::F => "f64",
            K::B => "bool",
        }
    }

    /// The `V` a value of this kind is, as a variant's name.
    pub(crate) fn variant(self) -> &'static str {
        match self {
            K::I => "Int",
            K::F => "Float",
            K::B => "Bool",
        }
    }

    fn zero(self) -> &'static str {
        match self {
            K::I => "0i64",
            K::F => "0.0f64",
            K::B => "false",
        }
    }
}

/// A typed function's signature: its parameters' kinds and its result's.
#[derive(Clone, Debug)]
pub(crate) struct Sig {
    pub params: Vec<K>,
    pub result: K,
}

/// Which of `unit`'s functions have a typed body, and the Rust of each.
pub(crate) fn typed(unit: &Unit) -> Vec<Option<(Sig, String)>> {
    let sigs: Vec<Option<Sig>> = unit.fns.iter().map(sig).collect();
    // Assume every candidate qualifies, then drop each whose body does not
    // (a call to one dropped included) until nothing changes.
    let mut ok: Vec<bool> = sigs.iter().map(Option::is_some).collect();
    loop {
        let mut changed = false;
        for (id, f) in unit.fns.iter().enumerate() {
            if ok[id] && T::new(&ok, &sigs).func(id as u32, f).is_none() {
                ok[id] = false;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    unit.fns
        .iter()
        .enumerate()
        .map(|(id, f)| {
            if !ok[id] {
                return None;
            }
            let code = T::new(&ok, &sigs).func(id as u32, f)?;
            Some((sigs[id].clone()?, code))
        })
        .collect()
}

/// The signature a function would have typed: `None` when a parameter,
/// local or its result is not one of the three kinds.
fn sig(f: &Func) -> Option<Sig> {
    if f.is_async || !f.type_params.is_empty() {
        return None;
    }
    let kinds: Option<Vec<K>> = f.body.locals.iter().map(|l| K::of(&l.ty)).collect();
    let kinds = kinds?;
    Some(Sig { params: kinds[..f.params as usize].to_vec(), result: K::of(&f.result)? })
}

struct T<'u> {
    ok: &'u [bool],
    sigs: &'u [Option<Sig>],
    /// The kinds of the function's locals, by slot.
    locals: Vec<K>,
    result: K,
}

impl<'u> T<'u> {
    fn new(ok: &'u [bool], sigs: &'u [Option<Sig>]) -> Self {
        T { ok, sigs, locals: Vec::new(), result: K::I }
    }

    /// `t{id}`, or `None` when anything in it is not typed.
    fn func(&mut self, id: u32, f: &Func) -> Option<String> {
        let sig = self.sigs.get(id as usize)?.clone()?;
        self.locals = f.body.locals.iter().map(|l| K::of(&l.ty)).collect::<Option<_>>()?;
        self.result = sig.result;
        let body = self.block(&f.body.block, 0, false, None, true)?;
        let mut code = format!("\n/// `{}`, typed\nfn t{id}(cx: &mut Cx", f.name.replace('\n', " "));
        for (i, k) in sig.params.iter().enumerate() {
            let _ = write!(code, ", mut l{i}: {}", k.rust());
        }
        let _ = writeln!(code, ") -> R<{}> {{", sig.result.rust());
        // The steps, counted here and handed back before a call, a return
        // or a failure: nothing else reads them while this body runs.
        code.push_str("let (mut __ops, __max) = cx.ops();\n");
        for (i, k) in self.locals.iter().enumerate().skip(sig.params.len()) {
            let _ = writeln!(code, "let mut l{i}: {} = {};", k.rust(), k.zero());
        }
        code.push_str(&body);
        code.push_str("}\n");
        Some(code)
    }

    /// `b`'s statements, as `Gen::block` writes them: with `value`, the
    /// block's value is the function's result; otherwise worked out and
    /// dropped, its failure placed at `at`, the enclosing statement.
    fn block(&mut self, b: &Block, depth: usize, in_loop: bool, at: Option<(u32, u32)>, value: bool) -> Option<String> {
        let mut s = String::new();
        let last = b.stmts.len();
        for (i, st) in b.stmts.iter().enumerate() {
            if i + 1 == last && b.ty.is_some() {
                if let StmtKind::Expr(e) = &st.kind {
                    let (start, end) = at.unwrap_or((st.at.start, st.at.end));
                    let (start, end) = if value { (u32::MAX, u32::MAX) } else { (start, end) };
                    let (x, k) = self.expr(e, start, end)?;
                    match (value, at) {
                        (true, _) if k == self.result => {
                            let _ = writeln!(s, "{}{}", tick(u32::MAX, u32::MAX), ret(&x));
                        }
                        (false, Some(_)) => {
                            let _ = writeln!(s, "{}let _ = {x};", tick(u32::MAX, u32::MAX));
                        }
                        _ => return None,
                    }
                    return Some(s);
                }
            }
            s.push_str(&self.stmt(st, depth, in_loop)?);
        }
        // Falling off the end gives `none`, which no typed result is.
        if value {
            return None;
        }
        Some(s)
    }

    fn stmt(&mut self, st: &rux_ir::ir::Stmt, depth: usize, in_loop: bool) -> Option<String> {
        let (s, e) = (st.at.start, st.at.end);
        let tick = tick(s, e);
        let body = match &st.kind {
            StmtKind::Expr(x) => format!("let _ = {};\n", self.expr(x, s, e)?.0),
            StmtKind::Let { local, value: Some(v) } => {
                let (x, k) = self.expr(v, s, e)?;
                if k != *self.locals.get(local.0 as usize)? {
                    return None;
                }
                format!("l{} = {x};\n", local.0)
            }
            StmtKind::Assign { place, op, value } if place.steps.is_empty() => {
                let Root::Local(l) = place.root else { return None };
                let lk = *self.locals.get(l.0 as usize)?;
                let (v, vk) = self.expr(value, s, e)?;
                let var = format!("l{}", l.0);
                // As `Gen` does it: the tick, the value, then the write.
                let write = match op {
                    None if vk == lk => format!("{var} = __v;\n"),
                    None => return None,
                    Some(op) => {
                        let (x, k) = binary(*op, (&var, lk), ("__v", vk), s, e)?;
                        if k != lk {
                            return None;
                        }
                        format!("{var} = {x};\n")
                    }
                };
                return Some(format!("{tick}let __v = {v};\n{write}"));
            }
            StmtKind::If { cond, then, otherwise } => {
                let c = self.cond(cond, s, e)?;
                let then = self.block(then, depth, in_loop, Some((s, e)), false)?;
                let other = match otherwise {
                    Some(o) => format!(" else {{\n{}}}", self.block(o, depth, in_loop, Some((s, e)), false)?),
                    None => String::new(),
                };
                format!("if {c} {{\n{then}}}{other}\n")
            }
            StmtKind::While { cond, body } => {
                let c = self.cond(cond, s, e)?;
                let body = self.block(body, depth + 1, true, Some((s, e)), false)?;
                format!("loop {{\nif !{c} {{ break; }}\n{body}}}\n")
            }
            StmtKind::ForRange { var, from, to, inclusive, body } => {
                let (a, ak) = self.expr(from, s, e)?;
                let (b, bk) = self.expr(to, s, e)?;
                if (ak, bk, *self.locals.get(var.0 as usize)?) != (K::I, K::I, K::I) {
                    return None;
                }
                let d = depth;
                let inner = self.block(body, depth + 1, true, Some((s, e)), false)?;
                let end = if *inclusive { format!("__hi{d}.saturating_add(1)") } else { format!("__hi{d}") };
                format!(
                    "let __lo{d}: i64 = {a};\nlet __hi{d}: i64 = {b};\n\
                     let __end{d} = {end};\nlet mut __i{d} = __lo{d};\n\
                     while __i{d} < __end{d} {{\nlet __cur{d} = __i{d};\n__i{d} += 1;\n\
                     {}l{} = __cur{d};\n{inner}}}\n",
                    tick,
                    var.0
                )
            }
            StmtKind::Break if in_loop => "break;\n".to_string(),
            StmtKind::Continue if in_loop => "continue;\n".to_string(),
            StmtKind::Return(Some(v)) => {
                let (x, k) = self.expr(v, s, e)?;
                if k != self.result {
                    return None;
                }
                format!("{}\n", ret(&x))
            }
            _ => return None,
        };
        Some(format!("{tick}{body}"))
    }

    /// A condition: only a `bool`, whose truth is itself.
    fn cond(&mut self, c: &Expr, s: u32, e: u32) -> Option<String> {
        match self.expr(c, s, e)? {
            (x, K::B) => Some(format!("({x})")),
            _ => None,
        }
    }

    /// `b` as a Rust block giving its value, as `Gen::block_value` does.
    fn block_value(&mut self, b: &Block, s: u32, end: u32) -> Option<(String, K)> {
        let mut code = String::from("{\n");
        let last = b.stmts.len();
        for (i, st) in b.stmts.iter().enumerate() {
            if i + 1 == last && b.ty.is_some() {
                if let StmtKind::Expr(e) = &st.kind {
                    let (x, k) = self.expr(e, s, end)?;
                    let _ = write!(code, "{}{x}\n}}", tick(s, end));
                    return Some((code, k));
                }
            }
            // No loop around it that the generated code could break out of:
            // as `Gen` does, a `break` in here is not compiled.
            code.push_str(&self.stmt(st, 64, false)?);
        }
        None
    }

    /// `e` as a Rust expression of its kind, failures placed at `s..end`.
    fn expr(&mut self, e: &Expr, s: u32, end: u32) -> Option<(String, K)> {
        let (code, k) = match &e.kind {
            ExprKind::Bool(b) => (format!("{b}"), K::B),
            ExprKind::Int(i) => (format!("{i}i64"), K::I),
            ExprKind::Float(f) => (format!("f64::from_bits({:#x})", f.to_bits()), K::F),
            ExprKind::Local(l) => (format!("l{}", l.0), *self.locals.get(l.0 as usize)?),
            ExprKind::Widen(x) => match self.expr(x, s, end)? {
                (x, K::I) => (format!("({x} as f64)"), K::F),
                _ => return None,
            },
            ExprKind::Check(x) => self.expr(x, s, end)?,
            ExprKind::Unary { op, expr } => {
                let (x, k) = self.expr(expr, s, end)?;
                match (op, k) {
                    (UnOp::Not, K::B) => (format!("(!{x})"), K::B),
                    (UnOp::NegFloat, K::F) => (format!("(-{x})"), K::F),
                    (UnOp::NegInt, K::I) => (
                        format!(
                            "{{ let __u: i64 = {x}; match __u.checked_neg() {{ Some(v) => v, \
                             None => u!(cx, __ops, aot::int_of(cx.unary(UnOp::NegInt, V::Int(__u))), {s}, {end}) }} }}"
                        ),
                        K::I,
                    ),
                    _ => return None,
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let (a, ak) = self.expr(lhs, s, end)?;
                let (b, bk) = self.expr(rhs, s, end)?;
                let (x, k) = binary(*op, ("__l", ak), ("__r", bk), s, end)?;
                (format!("{{ let __l = {a}; let __r = {b}; {x} }}"), k)
            }
            ExprKind::Logic { and, lhs, rhs } => match (self.expr(lhs, s, end)?, self.expr(rhs, s, end)?) {
                ((a, K::B), (b, K::B)) => {
                    (format!("{{ let __a: bool = {a}; if __a != {and} {{ __a }} else {{ {b} }} }}"), K::B)
                }
                _ => return None,
            },
            ExprKind::If { cond, then, otherwise: Some(o) } => {
                let c = self.cond(cond, s, end)?;
                let (t, tk) = self.block_value(then, s, end)?;
                let (o, ok) = self.block_value(o, s, end)?;
                if tk != ok {
                    return None;
                }
                (format!("(if {c} {t} else {o})"), tk)
            }
            ExprKind::Block(b) => self.block_value(b, s, end)?,
            ExprKind::Call { callee: Callee::Fn(id), args } => {
                let i = id.0 as usize;
                if !*self.ok.get(i)? {
                    return None;
                }
                let sig = self.sigs.get(i)?.clone()?;
                if args.len() != sig.params.len() {
                    return None;
                }
                // The arguments in order, then the call, as `Gen` makes it.
                let mut code = String::from("{ ");
                let mut names = Vec::with_capacity(args.len());
                for (n, (a, want)) in args.iter().zip(&sig.params).enumerate() {
                    let (x, k) = self.expr(a, s, end)?;
                    if k != *want {
                        return None;
                    }
                    let _ = write!(code, "let __a{n}: {} = {x}; ", k.rust());
                    names.push(format!("__a{n}"));
                }
                let _ = write!(
                    code,
                    "u!(cx, __ops, cx.enter(), {s}, {end}); cx.put_ops(__ops); let __c = t{i}(cx{}{}); \
                     __ops = cx.ops().0; cx.leave(); u!(cx, __ops, __c, {s}, {end}) }}",
                    if names.is_empty() { "" } else { ", " },
                    names.join(", ")
                );
                (code, sig.result)
            }
            _ => return None,
        };
        // What the checker said it is, or it is not trusted.
        if K::of(&e.ty) != Some(k) {
            return None;
        }
        Some((code, k))
    }
}

/// One step of the budget, as the interpreter's `tick` takes it, counted in
/// the body's own `__ops`; past the budget, its failure placed at `s..e`
/// (`u32::MAX`: left for the caller to place, as `t!` leaves it).
fn tick(s: u32, e: u32) -> String {
    let err = if s == u32::MAX {
        "aot::too_many(__max)".to_string()
    } else {
        format!("aot::at(aot::too_many(__max), {s}, {e})")
    };
    format!("__ops += 1;\nif __ops > __max {{ cx.put_ops(__ops); return Err({err}); }}\n")
}

/// Leaving the body with `x`: worked out first (it may call, and so count),
/// then the count handed back.
fn ret(x: &str) -> String {
    format!("{{ let __r = {x}; cx.put_ops(__ops); return Ok(__r); }}")
}

/// `a op b` on typed values held in `a.0` and `b.0`, as the interpreter's
/// `binary` works it out for those kinds: Rust's own operation where it
/// agrees, the interpreter's operator where Rust's stops.
fn binary(op: BinOp, a: (&str, K), b: (&str, K), s: u32, end: u32) -> Option<(String, K)> {
    let ((x, ak), (y, bk)) = (a, b);
    let slow = |op: BinOp| format!("u!(cx, __ops, aot::int_of(cx.binary(BinOp::{op:?}, V::Int({x}), V::Int({y}))), {s}, {end})");
    use BinOp::*;
    Some(match (op, ak, bk) {
        (AddInt, K::I, K::I) => (format!("match {x}.checked_add({y}) {{ Some(v) => v, None => {} }}", slow(op)), K::I),
        (SubInt, K::I, K::I) => (format!("match {x}.checked_sub({y}) {{ Some(v) => v, None => {} }}", slow(op)), K::I),
        (MulInt, K::I, K::I) => (format!("match {x}.checked_mul({y}) {{ Some(v) => v, None => {} }}", slow(op)), K::I),
        (RemInt, K::I, K::I) => (
            format!("match if {y} != 0 {{ {x}.checked_rem({y}) }} else {{ None }} {{ Some(v) => v, None => {} }}", slow(op)),
            K::I,
        ),
        (PowInt, K::I, K::I) => (slow(op), K::I),
        (AddFloat, K::F, K::F) => (format!("({x} + {y})"), K::F),
        (SubFloat, K::F, K::F) => (format!("({x} - {y})"), K::F),
        (MulFloat, K::F, K::F) => (format!("({x} * {y})"), K::F),
        (RemFloat, K::F, K::F) => (format!("({x} % {y})"), K::F),
        (PowFloat, K::F, K::F) => (format!("{x}.powf({y})"), K::F),
        (Div, K::I | K::F, K::I | K::F) => (format!("(({x} as f64) / ({y} as f64))"), K::F),
        (Lt | Le | Gt | Ge, K::I, K::I) | (Lt | Le | Gt | Ge, K::F, K::F) => {
            let sym = match op {
                Lt => "<",
                Le => "<=",
                Gt => ">",
                _ => ">=",
            };
            (format!("({x} {sym} {y})"), K::B)
        }
        (Eq | Ne, _, _) if ak == bk => (format!("({x} {} {y})", if op == Eq { "==" } else { "!=" }), K::B),
        _ => return None,
    })
}
