//! Source text to tokens.
//!
//! Step 2 of `docs/11-next.md` accepts exactly the language the rhai fork
//! accepts, so this follows the fork's tokenizer (`crates/rux-rhai/src/
//! tokenizer.rs`) rule for rule, quirks included: a `""` inside a string is one
//! quote, `1.)` is the float `1.0`, and `-1` is one token only where a minus
//! cannot be binary. Where a rule here looks odd, the fork is the reason, and
//! changing it is a change to the language, which belongs to step 3.

use crate::span::Span;
use crate::SyntaxError;

/// One token and where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

/// What a token is.
#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Int(i64),
    Float(f64),
    /// `"…"`, or a backtick string with no `${` in it.
    Str(String),
    /// `'x'`.
    Char(char),
    /// A backtick string with at least one `${ … }`.
    Template(Vec<TplPart>),
    /// A name that is neither a keyword nor reserved.
    Ident(String),
    /// A keyword: `let`, `if`, `fn`, … and `_`.
    Kw(&'static str),
    /// A word the fork reserves: `print`, `this`, `null`, `is`, `use`, …
    /// Some of them are callable (`print(x)`); the parser decides.
    Reserved(String),
    /// Punctuation and operators.
    Punct(&'static str),
    Eof,
}

/// A piece of a backtick string.
#[derive(Clone, Debug, PartialEq)]
pub enum TplPart {
    /// Text, with its escapes already applied.
    Text(String, Span),
    /// The tokens between `${` and its `}`, ending in [`Tok::Eof`]. The span
    /// covers the braces.
    Code(Vec<Token>, Span),
}

impl Tok {
    pub fn is_punct(&self, p: &str) -> bool {
        matches!(self, Tok::Punct(q) if *q == p)
    }

    pub fn is_kw(&self, k: &str) -> bool {
        matches!(self, Tok::Kw(q) if *q == k)
    }

    pub fn is_reserved(&self, r: &str) -> bool {
        matches!(self, Tok::Reserved(q) if q == r)
    }

    pub fn is_ident(&self, name: &str) -> bool {
        matches!(self, Tok::Ident(q) if q == name)
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

const KEYWORDS: &[&str] = &[
    "as", "break", "catch", "const", "continue", "do", "else", "export", "false", "fn", "for", "if",
    "import", "in", "let", "loop", "private", "return", "switch", "throw", "true", "try", "until",
    "while", "_",
];

/// Words the fork reserves. `exit` is on its list but not reserved.
const RESERVED_WORDS: &[&str] = &[
    "use", "case", "async", "public", "package", "super", "var", "protected", "spawn", "shared",
    "is", "sync", "curry", "static", "default", "print", "this", "is_def_var", "thread", "yield",
    "new", "call", "match", "eval", "await", "null", "debug", "type_of", "with", "void", "nil",
    "module", "Fn",
];

/// Reserved words that may still be called as functions: `print(x)`.
pub const CALLABLE_RESERVED: &[&str] = &["print", "debug", "type_of", "Fn", "call", "curry", "eval", "is_def_var"];

/// Reserved words that may still be called as methods: `f.call(1)`.
pub const METHOD_RESERVED: &[&str] = &["type_of", "call", "curry"];

/// Operators and punctuation, longest first so the first match is the right one.
const PUNCT: &[&str] = &[
    "===", "!==", "**=", "<<=", ">>=", "...", "..=", "::<", "==", "!=", "<=", ">=", "&&", "||",
    "??", "?.", "?[", "::", "=>", "..", "+=", "-=", "*=", "/=", "%=", "|=", "&=", "^=", "**", "<<",
    ">>", "++", "--", "->", "<-", "|>", "<|", ":=", ":;", "(*", "*)", "!.", "#{", "#!", "()", "+",
    "-", "*", "/", "%", "=", "<", ">", "!", "|", "&", "^", "~", "(", ")", "[", "]", "{", "}", ",",
    ";", ":", ".", "?", "@", "$", "#",
];

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
pub fn lex(src: &str) -> Result<Vec<Token>, SyntaxError> {
    let mut lexer = Lexer { src, chars: src.char_indices().collect(), i: 0 };
    let mut out = lexer.tokens(false)?;
    out.push(Token { tok: Tok::Eof, span: Span::at(src.len()) });
    Ok(out)
}

struct Lexer<'s> {
    src: &'s str,
    chars: Vec<(usize, char)>,
    i: usize,
}

impl Lexer<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).map(|&(_, c)| c)
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.i + n).map(|&(_, c)| c)
    }

    /// The byte offset of the next character, or the end.
    fn here(&self) -> usize {
        self.chars.get(self.i).map_or(self.src.len(), |&(b, _)| b)
    }

    /// The 1-based column of the character at index `i`.
    fn column_of(&self, i: usize) -> usize {
        let mut col = 1;
        let mut j = i;
        while j > 0 && self.chars[j - 1].1 != '\n' {
            j -= 1;
            col += 1;
        }
        col
    }

    fn error(&self, message: impl Into<String>, start: usize) -> SyntaxError {
        SyntaxError { message: message.into(), span: Span::new(start, self.here().max(start)) }
    }

    /// Tokens until the end, or, with `until_brace`, until the `}` matching
    /// one already consumed, which is consumed too.
    fn tokens(&mut self, until_brace: bool) -> Result<Vec<Token>, SyntaxError> {
        let mut out: Vec<Token> = Vec::new();
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

    fn next_token(&mut self, unary: bool) -> Result<Option<Token>, SyntaxError> {
        loop {
            let Some(c) = self.peek() else { return Ok(None) };
            let start = self.here();
            let next = self.peek_at(1);

            // Whitespace, ASCII only, as the fork has it.
            if c.is_ascii_whitespace() {
                self.i += 1;
                continue;
            }
            // Comments.
            if c == '/' && next == Some('/') {
                while let Some(c) = self.peek() {
                    self.i += 1;
                    if c == '\n' {
                        break;
                    }
                }
                continue;
            }
            if c == '/' && next == Some('*') {
                self.i += 2;
                let mut level = 1;
                // An unclosed block comment runs to the end and ends the input
                // quietly, as it does in the fork.
                while level > 0 {
                    match (self.peek(), self.peek_at(1)) {
                        (None, _) => return Ok(None),
                        (Some('/'), Some('*')) => {
                            level += 1;
                            self.i += 2;
                        }
                        (Some('*'), Some('/')) => {
                            level -= 1;
                            self.i += 2;
                        }
                        _ => self.i += 1,
                    }
                }
                continue;
            }

            let tok = if c.is_ascii_digit() || (c == '-' && unary && next.is_some_and(|n| n.is_ascii_digit())) {
                self.number()?
            } else if c == '"' {
                self.i += 1;
                let (text, _) = self.quoted('"', start, false, true, false)?;
                Tok::Str(text)
            } else if c == '`' {
                self.template(start)?
            } else if c == '\'' {
                self.i += 1;
                if self.peek() == Some('\'') {
                    self.i += 1;
                    return Err(self.error("`''` holds no character", start));
                }
                let (text, _) = self.quoted('\'', start, false, false, false)?;
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
            } else if c.is_ascii_alphabetic() || c == '_' {
                self.word(start)?
            } else if c == '!' && next == Some('i') && self.peek_at(2) == Some('n') {
                // `!in`, unless it is `!` before a name that starts `in`.
                if self.peek_at(3).is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                    self.i += 1;
                    Tok::Punct("!")
                } else {
                    self.i += 3;
                    Tok::Punct("!in")
                }
            } else if let Some(p) = self.punct() {
                // `+` and `-` read the same whichever side of an operand they
                // are on; only a following digit matters, handled above.
                p
            } else {
                self.i += 1;
                return Err(self.error(format!("`{c}` cannot appear here"), start));
            };
            return Ok(Some(Token { tok, span: Span::new(start, self.here()) }));
        }
    }

    fn punct(&mut self) -> Option<Tok> {
        let rest = &self.src[self.here()..];
        // `# ` and `# {` are reserved in the fork; `#` alone covers both here,
        // since neither means anything.
        let p = PUNCT.iter().find(|p| rest.starts_with(**p))?;
        self.i += p.chars().count();
        Some(Tok::Punct(p))
    }

    fn word(&mut self, start: usize) -> Result<Tok, SyntaxError> {
        while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            self.i += 1;
        }
        let word = &self.src[start..self.here()];
        if let Some(k) = KEYWORDS.iter().find(|k| **k == word) {
            return Ok(Tok::Kw(k));
        }
        if RESERVED_WORDS.contains(&word) {
            return Ok(Tok::Reserved(word.to_string()));
        }
        // A name needs a letter before any digit: `_1` and `__` are not names.
        let mut seen_letter = false;
        for c in word.chars() {
            if c.is_ascii_alphabetic() {
                seen_letter = true;
            } else if c != '_' && !seen_letter {
                return Err(self.error(format!("`{word}` is not a name: a name needs a letter before any digit"), start));
            }
        }
        if !seen_letter {
            return Err(self.error(format!("`{word}` is not a name: a name needs a letter"), start));
        }
        Ok(Tok::Ident(word.to_string()))
    }

    fn number(&mut self) -> Result<Tok, SyntaxError> {
        let start = self.here();
        let mut text = String::new();
        if self.peek() == Some('-') {
            text.push('-');
            self.i += 1;
        }
        let first = self.peek().unwrap();
        text.push(first);
        self.i += 1;
        let mut radix: Option<u32> = None;
        let mut has_period = false;
        let mut has_e = false;
        let digits_len = |t: &str| t.trim_start_matches('-').len();
        while let Some(c) = self.peek() {
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
                match self.peek_at(1) {
                    Some(d) if d.is_ascii_digit() => {
                        text.push('.');
                        self.i += 1;
                        has_period = true;
                    }
                    Some('_') | Some('.') | None => break,
                    // `1.)` and `1. ` are the float `1.0`; `1.foo` is a call.
                    Some(d) if !d.is_ascii_alphabetic() => {
                        text.push_str(".0");
                        self.i += 1;
                        has_period = true;
                    }
                    _ => break,
                }
            } else if (c == 'e' || c == 'E') && !has_e && radix.is_none() {
                match self.peek_at(1) {
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
    fn quoted(
        &mut self,
        term: char,
        start: usize,
        verbatim: bool,
        line_continuation: bool,
        interpolation: bool,
    ) -> Result<(String, bool), SyntaxError> {
        let start_col = if line_continuation { self.column_of(self.index_of(start)) } else { 0 };
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
            self.i += 1;

            if interpolation && !escape {
                if c == '$' && self.peek() == Some('{') {
                    return Ok((out, true));
                }
                if c == '\\' && self.peek() == Some('$') {
                    self.i += 1;
                    if self.peek() == Some('{') {
                        c = '$';
                    } else {
                        self.i -= 1;
                    }
                }
            }

            if c == term && !escape {
                if self.peek() == Some(term) {
                    self.i += 1;
                } else {
                    return Ok((out, false));
                }
            }

            match c {
                '\r' if self.peek() == Some('\n') => {}
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

    fn index_of(&self, byte: usize) -> usize {
        self.chars.partition_point(|&(b, _)| b < byte)
    }

    /// A backtick string, the backtick not yet consumed.
    fn template(&mut self, start: usize) -> Result<Tok, SyntaxError> {
        self.i += 1;
        // A string that opens at the end of a line starts on the next one.
        if self.peek() == Some('\r') {
            self.i += 1;
            if self.peek() == Some('\n') {
                self.i += 1;
            }
        } else if self.peek() == Some('\n') {
            self.i += 1;
        }
        let mut parts = Vec::new();
        loop {
            let text_start = self.here();
            let (text, interpolated) = self.quoted('`', start, true, false, true)?;
            parts.push(TplPart::Text(text, Span::new(text_start, self.here())));
            if !interpolated {
                break;
            }
            let brace = self.here();
            self.i += 1; // the `{`
            let mut tokens = self.tokens(true)?;
            let end = self.here();
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

    fn toks(src: &str) -> Vec<Tok> {
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
