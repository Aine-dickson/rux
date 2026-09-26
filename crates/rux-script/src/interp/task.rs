//! Running `async fn`s: tasks that stop at an `await` and go on later.
//!
//! Step 6 of `docs/11-next.md`. A call to an `async fn` that does not await
//! it starts a [`Task`]: its body runs at once, from its [`Flat`] ops, until
//! it finishes or reaches an `await` of something not answered yet. Then the
//! task is kept, with its place and its frames, and the caller goes on. When
//! the answer comes (see [`crate::host`]), the runtime asks for the task to
//! go on ([`Interp::resume`]), in the scope of whoever started it, and it runs
//! to its next such `await` or its end.
//!
//! An `await` of another `async fn` does not stop anything by itself: the
//! callee's frame goes on top of the task's and runs, and the task stops only
//! when something down that chain waits for the host. Everything between two
//! stops runs without interruption: there is one thread of script.
//!
//! What a task reads is never a dependency: an `effect` that starts one is
//! subscribed to what the effect read, not to what the task did.

use std::rc::Rc;

use rux_ir::ir::{Callee, ExprKind, FnId, LocalId};
use rux_reactive::Value;

use super::flat::{self, Flat, Op};
use super::{error_value, fail, Fault, Flow, Frame, Interp, R, V, MAX_DEPTH};
use crate::host;

/// A started `async fn` that has not finished.
pub(crate) struct Task {
    /// Innermost last: an `async fn` awaiting another is under it.
    frames: Vec<TaskFrame>,
    /// The answer it is to go on with, once it has come.
    ready: Option<Resume>,
    /// The function that was started, for a report.
    name: String,
}

struct TaskFrame {
    code: Rc<Flat>,
    pc: usize,
    frame: Frame,
    /// Open `try`s, innermost last: where an error goes, and the local it
    /// binds.
    tries: Vec<(usize, Option<LocalId>)>,
    /// Where the value of the `await` this frame is stopped at goes.
    into: Option<LocalId>,
    /// Where that `await` is, which an error it gives back is reported at.
    at: Option<rux_ir::ir::At>,
}

enum Resume {
    Value(V),
    Fault(Fault),
}

enum Outcome {
    Waiting,
    Done(V),
    Failed(Fault),
}

impl TaskFrame {
    fn new(code: Rc<Flat>, args: Vec<V>) -> Self {
        let n = code.locals.len();
        let mut frame = Frame { slots: vec![V::None; n], set: vec![false; n], captures: Vec::new() };
        for (i, a) in args.into_iter().enumerate().take(n) {
            frame.slots[i] = a;
            frame.set[i] = true;
        }
        TaskFrame { code, pc: 0, frame, tries: Vec::new(), into: None, at: None }
    }

    fn set(&mut self, local: LocalId, v: V) {
        self.frame.slots[local.0 as usize] = v;
        self.frame.set[local.0 as usize] = true;
    }
}

impl Interp {
    /// `id`'s ops, compiled the first time.
    fn flat(&mut self, id: FnId) -> R<Rc<Flat>> {
        if let Some(code) = self.flats.get(&id.0) {
            return code.clone().map_err(|m| Flow::Fault(Fault::new(m)));
        }
        let unit = Rc::clone(&self.unit);
        let f = &unit.fns[id.0 as usize];
        let code = flat::build(f, flat::everything()).map(Rc::new).map_err(|m| format!("`{}`: {m}", f.name));
        self.flats.insert(id.0, code.clone());
        code.map_err(|m| Flow::Fault(Fault::new(m)))
    }

    /// Start `id`, an `async fn`, with `args`: run it to its first stop. A
    /// failure before then is the task's, reported with [`Interp::take_failed`],
    /// not the caller's.
    pub(super) fn start_task(&mut self, id: FnId, args: Vec<V>) -> R<()> {
        let code = self.flat(id)?;
        let name = self.unit.fns[id.0 as usize].name.clone();
        let mut task = Task { frames: vec![TaskFrame::new(code, args)], ready: None, name };
        let tid = host::next_id();
        let out = crate::quiet_reads(|| self.drive(tid, &mut task, None));
        self.settle(tid, task, out);
        Ok(())
    }

    fn settle(&mut self, tid: u64, task: Task, out: Outcome) {
        match out {
            Outcome::Waiting => {
                self.tasks.insert(tid, task);
                self.started.push(tid);
            }
            Outcome::Done(_) => {}
            Outcome::Failed(f) => self.failed.push((task.name, f)),
        }
    }

    /// Call `id`, an ordinary function, through its ops rather than the tree
    /// walker: `RUX_FLAT=1` in a debug build. See [`flat::everything`].
    pub(super) fn call_flat(&mut self, id: FnId, args: Vec<V>) -> R<V> {
        let code = self.flat(id)?;
        let name = self.unit.fns[id.0 as usize].name.clone();
        let mut task = Task { frames: vec![TaskFrame::new(code, args)], ready: None, name };
        match self.drive(0, &mut task, None) {
            Outcome::Done(v) => Ok(v),
            Outcome::Failed(f) => Err(Flow::Fault(f)),
            Outcome::Waiting => fail("an ordinary function waited"),
        }
    }

    /// Run `f` with `frame` as the running one.
    fn in_frame<T>(&mut self, frame: &mut Frame, f: impl FnOnce(&mut Self) -> T) -> T {
        self.stack.push(std::mem::take(frame));
        let out = f(self);
        *frame = self.stack.pop().expect("the task's frame");
        out
    }

    /// Run `task` until it stops, finishes or fails, going on with `input`.
    fn drive(&mut self, tid: u64, task: &mut Task, mut input: Option<Resume>) -> Outcome {
        loop {
            let Some(top) = task.frames.last_mut() else { return Outcome::Done(V::None) };
            match input.take() {
                Some(Resume::Value(v)) => {
                    top.at = None;
                    if let Some(l) = top.into.take() {
                        top.set(l, v);
                    }
                }
                Some(Resume::Fault(f)) => {
                    let f = match top.at.take() {
                        Some(at) => located(f, at),
                        None => f,
                    };
                    match unwind(task, f) {
                        Some(f) => return Outcome::Failed(f),
                        None => continue,
                    }
                }
                None => {}
            }
            let top = task.frames.last_mut().expect("a frame");
            let code = Rc::clone(&top.code);
            let Some(op) = code.ops.get(top.pc) else {
                if let Finished::Done(v) = self.finish(task, V::None, &mut input) {
                    return Outcome::Done(v);
                }
                continue;
            };
            top.pc += 1;
            if let Err(e) = self.tick() {
                input = Some(Resume::Fault(fault_of(e)));
                continue;
            }
            let step: R<()> = match op {
                Op::Run { stmt, lp } => match self.in_frame(&mut top.frame, |me| me.stmt(stmt)) {
                    Ok(()) => Ok(()),
                    Err(Flow::Break | Flow::Continue) if lp.is_none() => fail("`break` or `continue` outside a loop"),
                    Err(Flow::Break) => {
                        let lp = lp.expect("a loop");
                        top.tries.truncate(lp.tries);
                        top.pc = lp.brk;
                        Ok(())
                    }
                    Err(Flow::Continue) => {
                        let lp = lp.expect("a loop");
                        top.tries.truncate(lp.tries);
                        top.pc = lp.cont;
                        Ok(())
                    }
                    Err(Flow::Return(v)) => {
                        if let Finished::Done(v) = self.finish(task, v, &mut input) {
                            return Outcome::Done(v);
                        }
                        continue;
                    }
                    Err(e) => Err(e),
                },
                Op::Await { into, call, at } => {
                    let ExprKind::Call { callee, args } = &call.kind else {
                        input = Some(Resume::Fault(located(Fault::new("only a call can be awaited"), *at)));
                        continue;
                    };
                    let argv: R<Vec<V>> =
                        self.in_frame(&mut top.frame, |me| args.iter().map(|a| me.expr(a)).collect());
                    let argv = match argv {
                        Ok(a) => a,
                        Err(e) => {
                            input = Some(Resume::Fault(located(fault_of(e), *at)));
                            continue;
                        }
                    };
                    top.into = *into;
                    top.at = Some(*at);
                    match callee {
                        Callee::Fn(id) if self.unit.fns[id.0 as usize].is_async => {
                            if task.frames.len() >= MAX_DEPTH {
                                let f = Fault::new(format!("Stack overflow: calls nested more than {MAX_DEPTH} deep"));
                                input = Some(Resume::Fault(located(f, *at)));
                                continue;
                            }
                            match self.flat(*id) {
                                Ok(code) => task.frames.push(TaskFrame::new(code, argv)),
                                Err(e) => input = Some(Resume::Fault(located(fault_of(e), *at))),
                            }
                            continue;
                        }
                        // Awaiting an ordinary function is having its value.
                        Callee::Fn(id) => {
                            match self.in_frame(&mut top.frame, |me| me.call_fn(*id, argv)) {
                                Ok(v) => input = Some(Resume::Value(v)),
                                Err(e) => input = Some(Resume::Fault(located(fault_of(e), *at))),
                            }
                            continue;
                        }
                        Callee::Host(name) => {
                            if let Some(f) = self.host.get(name) {
                                input = Some(Resume::Value(V::Float(f())));
                                continue;
                            }
                            let Some(f) = self.async_host.get(name).cloned() else {
                                let f = Fault::new(format!("Function not found: host::{name} ()"));
                                input = Some(Resume::Fault(located(f, *at)));
                                continue;
                            };
                            let ticket = host::next_id();
                            self.waiting.insert(ticket, tid);
                            let values: Vec<Value> = argv.iter().map(V::to_value).collect();
                            f(values, host::Completer::new(ticket, name));
                            return Outcome::Waiting;
                        }
                        _ => fail("only a call to an `async fn` or to a `host::` function can be awaited"),
                    }
                }
                Op::Test { cond, otherwise, .. } => {
                    match self.in_frame(&mut top.frame, |me| me.expr(cond)) {
                        Ok(v) => {
                            if !v.truthy() {
                                top.pc = *otherwise;
                            }
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                }
                Op::Jump(to) => {
                    top.pc = *to;
                    Ok(())
                }
                Op::EachStart { iter, list, idx, .. } => match self.in_frame(&mut top.frame, |me| me.expr(iter)) {
                    Ok(over) => match super::items_of(over) {
                        Ok(items) => {
                            top.set(*list, V::array(items));
                            top.set(*idx, V::Int(0));
                            Ok(())
                        }
                        Err(e) => Err(e),
                    },
                    Err(e) => Err(e),
                },
                Op::EachNext { list, idx, var, counter, exit } => {
                    let i = match top.frame.slots[idx.0 as usize] {
                        V::Int(i) => i,
                        _ => 0,
                    };
                    let item = match &top.frame.slots[list.0 as usize] {
                        V::Array(items) => items.get(i as usize).cloned(),
                        _ => None,
                    };
                    match item {
                        Some(item) => {
                            top.set(*var, item);
                            if let Some(c) = counter {
                                top.set(*c, V::Int(i));
                            }
                            top.set(*idx, V::Int(i + 1));
                        }
                        None => top.pc = *exit,
                    }
                    Ok(())
                }
                Op::RangeStart { from, to, inclusive, cur, end, .. } => {
                    let ends = self.in_frame(&mut top.frame, |me| Ok::<_, Flow>((me.expr(from)?, me.expr(to)?)));
                    match ends {
                        Ok((a, b)) => match (a.whole(), b.whole()) {
                            (Some(a), Some(b)) => {
                                top.set(*cur, V::Int(a));
                                top.set(*end, V::Int(if *inclusive { b.saturating_add(1) } else { b }));
                                Ok(())
                            }
                            _ => fail("a range needs two whole numbers"),
                        },
                        Err(e) => Err(e),
                    }
                }
                Op::RangeNext { cur, end, var, exit } => {
                    let (i, e) = (top.frame.slots[cur.0 as usize].whole(), top.frame.slots[end.0 as usize].whole());
                    match (i, e) {
                        (Some(i), Some(e)) if i < e => {
                            top.set(*var, V::Int(i));
                            top.set(*cur, V::Int(i + 1));
                        }
                        _ => top.pc = *exit,
                    }
                    Ok(())
                }
                Op::TryEnter { catch, var } => {
                    top.tries.push((*catch, *var));
                    Ok(())
                }
                Op::TryExit => {
                    top.tries.pop();
                    Ok(())
                }
                Op::Return { value, .. } => {
                    let v = match value {
                        Some(x) => self.in_frame(&mut top.frame, |me| me.expr(x)),
                        None => Ok(V::None),
                    };
                    match v {
                        Ok(v) => {
                            if let Finished::Done(v) = self.finish(task, v, &mut input) {
                                return Outcome::Done(v);
                            }
                            continue;
                        }
                        Err(e) => Err(e),
                    }
                }
                Op::Arm { value, patterns, guard, next, .. } => {
                    let v = top.frame.slots[value.0 as usize].clone();
                    let hit = self.in_frame(&mut top.frame, |me| me.arm_matches(&v, patterns, guard.as_ref()));
                    match hit {
                        Ok(true) => Ok(()),
                        Ok(false) => {
                            top.pc = *next;
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                }
            };
            // A `return` inside an expression an op worked out (an `if` used
            // as the body's value, a block) returns from the function.
            let step = match step {
                Err(Flow::Return(v)) => {
                    if let Finished::Done(v) = self.finish(task, v, &mut input) {
                        return Outcome::Done(v);
                    }
                    continue;
                }
                other => other,
            };
            if let Err(e) = step {
                let at = match op {
                    Op::Await { at, .. }
                    | Op::Test { at, .. }
                    | Op::EachStart { at, .. }
                    | Op::RangeStart { at, .. }
                    | Op::Return { at, .. }
                    | Op::Arm { at, .. } => Some(*at),
                    Op::Run { stmt, .. } => Some(stmt.at),
                    _ => None,
                };
                let mut f = fault_of(e);
                if f.at.is_none() {
                    f.at = at;
                }
                input = Some(Resume::Fault(f));
            }
        }
    }

    /// The top frame returned `v`: hand it to the frame under it, or finish.
    fn finish(&mut self, task: &mut Task, v: V, input: &mut Option<Resume>) -> Finished {
        task.frames.pop();
        if task.frames.is_empty() {
            return Finished::Done(v);
        }
        *input = Some(Resume::Value(v));
        Finished::Going
    }

    // ----- What the runtime asks ----------------------------------------------

    /// The tasks started since the last call that are still waiting, for the
    /// runtime to give an owner: whoever ran the code that started them.
    pub fn take_started(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.started)
    }

    /// The tasks that failed with nothing to catch the error, by the name of
    /// the function started, since the last call.
    pub fn take_failed(&mut self) -> Vec<(String, Fault)> {
        std::mem::take(&mut self.failed)
    }

    /// How many tasks are waiting.
    pub fn waiting_tasks(&self) -> usize {
        self.tasks.len()
    }

    /// Whether task `id` is waiting.
    pub fn has_task(&self, id: u64) -> bool {
        self.tasks.contains_key(&id)
    }

    /// Take the answers that have come for this interpreter's tasks, and say
    /// which tasks can go on.
    pub fn collect_answers(&mut self) -> Vec<u64> {
        if self.waiting.is_empty() {
            return Vec::new();
        }
        let answers = host::take_answers(|t| self.waiting.contains_key(&t));
        let mut ready = Vec::new();
        for (ticket, answer) in answers {
            let Some(tid) = self.waiting.remove(&ticket) else { continue };
            if let Some(task) = self.tasks.get_mut(&tid) {
                task.ready = Some(match answer {
                    Ok(v) => Resume::Value(V::from_value(&v)),
                    Err(message) => Resume::Fault(Fault::new(message)),
                });
                ready.push(tid);
            }
        }
        ready
    }

    /// Go on with task `id`, whose answer has come, with `locals` handed in as
    /// a handler's are: an instance's state, for a task a component started.
    /// Gives back the locals as they stand afterwards, or `None` when there
    /// is no such task ready.
    pub fn resume(&mut self, id: u64, locals: &[(String, Value)], globals: bool) -> Option<Vec<V>> {
        let mut task = self.tasks.remove(&id)?;
        let Some(input) = task.ready.take() else {
            self.tasks.insert(id, task);
            return None;
        };
        let names: Vec<String> = locals.iter().map(|(n, _)| n.clone()).collect();
        let piece = self.compiled("", &names, globals).expect("nothing parses");
        let given: Vec<V> = locals.iter().map(|(_, v)| V::from_value(v)).collect();
        self.enter(&piece, given);
        self.ops = 0;
        let out = crate::quiet_reads(|| self.drive(id, &mut task, Some(input)));
        let (after, _) = self.leave(&piece);
        self.settle(id, task, out);
        Some(after)
    }

    /// Stop tasks for good: an instance that started them has gone. An answer
    /// that comes for one later is dropped.
    pub fn drop_tasks(&mut self, ids: &[u64]) {
        for id in ids {
            self.tasks.remove(id);
        }
        let tickets: Vec<u64> = self.waiting.iter().filter(|(_, t)| ids.contains(t)).map(|(k, _)| *k).collect();
        for t in &tickets {
            self.waiting.remove(t);
        }
        host::abandon(tickets);
    }

    /// Whether `v` matches an arm of a `switch`, as the tree walker decides.
    pub(super) fn arm_matches(&mut self, v: &V, patterns: &[rux_ir::ir::Expr], guard: Option<&rux_ir::ir::Expr>) -> R<bool> {
        let mut hit = patterns.is_empty();
        for p in patterns {
            let p = self.expr(p)?;
            let matched = match (&p, v.whole()) {
                (V::Range(a, b), Some(x)) => *a <= x && x < *b,
                _ => p == *v,
            };
            if matched {
                hit = true;
                break;
            }
        }
        if hit {
            if let Some(g) = guard {
                return Ok(self.expr(g)?.truthy());
            }
        }
        Ok(hit)
    }
}

enum Finished {
    Done(V),
    Going,
}

/// Send `f` to the innermost open `try` of the task, or out of it: `None`
/// when a `catch` took it.
fn unwind(task: &mut Task, f: Fault) -> Option<Fault> {
    while let Some(top) = task.frames.last_mut() {
        if let Some((catch, var)) = top.tries.pop() {
            if let Some(var) = var {
                let e = f.thrown.clone().unwrap_or_else(|| error_value(&f));
                top.set(var, e);
            }
            top.pc = catch;
            top.into = None;
            return None;
        }
        task.frames.pop();
    }
    Some(f)
}

fn fault_of(flow: Flow) -> Fault {
    match flow {
        Flow::Fault(f) => f,
        Flow::Break | Flow::Continue => Fault::new("`break` or `continue` outside a loop"),
        Flow::Return(_) => Fault::new("a `return` where none can be"),
    }
}

fn located(mut f: Fault, at: rux_ir::ir::At) -> Fault {
    if f.at.is_none() {
        f.at = Some(at);
    }
    f
}
