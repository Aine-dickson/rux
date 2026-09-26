//! The IR as text, for reading and for snapshot tests.
//!
//! One node per parenthesised form, with its type after a `:` where the node
//! is not a literal. A local is written `name#slot`, a capture `^n`, a global
//! `@name`, a function `fn:name`.
//!
//! ```text
//! global 0 n: int signal
//! fn 0 bump(): void
//!   (assign @n add.int (int 1))
//! ```

use std::fmt::Write;

use crate::ir::*;

/// A whole unit.
pub fn unit(u: &Unit) -> String {
    let mut p = Printer { u, out: String::new(), frames: Vec::new() };
    p.unit();
    p.out
}

struct Printer<'u> {
    u: &'u Unit,
    out: String,
    /// The locals of each frame being printed, innermost last.
    frames: Vec<&'u [Local]>,
}

impl<'u> Printer<'u> {
    fn unit(&mut self) {
        let u = self.u;
        for (name, params, ty) in &u.types {
            if name == "Result" {
                continue;
            }
            let params = if params.is_empty() { String::new() } else { format!("<{}>", params.join(", ")) };
            let _ = writeln!(self.out, "type {name}{params} = {ty}");
        }
        for (i, g) in u.globals.iter().enumerate() {
            let kind = match g.kind {
                GlobalKind::Signal => "signal",
                GlobalKind::Let => "let",
                GlobalKind::Computed => "computed",
                GlobalKind::Prop => "prop",
                GlobalKind::Provided => "provided",
            };
            let _ = writeln!(self.out, "global {i} {}: {} {kind}", g.name, g.ty);
        }
        if !u.init.block.stmts.is_empty() {
            self.out.push_str("init\n");
            self.body(&u.init, 1);
        }
        for c in &u.computeds {
            let _ = writeln!(self.out, "computed @{}", u.globals[c.global.0 as usize].name);
            self.body(&c.value, 1);
        }
        for (label, bodies) in [("effect", &u.effects), ("mounted", &u.mounted), ("unmounted", &u.unmounted)] {
            for b in bodies {
                let _ = writeln!(self.out, "{label}");
                self.body(b, 1);
            }
        }
        for (i, f) in u.fns.iter().enumerate() {
            let tparams =
                if f.type_params.is_empty() { String::new() } else { format!("<{}>", f.type_params.join(", ")) };
            let params: Vec<String> = f.body.locals[..f.params as usize]
                .iter()
                .enumerate()
                .map(|(i, l)| format!("{}#{i}: {}", l.name, l.ty))
                .collect();
            let _ = writeln!(self.out, "fn {i} {}{tparams}({}): {}", f.name, params.join(", "), f.result);
            self.body(&f.body, 1);
        }
        for (i, piece) in u.pieces.iter().enumerate() {
            let kind = match piece.kind {
                PieceKind::Value => "value",
                PieceKind::Handler => "handler",
                PieceKind::Model => "model",
            };
            let given: Vec<String> = piece.body.locals[..piece.given as usize]
                .iter()
                .enumerate()
                .map(|(i, l)| format!("{}#{i}: {}", l.name, l.ty))
                .collect();
            let _ = writeln!(
                self.out,
                "piece {i} {kind} line {} {:?} ({})",
                piece.line,
                piece.src.trim(),
                given.join(", ")
            );
            self.body(&piece.body, 1);
        }
        for x in &u.unsupported {
            let _ = writeln!(self.out, "unsupported {} at {}..{}", x.what, x.at.start, x.at.end);
        }
    }

    fn indent(&mut self, depth: usize) {
        for _ in 0..depth {
            self.out.push_str("  ");
        }
    }

    fn body(&mut self, b: &'u Body, depth: usize) {
        self.frames.push(&b.locals);
        self.block(&b.block, depth);
        self.frames.pop();
    }

    fn block(&mut self, b: &'u Block, depth: usize) {
        for s in &b.stmts {
            self.stmt(s, depth);
        }
    }

    fn local(&self, id: LocalId) -> String {
        let name = self.frames.last().and_then(|f| f.get(id.0 as usize)).map_or("?", |l| l.name.as_str());
        format!("{name}#{}", id.0)
    }

    fn root(&self, r: Root) -> String {
        match r {
            Root::Local(id) => self.local(id),
            Root::Capture(i) => format!("^{i}"),
            Root::Global(g) => format!("@{}", self.u.globals.get(g.0 as usize).map_or("?", |g| g.name.as_str())),
            Root::Outer(n) => format!("outer:{}", self.u.outer.get(n as usize).map_or("?", |n| n.as_str())),
        }
    }

    fn stmt(&mut self, s: &'u Stmt, depth: usize) {
        self.indent(depth);
        match &s.kind {
            StmtKind::Expr(e) => {
                let e = self.expr(e);
                self.out.push_str(&e);
                self.out.push('\n');
            }
            StmtKind::Let { local, value } => {
                let ty = self.frames.last().and_then(|f| f.get(local.0 as usize)).map(|l| l.ty.to_string());
                let head = format!("(let {}: {}", self.local(*local), ty.unwrap_or_default());
                let value = value.as_ref().map(|v| format!(" {}", self.expr(v))).unwrap_or_default();
                let _ = writeln!(self.out, "{head}{value})");
            }
            StmtKind::Init { global, value } => {
                let v = self.expr(value);
                let _ = writeln!(self.out, "(init {} {v})", self.root(Root::Global(*global)));
            }
            StmtKind::Assign { place, op, value } => {
                let mut target = self.root(place.root);
                for step in &place.steps {
                    match step {
                        PlaceStep::Field(n) => {
                            let _ = write!(target, ".{n}");
                        }
                        PlaceStep::Index(i) => {
                            let i = self.expr(i);
                            let _ = write!(target, "[{i}]");
                        }
                    }
                }
                let op = op.map(|o| format!(" {}", o.name())).unwrap_or_default();
                let v = self.expr(value);
                let _ = writeln!(self.out, "(assign {target}: {}{op} {v})", place.ty);
            }
            StmtKind::If { cond, then, otherwise } => {
                let c = self.expr(cond);
                let _ = writeln!(self.out, "if {c}");
                self.block(then, depth + 1);
                if let Some(o) = otherwise {
                    self.indent(depth);
                    self.out.push_str("else\n");
                    self.block(o, depth + 1);
                }
            }
            StmtKind::While { cond, body } => {
                let c = self.expr(cond);
                let _ = writeln!(self.out, "while {c}");
                self.block(body, depth + 1);
            }
            StmtKind::ForRange { var, from, to, inclusive, body } => {
                let (f, t) = (self.expr(from), self.expr(to));
                let dots = if *inclusive { "..=" } else { ".." };
                let _ = writeln!(self.out, "for {} in {f}{dots}{t}", self.local(*var));
                self.block(body, depth + 1);
            }
            StmtKind::ForEach { var, counter, iter, over, body } => {
                let i = self.expr(iter);
                let counter = counter.map(|c| format!(", {}", self.local(c))).unwrap_or_default();
                let _ = writeln!(self.out, "for {}{counter} in {over:?} {i}", self.local(*var));
                self.block(body, depth + 1);
            }
            StmtKind::Break => self.out.push_str("break\n"),
            StmtKind::Continue => self.out.push_str("continue\n"),
            StmtKind::Return(v) => {
                let v = v.as_ref().map(|v| format!(" {}", self.expr(v))).unwrap_or_default();
                let _ = writeln!(self.out, "(return{v})");
            }
            StmtKind::Throw(v) => {
                let v = self.expr(v);
                let _ = writeln!(self.out, "(throw {v})");
            }
            StmtKind::Try { body, var, catch } => {
                self.out.push_str("try\n");
                self.block(body, depth + 1);
                self.indent(depth);
                let var = var.map(|v| format!(" {}", self.local(v))).unwrap_or_default();
                let _ = writeln!(self.out, "catch{var}");
                self.block(catch, depth + 1);
            }
        }
    }

    /// A block inside an expression, on one line.
    fn inline_block(&mut self, b: &'u Block) -> String {
        let saved = std::mem::take(&mut self.out);
        self.block(b, 0);
        let text = std::mem::replace(&mut self.out, saved);
        let parts: Vec<&str> = text.lines().map(str::trim).collect();
        format!("{{ {} }}", parts.join("; "))
    }

    fn expr(&mut self, e: &'u Expr) -> String {
        let typed = |s: String| format!("{s}:{}", e.ty);
        match &e.kind {
            ExprKind::None => "none".into(),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Int(n) => format!("(int {n})"),
            ExprKind::Float(f) => format!("(float {f:?})"),
            ExprKind::Str(s) => format!("{s:?}"),
            ExprKind::Template(parts) => {
                let parts: Vec<String> = parts.iter().map(|p| self.expr(p)).collect();
                typed(format!("(template {})", parts.join(" ")))
            }
            ExprKind::Array(items) => {
                let items: Vec<String> = items.iter().map(|i| self.expr(i)).collect();
                typed(format!("[{}]", items.join(" ")))
            }
            ExprKind::Map(entries) => {
                let entries: Vec<String> = entries.iter().map(|(k, v)| format!("{k}: {}", self.expr(v))).collect();
                typed(format!("{{{}}}", entries.join(", ")))
            }
            ExprKind::Local(id) => typed(self.local(*id)),
            ExprKind::Capture(i) => typed(format!("^{i}")),
            ExprKind::Global(g) => typed(self.root(Root::Global(*g))),
            ExprKind::Outer(n) => typed(self.root(Root::Outer(*n))),
            ExprKind::Call { callee, args } => {
                let callee = match callee {
                    Callee::Fn(f) => format!("fn:{}", self.u.fns.get(f.0 as usize).map_or("?", |f| f.name.as_str())),
                    Callee::Builtin(n) => n.clone(),
                    Callee::Host(n) => format!("host::{n}"),
                    Callee::Value(v) => self.expr(v),
                    Callee::Dyn(n) => format!("dyn:{n}"),
                };
                let args: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
                typed(format!("(call {callee}{}{})", if args.is_empty() { "" } else { " " }, args.join(" ")))
            }
            ExprKind::Method { recv, method, args, optional } => {
                let r = self.expr(recv);
                let args: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
                let on = format!("{:?}", method.on).to_lowercase();
                let q = if *optional { "?" } else { "" };
                typed(format!("({on}.{}{q} {r}{}{})", method.name, if args.is_empty() { "" } else { " " }, args.join(" ")))
            }
            ExprKind::Field { base, name, optional } => {
                let b = self.expr(base);
                typed(format!("({} {b} {name})", if *optional { "field?" } else { "field" }))
            }
            ExprKind::Index { base, index, optional } => {
                let (b, i) = (self.expr(base), self.expr(index));
                typed(format!("({} {b} {i})", if *optional { "index?" } else { "index" }))
            }
            ExprKind::Chain(inner) => {
                let i = self.expr(inner);
                typed(format!("(chain {i})"))
            }
            ExprKind::Unary { op, expr } => {
                let x = self.expr(expr);
                let op = match op {
                    UnOp::Not => "not",
                    UnOp::NegInt => "neg.int",
                    UnOp::NegFloat => "neg.float",
                    UnOp::NegDyn => "neg.dyn",
                    UnOp::PlusDyn => "plus.dyn",
                };
                typed(format!("({op} {x})"))
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let (l, r) = (self.expr(lhs), self.expr(rhs));
                let name = match op {
                    BinOp::Dyn(s) => format!("dyn{s}"),
                    op => op.name().to_string(),
                };
                typed(format!("({name} {l} {r})"))
            }
            ExprKind::Logic { and, lhs, rhs } => {
                let (l, r) = (self.expr(lhs), self.expr(rhs));
                typed(format!("({} {l} {r})", if *and { "and" } else { "or" }))
            }
            ExprKind::Coalesce { lhs, rhs } => {
                let (l, r) = (self.expr(lhs), self.expr(rhs));
                typed(format!("(?? {l} {r})"))
            }
            ExprKind::Is { expr, ty } => {
                let x = self.expr(expr);
                format!("(is {x} {ty})")
            }
            ExprKind::Widen(inner) => {
                let i = self.expr(inner);
                format!("(widen {i})")
            }
            ExprKind::Check(inner) => {
                let i = self.expr(inner);
                typed(format!("(check {i})"))
            }
            ExprKind::If { cond, then, otherwise } => {
                let c = self.expr(cond);
                let t = self.inline_block(then);
                let o = otherwise.as_ref().map(|o| format!(" else {}", self.inline_block(o))).unwrap_or_default();
                typed(format!("(if {c} {t}{o})"))
            }
            ExprKind::Match { value, arms } => {
                let v = self.expr(value);
                let mut parts = Vec::new();
                for arm in arms {
                    let pats: Vec<String> = arm.patterns.iter().map(|p| self.expr(p)).collect();
                    let pats = if pats.is_empty() { "_".to_string() } else { pats.join(" | ") };
                    let guard = arm.guard.as_ref().map(|g| format!(" if {}", self.expr(g))).unwrap_or_default();
                    let body = self.inline_block(&arm.body);
                    parts.push(format!("{pats}{guard} => {body}"));
                }
                typed(format!("(match {v} {})", parts.join(", ")))
            }
            ExprKind::Block(b) => {
                let b = self.inline_block(b);
                typed(format!("(block {b})"))
            }
            ExprKind::Closure(c) => {
                let caps: Vec<String> = c.captures.iter().map(|r| self.root(*r)).collect();
                self.frames.push(&c.body.locals);
                let params: Vec<String> = c.body.locals[..c.params as usize]
                    .iter()
                    .enumerate()
                    .map(|(i, l)| format!("{}#{i}: {}", l.name, l.ty))
                    .collect();
                let body = self.inline_block(&c.body.block);
                self.frames.pop();
                let caps = if caps.is_empty() { String::new() } else { format!(" [{}]", caps.join(" ")) };
                typed(format!("(closure ({}){caps} {body})", params.join(", ")))
            }
            ExprKind::Interval { args, body, .. } => {
                let args: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
                self.frames.push(&body.locals);
                let b = self.inline_block(&body.block);
                self.frames.pop();
                typed(format!("(interval {} {b})", args.join(" ")))
            }
        }
    }
}
