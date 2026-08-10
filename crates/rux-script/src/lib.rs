//! Rux script tier, milestone M8.
//!
//! Wraps a `rhai` engine that holds the app's live state (the script's top-level
//! `let` variables persist in a `Scope`) and evaluates `{{ }}` bindings,
//! `r-if`/`r-for` expressions, and `@tap` handlers against it. Native
//! capabilities are exposed under the `host::` namespace via the builder.
//!
//! This replaces the M5 signal reader and the M6 inline-expression evaluator
//! with a real scripting language: named `fn` handlers, full expressions, and
//! the compiled-Rust boundary (`docs/04-architecture.md`, script/host tiers).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use rhai::{Dynamic, Engine as RhaiEngine, ImmutableString, Module, Scope, AST};
use rux_reactive::{Value, Warning};

thread_local! {
    /// While `Some`, every signal read during evaluation is recorded here. This is
    /// how a binding discovers which signals it depends on (fine-grained
    /// reactivity groundwork): we switch it on around one binding's evaluation,
    /// evaluate, then take the set. `None` means "not tracking", so ordinary
    /// evaluation (and the build-time script run) records nothing.
    static READS: RefCell<Option<HashSet<String>>> = const { RefCell::new(None) };
}

/// Builds an [`Engine`]: register host functions, then `build` with the script.
/// Host functions must be registered before the script runs, since the script
/// may call them during initialization.
pub struct Builder {
    engine: RhaiEngine,
    host: Module,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    pub fn new() -> Self {
        let mut engine = RhaiEngine::new();
        // `signal(x)` is identity: `let level = signal(82)` just binds `level`.
        // Numbers are coerced to float so arithmetic stays consistent.
        engine.register_fn("signal", |x: Dynamic| -> Dynamic {
            match x.as_int() {
                Ok(i) => Dynamic::from(i as f64),
                Err(_) => x,
            }
        });
        // Numbers become text the same way everywhere.
        //
        // Every number in Rux is an f64, so rhai renders a whole one as "32.0"
        // while `{{ }}` renders it as "32": the same value spelled two ways in
        // one window, depending on whether it went through string concatenation
        // on the way. These overloads point rhai at the same rule `Value`
        // displays with, so `"over by " + total` and `{{ total }}` agree.
        engine.register_fn("to_string", |n: f64| Value::Number(n).to_display());
        engine.register_fn("+", |a: ImmutableString, b: f64| {
            format!("{a}{}", Value::Number(b).to_display())
        });
        engine.register_fn("+", |a: f64, b: ImmutableString| {
            format!("{}{b}", Value::Number(a).to_display())
        });
        // `emit("change")` / `emit("change", payload)`: a component telling its
        // caller that something happened. It only records the emission; who
        // listens, and in whose scope their handler runs, is the runtime's
        // business. A script function cannot mutate a signal, so it could not
        // run the caller's body itself even if it knew it.
        engine.register_fn("emit", |name: ImmutableString| {
            EMISSIONS.with(|e| e.borrow_mut().push((name.to_string(), None)));
        });
        engine.register_fn("emit", |name: ImmutableString, payload: Dynamic| {
            EMISSIONS.with(|e| e.borrow_mut().push((name.to_string(), Some(from_dynamic(&payload)))));
        });
        // `navigate("/path")`, `back()`, `forward()`: the router's verbs. Like
        // `emit`, they record an intent rather than acting on it. Navigation
        // moves the `route` signal and pushes history, and neither is something
        // a script function can reach from in here.
        engine.register_fn("navigate", |path: ImmutableString| {
            NAVIGATIONS.with(|n| n.borrow_mut().push(Nav::To(path.to_string())));
        });
        // `replace` is not a convenience over `navigate`: it is the only way to
        // redirect. A redirect done with `navigate` leaves the page that
        // redirected sitting in the history, so Back returns to it and it
        // redirects again, and the user cannot leave. Nothing in userland can
        // work around that.
        engine.register_fn("replace", |path: ImmutableString| {
            NAVIGATIONS.with(|n| n.borrow_mut().push(Nav::Replace(path.to_string())));
        });
        engine.register_fn("back", || {
            NAVIGATIONS.with(|n| n.borrow_mut().push(Nav::Back));
        });
        engine.register_fn("forward", || {
            NAVIGATIONS.with(|n| n.borrow_mut().push(Nav::Forward));
        });
        // Record every variable read while dependency-tracking is active, then
        // fall through (`Ok(None)`) to normal scope resolution. `on_var` is
        // flagged volatile upstream, not deprecated, hence the allow.
        #[allow(deprecated)]
        engine.on_var(|name, _index, _context| {
            READS.with(|r| {
                if let Some(set) = r.borrow_mut().as_mut() {
                    set.insert(name.to_string());
                }
            });
            Ok(None)
        });
        Self {
            engine,
            host: Module::new(),
        }
    }

    /// Register a zero-argument `host::<name>()` returning a number.
    pub fn host_number(
        &mut self,
        name: &str,
        f: impl Fn() -> f64 + Send + Sync + 'static,
    ) -> &mut Self {
        self.host.set_native_fn(name, move || -> Result<f64, Box<rhai::EvalAltResult>> {
            Ok(f())
        });
        self
    }

    /// Compile and initialize the script, producing a ready [`Engine`].
    pub fn build(mut self, script: &str) -> Result<Engine, String> {
        self.engine
            .register_static_module("host", self.host.into());

        let ast = self.engine.compile(script).map_err(|e| e.to_string())?;
        let mut scope = Scope::new();
        self.engine
            .run_ast_with_scope(&mut scope, &ast)
            .map_err(|e| e.to_string())?;
        let funcs = ast.clone_functions_only();

        // The top-level `let` bindings are the app's signals. The set is fixed
        // after init (no runtime `let` at top level), so capture it once here;
        // dependency tracking filters reads down to these names.
        let signals = scope.iter().map(|(name, _, _)| name.to_string()).collect();

        Ok(Engine {
            engine: self.engine,
            scope,
            funcs,
            signals,
        })
    }
}

/// The signal the router keeps the current path in. Reserved: a document that
/// declares it is warned rather than quietly overwritten.
pub const ROUTE_SIGNAL: &str = "route";

/// The signal holding what the matched route captured, as a map, so
/// `{{ params.id }}` works anywhere and not only inside the matched view.
pub const PARAMS_SIGNAL: &str = "params";

/// Whether there is anywhere to go back to, and anywhere to go forward to.
/// Signals rather than functions, because what they are for is disabling a
/// button, and a button's `:class` reads signals.
pub const CAN_BACK_SIGNAL: &str = "can_go_back";
pub const CAN_FORWARD_SIGNAL: &str = "can_go_forward";

/// Every name the router provides. A script declaring one of these is warned.
pub const ROUTER_SIGNALS: [&str; 4] =
    [ROUTE_SIGNAL, PARAMS_SIGNAL, CAN_BACK_SIGNAL, CAN_FORWARD_SIGNAL];

/// A live script engine: state in `scope`, script functions in `funcs`.
pub struct Engine {
    engine: RhaiEngine,
    scope: Scope<'static>,
    funcs: AST,
    /// Names of the top-level signals, the universe of reactive dependencies.
    signals: HashSet<String>,
}

// ── Warning collection ──────────────────────────────────────────────────────

thread_local! {
    /// Expression failures raised since the last drain. Mirrors the sink in
    /// `rux-style`: the runtime drains both after a build so the dev overlay can
    /// list everything wrong with the document, not just what reached stderr.
    static WARNINGS: RefCell<Vec<Warning>> = const { RefCell::new(Vec::new()) };
}

fn warn(message: String) {
    WARNINGS.with(|w| {
        let mut w = w.borrow_mut();
        // A binding is re-evaluated on every build, and an `r-for` evaluates the
        // same expression once per row, so the same failure arrives many times.
        if !w.iter().any(|existing: &Warning| existing.message == message) {
            if ECHO.with(|e| e.get()) {
                eprintln!("rux: {message}");
            }
            // Expression failures are still unplaced: an expression comes from a
            // template attribute or a `{{ }}` span, and the template parser does
            // not yet record where each of those started. See `rux-reactive`'s
            // `Warning` on why a guess would be worse than nothing.
            w.push(Warning::new(message));
        }
    });
}

thread_local! {
    /// Whether to mirror each warning to stderr as it happens.
    ///
    /// On for anyone running the window, where stderr is the only place a
    /// warning could go before the overlay existed. Off for a tool that drains
    /// the sink and formats it itself: printing each warning twice, once as
    /// prose and once as a diagnostic, is what makes machine-readable output
    /// unpipeable.
    static ECHO: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Stop (or resume) mirroring warnings to stderr.
///
/// On by default, which suits anyone running the window, where stderr was the
/// only place a warning could go before the overlay existed. A tool that drains
/// the sink and formats the warnings itself turns it off: printing each one
/// twice, once as prose and once as a diagnostic, is what makes machine-readable
/// output unpipeable.
pub fn set_stderr_echo(on: bool) {
    ECHO.with(|e| e.set(on));
}

/// Take the expression failures raised since the last call, emptying the sink.
pub fn take_warnings() -> Vec<Warning> {
    WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()))
}

/// Raise a script warning from outside the engine.
///
/// The runtime, not the engine, decides who receives an emitted event and how
/// far a chain of them may run, so it is the only layer that can notice an
/// event with nowhere to go. The warning belongs in this sink anyway: to the
/// overlay and to `rux check` it is one more thing wrong with the script, and
/// a second sink would let those two disagree about what was said.
pub fn warn_script(message: impl Into<String>) {
    warn(message.into());
}

thread_local! {
    /// Events raised by `emit` since the last drain, in the order they were
    /// raised. A sink rather than a return value because an emission can happen
    /// anywhere inside a handler, including several levels down a script
    /// function, and the handler's own result is already spoken for.
    static EMISSIONS: RefCell<Vec<(String, Option<Value>)>> = const { RefCell::new(Vec::new()) };
}

/// Take the events emitted since the last call, emptying the sink.
pub fn take_emissions() -> Vec<(String, Option<Value>)> {
    EMISSIONS.with(|e| std::mem::take(&mut *e.borrow_mut()))
}

/// One navigation asked for by a script: where to go, or which way along the
/// history that has already been walked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Nav {
    To(String),
    /// Go there *instead of* here: the current entry is overwritten rather than
    /// added to, so Back skips the page that redirected.
    Replace(String),
    Back,
    Forward,
}

thread_local! {
    /// Navigations asked for since the last drain, in order. A sink for the same
    /// reason as [`EMISSIONS`]: `navigate` can be called from anywhere inside a
    /// handler, and what it means (move the route, push history) belongs to the
    /// runtime rather than to the script tier.
    static NAVIGATIONS: RefCell<Vec<Nav>> = const { RefCell::new(Vec::new()) };
}

/// Take the navigations asked for since the last call, emptying the sink.
pub fn take_navigations() -> Vec<Nav> {
    NAVIGATIONS.with(|n| std::mem::take(&mut *n.borrow_mut()))
}

/// Collapse an expression to one short line for a message, a handler can be a
/// multi-line block, and the overlay has one line to spend on it.
fn trim_expr(src: &str) -> String {
    let flat: String = src.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 60 {
        format!("{}…", flat.chars().take(60).collect::<String>())
    } else {
        flat
    }
}

/// Strip rhai's own `(line N, position M)` suffix from an error message.
///
/// Every `{{ }}` and `@tap` is compiled as its own small script, so rhai's line
/// is **always 1** and its position counts characters inside the expression, not
/// inside the file. Printed beside a file name in the overlay or in `rux check`,
/// it reads as a location in the document and is not one: the reader is sent
/// confidently to line 1. That is the same failure this project removed from CSS
/// warnings, so it does not belong here either.
///
/// Nothing is lost by dropping it. The expression is already quoted in the
/// message, and a position within a string the reader can see is not worth the
/// cost of looking like a file position.
fn strip_rhai_position(message: &str) -> String {
    let trimmed = message.trim_end();
    // Only the exact trailing shape is removed, so a message that merely ends
    // in a parenthesis keeps it.
    let Some(open) = trimmed.rfind(" (line ") else {
        return trimmed.to_string();
    };
    let Some(inner) = trimmed[open + 1..].strip_prefix('(') else {
        return trimmed.to_string();
    };
    let Some(inner) = inner.strip_suffix(')') else {
        return trimmed.to_string();
    };
    let Some(rest) = inner.strip_prefix("line ") else {
        return trimmed.to_string();
    };
    let Some((line, position)) = rest.split_once(", position ") else {
        return trimmed.to_string();
    };
    let numeric = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    if numeric(line) && numeric(position) {
        trimmed[..open].trim_end().to_string()
    } else {
        trimmed.to_string()
    }
}

impl Engine {
    /// Evaluate `src` (an expression or statements) with `locals` temporarily in
    /// scope. Script functions are available. Returns the resulting value.
    fn eval(&mut self, src: &str, locals: &[(String, Value)]) -> Option<Dynamic> {
        let ast = match self.engine.compile(src) {
            Ok(ast) => ast,
            Err(e) => {
                // A `{{ }}` or `@tap` that doesn't compile used to evaluate to
                // nothing, silently, the same failure mode as ignored CSS. Record
                // it so the dev overlay can say what's wrong.
                warn(format!(
                    "expression `{}` failed to compile: {}",
                    trim_expr(src),
                    strip_rhai_position(&e.to_string())
                ));
                return None;
            }
        };
        let merged = self.funcs.merge(&ast);

        let base = self.scope.len();
        for (name, value) in locals {
            self.scope.push(name.clone(), to_dynamic(value));
        }
        let result = self.engine.eval_ast_with_scope::<Dynamic>(&mut self.scope, &merged);
        self.scope.rewind(base); // drop the temporary locals
        match result {
            Ok(value) => Some(value),
            Err(e) => {
                warn(format!(
                    "expression `{}` failed: {}",
                    trim_expr(src),
                    strip_rhai_position(&e.to_string())
                ));
                None
            }
        }
    }

    /// Evaluate an expression to a [`Value`].
    pub fn eval_value(&mut self, src: &str, locals: &[(String, Value)]) -> Option<Value> {
        self.eval(src, locals).map(|d| from_dynamic(&d))
    }

    /// Evaluate a `{{ }}` binding to its display string (empty on error).
    pub fn eval_display(&mut self, src: &str, locals: &[(String, Value)]) -> String {
        self.eval_value(src, locals)
            .map(|v| v.to_display())
            .unwrap_or_default()
    }

    /// Evaluate a condition (`r-if` / `r-elif` / `r-show`).
    pub fn eval_bool(&mut self, src: &str, locals: &[(String, Value)]) -> bool {
        self.eval_value(src, locals)
            .map(|v| v.is_truthy())
            .unwrap_or(false)
    }

    /// Run an `@tap` handler (statements or a function call). Returns whether it
    /// ran without error (assumed to have changed state).
    pub fn run_handler(&mut self, src: &str) -> bool {
        self.eval(src, &[]).is_some()
    }

    /// Evaluate an expression *and* report which signals it read, the binding's
    /// dependency set. Only top-level signal names are returned; loop-locals and
    /// function parameters are filtered out. This is the read half of fine-grained
    /// reactivity: a binding subscribes to exactly the signals it touches.
    pub fn eval_value_tracked(
        &mut self,
        src: &str,
        locals: &[(String, Value)],
    ) -> (Option<Value>, HashSet<String>) {
        READS.with(|r| *r.borrow_mut() = Some(HashSet::new()));
        let value = self.eval_value(src, locals);
        let mut reads = READS.with(|r| r.borrow_mut().take()).unwrap_or_default();
        reads.retain(|n| self.signals.contains(n));
        (value, reads)
    }

    /// Evaluate a `{{ }}` binding to its display string *and* report its signal
    /// deps (the tracked twin of `eval_display`).
    pub fn eval_display_tracked(
        &mut self,
        src: &str,
        locals: &[(String, Value)],
    ) -> (String, HashSet<String>) {
        let (value, deps) = self.eval_value_tracked(src, locals);
        (value.map(|v| v.to_display()).unwrap_or_default(), deps)
    }

    /// Evaluate a condition *and* report its signal deps (the tracked twin of
    /// `eval_bool`).
    pub fn eval_bool_tracked(
        &mut self,
        src: &str,
        locals: &[(String, Value)],
    ) -> (bool, HashSet<String>) {
        let (value, deps) = self.eval_value_tracked(src, locals);
        (value.map(|v| v.is_truthy()).unwrap_or(false), deps)
    }

    /// Run an `@tap` handler and report which signals it *changed*, the write
    /// half. Detected by diffing the signal values across the run, so it needs no
    /// cooperation from the handler source (which is arbitrary rhai). Returns an
    /// empty set if the handler errored or changed nothing.
    pub fn run_handler_tracked(&mut self, src: &str) -> HashSet<String> {
        let names: Vec<String> = self.signals.iter().cloned().collect();
        let before: HashMap<String, Option<Value>> =
            names.iter().map(|n| (n.clone(), self.read_signal(n))).collect();
        if !self.run_handler(src) {
            return HashSet::new();
        }
        names
            .into_iter()
            .filter(|n| self.read_signal(n) != before[n])
            .collect()
    }

    /// Put the current path in scope as the `route` signal.
    ///
    /// A signal rather than anything router-shaped, so `{{ route }}`, `r-if`,
    /// `:class` and the change diff all understand navigation with no knowledge
    /// of the router at all. It is added to the signal set as well as the scope,
    /// or dependency tracking would filter reads of it out as a stray local and
    /// nothing would subscribe.
    ///
    /// Returns whether the value actually moved, which is what tells the runtime
    /// there is anything to repaint.
    pub fn set_route(&mut self, path: &str) -> bool {
        let changed = self.read_signal(ROUTE_SIGNAL).as_ref()
            != Some(&Value::Text(path.to_string()));
        self.scope.set_or_push(ROUTE_SIGNAL, path.to_string());
        self.signals.insert(ROUTE_SIGNAL.to_string());
        changed
    }

    /// Put one of the router's other provided values in scope, the same way
    /// [`Self::set_route`] does with the path.
    ///
    /// Returns whether it moved, so the runtime can skip a repaint nothing
    /// asked for: `can_go_forward` in particular is false through most of a
    /// session and would otherwise report a change on every navigation.
    pub fn set_provided(&mut self, name: &str, value: Value) -> bool {
        let changed = self.read_signal(name).as_ref() != Some(&value);
        self.scope.set_or_push(name, to_dynamic(&value));
        self.signals.insert(name.to_string());
        changed
    }

    /// Whether the script declared one of the router's names itself, so the
    /// runtime can say so rather than silently overwriting it. Asked *before*
    /// the setters, which would otherwise make the answer always yes.
    pub fn declares(&self, name: &str) -> bool {
        self.signals.contains(name)
    }

    /// A signal's current value, read straight from the scope (no evaluation).
    fn read_signal(&self, name: &str) -> Option<Value> {
        self.scope.get_value::<Dynamic>(name).map(|d| from_dynamic(&d))
    }

    /// Read a signal's current value as a display string (for input `r-model`).
    pub fn get_string(&mut self, name: &str) -> String {
        self.get_string_in(name, &[])
    }

    /// The same, with a row's loop variables in scope.
    ///
    /// An `r-model` is recorded as written, so one inside an `r-for` can mention
    /// the loop variable (`items[item.at].note`). Read without it, that is not an
    /// expression at all, and the field comes back empty.
    pub fn get_string_in(&mut self, expr: &str, locals: &[(String, Value)]) -> String {
        self.eval_value(expr, locals).map(|v| v.to_display()).unwrap_or_default()
    }

    /// Set a signal to a string value (from input editing).
    pub fn set_string(&mut self, name: &str, value: &str) {
        self.scope.set_or_push(name, value.to_string());
    }

    /// Run a component's own top-level script in a scope of its own, and hand
    /// back the variables it declared: one instance's private state.
    ///
    /// The document's script is not visible, which is the point. A component
    /// that could read the app's signals by name would be coupled to the app it
    /// was first written for, and could not be used twice.
    pub fn init_scope(&mut self, script: &str) -> Vec<(String, Value)> {
        let ast = match self.engine.compile(script) {
            Ok(ast) => ast,
            Err(e) => {
                warn(format!(
                    "a component's script failed to compile: {}",
                    strip_rhai_position(&e.to_string())
                ));
                return Vec::new();
            }
        };
        // Its own functions plus everything already registered, so a component
        // can call helpers it declared beside its state.
        let merged = self.funcs.merge(&ast);
        let mut scope = Scope::new();
        if let Err(e) = self.engine.run_ast_with_scope(&mut scope, &merged) {
            warn(format!(
                "a component's script failed to run: {}",
                strip_rhai_position(&e.to_string())
            ));
        }
        scope.iter().map(|(name, _, value)| (name.to_string(), from_dynamic(&value))).collect()
    }

    /// Run a handler inside a component instance, whose state is `locals`.
    ///
    /// Returns the instance's variables as they stand afterwards, and which of
    /// the *document's* signals changed. Both matter: a handler in a component
    /// may touch its own state, a prop's underlying signal, or both.
    ///
    /// Reading the locals back before the scope is rewound is what makes a
    /// component's state writable at all. Ordinary evaluation drops them, which
    /// is right for a `{{ }}` binding and wrong for a `@tap`.
    pub fn run_scoped_handler(
        &mut self,
        src: &str,
        locals: &[(String, Value)],
    ) -> (Vec<(String, Value)>, HashSet<String>) {
        let names: Vec<String> = self.signals.iter().cloned().collect();
        let before: HashMap<String, Option<Value>> =
            names.iter().map(|n| (n.clone(), self.read_signal(n))).collect();

        let ast = match self.engine.compile(src) {
            Ok(ast) => ast,
            Err(e) => {
                warn(format!(
                    "handler `{}` failed to compile: {}",
                    trim_expr(src),
                    strip_rhai_position(&e.to_string())
                ));
                return (locals.to_vec(), HashSet::new());
            }
        };
        let merged = self.funcs.merge(&ast);
        let base = self.scope.len();
        for (name, value) in locals {
            self.scope.push(name.clone(), to_dynamic(value));
        }
        let result = self.engine.eval_ast_with_scope::<Dynamic>(&mut self.scope, &merged);
        // Read the instance's state back *before* rewinding, or the handler's
        // effect on it is dropped along with the temporary scope.
        let after: Vec<(String, Value)> = locals
            .iter()
            .map(|(name, previous)| {
                let value = self
                    .scope
                    .get_value::<Dynamic>(name)
                    .map(|d| from_dynamic(&d))
                    .unwrap_or_else(|| previous.clone());
                (name.clone(), value)
            })
            .collect();
        self.scope.rewind(base);

        if let Err(e) = result {
            warn(format!(
                "handler `{}` failed: {}",
                trim_expr(src),
                strip_rhai_position(&e.to_string())
            ));
            return (after, HashSet::new());
        }
        let changed = names.into_iter().filter(|n| self.read_signal(n) != before[n]).collect();
        (after, changed)
    }

    /// Re-evaluate a computed's expression and store the result under its name.
    ///
    /// Returns whether the value actually changed, and what it read. Only a real
    /// change is reported, so a computed that lands on the same answer does not
    /// invalidate the bindings that read it: recomputing is cheap, rebuilding a
    /// subtree is not.
    ///
    /// A computed is a signal like any other, because it is declared as a plain
    /// `let` in the script handed to rhai. That is what makes `{{ total }}`
    /// track it without anything else knowing computeds exist.
    pub fn recompute(&mut self, name: &str, expr: &str) -> (bool, HashSet<String>) {
        let (value, deps) = self.eval_value_tracked(expr, &[]);
        let Some(value) = value else { return (false, deps) };
        let changed = self.read_signal(name).as_ref() != Some(&value);
        if changed {
            self.scope.set_or_push(name, to_dynamic(&value));
        }
        (changed, deps)
    }

    /// Run an effect body, reporting what it read and what it wrote.
    ///
    /// Both halves are needed and neither can be inferred from the other: the
    /// reads say when to run it again, and the writes say what its running has
    /// invalidated. A handler only needs the writes, which is why this is not
    /// [`run_handler_tracked`](Self::run_handler_tracked).
    pub fn run_effect_tracked(&mut self, src: &str) -> (HashSet<String>, HashSet<String>) {
        let names: Vec<String> = self.signals.iter().cloned().collect();
        let before: HashMap<String, Option<Value>> =
            names.iter().map(|n| (n.clone(), self.read_signal(n))).collect();

        READS.with(|r| *r.borrow_mut() = Some(HashSet::new()));
        let ran = self.eval(src, &[]).is_some();
        let mut reads = READS.with(|r| r.borrow_mut().take()).unwrap_or_default();
        reads.retain(|n| self.signals.contains(n));
        if !ran {
            // It still subscribes to whatever it managed to read, so a fixed
            // signal re-runs it rather than leaving it dead until a reload.
            return (reads, HashSet::new());
        }
        let writes = names.into_iter().filter(|n| self.read_signal(n) != before[n]).collect();
        (reads, writes)
    }

    /// Write a string into whatever an `r-model` names, and report which signals
    /// that changed.
    ///
    /// An assignment rather than [`set_string`](Self::set_string), which can only
    /// set a scope variable *called* `name`: for anything but a bare signal
    /// (`user.name`, `items[0].note`) that quietly created a variable with a
    /// punctuation-filled name and left the real target untouched. Running it as
    /// script is also what lets a row's loop variable be in scope.
    pub fn assign_string(
        &mut self,
        target: &str,
        value: &str,
        locals: &[(String, Value)],
    ) -> HashSet<String> {
        let names: Vec<String> = self.signals.iter().cloned().collect();
        let before: HashMap<String, Option<Value>> =
            names.iter().map(|n| (n.clone(), self.read_signal(n))).collect();
        // The value is a person's typing, so it is quoted as a literal rather
        // than pasted in: a quote or a backslash in a text field would otherwise
        // be a syntax error at best.
        let src = format!("{target} = {}", rux_reactive::json_string(value));
        if self.eval(&src, locals).is_none() {
            return HashSet::new();
        }
        names.into_iter().filter(|n| self.read_signal(n) != before[n]).collect()
    }
}

fn to_dynamic(v: &Value) -> Dynamic {
    match v {
        Value::Number(n) => Dynamic::from(*n),
        Value::Text(s) => Dynamic::from(s.clone()),
        Value::Bool(b) => Dynamic::from(*b),
        Value::List(items) => {
            let arr: rhai::Array = items.iter().map(to_dynamic).collect();
            Dynamic::from(arr)
        }
        Value::Map(entries) => {
            let map: rhai::Map =
                entries.iter().map(|(k, v)| (k.as_str().into(), to_dynamic(v))).collect();
            Dynamic::from(map)
        }
    }
}

fn from_dynamic(d: &Dynamic) -> Value {
    if let Ok(i) = d.as_int() {
        return Value::Number(i as f64);
    }
    if let Ok(f) = d.as_float() {
        return Value::Number(f);
    }
    if let Ok(b) = d.as_bool() {
        return Value::Bool(b);
    }
    if let Some(s) = d.clone().try_cast::<String>() {
        return Value::Text(s);
    }
    if let Some(arr) = d.clone().try_cast::<rhai::Array>() {
        return Value::List(arr.iter().map(from_dynamic).collect());
    }
    if let Some(map) = d.clone().try_cast::<rhai::Map>() {
        return Value::Map(
            map.iter().map(|(k, v)| (k.to_string(), from_dynamic(v))).collect(),
        );
    }
    Value::Text(d.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A failing expression is *reported*, not just swallowed, it used to
    /// evaluate to an empty string with nothing said anywhere.
    #[test]
    fn a_failing_expression_is_reported() {
        let mut e = engine();
        let _ = take_warnings(); // start from a clean sink

        assert_eq!(e.eval_display("nope(1)", &[]), "", "still degrades to empty");
        let warnings = take_warnings();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].message.contains("nope(1)"), "names the expression: {warnings:?}");

        assert!(take_warnings().is_empty(), "draining empties the sink");
    }

    /// rhai appends its own `(line 1, position N)` to every error. Each binding
    /// is compiled alone, so that line is always 1 and the position counts
    /// inside the expression, never inside the file. Beside a file name it reads
    /// as a document location, which is the one thing a diagnostic must not do.
    ///
    /// Asserted against a real rhai error rather than a hand-written string, so
    /// it still fails if rhai changes the wording.
    #[test]
    fn a_failing_expression_does_not_quote_a_line_that_is_not_in_the_file() {
        let mut e = engine();
        let _ = take_warnings();

        let _ = e.eval_display("names", &[]); // an undefined variable
        let warnings = take_warnings();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        let message = &warnings[0].message;

        assert!(message.contains("Variable not found"), "keeps the cause: {message}");
        assert!(message.contains("names"), "keeps the expression: {message}");
        assert!(
            !message.contains("line 1"),
            "must not report a line that is not a line of the file: {message}"
        );
        assert!(!message.contains("position"), "nor a position: {message}");
    }

    /// The stripping is narrow: only rhai's exact trailing shape goes, so a
    /// message that merely ends in a parenthesis is left alone.
    #[test]
    fn stripping_the_position_leaves_other_parentheses_alone() {
        assert_eq!(
            strip_rhai_position("Variable not found: names (line 1, position 1)"),
            "Variable not found: names"
        );
        // Not the shape: no position, so nothing is removed.
        assert_eq!(strip_rhai_position("something (line 4)"), "something (line 4)");
        assert_eq!(
            strip_rhai_position("call to fn(a, b) failed"),
            "call to fn(a, b) failed"
        );
        assert_eq!(strip_rhai_position("plain message"), "plain message");
        // Non-numeric where digits belong, so it is not rhai's suffix.
        assert_eq!(
            strip_rhai_position("x (line one, position two)"),
            "x (line one, position two)"
        );
    }

    /// The same failing binding is re-evaluated on every build (and once per row
    /// in an `r-for`), so the sink must not grow a duplicate each time.
    #[test]
    fn repeated_failures_are_reported_once() {
        let mut e = engine();
        let _ = take_warnings();
        for _ in 0..5 {
            let _ = e.eval_display("nope(1)", &[]);
        }
        assert_eq!(take_warnings().len(), 1);
    }

    /// A working expression reports nothing.
    #[test]
    fn a_good_expression_is_silent() {
        let mut e = engine();
        let _ = take_warnings();
        assert_eq!(e.eval_display("double(4)", &[]), "8");
        assert!(take_warnings().is_empty());
    }

    fn engine() -> Engine {
        let mut b = Builder::new();
        b.host_number("full", || 100.0);
        b.build(
            "let level = signal(82); \
             let items = signal([1, 2, 3]); \
             fn double(x) { x * 2 }",
        )
        .expect("build engine")
    }

    #[test]
    fn reads_and_evaluates_state() {
        let mut e = engine();
        assert_eq!(e.eval_display("level", &[]), "82");
        assert_eq!(e.eval_display("level - 2", &[]), "80");
        assert!(e.eval_bool("level > 50", &[]));
        assert!(!e.eval_bool("level < 20", &[]));
    }

    #[test]
    fn runs_inline_handlers_and_pure_fns() {
        let mut e = engine();
        e.run_handler("level = level - 5"); // inline statement mutates scope state
        assert_eq!(e.eval_display("level", &[]), "77");
        e.run_handler("level = level + 3");
        assert_eq!(e.eval_display("level", &[]), "80");
        // A pure script function is usable inside a binding.
        assert_eq!(e.eval_display("double(level)", &[]), "160");
    }

    /// rhai backtick template literals interpolate `${…}`, this is what makes
    /// `:style="`background: ${c}`"` work (no template-layer code in Rux).
    #[test]
    fn evaluates_backtick_string_interpolation() {
        let mut e = engine(); // has `level = 82`
        // Strings interpolate exactly, the common `:style`/`:class` case.
        assert_eq!(
            e.eval_display("`background: ${c}`", &[("c".into(), Value::Text("teal".into()))]),
            "background: teal"
        );
        // A whole-number signal renders as `82`, not rhai's float default
        // `82.0`, because `to_string` is overridden to Rux's own rule. It used
        // to differ, so `${level}px` in a `:style` and `{{ level }}` in text
        // showed the same value two ways in one window.
        assert_eq!(e.eval_display("`level is ${level}`", &[]), "level is 82");
        // A fraction keeps its fraction; only the empty tail goes.
        assert_eq!(
            e.eval_display("`half is ${h}`", &[("h".into(), Value::Number(2.5))]),
            "half is 2.5"
        );
        // The read is tracked, so a `:style` reading a signal reconciles on change.
        let (_, deps) = e.eval_value_tracked("`level: ${level}`", &[]);
        assert!(deps.contains("level"));
    }

    #[test]
    fn calls_host_functions() {
        let mut e = engine();
        e.run_handler("level = host::full()");
        assert_eq!(e.eval_display("level", &[]), "100");
    }

    #[test]
    fn lists_and_locals() {
        let mut e = engine();
        let items = e.eval_value("items", &[]).unwrap();
        assert_eq!(items.as_list().unwrap().len(), 3);
        // A loop-local shadows for one evaluation.
        assert_eq!(e.eval_display("x + 1", &[("x".into(), Value::Number(4.0))]), "5");
    }

    fn deps(e: &mut Engine, src: &str, locals: &[(String, Value)]) -> Vec<String> {
        let (_, set) = e.eval_value_tracked(src, locals);
        let mut v: Vec<String> = set.into_iter().collect();
        v.sort();
        v
    }

    /// A binding reports exactly the signals it read, the subscription set.
    #[test]
    fn tracks_binding_dependencies() {
        let mut e = engine();
        assert_eq!(deps(&mut e, "level", &[]), ["level"]);
        assert_eq!(deps(&mut e, "level > 20", &[]), ["level"]);
        // A pure function reads its argument, not a phantom signal: `double`'s
        // parameter `x` is a local and must be filtered out, leaving just `level`.
        assert_eq!(deps(&mut e, "double(level)", &[]), ["level"]);
        // A loop-local is not a signal, so it contributes no dependency.
        assert_eq!(deps(&mut e, "x + 1", &[("x".into(), Value::Number(4.0))]), Vec::<String>::new());
        assert_eq!(deps(&mut e, "x + level", &[("x".into(), Value::Number(4.0))]), ["level"]);
        // Reading two signals subscribes to both.
        assert_eq!(deps(&mut e, "level + items[0]", &[]), ["items", "level"]);
    }

    /// A handler reports exactly the signals it changed, and nothing it left
    /// alone. This is what lets a write dirty only the affected bindings.
    #[test]
    fn tracks_handler_writes() {
        let mut e = engine();
        let changed = |e: &mut Engine, src: &str| {
            let mut v: Vec<String> = e.run_handler_tracked(src).into_iter().collect();
            v.sort();
            v
        };
        assert_eq!(changed(&mut e, "level = level - 5"), ["level"]);
        assert_eq!(e.eval_display("level", &[]), "77");
        // Writing a signal back to its own value is not a change.
        assert_eq!(changed(&mut e, "level = level"), Vec::<String>::new());
        // Touching one signal does not report the others.
        assert_eq!(changed(&mut e, "items = [9]"), ["items"]);
        assert_eq!(changed(&mut e, "level"), Vec::<String>::new()); // a bare read changes nothing
    }
}

