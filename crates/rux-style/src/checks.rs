//! A form field's built-in checks: `required`, `minlength`, `pattern`, a
//! number's `min` and `max`, and the shape an email or URL keyboard implies.
//!
//! One function answers for two callers. The build asks it to decide
//! `:invalid` and `:valid`, and the runtime asks it when a form is submitted,
//! so the style a field shows and the answer a submission gets can never
//! disagree.

use std::cell::RefCell;
use std::collections::HashMap;

use regex_lite::Regex;
use rux_layout::{Checks, Field, Keyboard, Shape};
use rux_reactive::Value;

/// What is wrong with a field's value, named the way HTML's `ValidityState`
/// names it.
#[derive(Clone, Debug, PartialEq)]
pub enum Problem {
    /// `required`, and the field is empty (or a checkbox is off).
    Missing,
    /// Shorter than `minlength`.
    TooShort { min: usize, length: usize },
    /// Does not match `pattern`.
    Mismatch,
    /// Below a number field's `min`.
    BelowMin(f64),
    /// Above a number field's `max`.
    AboveMax(f64),
    /// `inputmode="email"`, and the text is not an address.
    NotEmail,
    /// `inputmode="url"`, and the text is not an absolute URL.
    NotUrl,
}

impl Problem {
    /// The sentence `event.errors` carries, worded as browsers word theirs so
    /// an app that shows it as-is reads the way people expect.
    pub fn message(&self) -> String {
        match self {
            Self::Missing => "Fill in this field.".to_string(),
            Self::TooShort { min, length } => {
                format!("Use at least {min} characters (you are using {length}).")
            }
            Self::Mismatch => "Match the requested format.".to_string(),
            Self::BelowMin(n) => format!("Value must be {} or more.", number(*n)),
            Self::AboveMax(n) => format!("Value must be {} or less.", number(*n)),
            Self::NotEmail => "Enter an email address.".to_string(),
            Self::NotUrl => "Enter a URL.".to_string(),
        }
    }
}

/// `5`, not `5.0`: a bound written as a whole number reads back as one.
fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { n.to_string() }
}

/// A field's value, in the shape its checks need.
#[derive(Clone, Copy, Debug)]
pub enum FieldValue<'a> {
    /// A text field, whatever its kind.
    Text(&'a str),
    /// A number field. `None` when the signal holds nothing that is a number.
    Number(Option<f64>),
    /// A checkbox or switch.
    Flag(bool),
}

/// The first thing wrong with `value`, if anything is.
///
/// First only, in the order HTML reports them, because a person fixes one
/// thing at a time and a list of four complaints about an empty field is
/// three too many.
pub fn problem(checks: &Checks, keyboard: Keyboard, value: FieldValue) -> Option<Problem> {
    match value {
        FieldValue::Flag(on) => (checks.required && !on).then_some(Problem::Missing),
        FieldValue::Number(n) => {
            let Some(n) = n else {
                return checks.required.then_some(Problem::Missing);
            };
            if let Some(low) = checks.low.filter(|low| n < *low) {
                return Some(Problem::BelowMin(low));
            }
            checks.high.filter(|high| n > *high).map(Problem::AboveMax)
        }
        FieldValue::Text(text) => {
            if text.is_empty() {
                return checks.required.then_some(Problem::Missing);
            }
            match keyboard {
                Keyboard::Email if !matches(EMAIL, text) => return Some(Problem::NotEmail),
                Keyboard::Url if !matches(URL, text) => return Some(Problem::NotUrl),
                _ => {}
            }
            let length = text.encode_utf16().count();
            if let Some(min) = checks.minlength.filter(|min| length < *min) {
                return Some(Problem::TooShort { min, length });
            }
            match &checks.pattern {
                Some(pattern) if !matches(pattern, text) => Some(Problem::Mismatch),
                _ => None,
            }
        }
    }
}

/// The first thing wrong with a field whose signal holds `value`, reading it
/// in the shape the field says it has.
pub fn field_problem(field: &Field, value: Option<&Value>) -> Option<Problem> {
    let (checks, keyboard) = (&field.checks, field.keyboard);
    match field.shape {
        Shape::Flag => problem(checks, keyboard, FieldValue::Flag(value.is_some_and(Value::is_truthy))),
        Shape::Number => problem(checks, keyboard, FieldValue::Number(value.and_then(Value::as_number))),
        Shape::Text => {
            let text = value.map(Value::to_display).unwrap_or_default();
            problem(checks, keyboard, FieldValue::Text(&text))
        }
    }
}

/// HTML's own definition of a valid email address, from the `type=email`
/// section of the spec: deliberately looser than RFC 5322, which nobody types.
const EMAIL: &str = "[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\
                     (?:\\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*";

/// An absolute URL: a scheme, a colon, and something after it with no spaces.
const URL: &str = "[a-zA-Z][a-zA-Z0-9+.-]*:[^\\s]+";

/// Is a `pattern` something Rux can check? `Err` carries the reason, for the
/// build to report, since a pattern that cannot compile would otherwise
/// accept everything without a word.
pub fn check_pattern(pattern: &str) -> Result<(), String> {
    Regex::new(&anchored(pattern)).map(|_| ()).map_err(|e| e.to_string())
}

/// `pattern` matches the whole value, as HTML's does: `\d{3}` accepts `123`
/// and refuses `1234`.
fn anchored(pattern: &str) -> String {
    format!("^(?:{pattern})$")
}

thread_local! {
    /// Compiled patterns, because the build asks on every keystroke in a field
    /// that has one. A pattern that did not compile is kept as `None` and
    /// matches everything; the build has already said so.
    static COMPILED: RefCell<HashMap<String, Option<Regex>>> = RefCell::new(HashMap::new());
}

fn matches(pattern: &str, text: &str) -> bool {
    COMPILED.with(|c| {
        c.borrow_mut()
            .entry(pattern.to_string())
            .or_insert_with(|| Regex::new(&anchored(pattern)).ok())
            .as_ref()
            .is_none_or(|re| re.is_match(text))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(checks: &Checks, keyboard: Keyboard, value: &str) -> Option<Problem> {
        problem(checks, keyboard, FieldValue::Text(value))
    }

    #[test]
    fn empty_fails_only_required() {
        let mut checks = Checks { minlength: Some(3), pattern: Some("\\d+".into()), ..Checks::default() };
        assert_eq!(text(&checks, Keyboard::Email, ""), None);
        checks.required = true;
        assert_eq!(text(&checks, Keyboard::Email, ""), Some(Problem::Missing));
    }

    #[test]
    fn pattern_matches_the_whole_value() {
        let checks = Checks { pattern: Some("\\d{3}".into()), ..Checks::default() };
        assert_eq!(text(&checks, Keyboard::Text, "123"), None);
        assert_eq!(text(&checks, Keyboard::Text, "1234"), Some(Problem::Mismatch));
        assert_eq!(text(&checks, Keyboard::Text, "a123"), Some(Problem::Mismatch));
    }

    #[test]
    fn minlength_counts_like_maxlength() {
        let checks = Checks { minlength: Some(3), ..Checks::default() };
        assert_eq!(
            text(&checks, Keyboard::Text, "ab"),
            Some(Problem::TooShort { min: 3, length: 2 })
        );
        assert_eq!(text(&checks, Keyboard::Text, "abc"), None);
    }

    #[test]
    fn email_and_url_follow_the_keyboard() {
        let none = Checks::default();
        assert_eq!(text(&none, Keyboard::Email, "grace@navy.mil"), None);
        assert_eq!(text(&none, Keyboard::Email, "grace"), Some(Problem::NotEmail));
        assert_eq!(text(&none, Keyboard::Url, "https://ruxlang.dev"), None);
        assert_eq!(text(&none, Keyboard::Url, "ruxlang.dev"), Some(Problem::NotUrl));
        assert_eq!(text(&none, Keyboard::Text, "grace"), None);
    }

    #[test]
    fn numbers_and_flags() {
        let range = Checks { low: Some(1.0), high: Some(10.0), ..Checks::default() };
        let n = |v| problem(&range, Keyboard::Text, FieldValue::Number(Some(v)));
        assert_eq!(n(0.0), Some(Problem::BelowMin(1.0)));
        assert_eq!(n(11.0), Some(Problem::AboveMax(10.0)));
        assert_eq!(n(5.0), None);
        assert_eq!(Problem::BelowMin(1.0).message(), "Value must be 1 or more.");
        let required = Checks { required: true, ..Checks::default() };
        assert_eq!(problem(&required, Keyboard::Text, FieldValue::Flag(false)), Some(Problem::Missing));
        assert_eq!(problem(&required, Keyboard::Text, FieldValue::Flag(true)), None);
    }

    #[test]
    fn a_broken_pattern_is_reported_and_accepts_everything() {
        assert!(check_pattern("(").is_err());
        let checks = Checks { pattern: Some("(".into()), ..Checks::default() };
        assert_eq!(text(&checks, Keyboard::Text, "anything"), None);
    }
}
