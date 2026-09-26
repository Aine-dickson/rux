//! Walking the AST to change it: [`crate::visit`], with `&mut`.

use crate::ast::*;

/// Call `f` on every expression in `stmts`, outermost first, function bodies
/// and closures included. `f` may replace the expression it is handed; what
/// is inside the expression it leaves there is walked next. When `f` returns
/// `false`, that is skipped.
pub fn exprs_mut(stmts: &mut [Stmt], f: &mut impl FnMut(&mut Expr) -> bool) {
    for s in stmts {
        stmt(s, f);
    }
}

fn block(b: &mut Block, f: &mut impl FnMut(&mut Expr) -> bool) {
    for s in &mut b.stmts {
        stmt(s, f);
    }
}

fn opt(e: &mut Option<Expr>, f: &mut impl FnMut(&mut Expr) -> bool) {
    if let Some(e) = e {
        expr(e, f);
    }
}

fn stmt(s: &mut Stmt, f: &mut impl FnMut(&mut Expr) -> bool) {
    match &mut s.kind {
        StmtKind::Empty | StmtKind::Continue | StmtKind::Type { .. } | StmtKind::Import(_) => {}
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
        StmtKind::Fn(d) => block(&mut d.body, f),
        StmtKind::Computed { value, .. } => expr(value, f),
        StmtKind::Lifecycle { body, .. } => block(body, f),
        StmtKind::Prop(decls) => {
            for d in decls {
                opt(&mut d.default, f);
            }
        }
    }
}

fn if_stmt(i: &mut If, f: &mut impl FnMut(&mut Expr) -> bool) {
    expr(&mut i.cond, f);
    block(&mut i.then, f);
    if let Some(o) = &mut i.otherwise {
        stmt(o, f);
    }
}

fn switch(sw: &mut Switch, f: &mut impl FnMut(&mut Expr) -> bool) {
    expr(&mut sw.value, f);
    for arm in &mut sw.arms {
        for p in &mut arm.patterns {
            expr(p, f);
        }
        if let Some(g) = &mut arm.guard {
            expr(g, f);
        }
        stmt(&mut arm.body, f);
    }
}

fn expr(e: &mut Expr, f: &mut impl FnMut(&mut Expr) -> bool) {
    if !f(e) {
        return;
    }
    match &mut e.kind {
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
                expr(&mut en.value, f);
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
        ExprKind::Unary { expr: inner, .. } | ExprKind::Await(inner) => expr(inner, f),
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
