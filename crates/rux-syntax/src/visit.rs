//! Walking the AST.

use crate::ast::*;

/// A statement or an expression, as [`walk`] hands them over.
#[derive(Clone, Copy, Debug)]
pub enum Node<'a> {
    Stmt(&'a Stmt),
    Expr(&'a Expr),
}

/// Call `f` on every expression in `script`, outermost first. When `f`
/// returns `false`, what is inside that expression is skipped.
pub fn exprs(script: &Script, f: &mut impl FnMut(&Expr) -> bool) {
    walk_stmts(&script.stmts, &mut |n| match n {
        Node::Expr(e) => f(e),
        Node::Stmt(_) => true,
    });
}

/// Call `f` on every statement and expression in `stmts`, outermost first,
/// function bodies and closures included. When `f` returns `false`, what is
/// inside that node is skipped.
pub fn walk_stmts<'a>(stmts: &'a [Stmt], f: &mut impl FnMut(Node<'a>) -> bool) {
    for s in stmts {
        stmt(s, f);
    }
}

/// [`walk_stmts`] from one expression.
pub fn walk_expr<'a>(e: &'a Expr, f: &mut impl FnMut(Node<'a>) -> bool) {
    expr(e, f);
}

fn block<'a>(b: &'a Block, f: &mut impl FnMut(Node<'a>) -> bool) {
    for s in &b.stmts {
        stmt(s, f);
    }
}

fn opt<'a>(e: &'a Option<Expr>, f: &mut impl FnMut(Node<'a>) -> bool) {
    if let Some(e) = e {
        expr(e, f);
    }
}

fn stmt<'a>(s: &'a Stmt, f: &mut impl FnMut(Node<'a>) -> bool) {
    if !f(Node::Stmt(s)) {
        return;
    }
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

fn if_stmt<'a>(i: &'a If, f: &mut impl FnMut(Node<'a>) -> bool) {
    expr(&i.cond, f);
    block(&i.then, f);
    if let Some(o) = &i.otherwise {
        stmt(o, f);
    }
}

fn switch<'a>(sw: &'a Switch, f: &mut impl FnMut(Node<'a>) -> bool) {
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

fn expr<'a>(e: &'a Expr, f: &mut impl FnMut(Node<'a>) -> bool) {
    if !f(Node::Expr(e)) {
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
