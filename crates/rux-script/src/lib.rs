//! Rux script tier — milestone M8.
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

use rhai::{Dynamic, Engine as RhaiEngine, Module, Scope, AST};
use rux_reactive::Value;

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
        // Record every variable read while dependency-tracking is active, then
        // fall through (`Ok(None)`) to normal scope resolution. `on_var` is
        // flagged volatile upstream, not deprecated — hence the allow.
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

/// A live script engine: state in `scope`, script functions in `funcs`.
pub struct Engine {
    engine: RhaiEngine,
    scope: Scope<'static>,
    funcs: AST,
    /// Names of the top-level signals — the universe of reactive dependencies.
    signals: HashSet<String>,
}

impl Engine {
    /// Evaluate `src` (an expression or statements) with `locals` temporarily in
    /// scope. Script functions are available. Returns the resulting value.
    fn eval(&mut self, src: &str, locals: &[(String, Value)]) -> Option<Dynamic> {
        let ast = self.engine.compile(src).ok()?;
        let merged = self.funcs.merge(&ast);

        let base = self.scope.len();
        for (name, value) in locals {
            self.scope.push(name.clone(), to_dynamic(value));
        }
        let result = self.engine.eval_ast_with_scope::<Dynamic>(&mut self.scope, &merged);
        self.scope.rewind(base); // drop the temporary locals
        result.ok()
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

    /// Evaluate an expression *and* report which signals it read — the binding's
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

    /// Run an `@tap` handler and report which signals it *changed* — the write
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

    /// A signal's current value, read straight from the scope (no evaluation).
    fn read_signal(&self, name: &str) -> Option<Value> {
        self.scope.get_value::<Dynamic>(name).map(|d| from_dynamic(&d))
    }

    /// Read a signal's current value as a display string (for input `r-model`).
    pub fn get_string(&mut self, name: &str) -> String {
        self.eval_value(name, &[]).map(|v| v.to_display()).unwrap_or_default()
    }

    /// Set a signal to a string value (from input editing).
    pub fn set_string(&mut self, name: &str, value: &str) {
        self.scope.set_or_push(name, value.to_string());
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
    Value::Text(d.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// A binding reports exactly the signals it read — the subscription set.
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

    /// A handler reports exactly the signals it changed — and nothing it left
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
