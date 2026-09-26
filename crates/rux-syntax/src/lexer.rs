//! Source text to tokens.
//!
//! Step 2 of `docs/11-next.md` accepts exactly the language the rhai fork
//! accepts, so this follows the fork's tokenizer (`crates/rux-rhai/src/
//! tokenizer.rs`) rule for rule, quirks included: a `""` inside a string is one
//! quote, `1.)` is the float `1.0`, and `-1` is one token only where a minus
//! cannot be binary. Where a rule here looks odd, the fork is the reason, and
//! changing it is a change to the language, which belongs to step 3.

use std::borrow::Cow;

use crate::span::Span;
use crate::SyntaxError;

/// One token and where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Token<'s> {
    pub tok: Tok<'s>,
    pub span: Span,
}

/// What a token is. Names and text borrow from the source where they can;
/// a string with an escape in it is the one that has to be built.
#[derive(Clone, Debug, PartialEq)]
pub enum Tok<'s> {
    Int(i64),
    Float(f64),
    /// `"…"`, or a backtick string with no `${` in it.
    Str(Cow<'s, str>),
    /// `'x'`.
    Char(char),
    /// A backtick string with at least one `${ … }`.
    Template(Vec<TplPart<'s>>),
    /// A name that is neither a keyword nor reserved.
    Ident(&'s str),
    /// A keyword: `let`, `if`, `fn`, … and `_`.
    Kw(&'static str),
    /// A word the fork reserves: `print`, `this`, `null`, `is`, `use`, …
    /// Some of them are callable (`print(x)`); the parser decides.
    Reserved(&'s str),
    /// Punctuation and operators.
    Punct(&'static str),
    Eof,
}

/// A piece of a backtick string.
#[derive(Clone, Debug, PartialEq)]
pub enum TplPart<'s> {
    /// Text, with its escapes already applied.
    Text(Cow<'s, str>, Span),
    /// The tokens between `${` and its `}`, ending in [`Tok::Eof`]. The span
    /// covers the braces.
    Code(Vec<Token<'s>>, Span),
}

impl Tok<'_> {
    pub fn is_punct(&self, p: &str) -> bool {
        matches!(self, Tok::Punct(q) if *q == p)
    }

    pub fn is_kw(&self, k: &str) -> bool {
        matches!(self, Tok::Kw(q) if *q == k)
    }

    pub fn is_reserved(&self, r: &str) -> bool {
        matches!(self, Tok::Reserved(q) if *q == r)
    }

    pub fn is_ident(&self, name: &str) -> bool {
        matches!(self, Tok::Ident(q) if *q == name)
    }

    /// How the token reads in a message.
    pub fn describe(&self) -> String {
        match self {
            Tok::Int(n) => n.to_string(),
            Tok::Float(f) => format!("{f:?}"),
            Tok::Str(_) | Tok::Template(_) => "a string".into(),
            Tok::Char(c) => format!("'{c}'"),
            Tok::Ident(s) | Tok::Reserved(s) => format!("`{s}`"),
            Tok::Kw(k) | Tok::Punct(k) => format!("`{k}`"),
            Tok::Eof => "the end".into(),
        }
    }
}

/// The keyword `word` is, if it is one.
fn keyword(word: &str) -> Option<&'static str> {
    Some(match word {
        "as" => "as",
        "break" => "break",
        "catch" => "catch",
        "const" => "const",
        "continue" => "continue",
        "do" => "do",
        "else" => "else",
        "export" => "export",
        "false" => "false",
        "fn" => "fn",
        "for" => "for",
        "if" => "if",
        "import" => "import",
        "in" => "in",
        "let" => "let",
        "loop" => "loop",
        "private" => "private",
        "return" => "return",
        "switch" => "switch",
        "throw" => "throw",
        "true" => "true",
        "try" => "try",
        "until" => "until",
        "while" => "while",
        "_" => "_",
        _ => return None,
    })
}

/// Words the fork reserves. `exit` is on its list but not reserved.
fn is_reserved_word(word: &str) -> bool {
    matches!(
        word,
        "use" | "case" | "async" | "public" | "package" | "super" | "var" | "protected" | "spawn"
            | "shared" | "is" | "sync" | "curry" | "static" | "default" | "print" | "this"
            | "is_def_var" | "thread" | "yield" | "new" | "call" | "match" | "eval" | "await"
            | "null" | "debug" | "type_of" | "with" | "void" | "nil" | "module" | "Fn"
    )
}

/// Reserved words that may still be called as functions: `print(x)`.
pub const CALLABLE_RESERVED: &[&str] = &["print", "debug", "type_of", "Fn", "call", "curry", "eval", "is_def_var"];

/// Reserved words that may still be called as methods: `f.call(1)`.
pub const METHOD_RESERVED: &[&str] = &["type_of", "call", "curry"];

/// The operator or punctuation `rest` starts with, the longest that fits.
fn punct(rest: &[u8]) -> Option<&'static str> {
    if rest.len() >= 3 {
        let three = match &rest[..3] {
            b"===" => Some("==="),
            b"!==" => Some("!=="),
            b"**=" => Some("**="),
            b"<<=" => Some("<<="),
            b">>=" => Some(">>="),
            b"..." => Some("..."),
            b"..=" => Some("..="),
            b"::<" => Some("::<"),
            _ => None,
        };
        if three.is_some() {
            return three;
        }
    }
    if rest.len() >= 2 {
        let two = match &rest[..2] {
            b"==" => Some("=="),
            b"!=" => Some("!="),
            b"<=" => Some("<="),
            b">=" => Some(">="),
            b"&&" => Some("&&"),
            b"||" => Some("||"),
            b"??" => Some("??"),
            b"?." => Some("?."),
            b"?[" => Some("?["),
            b"::" => Some("::"),
            b"=>" => Some("=>"),
            b".." => Some(".."),
            b"+=" => Some("+="),
            b"-=" => Some("-="),
            b"*=" => Some("*="),
            b"/=" => Some("/="),
            b"%=" => Some("%="),
            b"|=" => Some("|="),
            b"&=" => Some("&="),
            b"^=" => Some("^="),
            b"**" => Some("**"),
            b"<<" => Some("<<"),
            b">>" => Some(">>"),
            b"++" => Some("++"),
            b"--" => Some("--"),
            b"->" => Some("->"),
            b"<-" => Some("<-"),
            b"|>" => Some("|>"),
            b"<|" => Some("<|"),
            b":=" => Some(":="),
            b":;" => Some(":;"),
            b"(*" => Some("(*"),
            b"*)" => Some("*)"),
            b"!." => Some("!."),
            b"#{" => Some("#{"),
            b"#!" => Some("#!"),
            b"()" => Some("()"),
            _ => None,
        };
        if two.is_some() {
            return two;
        }
    }
    Some(match *rest.first()? {
        b'+' => "+",
        b'-' => "-",
        b'*' => "*",
        b'/' => "/",
        b'%' => "%",
        b'=' => "=",
        b'<' => "<",
        b'>' => ">",
        b'!' => "!",
        b'|' => "|",
        b'&' => "&",
        b'^' => "^",
        b'~' => "~",
        b'(' => "(",
        b')' => ")",
        b'[' => "[",
        b']' => "]",
        b'{' => "{",
        b'}' => "}",
        b',' => ",",
        b';' => ";",
        b':' => ":",
        b'.' => ".",
        b'?' => "?",
        b'@' => "@",
        b'$' => "$",
        b'#' => "#",
        _ => return None,
    })
}

/// Symbols the fork reserves and never gives a meaning. `?` is among them in
/// an expression, and means something in a type, which the parser handles.
pub const RESERVED_SYMBOLS: &[&str] = &[
    "...", "::<", "->", "<-", "|>", "<|", ":=", ":;", "(*", "*)", "!.", "#!", "~", "@", "$", "#",
];

/// Whether a `-` or `+` after this token starts an operand, which is when
/// `-1` is lexed as one negative literal. The fork's `is_next_unary`.
fn unary_follows(tok: &Tok) -> bool {
    match tok {
        Tok::Punct(p) => matches!(
            *p,
            ";" | ":" | "," | "??" | ".." | "..=" | "{" | "(" | "[" | "?[" | "+" | "+=" | "-" | "-="
                | "*" | "*=" | "/" | "/=" | "%" | "%=" | "**" | "**=" | "<<" | "<<=" | ">>"
                | ">>=" | "=" | "==" | "!=" | "<" | ">" | "!" | "<=" | ">=" | "|" | "&" | "&&"
                | "&=" | "||" | "|=" | "^" | "^=" | "!in"
        ),
        Tok::Kw(k) => matches!(*k, "if" | "while" | "until" | "in" | "return" | "throw"),
        _ => false,
    }
}

/// Tokenize a whole source, ending in [`Tok::Eof`].
pub fn lex(src: &str) -> Result<Vec<Token<'_>>, SyntaxError> {
    let mut lexer = Lexer { src, bytes: src.as_bytes(), i: 0 };
    let mut out = lexer.tokens(false, src.len() / 4)?;
    out.push(Token { tok: Tok::Eof, span: Span::at(src.len()) });
    Ok(out)
}

/// A cursor over the source by byte offset. Everything outside a string or a
/// comment is ASCII, so most steps are one byte; the ones that may not be go
/// through [`Lexer::peek`] and [`Lexer::advance`], which step a whole
/// character.
struct Lexer<'s> {
    src: &'s str,
    bytes: &'s [u8],
    i: usize,
}

impl<'s> Lexer<'s> {
    fn byte(&self, n: usize) -> Option<u8> {
        self.bytes.get(self.i + n).copied()
    }

    fn peek(&self) -> Option<char> {
        let b = *self.bytes.get(self.i)?;
        if b < 0x80 {
            Some(b as char)
        } else {
            self.src[self.i..].chars().next()
        }
    }

    /// Past the character at the cursor.
    fn advance(&mut self) {
        if let Some(c) = self.peek() {
            self.i += c.len_utf8();
        }
    }

    /// The 1-based column, in characters, of byte offset `at`.
    fn column_of(&self, at: usize) -> usize {
        let line_start = self.bytes[..at].iter().rposition(|&b| b == b'\n').map_or(0, |n| n + 1);
        self.src[line_start..at].chars().count() + 1
    }

    fn error(&self, message: impl Into<String>, start: usize) -> SyntaxError {
        SyntaxError { message: message.into(), span: Span::new(start, self.i.max(start)) }
    }

    /// Tokens until the end, or, with `until_brace`, until the `}` matching
    /// one already consumed, which is consumed too.
    fn tokens(&mut self, until_brace: bool, capacity: usize) -> Result<Vec<Token<'s>>, SyntaxError> {
        let mut out: Vec<Token<'s>> = Vec::with_capacity(capacity);
        let mut depth = 0usize;
        // At the start of the input, and just inside a `{`, a minus is unary.
        let mut unary = true;
        loop {
            let Some(token) = self.next_token(unary)? else {
                if until_brace {
                    return Err(self.error("a `${` in a string is never closed with `}`", self.src.len()));
                }
                return Ok(out);
            };
            if until_brace {
                if token.tok.is_punct("{") || token.tok.is_punct("#{") {
                    depth += 1;
                } else if token.tok.is_punct("}") {
                    if depth == 0 {
                        return Ok(out);
                    }
                    depth -= 1;
                }
            }
            unary = unary_follows(&token.tok);
            out.push(token);
        }
    }

    fn next_token(&mut self, unary: bool) -> Result<Option<Token<'s>>, SyntaxError> {
        loop {
            let Some(b) = self.byte(0) else { return Ok(None) };
            let start = self.i;
            let next = self.byte(1);

            // Whitespace, ASCII only, as the fork has it.
            if b.is_ascii_whitespace() {
                self.i += 1;
                continue;
            }
            // Comments.
            if b == b'/' && next == Some(b'/') {
                self.i = self.bytes[self.i..].iter().position(|&b| b == b'\n').map_or(self.bytes.len(), |n| self.i + n + 1);
                continue;
            }
            if b == b'/' && next == Some(b'*') {
                self.i += 2;
                let mut level = 1;
                // An unclosed block comment runs to the end and ends the input
                // quietly, as it does in the fork.
                while level > 0 {
                    match (self.byte(0), self.byte(1)) {
                        (None, _) => return Ok(None),
                        (Some(b'/'), Some(b'*')) => {
                            level += 1;
                            self.i += 2;
                        }
                        (Some(b'*'), Some(b'/')) => {
                            level -= 1;
                            self.i += 2;
                        }
                        _ => self.i += 1,
                    }
                }
                continue;
            }

            let tok = if b.is_ascii_digit() || (b == b'-' && unary && next.is_some_and(|n| n.is_ascii_digit())) {
                self.number()?
            } else if b == b'"' {
                self.i += 1;
                let (text, _) = self.quoted(b'"', start, false, true, false)?;
                Tok::Str(text)
            } else if b == b'`' {
                self.template(start)?
            } else if b == b'\'' {
                self.i += 1;
                if self.byte(0) == Some(b'\'') {
                    self.i += 1;
                    return Err(self.error("`''` holds no character", start));
                }
                let (text, _) = self.quoted(b'\'', start, false, false, false)?;
                let mut chars = text.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Tok::Char(c),
                    _ => {
                        return Err(self.error(
                            format!("'{text}' is not one character; text is written in double quotes, \"{text}\""),
                            start,
                        ))
                    }
                }
            } else if b.is_ascii_alphabetic() || b == b'_' {
                self.word(start)?
            } else if b == b'!' && next == Some(b'i') && self.byte(2) == Some(b'n') {
                // `!in`, unless it is `!` before a name that starts `in`.
                if self.byte(3).is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') {
                    self.i += 1;
                    Tok::Punct("!")
                } else {
                    self.i += 3;
                    Tok::Punct("!in")
                }
            } else if let Some(p) = punct(&self.bytes[self.i..]) {
                // `+` and `-` read the same whichever side of an operand they
                // are on; only a following digit matters, handled above.
                self.i += p.len();
                Tok::Punct(p)
            } else {
                let c = self.peek().expect("not at the end");
                self.advance();
                return Err(self.error(format!("`{c}` cannot appear here"), start));
            };
            return Ok(Some(Token { tok, span: Span::new(start, self.i) }));
        }
    }

    fn word(&mut self, start: usize) -> Result<Tok<'s>, SyntaxError> {
        while self.byte(0).is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') {
            self.i += 1;
        }
        let word = &self.src[start..self.i];
        if let Some(k) = keyword(word) {
            return Ok(Tok::Kw(k));
        }
        if is_reserved_word(word) {
            return Ok(Tok::Reserved(word));
        }
        // A name needs a letter before any digit: `_1` and `__` are not names.
        let mut seen_letter = false;
        for c in word.bytes() {
            if c.is_ascii_alphabetic() {
                seen_letter = true;
                break;
            } else if c != b'_' {
                return Err(self.error(format!("`{word}` is not a name: a name needs a letter before any digit"), start));
            }
        }
        if !seen_letter {
            return Err(self.error(format!("`{word}` is not a name: a name needs a letter"), start));
        }
        Ok(Tok::Ident(word))
    }

    fn number(&mut self) -> Result<Tok<'s>, SyntaxError> {
        let start = self.i;
        let mut text = String::new();
        if self.byte(0) == Some(b'-') {
            text.push('-');
            self.i += 1;
        }
        let first = self.byte(0).unwrap() as char;
        text.push(first);
        self.i += 1;
        let mut radix: Option<u32> = None;
        let mut has_period = false;
        let mut has_e = false;
        let digits_len = |t: &str| t.trim_start_matches('-').len();
        while let Some(c) = self.byte(0).map(|b| b as char) {
            let valid = match radix {
                None => c.is_ascii_digit(),
                Some(16) => c.is_ascii_hexdigit(),
                Some(8) => ('0'..='7').contains(&c),
                Some(_) => c == '0' || c == '1',
            };
            if c == '_' {
                self.i += 1;
            } else if valid {
                text.push(c);
                self.i += 1;
            } else if c == '.' && !has_period && radix.is_none() {
                match self.byte(1).map(|b| b as char) {
                    Some(d) if d.is_ascii_digit() => {
                        text.push('.');
                        self.i += 1;
                        has_period = true;
                    }
                    Some('_') | Some('.') | None => break,
                    // `1.)` and `1. ` are the float `1.0`; `1.foo` is a call.
                    // A byte past 0x7F starts a character that is not a
                    // letter either, as the fork reads it.
                    Some(d) if !d.is_ascii_alphabetic() => {
                        text.push_str(".0");
                        self.i += 1;
                        has_period = true;
                    }
                    _ => break,
                }
            } else if (c == 'e' || c == 'E') && !has_e && radix.is_none() {
                match self.byte(1).map(|b| b as char) {
                    Some(d) if d.is_ascii_digit() => {
                        text.push('e');
                        self.i += 1;
                        has_e = true;
                        has_period = true;
                    }
                    Some(s @ ('+' | '-')) => {
                        text.push('e');
                        text.push(s);
                        self.i += 2;
                        has_e = true;
                        has_period = true;
                    }
                    _ => break,
                }
            } else if matches!(c, 'x' | 'o' | 'b' | 'X' | 'O' | 'B') && first == '0' && digits_len(&text) <= 1 {
                text.push(c);
                self.i += 1;
                radix = Some(match c.to_ascii_lowercase() {
                    'x' => 16,
                    'o' => 8,
                    _ => 2,
                });
            } else {
                break;
            }
        }
        let malformed = || SyntaxError {
            message: format!("`{text}` is not a number"),
            span: Span::new(start, start + text.len()),
        };
        if let Some(radix) = radix {
            // The fork reads a negated radix literal from the wrong offset and
            // rejects it; so does this.
            if text.starts_with('-') {
                return Err(malformed());
            }
            return u64::from_str_radix(&text[2..], radix).map(|v| Tok::Int(v as i64)).map_err(|_| malformed());
        }
        if let Ok(n) = text.parse::<i64>() {
            return Ok(Tok::Int(n));
        }
        text.parse::<f64>().map(Tok::Float).map_err(|_| malformed())
    }

    /// The body of a quoted string, the opening delimiter already consumed.
    /// Returns the text and whether it stopped at a `${`, in which case the
    /// `$` is consumed and the `{` is not.
    ///
    /// Most strings hold nothing to translate, and are borrowed from the
    /// source as they stand; the rest are built by [`Lexer::quoted_slowly`].
    fn quoted(
        &mut self,
        term: u8,
        start: usize,
        verbatim: bool,
        line_continuation: bool,
        interpolation: bool,
    ) -> Result<(Cow<'s, str>, bool), SyntaxError> {
        let from = self.i;
        let rest = &self.bytes[from..];
        let special = rest.iter().position(|&b| b == term || b == b'\\' || b == b'\n' || b == b'\r' || (interpolation && b == b'$'));
        if let Some(n) = special {
            if rest[n] == term && rest.get(n + 1) != Some(&term) {
                self.i = from + n + 1;
                return Ok((Cow::Borrowed(&self.src[from..from + n]), false));
            }
        }
        self.quoted_slowly(term as char, start, verbatim, line_continuation, interpolation).map(|(s, i)| (Cow::Owned(s), i))
    }

    fn quoted_slowly(
        &mut self,
        term: char,
        start: usize,
        verbatim: bool,
        line_continuation: bool,
        interpolation: bool,
    ) -> Result<(String, bool), SyntaxError> {
        let start_col = if line_continuation { self.column_of(start) } else { 0 };
        let mut out = String::new();
        let mut escape = false;
        let mut skip_space_until_col = 0usize;
        loop {
            let Some(mut c) = self.peek() else {
                if verbatim {
                    // The fork lets a backtick string run to the end.
                    return Ok((out, false));
                }
                if line_continuation && escape {
                    return Ok((out, false));
                }
                return Err(self.error("this string is never closed", start));
            };
            // Only a line continuation cares which column this is.
            let col = if skip_space_until_col > 0 { self.column_of(self.i) } else { 0 };
            self.advance();

            if interpolation && !escape {
                if c == '$' && self.byte(0) == Some(b'{') {
                    return Ok((out, true));
                }
                if c == '\\' && self.byte(0) == Some(b'$') {
                    self.i += 1;
                    if self.byte(0) == Some(b'{') {
                        c = '$';
                    } else {
                        self.i -= 1;
                    }
                }
            }

            if c == term && !escape {
                if self.peek() == Some(term) {
                    self.advance();
                } else {
                    return Ok((out, false));
                }
            }

            match c {
                '\r' if self.byte(0) == Some(b'\n') => {}
                'r' if escape => {
                    escape = false;
                    out.push('\r');
                }
                'n' if escape => {
                    escape = false;
                    out.push('\n');
                }
                '\\' if !verbatim && !escape => escape = true,
                '\\' if escape => {
                    escape = false;
                    out.push('\\');
                }
                't' if escape => {
                    escape = false;
                    out.push('\t');
                }
                'x' | 'u' | 'U' if escape => {
                    escape = false;
                    let len = match c {
                        'x' => 2,
                        'u' => 4,
                        _ => 8,
                    };
                    let mut value: u32 = 0;
                    for _ in 0..len {
                        let digit = self.peek().and_then(|d| d.to_digit(16));
                        let Some(digit) = digit else {
                            return Err(self.error(format!("`\\{c}` needs {len} hex digits after it"), start));
                        };
                        self.i += 1;
                        value = value * 16 + digit;
                    }
                    let Some(ch) = char::from_u32(value) else {
                        return Err(self.error(format!("`\\{c}{value:X}` is not a character"), start));
                    };
                    out.push(ch);
                }
                '\n' if verbatim => out.push('\n'),
                '\n' if line_continuation && escape => {
                    escape = false;
                    skip_space_until_col = start_col + 1;
                }
                '\n' => {
                    self.i -= 1;
                    return Err(self.error("this string is never closed on its line", start));
                }
                ch if ch == term && escape => {
                    escape = false;
                    out.push(term);
                }
                ch if escape => {
                    return Err(self.error(format!("`\\{ch}` is not an escape a string understands"), start));
                }
                ch if ch.is_whitespace() && col < skip_space_until_col => {}
                ch => {
                    out.push(ch);
                    skip_space_until_col = 0;
                }
            }
        }
    }

    /// A backtick string, the backtick not yet consumed.
    fn template(&mut self, start: usize) -> Result<Tok<'s>, SyntaxError> {
        self.i += 1;
        // A string that opens at the end of a line starts on the next one.
        if self.byte(0) == Some(b'\r') {
            self.i += 1;
            if self.byte(0) == Some(b'\n') {
                self.i += 1;
            }
        } else if self.byte(0) == Some(b'\n') {
            self.i += 1;
        }
        let mut parts = Vec::new();
        loop {
            let text_start = self.i;
            let (text, interpolated) = self.quoted(b'`', start, true, false, true)?;
            parts.push(TplPart::Text(text, Span::new(text_start, self.i)));
            if !interpolated {
                break;
            }
            let brace = self.i;
            self.i += 1; // the `{`
            let mut tokens = self.tokens(true, 8)?;
            let end = self.i;
            tokens.push(Token { tok: Tok::Eof, span: Span::at(end - 1) });
            parts.push(TplPart::Code(tokens, Span::new(brace, end)));
        }
        if parts.len() == 1 {
            let Some(TplPart::Text(text, _)) = parts.pop() else { unreachable!() };
            return Ok(Tok::Str(text));
        }
        Ok(Tok::Template(parts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok<'_>> {
        lex(src).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn numbers_follow_the_fork() {
        assert_eq!(toks("1_000"), [Tok::Int(1000), Tok::Eof]);
        assert_eq!(toks("1.5"), [Tok::Float(1.5), Tok::Eof]);
        assert_eq!(toks("2e3"), [Tok::Float(2000.0), Tok::Eof]);
        assert_eq!(toks("0x1F"), [Tok::Int(31), Tok::Eof]);
        assert_eq!(toks("0..3"), [Tok::Int(0), Tok::Punct(".."), Tok::Int(3), Tok::Eof]);
        assert_eq!(toks("1.foo"), [Tok::Int(1), Tok::Punct("."), Tok::Ident("foo".into()), Tok::Eof]);
        assert_eq!(toks("(1.)"), [Tok::Punct("("), Tok::Float(1.0), Tok::Punct(")"), Tok::Eof]);
    }

    #[test]
    fn a_minus_joins_a_number_only_where_it_cannot_be_binary() {
        assert_eq!(toks("-1"), [Tok::Int(-1), Tok::Eof]);
        assert_eq!(toks("a -1"), [Tok::Ident("a".into()), Tok::Punct("-"), Tok::Int(1), Tok::Eof]);
        assert_eq!(toks("f(-1)")[2], Tok::Int(-1));
    }

    #[test]
    fn strings_follow_the_fork() {
        assert_eq!(toks(r#""a\nb""#)[0], Tok::Str("a\nb".into()));
        assert_eq!(toks(r#""say ""hi""""#)[0], Tok::Str("say \"hi\"".into()));
        assert_eq!(toks("'x'")[0], Tok::Char('x'));
        assert!(lex("'xy'").is_err());
        assert!(lex("\"open\nclosed\"").is_err());
    }

    #[test]
    fn a_template_holds_its_code_as_tokens() {
        let t = toks("`a ${n + 1} b`");
        let Tok::Template(parts) = &t[0] else { panic!("{t:?}") };
        assert_eq!(parts.len(), 3);
        let TplPart::Code(code, _) = &parts[1] else { panic!() };
        assert_eq!(code[0].tok, Tok::Ident("n".into()));
        assert_eq!(toks("`plain`")[0], Tok::Str("plain".into()));
        assert_eq!(toks("`a ${ {x: 1}.x } b`").len(), 2);
    }

    #[test]
    fn comments_and_crlf() {
        assert_eq!(toks("a // x\r\nb /* c /* d */ e */ c"), [
            Tok::Ident("a".into()),
            Tok::Ident("b".into()),
            Tok::Ident("c".into()),
            Tok::Eof
        ]);
    }
}
