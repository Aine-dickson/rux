//! The Rux AST: what a script says, with a span on every node.
//!
//! The shape is Rux's, not rhai's. Rhai lowers much of what it reads while it
//! parses (`x++` becomes `x += 1`, `a in b` becomes a call to `contains`,
//! functions are lifted out of the statements); this keeps what was written,
//! so a checker, a formatter or an error message can talk about it in the
//! author's terms.

use crate::span::Span;

/// A whole script, a handler or a binding: statements in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    pub stmts: Vec<Stmt>,
}

/// A name and where it was written.
#[derive(Clone, Debug, PartialEq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// From the `{` to the `}`.
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    /// `;` on its own.
    Empty,
    Expr(Expr),
    /// `let x: T = v`, `const x = v`. `value` is `None` for `let x;`.
    Let { name: Ident, ty: Option<TypeExpr>, value: Option<Expr>, constant: bool },
    /// `target op value`, `op` being `=`, `+=`, …
    Assign { target: Expr, op: &'static str, value: Expr },
    /// `x++` or `x--`.
    Step { target: Expr, up: bool },
    If(If),
    /// `while cond { }`, and `loop { }` with no condition.
    While { cond: Option<Expr>, body: Block },
    /// `do { } while cond` or `do { } until cond`.
    Do { body: Block, cond: Expr, until: bool },
    /// `for x in iter { }` or `for (x, i) in iter { }`.
    For { var: Ident, counter: Option<Ident>, iter: Expr, body: Block },
    Break(Option<Expr>),
    Continue,
    Return(Option<Expr>),
    Throw(Option<Expr>),
    /// `try { } catch (e) { }`. The fork writes the name in parentheses.
    Try { body: Block, var: Option<Ident>, catch: Block },
    Switch(Switch),
    Block(Block),
    Fn(FnDecl),
    /// `type Name = T;`, or `type Name<T, U> = …;` with type parameters.
    Type { name: Ident, params: Vec<Ident>, ty: TypeExpr },
    /// `import "path" as name;`, rhai's module import.
    Import { path: Expr, alias: Option<Ident> },
    /// `export let x = …`, `export const …`, or `export name as alias`.
    Export(Export),
    /// `use a::b::C;`. Handled by the runtime before a script runs.
    Use(Vec<Ident>),
    /// `computed name: T = expr;`
    Computed { name: Ident, ty: Option<TypeExpr>, value: Expr },
    /// `effect { }`, `mounted { }` and `unmounted { }`.
    Lifecycle { kind: Lifecycle, body: Block },
    /// `prop a, b: T = default;`
    Prop(Vec<PropDecl>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    Effect,
    Mounted,
    Unmounted,
}

impl Lifecycle {
    pub fn keyword(self) -> &'static str {
        match self {
            Lifecycle::Effect => "effect",
            Lifecycle::Mounted => "mounted",
            Lifecycle::Unmounted => "unmounted",
        }
    }
}

/// One name of a `prop` statement.
#[derive(Clone, Debug, PartialEq)]
pub struct PropDecl {
    pub name: Ident,
    pub ty: Option<TypeExpr>,
    /// Only on a statement declaring one prop.
    pub default: Option<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Export {
    Let(Box<Stmt>),
    Name { name: Ident, alias: Option<Ident> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct If {
    pub cond: Expr,
    pub then: Block,
    /// A block, or another `if` for `else if`.
    pub otherwise: Option<Box<Stmt>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Switch {
    pub value: Expr,
    pub arms: Vec<Arm>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Arm {
    /// The values it matches, `a | b`; empty for `_`.
    pub patterns: Vec<Expr>,
    pub guard: Option<Expr>,
    /// A statement, usually an expression.
    pub body: Box<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FnDecl {
    pub name: Ident,
    pub private: bool,
    /// `fn Type.name()`, rhai's method on a type.
    pub this_type: Option<String>,
    /// `fn first<T>(…)`: the type parameters, and the span from `<` to `>`.
    pub type_params: Vec<Ident>,
    pub type_params_span: Option<Span>,
    pub params: Vec<Param>,
    pub result: Option<TypeExpr>,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: Ident,
    pub ty: Option<TypeExpr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    /// `()`.
    Unit,
    /// `none`, or `null`, which is read the same. The span's text says
    /// which was written.
    Null,
    Int(i64),
    Float(f64),
    Str(String),
    Char(char),
    Bool(bool),
    /// A backtick string with `${ }` in it.
    Template(Vec<TemplatePart>),
    Array(Vec<Expr>),
    /// `{ a: 1 }`, or `#{ a: 1 }` when `hash`.
    Map { entries: Vec<MapEntry>, hash: bool },
    Var(String),
    /// `a::b::c`: a name in a module.
    Path(Vec<Ident>),
    /// `this`, inside a function.
    This,
    /// `name(args)`, `a::b(args)`. `bang` is rhai's `f!(…)`.
    Call { callee: Vec<Ident>, args: Vec<Expr>, bang: bool },
    /// `recv.name(args)`, or `recv?.name(args)` when `optional`.
    Method { recv: Box<Expr>, name: Ident, args: Vec<Expr>, optional: bool },
    Field { base: Box<Expr>, name: Ident, optional: bool },
    Index { base: Box<Expr>, index: Box<Expr>, optional: bool },
    /// `-x`, `+x`, `!x`.
    Unary { op: &'static str, expr: Box<Expr> },
    /// Every binary operator, `&&`, `||`, `??`, `in`, `..` and the rest.
    Binary { op: &'static str, lhs: Box<Expr>, rhs: Box<Expr> },
    /// `x is T`.
    Is { expr: Box<Expr>, ty: TypeExpr },
    /// `|a, b| body` or `(a, b) => body`. The body is a statement, as it is in
    /// the fork: `x => x.done` is an expression statement.
    Closure { params: Vec<Param>, body: Box<Stmt>, arrow: bool },
    /// `setInterval(args) { body }`.
    Interval { args: Vec<Expr>, body: Block },
    /// A statement used as a value: an `if`, a `switch`, a block, a loop, and
    /// the `break`, `return` or `throw` the fork allows after `??`.
    Stmt(Box<Stmt>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum TemplatePart {
    Text(String),
    Code(Block),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapEntry {
    pub key: Ident,
    /// Written as a string, `"x-y": 1`.
    pub quoted: bool,
    pub value: Expr,
}

/// A type as written.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeExpr {
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeKind {
    /// `int`, `Task`.
    Name(String),
    /// `Page<int>`, `Array<T>`, `Map<string, T>`: a name given type arguments.
    Generic { name: String, args: Vec<TypeExpr> },
    /// `"all"`.
    Literal(String),
    /// `none`, or `null`.
    Null,
    /// `T[]`.
    Array(Box<TypeExpr>),
    /// `T?`.
    Optional(Box<TypeExpr>),
    /// `A | B`, flat.
    Union(Vec<TypeExpr>),
    /// `{ id: int, note?: string }`.
    Record(Vec<TypeField>),
    /// `{ [string]: T }`.
    Dict { key: String, value: Box<TypeExpr> },
    /// `(A, B) => R`.
    Function(Vec<TypeExpr>, Box<TypeExpr>),
    /// `(T)`.
    Paren(Box<TypeExpr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeField {
    pub name: Ident,
    pub optional: bool,
    pub ty: TypeExpr,
}

impl Stmt {
    /// Whether the fork lets this statement end without a `;` before the next
    /// one: those that end in a `}` of their own.
    pub fn is_self_terminated(&self) -> bool {
        matches!(
            self.kind,
            StmtKind::If(_)
                | StmtKind::Switch(_)
                | StmtKind::While { .. }
                | StmtKind::For { .. }
                | StmtKind::Block(_)
                | StmtKind::Try { .. }
                | StmtKind::Fn(_)
                | StmtKind::Type { .. }
                | StmtKind::Lifecycle { .. }
                // Line-based until step 2, so neither ever needed its `;`.
                | StmtKind::Computed { .. }
                | StmtKind::Prop(_)
                | StmtKind::Use(_)
        )
    }
}
