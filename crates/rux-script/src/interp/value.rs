//! The interpreter's values.
//!
//! Arrays, maps and records are values, as the language says ("Values move by
//! copy"): each is shared behind an [`Rc`] and copied the first time it is
//! written while shared, so `let b = a` costs nothing until `b` changes.
//!
//! A map keeps its keys sorted, which is the order the fork gave them and the
//! order `keys(m)`, a `:style` map and a displayed map have always had.
//!
//! A record keeps its fields in slots, in the order its type declares them,
//! with a [`Shape`] saying which name is in which slot (step 10, track (c)).
//! That is the order it is shown and walked in. An optional field never
//! given holds `none`, and is left out where the record is shown or walked,
//! as JavaScript's `JSON.stringify` leaves out `undefined`.

use std::collections::BTreeMap;
use std::rc::Rc;

use rux_ir::ir;
pub use rux_ir::shape::Shape;
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
    /// A record: its shape and its fields' values, in slots.
    Rec(Rc<Record>),
    /// `a..b` or `a..=b` kept as a value, end exclusive.
    Range(i64, i64),
    Fn(Rc<Closure>),
    Element(Rc<ElementHandle>),
    /// A native resource, held for Rust: step 8 of `docs/11-next.md`.
    Native(Rc<rux_native::Handle>),
}

/// A record's shape and the values in its slots, behind one `Rc`, so the
/// shape's pointer sits beside the slots' own.
#[derive(Clone, Debug)]
pub struct Record {
    pub shape: Rc<Shape>,
    pub vals: Box<[V]>,
}

impl Record {
    /// The value of field `name`: `None` when the shape has no such field.
    #[inline]
    pub fn get(&self, name: &str) -> Option<&V> {
        self.shape.slot(name).map(|i| &self.vals[i])
    }

    /// Whether slot `i` is shown and walked: every field but an optional
    /// one holding `none`.
    #[inline]
    pub fn shows(&self, i: usize) -> bool {
        !(matches!(self.vals[i], V::None) && self.shape.optional(i))
    }

    /// The fields shown and walked, in slot order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &V)> {
        self.shape
            .names()
            .iter()
            .zip(self.vals.iter())
            .enumerate()
            .filter(|(i, _)| self.shows(*i))
            .map(|(_, (k, v))| (&**k, v))
    }

    /// Whether `name` is one of the fields shown: what `in` asks.
    pub fn has(&self, name: &str) -> bool {
        self.shape.slot(name).is_some_and(|i| self.shows(i))
    }
}

/// A closure value: its code and what it captured, by value.
#[derive(Debug)]
pub struct Closure {
    pub code: Rc<ir::Closure>,
    pub captured: Vec<V>,
    /// Its body compiled, when a build compiled it (step 9 of
    /// `docs/11-next.md`): found once, when the closure is made, so a call
    /// looks nothing up.
    pub compiled: Option<Compiled>,
}

/// A compiled closure body: how many locals its frame has, whether it runs
/// in one, and the body.
pub type Compiled = (u32, bool, crate::aot::ClosureBody);

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

    /// [`V::display`] written onto the end of `out`, for a backtick string,
    /// which then makes no text of its own for each part.
    pub fn display_into(&self, out: &mut String) {
        use std::fmt::Write as _;
        match self {
            V::Str(s) => out.push_str(s),
            V::Int(i) => {
                let _ = write!(out, "{i}");
            }
            V::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            V::None => {}
            other => out.push_str(&other.display()),
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
            // A record is named as a map was, so a failure's words stay.
            V::Map(_) | V::Rec(_) => "map",
            V::Range(..) => "range",
            V::Fn(_) => "Fn",
            V::Element(_) => "Element",
            V::Native(h) => h.type_name(),
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
            V::Rec(r) => Value::Map(r.entries().map(|(k, v)| (k.to_string(), v.to_value())).collect()),
            V::Range(a, b) => Value::List((*a..*b).map(|i| Value::Number(i as f64)).collect()),
            V::Fn(_) => Value::Text("Fn(<closure>)".to_string()),
            V::Element(e) => Value::Text(format!("Element({})", e.facts.tag)),
            // The runtime's values are text and data: a resource kept in
            // one (a component instance's state) is shown by its type.
            V::Native(h) => Value::Text(format!("{}(..)", h.type_name())),
        }
    }

    /// A value from the runtime. Its numbers are all `f64`, and come in as
    /// `float`s, which is what the fork made of them too. Its maps come in as
    /// open records, keeping their order: a record the runtime was handed
    /// comes back in its type's order, and a map in the sorted order it went
    /// out in (a new key makes one a map again).
    pub fn from_value(v: &Value) -> V {
        match v {
            Value::Null => V::None,
            Value::Bool(b) => V::Bool(*b),
            Value::Number(n) => V::Float(*n),
            Value::Text(s) => V::str(s.as_str()),
            Value::List(items) => V::array(items.iter().map(V::from_value).collect()),
            Value::Map(entries) => {
                let shape = Shape::open(entries.iter().map(|(k, _)| k.as_str()));
                // A name twice: a map, the later entry replacing the earlier.
                if entries.iter().enumerate().any(|(i, (k, _))| shape.slot(k) != Some(i)) {
                    return V::Map(Rc::new(entries.iter().map(|(k, v)| (k.clone(), V::from_value(v))).collect()));
                }
                let vals = entries.iter().map(|(_, v)| V::from_value(v)).collect();
                V::Rec(Rc::new(Record { shape, vals }))
            }
        }
    }

    /// Field `name` of a map or a record, as a read finds it: `None` where
    /// there is none. An optional field never given is there, holding `none`.
    #[inline]
    pub fn field_of(&self, name: &str) -> Option<&V> {
        match self {
            V::Rec(r) => r.get(name),
            V::Map(m) => m.get(name),
            _ => None,
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
            // By field name, whatever the order: two records with the same
            // fields written in different orders are equal, and a record is
            // equal to a map of the same entries.
            (V::Rec(a), V::Rec(b)) if Rc::ptr_eq(&a.shape, &b.shape) => Rc::ptr_eq(a, b) || a.vals == b.vals,
            (V::Rec(a), V::Rec(b)) => {
                a.entries().count() == b.entries().count() && a.entries().all(|(k, v)| b.has(k) && b.get(k) == Some(v))
            }
            (V::Rec(r), V::Map(m)) | (V::Map(m), V::Rec(r)) => {
                r.entries().count() == m.len() && r.entries().all(|(k, v)| m.get(k) == Some(v))
            }
            (V::Range(a, b), V::Range(c, d)) => a == c && b == d,
            (V::Fn(a), V::Fn(b)) => Rc::ptr_eq(a, b),
            (V::Element(a), V::Element(b)) => a.facts.path == b.facts.path,
            (V::Native(a), V::Native(b)) => a.same(b),
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
            V::Rec(r) => Some(r.entries().all(|(k, v)| f(k, v))),
            _ => None,
        }
    }
    fn resource_name(&self) -> Option<&str> {
        match self {
            V::Native(h) => Some(h.type_name()),
            _ => None,
        }
    }
    fn with_field(&self, name: &str, f: &mut dyn FnMut(Option<&Self>) -> bool) -> Option<bool> {
        match self {
            V::Map(m) => Some(f(m.get(name))),
            // An optional field holding `none` is as good as absent.
            V::Rec(r) => Some(f(r.shape.slot(name).filter(|i| r.shows(*i)).map(|i| &r.vals[i]))),
            _ => None,
        }
    }
}
