//! Rux template parser — milestone M2.
//!
//! Two jobs, both hand-rolled (the one piece of the pipeline with no off-the-shelf
//! answer — see `docs/04-architecture.md`, Stage 1):
//!
//! 1. Split a `.rux` single-file component into its `<template>`, `<style>`, and
//!    `<script>` sections.
//! 2. Parse the template — an XML-shaped grammar that, unlike XML, must accept our
//!    attribute spellings (`@tap`, `:device`, `r-for`) and `{{ }}` interpolations.
//!
//! M2 keeps interpolations and directives as raw attribute/text strings; binding
//! compilation arrives with reactivity (M5).

use std::fmt;

/// Decode the HTML entities an author might write: the named ones (`&amp;`,
/// `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;`) and numeric (`&#38;`, `&#x26;`).
/// An unrecognised `&…;` is left as written.
///
/// Applied to **attribute values as they are parsed**, and by later stages to
/// text. Attributes need it because an attribute is delimited by the same `"` a
/// script expression uses for its string literals — so
/// `:class="if dark { &quot;dark&quot; } else { &quot;light&quot; } "` is the only
/// way to write one, and without decoding the engine would see the raw `&quot;`
/// and fail to parse.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp..];
        // An entity is short and has no spaces; only look at the next few chars.
        if let Some(semi) = after[1..].find(';').map(|i| i + 1) {
            if semi <= 12 {
                if let Some(ch) = entity_char(&after[1..semi]) {
                    out.push(ch);
                    rest = &after[semi + 1..];
                    continue;
                }
            }
        }
        out.push('&');
        rest = &after[1..];
    }
    out.push_str(rest);
    out
}

fn entity_char(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{00A0}'),
        _ => {
            let num = entity.strip_prefix('#')?;
            let code = match num.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => num.parse::<u32>().ok()?,
            };
            char::from_u32(code)
        }
    }
}

/// A parsed single-file component. `style`/`script` are raw source for later
/// stages; `template` is the parsed root element.
#[derive(Debug, Clone)]
pub struct Sfc {
    pub template: Element,
    pub style: String,
    pub script: String,
}

/// An element node: a tag, its attributes (in source order), and its children.
#[derive(Debug, Clone)]
pub struct Element {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

/// A node in the template tree.
#[derive(Debug, Clone)]
pub enum Node {
    Element(Element),
    Text(String),
}

impl Element {
    /// Value of an attribute by exact name, if present.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Whitespace-separated `class` tokens.
    pub fn classes(&self) -> Vec<&str> {
        self.attr("class")
            .map(|s| s.split_whitespace().collect())
            .unwrap_or_default()
    }

    pub fn id(&self) -> Option<&str> {
        self.attr("id")
    }

    pub fn role(&self) -> Option<&str> {
        self.attr("role")
    }
}

/// A parse failure: what went wrong and, when known, where.
///
/// The position is 1-based and relative to the **whole `.rux` file**, not the
/// `<template>` section it was found in, so it can be read straight off against
/// an editor's gutter.
#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl ParseError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), line: None, column: None }
    }

    pub fn at(message: impl Into<String>, line: usize, column: usize) -> Self {
        Self { message: message.into(), line: Some(line), column: Some(column) }
    }

    /// Shift a template-relative position onto the file's own line numbering.
    fn offset_lines(mut self, by: usize) -> Self {
        self.line = self.line.map(|l| l + by);
        self
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(l), Some(c)) => write!(f, "parse error at line {l}, column {c}: {}", self.message),
            _ => write!(f, "parse error: {}", self.message),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse a full `.rux` source into an [`Sfc`].
pub fn parse_sfc(src: &str) -> Result<Sfc, ParseError> {
    let (template_src, template_start) =
        section(src, "template").ok_or_else(|| ParseError::new("missing <template> section"))?;
    let style = section(src, "style").map(|(s, _)| s).unwrap_or_default();
    let script = section(src, "script").map(|(s, _)| s).unwrap_or_default();

    // Positions inside the template are relative to the section; shift them onto
    // the file's lines so a reported line matches the editor's gutter.
    let lines_before = src[..template_start].matches('\n').count();

    let mut parser = Parser::new(&template_src);
    let nodes = parser.parse_nodes(None).map_err(|e| e.offset_lines(lines_before))?;
    let template = nodes
        .into_iter()
        .find_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
        .ok_or_else(|| ParseError::new("<template> has no root element"))?;

    Ok(Sfc {
        template,
        style: style.trim().to_string(),
        script: script.trim().to_string(),
    })
}

/// Extract the inner text of a top-level `<name> … </name>` section, with the
/// byte offset it starts at (so errors inside it can be reported against the
/// file's own line numbers).
fn section(src: &str, name: &str) -> Option<(String, usize)> {
    let open = format!("<{name}");
    let start = src.find(&open)?;
    // Advance past the opening tag's closing `>`.
    let after_open = start + src[start..].find('>')? + 1;
    let close = format!("</{name}>");
    let end = src[after_open..].find(&close)? + after_open;
    Some((src[after_open..end].to_string(), after_open))
}

/// A small recursive-descent parser over the template characters.
struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn new(s: &str) -> Self {
        Self {
            chars: s.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    /// 1-based line and column of `pos` within the template section.
    fn line_col(&self, pos: usize) -> (usize, usize) {
        let mut line = 1;
        let mut col = 1;
        for &c in &self.chars[..pos.min(self.chars.len())] {
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    /// An error carrying the position the parser has reached — which is where the
    /// author needs to look, and the whole point of surfacing errors at all.
    fn err(&self, message: impl Into<String>) -> ParseError {
        let (line, col) = self.line_col(self.pos);
        ParseError::at(message, line, col)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn starts_with(&self, s: &str) -> bool {
        let sc: Vec<char> = s.chars().collect();
        if self.pos + sc.len() > self.chars.len() {
            return false;
        }
        self.chars[self.pos..self.pos + sc.len()] == sc[..]
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }

    /// Parse sibling nodes until end-of-input or an unmatched `</`.
    fn parse_nodes(&mut self, parent: Option<&str>) -> Result<Vec<Node>, ParseError> {
        let mut nodes = Vec::new();
        loop {
            if self.peek().is_none() {
                break;
            }
            if self.starts_with("<!--") {
                self.skip_comment();
                continue;
            }
            if self.starts_with("</") {
                break; // closing tag — caller consumes it
            }
            if self.peek() == Some('<') {
                let el = self.parse_element()?;
                nodes.push(Node::Element(el));
                continue;
            }
            // Text run up to the next '<'.
            let text = self.read_text();
            if !text.trim().is_empty() {
                nodes.push(Node::Text(text.trim().to_string()));
            }
        }
        let _ = parent;
        Ok(nodes)
    }

    fn skip_comment(&mut self) {
        // Assumes current position is at "<!--".
        self.pos += 4;
        while self.peek().is_some() && !self.starts_with("-->") {
            self.pos += 1;
        }
        if self.starts_with("-->") {
            self.pos += 3;
        }
    }

    fn read_text(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c == '<' {
                break;
            }
            s.push(c);
            self.pos += 1;
        }
        s
    }

    fn read_name(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                s.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        s
    }

    /// An attribute name may include our sigils: `@tap`, `:device`, `r-for`.
    fn read_attr_name(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_whitespace() || c == '=' || c == '>' || c == '/' {
                break;
            }
            s.push(c);
            self.pos += 1;
        }
        s
    }

    fn parse_element(&mut self) -> Result<Element, ParseError> {
        self.bump(); // consume '<'
        let tag = self.read_name();
        if tag.is_empty() {
            return Err(self.err("expected a tag name after `<`"));
        }

        let mut attrs = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                None => return Err(self.err(format!("unclosed tag <{tag}>"))),
                Some('>') => {
                    self.bump();
                    let children = self.parse_nodes(Some(&tag))?;
                    self.expect_closing(&tag)?;
                    return Ok(Element { tag, attrs, children });
                }
                Some('/') if self.starts_with("/>") => {
                    self.pos += 2;
                    return Ok(Element { tag, attrs, children: Vec::new() });
                }
                _ => {
                    let name = self.read_attr_name();
                    if name.is_empty() {
                        return Err(self.err(format!("malformed attribute in <{tag}>")));
                    }
                    self.skip_ws();
                    let value = if self.peek() == Some('=') {
                        self.bump();
                        self.skip_ws();
                        // Decoded here: an attribute is quoted with the same `"`
                        // a script expression needs for its own string literals,
                        // so `&quot;` is how you write one.
                        decode_entities(&self.read_attr_value())
                    } else {
                        String::new() // valueless attribute, e.g. `disabled`
                    };
                    attrs.push((name, value));
                }
            }
        }
    }

    fn read_attr_value(&mut self) -> String {
        match self.peek() {
            Some(q @ '"') | Some(q @ '\'') => {
                self.bump();
                let mut s = String::new();
                while let Some(c) = self.peek() {
                    if c == q {
                        self.bump();
                        break;
                    }
                    s.push(c);
                    self.pos += 1;
                }
                s
            }
            _ => {
                // Unquoted value: read to whitespace or tag end.
                let mut s = String::new();
                while let Some(c) = self.peek() {
                    if c.is_whitespace() || c == '>' || c == '/' {
                        break;
                    }
                    s.push(c);
                    self.pos += 1;
                }
                s
            }
        }
    }

    fn expect_closing(&mut self, tag: &str) -> Result<(), ParseError> {
        self.skip_ws();
        if !self.starts_with("</") {
            return Err(self.err(format!("expected </{tag}>")));
        }
        self.pos += 2;
        let close = self.read_name();
        if close != tag {
            return Err(self.err(format!(
                "mismatched closing tag: expected </{tag}>, found </{close}>"
            )));
        }
        self.skip_ws();
        if self.peek() != Some('>') {
            return Err(self.err(format!("unterminated </{tag}>")));
        }
        self.bump();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections_and_tree() {
        let src = r#"
            <template>
              <screen class="a">
                <view class="card" @tap="go()">
                  <text>Hello {{ name }}</text>
                </view>
              </screen>
            </template>
            <style> .a { color: red; } </style>
            <script> let name = signal("x"); </script>
        "#;
        let sfc = parse_sfc(src).expect("parse");
        assert_eq!(sfc.template.tag, "screen");
        assert_eq!(sfc.template.classes(), vec!["a"]);
        let card = match &sfc.template.children[0] {
            Node::Element(e) => e,
            _ => panic!("expected element"),
        };
        assert_eq!(card.tag, "view");
        assert_eq!(card.attr("@tap"), Some("go()"));
        assert!(sfc.style.contains("color: red"));
        assert!(sfc.script.contains("signal"));
    }

    #[test]
    fn self_closing_and_comments() {
        let src = r#"<template><view><!-- c --><input type="text" /></view></template>"#;
        let sfc = parse_sfc(src).unwrap();
        let input = match &sfc.template.children[0] {
            Node::Element(e) => e,
            _ => panic!(),
        };
        assert_eq!(input.tag, "input");
        assert_eq!(input.attr("type"), Some("text"));
    }
}
