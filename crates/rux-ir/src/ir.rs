//! The typed IR: what a Rux file means, once, for every backend.
//!
//! Step 4 of `docs/11-next.md`. The checker has worked out every type; this is
//! the program with those types written in and every name resolved, so a
//! backend never looks a name up or asks what `+` means. Rux's interpreter
//! (step 5) runs it, and Rust generation (step 9) reads it.
//!
//! It is a tree, not bytecode, because generated Rust wants structured control
//! flow, and it keeps generic functions generic: an interpreter runs one body
//! for every type, and compiling once per type is the code generator's job.
//!
//! Three rules hold everywhere, and [`crate::verify`] checks them:
//!
//! 1. **Every expression carries its type** ([`Expr::ty`]), with narrowing
//!    applied: a read of a `Task?` inside `if t != none` is a `Task`.
//! 2. **Every name is resolved** to a slot: a [`Local`] of the frame it is
//!    read in, a capture of a closure, a [`Global`] of the unit, a function,
//!    or a built-in.
//! 3. **Every conversion is written**: an `int` becoming a `float` is a
//!    [`ExprKind::Widen`], an `any` entering typed code is a
//!    [`ExprKind::Check`]. Nothing converts by itself.
//!
//! What the language does with `any` is kept: an operation on one is a
//! dynamic operation ([`BinOp::Dyn`], [`Callee::Dyn`], a field read with no
//! known record), decided when it runs, which is what keeps a half-finished
//! file running under hot reload.

use crate::types::Type;

/// Where something was written, as byte offsets into the text it came from:
/// the file's script, or a template piece's own text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct At {
    pub start: u32,
    pub end: u32,
}

/// One file: a document, a component or (step 7) a script-only module.
#[derive(Clone, Debug, Default)]
pub struct Unit {
    /// Every type it can name, with its type parameters: its own, what it
    /// imports, and the built-in `Result`.
    pub types: Vec<(String, Vec<String>, Type)>,
    /// Its state: signals, props, computeds and the names the runtime
    /// provides. A [`GlobalId`] indexes this.
    pub globals: Vec<Global>,
    /// Its functions. A [`FnId`] indexes this.
    pub fns: Vec<Func>,
    /// What runs when it loads: the top-level statements in order, which
    /// give each signal its first value. Its frame holds the top level's
    /// temporaries, which a top-level `let` without `signal` is not: that is
    /// a global.
    pub init: Body,
    /// `computed name = value`, refreshed in this order.
    pub computeds: Vec<Computed>,
    pub effects: Vec<Body>,
    pub mounted: Vec<Body>,
    pub unmounted: Vec<Body>,
    /// The template's bindings and handlers.
    pub pieces: Vec<Piece>,
    /// What the file says that the IR has no node for: the fork's own syntax
    /// that the language reference removes. A unit with any of these is not
    /// run from the IR.
    pub unsupported: Vec<Unsupported>,
    /// The names read or written that nothing in the file declares, which
    /// [`Root::Outer`] and [`ExprKind::Outer`] index. Found where they run,
    /// in whoever called: a component's function reading its instance's
    /// state, a component reading the document's. See `docs/11-next.md`,
    /// "What a function can see"; step 7 settles these with stores.
    pub outer: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GlobalId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FnId(pub u32);

/// A slot of the frame an expression runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalId(pub u32);

#[derive(Clone, Debug)]
pub struct Global {
    pub name: String,
    pub ty: Type,
    pub kind: GlobalKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalKind {
    /// `let n = signal(0)`: state, and a binding that reads it subscribes.
    Signal,
    /// A top-level `let` without `signal`.
    Let,
    /// `computed`: state the unit refreshes itself.
    Computed,
    /// `prop`: given by the tag, never written here.
    Prop,
    /// Given by the runtime: the route, `params`, `query`, `canBack`, ...
    Provided,
}

#[derive(Clone, Debug)]
pub struct Computed {
    pub global: GlobalId,
    /// The expression, run in its own frame.
    pub value: Body,
}

/// A frame and what runs in it: a function's body, a handler, an effect.
/// Parameters, when there are any, are its first locals.
#[derive(Clone, Debug, Default)]
pub struct Body {
    pub locals: Vec<Local>,
    pub block: Block,
}

#[derive(Clone, Debug)]
pub struct Local {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub struct Func {
    pub name: String,
    pub type_params: Vec<String>,
    /// How many of the body's first locals are parameters.
    pub params: u32,
    pub result: Type,
    pub body: Body,
    pub at: At,
}

/// A piece of the template, and where it sits.
#[derive(Clone, Debug)]
pub struct Piece {
    /// Its text, as the runtime evaluates it today.
    pub src: String,
    /// The file line it is written on.
    pub line: usize,
    /// What it is, as a finding names it: `:disabled on <button>`.
    pub what: String,
    pub kind: PieceKind,
    /// Its frame. The first locals are what the template around it binds:
    /// each enclosing `r-for`'s variable, outermost first, then `event` in a
    /// handler.
    pub body: Body,
    /// How many of the body's first locals the template binds.
    pub given: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceKind {
    /// `{{ }}`, a bound attribute, an `r-if` condition, an `r-for` source.
    Value,
    /// `@name`: statements, run with `event` bound.
    Handler,
    /// `r-model`: names what the input writes into.
    Model,
}

#[derive(Clone, Debug)]
pub struct Unsupported {
    /// The fork's syntax found, in a few words: "`do` loop".
    pub what: String,
    pub at: At,
    /// Which text [`Unsupported::at`] points into: `None` for the script, or
    /// the template piece's index.
    pub piece: Option<usize>,
}

/// Statements, and the value of the block: its last statement when that is
/// an expression, as the language says a block's value is.
#[derive(Clone, Debug, Default)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// The type of the block's value; `none` when it ends in a statement.
    pub ty: Option<Type>,
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub at: At,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Expr(Expr),
    /// `let x = value`: the local gets its first value. `None` for `let x;`,
    /// which holds `none`.
    Let { local: LocalId, value: Option<Expr> },
    /// A global's first value, in the unit's `init`.
    Init { global: GlobalId, value: Expr },
    /// `place = value`, or `place op= value` with the operator it applies.
    Assign { place: Place, op: Option<BinOp>, value: Expr },
    If { cond: Expr, then: Block, otherwise: Option<Block> },
    While { cond: Expr, body: Block },
    /// `for var in from..to` or `from..=to`.
    ForRange { var: LocalId, from: Expr, to: Expr, inclusive: bool, body: Block },
    /// `for var in iter`, and `for (var, counter) in iter`.
    ForEach { var: LocalId, counter: Option<LocalId>, iter: Expr, over: Iterable, body: Block },
    Break,
    Continue,
    Return(Option<Expr>),
    Throw(Expr),
    Try { body: Block, var: Option<LocalId>, catch: Block },
}

/// What a `for` walks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Iterable {
    Array,
    /// A string's characters, each a one-character string.
    Chars,
    /// Decided when it runs.
    Dyn,
}

/// Something written to: a name, then fields and indexes, to any depth.
#[derive(Clone, Debug)]
pub struct Place {
    pub root: Root,
    pub steps: Vec<PlaceStep>,
    /// The type of what is written.
    pub ty: Type,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Root {
    Local(LocalId),
    Capture(u32),
    Global(GlobalId),
    /// A name nothing in the file declares, [`Unit::outer`]'s `n`th, looked
    /// up by name where it runs.
    Outer(u32),
}

#[derive(Clone, Debug)]
pub enum PlaceStep {
    Field(String),
    Index(Expr),
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub ty: Type,
    pub kind: ExprKind,
    pub at: At,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// A backtick string: each part shown as text and joined.
    Template(Vec<Expr>),
    Array(Vec<Expr>),
    /// `{ a: 1 }`, a record or a map by its type.
    Map(Vec<(String, Expr)>),

    Local(LocalId),
    /// The `n`th name a closure captured. See [`Closure::captures`].
    Capture(u32),
    Global(GlobalId),
    /// A read of [`Unit::outer`]'s `n`th name, looked up where it runs.
    Outer(u32),

    Call { callee: Callee, args: Vec<Expr> },
    /// `recv.name(args)`, a method of Rux's own on the receiver's type. With
    /// `optional` (`recv?.name()`), a `none` receiver ends the
    /// [`ExprKind::Chain`] around it with `none`.
    Method { recv: Box<Expr>, method: Method, args: Vec<Expr>, optional: bool },
    /// `base.name`. With `optional`, a `none` base ends the [`ExprKind::Chain`]
    /// around it with `none`.
    Field { base: Box<Expr>, name: String, optional: bool },
    Index { base: Box<Expr>, index: Box<Expr>, optional: bool },
    /// A chain with a `?.` or `?[` in it: where a `none` stops, the value is
    /// `none`.
    Chain(Box<Expr>),

    Unary { op: UnOp, expr: Box<Expr> },
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    /// `a && b`, `a || b`: the right side runs only when it decides.
    Logic { and: bool, lhs: Box<Expr>, rhs: Box<Expr> },
    /// `a ?? b`: the right side runs only when the left is `none`.
    Coalesce { lhs: Box<Expr>, rhs: Box<Expr> },
    /// `x is T`.
    Is { expr: Box<Expr>, ty: Type },

    /// An `int` as a `float`.
    Widen(Box<Expr>),
    /// An `any` checked to be [`Expr::ty`], throwing `kind` `"type"` when it
    /// is not.
    Check(Box<Expr>),

    If { cond: Box<Expr>, then: Block, otherwise: Option<Block> },
    Match { value: Box<Expr>, arms: Vec<Arm> },
    Block(Block),
    Closure(std::rc::Rc<Closure>),
    /// `setInterval(args) { body }`: starts the body running every so often,
    /// and is the handle. `text` is the body as written, without its braces,
    /// which is how the runtime keeps a timer (see `rux_script::TimerRequest`).
    Interval { args: Vec<Expr>, body: Box<Body>, text: String },
}

#[derive(Clone, Debug)]
pub struct Arm {
    /// The values it matches; empty for `_`.
    pub patterns: Vec<Expr>,
    pub guard: Option<Expr>,
    pub body: Block,
}

#[derive(Clone, Debug)]
pub struct Closure {
    /// Its own frame, its parameters first.
    pub params: u32,
    pub body: Body,
    /// The names of the frame around it that it reads or writes, in the
    /// order its body numbers them.
    pub captures: Vec<Root>,
}

#[derive(Clone, Debug)]
pub enum Callee {
    /// A function of the unit's own.
    Fn(FnId),
    /// A global function of the language's: `print`, `parseInt`, `navigate`.
    Builtin(String),
    /// `host::name`, until step 8 makes these native modules.
    Host(String),
    /// A function value: a closure kept in a name.
    Value(Box<Expr>),
    /// A call to something only known when it runs.
    Dyn(String),
}

/// A method, by the kind of value it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Method {
    pub on: MethodOn,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MethodOn {
    Array,
    String,
    Int,
    Float,
    Map,
    /// `unwrap` on a `Result`.
    Result,
    /// Decided when it runs.
    Dyn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Not,
    NegInt,
    NegFloat,
    /// `-x` or `+x` on an `any`.
    NegDyn,
    PlusDyn,
}

/// A binary operator, by the types it works on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    AddInt,
    SubInt,
    MulInt,
    RemInt,
    PowInt,
    AddFloat,
    SubFloat,
    MulFloat,
    /// `/`, which always makes a `float`.
    Div,
    RemFloat,
    PowFloat,
    /// `+` with text on either side: both shown as text and joined.
    Concat,
    /// `+` of two arrays.
    ConcatArray,
    /// `==` and `!=`: values compared structurally.
    Eq,
    Ne,
    /// `<` and the rest, on two numbers or two strings.
    Lt,
    Le,
    Gt,
    Ge,
    /// `a in b`, `!in`.
    In,
    NotIn,
    /// `a..b`, `a..=b`, outside a `for`.
    Range,
    RangeInclusive,
    /// An operator with an `any` on one side, written as it was.
    Dyn(&'static str),
}

impl BinOp {
    /// The operator as written.
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::AddInt | BinOp::AddFloat | BinOp::Concat | BinOp::ConcatArray => "+",
            BinOp::SubInt | BinOp::SubFloat => "-",
            BinOp::MulInt | BinOp::MulFloat => "*",
            BinOp::Div => "/",
            BinOp::RemInt | BinOp::RemFloat => "%",
            BinOp::PowInt | BinOp::PowFloat => "**",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::In => "in",
            BinOp::NotIn => "!in",
            BinOp::Range => "..",
            BinOp::RangeInclusive => "..=",
            BinOp::Dyn(s) => s,
        }
    }

    /// Its name in the printed IR: `add.int`.
    pub fn name(self) -> &'static str {
        match self {
            BinOp::AddInt => "add.int",
            BinOp::SubInt => "sub.int",
            BinOp::MulInt => "mul.int",
            BinOp::RemInt => "rem.int",
            BinOp::PowInt => "pow.int",
            BinOp::AddFloat => "add.float",
            BinOp::SubFloat => "sub.float",
            BinOp::MulFloat => "mul.float",
            BinOp::Div => "div",
            BinOp::RemFloat => "rem.float",
            BinOp::PowFloat => "pow.float",
            BinOp::Concat => "concat",
            BinOp::ConcatArray => "concat.array",
            BinOp::Eq => "eq",
            BinOp::Ne => "ne",
            BinOp::Lt => "lt",
            BinOp::Le => "le",
            BinOp::Gt => "gt",
            BinOp::Ge => "ge",
            BinOp::In => "in",
            BinOp::NotIn => "not-in",
            BinOp::Range => "range",
            BinOp::RangeInclusive => "range.inclusive",
            BinOp::Dyn(_) => "dyn",
        }
    }
}

impl Expr {
    pub fn new(kind: ExprKind, ty: Type, at: At) -> Self {
        Expr { ty, kind, at }
    }
}
