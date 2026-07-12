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

/// A parse failure with a human-readable reason.
#[derive(Debug, Clone)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "parse error: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

/// Parse a full `.rux` source into an [`Sfc`].
pub fn parse_sfc(src: &str) -> Result<Sfc, ParseError> {
    let template_src = section(src, "template")
        .ok_or_else(|| ParseError("missing <template> section".into()))?;
    let style = section(src, "style").unwrap_or_default();
    let script = section(src, "script").unwrap_or_default();

    let mut parser = Parser::new(&template_src);
    let nodes = parser.parse_nodes(None)?;
    let template = nodes
        .into_iter()
        .find_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
        .ok_or_else(|| ParseError("<template> has no root element".into()))?;

    Ok(Sfc {
        template,
        style: style.trim().to_string(),
        script: script.trim().to_string(),
    })
}

/// Extract the inner text of a top-level `<name> … </name>` section.
fn section(src: &str, name: &str) -> Option<String> {
    let open = format!("<{name}");
    let start = src.find(&open)?;
    // Advance past the opening tag's closing `>`.
    let after_open = start + src[start..].find('>')? + 1;
    let close = format!("</{name}>");
    let end = src[after_open..].find(&close)? + after_open;
    Some(src[after_open..end].to_string())
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
            return Err(ParseError(format!("expected tag name at position {}", self.pos)));
        }

        let mut attrs = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                None => return Err(ParseError(format!("unclosed tag <{tag}>"))),
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
                        return Err(ParseError(format!(
                            "malformed attribute in <{tag}> at position {}",
                            self.pos
                        )));
                    }
                    self.skip_ws();
                    let value = if self.peek() == Some('=') {
                        self.bump();
                        self.skip_ws();
                        self.read_attr_value()
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
            return Err(ParseError(format!("expected </{tag}>")));
        }
        self.pos += 2;
        let close = self.read_name();
        if close != tag {
            return Err(ParseError(format!(
                "mismatched closing tag: expected </{tag}>, found </{close}>"
            )));
        }
        self.skip_ws();
        if self.peek() != Some('>') {
            return Err(ParseError(format!("unterminated </{tag}>")));
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
