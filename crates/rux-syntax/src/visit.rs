//! Walking the AST.

use crate::ast::*;

/// Call `f` on every expression in `script`, outermost first. When `f`
/// returns `false`, what is inside that expression is skipped.
pub fn exprs(script: &Script, f: &mut impl FnMut(&Expr) -> bool) {
    for s in &script.stmts {
        stmt(s, f);
    }
}

fn block(b: &Block, f: &mut impl FnMut(&Expr) -> bool) {
    for s in &b.stmts {
        stmt(s, f);
    }
}

fn opt(e: &Option<Expr>, f: &mut impl FnMut(&Expr) -> bool) {
    if let Some(e) = e {
        expr(e, f);
    }
}

fn stmt(s: &Stmt, f: &mut impl FnMut(&Expr) -> bool) {
    match &s.kind {
        StmtKind::Empty | StmtKind::Continue | StmtKind::Type { .. } | StmtKind::Use(_) => {}
        StmtKind::Expr(e) => expr(e, f),
        StmtKind::Let { value, .. } => opt(value, f),
        StmtKind::Assign { target, value, .. } => {
            expr(target, f);
            expr(value, f);
        }
        StmtKind::Step { target, .. } => expr(target, f),
        StmtKind::If(i) => if_stmt(i, f),
        StmtKind::While { cond, body } => {
            opt(cond, f);
            block(body, f);
        }
        StmtKind::Do { body, cond, .. } => {
            block(body, f);
            expr(cond, f);
        }
        StmtKind::For { iter, body, .. } => {
            expr(iter, f);
            block(body, f);
        }
        StmtKind::Break(e) | StmtKind::Return(e) | StmtKind::Throw(e) => opt(e, f),
        StmtKind::Try { body, catch, .. } => {
            block(body, f);
            block(catch, f);
        }
        StmtKind::Switch(sw) => switch(sw, f),
        StmtKind::Block(b) => block(b, f),
        StmtKind::Fn(d) => block(&d.body, f),
        StmtKind::Import { path, .. } => expr(path, f),
        StmtKind::Export(Export::Let(s)) => stmt(s, f),
        StmtKind::Export(Export::Name { .. }) => {}
        StmtKind::Computed { value, .. } => expr(value, f),
        StmtKind::Lifecycle { body, .. } => block(body, f),
        StmtKind::Prop(decls) => {
            for d in decls {
                opt(&d.default, f);
            }
        }
    }
}

fn if_stmt(i: &If, f: &mut impl FnMut(&Expr) -> bool) {
    expr(&i.cond, f);
    block(&i.then, f);
    if let Some(o) = &i.otherwise {
        stmt(o, f);
    }
}

fn switch(sw: &Switch, f: &mut impl FnMut(&Expr) -> bool) {
    expr(&sw.value, f);
    for arm in &sw.arms {
        for p in &arm.patterns {
            expr(p, f);
        }
        if let Some(g) = &arm.guard {
            expr(g, f);
        }
        stmt(&arm.body, f);
    }
}

fn expr(e: &Expr, f: &mut impl FnMut(&Expr) -> bool) {
    if !f(e) {
        return;
    }
    match &e.kind {
        ExprKind::Unit
        | ExprKind::Null
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Str(_)
        | ExprKind::Char(_)
        | ExprKind::Bool(_)
        | ExprKind::Var(_)
        | ExprKind::Path(_)
        | ExprKind::This => {}
        ExprKind::Template(parts) => {
            for p in parts {
                if let TemplatePart::Code(b) = p {
                    block(b, f);
                }
            }
        }
        ExprKind::Array(items) => {
            for i in items {
                expr(i, f);
            }
        }
        ExprKind::Map { entries, .. } => {
            for en in entries {
                expr(&en.value, f);
            }
        }
        ExprKind::Call { args, .. } => {
            for a in args {
                expr(a, f);
            }
        }
        ExprKind::Method { recv, args, .. } => {
            expr(recv, f);
            for a in args {
                expr(a, f);
            }
        }
        ExprKind::Field { base, .. } => expr(base, f),
        ExprKind::Index { base, index, .. } => {
            expr(base, f);
            expr(index, f);
        }
        ExprKind::Unary { expr: inner, .. } => expr(inner, f),
        ExprKind::Binary { lhs, rhs, .. } => {
            expr(lhs, f);
            expr(rhs, f);
        }
        ExprKind::Is { expr: inner, .. } => expr(inner, f),
        ExprKind::Closure { body, .. } => stmt(body, f),
        ExprKind::Interval { args, body } => {
            for a in args {
                expr(a, f);
            }
            block(body, f);
        }
        ExprKind::Stmt(s) => stmt(s, f),
    }
}
