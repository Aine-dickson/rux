//! Rux's shared value type.
//!
//! This crate began (M5) as the reactivity core: a flat `Signals` table plus a
//! little expression evaluator. M8 replaced both with the `rhai` engine in
//! `rux-script`, which owns state and evaluation now. What survives is `Value`
//! — the untyped representation that `rux-script` and `rux-style` pass between
//! each other for bindings, `r-for` locals, and props.
//!
//! The per-binding subscription model in `docs/04-architecture.md` is still
//! unbuilt: a signal change rebuilds the whole tree.

/// A dynamically-typed signal value. Untyped so template interpolation and the
/// future script tier can share one representation.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Text(String),
    Bool(bool),
    List(Vec<Value>),
}

impl Value {
    /// How the value appears when interpolated into text.
    pub fn to_display(&self) -> String {
        match self {
            Value::Number(n) => {
                if n.fract() == 0.0 {
                    format!("{}", *n as i64)
                } else {
                    format!("{n}")
                }
            }
            Value::Text(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::List(items) => items
                .iter()
                .map(Value::to_display)
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(items) => Some(items),
            _ => None,
        }
    }

    /// Truthiness for conditions: non-zero / non-empty / true.
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Number(n) => *n != 0.0,
            Value::Text(s) => !s.is_empty(),
            Value::Bool(b) => *b,
            Value::List(items) => !items.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_and_coerces() {
        assert_eq!(Value::Number(82.0).to_display(), "82"); // whole floats lose the .0
        assert_eq!(Value::Number(8.2).to_display(), "8.2");
        assert_eq!(
            Value::List(vec![Value::Text("a".into()), Value::Number(2.0)]).to_display(),
            "a, 2"
        );

        assert!(Value::Number(1.0).is_truthy());
        assert!(!Value::Number(0.0).is_truthy());
        assert!(!Value::Text(String::new()).is_truthy());
        assert!(!Value::List(Vec::new()).is_truthy());
    }
}
