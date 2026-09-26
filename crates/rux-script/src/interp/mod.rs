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

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use rux_ir::ir::{self, *};
use rux_reactive::Value;

pub use value::V;
use value::Closure;

use crate::lower::{lower_piece, Lowered};
use crate::MAX_OPERATIONS;

/// How deep calls may nest before the run is stopped, so a function that
/// calls itself forever is an error and not a crash.
const MAX_DEPTH: usize = 128;

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

enum Flow {
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

pub type Host = Rc<dyn Fn() -> f64>;

pub struct Interp {
    unit: Rc<Unit>,
    globals: Vec<V>,
    set: Vec<bool>,
    by_name: HashMap<String, GlobalId>,
    host: HashMap<String, Host>,
    pieces: HashMap<String, Rc<Compiled>>,
    stack: Vec<Frame>,
    roots: Vec<Root>,
    tracks: Vec<Track>,
    ops: u64,
    /// The most steps one run may take: [`MAX_OPERATIONS`] unless lowered.
    max_ops: u64,
}

/// How many pieces are kept before starting over, as the fork's cache did.
const PIECES_CAP: usize = 4096;

impl Interp {
    /// An interpreter for `unit`, its state not yet given values: see
    /// [`Interp::init`].
    pub fn new(unit: Unit, host: HashMap<String, Host>) -> Self {
        let n = unit.globals.len();
        let by_name = unit.globals.iter().enumerate().map(|(i, g)| (g.name.clone(), GlobalId(i as u32))).collect();
        Interp {
            unit: Rc::new(unit),
            globals: vec![V::None; n],
            set: vec![false; n],
            by_name,
            host,
            pieces: HashMap::new(),
            stack: Vec::new(),
            roots: Vec::new(),
            tracks: Vec::new(),
            ops: 0,
            max_ops: MAX_OPERATIONS,
        }
    }

    /// An interpreter for a whole script, checked, lowered and with its top
    /// level run: what [`crate::Builder::build`] makes beside the fork.
    pub fn from_script(script: &str) -> Result<Interp, String> {
        let parsed = rux_syntax::parse(script, rux_syntax::Options { declarations: true }).map_err(|e| e.message)?;
        let mut ir = crate::interpreter(&parsed, script, &[], HashMap::new());
        ir.init().map_err(|f| f.message)?;
        Ok(ir)
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
    fn compiled(&mut self, src: &str, given: &[String], globals: bool) -> Result<Rc<Compiled>, rux_syntax::SyntaxError> {
        let mut key = String::with_capacity(src.len() + 32);
        key.push(if globals { 'g' } else { 's' });
        for g in given {
            key.push_str(g);
            key.push('\u{1f}');
        }
        key.push('\u{1e}');
        key.push_str(src);
        if let Some(c) = self.pieces.get(&key) {
            return Ok(Rc::clone(c));
        }
        let script = crate::profile::time(crate::profile::Phase::Parse, || {
            rux_syntax::parse(src, rux_syntax::Options::default())
        })?;
        let unit = Rc::make_mut(&mut self.unit);
        let lowered = crate::profile::time(crate::profile::Phase::Lower, || {
            lower_piece(unit, &script, src, given, globals)
        });
        let c = Rc::new(Compiled { lowered, given: given.to_vec(), globals });
        if self.pieces.len() >= PIECES_CAP {
            self.pieces.clear();
        }
        self.pieces.insert(key, Rc::clone(&c));
        Ok(c)
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
        let piece = self.compiled(src, &names, globals)?;
        let given: Vec<V> = locals.iter().map(|(_, v)| V::from_value(v)).collect();
        Ok(self.run_piece(piece, given))
    }

    fn run_piece(&mut self, piece: Rc<Compiled>, given: Vec<V>) -> (Result<V, Fault>, Vec<V>, Vec<(String, V)>) {
        self.ops = 0;
        let n = piece.given.len();
        let body = &piece.lowered.body;
        let unsupported = piece.lowered.unsupported.first().map(|u| u.what.clone());
        let mut frame = Frame { slots: vec![V::None; body.locals.len()], set: vec![false; body.locals.len()], captures: Vec::new() };
        for (i, v) in given.into_iter().enumerate().take(n) {
            frame.slots[i] = v;
            frame.set[i] = true;
        }
        self.stack.push(frame);
        self.roots.push(Root { frame: self.stack.len() - 1, piece: Rc::clone(&piece) });
        let out = match unsupported {
            Some(what) => Err(Flow::Fault(Fault::new(format!("{what} is not part of Rux")))),
            None => crate::profile::time(crate::profile::Phase::Run, || self.block(&body.block)),
        };
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
        let out = match out {
            Ok(v) | Err(Flow::Return(v)) => Ok(v),
            Err(Flow::Fault(f)) => Err(f),
            Err(Flow::Break | Flow::Continue) => Err(Fault::new("`break` or `continue` outside a loop")),
        };
        (out, after, top)
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
        if self.stack.len() >= MAX_DEPTH {
            return fail(format!("Stack overflow: calls nested more than {MAX_DEPTH} deep"));
        }
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

    fn tick(&mut self) -> R<()> {
        self.ops += 1;
        if self.ops > self.max_ops {
            return fail(format!("Too many operations: more than {}", self.max_ops));
        }
        Ok(())
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
                let items: Vec<V> = match over {
                    V::Array(items) => items.as_ref().clone(),
                    V::Str(s) => s.chars().map(|c| V::str(c.to_string())).collect(),
                    V::Range(a, b) => (a..b).map(V::Int).collect(),
                    V::Map(_) => return fail("a map cannot be walked with `for`; walk `keys(m)` or `values(m)`"),
                    other => return fail(format!("`for` cannot walk a {}", other.type_name())),
                };
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
            StmtKind::Break => return Err(Flow::Break),
            StmtKind::Continue => return Err(Flow::Continue),
            StmtKind::Return(v) => {
                let v = match v {
                    Some(v) => self.expr(v)?,
                    None => V::None,
                };
                return Err(Flow::Return(v));
            }
            StmtKind::Throw(v) => {
                let v = self.expr(v)?;
                let message = match &v {
                    V::Map(m) => m.get("message").map(V::display).unwrap_or_else(|| v.display()),
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
        let mut target = self.take(place.root, &keys)?;
        let result = match op {
            None => Ok(v),
            // `list += item` adds the item, as the fork's `+=` did.
            Some(BinOp::Dyn("+") | BinOp::ConcatArray) if matches!(target, V::Array(_)) && !matches!(v, V::Array(_)) => {
                let mut t = std::mem::replace(&mut target, V::None);
                if let V::Array(items) = &mut t {
                    Rc::make_mut(items).push(v);
                }
                Ok(t)
            }
            Some(op) => self.binary(op, target.clone(), v),
        };
        match result {
            Ok(v) => self.put(place.root, &keys, v),
            Err(e) => {
                self.put(place.root, &keys, target)?;
                Err(e)
            }
        }
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
            if !root.piece.globals {
                return fail(format!("Variable not found: {name}"));
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
    fn take(&mut self, root: ir::Root, keys: &[Key]) -> R<V> {
        let slot = self.locate(root)?;
        self.note_written(root, slot);
        if let Slot::Global(g) = slot {
            if !self.set[g.0 as usize] {
                return fail(format!("Variable not found: {}", self.unit.globals[g.0 as usize].name));
            }
            self.wrote(g);
        }
        let mut here = self.slot_mut(slot);
        for k in keys {
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

    fn expr(&mut self, e: &Expr) -> R<V> {
        Ok(match &e.kind {
            ExprKind::None => V::None,
            ExprKind::Bool(b) => V::Bool(*b),
            ExprKind::Int(i) => V::Int(*i),
            ExprKind::Float(f) => V::Float(*f),
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
            ExprKind::Local(id) => self.frame().slots[id.0 as usize].clone(),
            ExprKind::Capture(i) => self.frame().captures[*i as usize].clone(),
            ExprKind::Global(g) => self.read_global(*g)?,
            ExprKind::Outer(n) => {
                let slot = self.outer(*n)?;
                if let Slot::Local(..) = slot {
                    let name = &self.unit.outer[*n as usize];
                    crate::note_read(name);
                }
                self.read_slot(slot)?
            }
            ExprKind::Call { callee, args } => self.call(callee, args)?,
            ExprKind::Method { .. } | ExprKind::Field { .. } | ExprKind::Index { .. } => {
                match self.step(e)? {
                    Some(v) => v,
                    None => V::None,
                }
            }
            ExprKind::Chain(inner) => self.step(inner)?.unwrap_or(V::None),
            ExprKind::Unary { op, expr } => {
                let v = self.expr(expr)?;
                unary(*op, v)?
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let a = self.expr(lhs)?;
                let b = self.expr(rhs)?;
                self.binary(*op, a, b)?
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
            ExprKind::Check(x) => self.expr(x)?,
            ExprKind::If { cond, then, otherwise } => {
                if self.expr(cond)?.truthy() {
                    self.block(then)?
                } else if let Some(o) = otherwise {
                    self.block(o)?
                } else {
                    V::None
                }
            }
            ExprKind::Match { value, arms } => {
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
                V::None
            }
            ExprKind::Block(b) => self.block(b)?,
            ExprKind::Closure(code) => {
                let mut captured = Vec::with_capacity(code.captures.len());
                for r in &code.captures {
                    let slot = self.locate(*r)?;
                    captured.push(self.read_slot(slot)?);
                }
                V::Fn(Rc::new(Closure { code: Rc::clone(code), captured }))
            }
            ExprKind::Interval { args, text, .. } => {
                let mut ms = 0.0;
                if let Some(a) = args.first() {
                    ms = self.expr(a)?.number().unwrap_or(0.0);
                }
                V::Float(crate::start_interval(ms, text.clone()))
            }
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
                        let mut target = self.take(root, &keys)?;
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
            Callee::Host(name) => match self.host.get(name) {
                Some(f) => Ok(V::Float(f())),
                None => fail(format!("Function not found: host::{name} ()")),
            },
            Callee::Dyn(name) => self.call_named(name, argv),
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

#[derive(Clone)]
enum Key {
    Field(String),
    Index(V),
}

/// The value an error is to `catch e`: `{ message, kind }`.
fn error_value(f: &Fault) -> V {
    let mut m = std::collections::BTreeMap::new();
    m.insert("message".to_string(), V::str(crate::explain(&f.message)));
    m.insert("kind".to_string(), V::str(f.kind));
    V::Map(Rc::new(m))
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
    match (here, k) {
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
        (V::Str(s), V::Str(n)) => s.contains(n.as_ref()),
        (V::Range(a, b), n) => n.whole().is_some_and(|x| *a <= x && x < *b),
        (other, _) => return fail(format!("`in` cannot look inside a {}", other.type_name())),
    })
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
            (V::Map(x), V::Map(y)) => {
                let mut out = x.as_ref().clone();
                for (k, v) in y.iter() {
                    out.insert(k.clone(), v.clone());
                }
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
                    return fail("Integer raised to a negative power: write it as a float");
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
