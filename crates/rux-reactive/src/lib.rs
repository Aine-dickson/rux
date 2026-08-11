//! Rux's shared value type.
//!
//! This crate began (M5) as the reactivity core: a flat `Signals` table plus a
//! little expression evaluator. M8 replaced both with the `rhai` engine in
//! `rux-script`, which owns state and evaluation now. What survives is `Value`
//!, the untyped representation that `rux-script` and `rux-style` pass between
//! each other for bindings, `r-for` locals, and props.
//!
//! The per-binding subscription model in `docs/04-architecture.md` is now built
//! (v0.3): `rux-script` tracks which signals each binding reads and which a
//! handler writes, and `rux-runtime` patches/reconciles just the affected nodes
//! in place instead of rebuilding the whole tree. This crate stays the shared
//! `Value` type those layers pass around.

/// Something the document does that will not work, with where it is if that is
/// known.
///
/// Lives here because both `rux-style` and `rux-script` raise these and neither
/// depends on the other; this is already the crate that exists to hold what they
/// pass between them. `rux-runtime` merges both sinks for the dev overlay, and
/// `rux check` turns them into editor diagnostics.
///
/// `line` is a **1-based line in the file**, not in the section that produced
/// it: a position relative to a `<style>` block would send a reader to the wrong
/// part of the file, which is worse than sending them nowhere. It is `None`
/// wherever the stage that noticed the problem does not know where it was, and
/// that is not a placeholder to be filled with a guess.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub message: String,
    pub line: Option<usize>,
}

impl Warning {
    /// A warning whose position is not known.
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), line: None }
    }

    /// A warning at a known 1-based file line.
    pub fn at(message: impl Into<String>, line: usize) -> Self {
        Self { message: message.into(), line: Some(line) }
    }

    /// Attach `line` if there is one, leaving the warning unplaced otherwise.
    pub fn maybe_at(message: impl Into<String>, line: Option<usize>) -> Self {
        Self { message: message.into(), line }
    }
}

/// Quote and escape `s` as a JSON string, per RFC 8259.
///
/// Lives beside [`Warning`] because both things that serialise one, the `rux
/// check` CLI and the browser playground, need exactly this and nothing more.
/// Two hand-rolled copies of an escaper is how the re-indenter went wrong.
/// Windows paths carry backslashes and messages quote the author's source, so
/// both of those have to survive the trip into an editor.
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Warning {
    /// `{"message": …, "line": … }`, with `line` present and null when unplaced
    /// so a consumer never has to tell "no position" from "field missing".
    pub fn to_json(&self) -> String {
        let line = match self.line {
            Some(line) => line.to_string(),
            None => "null".to_string(),
        };
        format!("{{\"message\": {}, \"line\": {line}}}", json_string(&self.message))
    }
}

impl std::fmt::Display for Warning {
    /// `line 12: message`, or just the message when it has no position. This is
    /// what the dev overlay shows, so it stays prose rather than becoming a
    /// `path:line:col` machine format.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(line) => write!(f, "line {line}: {}", self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

/// A dynamically-typed signal value. Untyped so template interpolation and the
/// future script tier can share one representation.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Text(String),
    Bool(bool),
    List(Vec<Value>),
    /// A rhai object map (`#{ key: value }`), key order as rhai yields it. Backs the
    /// object forms of `:class` (`#{ active: cond }`) and `:style` (`#{ bg: c }`).
    Map(Vec<(String, Value)>),
}

impl Value {
    /// How the value appears when interpolated into text.
    pub fn to_display(&self) -> String {
        match self {
            Value::Number(n) => {
                // Spelled the way JavaScript spells them, not the way Rust does.
                // Rust renders these "NaN", "inf" and "-inf"; a document that
                // divided by zero should not show its reader a word from a
                // language they are not writing in.
                if n.is_nan() {
                    "NaN".to_string()
                } else if n.is_infinite() {
                    if *n > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
                } else if n.fract() == 0.0 {
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
            Value::Map(entries) => entries
                .iter()
                .map(|(k, v)| format!("{k}: {}", v.to_display()))
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

    /// Truthiness for conditions, matching JavaScript exactly.
    ///
    /// Falsy: `false`, `0`, `NaN`, `""`, and the empty value. Everything else is
    /// truthy, **including an empty list and an empty map**.
    ///
    /// Those last two changed in v0.7 and are the only surprising part. An empty
    /// list used to be falsy, which reads better in isolation: `r-if="items"`
    /// meaning "there are items" is what most people would guess. It was dropped
    /// anyway, because a rule that is *almost* JavaScript is worse than either
    /// following it or visibly departing from it. Someone who knows the language
    /// Rux is modelled on should never have to discover a private exception, and
    /// `r-if="items.length"` says what it means without one.
    ///
    /// `NaN` is falsy because JavaScript says so, and because it is the honest
    /// answer for a number that is not one.
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::Text(s) => !s.is_empty(),
            Value::Bool(b) => *b,
            Value::List(..) | Value::Map(..) => true,
        }
    }

    /// The entries of a `Map`, for the object forms of `:class` / `:style`.
    pub fn as_map(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Map(entries) => Some(entries),
            _ => None,
        }
    }

    /// Serialize the value as rhai source, a literal that re-creates it. Used to
    /// bake an `r-for` loop binding into a `@tap` handler, which runs later in
    /// global scope where the loop variable no longer exists.
    pub fn to_rhai_literal(&self) -> String {
        match self {
            // Whole numbers become int literals (rhai's default numeric type, and
            // what collection indices/counters are), fractions stay floats.
            Value::Number(n) => {
                if n.fract() == 0.0 {
                    format!("{}", *n as i64)
                } else {
                    format!("{n}")
                }
            }
            Value::Text(s) => {
                let mut out = String::with_capacity(s.len() + 2);
                out.push('"');
                for c in s.chars() {
                    match c {
                        '\\' => out.push_str("\\\\"),
                        '"' => out.push_str("\\\""),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        _ => out.push(c),
                    }
                }
                out.push('"');
                out
            }
            Value::Bool(b) => b.to_string(),
            Value::List(items) => {
                let inner: Vec<_> = items.iter().map(Value::to_rhai_literal).collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Map(entries) => {
                let inner: Vec<_> = entries
                    .iter()
                    .map(|(k, v)| format!("{k}: {}", v.to_rhai_literal()))
                    .collect();
                format!("#{{{}}}", inner.join(", "))
            }
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

        // JavaScript's rules exactly, as of v0.7.
        assert!(Value::Number(1.0).is_truthy());
        assert!(!Value::Number(0.0).is_truthy());
        assert!(!Value::Number(f64::NAN).is_truthy(), "NaN is falsy, as in JS");
        assert!(!Value::Text(String::new()).is_truthy());
        assert!(Value::Text("0".into()).is_truthy(), "a non-empty string is truthy");
        assert!(Value::Bool(true).is_truthy());
        assert!(!Value::Bool(false).is_truthy());
        // The one that changed, and the one worth pinning: an empty list used
        // to be falsy. JS says every object is truthy, and a rule that is almost
        // JavaScript is worse than one that is or is not. `items.length` is how
        // you ask whether there are any.
        assert!(Value::List(Vec::new()).is_truthy());
        assert!(Value::Map(Vec::new()).is_truthy());
    }

    #[test]
    fn serializes_rhai_literals() {
        assert_eq!(Value::Number(3.0).to_rhai_literal(), "3");
        assert_eq!(Value::Number(2.5).to_rhai_literal(), "2.5");
        assert_eq!(Value::Bool(true).to_rhai_literal(), "true");
        assert_eq!(Value::Text("Charlie".into()).to_rhai_literal(), "\"Charlie\"");
        // Quotes and backslashes must be escaped so the handler still parses.
        assert_eq!(
            Value::Text("say \"hi\"\\n".into()).to_rhai_literal(),
            "\"say \\\"hi\\\"\\\\n\""
        );
        assert_eq!(
            Value::List(vec![Value::Number(1.0), Value::Text("a".into())]).to_rhai_literal(),
            "[1, \"a\"]"
        );
    }
}
