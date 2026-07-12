//! Rux reactivity core — milestone M5.
//!
//! Signals are named reactive values. In M5 they live in a flat `Signals` map
//! keyed by name, and `{{ }}` bindings read them by name at tree-build time; a
//! `revision` counter bumps on every mutation so the shell knows to rebuild.
//!
//! This is the *storage and value* half of reactivity. The full model from
//! `docs/04-architecture.md` — per-binding subscriptions and no whole-tree
//! rebuild — is a later refinement, and the `let x = signal(..)` reader here is
//! a stand-in for the `rhai` script tier that arrives in M8.

use std::collections::HashMap;

/// A dynamically-typed signal value. Untyped so template interpolation and the
/// future script tier can share one representation.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Text(String),
    Bool(bool),
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
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }
}

/// A flat table of named signals plus a revision counter.
#[derive(Clone, Debug, Default)]
pub struct Signals {
    map: HashMap<String, Value>,
    revision: u64,
}

impl Signals {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed signals by scanning a script for `let <name> = signal(<literal>);`.
    /// A deliberately tiny reader — replaced by the real script engine in M8.
    pub fn from_script(script: &str) -> Self {
        let mut signals = Signals::new();

        // Strip `//` line comments, then treat the script as `;`-separated
        // statements (a line may hold more than one).
        let mut code = String::new();
        for line in script.lines() {
            let line = line.find("//").map(|i| &line[..i]).unwrap_or(line);
            code.push_str(line);
            code.push('\n');
        }

        for stmt in code.split(';') {
            let stmt = stmt.trim();
            let Some(rest) = stmt.strip_prefix("let ") else {
                continue;
            };
            let Some((name, expr)) = rest.split_once('=') else {
                continue;
            };
            let expr = expr.trim();
            let Some(arg) = expr.strip_prefix("signal(").and_then(|s| s.strip_suffix(')')) else {
                continue;
            };
            signals
                .map
                .insert(name.trim().to_string(), parse_literal(arg.trim()));
        }
        signals
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.map.get(name)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn set(&mut self, name: &str, value: Value) {
        self.map.insert(name.to_string(), value);
        self.revision += 1;
    }

    /// Apply `f` to a numeric signal in place. No-op if it is absent or non-numeric.
    pub fn update_number(&mut self, name: &str, f: impl FnOnce(f64) -> f64) {
        if let Some(Value::Number(n)) = self.map.get(name) {
            let next = f(*n);
            self.map.insert(name.to_string(), Value::Number(next));
            self.revision += 1;
        }
    }
}

/// Parse a signal's initial literal: string, bool, number, else text.
fn parse_literal(arg: &str) -> Value {
    if let Some(inner) = arg.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return Value::Text(inner.to_string());
    }
    match arg {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    if let Ok(n) = arg.parse::<f64>() {
        return Value::Number(n);
    }
    Value::Text(arg.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_signal_declarations() {
        let s = Signals::from_script(r#"let level = signal(82); let name = signal("Cam");"#);
        assert_eq!(s.get("level"), Some(&Value::Number(82.0)));
        assert_eq!(s.get("name"), Some(&Value::Text("Cam".into())));
    }

    #[test]
    fn update_bumps_revision() {
        let mut s = Signals::from_script("let level = signal(10);");
        let r0 = s.revision();
        s.update_number("level", |v| v - 1.0);
        assert_eq!(s.get("level"), Some(&Value::Number(9.0)));
        assert!(s.revision() > r0);
    }
}
