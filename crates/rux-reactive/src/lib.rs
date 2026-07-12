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

    /// Read a signal as a number, treating absent/non-numeric as 0.0.
    fn number(&self, name: &str) -> f64 {
        self.get(name).and_then(Value::as_number).unwrap_or(0.0)
    }

    /// Run an inline `@tap` handler. M6 supports a single assignment statement:
    /// `name = expr`, `name += expr`, or `name -= expr`, where `expr` is
    /// arithmetic over numbers and signal names. Returns whether it applied.
    ///
    /// A deliberately tiny interpreter — the real script tier (rhai, M8)
    /// replaces it and adds function calls, conditionals, etc.
    pub fn run_handler(&mut self, src: &str) -> bool {
        let src = src.trim().trim_end_matches(';').trim();

        let (name, op, rhs) = if let Some((l, r)) = src.split_once("+=") {
            (l.trim(), '+', r)
        } else if let Some((l, r)) = src.split_once("-=") {
            (l.trim(), '-', r)
        } else if let Some((l, r)) = src.split_once('=') {
            (l.trim(), '=', r)
        } else {
            return false;
        };

        let value = eval(rhs, self);
        let next = match op {
            '+' => self.number(name) + value,
            '-' => self.number(name) - value,
            _ => value,
        };
        self.set(name, Value::Number(next));
        true
    }
}

/// Evaluate an arithmetic expression over numbers and signal names.
fn eval(expr: &str, signals: &Signals) -> f64 {
    let tokens: Vec<char> = expr.chars().collect();
    let mut parser = Expr {
        tokens: &tokens,
        pos: 0,
        signals,
    };
    parser.expr()
}

/// A tiny recursive-descent arithmetic parser: `+ - * /`, parens, numbers, names.
struct Expr<'a> {
    tokens: &'a [char],
    pos: usize,
    signals: &'a Signals,
}

impl Expr<'_> {
    fn skip_ws(&mut self) {
        while matches!(self.tokens.get(self.pos), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.skip_ws();
        self.tokens.get(self.pos).copied()
    }

    fn expr(&mut self) -> f64 {
        let mut acc = self.term();
        while let Some(op) = self.peek() {
            if op == '+' || op == '-' {
                self.pos += 1;
                let rhs = self.term();
                acc = if op == '+' { acc + rhs } else { acc - rhs };
            } else {
                break;
            }
        }
        acc
    }

    fn term(&mut self) -> f64 {
        let mut acc = self.factor();
        while let Some(op) = self.peek() {
            if op == '*' || op == '/' {
                self.pos += 1;
                let rhs = self.factor();
                acc = if op == '*' { acc * rhs } else { acc / rhs };
            } else {
                break;
            }
        }
        acc
    }

    fn factor(&mut self) -> f64 {
        match self.peek() {
            Some('(') => {
                self.pos += 1;
                let v = self.expr();
                if self.peek() == Some(')') {
                    self.pos += 1;
                }
                v
            }
            Some('-') => {
                self.pos += 1;
                -self.factor()
            }
            Some(c) if c.is_ascii_digit() || c == '.' => self.number_literal(),
            Some(c) if c.is_alphabetic() || c == '_' => {
                let name = self.identifier();
                self.signals.number(&name)
            }
            _ => 0.0,
        }
    }

    fn number_literal(&mut self) -> f64 {
        let start = self.pos;
        while matches!(self.tokens.get(self.pos), Some(c) if c.is_ascii_digit() || *c == '.') {
            self.pos += 1;
        }
        self.tokens[start..self.pos]
            .iter()
            .collect::<String>()
            .parse()
            .unwrap_or(0.0)
    }

    fn identifier(&mut self) -> String {
        let start = self.pos;
        while matches!(self.tokens.get(self.pos), Some(c) if c.is_alphanumeric() || *c == '_') {
            self.pos += 1;
        }
        self.tokens[start..self.pos].iter().collect()
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

    #[test]
    fn runs_inline_handlers() {
        let mut s = Signals::from_script("let level = signal(50);");

        assert!(s.run_handler("level = level - 1"));
        assert_eq!(s.get("level"), Some(&Value::Number(49.0)));

        s.run_handler("level += 10");
        assert_eq!(s.get("level"), Some(&Value::Number(59.0)));

        s.run_handler("level -= 9");
        assert_eq!(s.get("level"), Some(&Value::Number(50.0)));

        // Precedence and parens.
        s.run_handler("level = 2 + 3 * 4");
        assert_eq!(s.get("level"), Some(&Value::Number(14.0)));
        s.run_handler("level = (2 + 3) * 4");
        assert_eq!(s.get("level"), Some(&Value::Number(20.0)));
    }
}
