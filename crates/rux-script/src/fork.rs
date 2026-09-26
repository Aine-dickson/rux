//! The rhai fork, kept for one thing: debug builds run it beside Rux's
//! interpreter and compare the two (see [`crate::shadow`]). Nothing uses
//! its answers. Step 5.5 of `docs/11-next.md` deletes it, with `rux-rhai`.

use std::collections::HashMap;
use std::sync::Arc;

use rhai::{Dynamic, Engine as RhaiEngine, EvalAltResult, ImmutableString, Module, Scope, AST};
use rux_reactive::Value;

use crate::*;

/// The fork, with a document's state.
pub(crate) struct Fork {
    engine: RhaiEngine,
    scope: Scope<'static>,
    funcs: AST,
    compiled: HashMap<String, Arc<AST>>,
}

impl Fork {
    /// The fork with `script`'s top level run, or why it could not be.
    pub(crate) fn build(script: &str, host: &HashMap<String, interp::Host>) -> Result<Fork, String> {
        let mut engine = fork_engine();
        let mut module = Module::new();
        for (name, f) in host {
            let f = std::rc::Rc::clone(f);
            module.set_native_fn(name.as_str(), move || -> Result<f64, Box<EvalAltResult>> { Ok(f()) });
        }
        engine.register_static_module("host", module.into());
        let (ast, _) = front::compile_script_keeping(&engine, script).map_err(|e| e.message)?;
        let mut scope = Scope::new();
        engine.run_ast_with_scope(&mut scope, &ast).map_err(|e| e.to_string())?;
        let funcs = ast.clone_functions_only();
        Ok(Fork { engine, scope, funcs, compiled: HashMap::new() })
    }

    fn prepared(&mut self, src: &str) -> Result<Arc<AST>, String> {
        if let Some(ast) = self.compiled.get(src) {
            return Ok(Arc::clone(ast));
        }
        let ast = front::compile(&self.engine, src).map_err(|e| e.message)?;
        let merged = Arc::new(self.funcs.merge(&ast));
        self.compiled.insert(src.to_string(), Arc::clone(&merged));
        Ok(merged)
    }

    /// `src` run with `locals` in scope: its value, and each local as it
    /// stands afterwards, read back by name.
    pub(crate) fn run(&mut self, src: &str, locals: &[(String, Value)]) -> (Result<Value, String>, Vec<Value>) {
        let merged = match self.prepared(src) {
            Ok(m) => m,
            Err(e) => return (Err(e), locals.iter().map(|(_, v)| v.clone()).collect()),
        };
        let base = self.scope.len();
        for (name, value) in locals {
            self.scope.push(name.clone(), to_dynamic(value));
        }
        let result = self.engine.eval_ast_with_scope::<Dynamic>(&mut self.scope, &merged);
        let after = locals
            .iter()
            .map(|(name, previous)| {
                self.scope.get_value::<Dynamic>(name).map(|d| from_dynamic(&d)).unwrap_or_else(|| previous.clone())
            })
            .collect();
        self.scope.rewind(base);
        (result.map(|d| from_dynamic(&d)).map_err(|e| e.to_string()), after)
    }

    /// A component's script run apart from the document: the state it makes.
    pub(crate) fn init_scope(&mut self, script: &str) -> Vec<(String, Value)> {
        let Ok(merged) = self.prepared(script) else { return Vec::new() };
        let mut scope = Scope::new();
        let _ = self.engine.run_ast_with_scope(&mut scope, &merged);
        scope.iter().map(|(name, _, value)| (name.to_string(), from_dynamic(&value))).collect()
    }

    #[cfg(test)]
    pub(crate) fn set_max_operations(&mut self, n: u64) {
        self.engine.set_max_operations(n);
    }

    pub(crate) fn set(&mut self, name: &str, value: &Value) {
        self.scope.set_or_push(name, to_dynamic(value));
    }

    /// Every name in the document's scope, and its value.
    pub(crate) fn state(&self) -> Vec<(String, Value)> {
        self.scope.iter().map(|(n, _, v)| (n.to_string(), from_dynamic(&v))).collect()
    }
}

fn fork_engine() -> RhaiEngine {
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
        // What `front::lower` makes of `x is T` now, so the fork never reads a
        // type: the same test under a name the fork's own `is` does not use.
        engine.register_fn("__is", |value: Dynamic, written: ImmutableString| validate::is(&value, &written));
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
        engine
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
    //
    // Step 3 of `docs/11-next.md`: `parseInt` gives an `int?` and `parseFloat`
    // a `float?`, so text that is not a number is `none`, where JavaScript
    // says `NaN`. `Number` keeps JavaScript's answer.
    let or_none = |n: f64| if n.is_nan() { Dynamic::UNIT } else { Dynamic::from(n) };
    engine.register_fn("parseInt", move |s: ImmutableString| or_none(js_parse_int(&s, 10)));
    engine.register_fn("parseInt", move |s: ImmutableString, radix: Dynamic| {
        or_none(js_parse_int(&s, num(&radix) as u32))
    });
    engine.register_fn("parseInt", |n: f64| n.trunc());
    engine.register_fn("parseInt", |n: i64| n as f64);
    engine.register_fn("parseFloat", move |s: ImmutableString| or_none(js_parse_float(&s)));
    engine.register_fn("parseFloat", |n: f64| n);
    engine.register_fn("parseFloat", |n: i64| n as f64);
    engine.register_fn("String", |v: Dynamic| from_dynamic(&v).to_display());
    engine.register_fn("isNaN", |n: f64| n.is_nan());
    engine.register_fn("isNaN", |_: i64| false);

    // `Result<T, E>`: `{ ok: true, value }` or `{ ok: false, error }`, a plain
    // map, so reading it is the narrowing that already exists. `unwrap` gives
    // the value or throws the error. See `docs/11-next.md`, "Result".
    engine.register_fn("Ok", |value: Dynamic| {
        let mut m = rhai::Map::new();
        m.insert("ok".into(), Dynamic::from(true));
        m.insert("value".into(), value);
        m
    });
    engine.register_fn("Err", |error: Dynamic| {
        let mut m = rhai::Map::new();
        m.insert("ok".into(), Dynamic::from(false));
        m.insert("error".into(), error);
        m
    });
    engine.register_fn("unwrap", |r: &mut rhai::Map| -> Result<Dynamic, Box<EvalAltResult>> {
        match r.get("ok").and_then(|ok| ok.as_bool().ok()) {
            Some(true) => Ok(r.get("value").cloned().unwrap_or(Dynamic::UNIT)),
            Some(false) => {
                let error = r.get("error").map(|e| from_dynamic(e).to_display()).unwrap_or_default();
                Err(format!("unwrap() on an error: {error}").into())
            }
            None => Err("unwrap() is for a `Result`, made by `Ok(…)` or `Err(…)`".into()),
        }
    });

    // Between `int` and `float`. The checker keeps them apart; the numbers
    // themselves are all f64 until Rux's own interpreter (step 5 of
    // `docs/11-next.md`), so these answer with whole f64s where the type says
    // `int`. rhai already has `floor` and `round`, and calls `ceil` `ceiling`.
    engine.register_fn("toFloat", |n: f64| n);
    engine.register_fn("toFloat", |n: i64| n as f64);
    engine.register_fn("trunc", |n: f64| n.trunc());
    engine.register_fn("ceil", |n: f64| n.ceil());
    for name in ["trunc", "ceil", "floor", "round"] {
        engine.register_fn(name, |n: i64| n as f64);
    }
    engine.register_fn("intDiv", |a: Dynamic, b: Dynamic| -> Result<f64, Box<EvalAltResult>> {
        let (a, b) = (num(&a).trunc(), num(&b).trunc());
        if b == 0.0 {
            return Err("intDiv(a, 0): an `int` cannot be divided by zero".into());
        }
        Ok((a / b).trunc())
    });

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
        start_interval(num(&ms), body.to_string())
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

fn num(d: &Dynamic) -> f64 {
    if let Ok(i) = d.as_int() {
        return i as f64;
    }
    d.as_float().unwrap_or(0.0)
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
