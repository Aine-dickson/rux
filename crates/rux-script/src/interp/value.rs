//! The interpreter's values.
//!
//! Arrays, maps and records are values, as the language says ("Values move by
//! copy"): each is shared behind an [`Rc`] and copied the first time it is
//! written while shared, so `let b = a` costs nothing until `b` changes.
//!
//! A map keeps its keys sorted, which is the order the fork gave them and the
//! order `keys(m)`, a `:style` map and a displayed map have always had.

use std::collections::BTreeMap;
use std::rc::Rc;

use rux_ir::ir;
use rux_reactive::Value;

use crate::validate::Checkable;
use crate::ElementHandle;

#[derive(Clone, Debug)]
pub enum V {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    Array(Rc<Vec<V>>),
    Map(Rc<BTreeMap<String, V>>),
    /// `a..b` or `a..=b` kept as a value, end exclusive.
    Range(i64, i64),
    Fn(Rc<Closure>),
    Element(Rc<ElementHandle>),
}

/// A closure value: its code and what it captured, by value.
#[derive(Debug)]
pub struct Closure {
    pub code: Rc<ir::Closure>,
    pub captured: Vec<V>,
}

impl V {
    pub fn str(s: impl Into<Rc<str>>) -> V {
        V::Str(s.into())
    }

    pub fn array(items: Vec<V>) -> V {
        V::Array(Rc::new(items))
    }

    /// JavaScript's truthiness, the fork's divergence 5: `false`, zero, `NaN`,
    /// `""` and `none` are false; everything else, an empty array and an empty
    /// map included, is true.
    pub fn truthy(&self) -> bool {
        match self {
            V::None => false,
            V::Bool(b) => *b,
            V::Int(i) => *i != 0,
            V::Float(f) => *f != 0.0 && !f.is_nan(),
            V::Str(s) => !s.is_empty(),
            _ => true,
        }
    }

    /// Either number as an `f64`.
    pub fn number(&self) -> Option<f64> {
        match self {
            V::Int(i) => Some(*i as f64),
            V::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// A whole number, from an `int` or a `float` with nothing after the
    /// point (the fork's divergence 6): what an index, a count and a range
    /// take.
    pub fn whole(&self) -> Option<i64> {
        match self {
            V::Int(i) => Some(*i),
            V::Float(f) if f.is_finite() && f.fract() == 0.0 && f.abs() < 9.007_199_254_740_992e15 => Some(*f as i64),
            _ => None,
        }
    }

    /// How the value is shown as text: in `{{ }}`, in `+` with a string, in
    /// a backtick string and by `print`. One rule, [`Value::to_display`]'s.
    pub fn display(&self) -> String {
        match self {
            V::Str(s) => s.to_string(),
            V::Int(i) => i.to_string(),
            V::Float(f) => Value::Number(*f).to_display(),
            V::Bool(b) => b.to_string(),
            V::None => String::new(),
            _ => self.to_value().to_display(),
        }
    }

    /// The name `type_of` gives, the fork's spelling.
    pub fn type_name(&self) -> &'static str {
        match self {
            V::None => "()",
            V::Bool(_) => "bool",
            V::Int(_) => "i64",
            V::Float(_) => "f64",
            V::Str(_) => "string",
            V::Array(_) => "array",
            V::Map(_) => "map",
            V::Range(..) => "range",
            V::Fn(_) => "Fn",
            V::Element(_) => "Element",
        }
    }

    /// The value as the runtime holds it. Both numbers become its one number.
    pub fn to_value(&self) -> Value {
        match self {
            V::None => Value::Null,
            V::Bool(b) => Value::Bool(*b),
            V::Int(i) => Value::Number(*i as f64),
            V::Float(f) => Value::Number(*f),
            V::Str(s) => Value::Text(s.to_string()),
            V::Array(items) => Value::List(items.iter().map(V::to_value).collect()),
            V::Map(m) => Value::Map(m.iter().map(|(k, v)| (k.clone(), v.to_value())).collect()),
            V::Range(a, b) => Value::List((*a..*b).map(|i| Value::Number(i as f64)).collect()),
            V::Fn(_) => Value::Text("Fn(<closure>)".to_string()),
            V::Element(e) => Value::Text(format!("Element({})", e.facts.tag)),
        }
    }

    /// A value from the runtime. Its numbers are all `f64`, and come in as
    /// `float`s, which is what the fork made of them too.
    pub fn from_value(v: &Value) -> V {
        match v {
            Value::Null => V::None,
            Value::Bool(b) => V::Bool(*b),
            Value::Number(n) => V::Float(*n),
            Value::Text(s) => V::str(s.as_str()),
            Value::List(items) => V::array(items.iter().map(V::from_value).collect()),
            Value::Map(entries) => V::Map(Rc::new(entries.iter().map(|(k, v)| (k.clone(), V::from_value(v))).collect())),
        }
    }
}

/// `==`: the same value, compared through. The two numbers compare as
/// numbers, as they did in the fork; values of different kinds are unequal.
impl PartialEq for V {
    fn eq(&self, other: &V) -> bool {
        match (self, other) {
            (V::None, V::None) => true,
            (V::Bool(a), V::Bool(b)) => a == b,
            (V::Int(a), V::Int(b)) => a == b,
            (V::Int(_) | V::Float(_), V::Int(_) | V::Float(_)) => self.number() == other.number(),
            (V::Str(a), V::Str(b)) => a == b,
            (V::Array(a), V::Array(b)) => Rc::ptr_eq(a, b) || a == b,
            (V::Map(a), V::Map(b)) => Rc::ptr_eq(a, b) || a == b,
            (V::Range(a, b), V::Range(c, d)) => a == c && b == d,
            (V::Fn(a), V::Fn(b)) => Rc::ptr_eq(a, b),
            (V::Element(a), V::Element(b)) => a.facts.path == b.facts.path,
            _ => false,
        }
    }
}

impl Checkable for V {
    fn is_null(&self) -> bool {
        matches!(self, V::None)
    }
    fn as_number(&self) -> Option<f64> {
        self.number()
    }
    fn as_bool(&self) -> Option<bool> {
        match self {
            V::Bool(b) => Some(*b),
            _ => None,
        }
    }
    fn is_function(&self) -> bool {
        matches!(self, V::Fn(_))
    }
    fn with_text(&self, f: &mut dyn FnMut(&str) -> bool) -> Option<bool> {
        match self {
            V::Str(s) => Some(f(s)),
            _ => None,
        }
    }
    fn all_items(&self, f: &mut dyn FnMut(&Self) -> bool) -> Option<bool> {
        match self {
            V::Array(items) => Some(items.iter().all(f)),
            _ => None,
        }
    }
    fn all_entries(&self, f: &mut dyn FnMut(&str, &Self) -> bool) -> Option<bool> {
        match self {
            V::Map(m) => Some(m.iter().all(|(k, v)| f(k, v))),
            _ => None,
        }
    }
    fn with_field(&self, name: &str, f: &mut dyn FnMut(Option<&Self>) -> bool) -> Option<bool> {
        match self {
            V::Map(m) => Some(f(m.get(name))),
            _ => None,
        }
    }
}
