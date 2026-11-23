//! Handing over to other apps, from the app's own Rust: the same calls
//! script's `links` has.
//!
//! [`open`] shows a URL in the app that handles it (the browser, the dialer,
//! a maps app); [`dial`], [`sms`], [`directions`] and [`share`] build the right
//! URL or intent for the common cases; [`can_open`] says whether anything
//! would take a URL. Nothing here needs a permission: the person sees the
//! other app open, and it is theirs to confirm (the dialer does not place the
//! call).
//!
//! **Only some schemes open.** `https`, `http`, `tel`, `sms`, `mailto` and
//! `geo`. A link from a server must never reach an arbitrary intent
//! (`intent:`, `file:`, `content:`, `javascript:` or a custom scheme), so
//! those fail with `scheme`. A number or label given to [`dial`], [`sms`] or
//! [`directions`] is escaped, not trusted.
//!
//! ```ignore
//! use rux::links::{self, Directions};
//!
//! links::dial("+256 700 000 000")?;
//! links::directions(Directions::new(0.33, 32.6).label("Pickup, Kololo"))?;
//! ```
//!
//! The shell installs the backend at start-up with [`set_backend`], so these
//! calls fail with `unavailable` in a program that never started one.

use std::sync::OnceLock;

use crate::Error;

/// The URL schemes [`open`] will hand to the system.
pub const SCHEMES: [&str; 6] = ["https", "http", "tel", "sms", "mailto", "geo"];

/// A place to get to, for [`directions`].
#[derive(Clone, Debug, PartialEq)]
pub struct Directions {
    pub lat: f64,
    pub lng: f64,
    /// What a maps app calls the place. Empty for none.
    pub label: String,
}

impl Directions {
    pub fn new(lat: f64, lng: f64) -> Directions {
        Directions { lat, lng, label: String::new() }
    }

    pub fn label(mut self, label: impl Into<String>) -> Directions {
        self.label = label.into();
        self
    }

    /// `Ok` when the point is on the Earth.
    pub fn checked(&self) -> Result<(), Error> {
        let ok = self.lat.is_finite() && self.lng.is_finite() && (-90.0..=90.0).contains(&self.lat) && (-180.0..=180.0).contains(&self.lng);
        if ok {
            Ok(())
        } else {
            Err(Error::new(
                "option",
                format!("`lat` is between -90 and 90 and `lng` between -180 and 180, not {}, {}", self.lat, self.lng),
            ))
        }
    }

    /// Google Maps' turn-by-turn: the only URI that starts guidance.
    pub fn navigation_url(&self) -> String {
        format!("google.navigation:q={},{}", self.lat, self.lng)
    }

    /// A pin in whichever maps app the person has (`geo:`).
    pub fn pin_url(&self) -> String {
        if self.label.is_empty() {
            format!("geo:{},{}?q={},{}", self.lat, self.lng, self.lat, self.lng)
        } else {
            format!("geo:{},{}?q={},{}({})", self.lat, self.lng, self.lat, self.lng, percent_encode(&self.label))
        }
    }

    /// The same place as a web page: for a browser and a desktop.
    pub fn web_url(&self) -> String {
        format!("https://www.google.com/maps/dir/?api=1&destination={},{}", self.lat, self.lng)
    }
}

/// What to put in the share sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Share {
    pub title: String,
    pub text: String,
}

/// What the shell provides.
pub struct Backend {
    /// Show an already-checked URL in the app that handles it.
    pub open: fn(&str) -> Result<(), Error>,
    /// Navigate to a checked point.
    pub directions: fn(&Directions) -> Result<(), Error>,
    /// Show the share sheet.
    pub share: fn(&Share) -> Result<(), Error>,
    /// Whether something would handle an already-checked URL.
    pub can_open: fn(&str) -> bool,
}

static BACKEND: OnceLock<Backend> = OnceLock::new();

/// Install the backend. The first one wins, as the shell may start twice.
pub fn set_backend(backend: Backend) {
    let _ = BACKEND.set(backend);
}

fn backend() -> Result<&'static Backend, Error> {
    BACKEND.get().ok_or_else(|| Error::new("unavailable", "no links backend is installed: the shell has not started"))
}

/// Percent-encode `text` for a URL's query or path: everything but the
/// unreserved characters.
pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The URL, if its scheme is one that opens and it holds nothing a system
/// would read as more than one thing.
pub fn check_url(url: &str) -> Result<String, Error> {
    let url = url.trim();
    if url.is_empty() {
        return Err(Error::new("option", "`links.open` takes a URL, such as \"https://example.com\""));
    }
    if url.chars().any(|c| c.is_control() || c == '\u{7f}') {
        return Err(Error::new("scheme", "a URL has no control characters or line breaks"));
    }
    let Some((scheme, rest)) = url.split_once(':') else {
        return Err(Error::new("scheme", format!("`{url}` has no scheme: write it out, such as \"https://{url}\"")));
    };
    let lower = scheme.to_ascii_lowercase();
    if !SCHEMES.contains(&lower.as_str()) {
        return Err(Error::new(
            "scheme",
            format!("`{scheme}:` is not a scheme Rux opens. It opens {}", SCHEMES.map(|s| format!("{s}:")).join(", ")),
        ));
    }
    if matches!(lower.as_str(), "http" | "https") && !rest.starts_with("//") {
        return Err(Error::new("scheme", format!("`{url}` is not a web address: it is \"{lower}://host/path\"")));
    }
    if rest.is_empty() || rest == "//" {
        return Err(Error::new("scheme", format!("`{url}` has nothing after `{lower}:`")));
    }
    Ok(format!("{lower}:{rest}"))
}

/// `tel:` for a phone number: digits, a leading `+`, and the separators people
/// write (spaces, `-`, `.`, brackets), which are dropped.
pub fn tel_url(number: &str) -> Result<String, Error> {
    Ok(format!("tel:{}", clean_number(number)?))
}

/// `sms:` for a number, with `text` already in the message.
pub fn sms_url(number: &str, text: &str) -> Result<String, Error> {
    let number = clean_number(number)?;
    if text.is_empty() {
        Ok(format!("sms:{number}"))
    } else {
        Ok(format!("sms:{number}?body={}", percent_encode(text)))
    }
}

fn clean_number(number: &str) -> Result<String, Error> {
    let mut out = String::new();
    for (i, c) in number.trim().chars().enumerate() {
        match c {
            '+' if i == 0 => out.push('+'),
            '0'..='9' => out.push(c),
            ' ' | '-' | '.' | '(' | ')' => {}
            other => {
                return Err(Error::new(
                    "option",
                    format!("a phone number is digits, with a leading + and spaces or dashes if you like, not `{other}`"),
                ))
            }
        }
    }
    if !out.chars().any(|c| c.is_ascii_digit()) {
        return Err(Error::new("option", "a phone number needs at least one digit"));
    }
    Ok(out)
}

/// Show `url` in the app that handles it. Any thread. Fails with `scheme`,
/// `notfound` (nothing handles it) or, on the web, `gesture`.
pub fn open(url: &str) -> Result<(), Error> {
    let url = check_url(url)?;
    (backend()?.open)(&url)
}

/// Open the dialer with `number` filled in. It does not place the call.
pub fn dial(number: &str) -> Result<(), Error> {
    let url = tel_url(number)?;
    (backend()?.open)(&url)
}

/// Open the messaging app with `number` and `text` filled in.
pub fn sms(number: &str, text: &str) -> Result<(), Error> {
    let url = sms_url(number, text)?;
    (backend()?.open)(&url)
}

/// Navigate to a place in the person's maps app.
pub fn directions(to: Directions) -> Result<(), Error> {
    to.checked()?;
    (backend()?.directions)(&to)
}

/// Open the share sheet with `text`.
pub fn share(what: Share) -> Result<(), Error> {
    if what.text.trim().is_empty() {
        return Err(Error::new("option", "`links.share` needs a `text` to share"));
    }
    (backend()?.share)(&what)
}

/// Whether something on this device would handle `url`.
pub fn can_open(url: &str) -> Result<bool, Error> {
    let url = check_url(url)?;
    Ok((backend()?.can_open)(&url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_listed_schemes_open_and_the_rest_say_so() {
        assert_eq!(check_url("https://example.com/a?b=1").unwrap(), "https://example.com/a?b=1");
        assert_eq!(check_url("  HTTPS://Example.com ").unwrap(), "https://Example.com", "the scheme is lowercased, the rest kept");
        assert_eq!(check_url("tel:+256700000000").unwrap(), "tel:+256700000000");
        assert_eq!(check_url("mailto:a@b.co").unwrap(), "mailto:a@b.co");
        for bad in ["javascript:alert(1)", "file:///etc/passwd", "intent://x#Intent;end", "content://a/b", "myapp://x", "data:text/html,x"] {
            let e = check_url(bad).unwrap_err();
            assert_eq!(e.kind, "scheme", "{bad}");
            assert!(e.message.contains("is not a scheme Rux opens"), "{}", e.message);
        }
        assert_eq!(check_url("example.com").unwrap_err().kind, "scheme");
        assert!(check_url("example.com").unwrap_err().message.contains("https://example.com"));
        assert_eq!(check_url("https:example.com").unwrap_err().kind, "scheme");
        assert_eq!(check_url("tel:").unwrap_err().kind, "scheme");
        assert_eq!(check_url("https://a.com\nhttps://b.com").unwrap_err().kind, "scheme", "a line break is refused");
        assert_eq!(check_url("   ").unwrap_err().kind, "option");
    }

    #[test]
    fn a_number_is_cleaned_and_a_text_is_escaped() {
        assert_eq!(tel_url("+256 (700) 000-000").unwrap(), "tel:+256700000000");
        assert_eq!(tel_url("0700.000.000").unwrap(), "tel:0700000000");
        assert_eq!(sms_url("+256700000000", "Hi & bye, 50%").unwrap(), "sms:+256700000000?body=Hi%20%26%20bye%2C%2050%25");
        assert_eq!(sms_url("0700", "").unwrap(), "sms:0700");
        assert_eq!(sms_url("0700", "é").unwrap(), "sms:0700?body=%C3%A9");
        for bad in ["", "call me", "123;rm", "12+3", "+"] {
            assert_eq!(tel_url(bad).unwrap_err().kind, "option", "{bad}");
        }
    }

    #[test]
    fn directions_are_range_checked_and_the_label_is_escaped() {
        let d = Directions::new(0.33, 32.6).label("Pickup (Kololo) & co");
        d.checked().unwrap();
        assert_eq!(d.navigation_url(), "google.navigation:q=0.33,32.6");
        assert_eq!(d.pin_url(), "geo:0.33,32.6?q=0.33,32.6(Pickup%20%28Kololo%29%20%26%20co)");
        assert_eq!(Directions::new(0.33, 32.6).pin_url(), "geo:0.33,32.6?q=0.33,32.6");
        assert_eq!(d.web_url(), "https://www.google.com/maps/dir/?api=1&destination=0.33,32.6");
        for (lat, lng) in [(91.0, 0.0), (0.0, 181.0), (f64::NAN, 0.0), (0.0, f64::INFINITY)] {
            assert_eq!(Directions::new(lat, lng).checked().unwrap_err().kind, "option");
        }
    }

    #[test]
    fn a_call_without_a_shell_says_so() {
        assert_eq!(open("https://example.com").unwrap_err().kind, "unavailable");
        assert_eq!(dial("0700").unwrap_err().kind, "unavailable");
        assert_eq!(share(Share { title: String::new(), text: "x".into() }).unwrap_err().kind, "unavailable");
        assert_eq!(share(Share { title: String::new(), text: " ".into() }).unwrap_err().kind, "option");
        assert_eq!(can_open("javascript:x").unwrap_err().kind, "scheme");
    }
}
