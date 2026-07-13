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

use rhai::{Dynamic, Engine as RhaiEngine, Module, Scope, AST};
use rux_reactive::Value;

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

        Ok(Engine {
            engine: self.engine,
            scope,
            funcs,
        })
    }
}

/// A live script engine: state in `scope`, script functions in `funcs`.
pub struct Engine {
    engine: RhaiEngine,
    scope: Scope<'static>,
    funcs: AST,
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
}
