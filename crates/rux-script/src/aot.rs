//! Compiled functions: step 9 of `docs/11-next.md`.
//!
//! A release build writes Rust for a file's functions (`rux-codegen`) and
//! registers it here under a hash of the exact text it was generated from.
//! When the interpreter lowers a file whose text has that hash, a call to one
//! of those functions runs the compiled code instead of walking the tree.
//! Anything else (a hot-reloaded file, a document the build did not see) is
//! interpreted as before: compiled code never runs for text it was not made
//! from.
//!
//! The compiled code runs inside the interpreter, in a frame of the
//! interpreter's own, through [`Cx`]: the one surface it may call. Reading
//! and writing state is tracked by the same calls as the interpreter's, the
//! step budget is the same, and a statement the generator could not compile
//! is handed to the interpreter by its place in the function, in the same
//! frame.

use std::collections::HashMap;
use std::sync::Mutex;

use rux_ir::ir::{Block, FnId, Stmt, StmtKind};

use crate::interp::Interp;

pub use crate::interp::value::V;
pub use crate::interp::{Fault, Flow, Key};
pub use rux_ir::ir::{At, BinOp, GlobalId, LocalId, Root, UnOp};
pub use rux_ir::types::Type;

/// What a compiled step gives: a value, or the flow that leaves it.
pub type R<T> = Result<T, Flow>;

/// A compiled function's body. With a frame, it runs in the frame [`run`]
/// entered for it, its arguments already there, and is handed none; without
/// one (every statement of it compiled), its locals are its own Rust
/// variables and it is handed its arguments.
pub type Body = fn(&mut Cx, Vec<V>) -> R<V>;

/// One compiled function: its id in the unit, how many locals its frame
/// has, whether it runs in a frame of the interpreter's, and its body.
pub type Entry = (u32, u32, bool, Body);

/// A compiled closure body. With a frame, it runs in the frame
/// [`run_closure`] entered for it, arguments and captures already there;
/// without one, it is handed its arguments and then its captures, its own
/// copies, as each call of the interpreter's has.
pub type ClosureBody = fn(&mut Cx, Vec<V>, Vec<V>) -> R<V>;

/// One compiled closure body: the function creating it, its number there
/// (`rux_ir::ir::closures`), how many locals its frame has, whether it runs
/// in a frame, and its body.
pub type ClosureEntry = (u32, u32, u32, bool, ClosureBody);

/// What one text registered: its functions and its closures' bodies.
pub(crate) struct Table {
    pub fns: HashMap<u32, (u32, bool, Body)>,
    pub closures: Vec<ClosureEntry>,
}

type Registered = (&'static [Entry], &'static [ClosureEntry]);

static TABLES: Mutex<Option<HashMap<u64, Registered>>> = Mutex::new(None);

/// Register compiled functions and closure bodies for the file whose text
/// hashes to `hash` (see [`source_hash`]). What the generated `install()`
/// calls.
pub fn register(hash: u64, fns: &'static [Entry], closures: &'static [ClosureEntry]) {
    TABLES.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(hash, (fns, closures));
}

/// What is registered for `hash`.
pub(crate) fn lookup(hash: u64) -> Option<Table> {
    let tables = TABLES.lock().unwrap_or_else(|e| e.into_inner());
    let (fns, closures) = tables.as_ref()?.get(&hash)?;
    Some(Table {
        fns: fns.iter().map(|(id, n, framed, f)| (*id, (*n, *framed, *f))).collect(),
        closures: closures.to_vec(),
    })
}

/// FNV-1a over `parts`, each followed by a separator byte, so `["ab", "c"]`
/// and `["a", "bc"]` differ. A cache key: the compiled code is part of the
/// same binary as the text it was made from.
pub fn source_hash<'a>(parts: impl IntoIterator<Item = &'a str>) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.bytes().chain(std::iter::once(0xff)) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

/// Run compiled `body` as a call: a frame of `locals` slots with `args`
/// first, and what leaves it mapped as the interpreter maps a call's.
pub(crate) fn run(ip: &mut Interp, locals: u32, framed: bool, body: Body, args: Vec<V>) -> R<V> {
    let out = if framed {
        ip.push_frame(locals as usize, args)?;
        let out = body(&mut Cx { ip }, Vec::new());
        ip.pop_frame();
        out
    } else {
        // No frame, but a call all the same: it counts toward the depth a
        // run may nest to, so recursion that never ends is an error, not a
        // crash.
        ip.enter_call()?;
        let out = body(&mut Cx { ip }, args);
        ip.leave_call();
        out
    };
    match out {
        Ok(v) | Err(Flow::Return(v)) => Ok(v),
        Err(Flow::Break | Flow::Continue) => Err(Flow::Fault(Fault::new("`break` or `continue` outside a loop"))),
        Err(e) => Err(e),
    }
}

/// Run compiled closure body `body` as a call, with `args` (already as many
/// as it takes) and what the closure captured.
pub(crate) fn run_closure(ip: &mut Interp, locals: u32, framed: bool, body: ClosureBody, args: Vec<V>, captured: Vec<V>) -> R<V> {
    let out = if framed {
        ip.push_frame_with(locals as usize, args, captured)?;
        let out = body(&mut Cx { ip }, Vec::new(), Vec::new());
        ip.pop_frame();
        out
    } else {
        ip.enter_call()?;
        let out = body(&mut Cx { ip }, args, captured);
        ip.leave_call();
        out
    };
    match out {
        Ok(v) | Err(Flow::Return(v)) => Ok(v),
        Err(Flow::Break | Flow::Continue) => Err(Flow::Fault(Fault::new("`break` or `continue` outside a loop"))),
        Err(e) => Err(e),
    }
}

/// `e`, a failure in the statement at `start..end`, placed there unless it
/// already has a place, as the interpreter places one.
pub fn at(e: Flow, start: u32, end: u32) -> Flow {
    match e {
        Flow::Fault(mut f) if f.at.is_none() => {
            f.at = Some(At { start, end });
            Flow::Fault(f)
        }
        other => other,
    }
}

/// Whether `v` counts as true: JavaScript's rule, as the language's.
pub fn truthy(v: &V) -> bool {
    v.truthy()
}

/// A plain string value.
pub fn text(s: &str) -> V {
    V::str(s)
}

/// What `for x in over` walks, item by item.
pub fn items(over: V) -> R<Vec<V>> {
    Interp::items_pub(over)
}

/// `throw v`: what leaves the statement.
pub fn thrown(v: V) -> Flow {
    Interp::thrown_pub(v)
}

/// What `catch e` binds `e` to for failure `f`.
pub fn caught(f: &Fault) -> V {
    Interp::caught_pub(f)
}

/// Whether `switch` pattern `p` matches `v`: equal, or a range holding it.
pub fn matches(p: &V, v: &V) -> bool {
    Interp::matches_pub(p, v)
}

/// `v is ty`.
pub fn is(v: &V, ty: &Type) -> bool {
    Interp::is_pub(v, ty)
}

/// The type written `text`, for an `is` the generator wrote out: its text
/// is the type's own `Display`, which the generator checked reads back the
/// same.
pub fn type_of_text(text: &str) -> Type {
    rux_ir::types::parse_type(text).unwrap_or(Type::Any)
}

/// `setInterval(ms) { body }`: the timer started, as the interpreter starts
/// one, from the body's text, and its handle. Only the first argument is
/// worked out, as there.
pub fn interval(ms: &V, text: &str) -> V {
    V::Float(crate::start_interval(ms.number().unwrap_or(0.0), text.to_string()))
}

/// What typed code does where Rust's own operation stops (an `int` overflow,
/// `% 0`): the interpreter's operator on the same values, so the failure is
/// its, word for word. Its answer, when it has one, as the kind the checker
/// said it is.
pub fn int_of(r: R<V>) -> R<i64> {
    match r? {
        V::Int(i) => Ok(i),
        _ => Err(out_of_step()),
    }
}

pub fn float_of(r: R<V>) -> R<f64> {
    match r? {
        V::Float(f) => Ok(f),
        _ => Err(out_of_step()),
    }
}

fn out_of_step() -> Flow {
    Flow::Fault(Fault::new("compiled code out of step with its source"))
}

/// A step into a place by field name.
pub fn key(name: &str) -> Key {
    Key::Field(name.to_string())
}

/// An array of `items`.
pub fn array(items: Vec<V>) -> V {
    V::array(items)
}

/// A map or record of `entries`, a later key replacing an earlier one.
pub fn map(entries: Vec<(&str, V)>) -> V {
    V::Map(std::rc::Rc::new(entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect()))
}

/// The methods that change their receiver: `rux-codegen` hands a call to one
/// back, since its receiver is a place. Its own copy of this list is held to
/// this one by `rux-aot-tests`.
pub use crate::interp::stdlib::MUTATING;

/// The interpreter, as compiled code sees it.
pub struct Cx<'a> {
    pub(crate) ip: &'a mut Interp,
}

impl Cx<'_> {
    /// Local `i` of this frame.
    pub fn get(&mut self, i: u32) -> V {
        self.ip.local(i as usize)
    }

    /// What this closure captured, its `i`th, in this frame.
    pub fn capture(&mut self, i: u32) -> V {
        self.ip.capture(i as usize)
    }

    /// Give local `i` a value, as `let` does.
    pub fn set(&mut self, i: u32, v: V) {
        self.ip.set_local(i as usize, v);
    }

    /// Global `g`, read as the interpreter reads one: tracked, and a failure
    /// when it has no value yet.
    pub fn global(&mut self, g: u32) -> R<V> {
        self.ip.read_global_pub(GlobalId(g))
    }

    /// `place = v` or `place op= v` on a local or a global with no fields or
    /// indexes, as the interpreter assigns.
    pub fn assign_local(&mut self, i: u32, op: Option<BinOp>, v: V) -> R<()> {
        self.ip.assign_root(Root::Local(LocalId(i)), op, v)
    }

    pub fn assign_global(&mut self, g: u32, op: Option<BinOp>, v: V) -> R<()> {
        self.ip.assign_root(Root::Global(GlobalId(g)), op, v)
    }

    /// One step of the budget a run may take.
    #[inline]
    pub fn tick(&mut self) -> R<()> {
        self.ip.tick_pub()
    }

    /// A call from typed code straight into another typed function: counted
    /// toward the depth a run may nest to, as [`Cx::call`] counts one.
    /// [`Cx::leave`] after it, whatever it gave.
    #[inline]
    pub fn enter(&mut self) -> R<()> {
        self.ip.enter_call()
    }

    #[inline]
    pub fn leave(&mut self) {
        self.ip.leave_call()
    }

    /// A call to the unit's function `id`: compiled when it is, interpreted
    /// when not.
    pub fn call(&mut self, id: u32, args: Vec<V>) -> R<V> {
        self.ip.call_fn_pub(FnId(id), args)
    }

    /// What `place op= v` makes of `target`, as the interpreter works it
    /// out (`list += item` appends).
    pub fn apply(&mut self, op: BinOp, target: &V, v: V) -> R<V> {
        self.ip.apply_pub(op, target, v)
    }

    pub fn field(&mut self, b: &V, name: &str) -> R<V> {
        self.ip.field_pub(b, name)
    }

    pub fn index(&mut self, b: &V, i: &V) -> R<V> {
        self.ip.index_pub(b, i)
    }

    /// `b?.name`: `None` where a chain stops.
    pub fn field_opt(&mut self, b: &V, name: &str) -> R<Option<V>> {
        self.ip.field_opt(b, name)
    }

    /// `b?[i]`: `None` where a chain stops.
    pub fn index_opt(&mut self, b: &V, i: &V) -> R<Option<V>> {
        self.ip.index_opt(b, i)
    }

    /// A name nothing in the file declares, found where it runs.
    pub fn outer(&mut self, n: u32) -> R<V> {
        self.ip.outer_pub(n)
    }

    /// `root.keys = v` or `root.keys op= v`, the keys worked out.
    pub fn assign_at(&mut self, root: Root, keys: &[Key], op: Option<BinOp>, v: V) -> R<()> {
        self.ip.assign_at(root, keys, op, v)
    }

    /// The same, into a local the compiled code holds itself.
    pub fn assign_in(&mut self, target: &mut V, keys: &[Key], op: Option<BinOp>, v: V) -> R<()> {
        self.ip.assign_in(target, keys, op, v)
    }

    /// A method that changes its receiver, called on the place
    /// `root.keys`, the keys and arguments worked out.
    pub fn method_at(&mut self, root: Root, keys: &[Key], name: &str, args: Vec<V>) -> R<V> {
        self.ip.method_at(root, keys, name, args)
    }

    /// The same, on a local the compiled code holds itself.
    pub fn method_in(&mut self, target: &mut V, keys: &[Key], name: &str, args: Vec<V>) -> R<V> {
        self.ip.method_in(target, keys, name, args)
    }

    /// What a closure being created captures from `root`.
    pub fn captured(&mut self, root: Root) -> R<V> {
        self.ip.captured_pub(root)
    }

    /// Closure `k` of function `id`, as `rux_ir::ir::closures` numbers them,
    /// holding `captured`.
    pub fn closure(&mut self, id: u32, k: u32, captured: Vec<V>) -> R<V> {
        self.ip.closure_pub(id, k, captured)
    }

    /// A method that does not change its receiver.
    pub fn method(&mut self, recv: V, name: &str, args: Vec<V>) -> R<V> {
        self.ip.method_pub(recv, name, args)
    }

    /// A function of the language's: `print`, `parseInt`, `navigate`.
    pub fn builtin(&mut self, name: &str, args: Vec<V>) -> R<V> {
        self.ip.builtin_pub(name, args)
    }

    /// A native export, by linked name.
    pub fn native(&mut self, key: &str, args: Vec<V>) -> R<V> {
        self.ip.native_pub(key, args)
    }

    /// A function value: a closure kept in a name.
    pub fn call_value(&mut self, f: V, args: Vec<V>) -> R<V> {
        self.ip.call_value_pub(f, args)
    }

    /// A call to a name only known where it runs.
    pub fn call_named(&mut self, name: &str, args: Vec<V>) -> R<V> {
        self.ip.call_named_pub(name, args)
    }

    pub fn binary(&mut self, op: BinOp, a: V, b: V) -> R<V> {
        self.ip.binary_pub(op, a, b)
    }

    pub fn unary(&mut self, op: UnOp, v: V) -> R<V> {
        crate::interp::unary_pub(op, v)
    }

    /// Run, in this frame, the statement at `path` of function `id`'s body
    /// (see [`find`]): what the generator could not compile.
    pub fn stmt(&mut self, id: u32, path: &[u32]) -> R<()> {
        let unit = self.ip.unit_rc();
        match find(&unit.fns[id as usize].body.block, path) {
            Some(s) => self.ip.stmt_pub(s),
            None => Err(Flow::Fault(Fault::new("compiled code out of step with its source"))),
        }
    }

    /// The value of the expression statement at `path` of function `id`'s
    /// body, as a block's value is worked out.
    pub fn value(&mut self, id: u32, path: &[u32]) -> R<V> {
        let unit = self.ip.unit_rc();
        self.value_of(find(&unit.fns[id as usize].body.block, path))
    }

    /// [`Cx::stmt`] in the body of closure `k` of function `id`.
    pub fn cstmt(&mut self, id: u32, k: u32, path: &[u32]) -> R<()> {
        let code = self.ip.closure_code(id, k)?;
        match find(&code.body.block, path) {
            Some(s) => self.ip.stmt_pub(s),
            None => Err(Flow::Fault(Fault::new("compiled code out of step with its source"))),
        }
    }

    /// [`Cx::value`] in the body of closure `k` of function `id`.
    pub fn cvalue(&mut self, id: u32, k: u32, path: &[u32]) -> R<V> {
        let code = self.ip.closure_code(id, k)?;
        self.value_of(find(&code.body.block, path))
    }

    fn value_of(&mut self, s: Option<&Stmt>) -> R<V> {
        match s.map(|s| &s.kind) {
            Some(StmtKind::Expr(e)) => {
                self.ip.tick_pub()?;
                self.ip.expr_pub(e)
            }
            _ => Err(Flow::Fault(Fault::new("compiled code out of step with its source"))),
        }
    }
}

/// The statement at `path` in `b`: the first number is its place in `b`,
/// then, for each level down, which of its blocks (an `if`'s `then` 0 and
/// `else` 1, a loop's body 0, a `try`'s body 0 and `catch` 1) and the place
/// in that block. The generator numbers them the same way.
pub fn find<'b>(b: &'b Block, path: &[u32]) -> Option<&'b Stmt> {
    let (first, rest) = path.split_first()?;
    let s = b.stmts.get(*first as usize)?;
    if rest.is_empty() {
        return Some(s);
    }
    let (which, rest) = rest.split_first()?;
    let inner = match (&s.kind, which) {
        (StmtKind::If { then, .. }, 0) => then,
        (StmtKind::If { otherwise: Some(o), .. }, 1) => o,
        (StmtKind::While { body, .. } | StmtKind::ForRange { body, .. } | StmtKind::ForEach { body, .. }, 0) => body,
        (StmtKind::Try { body, .. }, 0) => body,
        (StmtKind::Try { catch, .. }, 1) => catch,
        _ => return None,
    };
    find(inner, rest)
}
