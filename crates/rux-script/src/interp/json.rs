//! `JSON.stringify` and `JSON.parse`, JavaScript's answers (Tracer's AW-020).
//!
//! A value Rux has and JSON does not (a function, a handle) is written as
//! `null`, as `JSON.stringify` writes `undefined` inside a list. A number
//! without a fraction or an exponent reads back as an `int`, one with either
//! as a `float`.

use std::collections::BTreeMap;
use std::rc::Rc;

use super::value::V;

/// `v` as JSON into `out`, each level `indent` further in, on one line when
/// `indent` is empty.
pub(crate) fn write(v: &V, indent: &str, level: usize, out: &mut String) {
    let open = |out: &mut String, items: usize, bracket: char, mut each: Box<dyn FnMut(&mut String, usize) + '_>| {
        out.push(bracket);
        for i in 0..items {
            if i > 0 {
                out.push(',');
            }
            if !indent.is_empty() {
                out.push('\n');
                out.push_str(&indent.repeat(level + 1));
            }
            each(out, i);
        }
        if items > 0 && !indent.is_empty() {
            out.push('\n');
            out.push_str(&indent.repeat(level));
        }
        out.push(if bracket == '[' { ']' } else { '}' });
    };
    let colon = if indent.is_empty() { ":" } else { ": " };
    match v {
        V::None => out.push_str("null"),
        V::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        V::Int(i) => out.push_str(&i.to_string()),
        V::Float(f) if f.is_finite() => out.push_str(&f.to_string()),
        V::Float(_) => out.push_str("null"),
        V::Str(s) => quote(s, out),
        V::Array(items) => open(out, items.len(), '[', Box::new(|out, i| write(&items[i], indent, level + 1, out))),
        V::Range(a, b) => {
            let items: Vec<V> = (*a..*b).map(V::Int).collect();
            write(&V::array(items), indent, level, out)
        }
        V::Map(m) => {
            let entries: Vec<(&String, &V)> = m.iter().collect();
            open(
                out,
                entries.len(),
                '{',
                Box::new(|out, i| {
                    quote(entries[i].0, out);
                    out.push_str(colon);
                    write(entries[i].1, indent, level + 1, out);
                }),
            )
        }
        V::Rec(r) => {
            let entries: Vec<(&str, &V)> = r.entries().collect();
            open(
                out,
                entries.len(),
                '{',
                Box::new(|out, i| {
                    quote(entries[i].0, out);
                    out.push_str(colon);
                    write(entries[i].1, indent, level + 1, out);
                }),
            )
        }
        _ => out.push_str("null"),
    }
}

fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
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
}

/// The value `text` holds, or what is wrong with it and where.
pub(crate) fn read(text: &str) -> Result<V, String> {
    let mut r = Reader { s: text.as_bytes(), at: 0 };
    let v = r.value()?;
    r.space();
    if r.at < r.s.len() {
        return Err(r.wrong("the end"));
    }
    Ok(v)
}

struct Reader<'a> {
    s: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn wrong(&self, wanted: &str) -> String {
        match self.s.get(self.at) {
            Some(_) => {
                let rest = String::from_utf8_lossy(&self.s[self.at..]);
                let near: String = rest.chars().take(12).collect();
                format!("expected {wanted} at character {}, found `{near}`", self.at + 1)
            }
            None => format!("expected {wanted}, and the text ended"),
        }
    }

    fn space(&mut self) {
        while self.s.get(self.at).is_some_and(|b| b.is_ascii_whitespace()) {
            self.at += 1;
        }
    }

    fn eat(&mut self, b: u8) -> bool {
        self.space();
        let here = self.s.get(self.at) == Some(&b);
        if here {
            self.at += 1;
        }
        here
    }

    fn word(&mut self, w: &str, v: V) -> Result<V, String> {
        if self.s[self.at..].starts_with(w.as_bytes()) {
            self.at += w.len();
            Ok(v)
        } else {
            Err(self.wrong("a value"))
        }
    }

    fn value(&mut self) -> Result<V, String> {
        self.space();
        match self.s.get(self.at) {
            Some(b'{') => {
                self.at += 1;
                let mut m = BTreeMap::new();
                if self.eat(b'}') {
                    return Ok(V::Map(Rc::new(m)));
                }
                loop {
                    self.space();
                    if self.s.get(self.at) != Some(&b'"') {
                        return Err(self.wrong("a key in quotes"));
                    }
                    let k = self.string()?;
                    if !self.eat(b':') {
                        return Err(self.wrong("`:`"));
                    }
                    let v = self.value()?;
                    m.insert(k, v);
                    if self.eat(b'}') {
                        return Ok(V::Map(Rc::new(m)));
                    }
                    if !self.eat(b',') {
                        return Err(self.wrong("`,` or `}`"));
                    }
                }
            }
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                if self.eat(b']') {
                    return Ok(V::array(items));
                }
                loop {
                    items.push(self.value()?);
                    if self.eat(b']') {
                        return Ok(V::array(items));
                    }
                    if !self.eat(b',') {
                        return Err(self.wrong("`,` or `]`"));
                    }
                }
            }
            Some(b'"') => Ok(V::str(self.string()?)),
            Some(b't') => self.word("true", V::Bool(true)),
            Some(b'f') => self.word("false", V::Bool(false)),
            Some(b'n') => self.word("null", V::None),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.wrong("a value")),
        }
    }

    fn number(&mut self) -> Result<V, String> {
        let start = self.at;
        let mut whole = true;
        while let Some(&b) = self.s.get(self.at) {
            match b {
                b'0'..=b'9' | b'-' | b'+' => {}
                b'.' | b'e' | b'E' => whole = false,
                _ => break,
            }
            self.at += 1;
        }
        let text = std::str::from_utf8(&self.s[start..self.at]).unwrap_or("");
        if whole {
            if let Ok(i) = text.parse::<i64>() {
                return Ok(V::Int(i));
            }
        }
        text.parse::<f64>().map(V::Float).map_err(|_| {
            self.at = start;
            self.wrong("a number")
        })
    }

    fn string(&mut self) -> Result<String, String> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let rest = std::str::from_utf8(&self.s[self.at..]).map_err(|_| "text that is not UTF-8".to_string())?;
            let Some(c) = rest.chars().next() else {
                return Err(self.wrong("`\"`"));
            };
            self.at += c.len_utf8();
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let Some(&e) = self.s.get(self.at) else { return Err(self.wrong("an escape")) };
                    self.at += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'u' => {
                            let code = |r: &mut Self| -> Result<u32, String> {
                                let hex = r.s.get(r.at..r.at + 4).and_then(|h| std::str::from_utf8(h).ok());
                                let n = hex.and_then(|h| u32::from_str_radix(h, 16).ok());
                                r.at += 4;
                                n.ok_or_else(|| "a `\\u` escape needs four hex digits".to_string())
                            };
                            let hi = code(self)?;
                            // A pair of halves is one character.
                            let c = if (0xd800..0xdc00).contains(&hi) && self.s[self.at..].starts_with(b"\\u") {
                                self.at += 2;
                                let lo = code(self)?;
                                char::from_u32(0x10000 + ((hi - 0xd800) << 10) + (lo.wrapping_sub(0xdc00)))
                            } else {
                                char::from_u32(hi)
                            };
                            out.push(c.unwrap_or('\u{fffd}'));
                        }
                        _ => return Err(self.wrong("an escape")),
                    }
                }
                c => out.push(c),
            }
        }
    }
}
