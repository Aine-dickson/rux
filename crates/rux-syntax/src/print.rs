//! The AST back to source, with every group made explicit.
//!
//! This is the canonical form the differential test hands to the fork: an
//! operator's operands are always in parentheses, so if this parser grouped
//! `a ?? 0 == 1` differently from the fork, the fork would build a different
//! tree from this text than from the original, and the test says so. It is
//! also the text a person reads when two parses disagree.
//!
//! It writes the fork's spelling where the fork has one it would read
//! differently (`#{` for a map, `__interval(…)` for `setInterval(…) { }`), and
//! is not a formatter: nothing here tries to look like what was written.
//!
//! Annotations are left out, as `rux-script` leaves them out of what it hands
//! the fork to run: a type means nothing at run time, and the fork is never
//! shown one it may not know how to read. `x is T` keeps its type, which is
//! checked at run time. [`ty`] writes a type on its own.

use crate::ast::*;

/// A whole script.
pub fn script(s: &Script, src: &str) -> String {
    let mut p = Printer { out: String::new(), src };
    for stmt in &s.stmts {
        p.stmt_line(stmt);
    }
    p.out
}

/// One expression.
pub fn expr(e: &Expr, src: &str) -> String {
    let mut p = Printer { out: String::new(), src };
    p.expr(e);
    p.out
}

/// A type.
pub fn ty(t: &TypeExpr) -> String {
    let mut p = Printer { out: String::new(), src: "" };
    p.ty(t);
    p.out
}

struct Printer<'s> {
    out: String,
    src: &'s str,
}

impl Printer<'_> {
    fn w(&mut self, s: &str) {
        self.out.push_str(s);
    }

    /// A statement, then its `;` when it needs one, then a newline.
    fn stmt_line(&mut self, s: &Stmt) {
        if matches!(s.kind, StmtKind::Empty) {
            return;
        }
        self.stmt(s);
        if !s.is_self_terminated() {
            self.w(";");
        }
        self.w("\n");
    }

    fn block(&mut self, b: &Block) {
        self.w("{\n");
        for s in &b.stmts {
            self.stmt_line(s);
        }
        self.w("}");
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Empty => {}
            StmtKind::Expr(e) => self.expr(e),
            StmtKind::Let { name, ty, value, constant } => {
                self.w(if *constant { "const " } else { "let " });
                self.w(&name.name);
                let _ = ty;
                if let Some(v) = value {
                    self.w(" = ");
                    self.expr(v);
                }
            }
            StmtKind::Assign { target, op, value } => {
                self.expr(target);
                self.w(" ");
                self.w(op);
                self.w(" ");
                self.expr(value);
            }
            StmtKind::Step { target, up } => {
                self.expr(target);
                self.w(if *up { "++" } else { "--" });
            }
            StmtKind::If(i) => self.if_stmt(i),
            StmtKind::While { cond: Some(c), body } => {
                self.w("while ");
                self.expr(c);
                self.w(" ");
                self.block(body);
            }
            StmtKind::While { cond: None, body } => {
                self.w("loop ");
                self.block(body);
            }
            StmtKind::Do { body, cond, until } => {
                self.w("do ");
                self.block(body);
                self.w(if *until { " until " } else { " while " });
                self.expr(cond);
            }
            StmtKind::For { var, counter, iter, body } => {
                self.w("for ");
                match counter {
                    Some(c) => {
                        self.w("(");
                        self.w(&var.name);
                        self.w(", ");
                        self.w(&c.name);
                        self.w(")");
                    }
                    None => self.w(&var.name),
                }
                self.w(" in ");
                self.expr(iter);
                self.w(" ");
                self.block(body);
            }
            StmtKind::Break(e) => self.keyword_expr("break", e.as_ref()),
            StmtKind::Continue => self.w("continue"),
            StmtKind::Return(e) => self.keyword_expr("return", e.as_ref()),
            StmtKind::Throw(e) => self.keyword_expr("throw", e.as_ref()),
            StmtKind::Try { body, var, catch } => {
                self.w("try ");
                self.block(body);
                self.w(" catch ");
                if let Some(v) = var {
                    self.w("(");
                    self.w(&v.name);
                    self.w(") ");
                }
                self.block(catch);
            }
            StmtKind::Switch(sw) => self.switch(sw),
            StmtKind::Block(b) => self.block(b),
            StmtKind::Fn(f) => {
                if f.private {
                    self.w("private ");
                }
                self.w("fn ");
                if let Some(t) = &f.this_type {
                    self.w(t);
                    self.w(".");
                }
                self.w(&f.name.name);
                self.params(&f.params);
                self.w(" ");
                self.block(&f.body);
            }
            StmtKind::Type { .. } => {}
            StmtKind::Import { path, alias } => {
                self.w("import ");
                self.expr(path);
                if let Some(a) = alias {
                    self.w(" as ");
                    self.w(&a.name);
                }
            }
            StmtKind::Export(Export::Let(s)) => {
                self.w("export ");
                self.stmt(s);
            }
            StmtKind::Export(Export::Name { name, alias }) => {
                self.w("export ");
                self.w(&name.name);
                if let Some(a) = alias {
                    self.w(" as ");
                    self.w(&a.name);
                }
            }
            StmtKind::Use(path) => {
                self.w("use ");
                self.w(&path.iter().map(|i| i.name.as_str()).collect::<Vec<_>>().join("::"));
                self.w(";");
            }
            StmtKind::Computed { name, ty, value } => {
                self.w("computed ");
                self.w(&name.name);
                if let Some(t) = ty {
                    self.w(": ");
                    self.ty(t);
                }
                self.w(" = ");
                self.expr(value);
                self.w(";");
            }
            StmtKind::Lifecycle { kind, body } => {
                self.w(kind.keyword());
                self.w(" ");
                self.block(body);
            }
            StmtKind::Prop(decls) => {
                self.w("prop ");
                for (i, d) in decls.iter().enumerate() {
                    if i > 0 {
                        self.w(", ");
                    }
                    self.w(&d.name.name);
                    if let Some(t) = &d.ty {
                        self.w(": ");
                        self.ty(t);
                    }
                    if let Some(v) = &d.default {
                        self.w(" = ");
                        self.expr(v);
                    }
                }
                self.w(";");
            }
        }
    }

    fn keyword_expr(&mut self, kw: &str, e: Option<&Expr>) {
        self.w(kw);
        if let Some(e) = e {
            self.w(" ");
            self.expr(e);
        }
    }

    fn if_stmt(&mut self, i: &If) {
        self.w("if ");
        self.expr(&i.cond);
        self.w(" ");
        self.block(&i.then);
        if let Some(o) = &i.otherwise {
            self.w(" else ");
            self.stmt(o);
        }
    }

    fn switch(&mut self, sw: &Switch) {
        self.w("switch ");
        self.expr(&sw.value);
        self.w(" {\n");
        for arm in &sw.arms {
            if arm.patterns.is_empty() {
                self.w("_");
            }
            for (i, p) in arm.patterns.iter().enumerate() {
                if i > 0 {
                    self.w(" | ");
                }
                // A case is a literal, and the fork reads a parenthesised
                // range as a call rather than a literal, so ranges go bare.
                match &p.kind {
                    ExprKind::Binary { op, lhs, rhs } => {
                        self.expr(lhs);
                        self.w(op);
                        self.expr(rhs);
                    }
                    _ => self.expr(p),
                }
            }
            if let Some(g) = &arm.guard {
                self.w(" if ");
                self.expr(g);
            }
            self.w(" => ");
            self.stmt(&arm.body);
            self.w(",\n");
        }
        self.w("}");
    }

    fn params(&mut self, params: &[Param]) {
        self.w("(");
        for (i, p) in params.iter().enumerate() {
            if i > 0 {
                self.w(", ");
            }
            self.w(&p.name.name);
        }
        self.w(")");
    }

    fn args(&mut self, args: &[Expr]) {
        self.w("(");
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                self.w(", ");
            }
            self.expr(a);
        }
        self.w(")");
    }

    /// An expression something postfix will follow: in parentheses unless
    /// it is already a single unit the fork reads the same way bare.
    fn receiver(&mut self, e: &Expr) {
        let bare = match &e.kind {
            ExprKind::Int(n) => *n >= 0,
            ExprKind::Float(f) => *f >= 0.0 && !f.is_sign_negative(),
            ExprKind::Var(_)
            | ExprKind::Path(_)
            | ExprKind::This
            | ExprKind::Call { .. }
            | ExprKind::Method { .. }
            | ExprKind::Field { .. }
            | ExprKind::Index { .. }
            | ExprKind::Str(_)
            | ExprKind::Char(_)
            | ExprKind::Bool(_)
            | ExprKind::Unit
            | ExprKind::Array(_)
            | ExprKind::Map { .. }
            | ExprKind::Template(_) => true,
            _ => false,
        };
        if bare {
            self.expr(e);
        } else {
            self.w("(");
            self.expr(e);
            self.w(")");
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Unit => self.w("()"),
            ExprKind::Null => self.w("null"),
            ExprKind::Int(n) => self.w(&n.to_string()),
            ExprKind::Float(f) => {
                let text = format!("{f:?}");
                self.w(&text);
            }
            ExprKind::Str(s) => self.string(s),
            ExprKind::Char(c) => {
                self.w("'");
                match c {
                    '\'' => self.w("\\'"),
                    '\\' => self.w("\\\\"),
                    '\n' => self.w("\\n"),
                    '\r' => self.w("\\r"),
                    '\t' => self.w("\\t"),
                    c if c.is_control() => self.w(&format!("\\u{:04X}", *c as u32)),
                    c => self.out.push(*c),
                }
                self.w("'");
            }
            ExprKind::Bool(b) => self.w(if *b { "true" } else { "false" }),
            ExprKind::Template(parts) => {
                // A newline straight after the backtick is dropped by the
                // lexer, so one is always written and the text starts clean.
                self.w("`\n");
                for part in parts {
                    match part {
                        TemplatePart::Text(t) => {
                            let mut chars = t.chars().peekable();
                            while let Some(c) = chars.next() {
                                match c {
                                    '`' => self.w("``"),
                                    '$' if chars.peek() == Some(&'{') => self.w("\\$"),
                                    c => self.out.push(c),
                                }
                            }
                        }
                        TemplatePart::Code(b) => {
                            self.w("${");
                            for s in &b.stmts {
                                self.stmt(s);
                                self.w(";");
                            }
                            self.w("}");
                        }
                    }
                }
                self.w("`");
            }
            ExprKind::Array(items) => {
                self.w("[");
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        self.w(", ");
                    }
                    self.expr(item);
                }
                self.w("]");
            }
            ExprKind::Map { entries, .. } => {
                self.w("#{");
                for (i, entry) in entries.iter().enumerate() {
                    if i > 0 {
                        self.w(",");
                    }
                    self.w(" ");
                    self.string(&entry.key.name);
                    self.w(": ");
                    self.expr(&entry.value);
                }
                self.w(" }");
            }
            ExprKind::Var(n) => self.w(n),
            ExprKind::Path(p) => self.w(&p.iter().map(|i| i.name.as_str()).collect::<Vec<_>>().join("::")),
            ExprKind::This => self.w("this"),
            ExprKind::Call { callee, args, bang } => {
                self.w(&callee.iter().map(|i| i.name.as_str()).collect::<Vec<_>>().join("::"));
                if *bang {
                    self.w("!");
                }
                self.args(args);
            }
            ExprKind::Method { recv, name, args, optional } => {
                self.receiver(recv);
                self.w(if *optional { "?." } else { "." });
                self.w(&name.name);
                self.args(args);
            }
            ExprKind::Field { base, name, optional } => {
                self.receiver(base);
                self.w(if *optional { "?." } else { "." });
                self.w(&name.name);
            }
            ExprKind::Index { base, index, optional } => {
                self.receiver(base);
                self.w(if *optional { "?[" } else { "[" });
                self.expr(index);
                self.w("]");
            }
            ExprKind::Unary { op, expr } => {
                self.w("(");
                self.w(op);
                self.w("(");
                self.expr(expr);
                self.w("))");
            }
            ExprKind::Binary { op, lhs, rhs } => {
                self.w("(");
                self.expr(lhs);
                self.w(" ");
                self.w(op);
                // An open range, `a..`, has nothing on its right.
                if !(matches!(*op, ".." | "..=") && matches!(rhs.kind, ExprKind::Unit) && rhs.span.start == rhs.span.end) {
                    self.w(" ");
                    self.expr(rhs);
                }
                self.w(")");
            }
            // The call the fork runs it as, so the fork never reads a type.
            ExprKind::Is { expr, ty } => {
                self.w("__is(");
                self.expr(expr);
                self.w(", ");
                self.string(&self::ty(ty));
                self.w(")");
            }
            ExprKind::Closure { params, body, arrow } => {
                self.w("(");
                if *arrow {
                    self.params(params);
                    self.w(" => ");
                } else {
                    self.w("|");
                    self.w(&params.iter().map(|p| p.name.name.as_str()).collect::<Vec<_>>().join(", "));
                    self.w("| ");
                }
                self.stmt(body);
                self.w(")");
            }
            ExprKind::Interval { args, body } => {
                // What `setInterval(ms) { … }` has always become for the fork:
                // the body, as written, in a string.
                self.w("__interval(");
                for a in args {
                    self.expr(a);
                    self.w(", ");
                }
                self.w("\"");
                let inner = &self.src[body.span.start as usize + 1..body.span.end as usize - 1];
                let mut escaped = String::new();
                for c in inner.chars() {
                    match c {
                        '\\' => escaped.push_str("\\\\"),
                        '"' => escaped.push_str("\\\""),
                        '\n' => escaped.push_str("\\n"),
                        '\r' => {}
                        c => escaped.push(c),
                    }
                }
                self.w(&escaped);
                self.w("\")");
            }
            ExprKind::Stmt(s) => {
                self.w("(");
                self.stmt(s);
                self.w(")");
            }
        }
    }

    fn string(&mut self, s: &str) {
        self.w("\"");
        for c in s.chars() {
            match c {
                '"' => self.w("\\\""),
                '\\' => self.w("\\\\"),
                '\n' => self.w("\\n"),
                '\r' => self.w("\\r"),
                '\t' => self.w("\\t"),
                c if c.is_control() => self.w(&format!("\\u{:04X}", c as u32)),
                c => self.out.push(c),
            }
        }
        self.w("\"");
    }

    fn ty(&mut self, t: &TypeExpr) {
        match &t.kind {
            TypeKind::Name(n) => self.w(n),
            TypeKind::Generic { name, args } => {
                self.w(name);
                self.w("<");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.w(", ");
                    }
                    self.ty(a);
                }
                self.w(">");
            }
            TypeKind::Literal(s) => self.string(s),
            TypeKind::Null => self.w("null"),
            TypeKind::Array(inner) => {
                self.ty_postfix_operand(inner);
                self.w("[]");
            }
            TypeKind::Optional(inner) => {
                self.ty_postfix_operand(inner);
                self.w("?");
            }
            TypeKind::Union(members) => {
                for (i, m) in members.iter().enumerate() {
                    if i > 0 {
                        self.w(" | ");
                    }
                    self.ty(m);
                }
            }
            TypeKind::Record(fields) => {
                self.w("{ ");
                for (i, f) in fields.iter().enumerate() {
                    if i > 0 {
                        self.w(", ");
                    }
                    self.w(&f.name.name);
                    if f.optional {
                        self.w("?");
                    }
                    self.w(": ");
                    self.ty(&f.ty);
                }
                self.w(" }");
            }
            TypeKind::Dict { key, value } => {
                self.w("{ [");
                self.w(key);
                self.w("]: ");
                self.ty(value);
                self.w(" }");
            }
            TypeKind::Function(params, result) => {
                self.w("(");
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        self.w(", ");
                    }
                    self.ty(p);
                }
                self.w(") => ");
                self.ty(result);
            }
            TypeKind::Paren(inner) => {
                self.w("(");
                self.ty(inner);
                self.w(")");
            }
        }
    }

    fn ty_postfix_operand(&mut self, t: &TypeExpr) {
        match t.kind {
            TypeKind::Union(_) | TypeKind::Function(..) => {
                self.w("(");
                self.ty(t);
                self.w(")");
            }
            _ => self.ty(t),
        }
    }
}
