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

pub mod check;
mod front;
pub mod profile;
pub mod types;
pub mod validate;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use rhai::{Dynamic, Engine as RhaiEngine, EvalAltResult, ImmutableString, Module, Position, Scope, AST};
use rux_reactive::{Value, Warning};

thread_local! {
    /// While `Some`, every signal read during evaluation is recorded here. This is
    /// how a binding discovers which signals it depends on (fine-grained
    /// reactivity groundwork): we switch it on around one binding's evaluation,
    /// evaluate, then take the set. `None` means "not tracking", so ordinary
    /// evaluation (and the build-time script run) records nothing.
    static READS: RefCell<Option<HashSet<String>>> = const { RefCell::new(None) };
    /// Open read spans, innermost last. Unlike [`READS`], which one binding
    /// switches on around itself, a span collects every name read by
    /// everything evaluated while it is open, tracked or not, and spans nest:
    /// a read lands in every open one. See [`begin_reads`].
    static SPANS: RefCell<Vec<HashSet<String>>> = const { RefCell::new(Vec::new()) };
}

/// Start collecting every name any evaluation reads, until the matching
/// [`end_reads`]. What a build uses to learn everything one part of the tree
/// read, so that part can be reused while none of it has changed.
pub fn begin_reads() {
    SPANS.with(|s| s.borrow_mut().push(HashSet::new()));
}

/// Close the innermost span [`begin_reads`] opened and hand back what was read
/// inside it. Names, not signals: filter with [`Engine::declares`].
pub fn end_reads() -> HashSet<String> {
    SPANS.with(|s| s.borrow_mut().pop()).unwrap_or_default()
}

/// Count `names` as read by every open span, for a caller that reused work
/// instead of evaluating it again: the reads happened once, and whatever
/// encloses the reuse depends on them all the same.
pub fn note_reads<'a>(names: impl Iterator<Item = &'a str>) {
    SPANS.with(|s| {
        let mut s = s.borrow_mut();
        if s.is_empty() {
            return;
        }
        for name in names {
            for set in s.iter_mut() {
                if !set.contains(name) {
                    set.insert(name.to_string());
                }
            }
        }
    })
}

/// What a query knows about one matched element.
///
/// Deliberately plain data. The selector engine that produced it lives in
/// `rux-style`, which depends on this crate and so cannot be depended on from
/// here; the runtime bridges the two by installing a resolver (see
/// [`with_elements`]) rather than by anything in this crate knowing what a
/// selector is.
#[derive(Clone, Debug, PartialEq)]
pub struct ElementFacts {
    /// The node's child-index path from the root, the identity the binding
    /// registry and the layout's regions both already use.
    pub path: Vec<usize>,
    pub tag: String,
    pub id: Option<String>,
    pub classes: Vec<String>,
    /// Where the last laid-out frame put this node, if it was in one.
    ///
    /// `None` has two honest causes and script cannot tell them apart, nor
    /// should it: nothing has been laid out yet (`rux check` has no window and
    /// no GPU, which is the point of it running in CI), or the node is hidden
    /// by `r-show="false"` and so is not on screen to have a box.
    pub bounds: Option<ElementBox>,
}

/// A laid-out box in absolute window pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElementBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Resolves a selector against the tree the last build produced.
pub type ElementResolver = Arc<dyn Fn(&str) -> Option<Vec<ElementFacts>> + Send + Sync>;

thread_local! {
    /// The tree, as `query()` can see it, installed only for the duration of one
    /// handler.
    ///
    /// **Its absence is the handler-only rule**, not a separate check. A `{{ }}`
    /// binding, a `:style` or an `r-if` is evaluated with nothing installed, so
    /// `query()` there raises instead of quietly answering. That matters because
    /// the alternative has no fixed point: a binding that reads the tree would
    /// have to invalidate when layout changes, and invalidating it rebuilds and
    /// relayouts. Making the capability simply not exist outside a handler costs
    /// nothing, since every real use of it is handler-shaped.
    static ELEMENTS: RefCell<Option<ElementResolver>> = const { RefCell::new(None) };
}

/// Run `f` with `resolver` installed, so `query()` inside it can see the tree.
///
/// Restores whatever was installed before rather than clearing, so a handler
/// that runs another handler (a component listener fired by `emit`) does not
/// leave the outer one unable to query.
pub fn with_elements<R>(resolver: ElementResolver, f: impl FnOnce() -> R) -> R {
    let previous = ELEMENTS.with(|e| e.borrow_mut().replace(resolver));
    let out = f();
    ELEMENTS.with(|e| *e.borrow_mut() = previous);
    out
}

/// One element matched by [`query`], as script sees it.
#[derive(Clone, Debug)]
pub struct ElementHandle {
    facts: ElementFacts,
}

impl ElementHandle {
    /// The node's path, for whoever has to act on it.
    pub fn path(&self) -> &[usize] {
        &self.facts.path
    }
}

/// Register `query()` and the handle it returns.
fn register_elements(engine: &mut RhaiEngine) {
    engine
        .register_type_with_name::<ElementHandle>("Element")
        .register_get("tag", |e: &mut ElementHandle| e.facts.tag.clone())
        // Absent rather than empty, so `el.id ?? "none"` reads the way it does
        // everywhere else in the language.
        .register_get("id", |e: &mut ElementHandle| match &e.facts.id {
            Some(id) => Dynamic::from(id.clone()),
            None => Dynamic::UNIT,
        })
        .register_get("classes", |e: &mut ElementHandle| {
            e.facts.classes.iter().cloned().map(Dynamic::from).collect::<rhai::Array>()
        });

    // Geometry, from the frame that is currently on screen.
    //
    // **One frame stale, and that is the guarantee, not a defect.** A handler
    // runs before the next layout, so it reads the numbers the last one
    // produced, exactly as `getBoundingClientRect` does in a browser. Anything
    // else would mean laying out mid-handler, from within a tap that is about
    // to change the very state the layout depends on.
    //
    // Absent rather than zero when there is no frame to read, so "not laid out"
    // stays distinguishable from "laid out, and genuinely zero wide".
    fn dimension(
        e: &ElementHandle,
        pick: impl Fn(&ElementBox) -> f32,
    ) -> Dynamic {
        match &e.facts.bounds {
            Some(b) => Dynamic::from(pick(b) as f64),
            None => Dynamic::UNIT,
        }
    }
    engine
        .register_get("x", |e: &mut ElementHandle| dimension(e, |b| b.x))
        .register_get("y", |e: &mut ElementHandle| dimension(e, |b| b.y))
        .register_get("width", |e: &mut ElementHandle| dimension(e, |b| b.width))
        .register_get("height", |e: &mut ElementHandle| dimension(e, |b| b.height));

    // The actions. Each records an intent and returns nothing; the runtime
    // applies them after the handler has finished, so a handler that focuses
    // something and then changes state does not race its own tree.
    engine
        .register_fn("focus", |e: &mut ElementHandle| {
            let path = e.facts.path.clone();
            ELEMENT_ACTIONS.with(|a| a.borrow_mut().push(ElementAction::Focus(path)));
        })
        .register_fn("scrollIntoView", |e: &mut ElementHandle| {
            let path = e.facts.path.clone();
            ELEMENT_ACTIONS.with(|a| a.borrow_mut().push(ElementAction::ScrollIntoView(path)));
        })
        .register_fn("tap", |e: &mut ElementHandle| {
            let path = e.facts.path.clone();
            ELEMENT_ACTIONS.with(|a| a.borrow_mut().push(ElementAction::Tap(path)));
        });
    // `blur()` is free-standing rather than a method, because there is only one
    // focused element and blurring "this one" would either do nothing or take
    // focus from something else.
    engine.register_fn("blur", || {
        ELEMENT_ACTIONS.with(|a| a.borrow_mut().push(ElementAction::Blur));
    });
    engine.register_fn(SUBMIT_FN, |form: ImmutableString| {
        let path = form.split('.').filter_map(|i| i.parse().ok()).collect();
        ELEMENT_ACTIONS.with(|a| a.borrow_mut().push(ElementAction::Submit(path)));
    });

    engine.register_fn(
        "query",
        |selector: ImmutableString| -> Result<rhai::Array, Box<EvalAltResult>> {
            let resolver = ELEMENTS.with(|e| e.borrow().clone());
            let Some(resolver) = resolver else {
                return Err("query() is only available inside a handler, because a \
                            binding that reads the tree would rebuild the tree it read"
                    .into());
            };
            // A selector that cannot be parsed is an error, not an empty list.
            // Matching nothing in silence is the failure this language keeps
            // closing off, and a typo in a selector is exactly that failure.
            let Some(found) = resolver(&selector) else {
                return Err(format!("`{selector}` is not a selector this can match").into());
            };
            Ok(found.into_iter().map(|facts| Dynamic::from(ElementHandle { facts })).collect())
        },
    );
}

/// Builds an [`Engine`]: register host functions, then `build` with the script.
/// Host functions must be registered before the script runs, since the script
/// may call them during initialization.
pub struct Builder {
    engine: RhaiEngine,
    host: Module,
    /// The type of every `host::` function registered, for the checker.
    host_types: Vec<(String, types::Type)>,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

/// A script that would not compile or would not run, with the position kept.
///
/// `line` and `column` are **relative to the script that was compiled**, which
/// is not the file: a `.rux` document's `<script>` starts part-way down, and
/// the runtime strips `use` and reactive declarations before compiling. Whoever
/// knows that offset is responsible for adding it, which is
/// `Document::load_checked`. Reporting a section-relative number as though it
/// were a file position is the bug this type exists to make hard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptError {
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl ScriptError {
    fn at(message: String, position: Position) -> Self {
        Self { message, line: position.line(), column: position.position() }
    }

    /// A failure with nothing to point at.
    pub fn plain(message: String) -> Self {
        Self { message, line: None, column: None }
    }
}

impl std::fmt::Display for ScriptError {
    /// The sentence rhai produced, unchanged, so anything that used to do
    /// `.to_string()` on the old `String` error reads exactly as it did.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ScriptError {}

impl From<ScriptError> for String {
    fn from(e: ScriptError) -> String {
        e.message
    }
}

/// The most steps one evaluation may take before it is stopped. See the
/// limits in [`Builder::new`].
pub const MAX_OPERATIONS: u64 = 5_000_000;

impl Builder {
    pub fn new() -> Self {
        let mut engine = RhaiEngine::new();

        // Strict bindings: `user.nmae` raises instead of evaluating to `()`.
        //
        // This was recorded for two milestones as a reason the rhai fork had to
        // exist, on the belief that silent map lookup was rhai's semantics and
        // could not be closed from outside the engine. It can: the option landed
        // upstream and nobody had looked. Turning it on here is the whole of what
        // `docs/06-roadmap.md` calls v0.7 item 1, and it costs no divergence.
        //
        // The failure it kills is the one the dev overlay exists for. A typo in a
        // `{{ }}` binding used to render empty, which looks exactly like a value
        // that is legitimately absent, so nothing was reported and the author
        // went looking at their data instead of their spelling.
        engine.set_fail_on_invalid_map_property(true);

        // Limits, so a script that never stops is stopped. Without them a
        // `while true {}` in a handler held the UI thread for good, which on
        // Android is an "app not responding" and on the web a frozen tab, and
        // nothing was ever reported. Harmless while every document is its
        // author's own; a denial of service once one is fetched or shared.
        //
        // Each evaluation counts separately (a binding, a handler, a body), so
        // the ceilings are per piece of work and generous: far past anything
        // an app does in one go, and still a fraction of a second in a release
        // build before a runaway loop is ended.
        engine.set_max_operations(MAX_OPERATIONS);
        engine.set_max_string_size(64 * 1024 * 1024);
        engine.set_max_array_size(10_000_000);
        engine.set_max_map_size(10_000_000);

        // Do not let the optimizer delete calls made for their side effects.
        //
        // rhai's default optimization pass removes a call whose result is
        // unused when it believes the call is pure, and it cannot know that a
        // function registered from the host is not. Nearly every Rux builtin is
        // called for effect and discarded: `print(x)`, `emit("change")`,
        // `navigate("/")`. Found by `print(1); print(1)` producing one line
        // instead of two, which is a mild symptom of a rule that could just as
        // easily have eaten a navigation.
        //
        // The expressions being compiled here are single bindings and handler
        // bodies, so there is nothing for an optimizer to win.
        engine.set_optimization_level(rhai::OptimizationLevel::None);

        // `===` and `!==` mean what `==` and `!=` mean.
        //
        // Muscle memory only, for anyone arriving from JS. Deliberately *not*
        // JS's loose `==`: both spellings are the strict comparison, so there is
        // no coercion rule to learn and no pair of operators to choose between.
        //
        // Reserved but unimplemented upstream, and `register_custom_operator`
        // accepts a reserved token, so this is a registration rather than a fork
        // change. Precedence 90 is what rhai gives `==` and `!=`.
        //
        // Both compare through `Value` rather than rhai's per-type equality, so
        // `===` answers the same question `{{ }}` and `r-if` would: Rux's value
        // model is the one the language user can see.
        let _ = engine.register_custom_operator("===", 90);
        let _ = engine.register_custom_operator("!==", 90);
        engine.register_fn("===", |a: Dynamic, b: Dynamic| from_dynamic(&a) == from_dynamic(&b));
        engine.register_fn("!==", |a: Dynamic, b: Dynamic| from_dynamic(&a) != from_dynamic(&b));

        // `signal(x)` is identity: `let level = signal(82)` just binds `level`.
        // Numbers are coerced to float so arithmetic stays consistent.
        engine.register_fn("signal", |x: Dynamic| -> Dynamic {
            match x.as_int() {
                Ok(i) => Dynamic::from(i as f64),
                Err(_) => x,
            }
        });
        // `10 / 3` is 3.333…, not 3.
        //
        // The last visible place where Rux's two numeric types disagreed. A
        // signal is coerced to f64 by `signal()`, so almost every number a
        // document handles is already a float and divides like one; two bare
        // literals were the exception, and integer division is not a rule anyone
        // arriving from JavaScript expects to meet.
        //
        // A registration rather than a fork change, and deliberately so: the
        // engine's integer division lives in a macro covering every integer
        // width, while this needs to change for exactly one pair of types.
        // Registering the same signature shadows the built-in.
        //
        // Division by zero yields infinity rather than raising, as in JS. That
        // follows from f64 division and is left alone rather than special-cased.
        //
        // Fast-operators mode has to be off for this to be reachable at all: it
        // is on by default and dispatches the built-in arithmetic for known type
        // pairs *without consulting the function registry*, so a registered `/`
        // for two integers is simply never called. The symptom is a registration
        // that compiles, runs, and does nothing.
        engine.set_fast_operators(false);
        engine.register_fn("/", |a: i64, b: i64| a as f64 / b as f64);

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
        // `path_for("crew-detail", #{ id: "grace" })` builds a path from a
        // named route. A function returning a string rather than a second form
        // of `navigate`, because a path is what `to=`, `:to=`, `navigate` and
        // `replace` all already take: one new function reaches all four, and
        // there is no second way to say the same thing.
        engine.register_fn("path_for", |name: ImmutableString, values: rhai::Map| {
            let values: Vec<(String, Value)> =
                values.into_iter().map(|(k, v)| (k.to_string(), from_dynamic(&v))).collect();
            build_named_path(&name, &values)
        });
        // A route with no parameters still has a name worth using.
        engine.register_fn("path_for", |name: ImmutableString| {
            build_named_path(&name, &[])
        });
        engine.register_fn("back", || {
            NAVIGATIONS.with(|n| n.borrow_mut().push(Nav::Back));
        });
        engine.register_fn("forward", || {
            NAVIGATIONS.with(|n| n.borrow_mut().push(Nav::Forward));
        });
        register_js_names(&mut engine);
        // `x is T`, which the fork parses into this call. See `validate`.
        engine.register_fn(rhai::IS_FUNCTION, |value: Dynamic, written: ImmutableString| {
            validate::is(&value, &written)
        });
        register_elements(&mut engine);

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
            SPANS.with(|s| {
                for set in s.borrow_mut().iter_mut() {
                    if !set.contains(name) {
                        set.insert(name.to_string());
                    }
                }
            });
            Ok(None)
        });

        // Told before any variable is written or passed where it may change
        // (fork item 11). While a handler runs under `Engine::tracking`, the
        // first time a signal is about to change, its value is kept, so what
        // the handler changed is found by comparing only those.
        engine.on_var_write(note_write);

        // `let add = (a, b) => a + b; add(2, 3)`: a variable holding an arrow,
        // called the way JavaScript calls one. rhai only knew `add.call(2, 3)`,
        // and `add(2, 3)` failed with "there is no function `add`", which is
        // the first thing anyone writes after declaring one.
        //
        // Only when no function of that name exists, so a real `fn add` still
        // wins, and only for a plain call: `x.add()` is a method. A closure's
        // captured values are its curried arguments and go first, as rhai's
        // own `call` passes them.
        #[allow(deprecated)]
        engine.on_missing_function(|name, args, is_method_call, mut context| {
            if is_method_call {
                return Ok(None);
            }
            let Some(fn_ptr) = context.scope().get_value::<rhai::FnPtr>(name) else {
                return Ok(None);
            };
            let mut values: Vec<Dynamic> = fn_ptr.curry().to_vec();
            values.extend(args.iter_mut().map(|a| std::mem::take(&mut **a)));
            let mut refs: Vec<&mut Dynamic> = values.iter_mut().collect();
            context.call_fn_raw(fn_ptr.fn_name(), false, false, &mut refs).map(Some)
        });
        Self {
            engine,
            host: Module::new(),
            host_types: Vec::new(),
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
        self.host_types
            .push((name.to_string(), types::Type::Function(Vec::new(), Box::new(types::Type::Number))));
        self
    }

    /// Compile and initialize the script, producing a ready [`Engine`].
    ///
    /// The error keeps the position rhai reported. It used to be flattened with
    /// `e.to_string()` here, which left the line and column readable only as
    /// prose inside the sentence, and every consumer downstream had `None` for
    /// both: `rux check --format json` emitted `"line": null` and the editor
    /// put the squiggle on line 1. See [`ScriptError`].
    pub fn build(mut self, script: &str) -> Result<Engine, ScriptError> {
        self.engine
            .register_static_module("host", self.host.into());

        let ast = front::compile(&self.engine, script)
            .map_err(|e| ScriptError::at(explain(&e.message), e.position))?;
        // What `x is T` resolves a declared name against: this script's own
        // `type`s, before the script's first statement can ask. The runtime adds what it imports (`validate::know_types`).
        validate::reset_types(
            ast.annotations()
                .iter()
                .filter(|a| a.kind == rhai::AnnotationKind::Type)
                .map(|a| (a.name.to_string(), a.ty.clone())),
        );
        let mut scope = Scope::new();
        self.engine
            .run_ast_with_scope(&mut scope, &ast)
            .map_err(|e| {
                let at = e.position();
                ScriptError::at(explain(&e.to_string()), at)
            })?;
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
            checked: ast,
            host_types: self.host_types,
            compiled: HashMap::new(),
        })
    }
}

/// Give rhai's collection and string library the names JS uses for the same
/// operations.
///
/// None of this changes what the language *does*. It is the cheapest thing on
/// the v0.7 list and the one that most decides whether Rux reads as "JS-ish" or
/// as a foreign language wearing JS's syntax: someone who reaches for
/// `items.length` and gets an error has learned that their instincts do not
/// apply here, and they learn it in the first ten minutes.
///
/// rhai's own spellings keep working. These are additional names for the same
/// behaviour, not replacements, so nothing written against the old surface
/// breaks and the docs can simply teach the JS one.
fn register_js_names(engine: &mut RhaiEngine) {
    use rhai::{Array, FnPtr, NativeCallContext};

    // `null`, the empty value under the name someone arriving from JS uses.
    //
    // Not a variable and not a constant: `null` is a *reserved keyword* in rhai,
    // so it never reaches variable resolution and cannot be bound in a scope.
    // Custom syntax is the one hook that sees it, which also gives the property
    // that matters, that `null` cannot be shadowed by a `let` and never lands in
    // the signal set. It is a literal, not a piece of state anything could
    // subscribe to.
    let _ = engine.register_custom_syntax(["null"], false, |_ctx, _inputs| Ok(Dynamic::UNIT));

    // `.length`, not `.len()`. A property getter, as in JS.
    //
    // Arrays and strings only. JS has no `length` on a plain object, and adding
    // one to maps would be inventing a rule rather than matching a known one,
    // which is the thing this whole exercise is trying not to do. `keys(m)` and
    // `values(m)` are already there for that.
    // Returns an integer, not an f64, which is the opposite of the rule
    // everywhere else in Rux and is deliberate until numbers are unified.
    //
    // `items[items.length - 1]` and `for i in 0..items.length` are the two most
    // common things anyone does with a length, and both need an integer: rhai
    // indexes and builds ranges with `INT`, and hands back "Data type incorrect:
    // f64 (expecting i64)" for a float. A `length` that reads correctly in a
    // binding and fails the moment it is used to index would be worse than not
    // having it, so it matches `len()` exactly for now.
    //
    // The all-f64 change on `docs/06-roadmap.md` is what makes this an f64 like
    // everything else, and it has to teach indexing and ranges to coerce at the
    // same time. Found by an example: `keyed-list.rux` was rewritten to use
    // `.length` and stopped rotating.
    engine.register_get("length", |a: &mut Array| a.len() as i64);
    engine.register_get("length", |s: &mut ImmutableString| s.chars().count() as i64);

    // Text to a number, and back, under JavaScript's names and with its
    // answers. A route parameter arrives as text (`/task/:id` gives `"2"`), and
    // a list whose ids are numbers could not be matched against it: rhai's
    // `parse_int` and `parse_float` exist, but nobody arriving from JS looks
    // for them, and they raise on bad input where JS says `NaN`.
    //
    // `Number` reads the whole text or nothing (`Number("12px")` is `NaN`,
    // `Number("")` is 0); `parseInt` and `parseFloat` read what leads and stop
    // (`parseFloat("12.5px")` is 12.5). Every answer is an f64, like every
    // other number in Rux.
    engine.register_fn("Number", |s: ImmutableString| js_number(&s));
    engine.register_fn("Number", |b: bool| if b { 1.0 } else { 0.0 });
    engine.register_fn("Number", |n: f64| n);
    engine.register_fn("Number", |n: i64| n as f64);
    engine.register_fn("parseInt", |s: ImmutableString| js_parse_int(&s, 10));
    engine.register_fn("parseInt", |s: ImmutableString, radix: Dynamic| {
        js_parse_int(&s, num(&radix) as u32)
    });
    engine.register_fn("parseInt", |n: f64| n.trunc());
    engine.register_fn("parseInt", |n: i64| n as f64);
    engine.register_fn("parseFloat", |s: ImmutableString| js_parse_float(&s));
    engine.register_fn("parseFloat", |n: f64| n);
    engine.register_fn("parseFloat", |n: i64| n as f64);
    engine.register_fn("String", |v: Dynamic| from_dynamic(&v).to_display());
    engine.register_fn("isNaN", |n: f64| n.is_nan());
    engine.register_fn("isNaN", |_: i64| false);

    // Membership and position. rhai spells these `contains` and `index_of`.
    //
    // Comparison goes through `Value`, so `includes` answers the same question
    // `===` does and the two cannot disagree about what equality means.
    engine.register_fn("includes", |a: Array, item: Dynamic| {
        let needle = from_dynamic(&item);
        a.iter().any(|v| from_dynamic(v) == needle)
    });
    engine.register_fn("indexOf", |a: Array, item: Dynamic| {
        let needle = from_dynamic(&item);
        a.iter().position(|v| from_dynamic(v) == needle).map_or(-1.0, |i| i as f64)
    });
    engine.register_fn("includes", |s: ImmutableString, part: ImmutableString| {
        s.contains(part.as_str())
    });
    engine.register_fn("indexOf", |s: ImmutableString, part: ImmutableString| {
        s.find(part.as_str()).map_or(-1.0, |i| s[..i].chars().count() as f64)
    });

    // `join`, which rhai does not have at all.
    engine.register_fn("join", |a: Array, sep: ImmutableString| {
        a.iter().map(|v| from_dynamic(v).to_display()).collect::<Vec<_>>().join(&sep)
    });

    // `slice`, with JS's forgiving bounds: out-of-range clamps and an inverted
    // range yields nothing, rather than raising. That leniency is the whole
    // reason people reach for `slice` instead of indexing, so a strict version
    // wearing the name would be worse than not having it.
    fn bounds(len: usize, start: f64, end: Option<f64>) -> (usize, usize) {
        let resolve = |v: f64| -> usize {
            if v < 0.0 {
                (len as f64 + v).max(0.0) as usize
            } else {
                (v as usize).min(len)
            }
        };
        let from = resolve(start);
        let to = end.map_or(len, resolve);
        (from, to.max(from))
    }
    // Every numeric argument arrives as `Dynamic` and is coerced, rather than
    // being declared `f64`.
    //
    // A literal `1` in a script is still an rhai integer, while anything that
    // came through `signal()` is a float, so `items.slice(1)` and
    // `items.slice(start)` would otherwise resolve to different overloads and
    // one of them would not exist. This is the numbers-are-two-types problem
    // showing up in the first five minutes of use, and the reason
    // `docs/06-roadmap.md` commits the fork to making every number an f64.
    // Coercing at each boundary is the version of that available without a fork.
    engine.register_fn("slice", |a: Array, start: Dynamic| {
        let (from, to) = bounds(a.len(), num(&start), None);
        a[from..to].to_vec()
    });
    engine.register_fn("slice", |a: Array, start: Dynamic, end: Dynamic| {
        let (from, to) = bounds(a.len(), num(&start), Some(num(&end)));
        a[from..to].to_vec()
    });
    engine.register_fn("slice", |s: ImmutableString, start: Dynamic| {
        let chars: Vec<char> = s.chars().collect();
        let (from, to) = bounds(chars.len(), num(&start), None);
        chars[from..to].iter().collect::<String>()
    });
    engine.register_fn("slice", |s: ImmutableString, start: Dynamic, end: Dynamic| {
        let chars: Vec<char> = s.chars().collect();
        let (from, to) = bounds(chars.len(), num(&start), Some(num(&end)));
        chars[from..to].iter().collect::<String>()
    });

    // The string methods whose only difference from rhai's is the name.
    engine.register_fn("toUpperCase", |s: ImmutableString| s.to_uppercase());
    engine.register_fn("toLowerCase", |s: ImmutableString| s.to_lowercase());
    engine.register_fn("startsWith", |s: ImmutableString, p: ImmutableString| {
        s.starts_with(p.as_str())
    });
    engine.register_fn("endsWith", |s: ImmutableString, p: ImmutableString| {
        s.ends_with(p.as_str())
    });
    engine.register_fn("repeat", |s: ImmutableString, n: Dynamic| {
        s.repeat(num(&n).max(0.0) as usize)
    });
    // `trim` returns the trimmed string instead of emptying the one it was given.
    //
    // rhai's `trim` takes its receiver by `&mut` and trims **in place**, returning
    // `()`. Every other string method here returns a value, and so does JS's, so
    // `{{ name.trim() }}` rendered *empty* rather than trimmed: the call returned
    // nothing and the nothing was displayed. That is the silent-wrong failure this
    // language keeps closing off, and it is worse than most, because the value it
    // quietly replaces is the one the author was looking at.
    //
    // Registering the same name with a by-value receiver shadows the built-in, the
    // same move `/` on two integers and `print` already make. Found while writing
    // `docs/07-script.md`, by checking the method list rather than trusting it.
    engine.register_fn("trim", |s: ImmutableString| s.trim().to_string());

    engine.register_fn("charAt", |s: ImmutableString, i: Dynamic| {
        s.chars().nth(num(&i).max(0.0) as usize).map(String::from).unwrap_or_default()
    });

    // `forEach`, which is the one array method with no rhai equivalent under any
    // name: `map` and `filter` build a new array, and a loop is a statement, so
    // there was no way to run a side effect per item as an expression.
    engine.register_fn(
        "forEach",
        |ctx: NativeCallContext, a: Array, f: FnPtr| -> Result<(), Box<rhai::EvalAltResult>> {
            for (i, item) in a.into_iter().enumerate() {
                // Called with `(item, index)` like JS, falling back to `(item)`.
                //
                // rhai resolves a closure call by arity and does *not* tolerate
                // being handed more arguments than the closure declares, so the
                // two-argument call fails outright against `|x| …`, which is the
                // form nearly everyone writes. Trying the JS shape first and
                // falling back keeps both working; the alternative is supporting
                // only one of them, and either choice would surprise someone.
                if f.call_within_context::<Dynamic>(&ctx, (item.clone(), i as f64)).is_err() {
                    let _ = f.call_within_context::<Dynamic>(&ctx, (item,))?;
                }
            }
            Ok(())
        },
    );

    // The other two callback methods JavaScript has and rhai names differently
    // or not at all. `some` and `find` are rhai's own and already match.
    // Truthiness is JavaScript's, as everywhere in Rux.
    engine.register_fn(
        "every",
        |ctx: NativeCallContext, a: Array, f: FnPtr| -> Result<bool, Box<rhai::EvalAltResult>> {
            for item in a {
                if !f.call_within_context::<Dynamic>(&ctx, (item,))?.is_truthy() {
                    return Ok(false);
                }
            }
            Ok(true)
        },
    );
    engine.register_fn(
        "findIndex",
        |ctx: NativeCallContext, a: Array, f: FnPtr| -> Result<f64, Box<rhai::EvalAltResult>> {
            for (i, item) in a.into_iter().enumerate() {
                if f.call_within_context::<Dynamic>(&ctx, (item,))?.is_truthy() {
                    return Ok(i as f64);
                }
            }
            Ok(-1.0)
        },
    );

    // `sort` with a comparison, answering as JavaScript's does: the sorted
    // array. rhai's sorts in place and returns nothing, so
    // `[3, 1, 2].sort((a, b) => a - b)[0]` failed on indexing `()`, and it
    // wanted its comparison to return an integer where `a - b` of two Rux
    // numbers is a float. The receiver is sorted too, as JavaScript's is.
    engine.register_fn(
        "sort",
        |ctx: NativeCallContext, a: &mut Array, f: FnPtr| -> Result<Array, Box<rhai::EvalAltResult>> {
            let mut failed = None;
            a.sort_by(|x, y| {
                if failed.is_some() {
                    return std::cmp::Ordering::Equal;
                }
                match f.call_within_context::<Dynamic>(&ctx, (x.clone(), y.clone())) {
                    Ok(d) => num(&d).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal),
                    Err(e) => {
                        failed = Some(e);
                        std::cmp::Ordering::Equal
                    }
                }
            });
            match failed {
                Some(e) => Err(e),
                None => Ok(a.clone()),
            }
        },
    );

    // `setInterval(ms) { … }`, which reaches here already rewritten by
    // [`front::lower`] into `__interval(ms, "body")`. The rewrite is what
    // lets the body be a block in the source and text by the time it is stored;
    // see [`TimerRequest`] for why it cannot be a callable.
    //
    // The id comes back immediately, so `let t = setInterval(…) { … }` binds a
    // handle in the same statement that starts the timer. The runtime has not
    // seen the request yet at that point, which is fine: nothing can fire until
    // the handler this is running inside has finished.
    // The period arrives as whatever the author wrote, and `1000` is an integer
    // in rhai: Rux kept both numeric types rather than going all-f64, so a
    // registration typed to `f64` alone would not be found at all.
    engine.register_fn("__interval", |ms: Dynamic, body: ImmutableString| -> f64 {
        let ms = num(&ms);
        let id = NEXT_TIMER_ID.with(|n| {
            let id = n.get();
            n.set(id + 1.0);
            id
        });
        TIMER_REQUESTS.with(|t| {
            t.borrow_mut().push(TimerRequest::Start { id, ms, body: body.to_string() })
        });
        id
    });

    engine.register_fn("clearInterval", |id: Dynamic| {
        TIMER_REQUESTS.with(|t| t.borrow_mut().push(TimerRequest::Cancel(num(&id))));
    });

    // Printf-debugging, which the script tier had no way to do at all.
    //
    // Spelled `print(…)` and `debug(…)`, rhai's own names, wired to a Rux sink
    // through `on_print`/`on_debug`. Deliberately **not** `log(…)`, even though
    // that is what a JS developer would reach for first: rhai's arithmetic
    // package already defines `log` as the logarithm, and a more specific `f64`
    // overload beats a `Dynamic` one, so `log(2)` would quietly compute 0.301
    // instead of printing. It resolves, returns a number and reports nothing,
    // which is the worst available outcome and exactly the class of silent
    // failure the rest of this milestone exists to remove.
    //
    // `console.log` is not offered either: there is no `console` object, and
    // inventing one to hold a single function would misrepresent what else is
    // there.
    engine.on_print(|s| log_line(s.to_string()));
    // Numbers and collections print the way `{{ }}` renders them.
    //
    // `on_print` receives text rhai has already formatted, so a whole number
    // arrives as "1.0" while the same value in a binding reads "1". That is the
    // same disagreement the `to_string` and `+` overloads above exist to settle,
    // and printf-debugging is the worst place to have it: the whole purpose of
    // the call is to show you what a value is, so it must not show you a
    // spelling the rest of the language never uses. These overloads intercept
    // before formatting; anything else still goes through `on_print` unchanged.
    //
    // A `print` overload returns the text to be printed rather than printing it:
    // rhai calls the function and hands the result to `on_print`, so returning
    // `()` here fails the call with "expecting string" and, because a handler
    // body is one script, takes every statement after it down with it.
    engine.register_fn("print", |n: f64| Value::Number(n).to_display());
    engine.register_fn("print", |v: Dynamic| from_dynamic(&v).to_display());
    // `debug` additionally carries the source position rhai knows about.
    engine.on_debug(|s, src, pos| {
        let where_ = match (src, pos.is_none()) {
            (Some(src), _) => format!(" ({src})"),
            (None, false) => format!(" (line {})", pos.line().unwrap_or(0)),
            (None, true) => String::new(),
        };
        log_line(format!("{s}{where_}"))
    });
}

/// Read a script number whichever of rhai's two numeric types it arrived as.
///
/// A literal is an integer and a signal is a float, so any argument a user might
/// write either way has to accept both. The fork's all-f64 change is what
/// removes the need for this.
/// JavaScript's `Number(text)`: the whole text, trimmed, as a number, 0 for
/// nothing, `NaN` for anything that is not one. `Infinity` is JS's word for
/// it; Rust's `inf` and `nan` are not, so they are `NaN` here as there.
fn js_number(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    let (sign, body) = match t.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, t.strip_prefix('+').unwrap_or(t)),
    };
    if body == "Infinity" {
        return sign * f64::INFINITY;
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(digits) = t.strip_prefix(prefix) {
            return i64::from_str_radix(digits, radix).map_or(f64::NAN, |n| n as f64);
        }
    }
    let numeric = body.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'));
    if !numeric || !body.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
        return f64::NAN;
    }
    body.parse::<f64>().map_or(f64::NAN, |n| sign * n)
}

/// JavaScript's `parseInt(text, radix)`: the digits that lead, after space
/// and a sign, in `radix` (a `0x` prefix means 16 when none is given).
fn js_parse_int(s: &str, radix: u32) -> f64 {
    let t = s.trim_start();
    let (sign, mut t) = match t.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, t.strip_prefix('+').unwrap_or(t)),
    };
    let mut radix = if radix == 0 { 10 } else { radix };
    if (radix == 16 || radix == 10) && (t.starts_with("0x") || t.starts_with("0X")) {
        radix = 16;
        t = &t[2..];
    }
    if !(2..=36).contains(&radix) {
        return f64::NAN;
    }
    let digits: String = t.chars().take_while(|c| c.is_digit(radix)).collect();
    if digits.is_empty() {
        return f64::NAN;
    }
    let mut n = 0.0f64;
    for c in digits.chars() {
        n = n * radix as f64 + c.to_digit(radix).unwrap_or(0) as f64;
    }
    sign * n
}

/// JavaScript's `parseFloat(text)`: the longest number that leads, after
/// space. `parseFloat("12.5px")` is 12.5, `parseFloat("px")` is `NaN`.
fn js_parse_float(s: &str) -> f64 {
    let t = s.trim_start();
    let (sign, body) = match t.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, t.strip_prefix('+').unwrap_or(t)),
    };
    if body.starts_with("Infinity") {
        return sign * f64::INFINITY;
    }
    // Grow the candidate one character at a time and keep the longest that
    // parses, so `1e5x` is 1e5 and `1.2.3` is 1.2, as a browser reads them.
    if !body.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
        return f64::NAN;
    }
    let mut best = None;
    for (i, c) in body.char_indices() {
        if !(c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')) {
            break;
        }
        if let Ok(n) = body[..i + c.len_utf8()].parse::<f64>() {
            best = Some(n);
        }
    }
    best.map_or(f64::NAN, |n| sign * n)
}

fn num(d: &Dynamic) -> f64 {
    if let Ok(i) = d.as_int() {
        return i as f64;
    }
    d.as_float().unwrap_or(0.0)
}

fn log_line(text: String) {
    LOGS.with(|l| l.borrow_mut().push(text.clone()));
    if ECHO.with(|e| e.get()) {
        eprintln!("rux print: {text}");
    }
}

thread_local! {
    /// What `print(…)` and `debug(…)` have said since the last drain.
    ///
    /// A sink of its own rather than a line in [`WARNINGS`], because the two are
    /// not the same kind of thing: a warning is something wrong with the
    /// document and a log is the author talking to themselves. Merging them
    /// would make the overlay's list of problems fill up with output that is
    /// working exactly as intended, and `rux check` would start failing on it.
    ///
    /// Not deduplicated, unlike warnings. A binding re-evaluated on every build
    /// repeats its warning and there is nothing to learn from the repetition,
    /// but a `print` inside a loop writing the same line ten times *is* the
    /// information.
    static LOGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Take what `print(…)` and `debug(…)` have said since the last call, emptying
/// the sink.
pub fn take_logs() -> Vec<String> {
    LOGS.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

/// The signal the router keeps the current path in. Reserved: a document that
/// declares it is warned rather than quietly overwritten.
pub const ROUTE_SIGNAL: &str = "route";

/// The signal holding what the matched route captured, as a map, so
/// `{{ params.id }}` works anywhere and not only inside the matched view.
pub const PARAMS_SIGNAL: &str = "params";

/// The signal holding the query string, parsed, as a map.
///
/// Separate from `route` rather than part of it, so `route == "/search"` keeps
/// meaning what it says once a query is present. Matching ignores it too: a
/// query is an argument to a page, not a different page.
pub const QUERY_SIGNAL: &str = "query";

/// Whether there is anywhere to go back to, and anywhere to go forward to.
/// Signals rather than functions, because what they are for is disabling a
/// button, and a button's `:class` reads signals.
pub const CAN_BACK_SIGNAL: &str = "can_go_back";
pub const CAN_FORWARD_SIGNAL: &str = "can_go_forward";

/// Every name the router provides. A script declaring one of these is warned.
pub const ROUTER_SIGNALS: [&str; 5] =
    [ROUTE_SIGNAL, PARAMS_SIGNAL, QUERY_SIGNAL, CAN_BACK_SIGNAL, CAN_FORWARD_SIGNAL];

/// A live script engine: state in `scope`, script functions in `funcs`.
/// Something a handler calls that cannot do what it was written to do.
///
/// Two shapes, because they are two different mistakes and collapsing them into
/// one message would repeat the defect this whole check exists to fix: a
/// message that says "`set` does not exist" about a function that plainly does
/// teaches an author to distrust the next one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallProblem {
    /// No function of this name is registered, defined by a `fn`, or built in.
    /// Nothing a row or a component instance brings into scope can add one, so
    /// this can never resolve under any state.
    NoSuchFunction(String),
    /// A signal used through the API `docs/02-spec.md` described before v0.3
    /// replaced it with ordinary assignment.
    ///
    /// Reported separately because `set` and `get` **do** exist, on arrays,
    /// maps, blobs and strings, so the name check cannot see this and a message
    /// about an unknown function would be false. What makes it safe to report
    /// is the arity: every registered `set` takes two arguments after its
    /// receiver and the map's `get` takes one, so a one-argument `set` or a
    /// no-argument `get` on a signal is the superseded API and nothing else.
    StaleSignalApi { signal: String, method: String },
    /// A `fn` this document declared, called with a number of arguments no
    /// version of it takes.
    ///
    /// Only for the document's own functions. A registered native can be
    /// overloaded on types this has no way to see, and a method call's receiver
    /// is an argument, so checking arity against the whole registry would flag
    /// working code. A `fn` in `<script>` has a parameter list that is right
    /// there in the file, and rhai dispatches it on count alone.
    WrongArgumentCount { name: String, given: usize, wanted: Vec<usize> },
}

impl CallProblem {
    /// The whole sentence, ready to follow "the `@tap` handler ".
    ///
    /// Rendered here rather than by the caller so the runtime, `rux check` and
    /// anything else that grows a use of this cannot word it three ways.
    pub fn describe(&self) -> String {
        match self {
            Self::NoSuchFunction(name) => format!(
                "calls `{name}`, which does not exist, so it will do nothing at that \
                 point when it runs"
            ),
            Self::WrongArgumentCount { name, given, wanted } => {
                let plural = |n: usize| if n == 1 { "argument" } else { "arguments" };
                let takes = match wanted.as_slice() {
                    [one] => format!("takes {one} {}", plural(*one)),
                    many => format!(
                        "takes {}",
                        many.iter()
                            .map(|n| format!("{n} {}", plural(*n)))
                            .collect::<Vec<_>>()
                            .join(" or ")
                    ),
                };
                format!(
                    "calls `{name}` with {given} {}, and `{name}` {takes}, so the call \
                     fails the moment it runs",
                    plural(*given)
                )
            }
            Self::StaleSignalApi { signal, method } => {
                let fix = match method.as_str() {
                    "set" => format!("assign to it instead: `{signal} = …`"),
                    _ => format!("read it by naming it instead: `{signal}`"),
                };
                format!(
                    "calls `{signal}.{method}()`, which was how signals worked before \
                     v0.3 and has not existed since. It does nothing now, in silence. \
                     {fix}"
                )
            }
        }
    }
}

/// What a tracked run has been about to write. See [`Engine::tracking`].
struct WriteLog {
    /// The scope the run evaluates in, by address: a write found in any other
    /// scope went through a closure's capture.
    scope: usize,
    signals: HashSet<String>,
    /// Each signal's value just before the run first reached it in a form
    /// that could change it.
    before: HashMap<String, Option<Value>>,
    /// Signals written from somewhere with no old value to compare against:
    /// a closure writing what it captured. Reported as changed.
    unsnapped: HashSet<String>,
}

thread_local! {
    static WRITES: RefCell<Option<WriteLog>> = const { RefCell::new(None) };
}

/// The fork's `on_var_write` hook: keep a signal's value the first time a
/// tracked run is about to change it.
fn note_write(name: &str, scope: &Scope) {
    WRITES.with(|w| {
        let mut w = w.borrow_mut();
        let Some(log) = w.as_mut() else { return };
        if !log.signals.contains(name) || log.before.contains_key(name) || log.unsnapped.contains(name) {
            return;
        }
        if scope as *const Scope as *const () as usize != log.scope {
            log.unsnapped.insert(name.to_string());
            return;
        }
        // The signals are the scope's first entries, so a later entry of the
        // same name is a local shadowing one: that write goes to the local
        // and leaves the signal alone.
        let mut named = scope.iter_raw().filter(|(n, ..)| *n == name);
        let signal = named.next();
        if named.next().is_some() {
            return;
        }
        log.before.insert(name.to_string(), signal.map(|(_, _, d)| from_dynamic(d)));
    });
}

pub struct Engine {
    engine: RhaiEngine,
    scope: Scope<'static>,
    funcs: AST,
    /// Names of the top-level signals, the universe of reactive dependencies.
    signals: HashSet<String>,
    /// The whole script as compiled, statements and annotations included, for
    /// the type checker. `funcs` keeps only the functions, which is all
    /// running needs.
    checked: AST,
    /// The `host::` functions' types, as registered.
    host_types: Vec<(String, types::Type)>,
    /// Every source this engine has run, compiled and merged with `funcs`,
    /// keyed by the source text. A binding is evaluated on every build and a
    /// handler on every tap, and compiling each of them again was most of the
    /// script tier's cost in a long list. `funcs` never changes after the
    /// engine is built (a reload builds a new engine), so an entry never goes
    /// stale. Bounded by [`COMPILED_CAP`]: a source built from changing text
    /// would otherwise grow it for as long as the app runs.
    compiled: HashMap<String, Arc<AST>>,
}

/// How many compiled sources an engine keeps before starting over. A real
/// document has a few hundred distinct bindings and handlers.
const COMPILED_CAP: usize = 4096;

// ── Warning collection ──────────────────────────────────────────────────────

thread_local! {
    /// Expression failures raised since the last drain. Mirrors the sink in
    /// `rux-style`: the runtime drains both after a build so the dev overlay can
    /// list everything wrong with the document, not just what reached stderr.
    static WARNINGS: RefCell<Vec<Warning>> = const { RefCell::new(Vec::new()) };
}

thread_local! {
    /// The file line the expression being evaluated was written on, if the
    /// caller knew it.
    ///
    /// The same arrangement `rux-style` uses for stylesheet warnings, and
    /// deliberately the same shape, so that knowing how one works is knowing how
    /// both do. It is a thread-local rather than a parameter because the path
    /// from "build this element" to "this expression failed" runs through
    /// expression evaluation, dependency tracking and `r-for` expansion, none of
    /// which has any other reason to know what a file is.
    static AT_LINE: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Run `f` with any warning it raises attributed to `line`.
///
/// Restores whatever was set before rather than clearing, so nesting unwinds
/// correctly: an `r-for` row's `{{ }}` is evaluated inside the element that
/// carries the loop, and the inner position must not outlive the inner call.
pub fn located<T>(line: Option<usize>, f: impl FnOnce() -> T) -> T {
    let previous = AT_LINE.with(|l| l.replace(line));
    let out = f();
    AT_LINE.with(|l| l.set(previous));
    out
}

thread_local! {
    /// The file the thing being built came from, when it is not the document.
    ///
    /// Set while an imported component's subtree is built, so a warning raised
    /// in there names the component's file. Coarser than [`AT_LINE`] on purpose:
    /// a line changes per attribute, a file changes only at a component
    /// boundary, so they are two scopes rather than one pair.
    static IN_FILE: RefCell<Option<std::path::PathBuf>> = const { RefCell::new(None) };
}

/// Run `f` with any warning it raises attributed to `file`.
///
/// Restores what was set before rather than clearing, so a component that uses
/// another component unwinds to the outer one rather than to the document.
pub fn in_file<T>(file: Option<std::path::PathBuf>, f: impl FnOnce() -> T) -> T {
    let previous = IN_FILE.with(|c| c.replace(file));
    let out = f();
    IN_FILE.with(|c| c.replace(previous));
    out
}

thread_local! {
    /// Whether this document is a **fragment**: a component, whose surroundings
    /// are supplied by whoever uses it, rather than a page that stands alone.
    ///
    /// Two things reach a component from outside, and neither is declared
    /// anywhere in its own file:
    ///
    /// - **Props.** A component reads `{{ label }}` with nothing saying `label`
    ///   is expected, so a checker reading that file alone cannot tell a prop
    ///   from a typo.
    /// - **Functions.** Only `fn` definitions are shared into the one engine,
    ///   so a component's `@tap="bump_it(2)"` legitimately calls a `fn` its
    ///   parent declared and it has never heard of.
    ///
    /// So a fragment read on its own is an incomplete program, and the things
    /// it appears to be missing are exactly the things a caller provides. A page
    /// (`<screen>` root) has no caller, so what is missing there is missing.
    /// Severity follows that, and only severity: both are still reported.
    ///
    /// This is inference standing in for a declaration. `rux new` scaffolds a
    /// component that reads two props, so escalating everywhere made the tool
    /// ship a project failing its own `rux check`, which is how the rule was
    /// found. A way to declare a prop would make the question answerable
    /// instead; it is scheduled, not built.
    static IS_FRAGMENT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Say whether this document is a fragment. See [`IS_FRAGMENT`].
///
/// Set once per load, from the document's root element. `rux-runtime` owns the
/// page/fragment distinction because it is the layer that has the template.
pub fn set_is_fragment(yes: bool) {
    IS_FRAGMENT.with(|c| c.set(yes));
}

/// Whether what this document is missing might be supplied by a caller.
///
/// Read by the runtime to decide whether an unresolvable call is an error or a
/// warning, the same question [`level_for`] answers for an undefined name.
pub fn is_fragment() -> bool {
    IS_FRAGMENT.with(std::cell::Cell::get)
}

thread_local! {
    /// The props the file being loaded declares with `prop` and gives no
    /// default. Read on its own, a component has no caller to pass them, so
    /// reading one fails, and that failure is the caller's to owe rather than
    /// this file's mistake. Only consulted in a fragment.
    static DECLARED_PROPS: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// What a file with no `<template>` declares besides types: the first thing in
/// its script that is not a `type`, as a short description, or `None` when
/// there is nothing else. Such a file exists for other files to `use` its
/// types, so a function or a statement in it would never run.
///
/// A script that does not compile answers `None`: the load reports that error
/// in its own words, and a second one here would only repeat it.
pub fn declares_besides_types(script: &str) -> Option<&'static str> {
    let ast = front::compile(&RhaiEngine::new(), script).ok()?;
    if ast.iter_functions().next().is_some() {
        Some("a function")
    } else if !ast.statements().is_empty() {
        Some("a statement")
    } else {
        None
    }
}

/// Record the props this document declares. See [`DECLARED_PROPS`].
pub fn set_declared_props(names: impl IntoIterator<Item = String>) {
    DECLARED_PROPS.with(|p| *p.borrow_mut() = names.into_iter().collect());
}

/// Whether a failure is only a declared prop that nobody passed, which a
/// fragment read on its own always has.
fn is_owed_prop(rhai_message: &str) -> bool {
    let Some(rest) = rhai_message.strip_prefix("Variable not found:") else { return false };
    let name: String =
        rest.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    is_fragment() && DECLARED_PROPS.with(|p| p.borrow().contains(&name))
}

/// Whether a failed expression is definitely wrong or only probably.
///
/// Only the undefined-name case is softened, and only for a fragment. Anything
/// else that fails (a call to nothing, a syntax error, a bad index) is wrong
/// wherever it is written, because no caller can supply a missing function.
fn level_for(rhai_message: &str) -> rux_reactive::Level {
    let undefined_name = rhai_message.starts_with("Variable not found:");
    if undefined_name && is_fragment() {
        rux_reactive::Level::Warning
    } else {
        rux_reactive::Level::Error
    }
}

fn warn(message: String) {
    raise(message, rux_reactive::Level::Warning);
}

/// Raise something that is definitely wrong rather than merely dead.
///
/// An expression that cannot compile, or that fails with every local in scope,
/// is not "ignored" the way an unhonored CSS property is: it is a binding that
/// can never show anything. `rux check` exits non-zero for these, and it used
/// to call such a document clean.
fn error(message: String) {
    raise(message, rux_reactive::Level::Error);
}

thread_local! {
    /// How many times anything was raised, whether or not the sink kept it.
    /// Part of [`effects`].
    static RAISED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A number that moves whenever evaluation does anything besides produce a
/// value: raises a warning, prints, emits, navigates, starts or stops a timer,
/// or acts on an element. Equal before and after an evaluation means it had
/// no effect beyond its result, so reusing that result instead of evaluating
/// again loses nothing.
pub fn effects() -> u64 {
    let len = |n: usize| n as u64;
    RAISED.with(std::cell::Cell::get)
        + LOGS.with(|l| len(l.borrow().len()))
        + EMISSIONS.with(|e| len(e.borrow().len()))
        + NAVIGATIONS.with(|n| len(n.borrow().len()))
        + TIMER_REQUESTS.with(|t| len(t.borrow().len()))
        + ELEMENT_ACTIONS.with(|a| len(a.borrow().len()))
}

/// Run `f` and then drop whatever it added to the queues [`effects`] counts,
/// for evaluating something a second time purely to check the first.
pub fn without_effects<T>(f: impl FnOnce() -> T) -> T {
    let lens = (
        WARNINGS.with(|w| w.borrow().len()),
        LOGS.with(|l| l.borrow().len()),
        EMISSIONS.with(|e| e.borrow().len()),
        NAVIGATIONS.with(|n| n.borrow().len()),
        TIMER_REQUESTS.with(|t| t.borrow().len()),
        ELEMENT_ACTIONS.with(|a| a.borrow().len()),
    );
    let out = f();
    WARNINGS.with(|w| w.borrow_mut().truncate(lens.0));
    LOGS.with(|l| l.borrow_mut().truncate(lens.1));
    EMISSIONS.with(|e| e.borrow_mut().truncate(lens.2));
    NAVIGATIONS.with(|n| n.borrow_mut().truncate(lens.3));
    TIMER_REQUESTS.with(|t| t.borrow_mut().truncate(lens.4));
    ELEMENT_ACTIONS.with(|a| a.borrow_mut().truncate(lens.5));
    out
}

fn raise(message: String, level: rux_reactive::Level) {
    RAISED.with(|r| r.set(r.get() + 1));
    let at = AT_LINE.with(|l| l.get());
    let file = IN_FILE.with(|c| c.borrow().clone());
    WARNINGS.with(|w| {
        let mut w = w.borrow_mut();
        // A binding is re-evaluated on every build, and an `r-for` evaluates the
        // same expression once per row, so the same failure arrives many times.
        // Deduped by message *and* line, as the cascade's sink is: the same
        // mistake on two lines is two places to go and fix, and an editor wants
        // a squiggle on each.
        if !w.iter().any(|existing: &Warning| {
            existing.message == message && existing.line == at && existing.file == file
        }) {
            if ECHO.with(|e| e.get()) {
                eprintln!("rux: {message}");
            }
            let warning = Warning::maybe_at(message, at).in_file(file);
            w.push(if level == rux_reactive::Level::Error { warning.as_error() } else { warning });
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

/// Raise a script *error* from outside the engine: something definitely wrong
/// rather than merely dead. See [`rux_reactive::Level`].
pub fn error_script(message: impl Into<String>) {
    error(message.into());
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

/// What a script asked of the timers.
///
/// The body travels as **text**, like every other piece of Rux that runs later:
/// an `@tap`, a lifecycle hook, a component listener. A callable would read
/// better and would not work, because a closure called after the fact writes to
/// its own captured copies and cannot move a signal, which for an interval is
/// the entire point of having one.
#[derive(Clone, Debug, PartialEq)]
pub enum TimerRequest {
    Start { id: f64, ms: f64, body: String },
    /// `clearInterval(id)`. An id that names nothing is ignored: clearing a
    /// timer twice, or clearing one whose instance has already taken it away,
    /// is ordinary rather than a mistake.
    Cancel(f64),
}

thread_local! {
    static TIMER_REQUESTS: RefCell<Vec<TimerRequest>> = const { RefCell::new(Vec::new()) };
    /// Handed out in order. Never reused, so a stale handle held by a script
    /// cannot come to name somebody else's timer later.
    static NEXT_TIMER_ID: std::cell::Cell<f64> = const { std::cell::Cell::new(1.0) };
}

/// Take the timer starts and cancels asked for since the last call.
pub fn take_timer_requests() -> Vec<TimerRequest> {
    TIMER_REQUESTS.with(|t| std::mem::take(&mut *t.borrow_mut()))
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

/// Something a handler asked to happen to an element.
///
/// None of these edits the tree, which is why they can exist at all: they move
/// host state (what has focus, where a scroller is scrolled to) and the next
/// build reproduces the tree exactly as before.
#[derive(Clone, Debug, PartialEq)]
pub enum ElementAction {
    /// Put the caret in this element, if it is an input.
    Focus(Vec<usize>),
    /// Drop focus entirely. Takes no element: there is only one focused thing,
    /// and asking a *particular* element to blur when it is not the focused one
    /// would either do nothing or steal focus from elsewhere, both surprising.
    Blur,
    /// Scroll whatever scroller contains this element until it is visible.
    ScrollIntoView(Vec<usize>),
    /// Press this element as a finger would.
    ///
    /// Deliberately the *whole* gesture rather than "run its `@tap` body": a
    /// real press also follows a link, toggles a checkbox, opens a select,
    /// moves keyboard focus and puts the caret in a text input, and a script
    /// that has to remember which of those to do separately is a script that
    /// will get it wrong. The shell resolves it against the element's box and
    /// runs the same dispatch a pointer does.
    Tap(Vec<usize>),
    /// Submit the `role="form"` at this tree path: check its fields, then run
    /// its `@submit` or its `@invalid`. Asked for by a `<button type="submit">`,
    /// whose tap the build ends with a call to [`SUBMIT_FN`].
    Submit(Vec<usize>),
}

/// The function a submit button's tap calls, with its form's tree path
/// written as `"0.2.1"`. Not for authors: a form is submitted by a
/// `type="submit"` button or by Enter in its last field.
pub const SUBMIT_FN: &str = "__rux_submit";

thread_local! {
    /// Element actions asked for since the last drain, in order.
    ///
    /// A sink for exactly the reason [`EMISSIONS`] and [`NAVIGATIONS`] are: what
    /// focusing something *means* (which input, which row, where the caret
    /// lands) belongs to the runtime and the shell, and a script function
    /// cannot reach either. This is the fourth use of that idiom rather than a
    /// new mechanism.
    static ELEMENT_ACTIONS: RefCell<Vec<ElementAction>> = const { RefCell::new(Vec::new()) };
}

/// Take the element actions asked for since the last call, emptying the sink.
pub fn take_element_actions() -> Vec<ElementAction> {
    ELEMENT_ACTIONS.with(|a| std::mem::take(&mut *a.borrow_mut()))
}

thread_local! {
    /// Named routes, as `(name, pattern)` in the order written, so `path_for`
    /// can build a path from a name and some parameters.
    ///
    /// A sink like the others, and for the same reason: `path_for` is a plain
    /// rhai function registered once on the engine, and it has no way to reach
    /// the document's template from inside a call.
    static ROUTES: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

/// Tell the script tier what the document's named routes are. Replaces the
/// previous set, since a reload may have changed them.
pub fn set_routes(routes: Vec<(String, String)>) {
    ROUTES.with(|r| *r.borrow_mut() = routes);
}

/// Build a path from a named route and a map of values.
///
/// Values matching a `:name` segment fill it. Anything left over becomes a
/// query string, which is what makes this usable for a route that takes no path
/// parameters at all (`path_for("search", #{ q: "rust" })`).
///
/// Every failure is warned about and then produces a path that visibly does not
/// work, rather than one that quietly goes somewhere plausible: landing on the
/// fallback page is a bug you can see, and landing on the wrong record is not.
fn build_named_path(name: &str, values: &[(String, Value)]) -> String {
    let pattern = ROUTES.with(|r| {
        r.borrow().iter().find(|(n, _)| n == name).map(|(_, p)| p.clone())
    });
    let Some(pattern) = pattern else {
        warn(format!(
            "`path_for(\"{name}\", …)` names a route that does not exist; add `name=\"{name}\"` \
             to the <route> it means"
        ));
        return name.to_string();
    };

    let mut used: Vec<&str> = Vec::new();
    let mut path = String::new();
    for segment in pattern.split('/').filter(|s| !s.is_empty()) {
        path.push('/');
        match segment.strip_prefix(':') {
            Some(param) => match values.iter().find(|(k, _)| k == param) {
                Some((key, value)) => {
                    used.push(key.as_str());
                    path.push_str(&encode(&value.to_display()));
                }
                None => {
                    warn(format!(
                        "`path_for(\"{name}\", …)` was not given `{param}`, which the route \
                         `{pattern}` needs"
                    ));
                    path.push_str(segment);
                }
            },
            None => path.push_str(segment),
        }
    }
    if path.is_empty() {
        path.push('/');
    }

    // Whatever the pattern did not take is a query. Order is the caller's, so
    // the same call always produces the same URL.
    let query: Vec<String> = values
        .iter()
        .filter(|(k, _)| !used.contains(&k.as_str()))
        .map(|(k, v)| format!("{}={}", encode(k), encode(&v.to_display())))
        .collect();
    if query.is_empty() {
        path
    } else {
        format!("{path}?{}", query.join("&"))
    }
}

/// Split a location into its path and its query string.
pub fn split_query(location: &str) -> (&str, &str) {
    match location.split_once('?') {
        Some((path, query)) => (path, query),
        None => (location, ""),
    }
}

/// Parse `a=1&b=two` into pairs, undoing percent-encoding.
///
/// A key with no `=` is present with an empty value, which is how a flag in a
/// URL (`?debug`) reads, and a repeated key keeps the first: a map has one slot
/// per name, and the alternative (a list, sometimes) would make every read of
/// every query parameter check which it got.
pub fn parse_query(query: &str) -> Vec<(String, Value)> {
    let mut out: Vec<(String, Value)> = Vec::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        let key = decode(key);
        if key.is_empty() || out.iter().any(|(k, _)| *k == key) {
            continue;
        }
        out.push((key, Value::Text(decode(value))));
    }
    out
}

/// Percent-decode, treating `+` as a space the way a query string does.
///
/// Public because a path parameter has to come back out of a URL as whatever
/// went in: `path_for` escapes a `/` in an id, and the route that captures it
/// has to undo that, or the view is handed `a%2Fb` and shows it to somebody.
pub fn percent_decode(s: &str) -> String {
    decode(s)
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    Ok(byte) => {
                        out.push(byte);
                        i += 2;
                    }
                    // Not an escape after all, so it is a literal `%`.
                    Err(_) => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    // A URL can carry any bytes; only valid UTF-8 can come back out as text.
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encode everything that is not unreserved, so a value carrying a `/`,
/// an `&` or a space survives being put in a URL and read back.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
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

/// Turn a rhai error into the sentence a Rux author should read: drop the
/// position that would point at the wrong place, then say it in Rux's terms.
///
/// One function so the two steps cannot be applied in only one of the six places
/// an expression can fail.
fn explain(message: &str) -> String {
    rux_phrasing(&strip_rhai_position(message))
}

/// Say what went wrong in Rux's vocabulary rather than rhai's.
///
/// The script tier is an implementation detail. Someone writing a `.rux` file
/// has been told they are writing Rux, and every message they see should be
/// about the thing they wrote: a binding, a signal, a handler. "Property not
/// found: nmae" is rhai talking to a rhai user about a rhai object map, and it
/// answers none of the questions the person reading it actually has, which are
/// what to change and where.
///
/// Only the failures people actually hit are translated. Anything unrecognised
/// passes through unchanged, because a slightly foreign message is much better
/// than a confidently wrong one, and this list will always be incomplete.
///
/// The advice matters as much as the wording. Each rewrite says what to do next,
/// since these are all failures with exactly one sensible fix.
fn rux_phrasing(message: &str) -> String {
    if message.starts_with("Too many operations") {
        return format!(
            "stopped after {MAX_OPERATIONS} steps without finishing; a loop that \
             never ends, or work too big for one handler"
        );
    }
    // `user.nmae` where the map has no `nmae`. The most common failure in the
    // language now that strict bindings raise instead of yielding `()`, so it
    // gets the fullest advice, including the escape hatch, which is not
    // guessable: rhai's `?.` does not guard a missing property.
    if let Some(name) = message.strip_prefix("Property not found: ") {
        let name = name.trim();
        return format!(
            "there is no `{name}` on that value; check the spelling, or write \
             `\"{name}\" in …` if it is legitimately sometimes absent"
        );
    }
    // A name that was never declared. Almost always a signal the author meant to
    // create, or a loop variable used outside its `r-for`.
    if let Some(name) = message.strip_prefix("Variable not found: ") {
        let name = name.trim();
        // In a component the likelier miss is a prop nobody declared.
        if is_fragment() {
            return format!(
                "`{name}` is not defined; if the caller passes it, declare it in <script> \
                 as `prop {name};`, otherwise as `let {name} = signal(…)`, or check the spelling"
            );
        }
        return format!(
            "`{name}` is not defined; declare it in <script> as \
             `let {name} = signal(…)`, or check the spelling"
        );
    }
    // rhai reports the argument *types* it looked for, which name rhai's types
    // (`i64`, `ImmutableString`) rather than anything a Rux author has heard of.
    // The count is the useful part of that, so it is the part kept.
    if let Some(rest) = message.strip_prefix("Function not found: ") {
        let rest = rest.trim();
        let (name, args) = match rest.split_once(" (") {
            Some((name, args)) => {
                let args = args.trim_end_matches(')');
                let count = if args.trim().is_empty() {
                    0
                } else {
                    args.split(',').count()
                };
                (name, Some(count))
            }
            None => (rest, None),
        };
        return match args {
            Some(1) => format!("there is no function `{name}` taking 1 argument"),
            Some(n) => format!("there is no function `{name}` taking {n} arguments"),
            None => format!("there is no function `{name}`"),
        };
    }
    // Reserved words are a real trap, because the good ones are exactly the
    // names someone would pick: `null`, `this`, `type_of`.
    if let Some(rest) = message.strip_suffix(" is a reserved keyword") {
        let name = rest.trim().trim_matches('\'');
        return format!("`{name}` is a reserved word, so it cannot be used as a name here");
    }
    message.to_string()
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
    /// Check the script against its type annotations. See [`check`] and
    /// `docs/10-types.md`. `cx.host` is filled in from what was registered.
    pub fn check_types(&self, cx: &check::Context) -> Vec<check::Finding> {
        self.check_types_recording(cx, false).0
    }

    /// [`Engine::check_types`], and with `record` set, the types it worked
    /// out, for an editor. See [`check::check_recording`].
    pub fn check_types_recording(&self, cx: &check::Context, record: bool) -> (Vec<check::Finding>, check::Table) {
        let mut cx = cx.clone();
        cx.host.extend(self.host_types.iter().cloned());
        // A template's pieces compile as the runtime compiles them to run.
        check::check_recording(&self.checked, &cx, &|src| front::compile(&self.engine, src).ok(), record)
    }

    /// The `type` declarations in `script`, as name and text, for another file
    /// that imports them with `use`. A script that does not compile declares
    /// nothing here; its own load says why.
    pub fn declared_types(script: &str) -> Vec<(String, String)> {
        match front::compile(&RhaiEngine::new(), script) {
            Ok(ast) => ast
                .annotations()
                .iter()
                .filter(|a| a.kind == rhai::AnnotationKind::Type)
                .map(|a| (a.name.to_string(), a.ty.clone()))
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// `src` compiled and merged with the document's functions, from the cache
    /// when it has been seen before. A source that fails to compile is not
    /// kept, so its error is reported every time it is run, as before.
    fn prepared(&mut self, src: &str) -> Result<Arc<AST>, front::CompileError> {
        if let Some(ast) = self.compiled.get(src) {
            return Ok(Arc::clone(ast));
        }
        let ast = front::compile(&self.engine, src)?;
        let merged = Arc::new(profile::time(profile::Phase::Merge, || self.funcs.merge(&ast)));
        if self.compiled.len() >= COMPILED_CAP {
            self.compiled.clear();
        }
        self.compiled.insert(src.to_string(), Arc::clone(&merged));
        Ok(merged)
    }

    /// Evaluate `src` (an expression or statements) with `locals` temporarily in
    /// scope. Script functions are available. Returns the resulting value.
    fn eval(&mut self, src: &str, locals: &[(String, Value)]) -> Option<Dynamic> {
        let merged = match self.prepared(src) {
            Ok(merged) => merged,
            Err(e) => {
                // A `{{ }}` or `@tap` that doesn't compile used to evaluate to
                // nothing, silently, the same failure mode as ignored CSS. Record
                // it so the dev overlay can say what's wrong.
                error(format!(
                    "expression `{}` failed to compile: {}",
                    trim_expr(src),
                    explain(&e.to_string())
                ));
                return None;
            }
        };
        let base = self.scope.len();
        for (name, value) in locals {
            self.scope.push(name.clone(), to_dynamic(value));
        }
        let result = profile::time(profile::Phase::Run, || {
            self.engine.eval_ast_with_scope::<Dynamic>(&mut self.scope, &merged)
        });
        self.scope.rewind(base); // drop the temporary locals
        match result {
            Ok(value) => Some(value),
            Err(e) => {
                let raw = e.to_string();
                if is_owed_prop(&raw) {
                    return None;
                }
                raise(
                    format!("expression `{}` failed: {}", trim_expr(src), explain(&raw)),
                    level_for(&raw),
                );
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

    /// [`run_handler`](Self::run_handler) with `locals` in scope, such as the
    /// `event` a gesture or an `emit` hands its handler.
    pub fn run_handler_in(&mut self, src: &str, locals: &[(String, Value)]) -> bool {
        self.eval(src, locals).is_some()
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
    /// Whether `src` is syntactically a script at all, without running it.
    ///
    /// Syntax only, deliberately. A handler names things that do not exist
    /// until it runs (an `r-for` local, a component's own state), and those are
    /// runtime lookups rather than compile errors, so compiling cannot produce
    /// a false alarm about them. What it does catch is a handler that could
    /// never run under any state, which until now reached the window and did
    /// nothing at all, silently, because nothing compiles a handler until the
    /// moment it is tapped.
    pub fn check_syntax(&self, src: &str) -> Result<(), String> {
        front::compile(&self.engine, src).map(|_| ()).map_err(|e| rux_phrasing(&e.message))
    }

    /// The functions and methods `src` calls that nothing could ever resolve.
    ///
    /// **This is the other half of [`Self::check_syntax`], and the half that was
    /// missing.** rhai resolves a function *name* when the call runs, not when
    /// it compiles, so `@tap="alert(…)"` compiles perfectly and does nothing
    /// when tapped. A `{{ }}` expression is evaluated during the build and
    /// reports the same mistake immediately, so the two halves of the language
    /// disagreed about whether a typo was worth mentioning: interpolations said
    /// so, handlers did not. A handler that does nothing is indistinguishable
    /// from a handler that never fired, which makes it the worse place to be
    /// silent.
    ///
    /// **Names only, deliberately, and not arity.** A name that is registered
    /// nowhere, defined by no `fn` and built into nothing can never resolve
    /// under any state, so saying so cannot be a false alarm. Arity is a
    /// different question with real traps in it (a method call carries its
    /// receiver as an argument, operators arrive as calls, a closure is called
    /// through `call`), and getting it wrong would flag working code. Noise
    /// here would be worse than the silence being fixed, because the whole
    /// value of a diagnostic is that it is worth reading.
    ///
    /// Variables are left alone for the reason [`Self::check_syntax`] gives: a
    /// handler legitimately names an `r-for` local or a component's own state
    /// that does not exist until it runs. Functions are not like that. Nothing
    /// a row or an instance brings into scope can add a function name.
    pub fn unknown_calls(&self, src: &str) -> Vec<CallProblem> {
        let Ok(ast) = front::compile(&self.engine, src) else {
            return Vec::new(); // a syntax error is `check_syntax`'s to report
        };
        self.unresolvable_calls(&ast)
    }

    /// The same check, over the `fn` bodies the document declared.
    ///
    /// A handler is not the only place a call hides. `fn refresh() { alert(…) }`
    /// is never compiled against anything until something calls it, and a `fn`
    /// nothing calls yet is exactly where a typo waits quietest. The bodies are
    /// already held as [`Self::funcs`] for dispatch, so checking them costs one
    /// more walk of an AST that is sitting there.
    ///
    /// Calls between the document's own functions resolve normally, because
    /// [`Self::callable_names`] reads the same `AST` this walks: a `fn` may call
    /// one declared below it, as it may at run time.
    pub fn unknown_calls_in_functions(&self) -> Vec<CallProblem> {
        self.unresolvable_calls(&self.funcs)
    }

    /// The walk both of the above share.
    fn unresolvable_calls(&self, ast: &AST) -> Vec<CallProblem> {
        let mut known = self.callable_names();
        // rhai's own keywords, which the interpreter answers without a
        // registration to find. `curry` is the one that mattered: rhai writes
        // it itself whenever a closure captures a local, so `items.filter(x =>
        // x >= limit)` with a local `limit` was reported as calling a function
        // that does not exist.
        known.extend(
            ["curry", "call", "Fn", "is_shared", "is_def_var", "is_def_fn", "type_of", "eval", "print", "debug"]
                .map(str::to_string),
        );
        // A plain call to a name declared as a variable may be an arrow held in
        // it (`let add = (a, b) => a + b; add(2, 3)`), which only running it
        // can tell. Left to run time, where it is an error if it is not one.
        let mut variables: HashSet<String> = self.scope.iter_raw().map(|(n, ..)| n.to_string()).collect();
        ast.walk(&mut |path| {
            if let Some(rhai::ASTNode::Stmt(rhai::Stmt::Var(decl, ..))) = path.last() {
                variables.insert(decl.0.name.to_string());
            }
            true
        });
        let own = self.own_fn_arities();
        let mut unknown: Vec<CallProblem> = Vec::new();
        ast.walk(&mut |path| {
            let Some(rhai::ASTNode::Expr(expr)) = path.last() else { return true };
            let mut note = |problem: CallProblem| {
                if !unknown.contains(&problem) {
                    unknown.push(problem);
                }
            };
            match expr {
                rhai::Expr::FnCall(call, ..) | rhai::Expr::MethodCall(call, ..) => {
                    // An operator reaches the AST as a call (`a + b` is `+`),
                    // and so does every comparison and index. They resolve
                    // through the interpreter's own tables rather than by name,
                    // so asking whether `+` is registered proves nothing.
                    if call.op_token.is_some() {
                        return true;
                    }
                    let name = call.name.as_str();
                    let held = matches!(expr, rhai::Expr::FnCall(..)) && variables.contains(name);
                    if !known.contains(name) && !held {
                        note(CallProblem::NoSuchFunction(name.to_string()));
                    } else if let Some(wanted) = own.get(name) {
                        // A method call carries its receiver as the first
                        // argument, which is how `x.f(y)` reaches `fn f(a, b)`.
                        let given = call.args.len()
                            + usize::from(matches!(expr, rhai::Expr::MethodCall(..)));
                        if !wanted.contains(&given) {
                            let mut wanted: Vec<usize> = wanted.iter().copied().collect();
                            wanted.sort_unstable();
                            note(CallProblem::WrongArgumentCount {
                                name: name.to_string(),
                                given,
                                wanted,
                            });
                        }
                    }
                }
                // A method call and its receiver are two nodes, and the
                // receiver is what says whether `set` here is an array's or a
                // signal's. Both arms are needed: the walk visits the `Dot` and
                // the `MethodCall` separately, and only the `Dot` has the pair.
                rhai::Expr::Dot(pair, ..) => {
                    if let Some(problem) = self.stale_signal_api(&pair.lhs, &pair.rhs) {
                        note(problem);
                    }
                }
                _ => {}
            }
            true
        });
        unknown
    }

    /// Names read inside a `fn` body that **nothing anywhere could supply**.
    ///
    /// ## Why this is not the obvious check
    ///
    /// A `fn` body cannot be checked against its own parameters and locals, the
    /// way it could in most languages, because of divergence 4 in the fork: in
    /// Rux **a call runs in the scope it was written in**, so a function sees
    /// its caller's locals. Driven and confirmed: `fn outer() { let x = 42;
    /// inner(); }` with `fn inner() { n = x; }` sets `n` to 42. Checking a body
    /// in isolation would therefore report the single most useful thing the
    /// fork exists to allow.
    ///
    /// So the question asked here is weaker and answerable: **is this name
    /// declared anywhere at all?** A name that is no signal, no parameter, no
    /// `let` and no loop variable in this document cannot be in scope under any
    /// caller, because scope is made of declarations and there is no
    /// declaration of it to be in. `fn addUser() { Have }` is the shape this
    /// catches, and it is the shape a typo takes.
    ///
    /// `also` is what the *runtime* knows and this crate does not: the names an
    /// `r-for` row brings in, and the locals of every handler in the template,
    /// since any of those can be the caller whose scope a body is running in.
    ///
    /// ## Reported as an error
    ///
    /// Decided by the user, 2026-09-17, looking at the squiggle: a name nothing
    /// anywhere declares is a mistake, and a caution let `rux check` exit 0 on a
    /// document that cannot work. What keeps that safe is how narrow the
    /// question is. This never asks whether a name is in scope *here*, only
    /// whether it is declared *at all*, and `also` carries the names the runtime
    /// knows about that this crate cannot see. The measure is the corpus: all 47
    /// examples and all 21 components alone produce no report.
    /// `own_script_lines` is how much of the compiled text is **this document's
    /// own** `<script>`. Every component's `fn`s are appended to it before
    /// compiling, so the one AST holds functions from several files, and a
    /// report about a line past that mark belongs to a file this is not
    /// checking. Naming the wrong file is worse than saying nothing: unplaced is
    /// vague, placed and wrong is a trap, which this project has already paid
    /// for once. A component is reported when it is the file being checked.
    /// Each report carries the **script-relative** line it was read on, which
    /// the runtime turns into a line in the file. Without one it is drawn at
    /// the top of the document, pointing at `<template>` for a mistake in
    /// `<script>`.
    pub fn unknown_names_in_functions(
        &self,
        also: &HashSet<String>,
        own_script_lines: usize,
    ) -> Vec<(String, Option<usize>)> {
        let mut in_scope = declared_in(&self.funcs);
        in_scope.extend(self.signals.iter().cloned());
        in_scope.extend(also.iter().cloned());

        let mut unknown: Vec<(String, Option<usize>)> = Vec::new();
        self.funcs.walk(&mut |path| {
            let Some(rhai::ASTNode::Expr(rhai::Expr::Variable(var, _, pos))) = path.last() else {
                return true;
            };
            // Past the mark is a component's function, riding in the same AST.
            if pos.line().is_some_and(|line| line > own_script_lines) {
                return true;
            }
            // A qualified name (`mod::thing`) resolves through a module and not
            // through any scope, so scope has nothing to say about it.
            if !var.2.is_empty() {
                return true;
            }
            let name = var.1.as_str();
            if in_scope.contains(name) {
                return true;
            }
            // A bare name that is really a function is a call the other check
            // owns, and reporting it here would say the same thing twice in
            // different words.
            if self.callable_names().contains(name) {
                return true;
            }
            // One report per name, at the first place it is read: a name
            // misspelled the same way twice is one thing to fix.
            if !unknown.iter().any(|(n, _)| n == name) {
                unknown.push((name.to_string(), pos.line()));
            }
            true
        });
        unknown
    }

    /// Every name `src` declares, for handing back as part of `also` above.
    ///
    /// A handler is a caller, so its locals are in scope inside whatever it
    /// calls. Compiling it here rather than in the runtime keeps every piece of
    /// rhai knowledge on this side of the boundary.
    pub fn declared_names(&self, src: &str) -> HashSet<String> {
        match front::compile(&self.engine, src) {
            Ok(ast) => declared_in(&ast),
            // A syntax error is `check_syntax`'s to report, and an AST that does
            // not exist declares nothing.
            Err(_) => HashSet::new(),
        }
    }

    /// `signal.set(x)` and `signal.get()`, which name real functions and still
    /// cannot work.
    ///
    /// These two escape [`Self::unknown_calls`]'s name check because `set` and
    /// `get` *are* registered: `set` on an array, a map, a blob and a string,
    /// `get` on a map. So `searching.set(true)` compiles, resolves nothing at
    /// run time for a bool, and does nothing, in silence. It is the single most
    /// misleading thing an author can write, because it is what
    /// `docs/02-spec.md` taught until 2026-08-25.
    ///
    /// The arity is what makes this safe to report. Every registered `set`
    /// takes **two** arguments after the receiver (`tasks.set(0, "x")` on an
    /// array signal is legitimate and stays silent), and the map's `get` takes
    /// one. A one-argument `set` or a no-argument `get` on a name that is a
    /// signal is the v0.3-superseded API and nothing else, so there is no state
    /// in which it could have worked.
    fn stale_signal_api(&self, receiver: &rhai::Expr, call: &rhai::Expr) -> Option<CallProblem> {
        let rhai::Expr::Variable(var, ..) = receiver else { return None };
        let signal = var.1.as_str();
        if !self.signals.contains(signal) {
            return None;
        }
        let (rhai::Expr::MethodCall(call, ..) | rhai::Expr::FnCall(call, ..)) = call else {
            return None;
        };
        let stale = matches!((call.name.as_str(), call.args.len()), ("set", 1) | ("get", 0));
        stale.then(|| CallProblem::StaleSignalApi {
            signal: signal.to_string(),
            method: call.name.to_string(),
        })
    }

    /// The parameter counts of the `fn`s this document declared.
    ///
    /// A name may appear more than once: rhai dispatches a script function on
    /// its argument count, so `fn f(a)` and `fn f(a, b)` are two functions and
    /// both counts are legal.
    ///
    /// **A name the host or a package also registered is left out entirely.**
    /// The native side can be overloaded on types nothing here can see, so a
    /// count that no `fn` accepts might still be resolved by a native of the
    /// same name. Dropping those is what keeps this from flagging working code,
    /// which matters more than catching every case.
    fn own_fn_arities(&self) -> HashMap<String, HashSet<usize>> {
        let native: HashSet<String> = self
            .engine
            .collect_fn_metadata(None, |f| Some(f.metadata.name.to_string()), true)
            .into_iter()
            .collect();
        let mut arities: HashMap<String, HashSet<usize>> = HashMap::new();
        for f in self.funcs.iter_functions() {
            if native.contains(f.name) {
                continue;
            }
            arities.entry(f.name.to_string()).or_default().insert(f.params.len());
        }
        arities
    }

    /// Every function name this engine could resolve a call to.
    ///
    /// Three sources, because a call can land in any of them: what the host and
    /// the script tier registered, what the standard packages bring, and the
    /// `fn`s the document itself declared. The first two come from the engine,
    /// which is the same place the interpreter looks, so this cannot drift from
    /// what would actually happen at run time. The third is the document's own
    /// `AST`, which is where `fn refresh()` lives.
    fn callable_names(&self) -> HashSet<String> {
        let mut names: HashSet<String> = self
            .engine
            .collect_fn_metadata(None, |f| Some(f.metadata.name.to_string()), true)
            .into_iter()
            .collect();
        names.extend(self.funcs.iter_functions().map(|f| f.name.to_string()));
        names
    }

    pub fn run_handler_tracked(&mut self, src: &str) -> HashSet<String> {
        self.run_handler_tracked_in(src, &[])
    }

    /// Run `f` and report which signals it changed.
    ///
    /// Only the signals it was about to change are compared, each against the
    /// value it had just before: the fork tells [`note_write`] about every
    /// assignment, property or index chain and by-reference call on a variable
    /// before it happens. Copying and comparing every signal around every run
    /// used to cost more than the run itself once a list was large.
    ///
    /// A tracked run inside another hands what it found to the outer one, as
    /// signals changed, so nothing is lost by nesting.
    fn tracking<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> (R, HashSet<String>) {
        let log = WriteLog {
            scope: &self.scope as *const Scope as *const () as usize,
            signals: self.signals.clone(),
            before: HashMap::new(),
            unsnapped: HashSet::new(),
        };
        // Debug builds also take the old whole-state copy and check the
        // tracked answer against it, so every test that runs a handler proves
        // the fork's hook missed nothing. Release builds skip it.
        #[cfg(debug_assertions)]
        let every: Vec<(String, Option<Value>)> =
            self.signals.iter().map(|n| (n.clone(), self.read_signal(n))).collect();
        let outer = WRITES.with(|w| w.borrow_mut().replace(log));
        let out = f(self);
        let log = WRITES.with(|w| std::mem::replace(&mut *w.borrow_mut(), outer));
        let Some(log) = log else { return (out, HashSet::new()) };
        let changed: HashSet<String> = profile::time(profile::Phase::Diff, || {
            let mut changed = log.unsnapped;
            for (name, before) in log.before {
                if self.read_signal(&name) != before {
                    changed.insert(name);
                }
            }
            changed
        });
        #[cfg(debug_assertions)]
        for (name, before) in every {
            if self.read_signal(&name) != before {
                assert!(
                    changed.contains(&name),
                    "write tracking missed `{name}`: it changed, and no write to it was seen"
                );
            }
        }
        WRITES.with(|w| {
            if let Some(outer) = w.borrow_mut().as_mut() {
                outer.unsnapped.extend(changed.iter().cloned());
            }
        });
        (out, changed)
    }

    /// [`run_handler_tracked`](Self::run_handler_tracked) with `locals` in
    /// scope, such as the `event` a gesture hands its handler. Passed as a
    /// local rather than written into the source, so the source stays the
    /// same from one tap to the next and is compiled once.
    pub fn run_handler_tracked_in(&mut self, src: &str, locals: &[(String, Value)]) -> HashSet<String> {
        let (ran, changed) = self.tracking(|e| e.run_handler_in(src, locals));
        if ran { changed } else { HashSet::new() }
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
    /// `None` for a name that is not a signal.
    pub fn signal_value(&self, name: &str) -> Option<Value> {
        if !self.signals.contains(name) {
            return None;
        }
        self.read_signal(name)
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
        let merged = match self.prepared(script) {
            Ok(merged) => merged,
            Err(e) => {
                warn(format!(
                    "a component's script failed to compile: {}",
                    explain(&e.to_string())
                ));
                return Vec::new();
            }
        };
        // Its own functions plus everything already registered, so a component
        // can call helpers it declared beside its state: `prepared` merges them.
        let mut scope = Scope::new();
        if let Err(e) =
            profile::time(profile::Phase::Run, || self.engine.run_ast_with_scope(&mut scope, &merged))
        {
            warn(format!(
                "a component's script failed to run: {}",
                explain(&e.to_string())
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
        let merged = match self.prepared(src) {
            Ok(merged) => merged,
            Err(e) => {
                warn(format!(
                    "handler `{}` failed to compile: {}",
                    trim_expr(src),
                    explain(&e.to_string())
                ));
                return (locals.to_vec(), HashSet::new());
            }
        };
        let ((after, result), changed) = self.tracking(|e| {
            let base = e.scope.len();
            for (name, value) in locals {
                e.scope.push(name.clone(), to_dynamic(value));
            }
            let result = profile::time(profile::Phase::Run, || {
                e.engine.eval_ast_with_scope::<Dynamic>(&mut e.scope, &merged)
            });
            // Read the instance's state back *before* rewinding, or the handler's
            // effect on it is dropped along with the temporary scope.
            let after: Vec<(String, Value)> = locals
                .iter()
                .map(|(name, previous)| {
                    let value = e
                        .scope
                        .get_value::<Dynamic>(name)
                        .map(|d| from_dynamic(&d))
                        .unwrap_or_else(|| previous.clone());
                    (name.clone(), value)
                })
                .collect();
            e.scope.rewind(base);
            (after, result)
        });

        if let Err(e) = result {
            warn(format!(
                "handler `{}` failed: {}",
                trim_expr(src),
                explain(&e.to_string())
            ));
            return (after, HashSet::new());
        }
        (after, changed)
    }

    /// [`run_scoped_handler`](Self::run_scoped_handler), also reporting what the
    /// body **read**.
    ///
    /// An effect inside a component needs all three answers: its own instance's
    /// state afterwards, which document signals it moved, and what it read, since
    /// what it read is what it is subscribed to. A handler needs only the first
    /// two, which is why the plain form does not pay for the tracking.
    pub fn run_scoped_effect(
        &mut self,
        src: &str,
        locals: &[(String, Value)],
    ) -> (Vec<(String, Value)>, HashSet<String>, HashSet<String>) {
        READS.with(|r| *r.borrow_mut() = Some(HashSet::new()));
        let (after, changed) = self.run_scoped_handler(src, locals);
        let mut reads = READS.with(|r| r.borrow_mut().take()).unwrap_or_default();
        // Only document signals are subscriptions. An instance's own names are
        // not: they change through a handler, which rebuilds anyway.
        reads.retain(|n| self.signals.contains(n));
        (after, changed, reads)
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
        READS.with(|r| *r.borrow_mut() = Some(HashSet::new()));
        let (ran, writes) = self.tracking(|e| e.eval(src, &[]).is_some());
        let mut reads = READS.with(|r| r.borrow_mut().take()).unwrap_or_default();
        reads.retain(|n| self.signals.contains(n));
        if !ran {
            // It still subscribes to whatever it managed to read, so a fixed
            // signal re-runs it rather than leaving it dead until a reload.
            return (reads, HashSet::new());
        }
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
        // The value is a person's typing, so it is quoted as a literal rather
        // than pasted in: a quote or a backslash in a text field would otherwise
        // be a syntax error at best.
        let src = format!("{target} = {}", rux_reactive::json_string(value));
        let (ran, changed) = self.tracking(|e| e.eval(&src, locals).is_some());
        if ran { changed } else { HashSet::new() }
    }

    /// [`assign_string`](Self::assign_string) for a value that is not text: a
    /// `type="number"` writes a number, and a slider does too.
    ///
    /// Handed over as a local rather than spelled as a literal, so no value
    /// has to survive a round trip through script syntax (`1e21`, `-0`).
    pub fn assign_value(
        &mut self,
        target: &str,
        value: &Value,
        locals: &[(String, Value)],
    ) -> HashSet<String> {
        let mut locals = locals.to_vec();
        locals.push((ASSIGNED.to_string(), value.clone()));
        let src = format!("{target} = {ASSIGNED}");
        let (ran, changed) = self.tracking(|e| e.eval(&src, &locals).is_some());
        if ran { changed } else { HashSet::new() }
    }
}

/// The local [`Engine::assign_value`] hands its value over in. Spelled so no
/// author's name can shadow it.
pub const ASSIGNED: &str = "__rux_assigned";

/// Every name an AST declares: `fn` parameters, `let` bindings, and the
/// variables a `for` loop brings in.
///
/// Deliberately flat rather than per-scope. A `let` inside one block does not
/// really reach a sibling block, but pretending it might is the safe direction
/// of wrong: it can only make this quieter, never louder, and quiet is the side
/// a name check has to fail towards.
pub(crate) fn declared_in(ast: &AST) -> HashSet<String> {
    let mut names: HashSet<String> = HashSet::new();
    for f in ast.iter_functions() {
        names.extend(f.params.iter().map(|p| p.to_string()));
    }
    ast.walk(&mut |path| {
        match path.last() {
            Some(rhai::ASTNode::Stmt(rhai::Stmt::Var(decl, ..))) => {
                names.insert(decl.0.name.to_string());
            }
            Some(rhai::ASTNode::Stmt(rhai::Stmt::For(loop_, ..))) => {
                names.insert(loop_.0.name.to_string());
                if let Some(counter) = &loop_.1 {
                    names.insert(counter.name.to_string());
                }
            }
            _ => {}
        }
        true
    });
    names
}

fn to_dynamic(v: &Value) -> Dynamic {
    match v {
        Value::Number(n) => Dynamic::from(*n),
        Value::Text(s) => Dynamic::from(s.clone()),
        Value::Bool(b) => Dynamic::from(*b),
        Value::Null => Dynamic::UNIT,
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
    // Before the fallback below, which wrote `()` as the empty text.
    if d.is_unit() {
        return Value::Null;
    }
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

        // The cause survives, in Rux's wording rather than rhai's: this test is
        // about the position suffix, not about who phrases the message.
        assert!(message.contains("is not defined"), "keeps the cause: {message}");
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

    /// Strict bindings, v0.7 item 1. A typo in a property name has to be a
    /// reported failure, not an empty render.
    ///
    /// This is the behaviour two milestones of notes said required forking rhai.
    /// It did not: `set_fail_on_invalid_map_property` is a stock engine option.
    /// The test is here to pin that down, because the belief that it was
    /// impossible survived a long time on nobody checking.
    #[test]
    fn a_missing_map_property_is_reported_not_silent() {
        let mut e = engine();
        let user = Value::Map(vec![("name".into(), Value::Text("Grace".into()))]);
        let locals = [("user".to_string(), user)];

        let _ = take_warnings();
        assert_eq!(e.eval_display("user.name", &locals), "Grace");
        assert!(take_warnings().is_empty(), "a property that exists says nothing");

        // The typo. Before strict bindings this rendered "" and reported
        // nothing, which reads exactly like a value that is legitimately absent.
        assert_eq!(e.eval_display("user.nmae", &locals), "");
        let warnings = take_warnings();
        assert_eq!(warnings.len(), 1, "the typo is reported");
        assert!(
            warnings[0].message.contains("nmae"),
            "the report names the property: {}",
            warnings[0].message
        );
    }

    /// `?.` and `??`, which strict bindings creates the need for: once a missing
    /// property raises, there has to be a way to say "absent is fine".
    ///
    /// Both are stock rhai tokens. Verified here rather than assumed, because
    /// the tokenizer having a symbol is not the same as the runtime honouring
    /// it, and item 1 is unusable if they do not work.
    #[test]
    fn optional_chaining_opts_out_of_strictness() {
        let mut e = engine();
        let user = Value::Map(vec![("name".into(), Value::Text("Grace".into()))]);
        let locals = [("user".to_string(), user)];

        let _ = take_warnings();

        // `??` works, and on a property that exists it is the whole story.
        assert_eq!(e.eval_display("user.name ?? \"none\"", &locals), "Grace");
        assert!(take_warnings().is_empty());

        // `?.` guards an absent *property*, which is the fork's first
        // divergence. Upstream raises here, because its `?.` only guards an
        // absent base, and under strict bindings that left the strictness with
        // no opt-out at all. See crates/rux-rhai/DIVERGENCE.md.
        assert_eq!(e.eval_display("user?.nickname", &locals), "");
        assert!(
            take_warnings().is_empty(),
            "`?.` says the author expected this to be absent"
        );

        // The two compose into the shape someone actually writes.
        assert_eq!(e.eval_display("user?.nickname ?? \"none\"", &locals), "none");
        assert_eq!(e.eval_display("user?.name ?? \"none\"", &locals), "Grace");
        assert!(take_warnings().is_empty());

        // What `?.` guarded before, and still does: a base that is absent.
        assert_eq!(e.eval_display("missing?.anything", &[]), "");

        // And a plain `.` is still strict. The divergence widens `?.`; it does
        // not weaken `.`, which is the whole point of having both.
        let _ = take_warnings();
        let _ = e.eval_display("user.nickname", &locals);
        assert_eq!(take_warnings().len(), 1, "`.` still raises on a missing property");
    }

    /// `===` and `!==` are the strict comparison under a JS spelling. No loose
    /// equality comes with them: `"5" === 5` is false, as in JS.
    #[test]
    fn js_equality_spellings_are_strict() {
        let mut e = engine();
        assert!(e.eval_bool("level === 82", &[]));
        assert!(!e.eval_bool("level !== 82", &[]));
        assert!(e.eval_bool("\"a\" === \"a\"", &[]));
        assert!(!e.eval_bool("\"5\" === 5", &[]), "no coercion, unlike JS `==`");
        assert!(e.eval_bool("\"5\" !== 5", &[]));
    }

    /// `n++` and `n--`, the counter idiom, which is Rux's own hello-world.
    ///
    /// Desugared in the fork to `n += 1` / `n -= 1`, so what is being checked
    /// here is that the spelling reaches the same place the long form does,
    /// including through a signal (an f64) rather than only an integer.
    #[test]
    fn increment_and_decrement() {
        let mut e = engine();
        let _ = take_warnings();

        e.run_handler("level++");
        assert_eq!(e.eval_display("level", &[]), "83");
        e.run_handler("level--");
        e.run_handler("level--");
        assert_eq!(e.eval_display("level", &[]), "81");
        assert!(take_warnings().is_empty(), "no warning: {:?}", take_warnings());

        // It is an assignment, so it tracks as a write like any other, which is
        // what makes a binding on `level` update.
        let written = e.run_handler_tracked("level++");
        assert!(written.contains("level"), "the write is tracked: {written:?}");
    }

    /// `++` works on a map property too, since it desugars to `+=` and inherits
    /// everything `+=` already handles.
    #[test]
    fn increment_reaches_a_property() {
        let mut e = engine();
        let _ = take_warnings();
        assert_eq!(
            e.eval_display("let c = #{ taps: 0 }; c.taps++; c.taps++; c.taps", &[]),
            "2"
        );
        assert!(take_warnings().is_empty(), "{:?}", take_warnings());
    }

    /// Arrow functions, in all four shapes a JS developer writes.
    ///
    /// Each is exactly the matching `|…|` form; the fork adds the spelling and
    /// nothing else.
    #[test]
    fn arrow_functions() {
        let mut e = engine();
        let _ = take_warnings();
        let nums = Value::List(vec![Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)]);
        let locals = [("nums".to_string(), nums)];

        // `x => …`, the form nearly every callback is written in.
        assert_eq!(e.eval_display("nums.map(x => x * 2).join(\",\")", &locals), "2,4,6");
        // `(x) => …`, the same thing with parentheses, which is where the
        // ambiguity with an ordinary parenthesised expression begins.
        assert_eq!(e.eval_display("nums.map((x) => x + 1).join(\",\")", &locals), "2,3,4");
        // `(a, b) => …`, the shape that needs more than one token of lookahead
        // and the reason the token stream had to grow a buffer at all.
        assert_eq!(e.eval_display("nums.reduce(|sum, x| sum + x, 0)", &locals), "6");
        assert_eq!(e.eval_display("nums.filter((x, i) => i > 0).join(\",\")", &locals), "2,3");
        // `() => …`, no parameters.
        assert_eq!(e.eval_display("let f = () => 7; f.call()", &[]), "7");

        assert!(take_warnings().is_empty(), "{:?}", take_warnings());
    }

    /// The lookahead must not change what a parenthesised expression means.
    ///
    /// This is the whole risk of the change: `(a, b) => …` and `(a + b)` start
    /// identically, so a lookahead that guesses wrong would quietly break
    /// ordinary arithmetic rather than failing loudly.
    #[test]
    fn parentheses_still_mean_what_they_meant() {
        let mut e = engine();
        let _ = take_warnings();

        assert_eq!(e.eval_display("(2 + 3) * 4", &[]), "20");
        assert_eq!(e.eval_display("(level)", &[]), "82");
        assert_eq!(e.eval_display("(level) + 1", &[]), "83");
        // A parenthesised name followed by something that is *not* `=>`.
        assert_eq!(e.eval_display("(level) > 50", &[]), "true");
        // And the unit value, which shares its token with `() =>`.
        assert_eq!(e.eval_display("()", &[]), "");
        assert!(take_warnings().is_empty(), "{:?}", take_warnings());
    }

    /// `{ a: 1 }` is a map, as in JS, and `#{ a: 1 }` still is.
    #[test]
    fn bare_brace_maps() {
        let mut e = engine();
        let _ = take_warnings();
        let nums = Value::List(vec![Value::Number(1.0), Value::Number(2.0)]);
        let locals = [("nums".to_string(), nums)];

        assert_eq!(e.eval_display("let m = { a: 1, b: \"two\" }; m.b", &[]), "two");
        assert_eq!(e.eval_display("{ a: 1 }.a + 1", &[]), "2");
        assert_eq!(e.eval_display("let m = { \"x-y\": 3 }; m[\"x-y\"]", &[]), "3");
        assert_eq!(e.eval_display("let m = {}; m.len()", &[]), "0");
        assert_eq!(e.eval_display("{ a: { b: 5 } }.a.b", &[]), "5");
        assert_eq!(e.eval_display("[{ a: 1 }, { a: 2 }][1].a", &[]), "2");
        assert_eq!(e.eval_display("#{ a: 7 }.a", &[]), "7");
        // An arrow's body: JS needs `({ … })` here, this does not.
        assert_eq!(
            e.eval_display("nums.map(n => { id: n * 10 })[1].id", &locals),
            "20"
        );
        // `() => {}` is still an empty body, the no-op handler.
        assert_eq!(e.eval_display("let f = () => {}; f.call()", &[]), "");
        assert!(take_warnings().is_empty(), "{:?}", take_warnings());
    }

    /// A `{` that does not start with `name:` or `"string":` is a block, as
    /// before. `host::x` must not read as a key, since `::` is its own token.
    #[test]
    fn braces_that_are_not_maps_are_still_blocks() {
        let mut e = engine();
        let _ = take_warnings();

        assert_eq!(e.eval_display("let x = { let y = 2; y * 3 }; x", &[]), "6");
        assert_eq!(e.eval_display("let x = { level + 1 }; x", &[]), "83");
        assert_eq!(e.eval_display("let f = n => { n + 1 }; f.call(1)", &[]), "2");
        assert_eq!(e.eval_display("let f = n => { let d = n * 2; d }; f.call(4)", &[]), "8");
        assert_eq!(e.eval_display("if true { 1 } else { 2 }", &[]), "1");
        assert!(take_warnings().is_empty(), "{:?}", take_warnings());
    }

    /// **Annotations are erased.** Every form the type system adds parses and
    /// runs exactly as the same script would with its annotations deleted.
    #[test]
    fn annotations_are_erased() {
        let mut e = engine();
        let _ = take_warnings();
        let nums = Value::List(vec![Value::Number(1.0), Value::Number(2.0)]);
        let locals = [("nums".to_string(), nums)];

        assert_eq!(e.eval_display("let n: int = 2; n + 1", &[]), "3");
        assert_eq!(e.eval_display("const LIMIT: number = 4; LIMIT", &[]), "4");
        assert_eq!(e.eval_display("let s: string? = null; s ?? \"none\"", &[]), "none");
        assert_eq!(
            e.eval_display("let t: { id: int, note?: string } = { id: 7 }; t.id", &[]),
            "7"
        );
        assert_eq!(
            e.eval_display("type Filter = \"all\" | \"open\"; let f: Filter = \"all\"; f", &[]),
            "all"
        );
        assert_eq!(e.eval_display("nums.map((n: number) => n * 10)[1]", &locals), "20");
        assert_eq!(e.eval_display("let add = (a: number, b: number) => a + b; add(2, 3)", &[]), "5");
        assert_eq!(e.eval_display("nums.map(n => { id: n })[0].id", &locals), "1");
        // `type` is a word only where a declaration can start.
        assert_eq!(e.eval_display("let type = 1; type + 1", &[]), "2");
        assert_eq!(e.eval_display("type_of(\"x\")", &[]), "string");
        assert!(take_warnings().is_empty(), "{:?}", take_warnings());
    }

    /// **`?[` guards a missing key**, as `?.` guards a missing property
    /// (DIVERGENCE.md, item 9), and a past-the-end index too. Plain `[` still
    /// raises on both. Also the upstream bug item 9 fixed on the way: a `?[`
    /// followed by more chain lost its guard.
    #[test]
    fn question_bracket_guards_a_missing_key() {
        let mut e = engine();
        let _ = take_warnings();
        assert_eq!(e.eval_display("let m = { x: 1 }; let k = \"nope\"; m?[k] ?? \"safe\"", &[]), "safe");
        assert_eq!(e.eval_display("let m = { x: 1 }; m?[\"x\"]", &[]), "1");
        assert_eq!(e.eval_display("let xs = [1]; xs?[5] ?? \"past\"", &[]), "past");
        assert_eq!(e.eval_display("let m = { x: 1 }; let k = \"no\"; m?[k].y ?? \"chain\"", &[]), "chain");
        assert_eq!(e.eval_display("let a = null; a?[0].x ?? \"null base\"", &[]), "null base");
        assert!(take_warnings().is_empty(), "{:?}", take_warnings());
        // Plain `[` is as strict as ever.
        let _ = e.eval_display("let m = { x: 1 }; m[\"nope\"]", &[]);
        assert_eq!(take_warnings().len(), 1, "a missing key under `[` still raises");
    }

    /// Functions with annotated parameters and results, called both ways.
    #[test]
    fn annotated_functions_run() {
        let mut e = Builder::new()
            .build(
                "fn dbl(x: number): number { x * 2 }\n\
                 fn label(t: { title: string }, i: int): string { `${i}. ${t.title}` }\n\
                 fn shape(): { a: number } { { a: 1 } }\n\
                 fn none() { 5 }",
            )
            .expect("compiles");
        assert_eq!(e.eval_display("dbl(4)", &[]), "8");
        assert_eq!(e.eval_display("label({ title: \"x\" }, 2)", &[]), "2. x");
        assert_eq!(e.eval_display("shape().a", &[]), "1");
        assert_eq!(e.eval_display("none()", &[]), "5");
    }

    /// The side table: every annotation, with what it annotates and the text
    /// of its type, and nothing in the AST itself.
    #[test]
    fn annotations_are_kept_beside_the_ast() {
        use rhai::AnnotationKind as K;
        let src = "type Task = { id: int, title: string, note?: string };\n\
                   type Load =\n  | { state: \"idle\" }\n  | { state: \"done\", rows: Task[] };\n\
                   type Flags = { [string]: bool };\n\
                   type Pick = (Task, int) => bool;\n\
                   type Later = () => null;\n\
                   type Maybe = (Task | string)[];\n\
                   type Holes = string?[];\n\
                   let tasks: Task[] = [];\n\
                   const LIMIT: int = 3;\n\
                   fn label(t: Task, i: int): string { t.title }\n\
                   fn shape(): { a: number } { { a: 1 } }\n\
                   let f = (a: number, b) => a + b;";
        let ast = rhai::Engine::new().compile(src).expect("compiles");
        let got: Vec<(K, String, String)> = ast
            .annotations()
            .iter()
            .map(|a| (a.kind.clone(), a.name.to_string(), a.ty.clone()))
            .collect();
        let find = |name: &str| {
            got.iter()
                .find(|(_, n, _)| n == name)
                .unwrap_or_else(|| panic!("no annotation for {name}: {got:?}"))
                .clone()
        };

        assert_eq!(find("Task").0, K::Type);
        assert_eq!(find("Task").2, "{ id: int, title: string, note?: string }");
        assert_eq!(find("Load").2, "| { state: \"idle\" } | { state: \"done\", rows: Task[] }");
        assert_eq!(find("Flags").2, "{[string]: bool }");
        assert_eq!(find("Pick").2, "( Task, int ) => bool");
        assert_eq!(find("Later").2, "() => null");
        assert_eq!(find("Maybe").2, "( Task | string )[]");
        assert_eq!(find("Holes").2, "string?[]");
        assert_eq!((find("tasks").0, find("tasks").2), (K::Var, "Task[]".to_string()));
        assert_eq!((find("LIMIT").0, find("LIMIT").2), (K::Var, "int".to_string()));

        let label = K::Param { function: "label".into(), arity: 2 };
        assert!(got.contains(&(label.clone(), "t".into(), "Task".into())), "{got:?}");
        assert!(got.contains(&(label, "i".into(), "int".into())), "{got:?}");
        assert!(got.contains(&(K::Result { arity: 2 }, "label".into(), "string".into())), "{got:?}");
        assert!(got.contains(&(K::Result { arity: 0 }, "shape".into(), "{ a: number }".into())));

        // An arrow's parameter belongs to the generated function; `b` has no
        // annotation, so only `a` is recorded.
        let arrow: Vec<_> = got.iter().filter(|(k, ..)| matches!(k, K::Param { function, .. } if function.starts_with("anon$"))).collect();
        assert_eq!(arrow.len(), 1, "{got:?}");
        assert_eq!((arrow[0].1.as_str(), arrow[0].2.as_str()), ("a", "number"));

        // The positions point at the names, which is what the checker keys on.
        let tasks = ast.annotations().iter().find(|a| a.name == "tasks").unwrap();
        assert_eq!((tasks.pos.line(), tasks.pos.position()), (Some(10), Some(5)));
    }

    /// A malformed annotation is a syntax error at the place it goes wrong,
    /// not a silently different program.
    #[test]
    fn a_malformed_annotation_is_a_syntax_error() {
        let fails = |src: &str| rhai::Engine::new().compile(src).err().map(|e| e.to_string());
        assert!(fails("let x: = 1;").unwrap().contains("Expecting a type after ':'"));
        assert!(fails("fn f(x: ) { x }").unwrap().contains("Expecting a type"));
        assert!(fails("type T = ;").unwrap().contains("Expecting a type after '='"));
        assert!(fails("let x: { a int } = 1;").is_some());
        assert!(fails("fn f() { type T = int; 1 }").unwrap().contains("top level"));
        assert_eq!(fails("let x: { a: int, } = 1;"), None, "a trailing comma is allowed");
    }

    /// `forEach` with the JS-shaped callback, which is the case that motivated
    /// arrow functions in the first place.
    #[test]
    fn for_each_with_an_arrow() {
        let mut e = engine();
        let items = Value::List(vec![Value::Text("a".into()), Value::Text("b".into())]);
        let locals = [("items".to_string(), items)];
        let _ = take_logs();

        e.eval("items.forEach((item, i) => print(`${i}:${item}`))", &locals);
        assert_eq!(take_logs(), vec!["0:a", "1:b"]);
    }

    /// **A named `fn` can read and write the state around it.**
    ///
    /// This retires the single biggest trap in the language. Until the fork,
    /// rhai functions were scopeless: a `fn` could not see a top-level `let` at
    /// all, which is why every handler in every example and every doc had to be
    /// written inline, and why `/learn` spent a callout on it.
    #[test]
    fn a_function_sees_and_writes_the_scope_around_it() {
        let mut e = engine(); // has `level = 82`
        let _ = take_warnings();

        // Reading. Upstream this raised: `level` was not in the function's
        // scope, and no argument was passed.
        assert_eq!(e.eval_display("read_level()", &[]), "82");

        // Writing, which is the half that makes a reusable handler possible.
        e.run_handler("bump()");
        assert_eq!(e.eval_display("level", &[]), "83");
        e.run_handler("bump(); bump()");
        assert_eq!(e.eval_display("level", &[]), "85");

        // A function calling another function, both reaching the same state.
        // The narrower "signals are shared cells" design would not have covered
        // this: it is a plain read of a top-level name from two frames down.
        assert_eq!(e.eval_display("describe()", &[]), "level is 85");

        assert!(take_warnings().is_empty(), "{:?}", take_warnings());
    }

    /// The write a function makes is tracked like any other, so a binding that
    /// reads the signal actually updates.
    ///
    /// Without this the feature would look like it worked and the screen would
    /// not move, which is the failure mode this project keeps finding.
    #[test]
    fn a_write_from_inside_a_function_is_tracked() {
        let mut e = engine();
        let written = e.run_handler_tracked("bump()");
        assert!(written.contains("level"), "the write is seen: {written:?}");
    }

    /// A function's own locals stay its own: seeing the outer scope must not
    /// mean leaking into it.
    #[test]
    fn function_locals_do_not_escape() {
        let mut e = engine();
        let _ = take_warnings();
        assert_eq!(e.eval_display("with_local()", &[]), "5");
        // `temp` was declared inside the function and is gone.
        let _ = e.eval_display("temp", &[]);
        assert_eq!(take_warnings().len(), 1, "the local did not leak out");
        // And a parameter shadows an outer name for the call only.
        assert_eq!(e.eval_display("shadow(1)", &[]), "1");
        assert_eq!(e.eval_display("level", &[]), "82", "the outer name is unharmed");
    }

    /// JavaScript truthiness, in a condition and behind `!`.
    ///
    /// Upstream a condition had to already be a `bool`; anything else was a type
    /// error. Someone arriving from JS writes `if user { … }` and
    /// `r-if="items.length"`, and in binding position they are not thinking
    /// about a scripting language's type rules at all.
    #[test]
    fn js_truthiness() {
        let mut e = engine();
        let _ = take_warnings();

        // Numbers, strings and the empty value.
        assert!(e.eval_bool("1", &[]));
        assert!(!e.eval_bool("0", &[]));
        assert!(e.eval_bool("\"a\"", &[]));
        assert!(!e.eval_bool("\"\"", &[]));
        assert!(!e.eval_bool("null", &[]));
        // A non-empty string is truthy whatever it says, as in JS.
        assert!(e.eval_bool("\"0\"", &[]));
        assert!(e.eval_bool("\"false\"", &[]));

        // Inside the script, not only in a binding: this is the half that was
        // a type error before.
        assert_eq!(e.eval_display("if level { \"yes\" } else { \"no\" }", &[]), "yes");
        assert_eq!(e.eval_display("if 0 { \"yes\" } else { \"no\" }", &[]), "no");
        assert_eq!(e.eval_display("if \"\" { \"yes\" } else { \"no\" }", &[]), "no");

        // `!` on anything.
        assert!(e.eval_bool("!\"\"", &[]));
        assert!(!e.eval_bool("!\"a\"", &[]));
        assert!(e.eval_bool("!0", &[]));

        // `&&` and `||` short-circuit over any value.
        assert_eq!(e.eval_display("\"\" || \"fallback\"", &[]), "true");
        assert!(e.eval_bool("level && true", &[]));

        assert!(take_warnings().is_empty(), "{:?}", take_warnings());
    }

    /// An empty list and an empty map are **truthy**, which is the one place
    /// this departs from what most people would guess.
    ///
    /// Pinned deliberately. `r-if="items"` used to mean "there are items"; it
    /// now means "items exists", and `r-if="items.length"` is how to ask the
    /// other question. Any change here is a silent behaviour change in every
    /// document that ever wrote the short form.
    #[test]
    fn an_empty_collection_is_truthy() {
        let mut e = engine();
        let empty = [("rows".to_string(), Value::List(Vec::new()))];
        assert!(e.eval_bool("rows", &empty), "JS: every object is truthy");
        assert!(!e.eval_bool("rows.length", &empty), "and this is how to ask");

        let full = [("rows".to_string(), Value::List(vec![Value::Number(1.0)]))];
        assert!(e.eval_bool("rows", &full));
        assert!(e.eval_bool("rows.length", &full));
    }

    /// Division reads the way it reads in JS, and the special values are
    /// spelled the way JS spells them.
    #[test]
    fn division_is_not_integer_division() {
        let mut e = engine();
        let _ = take_warnings();
        assert_eq!(e.eval_display("10 / 3", &[]), "3.3333333333333335");
        assert_eq!(e.eval_display("10 / 2", &[]), "5", "a whole result stays whole");
        assert_eq!(e.eval_display("7 / 2", &[]), "3.5");
        // A signal was always an f64, so this half already worked; it is here so
        // the two spellings are pinned as agreeing.
        assert_eq!(e.eval_display("level / 4", &[]), "20.5");
        // As in JS, rather than raising.
        assert_eq!(e.eval_display("1 / 0", &[]), "Infinity");
        assert_eq!(e.eval_display("-1 / 0", &[]), "-Infinity");
        assert_eq!(e.eval_display("0 / 0", &[]), "NaN");
        // And NaN is falsy, which is what makes that survivable in a binding.
        assert!(!e.eval_bool("0 / 0", &[]));
        assert!(take_warnings().is_empty(), "{:?}", take_warnings());
    }

    /// Indexing with a number that arrived as an f64, which is what every signal
    /// is. If this fails, `items[i]` is broken for any `i` a document computed.
    #[test]
    fn indexing_with_a_signal_number() {
        let mut e = engine();
        let _ = take_warnings();
        let locals = [
            ("rows".to_string(), Value::List(vec![
                Value::Text("a".into()),
                Value::Text("b".into()),
                Value::Text("c".into()),
            ])),
            ("i".to_string(), Value::Number(1.0)),
        ];
        assert_eq!(e.eval_display("rows[i]", &locals), "b");
        assert_eq!(e.eval_display("rows[i + 1]", &locals), "c");
        assert!(take_warnings().is_empty(), "{:?}", take_warnings());
    }

    /// `null` is the empty value under the name someone arriving from JS uses.
    #[test]
    fn null_is_the_empty_value() {
        let mut e = engine();
        assert_eq!(e.eval_display("null", &[]), "");
        assert_eq!(e.eval_display("null ?? \"fallback\"", &[]), "fallback");
        // It is a literal, not state: it never reaches the signal set, so
        // nothing can subscribe to it.
        assert!(!e.signals.contains("null"));
    }

    /// `null` leaves script as `Value::Null`, not the empty text, and comes
    /// back as `null`: in scope, and baked into a handler as a literal.
    #[test]
    fn null_survives_the_trip_out_of_script_and_back() {
        let mut e = engine();
        assert_eq!(e.eval_value("null", &[]), Some(Value::Null));
        let row = e.eval_value("{ title: \"a\", note: null }", &[]).unwrap();
        assert_eq!(row, Value::Map(vec![("note".into(), Value::Null), ("title".into(), Value::Text("a".into()))]));
        let locals = [("row".to_string(), row.clone())];
        assert_eq!(e.eval_value("row?.note ?? \"none\"", &locals), Some(Value::Text("none".into())));
        let baked = format!("let row = {}; row.note == null", row.to_rhai_literal());
        assert_eq!(e.eval_value(&baked, &[]), Some(Value::Bool(true)));
    }

    /// The JS names for operations rhai already had under other names, plus the
    /// two it did not have at all (`join`, `forEach`).
    #[test]
    fn js_collection_and_string_names_work() {
        let mut e = engine();
        let items = Value::List(vec![
            Value::Text("a".into()),
            Value::Text("b".into()),
            Value::Text("c".into()),
        ]);
        let locals = [("items".to_string(), items)];

        assert_eq!(e.eval_display("items.length", &locals), "3");
        assert_eq!(e.eval_display("\"hello\".length", &[]), "5");
        // A length is used to index and to build a range far more often than it
        // is displayed, and both need an integer. `.length` returning an f64
        // read correctly here and broke `keyed-list.rux`, so both uses are
        // pinned.
        assert_eq!(e.eval_display("items[items.length - 1]", &locals), "c");
        assert_eq!(
            e.eval_display("let n = 0; for i in 0..items.length { n += 1 } n", &locals),
            "3"
        );
        assert!(e.eval_bool("items.includes(\"b\")", &locals));
        assert!(!e.eval_bool("items.includes(\"z\")", &locals));
        assert_eq!(e.eval_display("items.indexOf(\"c\")", &locals), "2");
        assert_eq!(e.eval_display("items.indexOf(\"z\")", &locals), "-1");
        assert_eq!(e.eval_display("items.join(\"-\")", &locals), "a-b-c");
        assert_eq!(e.eval_display("items.slice(1).join(\"\")", &locals), "bc");
        assert_eq!(e.eval_display("items.slice(0, 2).join(\"\")", &locals), "ab");
        // JS's negative index, counting from the end.
        assert_eq!(e.eval_display("items.slice(-1).join(\"\")", &locals), "c");
        // Out of range clamps rather than raising, which is the reason to use
        // `slice` instead of indexing in the first place.
        assert_eq!(e.eval_display("items.slice(9).length", &locals), "0");

        assert_eq!(e.eval_display("\"Rux\".toUpperCase()", &[]), "RUX");
        assert_eq!(e.eval_display("\"Rux\".toLowerCase()", &[]), "rux");
        assert!(e.eval_bool("\"hello\".startsWith(\"he\")", &[]));
        assert!(e.eval_bool("\"hello\".endsWith(\"lo\")", &[]));
        assert!(e.eval_bool("\"hello\".includes(\"ell\")", &[]));
        assert_eq!(e.eval_display("\"hello\".indexOf(\"l\")", &[]), "2");
        assert_eq!(e.eval_display("\"hello\".slice(1, 3)", &[]), "el");
        assert_eq!(e.eval_display("\"ab\".repeat(3)", &[]), "ababab");
        assert_eq!(e.eval_display("\"hello\".charAt(1)", &[]), "e");
        assert_eq!(e.eval_display("\"hello\".charAt(99)", &[]), "");
    }

    /// `forEach` is the one array method with no rhai equivalent under any name.
    #[test]
    fn for_each_runs_a_callback_per_item() {
        let mut e = engine();
        let items = Value::List(vec![Value::Number(1.0), Value::Number(2.0)]);
        let locals = [("items".to_string(), items)];
        let _ = take_logs();

        e.eval("items.forEach(|x| print(x))", &locals);
        // "1", not rhai's "1.0": a printed number reads the way the same number
        // reads in a `{{ }}` binding.
        assert_eq!(take_logs(), vec!["1", "2"]);

        // JS hands the index as a second argument, and a one-parameter callback
        // still works, which is the form nearly everyone writes.
        e.eval("items.forEach(|x, i| print(`${i} ${x}`))", &locals);
        assert_eq!(take_logs(), vec!["0 1", "1 2"]);
    }

    /// Text becomes a number under JavaScript's names, with JavaScript's
    /// answers, which is what matching a route parameter against numeric ids
    /// needs (watchlist #11).
    #[test]
    fn text_becomes_a_number_as_in_javascript() {
        let mut e = engine();
        let n = |e: &mut Engine, src: &str| match e.eval_value(src, &[]) {
            Some(Value::Number(n)) => n,
            other => panic!("{src} gave {other:?}"),
        };
        assert_eq!(n(&mut e, "Number(\"2\") + 1"), 3.0);
        assert_eq!(n(&mut e, "Number(\" 2.5 \")"), 2.5);
        assert_eq!(n(&mut e, "Number(\"\")"), 0.0);
        assert!(n(&mut e, "Number(\"12px\")").is_nan());
        assert_eq!(n(&mut e, "Number(\"0x1F\")"), 31.0);
        assert_eq!(n(&mut e, "Number(true)"), 1.0);
        assert_eq!(n(&mut e, "parseInt(\"42px\")"), 42.0);
        assert_eq!(n(&mut e, "parseInt(\"-7.9\")"), -7.0);
        assert_eq!(n(&mut e, "parseInt(\"ff\", 16)"), 255.0);
        assert!(n(&mut e, "parseInt(\"px\")").is_nan());
        assert_eq!(n(&mut e, "parseFloat(\"12.5px\")"), 12.5);
        assert_eq!(n(&mut e, "parseFloat(\"1e3\")"), 1000.0);
        assert_eq!(n(&mut e, "parseFloat(\".5\")"), 0.5);
        assert!(n(&mut e, "parseFloat(\"abc\")").is_nan());
        assert!(e.eval_bool("isNaN(Number(\"x\"))", &[]));
        assert_eq!(e.eval_display("String(2.5) + \"!\"", &[]), "2.5!");
        // The case that found it: an id from a route matched to a number.
        assert!(e.eval_bool("Number(\"2\") == 2", &[]));
    }

    /// An arrow held in a variable is called like a function, as in
    /// JavaScript, and `rux check` does not call that a typo. A real `fn` of
    /// the same name still wins, and a real typo is still reported.
    #[test]
    fn an_arrow_in_a_variable_is_called_like_a_function() {
        let mut e = Builder::new()
            .build("let out = signal(0); let twice = x => x * 2; fn add(a, b) { a + b + 100 }")
            .expect("build");
        let run = |e: &mut Engine, src: &str| {
            e.run_handler(src);
            e.eval_display("out", &[])
        };
        assert_eq!(run(&mut e, "let sum = (a, b) => a + b; out = sum(2, 3)"), "5");
        assert_eq!(run(&mut e, "let seven = () => 7; out = seven()"), "7");
        assert_eq!(run(&mut e, "let k = 10; let plus = x => x + k; out = plus(1)"), "11", "captures");
        assert_eq!(run(&mut e, "out = twice(21)"), "42", "declared in the script");
        assert_eq!(run(&mut e, "let bump = () => out += 1; out = 0; bump(); bump()"), "2");
        assert_eq!(run(&mut e, "let add = (a, b) => a * b; out = add(2, 3)"), "105", "a fn wins");

        assert!(e.unknown_calls("let sum = (a, b) => a + b; out = sum(2, 3)").is_empty());
        assert!(
            e.unknown_calls("let t = 2; out = [1, 2, 3].filter(x => x >= t).length").is_empty(),
            "a closure capturing a local compiles to `curry`, which is rhai's own"
        );
        assert_eq!(e.unknown_calls("out = nothing_here(1)").len(), 1, "a typo is still a typo");
    }

    /// A handler that never finishes is stopped and says so, instead of
    /// holding the UI thread for good (watchlist #31).
    #[test]
    fn an_endless_loop_is_stopped_and_reported() {
        let mut e = engine();
        // A small ceiling, so the test is quick in a debug build; the real one
        // is [`MAX_OPERATIONS`].
        e.engine.set_max_operations(100_000);
        let _ = take_warnings();
        assert!(!e.run_handler("let n = 0; while true { n += 1; }"));
        let said: Vec<String> = take_warnings().into_iter().map(|w| w.message).collect();
        assert!(said.iter().any(|m| m.contains("without finishing")), "{said:?}");
        // And the engine is still usable afterwards.
        e.run_handler("level = 5");
        assert_eq!(e.eval_display("level", &[]), "5");
    }

    /// Upstream rhai #1126 (fixed in 1.26.0, after this fork's 1.25.1 base):
    /// the optimizer could delete a block's `let` while its reads kept their
    /// scope slot, so `b` below answered with `a`'s value, 1, and said nothing.
    /// Reproduced in the fork on 2026-09-24. Rux is clear of it only because
    /// it runs with the optimizer off, for its own reason (see `Builder::new`);
    /// this fails first if that is ever turned back on before the fix is in.
    #[test]
    fn a_block_local_read_by_a_switch_keeps_its_own_value() {
        let mut e = engine();
        assert!(e.run_handler("let a = 1; level = { let b = 99; switch b { _ => b } };"));
        assert_eq!(e.eval_display("level", &[]), "99");
    }

    /// Upstream rhai #1123 and #1117, backported into the fork 2026-09-24: a
    /// variable an arrow had captured never matched its `switch` case, and an
    /// exact case whose guard failed skipped the ranges. Both answered with
    /// the default branch and said nothing, and Rux arrows capture all the time.
    #[test]
    fn a_captured_variable_still_matches_its_switch_case() {
        let mut e = engine();
        assert!(e.run_handler(
            "let x = 5; let f = || x; level = switch x { 5 => 1, _ => 0 };"
        ));
        assert_eq!(e.eval_display("level", &[]), "1");
        assert!(e.run_handler(
            "let x = 5; level = switch x { 5 if false => 1, 0..10 => 2, _ => 0 };"
        ));
        assert_eq!(e.eval_display("level", &[]), "2");
    }

    /// The same loop, inside a closure a native method calls back. Upstream
    /// rhai found (after 1.26.1, unreleased at the fork's base) that
    /// operations counted inside such a callback were discarded, so this is
    /// the shape that could walk past the limit.
    #[test]
    fn an_endless_loop_inside_a_callback_is_stopped_too() {
        let mut e = engine();
        e.engine.set_max_operations(100_000);
        let _ = take_warnings();
        let started = std::time::Instant::now();
        assert!(!e.run_handler("[1, 2].map(|x| { let n = 0; while true { n += 1; } })"));
        assert!(started.elapsed().as_secs() < 30, "stopped, not left running");
        let said: Vec<String> = take_warnings().into_iter().map(|w| w.message).collect();
        assert!(said.iter().any(|m| m.contains("without finishing")), "{said:?}");
    }

    /// The callback methods JavaScript has that rhai lacked or answered
    /// differently.
    #[test]
    fn every_find_index_and_sort_answer_as_javascript_does() {
        let mut e = engine();
        assert!(e.eval_bool("[1, 2, 3].every(x => x > 0)", &[]));
        assert!(!e.eval_bool("[1, 2, 3].every(x => x > 1)", &[]));
        assert_eq!(e.eval_display("[5, 6, 7].findIndex(x => x == 7)", &[]), "2");
        assert_eq!(e.eval_display("[5, 6, 7].findIndex(x => x == 9)", &[]), "-1");
        assert_eq!(e.eval_display("[3, 1, 2].sort((a, b) => a - b)[0]", &[]), "1");
        assert_eq!(e.eval_display("[3, 1, 2].sort((a, b) => b - a).join(\",\")", &[]), "3,2,1");
    }

    /// `log` is the printf-debugging the script tier had no way to do at all.
    /// Its sink is separate from the warning sink on purpose: output that is
    /// working as intended must not show up in the overlay's list of problems.
    #[test]
    fn log_collects_separately_from_warnings() {
        let mut e = engine();
        let _ = take_logs();
        let _ = take_warnings();

        e.eval("print(\"level is \" + level)", &[]);
        assert_eq!(take_logs(), vec!["level is 82"]);
        assert!(take_warnings().is_empty(), "a log is not a problem");

        // Repetition is kept, unlike warnings: a log printed ten times in a loop
        // is telling you it ran ten times.
        e.eval("print(1); print(1)", &[]);
        assert_eq!(take_logs().len(), 2);
    }

    /// Failures are reported in Rux's vocabulary, not rhai's, and each one says
    /// what to do about it.
    #[test]
    fn messages_are_phrased_for_a_rux_author() {
        // The commonest failure now that bindings are strict. The advice
        // includes the escape hatch, which is not guessable: `?.` does not
        // guard a missing property.
        let m = rux_phrasing("Property not found: nmae");
        assert!(m.contains("`nmae`"), "{m}");
        assert!(m.contains("spelling"), "{m}");
        assert!(m.contains("\"nmae\" in"), "the escape hatch is offered: {m}");

        let m = rux_phrasing("Variable not found: counter");
        assert!(m.contains("`counter` is not defined"), "{m}");
        assert!(m.contains("signal("), "it says how to declare one: {m}");

        // rhai names its own types in the argument list; the count is the part
        // worth keeping, and it is spelled for a human.
        assert_eq!(
            rux_phrasing("Function not found: greet (i64, ImmutableString)"),
            "there is no function `greet` taking 2 arguments"
        );
        assert_eq!(
            rux_phrasing("Function not found: greet (i64)"),
            "there is no function `greet` taking 1 argument"
        );
        assert_eq!(
            rux_phrasing("Function not found: greet ()"),
            "there is no function `greet` taking 0 arguments"
        );

        let m = rux_phrasing("'this' is a reserved keyword");
        assert!(m.contains("`this` is a reserved word"), "{m}");

        // Anything unrecognised passes through untouched. A slightly foreign
        // message beats a confidently wrong one, and this list is never
        // complete.
        assert_eq!(rux_phrasing("Runtime error: something odd"), "Runtime error: something odd");
    }

    /// End to end: a real typo in a real binding produces the rewritten message,
    /// so the six failure sites are actually routed through it.
    #[test]
    fn a_typo_in_a_binding_reads_as_rux() {
        let mut e = engine();
        let user = Value::Map(vec![("name".into(), Value::Text("Grace".into()))]);
        let _ = take_warnings();
        let _ = e.eval_display("user.nmae", &[("user".to_string(), user)]);
        let warnings = take_warnings();
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].message.contains("there is no `nmae` on that value"),
            "rhai's wording did not survive: {}",
            warnings[0].message
        );
        assert!(
            !warnings[0].message.contains("Property not found"),
            "rhai's wording leaked: {}",
            warnings[0].message
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
             fn double(x) { x * 2 } \
             fn read_level() { level } \
             fn bump() { level++ } \
             fn describe() { \"level is \" + read_level() } \
             fn with_local() { let temp = 5; temp } \
             fn shadow(level) { level }",
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

    /// Every way a handler can change a signal is seen by the fork's write
    /// hook, which is all the handler's changes are found from now. Each case
    /// here is a path into the engine that reaches a variable differently.
    #[test]
    fn tracks_every_kind_of_write() {
        let changed = |src: &str| {
            let mut e = Builder::new()
                .build(
                    "let n = signal(1); let items = signal([1, 2]); \
                     let user = signal(#{ name: \"a\" }); let total = signal(0); \
                     fn bump() { n += 1; } fn push_one(list) { list.push(1); }",
                )
                .unwrap();
            let mut v: Vec<String> = e.run_handler_tracked(src).into_iter().collect();
            v.sort();
            v
        };
        // A method that changes its receiver.
        assert_eq!(changed("items.push(3)"), ["items"]);
        // An index and a property, written through a chain.
        assert_eq!(changed("items[0] = 9"), ["items"]);
        assert_eq!(changed("user.name = \"b\""), ["user"]);
        // A function writing a signal it reaches through the caller's scope.
        assert_eq!(changed("bump()"), ["n"]);
        // A closure writing what it captured.
        assert_eq!(changed("[1, 2].forEach(x => total += x)"), ["total"]);
        // A local that shadows a signal takes the write; the signal is untouched.
        assert_eq!(changed("let n = 5; n = 6;"), Vec::<String>::new());
        // A method that only reads is reported to the hook, and then compared
        // equal, so nothing is claimed.
        assert_eq!(changed("let k = items.len();"), Vec::<String>::new());
        // A script function handed the list gets a copy; the signal stays put.
        assert_eq!(changed("push_one(items)"), Vec::<String>::new());
    }

    // ── query() ─────────────────────────────────────────────────────────────

    /// A resolver standing in for the one the runtime installs, answering with
    /// two cards for `.card` and nothing for anything else.
    fn cards() -> ElementResolver {
        Arc::new(|selector: &str| match selector {
            ".card" => Some(vec![
                ElementFacts {
                    path: vec![0, 0],
                    tag: "view".into(),
                    id: Some("first".into()),
                    classes: vec!["card".into()],
                    bounds: Some(ElementBox { x: 4.0, y: 8.0, width: 120.0, height: 40.0 }),
                },
                ElementFacts {
                    path: vec![0, 1],
                    tag: "view".into(),
                    id: None,
                    classes: vec!["card".into(), "wide".into()],
                    // Stands for a node with no box: hidden, or never laid out.
                    bounds: None,
                },
            ]),
            "!" => None, // stands for a selector that does not parse
            _ => Some(Vec::new()),
        })
    }

    #[test]
    fn query_returns_handles_inside_a_handler() {
        let mut e = engine();
        with_elements(cards(), || {
            assert_eq!(e.eval_display("query(\".card\").length", &[]), "2");
            assert_eq!(e.eval_display("query(\".card\")[0].tag", &[]), "view");
            assert_eq!(e.eval_display("query(\".card\")[0].id", &[]), "first");
            assert_eq!(e.eval_display("query(\".card\")[1].classes.join(\" \")", &[]), "card wide");
            // A selector that matches nothing is an empty list, not an error.
            assert_eq!(e.eval_display("query(\".nope\").length", &[]), "0");
        });
    }

    /// `trim` hands back the trimmed string rather than emptying its receiver.
    ///
    /// rhai's trims in place and returns `()`, so `{{ name.trim() }}` rendered
    /// blank: the call returned nothing and the nothing was displayed. Every
    /// other string method here returns a value, and so does JavaScript's.
    #[test]
    fn trim_returns_the_trimmed_string() {
        let mut e = engine();
        assert_eq!(e.eval_display("\"  hi  \".trim()", &[]), "hi");
        // Chains, which the in-place version could not do at all.
        assert_eq!(e.eval_display("\"  a,b \".trim().split(\",\").length", &[]), "2");
        // And the receiver is untouched, so a signal is not silently emptied.
        assert_eq!(e.eval_display("let s = \" x \"; s.trim(); s", &[]), " x ");
    }

    /// Geometry reads back as plain numbers, so it does arithmetic like any
    /// other number in the language.
    #[test]
    fn geometry_reads_back_as_numbers() {
        let mut e = engine();
        with_elements(cards(), || {
            assert_eq!(e.eval_display("query(\".card\")[0].width", &[]), "120");
            assert_eq!(e.eval_display("query(\".card\")[0].height", &[]), "40");
            assert_eq!(e.eval_display("query(\".card\")[0].x", &[]), "4");
            assert_eq!(e.eval_display("query(\".card\")[0].y", &[]), "8");
            // And it is a number, not a string that happens to look like one.
            assert_eq!(e.eval_display("query(\".card\")[0].width / 2", &[]), "60");
        });
    }

    /// A node with no box reads as absent, never as zero. "Not laid out" and
    /// "laid out, and genuinely zero wide" are different answers and a script
    /// has to be able to tell them apart.
    #[test]
    fn geometry_of_an_unlaid_node_is_absent_not_zero() {
        let mut e = engine();
        with_elements(cards(), || {
            assert_eq!(e.eval_display("query(\".card\")[1].width ?? \"none\"", &[]), "none");
        });
    }

    /// The actions record an intent and nothing more. What focusing *means*
    /// belongs to the runtime, which is why this is a queue and not a call.
    #[test]
    fn the_actions_queue_rather_than_act() {
        let mut e = engine();
        let _ = take_element_actions();
        with_elements(cards(), || {
            e.eval_display("query(\".card\")[0].focus()", &[]);
            e.eval_display("query(\".card\")[1].scrollIntoView()", &[]);
            e.eval_display("blur()", &[]);
        });

        assert_eq!(
            take_element_actions(),
            vec![
                ElementAction::Focus(vec![0, 0]),
                ElementAction::ScrollIntoView(vec![0, 1]),
                ElementAction::Blur,
            ],
            "in the order they were asked for"
        );
        assert!(take_element_actions().is_empty(), "draining empties the sink");
    }

    /// An absent id reads as absent, so `??` works on it like anything else.
    #[test]
    fn a_missing_id_is_absent_rather_than_empty() {
        let mut e = engine();
        with_elements(cards(), || {
            assert_eq!(e.eval_display("query(\".card\")[1].id ?? \"none\"", &[]), "none");
        });
    }

    /// The handler-only rule, which is enforced by the resolver simply not
    /// being installed rather than by a check anyone has to remember.
    #[test]
    fn query_outside_a_handler_is_an_error_not_an_empty_list() {
        let mut e = engine();
        let _ = take_warnings();

        assert_eq!(e.eval_display("query(\".card\").length", &[]), "");
        let warnings = take_warnings();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].message.contains("only available inside a handler"),
            "says why: {warnings:?}"
        );
    }

    /// A selector that cannot be parsed is reported. Answering "nothing
    /// matched" would make a typo indistinguishable from an empty result.
    #[test]
    fn an_unparseable_selector_is_reported() {
        let mut e = engine();
        with_elements(cards(), || {
            let _ = take_warnings();
            assert_eq!(e.eval_display("query(\"!\").length", &[]), "");
            let warnings = take_warnings();
            assert_eq!(warnings.len(), 1, "{warnings:?}");
            assert!(warnings[0].message.contains("not a selector"), "{warnings:?}");
        });
    }

    /// The install is scoped and restores what it replaced, so a handler that
    /// triggers another handler does not come back unable to query.
    #[test]
    fn nesting_restores_the_outer_resolver() {
        let mut e = engine();
        with_elements(cards(), || {
            with_elements(Arc::new(|_: &str| Some(Vec::new())), || {
                assert_eq!(e.eval_display("query(\".card\").length", &[]), "0");
            });
            assert_eq!(e.eval_display("query(\".card\").length", &[]), "2", "outer is back");
        });
        // And once outside every scope, the capability is gone again.
        let _ = take_warnings();
        assert_eq!(e.eval_display("query(\".card\").length", &[]), "");
        assert_eq!(take_warnings().len(), 1);
    }
}



