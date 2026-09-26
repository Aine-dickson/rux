//! Whether a value fits a type, at run time.
//!
//! Types are erased, so this is the only place one meets a value. Two things
//! ask: a component tag, whose props are checked against what the component
//! declared, and `x is T` in script. See `docs/10-types.md`, "Props" and
//! "Boundaries". The walk never raises: a value that does not fit is `false`.
//!
//! A prop arrives as a [`Value`] and `is` sees a rhai [`Dynamic`], so the walk
//! is written once over [`Checkable`] and both implement it.

use std::cell::RefCell;
use std::collections::HashMap;

use rhai::Dynamic;
use rux_reactive::Value;

use crate::types::{parse_decl, parse_type, Type};

/// A declared type as the walk resolves it: its type parameters, none for
/// one that is not generic, and its body, which reads them as
/// [`Type::Param`]s. See [`crate::types::parse_decl`].
pub type Decl = (Vec<String>, Type);

/// How deep a named type may refer to another before the walk gives up and
/// answers `false`. Only a type that names itself with nothing in between
/// (`type A = B; type B = A`) gets there; a real value runs out first.
const MAX_NAMED_DEPTH: usize = 64;

/// What the walk needs to know about a value.
pub trait Checkable {
    fn is_null(&self) -> bool;
    fn as_number(&self) -> Option<f64>;
    fn as_bool(&self) -> Option<bool>;
    fn is_function(&self) -> bool;
    /// `f` on its text, if it is text.
    fn with_text(&self, f: &mut dyn FnMut(&str) -> bool) -> Option<bool>;
    /// Whether `f` holds for every element, if it is a list.
    fn all_items(&self, f: &mut dyn FnMut(&Self) -> bool) -> Option<bool>;
    /// Whether `f` holds for every entry, if it is a map.
    fn all_entries(&self, f: &mut dyn FnMut(&str, &Self) -> bool) -> Option<bool>;
    /// `f` on the entry `name`, absent being `None`, if it is a map.
    fn with_field(&self, name: &str, f: &mut dyn FnMut(Option<&Self>) -> bool) -> Option<bool>;
}

/// Whether `value` fits `ty`. `named` says what a declared type stands for;
/// a name it does not know fits nothing, since a check that passes by not
/// knowing is the silent failure this project keeps closing off.
pub fn fits<V: Checkable>(value: &V, ty: &Type, named: &dyn Fn(&str) -> Option<Decl>) -> bool {
    fits_at(value, ty, named, 0)
}

fn fits_at<V: Checkable>(value: &V, ty: &Type, named: &dyn Fn(&str) -> Option<Decl>, depth: usize) -> bool {
    match ty {
        Type::Any => true,
        Type::Null => value.is_null(),
        Type::Float => value.as_number().is_some(),
        // Whole, as `.length` and an index are. A script's numbers are floats
        // once they have been through a signal, so `3.0` is an `int` too.
        Type::Int => value.as_number().is_some_and(|n| n.is_finite() && n.fract() == 0.0),
        Type::Bool => value.as_bool().is_some(),
        Type::String => value.with_text(&mut |_| true).unwrap_or(false),
        Type::Literal(want) => value.with_text(&mut |s| s == want).unwrap_or(false),
        Type::Array(item) => value.all_items(&mut |v| fits_at(v, item, named, depth)).unwrap_or(false),
        Type::Dict(item) => value.all_entries(&mut |_, v| fits_at(v, item, named, depth)).unwrap_or(false),
        // Every declared field present, unless optional, and of its type.
        // Fields the type does not mention are allowed: `is` asks whether the
        // value can be used as a `T`, and an extra field does not stop that.
        // An optional field may be null, since null is how a script says absent.
        Type::Record(fields) => fields.iter().all(|field| {
            value
                .with_field(&field.name, &mut |v| match v {
                    None => field.optional,
                    Some(v) if field.optional && v.is_null() => true,
                    Some(v) => fits_at(v, &field.ty, named, depth),
                })
                .unwrap_or(false)
        }),
        Type::Union(members) => members.iter().any(|m| fits_at(value, m, named, depth)),
        // The parameters are not checked: a function value carries no types.
        Type::Function(..) => value.is_function(),
        Type::Named(_) | Type::Generic(..) => {
            depth < MAX_NAMED_DEPTH
                && resolved(ty, named).is_some_and(|resolved| fits_at(value, &resolved, named, depth + 1))
        }
        // Only a checker error reaches here: at run time no `T` is known.
        Type::Param(_) => false,
    }
}

/// What a named type stands for, with a generic one's arguments in place. A
/// generic type named without its arguments, which the checker reports,
/// takes `any` for each.
fn resolved(ty: &Type, named: &dyn Fn(&str) -> Option<Decl>) -> Option<Type> {
    let (name, args): (&str, &[Type]) = match ty {
        Type::Named(name) => (name, &[]),
        Type::Generic(name, args) => (name, args),
        _ => return None,
    };
    let (params, body) = named(name)?;
    if params.is_empty() {
        return args.is_empty().then_some(body);
    }
    if !args.is_empty() && args.len() != params.len() {
        return None;
    }
    let given = params
        .into_iter()
        .enumerate()
        .map(|(i, p)| (p, args.get(i).cloned().unwrap_or(Type::Any)))
        .collect();
    Some(body.substitute(&given))
}

/// `text` as a value of `ty`, for a route segment, which is always text: `"42"`
/// is `42` for a `prop id: int`. `None` when it cannot be one.
///
/// Text that already fits stays text, so `string` and a literal union keep it
/// as it is; otherwise each member of a union is tried in the order written.
pub fn from_text(text: &str, ty: &Type, named: &dyn Fn(&str) -> Option<Decl>) -> Option<Value> {
    from_text_at(text, ty, named, 0)
}

fn from_text_at(text: &str, ty: &Type, named: &dyn Fn(&str) -> Option<Decl>, depth: usize) -> Option<Value> {
    let as_text = Value::Text(text.to_string());
    if fits(&as_text, ty, named) {
        return Some(as_text);
    }
    match ty {
        Type::Int => text.trim().parse::<i64>().ok().map(|n| Value::Number(n as f64)),
        Type::Float => text.trim().parse::<f64>().ok().filter(|n| n.is_finite()).map(Value::Number),
        Type::Bool => match text.trim() {
            "true" => Some(Value::Bool(true)),
            "false" => Some(Value::Bool(false)),
            _ => None,
        },
        Type::Union(members) => members.iter().find_map(|m| from_text_at(text, m, named, depth)),
        Type::Named(_) | Type::Generic(..) if depth < MAX_NAMED_DEPTH => {
            resolved(ty, named).and_then(|resolved| from_text_at(text, &resolved, named, depth + 1))
        }
        _ => None,
    }
}

/// Some value of `ty`, for `rux check`, which visits a route pattern with a
/// placeholder segment and needs its view built with a prop that fits. `None`
/// for a type with no simple member.
pub fn sample(ty: &Type, named: &dyn Fn(&str) -> Option<Decl>) -> Option<Value> {
    sample_at(ty, named, 0)
}

fn sample_at(ty: &Type, named: &dyn Fn(&str) -> Option<Decl>, depth: usize) -> Option<Value> {
    match ty {
        Type::Int | Type::Float => Some(Value::Number(0.0)),
        Type::Bool => Some(Value::Bool(false)),
        Type::String | Type::Any => Some(Value::Text(String::new())),
        Type::Literal(s) => Some(Value::Text(s.clone())),
        Type::Union(members) => members.iter().find_map(|m| sample_at(m, named, depth)),
        Type::Named(_) | Type::Generic(..) if depth < MAX_NAMED_DEPTH => {
            resolved(ty, named).and_then(|resolved| sample_at(&resolved, named, depth + 1))
        }
        _ => None,
    }
}

impl Checkable for Value {
    fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
    fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }
    fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    fn is_function(&self) -> bool {
        false
    }
    fn with_text(&self, f: &mut dyn FnMut(&str) -> bool) -> Option<bool> {
        match self {
            Value::Text(s) => Some(f(s)),
            _ => None,
        }
    }
    fn all_items(&self, f: &mut dyn FnMut(&Self) -> bool) -> Option<bool> {
        match self {
            Value::List(items) => Some(items.iter().all(f)),
            _ => None,
        }
    }
    fn all_entries(&self, f: &mut dyn FnMut(&str, &Self) -> bool) -> Option<bool> {
        match self {
            Value::Map(entries) => Some(entries.iter().all(|(k, v)| f(k, v))),
            _ => None,
        }
    }
    fn with_field(&self, name: &str, f: &mut dyn FnMut(Option<&Self>) -> bool) -> Option<bool> {
        match self {
            Value::Map(entries) => Some(f(entries.iter().find(|(k, _)| k == name).map(|(_, v)| v))),
            _ => None,
        }
    }
}

impl Checkable for Dynamic {
    fn is_null(&self) -> bool {
        self.is_unit()
    }
    fn as_number(&self) -> Option<f64> {
        self.as_int().map(|i| i as f64).or_else(|_| self.as_float()).ok()
    }
    fn as_bool(&self) -> Option<bool> {
        Dynamic::as_bool(self).ok()
    }
    fn is_function(&self) -> bool {
        self.is_fnptr()
    }
    fn with_text(&self, f: &mut dyn FnMut(&str) -> bool) -> Option<bool> {
        if let Ok(s) = self.as_immutable_string_ref() {
            return Some(f(&s));
        }
        self.as_char().ok().map(|c| f(c.encode_utf8(&mut [0; 4])))
    }
    fn all_items(&self, f: &mut dyn FnMut(&Self) -> bool) -> Option<bool> {
        let items = self.as_array_ref().ok()?;
        Some(items.iter().all(f))
    }
    fn all_entries(&self, f: &mut dyn FnMut(&str, &Self) -> bool) -> Option<bool> {
        let entries = self.as_map_ref().ok()?;
        Some(entries.iter().all(|(k, v)| f(k, v)))
    }
    fn with_field(&self, name: &str, f: &mut dyn FnMut(Option<&Self>) -> bool) -> Option<bool> {
        let entries = self.as_map_ref().ok()?;
        Some(f(entries.get(name)))
    }
}

thread_local! {
    /// The declared types `x is T` resolves a name against, as text: the
    /// script's own `type`s, and whatever the runtime adds with
    /// [`know_types`]. Parsed on first use, then kept.
    static KNOWN: RefCell<HashMap<String, (String, Option<Decl>)>> = RefCell::new(HashMap::new());
    /// Each `is` right-hand side, parsed once.
    static WRITTEN: RefCell<HashMap<String, Option<Type>>> = RefCell::new(HashMap::new());
}

/// Replace the declared types `is` resolves names against.
pub(crate) fn reset_types(types: impl IntoIterator<Item = (String, String)>) {
    KNOWN.with(|k| *k.borrow_mut() = types.into_iter().map(|(n, t)| (n, (t, None))).collect());
    WRITTEN.with(|w| w.borrow_mut().clear());
}

/// Add declared types for `is` to resolve names against: the types a file
/// imports with `use`, and the types its components declare. A name already
/// known keeps its first meaning.
pub fn know_types(types: impl IntoIterator<Item = (String, String)>) {
    KNOWN.with(|k| {
        let mut k = k.borrow_mut();
        for (name, text) in types {
            k.entry(name).or_insert((text, None));
        }
    });
}

/// What a declared type stands for, as `is` sees it.
fn known(name: &str) -> Option<Decl> {
    KNOWN.with(|k| {
        let mut k = k.borrow_mut();
        let (text, parsed) = k.get_mut(name)?;
        if parsed.is_none() {
            *parsed = parse_decl(text).ok();
        }
        parsed.clone()
    })
}

/// `value is written`, the function `x is T` compiles to.
pub(crate) fn is(value: &Dynamic, written: &str) -> bool {
    let ty = WRITTEN.with(|w| w.borrow_mut().entry(written.to_string()).or_insert_with(|| parse_type(written).ok()).clone());
    let value = value.flatten_clone();
    ty.is_some_and(|ty| fits(&value, &ty, &known))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Builder;

    fn answers(script: &str, expr: &str) -> Option<Value> {
        let mut engine = Builder::new().build(script).expect("builds");
        engine.eval_value(expr, &[])
    }

    #[test]
    fn is_walks_a_record_and_narrows_nothing_at_run_time() {
        let script = r#"
            type Task = { id: int, title: string, note?: string, tags: string[] };
            let good = { id: 1, title: "a", tags: ["x"] };
            let noted = { id: 2, title: "b", note: "n", tags: [], extra: true };
            let bad_id = { id: 1.5, title: "a", tags: [] };
            let missing = { id: 1, tags: [] };
            let bad_tag = { id: 1, title: "a", tags: [3] };
        "#;
        for (expr, want) in [
            ("good is Task", true),
            ("noted is Task", true),
            ("bad_id is Task", false),
            ("missing is Task", false),
            ("bad_tag is Task", false),
            ("42 is Task", false),
            ("null is Task?", true),
            ("good is Task && 1 < 2", true),
            ("!(good is Task)", false),
        ] {
            assert_eq!(answers(script, expr), Some(Value::Bool(want)), "{expr}");
        }
    }

    #[test]
    fn is_tries_every_member_of_a_union() {
        let script = r#"
            type Filter = "all" | "open" | "done";
            type Shape = { kind: "circle", r: float } | { kind: "square", side: float };
        "#;
        for (expr, want) in [
            (r#""open" is Filter"#, true),
            (r#""closed" is Filter"#, false),
            (r#"{ kind: "square", side: 2 } is Shape"#, true),
            (r#"{ kind: "square", r: 2 } is Shape"#, false),
            ("3 is int", true),
            ("3.5 is int", false),
            ("3.5 is float", true),
            ("3 is float", true),
            (r#""3" is float"#, false),
            ("[1, 2] is float[]", true),
            (r#"{ a: 1, b: 2 } is { [string]: float }"#, true),
            (r#"{ a: 1, b: "x" } is { [string]: float }"#, false),
            ("((x) => x) is (float) => float", true),
            ("true is bool", true),
        ] {
            assert_eq!(answers(script, expr), Some(Value::Bool(want)), "{expr}");
        }
    }

    #[test]
    fn is_fills_in_a_generic_type() {
        let script = "type Page<T> = { items: T[], next: string? };";
        for (expr, want) in [
            ("{ items: [1, 2], next: none } is Page<int>", true),
            ("{ items: [\"a\"], next: none } is Page<int>", false),
            ("{ items: [\"a\"], next: \"p2\" } is Page<string>", true),
            ("[1] is Array<int>", true),
            ("none is Option<int>", true),
            ("{ a: true } is Map<string, bool>", true),
            ("(!({ items: [] } is Page<int>))", true),
        ] {
            assert_eq!(answers(script, expr), Some(Value::Bool(want)), "{expr}");
        }
    }

    #[test]
    fn a_name_nobody_declared_fits_nothing() {
        assert_eq!(answers("", "1 is Nowhere"), Some(Value::Bool(false)));
        know_types([("Nowhere".to_string(), "float".to_string())]);
        assert_eq!(answers("", "1 is Nowhere"), Some(Value::Bool(false)), "a new engine starts over");
        let mut engine = Builder::new().build("").unwrap();
        know_types([("Nowhere".to_string(), "float".to_string())]);
        assert_eq!(engine.eval_value("1 is Nowhere", &[]), Some(Value::Bool(true)));
    }

    #[test]
    fn is_binds_as_a_comparison_does() {
        // `1 + 2.5` is one operand: arithmetic binds tighter than `is`.
        assert_eq!(answers("", "1 + 2.5 is int"), Some(Value::Bool(false)));
        assert_eq!(answers("", "1 + 2 is int == true"), Some(Value::Bool(true)));
    }

    #[test]
    fn a_route_segment_becomes_what_the_prop_declares() {
        let none = |_: &str| None;
        let parse = |t: &str| parse_type(t).unwrap();
        assert_eq!(from_text("42", &parse("int"), &none), Some(Value::Number(42.0)));
        assert_eq!(from_text("4.5", &parse("int"), &none), None);
        assert_eq!(from_text("4.5", &parse("float"), &none), Some(Value::Number(4.5)));
        assert_eq!(from_text("abc", &parse("float"), &none), None);
        assert_eq!(from_text("true", &parse("bool"), &none), Some(Value::Bool(true)));
        assert_eq!(from_text("open", &parse(r#""open" | "done""#), &none), Some(Value::Text("open".into())));
        assert_eq!(from_text("nope", &parse(r#""open" | "done""#), &none), None);
        assert_eq!(from_text("42", &parse("string"), &none), Some(Value::Text("42".into())));
        assert_eq!(from_text("42", &parse("int | string"), &none), Some(Value::Text("42".into())));
    }

    #[test]
    fn a_prop_value_is_checked_as_a_value() {
        let none = |_: &str| None;
        let parse = |t: &str| parse_type(t).unwrap();
        let task = Value::Map(vec![("id".into(), Value::Number(1.0)), ("note".into(), Value::Null)]);
        assert!(fits(&task, &parse("{ id: int, note?: string }"), &none));
        assert!(fits(&task, &parse("{ id: int, note: string? }"), &none));
        assert!(!fits(&task, &parse("{ id: int, note: string }"), &none));
        assert!(!fits(&task, &parse("{ id: string }"), &none));
        assert!(fits(&Value::Null, &parse("{ id: int }?"), &none));
        // The empty text is text, not null: it was, until `Value::Null`.
        assert!(!fits(&Value::Text(String::new()), &parse("{ id: int }?"), &none));
    }
}
