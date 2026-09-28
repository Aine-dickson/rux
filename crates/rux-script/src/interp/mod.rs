//! Rux's interpreter: runs the typed IR.
//!
//! Step 5 of `docs/11-next.md`. A file is checked and lowered to a
//! [`Unit`] once, when it loads; what the runtime hands in afterwards (a
//! binding, a handler, an effect's body, a component's script) is lowered
//! against that unit when it is first run, and kept. See
//! [`crate::lower::lower_piece`].
//!
//! It is a tree walker over the IR. Every name is a slot, every operator is
//! chosen, and every value is one of [`V`]'s. A typed operation that meets a
//! value of another kind (a `float` where the checker saw an `int`, which
//! the runtime's single number type can produce) is done the way the dynamic
//! operation would do it, so the answer never depends on how much the
//! checker knew.

pub mod value;
pub mod stdlib;
pub mod flat;
mod task;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use rux_ir::ir::{self, *};
use rux_reactive::Value;

pub use value::V;
use value::Closure;

use crate::lower::{lower_piece, Lowered};
use crate::MAX_OPERATIONS;

/// How deep calls may nest before the run is stopped, so a function that
/// calls itself forever is an error and not a crash. A release build's
/// calls take 1.7 KB of Rust's stack interpreted, 2.1 KB compiled and 0.4 KB
/// typed (measured 2026-09-28 by `tests/stack.rs`), so [`STACK_BUDGET`]
/// holds about 376 of the largest; 256 leaves room for a call made from
/// deep inside an expression, which takes more. Going far past that (Dart
/// nests about 57 000) needs smaller frames or a stack that grows, not a
/// larger number here: step 10.4 of `docs/11-next.md`.
pub(crate) const MAX_DEPTH: usize = if cfg!(debug_assertions) { 128 } else { 256 };

/// How much of Rust's stack a run may use, measured from where it began.
/// The depth above is not enough on its own: a debug build's frames are
/// large, and 128 of them overflowed a 2 MB thread (found 2026-09-27, while
/// proving step 9), which is a crash, not an error. Windows gives a
/// program's main thread 1 MB, and the window runs script on it. A debug
/// build spent about 22 KB of it per script call, which stopped it near 35
/// deep, until [`Interp::expr`] was split (watchlist 35): now about 6 KB, so
/// it meets `MAX_DEPTH` first too, as a release build does.
/// `crates/rux-aot-tests/tests/stack.rs` measures it.
const STACK_BUDGET: usize = 768 * 1024;

/// What a run past its step budget of `max` is told, wherever it counted.
pub(crate) fn too_many(max: u64) -> Flow {
    Flow::Fault(Fault::new(format!("Too many operations: more than {max}")))
}

/// What a run nested too deep is told, whichever limit it met.
fn too_deep<T>() -> R<T> {
    fail(format!("Stack overflow: calls nested too deep (at most {MAX_DEPTH})"))
}

/// Something that went wrong, or was thrown, while running.
#[derive(Clone, Debug)]
pub struct Fault {
    /// Said the way the fork said it, so the runtime's wording applies:
    /// `Variable not found: x`, `Property not found: y`.
    pub message: String,
    /// What `catch e` reads as `e.kind`.
    pub kind: &'static str,
    /// The value a `throw` threw, which `catch` gives back as it was.
    pub thrown: Option<V>,
    /// The statement it happened in, as offsets into the text that
    /// statement was lowered from.
    pub at: Option<At>,
}

impl Fault {
    pub fn new(message: impl Into<String>) -> Self {
        Fault { message: message.into(), kind: "error", thrown: None, at: None }
    }
}

pub enum Flow {
    Break,
    Continue,
    Return(V),
    Fault(Fault),
}

impl From<Fault> for Flow {
    fn from(f: Fault) -> Self {
        Flow::Fault(f)
    }
}

type R<T> = Result<T, Flow>;

fn fail<T>(message: impl Into<String>) -> R<T> {
    Err(Flow::Fault(Fault::new(message)))
}

/// One running body: its locals, and, in a closure, what it captured.
#[derive(Default)]
struct Frame {
    slots: Vec<V>,
    /// Whether each slot has been given a value yet.
    set: Vec<bool>,
    captures: Vec<V>,
}

/// A piece of text the runtime runs, lowered.
struct Compiled {
    lowered: Lowered,
    /// The names handed in with it, in order.
    given: Vec<String>,
    /// Whether it sees the unit's state.
    globals: bool,
}

/// What a tracked run wrote: each global's value before its first write.
#[derive(Default)]
struct Track {
    before: HashMap<u32, V>,
    /// Changed by a tracked run inside this one.
    changed: HashSet<u32>,
}

/// A running piece, for finding a name nothing declared.
struct Root {
    frame: usize,
    piece: Rc<Compiled>,
}

pub struct Interp {
    unit: Rc<Unit>,
    globals: Vec<V>,
    set: Vec<bool>,
    by_name: HashMap<String, GlobalId>,
    /// Each native export called so far, by linked name, so the registry
    /// is asked once.
    natives: HashMap<String, rux_native::Call>,
    /// Each function's closures, numbered by [`ir::closures`], for compiled
    /// code that creates one.
    closure_codes: HashMap<u32, Vec<Rc<ir::Closure>>>,
    /// The unit's compiled functions, when a build registered them for its
    /// exact text: step 9 of `docs/11-next.md`, and [`crate::aot`].
    aot: HashMap<u32, (u32, bool, crate::aot::Body)>,
    /// Compiled closure bodies, by the address of the code they were
    /// compiled from: a closure runs compiled however it was created, by
    /// compiled code or by the interpreter, when its code is in this unit.
    /// Looked up when a closure is made ([`Interp::make_closure`]).
    closure_aot: HashMap<usize, value::Compiled>,
    /// Compiled calls running without a frame, which count toward
    /// [`MAX_DEPTH`] as frames do.
    frameless: usize,
    /// Where on Rust's stack the outermost call of the current run began:
    /// what [`STACK_BUDGET`] is measured from.
    stack_base: usize,
    pieces: HashMap<String, Rc<Compiled>>,
    stack: Vec<Frame>,
    roots: Vec<Root>,
    tracks: Vec<Track>,
    ops: u64,
    /// The most steps one run may take: [`MAX_OPERATIONS`] unless lowered.
    max_ops: u64,
    /// Started `async fn`s that are waiting, by task id. See [`task`].
    tasks: HashMap<u64, task::Task>,
    /// Each `async fn`'s ops, compiled when first started, by function.
    flats: HashMap<u32, Result<Rc<flat::Flat>, String>>,
    /// Tasks started and waiting since the runtime last asked.
    started: Vec<u64>,
    /// Tasks that failed with nothing to catch it, since the runtime asked.
    failed: Vec<(String, Fault)>,
    /// What each native call being waited on is for: its ticket, and the task.
    waiting: HashMap<u64, u64>,
    /// What text handed in is linked against before it is lowered: the
    /// document's imports. See [`crate::link`].
    linking: Rc<crate::link::Linking>,
}

impl Drop for Interp {
    /// A document replaced (a reload, a new page) takes its tasks with it,
    /// and nothing is left waiting for their answers.
    fn drop(&mut self) {
        crate::host::abandon(self.waiting.keys().copied());
    }
}

/// How many pieces are kept before starting over, as the fork's cache did.
const PIECES_CAP: usize = 4096;

impl Interp {
    /// An interpreter for `unit`, its state not yet given values: see
    /// [`Interp::init`].
    pub fn new(unit: Unit) -> Self {
        let n = unit.globals.len();
        let by_name = unit.globals.iter().enumerate().map(|(i, g)| (g.name.clone(), GlobalId(i as u32))).collect();
        Interp {
            unit: Rc::new(unit),
            globals: vec![V::None; n],
            set: vec![false; n],
            by_name,
            natives: HashMap::new(),
            closure_codes: HashMap::new(),
            aot: HashMap::new(),
            closure_aot: HashMap::new(),
            frameless: 0,
            stack_base: 0,
            pieces: HashMap::new(),
            stack: Vec::new(),
            roots: Vec::new(),
            tracks: Vec::new(),
            ops: 0,
            max_ops: MAX_OPERATIONS,
            tasks: HashMap::new(),
            flats: HashMap::new(),
            started: Vec::new(),
            failed: Vec::new(),
            waiting: HashMap::new(),
            linking: Rc::default(),
        }
    }

    /// Link every piece of text compiled from now on against `linking`.
    pub fn set_linking(&mut self, linking: Rc<crate::link::Linking>) {
        self.linking = linking;
        self.pieces.clear();
    }

    /// An interpreter for a whole script, checked, lowered and with its top
    /// level run: what [`crate::Builder::build`] makes beside the fork.
    pub fn from_script(script: &str) -> Result<Interp, String> {
        let parsed = rux_syntax::parse(script, rux_syntax::Options { declarations: true }).map_err(|e| e.message)?;
        let mut ir = crate::interpreter(&parsed, script);
        ir.init().map_err(|f| f.message)?;
        Ok(ir)
    }

    /// Whether what `target` names was declared an `int`: a signal, or a
    /// field or element of one (`count`, `user.age`, `rows[0].qty`). `false`
    /// for anything whose type is not known here, a row's variable included.
    pub fn declared_int(&self, target: &str) -> bool {
        use crate::types::Type;
        use rux_syntax::ast::{ExprKind as E, StmtKind as S};
        let Ok(script) = rux_syntax::parse(target, rux_syntax::Options::default()) else { return false };
        let [stmt] = &script.stmts[..] else { return false };
        let S::Expr(e) = &stmt.kind else { return false };
        let table = rux_ir::table::Table::new(&self.unit.types);
        fn walk(me: &Interp, table: &rux_ir::table::Table, e: &rux_syntax::ast::Expr) -> Option<Type> {
            match &e.kind {
                E::Var(n) => me.by_name.get(n).map(|g| me.unit.globals[g.0 as usize].ty.clone()),
                E::Field { base, name, .. } => match table.resolve(&walk(me, table, base)?) {
                    Type::Record(fields) => fields.into_iter().find(|f| f.name == name.name).map(|f| f.ty),
                    Type::Dict(v) => Some(*v),
                    _ => None,
                },
                E::Index { base, .. } => match table.resolve(&walk(me, table, base)?) {
                    Type::Array(t) | Type::Dict(t) => Some(*t),
                    _ => None,
                },
                _ => None,
            }
        }
        match walk(self, &table, e).map(|t| table.resolve(&t)) {
            Some(Type::Int) => true,
            Some(Type::Union(members)) => members.contains(&Type::Int) && !members.contains(&Type::Float),
            _ => false,
        }
    }

    /// Stop a run after `n` steps rather than [`MAX_OPERATIONS`].
    pub fn set_max_operations(&mut self, n: u64) {
        self.max_ops = n;
    }

    pub fn unit(&self) -> &Unit {
        &self.unit
    }

    /// Run the unit's top level, which gives each global its first value.
    pub fn init(&mut self) -> Result<(), Fault> {
        let unit = Rc::clone(&self.unit);
        self.ops = 0;
        let out = self.run_body(&unit.init, Vec::new(), Vec::new());
        // A top-level `let x;` is state that holds `none`, as the fork had it.
        for (i, g) in unit.globals.iter().enumerate() {
            if matches!(g.kind, GlobalKind::Signal | GlobalKind::Let) && !self.set[i] && out.is_ok() {
                self.set[i] = true;
            }
        }
        match out {
            Ok(_) | Err(Flow::Return(_)) => Ok(()),
            Err(Flow::Fault(f)) => Err(f),
            Err(Flow::Break | Flow::Continue) => Err(Fault::new("`break` or `continue` outside a loop")),
        }
    }

    // ----- The state --------------------------------------------------------

    /// Every name the unit's state goes by.
    pub fn global_names(&self) -> impl Iterator<Item = &str> {
        self.unit.globals.iter().enumerate().filter(|(i, _)| self.set[*i]).map(|(_, g)| g.name.as_str())
    }

    pub fn has_global(&self, name: &str) -> bool {
        self.by_name.get(name).is_some_and(|g| self.set[g.0 as usize])
    }

    pub fn global(&self, name: &str) -> Option<&V> {
        let g = self.by_name.get(name)?;
        self.set[g.0 as usize].then(|| &self.globals[g.0 as usize])
    }

    /// Give `name` a value, adding it to the state if it is new: how the
    /// runtime provides the route and its companions.
    pub fn set_global(&mut self, name: &str, value: V) {
        let g = match self.by_name.get(name) {
            Some(g) => *g,
            None => {
                let unit = Rc::make_mut(&mut self.unit);
                let g = GlobalId(unit.globals.len() as u32);
                unit.globals.push(Global { name: name.to_string(), ty: crate::types::Type::Any, kind: GlobalKind::Provided });
                self.by_name.insert(name.to_string(), g);
                self.globals.push(V::None);
                self.set.push(false);
                g
            }
        };
        self.globals[g.0 as usize] = value;
        self.set[g.0 as usize] = true;
    }

    // ----- Running what the runtime hands in -------------------------------

    /// `src` lowered for running with `given` handed in, from the cache when
    /// it has run before. A syntax error comes back as the parser gave it.
    fn compiled(
        &mut self,
        src: &str,
        given: &[String],
        globals: bool,
        scope: Option<&str>,
    ) -> Result<Rc<Compiled>, rux_syntax::SyntaxError> {
        // A component's code sees its own names, not the document's.
        let globals = globals && scope.is_none();
        let mut key = String::with_capacity(src.len() + 32);
        key.push(if globals { 'g' } else { 's' });
        key.push_str(scope.unwrap_or(""));
        key.push('\u{1d}');
        for g in given {
            key.push_str(g);
            key.push('\u{1f}');
        }
        key.push('\u{1e}');
        key.push_str(src);
        if let Some(c) = self.pieces.get(&key) {
            return Ok(Rc::clone(c));
        }
        let mut script = crate::profile::time(crate::profile::Phase::Parse, || {
            rux_syntax::parse(src, rux_syntax::Options::default())
        })?;
        // What does not link is reported where the load checks the text; a
        // name left as it was is refused when it runs.
        let linking = Rc::clone(&self.linking);
        crate::link::resolve(&mut script, linking.aliases_in(scope), &linking.exports);
        let own = linking.own_prefix(scope);
        let unit = Rc::make_mut(&mut self.unit);
        let lowered = crate::profile::time(crate::profile::Phase::Lower, || {
            lower_piece(unit, &script, src, given, globals, own)
        });
        let c = Rc::new(Compiled { lowered, given: given.to_vec(), globals });
        if self.pieces.len() >= PIECES_CAP {
            self.pieces.clear();
        }
        self.pieces.insert(key, Rc::clone(&c));
        Ok(c)
    }

    /// A component's script run to make an instance's state: the locals
    /// its top level declares, and the marker that puts every later run
    /// with that state in the component's scope ([`crate::SCOPE_LOCAL`]).
    pub fn run_instance_init(
        &mut self,
        src: &str,
        scope: &str,
    ) -> Result<(Result<V, Fault>, Vec<(String, V)>), rux_syntax::SyntaxError> {
        let piece = self.compiled(src, &[], false, Some(scope))?;
        let (out, _, mut top) = self.run_piece(piece, Vec::new());
        top.push((crate::SCOPE_LOCAL.to_string(), V::Str(scope.into())));
        Ok((out, top))
    }

    /// Whether `src` parses, without running it.
    pub fn parses(src: &str) -> Result<rux_syntax::ast::Script, rux_syntax::SyntaxError> {
        rux_syntax::parse(src, rux_syntax::Options::default())
    }

    /// Run `src` with `locals` handed in, and give back its value and the
    /// locals as they stand afterwards (a handler may write them).
    pub fn run(
        &mut self,
        src: &str,
        locals: &[(String, Value)],
        globals: bool,
    ) -> Result<(Result<V, Fault>, Vec<V>, Vec<(String, V)>), rux_syntax::SyntaxError> {
        let names: Vec<String> = locals.iter().map(|(n, _)| n.clone()).collect();
        let scope = scope_of(locals);
        let piece = self.compiled(src, &names, globals, scope.as_deref())?;
        let given: Vec<V> = locals.iter().map(|(_, v)| V::from_value(v)).collect();
        Ok(self.run_piece(piece, given))
    }

    fn run_piece(&mut self, piece: Rc<Compiled>, given: Vec<V>) -> (Result<V, Fault>, Vec<V>, Vec<(String, V)>) {
        self.ops = 0;
        let unsupported = piece.lowered.unsupported.first().map(|u| u.what.clone());
        self.enter(&piece, given);
        let out = match unsupported {
            Some(what) => Err(Flow::Fault(Fault::new(format!("{what} is not part of Rux")))),
            None => crate::profile::time(crate::profile::Phase::Run, || self.block(&piece.lowered.body.block)),
        };
        let (after, top) = self.leave(&piece);
        let out = match out {
            Ok(v) | Err(Flow::Return(v)) => Ok(v),
            Err(Flow::Fault(f)) => Err(f),
            Err(Flow::Break | Flow::Continue) => Err(Fault::new("`break` or `continue` outside a loop")),
        };
        (out, after, top)
    }

    /// Make `piece` the running root, with `given` handed in.
    fn enter(&mut self, piece: &Rc<Compiled>, given: Vec<V>) {
        let n = piece.given.len();
        let body = &piece.lowered.body;
        let mut frame = Frame { slots: vec![V::None; body.locals.len()], set: vec![false; body.locals.len()], captures: Vec::new() };
        for (i, v) in given.into_iter().enumerate().take(n) {
            frame.slots[i] = v;
            frame.set[i] = true;
        }
        self.stack.push(frame);
        self.roots.push(Root { frame: self.stack.len() - 1, piece: Rc::clone(piece) });
    }

    /// End the root [`Interp::enter`] began: what was handed in, as it stands
    /// now, and what its top level declared.
    fn leave(&mut self, piece: &Rc<Compiled>) -> (Vec<V>, Vec<(String, V)>) {
        let n = piece.given.len();
        self.roots.pop();
        let frame = self.stack.pop().expect("the piece's frame");
        // A name handed in twice (an instance's state, then a row's local of
        // the same name) is read back by name, as the fork did: each copy
        // gets the value of the last, which is the one a write reached.
        // So does a top-level `let` of the handler's own that shadows one.
        let after: Vec<V> = (0..n)
            .map(|i| {
                let name = &piece.given[i];
                let own = piece.lowered.top.iter().rev().find(|(t, id)| t == name && frame.set[id.0 as usize]);
                let last = match own {
                    Some((_, id)) => id.0 as usize,
                    None => (i..n).rev().find(|&j| piece.given[j] == *name).unwrap_or(i),
                };
                frame.slots[last].clone()
            })
            .collect();
        let top: Vec<(String, V)> = piece
            .lowered
            .top
            .iter()
            .filter(|(_, id)| frame.set[id.0 as usize])
            .map(|(name, id)| (name.clone(), frame.slots[id.0 as usize].clone()))
            .collect();
        (after, top)
    }

    // ----- Tracking ----------------------------------------------------------

    /// Run `f`, and report which globals it changed: each one it wrote whose
    /// value differs from before. A tracked run inside another hands what it
    /// found outwards.
    pub fn tracked<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> (T, HashSet<String>) {
        self.begin_track();
        let out = f(self);
        (out, self.end_track())
    }

    /// Start noting what is written, for [`Interp::end_track`].
    pub fn begin_track(&mut self) {
        self.tracks.push(Track::default());
    }

    /// Stop, and report which globals changed since the matching
    /// [`Interp::begin_track`].
    pub fn end_track(&mut self) -> HashSet<String> {
        let track = self.tracks.pop().expect("a track begun");
        let mut changed: HashSet<u32> = track.changed;
        for (g, before) in track.before {
            if self.globals[g as usize] != before {
                changed.insert(g);
            }
        }
        if let Some(outer) = self.tracks.last_mut() {
            outer.changed.extend(changed.iter().copied());
        }
        changed.into_iter().map(|g| self.unit.globals[g as usize].name.clone()).collect()
    }

    fn wrote(&mut self, g: GlobalId) {
        if let Some(t) = self.tracks.last_mut() {
            if !t.before.contains_key(&g.0) {
                t.before.insert(g.0, self.globals[g.0 as usize].clone());
            }
        }
    }

    fn read_global(&mut self, g: GlobalId) -> R<V> {
        let i = g.0 as usize;
        if !self.set[i] {
            return fail(format!("Variable not found: {}", self.unit.globals[i].name));
        }
        crate::note_read(&self.unit.globals[i].name);
        Ok(self.globals[i].clone())
    }

    // ----- Frames ------------------------------------------------------------

    fn frame(&mut self) -> &mut Frame {
        self.stack.last_mut().expect("a frame")
    }

    fn run_body(&mut self, body: &Body, args: Vec<V>, captures: Vec<V>) -> R<V> {
        self.deeper()?;
        let n = body.locals.len();
        let mut frame = Frame { slots: vec![V::None; n], set: vec![false; n], captures };
        for (i, a) in args.into_iter().enumerate().take(n) {
            frame.slots[i] = a;
            frame.set[i] = true;
        }
        self.stack.push(frame);
        let out = self.block(&body.block);
        self.stack.pop();
        out
    }

    // ----- What compiled code calls: see `crate::aot` ------------------------

    /// Use `table`, compiled for this unit's exact text, for its functions
    /// and its closures' bodies.
    pub(crate) fn set_aot(&mut self, table: crate::aot::Table) {
        self.aot = table.fns;
        for (id, k, locals, framed, body) in table.closures {
            if let Ok(code) = self.closure_code(id, k) {
                self.closure_aot.insert(Rc::as_ptr(&code) as usize, (locals, framed, body));
            }
        }
    }

    /// How many closure bodies run compiled.
    pub fn compiled_closures(&self) -> usize {
        self.closure_aot.len()
    }

    /// How many of the unit's functions run compiled.
    pub fn compiled_functions(&self) -> usize {
        self.aot.len()
    }

    #[inline]
    pub(crate) fn enter_call(&mut self) -> R<()> {
        self.deeper()?;
        self.frameless += 1;
        Ok(())
    }

    /// Whether one more call may nest: under [`MAX_DEPTH`], and within
    /// [`STACK_BUDGET`] of where the run began.
    #[inline]
    fn deeper(&mut self) -> R<()> {
        self.deeper_at(self.depth())
    }

    /// How deep calls are nested now: frames and frameless calls.
    #[inline]
    pub(crate) fn depth(&self) -> usize {
        self.stack.len() + self.frameless
    }

    /// [`Interp::deeper`] for a call made at `depth`, which typed code
    /// counts itself rather than in [`Interp::frameless`].
    #[inline]
    pub(crate) fn deeper_at(&mut self, depth: usize) -> R<()> {
        let probe = 0u8;
        let here = std::ptr::addr_of!(probe) as usize;
        // The outermost call of a run: a handler's or a binding's own frame
        // is pushed without coming here, so its first call is at depth 1.
        if depth <= 1 {
            self.stack_base = here;
            return Ok(());
        }
        if depth >= MAX_DEPTH || self.stack_base.abs_diff(here) > STACK_BUDGET {
            return too_deep();
        }
        Ok(())
    }

    #[inline]
    pub(crate) fn leave_call(&mut self) {
        self.frameless -= 1;
    }

    /// `target op v`, as an assignment with an operator works it out.
    pub(crate) fn apply_pub(&mut self, op: BinOp, target: &V, v: V) -> R<V> {
        match (op, target) {
            (BinOp::Dyn("+") | BinOp::ConcatArray, V::Array(items)) if !matches!(v, V::Array(_)) => {
                let mut items = Rc::clone(items);
                Rc::make_mut(&mut items).push(v);
                Ok(V::Array(items))
            }
            _ => self.binary(op, target.clone(), v),
        }
    }

    pub(crate) fn push_frame(&mut self, n: usize, args: Vec<V>) -> R<()> {
        self.push_frame_with(n, args, Vec::new())
    }

    /// A frame for a closure's body: its locals, `args` first, and what it
    /// captured.
    pub(crate) fn push_frame_with(&mut self, n: usize, args: Vec<V>, captures: Vec<V>) -> R<()> {
        self.deeper()?;
        let mut frame = Frame { slots: vec![V::None; n], set: vec![false; n], captures };
        for (i, a) in args.into_iter().enumerate().take(n) {
            frame.slots[i] = a;
            frame.set[i] = true;
        }
        self.stack.push(frame);
        Ok(())
    }

    pub(crate) fn pop_frame(&mut self) {
        self.stack.pop();
    }

    pub(crate) fn capture(&mut self, i: usize) -> V {
        self.frame().captures[i].clone()
    }

    pub(crate) fn local(&mut self, i: usize) -> V {
        self.frame().slots[i].clone()
    }

    pub(crate) fn set_local(&mut self, i: usize, v: V) {
        let f = self.frame();
        f.slots[i] = v;
        f.set[i] = true;
    }

    pub(crate) fn read_global_pub(&mut self, g: GlobalId) -> R<V> {
        self.read_global(g)
    }

    /// `root = v` or `root op= v`, as [`Interp::assign`] does it for a place
    /// with no steps.
    pub(crate) fn assign_root(&mut self, root: ir::Root, op: Option<BinOp>, v: V) -> R<()> {
        self.assign_value(root, &[], op, v)
    }

    #[inline]
    pub(crate) fn tick_pub(&mut self) -> R<()> {
        self.tick()
    }

    pub(crate) fn call_fn_pub(&mut self, id: FnId, argv: Vec<V>) -> R<V> {
        self.call_fn(id, argv)
    }

    pub(crate) fn binary_pub(&mut self, op: BinOp, a: V, b: V) -> R<V> {
        self.binary(op, a, b)
    }

    /// `b.name`, as a chain with no `?.` reads it.
    pub(crate) fn field_pub(&mut self, b: &V, name: &str) -> R<V> {
        Ok(field(b, name, false)?.unwrap_or(V::None))
    }

    /// `b[i]`, as a chain with no `?[` reads it.
    pub(crate) fn index_pub(&mut self, b: &V, i: &V) -> R<V> {
        Ok(index_of(b, i, false)?.unwrap_or(V::None))
    }

    /// `b?.name`: `None` where the chain stops.
    pub(crate) fn field_opt(&mut self, b: &V, name: &str) -> R<Option<V>> {
        field(b, name, true)
    }

    /// `b?[i]`: `None` where the chain stops.
    pub(crate) fn index_opt(&mut self, b: &V, i: &V) -> R<Option<V>> {
        index_of(b, i, true)
    }

    /// What `catch e` binds `e` to for failure `f`.
    pub(crate) fn caught_pub(f: &Fault) -> V {
        f.thrown.clone().unwrap_or_else(|| error_value(f))
    }

    /// `v is ty`.
    pub(crate) fn is_pub(v: &V, ty: &rux_ir::types::Type) -> bool {
        crate::validate::fits(v, ty, &crate::validate::known)
    }

    /// Whether `switch` pattern `p` matches `v`.
    pub(crate) fn matches_pub(p: &V, v: &V) -> bool {
        match (p, v.whole()) {
            (V::Range(a, b), Some(x)) => *a <= x && x < *b,
            _ => p == v,
        }
    }

    /// `recv.name(args)` for a method that does not change its receiver.
    pub(crate) fn method_pub(&mut self, recv: V, name: &str, argv: Vec<V>) -> R<V> {
        let mut recv = recv;
        self.method_mut(&mut recv, name, argv)
    }

    /// What `for x in over` walks, item by item.
    pub(crate) fn items_pub(over: V) -> R<Items> {
        items_of(over)
    }

    /// `throw v`, as the interpreter throws it.
    pub(crate) fn thrown_pub(v: V) -> Flow {
        let message = match &v {
            V::Map(_) | V::Rec(_) => v.field_of("message").map(V::display).unwrap_or_else(|| v.display()),
            other => other.display(),
        };
        Flow::Fault(Fault { message, kind: "error", thrown: Some(v), at: None })
    }

    pub(crate) fn builtin_pub(&mut self, name: &str, argv: Vec<V>) -> R<V> {
        self.builtin(name, argv)
    }

    /// A native export called without `await`, as [`Interp::call`] calls one.
    pub(crate) fn native_pub(&mut self, key: &str, argv: Vec<V>) -> R<V> {
        match self.native(key)? {
            rux_native::Call::Sync(f) => call_native(&f, &argv),
            rux_native::Call::Async(_) => {
                let name = key.replace('/', "::");
                fail(format!("{name} is `async` in Rust: await it inside an `async fn`"))
            }
        }
    }

    pub(crate) fn call_value_pub(&mut self, f: V, argv: Vec<V>) -> R<V> {
        self.call_value(f, argv)
    }

    pub(crate) fn call_named_pub(&mut self, name: &str, argv: Vec<V>) -> R<V> {
        self.call_named(name, argv)
    }

    pub(crate) fn unit_rc(&self) -> Rc<Unit> {
        Rc::clone(&self.unit)
    }

    pub(crate) fn stmt_pub(&mut self, s: &Stmt) -> R<()> {
        self.stmt(s)
    }

    pub(crate) fn expr_pub(&mut self, e: &Expr) -> R<V> {
        self.expr(e)
    }

    #[inline]
    fn tick(&mut self) -> R<()> {
        self.ops += 1;
        if self.ops > self.max_ops {
            return Err(too_many(self.max_ops));
        }
        Ok(())
    }

    /// The steps taken this run, and the budget: typed compiled code
    /// counts in a local of its own and hands the count back
    /// ([`Interp::set_ops`]) before anything else can read it.
    #[inline]
    pub(crate) fn ops(&self) -> (u64, u64) {
        (self.ops, self.max_ops)
    }

    #[inline]
    pub(crate) fn set_ops(&mut self, n: u64) {
        self.ops = n;
    }

    // ----- Statements --------------------------------------------------------

    fn block(&mut self, b: &Block) -> R<V> {
        let last = b.stmts.len();
        for (i, s) in b.stmts.iter().enumerate() {
            if i + 1 == last && b.ty.is_some() {
                if let StmtKind::Expr(e) = &s.kind {
                    self.tick()?;
                    return self.expr(e);
                }
            }
            self.stmt(s)?;
        }
        Ok(V::None)
    }

    fn stmt(&mut self, s: &Stmt) -> R<()> {
        match self.stmt_here(s) {
            Err(Flow::Fault(mut f)) if f.at.is_none() => {
                f.at = Some(s.at);
                Err(Flow::Fault(f))
            }
            other => other,
        }
    }

    fn stmt_here(&mut self, s: &Stmt) -> R<()> {
        self.tick()?;
        match &s.kind {
            StmtKind::Expr(e) => {
                self.expr(e)?;
            }
            StmtKind::Let { local, value } => {
                let v = match value {
                    Some(v) => self.expr(v)?,
                    None => V::None,
                };
                let f = self.frame();
                f.slots[local.0 as usize] = v;
                f.set[local.0 as usize] = true;
            }
            StmtKind::Init { global, value } => {
                let v = self.expr(value)?;
                self.globals[global.0 as usize] = v;
                self.set[global.0 as usize] = true;
            }
            StmtKind::Assign { place, op, value } => self.assign(place, *op, value)?,
            StmtKind::If { cond, then, otherwise } => {
                if self.expr(cond)?.truthy() {
                    self.block(then)?;
                } else if let Some(o) = otherwise {
                    self.block(o)?;
                }
            }
            StmtKind::While { .. } | StmtKind::ForRange { .. } | StmtKind::ForEach { .. } => self.stmt_loop(s)?,
            StmtKind::Break => return Err(Flow::Break),
            StmtKind::Continue => return Err(Flow::Continue),
            StmtKind::Return(v) => {
                let v = match v {
                    Some(v) => self.expr(v)?,
                    None => V::None,
                };
                return Err(Flow::Return(v));
            }
            StmtKind::Throw(_) | StmtKind::Try { .. } => return self.stmt_rest(s),
        }
        Ok(())
    }

    /// A loop, in a frame of its own: see [`Interp::expr`].
    #[cfg_attr(debug_assertions, inline(never))]
    fn stmt_loop(&mut self, s: &Stmt) -> R<()> {
        match &s.kind {
            StmtKind::While { cond, body } => {
                while self.expr(cond)?.truthy() {
                    match self.block(body) {
                        Ok(_) | Err(Flow::Continue) => {}
                        Err(Flow::Break) => break,
                        Err(e) => return Err(e),
                    }
                }
            }
            StmtKind::ForRange { var, from, to, inclusive, body } => {
                let a = self.expr(from)?;
                let b = self.expr(to)?;
                let (Some(a), Some(b)) = (a.whole(), b.whole()) else {
                    return fail("a range needs two whole numbers");
                };
                let end = if *inclusive { b.saturating_add(1) } else { b };
                let mut i = a;
                while i < end {
                    self.tick()?;
                    let f = self.frame();
                    f.slots[var.0 as usize] = V::Int(i);
                    f.set[var.0 as usize] = true;
                    match self.block(body) {
                        Ok(_) | Err(Flow::Continue) => {}
                        Err(Flow::Break) => break,
                        Err(e) => return Err(e),
                    }
                    i += 1;
                }
            }
            StmtKind::ForEach { var, counter, iter, body, .. } => {
                let over = self.expr(iter)?;
                let items = items_of(over)?;
                for (i, item) in items.into_iter().enumerate() {
                    self.tick()?;
                    let f = self.frame();
                    f.slots[var.0 as usize] = item;
                    f.set[var.0 as usize] = true;
                    if let Some(c) = counter {
                        f.slots[c.0 as usize] = V::Int(i as i64);
                        f.set[c.0 as usize] = true;
                    }
                    match self.block(body) {
                        Ok(_) | Err(Flow::Continue) => {}
                        Err(Flow::Break) => break,
                        Err(e) => return Err(e),
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// `throw` and `try`, in a frame of their own.
    #[cfg_attr(debug_assertions, inline(never))]
    fn stmt_rest(&mut self, s: &Stmt) -> R<()> {
        match &s.kind {
            StmtKind::Throw(v) => {
                let v = self.expr(v)?;
                let message = match &v {
                    V::Map(_) | V::Rec(_) => v.field_of("message").map(V::display).unwrap_or_else(|| v.display()),
                    other => other.display(),
                };
                return Err(Flow::Fault(Fault { message, kind: "error", thrown: Some(v), at: None }));
            }
            StmtKind::Try { body, var, catch } => match self.block(body) {
                Err(Flow::Fault(f)) => {
                    if let Some(var) = var {
                        let e = f.thrown.clone().unwrap_or_else(|| error_value(&f));
                        let fr = self.frame();
                        fr.slots[var.0 as usize] = e;
                        fr.set[var.0 as usize] = true;
                    }
                    self.block(catch)?;
                }
                other => {
                    other?;
                }
            },
            _ => {}
        }
        Ok(())
    }

    // ----- Places ------------------------------------------------------------

    /// The steps of a place, with each index worked out.
    fn keys(&mut self, steps: &[PlaceStep]) -> R<Vec<Key>> {
        let mut out = Vec::with_capacity(steps.len());
        for s in steps {
            out.push(match s {
                PlaceStep::Field(n) => Key::Field(n.clone()),
                PlaceStep::Index(e) => Key::Index(self.expr(e)?),
            });
        }
        Ok(out)
    }

    fn assign(&mut self, place: &Place, op: Option<BinOp>, value: &Expr) -> R<()> {
        let keys = self.keys(&place.steps)?;
        let v = self.expr(value)?;
        self.assign_value(place.root, &keys, op, v)
    }

    /// The value `v` written at `root` and `keys`, applying `op` to what is
    /// there first.
    fn assign_value(&mut self, root: ir::Root, keys: &[Key], op: Option<BinOp>, v: V) -> R<()> {
        let target = self.take(root, keys, true)?;
        match self.combine(op, target, v) {
            Ok(v) => self.put(root, keys, v),
            Err((e, target)) => {
                self.put(root, keys, target)?;
                Err(e)
            }
        }
    }

    /// What `target op= v` (or `= v`) leaves, or the failure with `target`
    /// to put back.
    fn combine(&mut self, op: Option<BinOp>, mut target: V, v: V) -> Result<V, (Flow, V)> {
        match op {
            None => Ok(v),
            // `list += item` adds the item, as the fork's `+=` did.
            Some(BinOp::Dyn("+") | BinOp::ConcatArray) if matches!(target, V::Array(_)) && !matches!(v, V::Array(_)) => {
                if let V::Array(items) = &mut target {
                    Rc::make_mut(items).push(v);
                }
                Ok(target)
            }
            Some(op) => match self.binary(op, target.clone(), v) {
                Ok(n) => Ok(n),
                Err(e) => Err((e, target)),
            },
        }
    }

    /// `root.keys = v` (or `op=`), for compiled code: as [`Interp::assign`]
    /// once its keys and value are worked out.
    pub(crate) fn assign_at(&mut self, root: ir::Root, keys: &[Key], op: Option<BinOp>, v: V) -> R<()> {
        self.assign_value(root, keys, op, v)
    }

    /// The same on a value compiled code holds in a Rust variable: a local
    /// nothing tracks, so only the walk and the write are the interpreter's.
    pub(crate) fn assign_in(&mut self, target: &mut V, keys: &[Key], op: Option<BinOp>, v: V) -> R<()> {
        let mut here = target;
        for (i, k) in keys.iter().enumerate() {
            if i + 1 == keys.len() {
                undeclared_write(here, k)?;
            }
            here = step_mut(here, k, false)?;
        }
        let old = std::mem::replace(here, V::None);
        match self.combine(op, old, v) {
            Ok(n) => {
                *here = n;
                Ok(())
            }
            Err((e, old)) => {
                *here = old;
                Err(e)
            }
        }
    }

    /// `root.keys.name(args)` for a method that changes its receiver, for
    /// compiled code: as [`Interp::step`] does it once the keys and
    /// arguments are worked out.
    pub(crate) fn method_at(&mut self, root: ir::Root, keys: &[Key], name: &str, argv: Vec<V>) -> R<V> {
        let mut target = self.take(root, keys, false)?;
        let out = self.method_mut(&mut target, name, argv);
        self.put(root, keys, target)?;
        out
    }

    /// The same on a value compiled code holds in a Rust variable.
    pub(crate) fn method_in(&mut self, target: &mut V, keys: &[Key], name: &str, argv: Vec<V>) -> R<V> {
        let mut here = target;
        for k in keys {
            here = step_mut(here, k, false)?;
        }
        let mut t = std::mem::replace(here, V::None);
        let out = self.method_mut(&mut t, name, argv);
        *here = t;
        out
    }

    /// What a closure captures from `root`, read as [`Interp::expr`] reads
    /// it when it creates one.
    pub(crate) fn captured_pub(&mut self, root: ir::Root) -> R<V> {
        let slot = self.locate(root)?;
        self.read_slot(slot)
    }

    /// Closure `k` of function `id` (numbered by [`ir::closures`]), with
    /// what it captured.
    pub(crate) fn closure_pub(&mut self, id: u32, k: u32, captured: Vec<V>) -> R<V> {
        let code = self.closure_code(id, k)?;
        Ok(self.make_closure(code, captured))
    }

    /// A closure value, with its compiled body when it has one.
    fn make_closure(&self, code: Rc<ir::Closure>, captured: Vec<V>) -> V {
        let compiled = if self.closure_aot.is_empty() {
            None
        } else {
            self.closure_aot.get(&(Rc::as_ptr(&code) as usize)).copied()
        };
        V::Fn(Rc::new(Closure { code, captured, compiled }))
    }

    /// The code of closure `k` of function `id`, numbered by
    /// [`ir::closures`].
    pub(crate) fn closure_code(&mut self, id: u32, k: u32) -> R<Rc<ir::Closure>> {
        if !self.closure_codes.contains_key(&id) {
            let Some(f) = self.unit.fns.get(id as usize) else {
                return fail("compiled code out of step with its source");
            };
            let list = ir::closures(&f.body.block);
            self.closure_codes.insert(id, list);
        }
        match self.closure_codes.get(&id).and_then(|l| l.get(k as usize)) {
            Some(code) => Ok(Rc::clone(code)),
            None => fail("compiled code out of step with its source"),
        }
    }

    /// A name nothing declared, read where it runs, as [`Interp::expr`]
    /// reads [`ExprKind::Outer`].
    pub(crate) fn outer_pub(&mut self, n: u32) -> R<V> {
        let slot = self.outer(n)?;
        if let Slot::Local(..) = slot {
            let name = &self.unit.outer[n as usize];
            crate::note_read(name);
        }
        self.read_slot(slot)
    }

    /// Where a root is: a slot of a frame, or a global.
    fn locate(&mut self, root: ir::Root) -> R<Slot> {
        Ok(match root {
            ir::Root::Local(id) => Slot::Local(self.stack.len() - 1, id.0 as usize),
            ir::Root::Capture(i) => Slot::Capture(i as usize),
            ir::Root::Global(g) => Slot::Global(g),
            ir::Root::Outer(n) => self.outer(n)?,
        })
    }

    /// A name nothing declared, found where it runs: in the piece that is
    /// running, then in the unit's state when the piece sees it.
    fn outer(&mut self, n: u32) -> R<Slot> {
        let name = self.unit.outer.get(n as usize).cloned().unwrap_or_default();
        if let Some(root) = self.roots.last() {
            let locals = &root.piece.lowered.body.locals;
            let frame = &self.stack[root.frame];
            if let Some(i) = (0..locals.len()).rev().find(|&i| frame.set[i] && locals[i].name == name) {
                return Ok(Slot::Local(root.frame, i));
            }
            // A component's code reaches no document state, only what the
            // runtime provides every file.
            if !root.piece.globals {
                return match self.by_name.get(&name) {
                    Some(g) if self.set[g.0 as usize] && self.unit.globals[g.0 as usize].kind == GlobalKind::Provided => {
                        Ok(Slot::Global(*g))
                    }
                    _ => fail(format!("Variable not found: {name}")),
                };
            }
        }
        match self.by_name.get(&name) {
            Some(g) if self.set[g.0 as usize] => Ok(Slot::Global(*g)),
            _ => fail(format!("Variable not found: {name}")),
        }
    }

    fn slot_mut(&mut self, slot: Slot) -> &mut V {
        match slot {
            Slot::Local(f, i) => {
                self.stack[f].set[i] = true;
                &mut self.stack[f].slots[i]
            }
            Slot::Capture(i) => &mut self.frame().captures[i],
            Slot::Global(g) => &mut self.globals[g.0 as usize],
        }
    }

    /// The value at a place, taken out of it: the place holds `none` until
    /// [`Interp::put`] gives it back.
    fn take(&mut self, root: ir::Root, keys: &[Key], writing: bool) -> R<V> {
        let slot = self.locate(root)?;
        self.note_written(root, slot);
        if let Slot::Global(g) = slot {
            if !self.set[g.0 as usize] {
                return fail(format!("Variable not found: {}", self.unit.globals[g.0 as usize].name));
            }
            self.wrote(g);
        }
        let mut here = self.slot_mut(slot);
        for (i, k) in keys.iter().enumerate() {
            if writing && i + 1 == keys.len() {
                undeclared_write(here, k)?;
            }
            here = step_mut(here, k, false)?;
        }
        Ok(std::mem::replace(here, V::None))
    }

    fn put(&mut self, root: ir::Root, keys: &[Key], v: V) -> R<()> {
        let slot = self.locate(root)?;
        if let Slot::Global(g) = slot {
            self.wrote(g);
            self.set[g.0 as usize] = true;
        }
        let mut here = self.slot_mut(slot);
        for k in keys {
            here = step_mut(here, k, true)?;
        }
        *here = v;
        Ok(())
    }

    /// A write is counted as a read of what it writes to, as the fork's
    /// `on_var` counted every name it resolved: an effect that writes `n`
    /// runs again when `n` changes, and the runtime relies on it.
    fn note_written(&self, root: ir::Root, slot: Slot) {
        match (root, slot) {
            (_, Slot::Global(g)) => crate::note_read(&self.unit.globals[g.0 as usize].name),
            (ir::Root::Outer(n), _) => crate::note_read(&self.unit.outer[n as usize]),
            _ => {}
        }
    }

    fn read_slot(&mut self, slot: Slot) -> R<V> {
        Ok(match slot {
            Slot::Local(f, i) => self.stack[f].slots[i].clone(),
            Slot::Capture(i) => self.frame().captures[i].clone(),
            Slot::Global(g) => self.read_global(g)?,
        })
    }

    // ----- Expressions -------------------------------------------------------

    /// An expression's value. Only the cheap kinds are worked out here, and
    /// every other kind in a function of its own: in a debug build a
    /// function's frame holds every temporary of every arm, so one `match`
    /// over every kind cost 17 KB of stack per nested call (watchlist 35).
    /// The kinds a call nests through (a call, a block, an `if`, an
    /// operator) keep their own frames small. Only a debug build is kept
    /// from inlining the parts back: a release build's frames are small
    /// anyway, and its speed is the compiler's to decide.
    fn expr(&mut self, e: &Expr) -> R<V> {
        match &e.kind {
            ExprKind::None => Ok(V::None),
            ExprKind::Bool(b) => Ok(V::Bool(*b)),
            ExprKind::Int(i) => Ok(V::Int(*i)),
            ExprKind::Float(f) => Ok(V::Float(*f)),
            ExprKind::Local(id) => Ok(self.frame().slots[id.0 as usize].clone()),
            ExprKind::Capture(i) => Ok(self.frame().captures[*i as usize].clone()),
            ExprKind::Global(g) => self.read_global(*g),
            ExprKind::Call { callee, args } => self.call(callee, args),
            ExprKind::Check(x) => self.expr(x),
            ExprKind::Block(b) => self.block(b),
            ExprKind::Binary { op, lhs, rhs } => self.expr_binary(*op, lhs, rhs),
            ExprKind::If { cond, then, otherwise } => self.expr_if(cond, then, otherwise.as_ref()),
            ExprKind::Method { .. } | ExprKind::Field { .. } | ExprKind::Index { .. } | ExprKind::Chain(_) => {
                self.expr_step(e)
            }
            ExprKind::Match { value, arms } => self.expr_match(value, arms),
            _ => self.expr_rest(e),
        }
    }

    #[cfg_attr(debug_assertions, inline(never))]
    fn expr_binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> R<V> {
        let a = self.expr(lhs)?;
        let b = self.expr(rhs)?;
        self.binary(op, a, b)
    }

    #[cfg_attr(debug_assertions, inline(never))]
    fn expr_if(&mut self, cond: &Expr, then: &Block, otherwise: Option<&Block>) -> R<V> {
        if self.expr(cond)?.truthy() {
            self.block(then)
        } else if let Some(o) = otherwise {
            self.block(o)
        } else {
            Ok(V::None)
        }
    }

    #[cfg_attr(debug_assertions, inline(never))]
    fn expr_step(&mut self, e: &Expr) -> R<V> {
        let inner = match &e.kind {
            ExprKind::Chain(inner) => inner,
            _ => e,
        };
        Ok(self.step(inner)?.unwrap_or(V::None))
    }

    #[cfg_attr(debug_assertions, inline(never))]
    fn expr_match(&mut self, value: &Expr, arms: &[ir::Arm]) -> R<V> {
        let v = self.expr(value)?;
        for arm in arms {
            let mut hit = arm.patterns.is_empty();
            for p in &arm.patterns {
                let p = self.expr(p)?;
                let matched = match (&p, v.whole()) {
                    (V::Range(a, b), Some(x)) => *a <= x && x < *b,
                    _ => p == v,
                };
                if matched {
                    hit = true;
                    break;
                }
            }
            if hit {
                if let Some(g) = &arm.guard {
                    if !self.expr(g)?.truthy() {
                        continue;
                    }
                }
                return self.block(&arm.body);
            }
        }
        Ok(V::None)
    }

    /// Every other kind of expression: what a call seldom nests through.
    #[cfg_attr(debug_assertions, inline(never))]
    fn expr_rest(&mut self, e: &Expr) -> R<V> {
        Ok(match &e.kind {
            ExprKind::Str(s) => V::str(s.as_str()),
            ExprKind::Template(parts) => {
                let mut out = String::new();
                for p in parts {
                    out.push_str(&self.expr(p)?.display());
                }
                V::str(out)
            }
            ExprKind::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for i in items {
                    out.push(self.expr(i)?);
                }
                V::array(out)
            }
            ExprKind::Map(entries) => {
                let mut m = std::collections::BTreeMap::new();
                for (k, v) in entries {
                    let v = self.expr(v)?;
                    m.insert(k.clone(), v);
                }
                V::Map(Rc::new(m))
            }
            ExprKind::Record(shape, entries) => {
                let mut vals = vec![V::None; shape.len()];
                for (slot, v) in entries {
                    vals[*slot as usize] = self.expr(v)?;
                }
                V::Rec(Rc::new(value::Record { shape: Rc::clone(shape), vals: vals.into_boxed_slice() }))
            }
            ExprKind::Outer(n) => {
                let slot = self.outer(*n)?;
                if let Slot::Local(..) = slot {
                    let name = &self.unit.outer[*n as usize];
                    crate::note_read(name);
                }
                self.read_slot(slot)?
            }
            ExprKind::Start { func, args } => {
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.expr(a)?);
                }
                self.start_task(*func, argv)?;
                V::None
            }
            // Only an `async fn`'s own ops wait; see `task`.
            ExprKind::Await(_) => return fail("`await` is only allowed inside an `async fn`"),
            ExprKind::Unary { op, expr } => {
                let v = self.expr(expr)?;
                unary(*op, v)?
            }
            ExprKind::Logic { and, lhs, rhs } => {
                let a = self.expr(lhs)?;
                if a.truthy() != *and {
                    // `a && b` is `a` when `a` is false; the fork answered a bool.
                    V::Bool(a.truthy())
                } else {
                    V::Bool(self.expr(rhs)?.truthy())
                }
            }
            ExprKind::Coalesce { lhs, rhs } => {
                let a = self.expr(lhs)?;
                if matches!(a, V::None) {
                    self.expr(rhs)?
                } else {
                    a
                }
            }
            ExprKind::Is { expr, ty } => {
                let v = self.expr(expr)?;
                V::Bool(crate::validate::fits(&v, ty, &crate::validate::known))
            }
            ExprKind::Widen(x) => match self.expr(x)? {
                V::Int(i) => V::Float(i as f64),
                other => other,
            },
            ExprKind::Closure(code) => {
                let mut captured = Vec::with_capacity(code.captures.len());
                for r in &code.captures {
                    let slot = self.locate(*r)?;
                    captured.push(self.read_slot(slot)?);
                }
                self.make_closure(Rc::clone(code), captured)
            }
            ExprKind::Interval { args, text, .. } => {
                let mut ms = 0.0;
                if let Some(a) = args.first() {
                    ms = self.expr(a)?.number().unwrap_or(0.0);
                }
                V::Float(crate::start_interval(ms, text.clone()))
            }
            ExprKind::None
            | ExprKind::Bool(_)
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Local(_)
            | ExprKind::Capture(_)
            | ExprKind::Global(_)
            | ExprKind::Call { .. }
            | ExprKind::Check(_)
            | ExprKind::Block(_)
            | ExprKind::Binary { .. }
            | ExprKind::If { .. }
            | ExprKind::Method { .. }
            | ExprKind::Field { .. }
            | ExprKind::Index { .. }
            | ExprKind::Chain(_)
            | ExprKind::Match { .. } => return self.expr(e),
        })
    }

    /// A field, index or method, with the rest of its chain. `None` when a
    /// `?.` or `?[` met `none` and ended the chain.
    fn step(&mut self, e: &Expr) -> R<Option<V>> {
        let base_of = |me: &mut Self, b: &Expr| -> R<Option<V>> {
            match &b.kind {
                ExprKind::Field { .. } | ExprKind::Index { .. } | ExprKind::Method { .. } => me.step(b),
                _ => me.expr(b).map(Some),
            }
        };
        match &e.kind {
            ExprKind::Field { base, name, optional } => {
                let Some(b) = base_of(self, base)? else { return Ok(None) };
                field(&b, name, *optional)
            }
            ExprKind::Index { base, index, optional } => {
                let Some(b) = base_of(self, base)? else { return Ok(None) };
                let i = self.expr(index)?;
                index_of(&b, &i, *optional)
            }
            ExprKind::Method { recv, method, args, optional } => {
                let mut argv = Vec::with_capacity(args.len());
                // A method that changes its receiver changes the place it
                // was read from, as the fork's did.
                if stdlib::mutates(&method.name) && !*optional {
                    if let Some((root, keys)) = self.receiver(recv)? {
                        for a in args {
                            argv.push(self.expr(a)?);
                        }
                        let mut target = self.take(root, &keys, false)?;
                        let out = self.method_mut(&mut target, &method.name, argv);
                        self.put(root, &keys, target)?;
                        return out.map(Some);
                    }
                }
                let Some(r) = base_of(self, recv)? else { return Ok(None) };
                if *optional && matches!(r, V::None) {
                    return Ok(None);
                }
                for a in args {
                    argv.push(self.expr(a)?);
                }
                let mut r = r;
                self.method_mut(&mut r, &method.name, argv).map(Some)
            }
            _ => self.expr(e).map(Some),
        }
    }

    /// A receiver that is a place (a name, then fields and indexes without
    /// `?`), with its indexes worked out. `None` for any other receiver.
    fn receiver(&mut self, e: &Expr) -> R<Option<(ir::Root, Vec<Key>)>> {
        let mut chain = Vec::new();
        let mut at = e;
        let root = loop {
            match &at.kind {
                ExprKind::Local(id) => break ir::Root::Local(*id),
                ExprKind::Capture(i) => break ir::Root::Capture(*i),
                ExprKind::Global(g) => break ir::Root::Global(*g),
                ExprKind::Outer(n) => break ir::Root::Outer(*n),
                ExprKind::Field { base, optional: false, .. } | ExprKind::Index { base, optional: false, .. } => {
                    chain.push(at);
                    at = base;
                }
                _ => return Ok(None),
            }
        };
        let mut keys = Vec::with_capacity(chain.len());
        for step in chain.into_iter().rev() {
            keys.push(match &step.kind {
                ExprKind::Field { name, .. } => Key::Field(name.clone()),
                ExprKind::Index { index, .. } => Key::Index(self.expr(index)?),
                _ => unreachable!("only fields and indexes are kept"),
            });
        }
        Ok(Some((root, keys)))
    }

    // ----- Calls -------------------------------------------------------------

    fn call(&mut self, callee: &Callee, args: &[Expr]) -> R<V> {
        let mut argv = Vec::with_capacity(args.len());
        let func = match callee {
            Callee::Value(f) => Some(self.expr(f)?),
            _ => None,
        };
        for a in args {
            argv.push(self.expr(a)?);
        }
        match callee {
            Callee::Fn(id) => self.call_fn(*id, argv),
            Callee::Value(_) => self.call_value(func.unwrap_or(V::None), argv),
            Callee::Builtin(name) => self.builtin(name, argv),
            Callee::Native(key) => match self.native(key)? {
                rux_native::Call::Sync(f) => call_native(&f, &argv),
                rux_native::Call::Async(_) => {
                    let name = key.replace('/', "::");
                    fail(format!("{name} is `async` in Rust: await it inside an `async fn`"))
                }
            },
            Callee::Dyn(name) => self.call_named(name, argv),
        }
    }

    /// How to call native export `key`, asked of the registry once.
    fn native(&mut self, key: &str) -> R<rux_native::Call> {
        if let Some(c) = self.natives.get(key) {
            return Ok(c.clone());
        }
        match crate::native::call_of(key) {
            Ok(c) => {
                self.natives.insert(key.to_string(), c.clone());
                Ok(c)
            }
            Err(message) => fail(message),
        }
    }

    /// A call to a name only known where it runs.
    fn call_named(&mut self, name: &str, argv: Vec<V>) -> R<V> {
        if let Some(i) = self.unit.fns.iter().position(|f| f.name == name && f.params as usize == argv.len()) {
            return self.call_fn(FnId(i as u32), argv);
        }
        if let Some(n) = self.unit.outer.iter().position(|o| o == name) {
            if let Ok(slot) = self.outer(n as u32) {
                if let V::Fn(c) = self.read_slot(slot)? {
                    return self.call_closure(&c, argv);
                }
            }
        }
        if let Some(g) = self.by_name.get(name).copied() {
            if let V::Fn(c) = self.read_global(g)? {
                return self.call_closure(&c, argv);
            }
        }
        self.builtin(name, argv)
    }

    fn call_fn(&mut self, id: FnId, argv: Vec<V>) -> R<V> {
        let unit = Rc::clone(&self.unit);
        let f = &unit.fns[id.0 as usize];
        // Called by a name found where it runs: started, as the IR's
        // `Start` would.
        if f.is_async {
            self.start_task(id, argv)?;
            return Ok(V::None);
        }
        if flat::everything() {
            return self.call_flat(id, argv);
        }
        if let Some((locals, framed, body)) = self.aot.get(&id.0).copied() {
            return crate::aot::run(self, locals, framed, body, argv);
        }
        match self.run_body(&f.body, argv, Vec::new()) {
            Ok(v) | Err(Flow::Return(v)) => Ok(v),
            Err(Flow::Break | Flow::Continue) => fail("`break` or `continue` outside a loop"),
            Err(e) => Err(e),
        }
    }

    fn call_value(&mut self, f: V, argv: Vec<V>) -> R<V> {
        match f {
            V::Fn(c) => self.call_closure(&c, argv),
            other => fail(format!("a {} is not a function", other.type_name())),
        }
    }

    fn call_closure(&mut self, c: &Rc<Closure>, mut argv: Vec<V>) -> R<V> {
        let code = Rc::clone(&c.code);
        // Called with more than it takes, as `forEach` calls with the index:
        // the rest are not seen. Called with fewer, the rest are `none`.
        argv.truncate(code.params as usize);
        while argv.len() < code.params as usize {
            argv.push(V::None);
        }
        if let Some((locals, framed, body)) = c.compiled {
            if !flat::everything() {
                return crate::aot::run_closure(self, locals, framed, body, argv, c.captured.clone());
            }
        }
        match self.run_body(&code.body, argv, c.captured.clone()) {
            Ok(v) | Err(Flow::Return(v)) => Ok(v),
            Err(Flow::Break | Flow::Continue) => fail("`break` or `continue` outside a loop"),
            Err(e) => Err(e),
        }
    }

    /// A method of the language's, or failing that a function of the file's
    /// called with the receiver first: `x.helper(a)` is `helper(x, a)`.
    fn method_mut(&mut self, recv: &mut V, name: &str, argv: Vec<V>) -> R<V> {
        match self.method(recv, name, &argv)? {
            Some(v) => Ok(v),
            None => {
                let n = argv.len() + 1;
                if let Some(i) = self.unit.fns.iter().position(|f| f.name == name && f.params as usize == n) {
                    let mut all = Vec::with_capacity(n);
                    all.push(recv.clone());
                    all.extend(argv);
                    return self.call_fn(FnId(i as u32), all);
                }
                // Where the receiver's type was not known when this was
                // lowered (a handler's own expressions are `any`), a native
                // method is found by the value itself.
                if let Some(key) = crate::native::method_for_value(recv, name) {
                    let mut all = Vec::with_capacity(n);
                    all.push(recv.clone());
                    all.extend(argv);
                    return match self.native(&key)? {
                        rux_native::Call::Sync(f) => call_native(&f, &all),
                        rux_native::Call::Async(_) => {
                            fail(format!("`{name}` is `async` in Rust: await it inside an `async fn`"))
                        }
                    };
                }
                let mut types = vec![recv.type_name()];
                types.extend(argv.iter().map(V::type_name));
                fail(format!("Function not found: {name} ({})", types.join(", ")))
            }
        }
    }

    // ----- Operators ---------------------------------------------------------

    fn binary(&mut self, op: BinOp, a: V, b: V) -> R<V> {
        use BinOp::*;
        Ok(match (op, &a, &b) {
            (AddInt, V::Int(x), V::Int(y)) => V::Int(x.checked_add(*y).ok_or_else(overflow)?),
            (SubInt, V::Int(x), V::Int(y)) => V::Int(x.checked_sub(*y).ok_or_else(overflow)?),
            (MulInt, V::Int(x), V::Int(y)) => V::Int(x.checked_mul(*y).ok_or_else(overflow)?),
            (AddFloat, V::Float(x), V::Float(y)) => V::Float(x + y),
            (SubFloat, V::Float(x), V::Float(y)) => V::Float(x - y),
            (MulFloat, V::Float(x), V::Float(y)) => V::Float(x * y),
            (Eq, _, _) => V::Bool(a == b),
            (Ne, _, _) => V::Bool(a != b),
            (Concat, _, _) => {
                let mut s = a.display();
                s.push_str(&b.display());
                V::str(s)
            }
            (AddInt | AddFloat | ConcatArray, ..) => dynamic("+", a, b)?,
            (SubInt | SubFloat, ..) => dynamic("-", a, b)?,
            (MulInt | MulFloat, ..) => dynamic("*", a, b)?,
            (Div, ..) => dynamic("/", a, b)?,
            (RemInt | RemFloat, ..) => dynamic("%", a, b)?,
            (PowInt | PowFloat, ..) => dynamic("**", a, b)?,
            (Lt, ..) => dynamic("<", a, b)?,
            (Le, ..) => dynamic("<=", a, b)?,
            (Gt, ..) => dynamic(">", a, b)?,
            (Ge, ..) => dynamic(">=", a, b)?,
            (In, ..) => V::Bool(contains(&b, &a)?),
            (NotIn, ..) => V::Bool(!contains(&b, &a)?),
            (Range | RangeInclusive, ..) => {
                let (Some(x), Some(y)) = (a.whole(), b.whole()) else {
                    return fail("a range needs two whole numbers");
                };
                V::Range(x, if op == RangeInclusive { y.saturating_add(1) } else { y })
            }
            (Dyn(sym), ..) => dynamic(sym, a, b)?,
        })
    }
}

#[derive(Clone, Copy)]
enum Slot {
    Local(usize, usize),
    Capture(usize),
    Global(GlobalId),
}

/// One step into a place: a field by name, or an index worked out.
#[derive(Clone)]
pub enum Key {
    Field(String),
    Index(V),
}

/// What `for` walks, item by item. An array is walked where it is, by
/// index, and kept alive by the walk: a write to it inside the loop copies
/// it first (it is shared), so the loop still walks what it started with,
/// as it did when the walk began with a copy of the whole array.
pub enum Items {
    Shared(Rc<Vec<V>>, usize),
    Owned(std::vec::IntoIter<V>),
    Range(i64, i64),
}

impl Iterator for Items {
    type Item = V;

    #[inline]
    fn next(&mut self) -> Option<V> {
        match self {
            Items::Shared(items, i) => {
                let v = items.get(*i)?.clone();
                *i += 1;
                Some(v)
            }
            Items::Owned(it) => it.next(),
            Items::Range(a, b) => {
                if a >= b {
                    return None;
                }
                *a += 1;
                Some(V::Int(*a - 1))
            }
        }
    }
}

impl Items {
    /// The items as one array, for a resumable `for`, which keeps it in a
    /// slot of its frame.
    fn into_array(self) -> V {
        match self {
            Items::Shared(items, 0) => V::Array(items),
            other => V::array(other.collect()),
        }
    }
}

/// What `for` walks in `over`, item by item.
fn items_of(over: V) -> R<Items> {
    Ok(match over {
        V::Array(items) => Items::Shared(items, 0),
        V::Str(s) => Items::Owned(s.chars().map(|c| V::str(c.to_string())).collect::<Vec<_>>().into_iter()),
        V::Range(a, b) => Items::Range(a, b),
        V::Map(_) | V::Rec(_) => return fail("a map cannot be walked with `for`; walk `keys(m)` or `values(m)`"),
        other => return fail(format!("`for` cannot walk a {}", other.type_name())),
    })
}

/// What a synchronous native export is.
type SyncNative = std::sync::Arc<dyn Fn(Vec<rux_native::Any>) -> Result<rux_native::Any, rux_native::Error> + Send + Sync>;

/// A synchronous native call: the arguments into Rust, and the answer, or
/// the error it threw, back.
fn call_native(f: &SyncNative, argv: &[V]) -> R<V> {
    let args = argv.iter().map(crate::native::to_any).collect();
    f(args).map(crate::native::from_any).map_err(|e| Flow::Fault(native_fault(&e)))
}

/// A native error as a fault, which `catch e` reads as `{ message, kind }`
/// with the Rust error's kind.
pub(crate) fn native_fault(e: &rux_native::Error) -> Fault {
    Fault { message: e.message.clone(), kind: "error", thrown: Some(crate::native::error_value(e)), at: None }
}

/// The value an error is to `catch e`: `{ message, kind }`.
fn error_value(f: &Fault) -> V {
    error_record(V::str(crate::explain(&f.message)), V::str(f.kind))
}

/// An `Error`, `{ message: string, kind: string }`, as a record in that order.
pub(crate) fn error_record(message: V, kind: V) -> V {
    thread_local! {
        static SHAPE: Rc<value::Shape> = value::Shape::closed(&[
            rux_ir::types::Field { name: "message".into(), optional: false, ty: rux_ir::types::Type::String },
            rux_ir::types::Field { name: "kind".into(), optional: false, ty: rux_ir::types::Type::String },
        ]);
    }
    let shape = SHAPE.with(Rc::clone);
    V::Rec(Rc::new(value::Record { shape, vals: vec![message, kind].into_boxed_slice() }))
}

/// A write of a field a record's type does not declare, which the checker
/// refuses: at run time it fails too, with `kind` `"type"`.
pub(crate) fn undeclared<T>(name: &str) -> R<T> {
    Err(Flow::Fault(Fault {
        message: format!("cannot write `{name}`: the record's type has no such field"),
        kind: "type",
        thrown: None,
        at: None,
    }))
}

/// A write of `k` into `here` that its record's type does not declare.
fn undeclared_write(here: &V, k: &Key) -> R<()> {
    let name = match k {
        Key::Field(n) => n.as_str(),
        Key::Index(V::Str(n)) => n,
        Key::Index(_) => return Ok(()),
    };
    match here {
        V::Rec(r) if r.shape.is_closed() && r.shape.slot(name).is_none() => undeclared(name),
        // `=` writes a key that is there; `set` adds one (owner, 2026-09-28).
        V::Rec(r) if r.shape.slot(name).is_none() => new_key(name),
        V::Map(m) if !m.contains_key(name) => new_key(name),
        _ => Ok(()),
    }
}

/// `m.k = v` or `m["k"] = v` where `m` has no `k`: a key is added by `set`.
fn new_key<T>(name: &str) -> R<T> {
    fail(format!("`{name}` is not a key of this map yet: `=` writes a key that is there, and `.set(\"{name}\", value)` adds one"))
}

/// A record's shown fields as a map, for what only a map does: a new key
/// written into a record nothing declared, and `+`.
pub(crate) fn record_map(r: &value::Record) -> std::collections::BTreeMap<String, V> {
    r.entries().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn overflow() -> Flow {
    Flow::Fault(Fault {
        message: "Arithmetic overflow: an `int` went past its limits".into(),
        kind: "overflow",
        thrown: None,
        at: None,
    })
}

/// One step into a value for writing, copying what is shared. `create`
/// lets a write make a map's missing key.
fn step_mut<'v>(here: &'v mut V, k: &Key, create: bool) -> R<&'v mut V> {
    // A record's field: its slot, or, for a new name, an error (a declared
    // type's record) or a map made of it (one nothing declared).
    let name = match k {
        Key::Field(n) => Some(n.as_str()),
        Key::Index(V::Str(n)) => Some(&**n),
        Key::Index(_) => None,
    };
    if let (V::Rec(r), Some(name)) = (&*here, name) {
        match r.shape.slot(name) {
            Some(_) => {}
            None if !create => return fail(format!("Property not found: {name}")),
            None if r.shape.is_closed() => return undeclared(name),
            None => {
                let m = record_map(r);
                *here = V::Map(Rc::new(m));
            }
        }
    }
    match (here, k) {
        (V::Rec(r), Key::Field(_) | Key::Index(V::Str(_))) => {
            let i = r.shape.slot(name.expect("a name")).expect("found above");
            Ok(&mut Rc::make_mut(r).vals[i])
        }
        (V::Map(m), Key::Field(name)) => {
            let m = Rc::make_mut(m);
            if !m.contains_key(name) {
                if !create {
                    return fail(format!("Property not found: {name}"));
                }
                m.insert(name.clone(), V::None);
            }
            Ok(m.get_mut(name).expect("just made"))
        }
        (V::Map(m), Key::Index(V::Str(name))) => {
            let m = Rc::make_mut(m);
            if !m.contains_key(name.as_ref()) {
                if !create {
                    return fail(format!("Property not found: {name}"));
                }
                m.insert(name.to_string(), V::None);
            }
            Ok(m.get_mut(name.as_ref()).expect("just made"))
        }
        (V::Array(items), Key::Index(i)) => {
            let n = items.len();
            let at = array_index(n, i)?;
            Ok(&mut Rc::make_mut(items)[at])
        }
        (V::None, _) => fail("cannot write into `none`"),
        (other, Key::Field(name)) => fail(format!("cannot write `{name}` on a {}", other.type_name())),
        (other, Key::Index(_)) => fail(format!("cannot index into a {}", other.type_name())),
    }
}

/// Where `i` points in a list of `n`: a negative index counts from the end,
/// as the fork's did.
fn array_index(n: usize, i: &V) -> R<usize> {
    let Some(i) = i.whole() else {
        return match i {
            V::Float(_) => fail("an index must be a whole number"),
            other => fail(format!("an index must be a whole number, not a {}", other.type_name())),
        };
    };
    let at = if i < 0 { n as i64 + i } else { i };
    if at < 0 || at as usize >= n {
        return fail(format!("Array index {i} out of bounds: only {n} elements"));
    }
    Ok(at as usize)
}

fn field(b: &V, name: &str, optional: bool) -> R<Option<V>> {
    match b {
        V::Map(m) => match m.get(name) {
            Some(v) => Ok(Some(v.clone())),
            None if optional => Ok(None),
            None => fail(format!("Property not found: {name}")),
        },
        V::Rec(r) => match r.get(name) {
            Some(v) => Ok(Some(v.clone())),
            None if optional => Ok(None),
            None => fail(format!("Property not found: {name}")),
        },
        V::None if optional => Ok(None),
        V::Array(items) if name == "length" => Ok(Some(V::Int(items.len() as i64))),
        V::Str(s) if name == "length" => Ok(Some(V::Int(s.chars().count() as i64))),
        V::Element(el) => Ok(Some(stdlib::element_field(el, name)?)),
        other => {
            if optional {
                return Ok(None);
            }
            fail(format!("Property not found: {name} on a {}", other.type_name()))
        }
    }
}

fn index_of(b: &V, i: &V, optional: bool) -> R<Option<V>> {
    match (b, i) {
        (V::Array(items), _) => {
            let n = items.len();
            match array_index(n, i) {
                Ok(at) => Ok(Some(items[at].clone())),
                Err(_) if optional && i.whole().is_some() => Ok(None),
                Err(e) => Err(e),
            }
        }
        (V::Map(m), V::Str(k)) => match m.get(k.as_ref()) {
            Some(v) => Ok(Some(v.clone())),
            None if optional => Ok(None),
            None => fail(format!("Property not found: {k}")),
        },
        (V::Rec(r), V::Str(k)) => match r.get(k) {
            Some(v) => Ok(Some(v.clone())),
            None if optional => Ok(None),
            None => fail(format!("Property not found: {k}")),
        },
        (V::Str(s), _) => {
            let chars: Vec<char> = s.chars().collect();
            match array_index(chars.len(), i) {
                Ok(at) => Ok(Some(V::str(chars[at].to_string()))),
                Err(_) if optional => Ok(None),
                Err(e) => Err(e),
            }
        }
        (V::None, _) if optional => Ok(None),
        (other, _) => fail(format!("cannot index into a {}", other.type_name())),
    }
}

fn contains(hay: &V, needle: &V) -> R<bool> {
    Ok(match (hay, needle) {
        (V::Array(items), _) => items.iter().any(|x| x == needle),
        (V::Map(m), V::Str(k)) => m.contains_key(k.as_ref()),
        (V::Rec(r), V::Str(k)) => r.has(k),
        (V::Str(s), V::Str(n)) => s.contains(n.as_ref()),
        (V::Range(a, b), n) => n.whole().is_some_and(|x| *a <= x && x < *b),
        (other, _) => return fail(format!("`in` cannot look inside a {}", other.type_name())),
    })
}

/// `unary`, for compiled code.
pub(crate) fn unary_pub(op: UnOp, v: V) -> R<V> {
    unary(op, v)
}

fn unary(op: UnOp, v: V) -> R<V> {
    Ok(match (op, v) {
        (UnOp::Not, v) => V::Bool(!v.truthy()),
        (UnOp::NegInt | UnOp::NegFloat | UnOp::NegDyn, V::Int(i)) => V::Int(i.checked_neg().ok_or_else(overflow)?),
        (UnOp::NegInt | UnOp::NegFloat | UnOp::NegDyn, V::Float(f)) => V::Float(-f),
        (UnOp::PlusDyn, v @ (V::Int(_) | V::Float(_))) => v,
        (_, other) => return fail(format!("Function not found: - ({})", other.type_name())),
    })
}

/// An operator decided by the values it meets.
fn dynamic(op: &str, a: V, b: V) -> R<V> {
    let no = |a: &V, b: &V| -> R<V> {
        fail(format!("Function not found: {op} ({}, {})", a.type_name(), b.type_name()))
    };
    Ok(match op {
        "+" => match (&a, &b) {
            (V::Int(x), V::Int(y)) => V::Int(x.checked_add(*y).ok_or_else(overflow)?),
            (V::Int(_) | V::Float(_), V::Int(_) | V::Float(_)) => V::Float(a.number().unwrap() + b.number().unwrap()),
            (V::Str(_), _) | (_, V::Str(_)) => {
                let mut s = a.display();
                s.push_str(&b.display());
                V::str(s)
            }
            (V::Array(x), V::Array(y)) => {
                let mut out = x.as_ref().clone();
                out.extend(y.iter().cloned());
                V::array(out)
            }
            (V::Map(_) | V::Rec(_), V::Map(_) | V::Rec(_)) => {
                let as_map = |v: &V| match v {
                    V::Map(m) => m.as_ref().clone(),
                    V::Rec(r) => record_map(r),
                    _ => unreachable!(),
                };
                let mut out = as_map(&a);
                out.extend(as_map(&b));
                V::Map(Rc::new(out))
            }
            _ => return no(&a, &b),
        },
        "-" | "*" | "%" => match (&a, &b) {
            (V::Int(x), V::Int(y)) => V::Int(
                match op {
                    "-" => x.checked_sub(*y),
                    "*" => x.checked_mul(*y),
                    _ => {
                        if *y == 0 {
                            return fail("Division by zero: `%` of an `int` by 0");
                        }
                        x.checked_rem(*y)
                    }
                }
                .ok_or_else(overflow)?,
            ),
            (V::Int(_) | V::Float(_), V::Int(_) | V::Float(_)) => {
                let (x, y) = (a.number().unwrap(), b.number().unwrap());
                V::Float(match op {
                    "-" => x - y,
                    "*" => x * y,
                    _ => x % y,
                })
            }
            _ => return no(&a, &b),
        },
        "/" => match (a.number(), b.number()) {
            (Some(x), Some(y)) => V::Float(x / y),
            _ => return no(&a, &b),
        },
        "**" => match (&a, &b) {
            (V::Int(x), V::Int(y)) => {
                if *y < 0 {
                    return fail("Integer raised to a negative power: make the base a float (`2.0 ** n`)");
                }
                V::Int(u32::try_from(*y).ok().and_then(|y| x.checked_pow(y)).ok_or_else(overflow)?)
            }
            (V::Int(_) | V::Float(_), V::Int(_) | V::Float(_)) => V::Float(a.number().unwrap().powf(b.number().unwrap())),
            _ => return no(&a, &b),
        },
        "<" | "<=" | ">" | ">=" => {
            let ord = match (&a, &b) {
                (V::Int(x), V::Int(y)) => x.partial_cmp(y),
                (V::Int(_) | V::Float(_), V::Int(_) | V::Float(_)) => a.number().unwrap().partial_cmp(&b.number().unwrap()),
                (V::Str(x), V::Str(y)) => x.partial_cmp(y),
                // Values of different kinds are never ordered, as in the fork.
                _ => return Ok(V::Bool(false)),
            };
            use std::cmp::Ordering::*;
            V::Bool(match (op, ord) {
                (_, None) => false,
                ("<", Some(o)) => o == Less,
                ("<=", Some(o)) => o != Greater,
                (">", Some(o)) => o == Greater,
                (_, Some(o)) => o != Less,
            })
        }
        "==" => V::Bool(a == b),
        "!=" => V::Bool(a != b),
        _ => return no(&a, &b),
    })
}

/// The component a run belongs to, from the marker its instance's state
/// carries: see [`crate::SCOPE_LOCAL`].
pub(super) fn scope_of(locals: &[(String, Value)]) -> Option<String> {
    locals.iter().rev().find(|(n, _)| n == crate::SCOPE_LOCAL).and_then(|(_, v)| match v {
        Value::Text(t) => Some(t.clone()),
        _ => None,
    })
}
