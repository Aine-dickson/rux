//! A minimal TextMate grammar interpreter.
//!
//! Rux has exactly one description of its own syntax colouring,
//! `rux.tmLanguage.json`: and it already feeds two consumers: the VS Code
//! extension, and the code fences on the site (via Zola). This crate makes the
//! browser playground a third, rather than letting it grow a hand-written
//! tokenizer that would drift from the other two.
//!
//! It is deliberately not a general TextMate engine. It implements the subset
//! the Rux grammar actually uses:
//!
//! - `patterns`, and `include` of a `#repository` rule
//! - `match` with `captures`
//! - `begin` / `end` with `beginCaptures` / `endCaptures` and `contentName`
//!
//! and nothing else, no injections, no `while`, no `$self` / `$base`, no
//! backreferences from `begin` into `end`, and no cross-grammar includes such as
//! `source.css`. The Rux grammar is self-contained and uses none of them. If a
//! future grammar edit reaches for one, loading fails loudly rather than
//! silently mis-colouring, and this is the file to extend.
//!
//! Matching goes through `ferroni`, a pure-Rust Oniguruma, the same engine
//! family the real TextMate implementations use, so the regex dialect in the
//! grammar behaves the way it does in VS Code. Pure Rust is not incidental: the
//! C-backed alternatives cannot target wasm, and the browser is the whole reason
//! this crate exists.

use std::collections::HashMap;

use ferroni::prelude::{Scanner, ScannerFindOptions};
use serde::Deserialize;

// ── Grammar JSON ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RawGrammar {
    #[serde(default)]
    patterns: Vec<RawRule>,
    #[serde(default)]
    repository: HashMap<String, RawRule>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RawRule {
    name: Option<String>,
    content_name: Option<String>,
    #[serde(rename = "match")]
    match_: Option<String>,
    begin: Option<String>,
    end: Option<String>,
    #[serde(default)]
    captures: HashMap<String, ScopeName>,
    #[serde(default)]
    begin_captures: HashMap<String, ScopeName>,
    #[serde(default)]
    end_captures: HashMap<String, ScopeName>,
    #[serde(default)]
    patterns: Vec<RawRule>,
    include: Option<String>,
}

#[derive(Deserialize, Clone)]
struct ScopeName {
    name: Option<String>,
}

// ── Compiled form ────────────────────────────────────────────────────────────

/// Capture group index → scope name, ordered by group.
type Captures = Vec<(usize, String)>;

enum Kind {
    /// A pattern that matches within one line.
    Match { name: Option<String>, captures: Captures },
    /// A region delimited by `begin` and `end`, which may span lines.
    Block {
        name: Option<String>,
        content_name: Option<String>,
        begin_captures: Captures,
        end_captures: Captures,
        /// Index into `Grammar::contexts`, entered once `begin` matches.
        context: usize,
    },
}

struct Rule {
    kind: Kind,
    /// The rule's own trigger: `match`, or `begin` for a block.
    pattern: String,
}

/// A set of alternatives scanned together.
struct Context {
    /// Rule indices, in scanner order (after the `end` slot if `has_end`).
    rules: Vec<usize>,
    /// Whether scanner pattern 0 is this context's `end`.
    has_end: bool,
    /// Patterns in scanner order, kept so the scanner can be built on demand.
    sources: Vec<String>,
    /// Built on first entry: compiling Oniguruma is not cheap, and a document
    /// typically enters only a few of a grammar's contexts.
    scanner: Option<Scanner>,
}

pub struct Grammar {
    rules: Vec<Rule>,
    contexts: Vec<Context>,
}

/// A run of source text and the CSS class it should carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    /// `None` for text the grammar gave no scope.
    pub class: Option<&'static str>,
}

const ROOT: usize = 0;

impl Grammar {
    /// Compile a TextMate grammar from its JSON.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let raw: RawGrammar =
            serde_json::from_str(json).map_err(|e| format!("grammar JSON: {e}"))?;

        let mut g = Grammar { rules: Vec::new(), contexts: Vec::new() };
        // Reserve the root slot so context indices allocated during compilation
        // never collide with it.
        g.contexts.push(Context {
            rules: Vec::new(),
            has_end: false,
            sources: Vec::new(),
            scanner: None,
        });

        let mut resolved = HashMap::new();
        let rules = g.compile_patterns(&raw.patterns, &raw.repository, &mut resolved, &mut Vec::new())?;
        let sources = rules.iter().map(|&i| g.rules[i].pattern.clone()).collect();
        g.contexts[ROOT] = Context { rules, has_end: false, sources, scanner: None };
        Ok(g)
    }

    /// Flatten a `patterns` array into rule indices, following `include`s.
    fn compile_patterns(
        &mut self,
        patterns: &[RawRule],
        repo: &HashMap<String, RawRule>,
        resolved: &mut HashMap<String, Vec<usize>>,
        stack: &mut Vec<String>,
    ) -> Result<Vec<usize>, String> {
        let mut out = Vec::new();
        for raw in patterns {
            if let Some(inc) = &raw.include {
                let key = inc.strip_prefix('#').ok_or_else(|| {
                    format!(
                        "unsupported include {inc:?}: this interpreter resolves only \
                         `#repository` references, not other grammars or $self/$base"
                    )
                })?;
                if let Some(done) = resolved.get(key) {
                    out.extend(done.iter().copied());
                    continue;
                }
                // A repository rule can reach itself. Contributing nothing on
                // re-entry is where a recursive descent would bottom out anyway,
                // and it keeps compilation terminating.
                if stack.iter().any(|s| s == key) {
                    continue;
                }
                let target = repo
                    .get(key)
                    .ok_or_else(|| format!("include #{key} has no entry in the repository"))?;
                stack.push(key.to_string());
                let ids = self.compile_rule(target, repo, resolved, stack);
                stack.pop();
                let ids = ids?;
                resolved.insert(key.to_string(), ids.clone());
                out.extend(ids);
            } else {
                out.extend(self.compile_rule(raw, repo, resolved, stack)?);
            }
        }
        Ok(out)
    }

    /// Compile one rule. A bare `patterns` container expands to several.
    fn compile_rule(
        &mut self,
        raw: &RawRule,
        repo: &HashMap<String, RawRule>,
        resolved: &mut HashMap<String, Vec<usize>>,
        stack: &mut Vec<String>,
    ) -> Result<Vec<usize>, String> {
        if let Some(pattern) = &raw.match_ {
            let id = self.rules.len();
            self.rules.push(Rule {
                kind: Kind::Match { name: raw.name.clone(), captures: captures_of(&raw.captures) },
                pattern: pattern.clone(),
            });
            return Ok(vec![id]);
        }

        if let (Some(begin), Some(end)) = (&raw.begin, &raw.end) {
            // Reserve this rule's index before compiling children, so a nested
            // include that loops back sees a stable slot.
            let id = self.rules.len();
            self.rules.push(Rule {
                kind: Kind::Match { name: None, captures: Vec::new() },
                pattern: begin.clone(),
            });

            let inner = self.compile_patterns(&raw.patterns, repo, resolved, stack)?;
            let mut sources = vec![end.clone()];
            sources.extend(inner.iter().map(|&i| self.rules[i].pattern.clone()));

            let context = self.contexts.len();
            self.contexts.push(Context { rules: inner, has_end: true, sources, scanner: None });

            self.rules[id].kind = Kind::Block {
                name: raw.name.clone(),
                content_name: raw.content_name.clone(),
                begin_captures: captures_of(&raw.begin_captures),
                end_captures: captures_of(&raw.end_captures),
                context,
            };
            return Ok(vec![id]);
        }

        if raw.begin.is_some() || raw.end.is_some() {
            return Err("a rule has `begin` without `end`, or the reverse".into());
        }
        if !raw.patterns.is_empty() {
            return self.compile_patterns(&raw.patterns, repo, resolved, stack);
        }
        Err("a rule has none of `match`, `begin`/`end`, `include` or `patterns`".into())
    }

    fn scanner_for(&mut self, ctx: usize) -> Result<&mut Scanner, String> {
        if self.contexts[ctx].scanner.is_none() {
            let refs: Vec<&str> = self.contexts[ctx].sources.iter().map(String::as_str).collect();
            let scanner = Scanner::new(&refs).map_err(|e| format!("compiling patterns: {e}"))?;
            self.contexts[ctx].scanner = Some(scanner);
        }
        Ok(self.contexts[ctx].scanner.as_mut().expect("just built"))
    }

    /// Tokenize `text` into non-overlapping spans covering it end to end.
    pub fn spans(&mut self, text: &str) -> Vec<Span> {
        let mut out = Vec::new();
        let mut stack = vec![Frame { context: ROOT, base: None, block: None }];

        // Blocks span lines, so the stack persists across them; patterns
        // themselves never match past a newline, which is why scanning is
        // per-line at all.
        let mut line_start = 0usize;
        for line in text.split_inclusive('\n') {
            self.scan_line(line, line_start, &mut stack, &mut out);
            line_start += line.len();
        }
        merge(out, text.len())
    }

    fn scan_line(
        &mut self,
        line: &str,
        line_start: usize,
        stack: &mut Vec<Frame>,
        out: &mut Vec<Span>,
    ) {
        let mut pos = 0usize;
        loop {
            let frame = *stack.last().expect("stack is never empty");
            let (ctx, base) = (frame.context, frame.base);
            let has_end = self.contexts[ctx].has_end;

            let found = match self.scanner_for(ctx) {
                Ok(scanner) => scanner.find_next_match(line, pos, ScannerFindOptions::NONE),
                // A pattern that will not compile disables colouring for this
                // context rather than losing the text.
                Err(_) => None,
            };
            let Some(m) = found else {
                push(out, line_start + pos, line_start + line.len(), base);
                return;
            };

            let whole = &m.capture_indices[0];
            push(out, line_start + pos, line_start + whole.start, base);

            let is_end = has_end && m.index == 0;
            let rule_id = if is_end { None } else { Some(self.contexts[ctx].rules[m.index - usize::from(has_end)]) };

            match rule_id {
                None => {
                    // Closing the current block. The end match still belongs to
                    // the block, so it is emitted before popping, and its scope
                    // is the block's own `name`, not the content scope, which is
                    // why the frame remembers which rule opened it.
                    let (caps, fallback) = match frame.block.map(|b| &self.rules[b].kind) {
                        Some(Kind::Block { end_captures, name, .. }) => {
                            (end_captures.clone(), class_of(name.as_deref()).or(base))
                        }
                        _ => (Vec::new(), base),
                    };
                    emit(out, line_start, &m, &caps, fallback);
                    stack.pop();
                }
                Some(id) => match &self.rules[id].kind {
                    Kind::Match { name, captures } => {
                        let fallback = class_of(name.as_deref()).or(base);
                        let captures = captures.clone();
                        emit(out, line_start, &m, &captures, fallback);
                    }
                    Kind::Block { name, content_name, begin_captures, context, .. } => {
                        let fallback = class_of(name.as_deref()).or(base);
                        let inner_base = class_of(content_name.as_deref())
                            .or(class_of(name.as_deref()))
                            .or(base);
                        let (caps, ctx_next) = (begin_captures.clone(), *context);
                        emit(out, line_start, &m, &caps, fallback);
                        stack.push(Frame { context: ctx_next, base: inner_base, block: Some(id) });
                    }
                },
            }

            // A zero-width match would spin forever, so step past one character,
            // but *emit* that character, or it vanishes from the output. This is
            // how a line comment ending on `$` used to eat the newline: `$`
            // matches before `\n`, so the skipped char was the line terminator
            // itself, and only files with CRLF made it visible.
            pos = if whole.end > whole.start {
                whole.end
            } else {
                match line[whole.end..].chars().next() {
                    Some(c) => {
                        let next = whole.end + c.len_utf8();
                        let base_now = stack.last().expect("stack is never empty").base;
                        push(out, line_start + whole.end, line_start + next, base_now);
                        next
                    }
                    None => return,
                }
            };
            if pos >= line.len() {
                push(out, line_start + pos, line_start + line.len(), stack.last().unwrap().base);
                return;
            }
        }
    }
}

/// One level of block nesting.
#[derive(Clone, Copy)]
struct Frame {
    context: usize,
    /// Class applied to text inside this frame that no capture claimed.
    base: Option<&'static str>,
    /// The block rule that opened this frame, so its `end` captures can be found
    /// when it closes. `None` only for the root.
    block: Option<usize>,
}

fn captures_of(map: &HashMap<String, ScopeName>) -> Captures {
    let mut out: Captures = map
        .iter()
        .filter_map(|(k, v)| Some((k.parse::<usize>().ok()?, v.name.clone()?)))
        .collect();
    out.sort_by_key(|(i, _)| *i);
    out
}

fn push(out: &mut Vec<Span>, start: usize, end: usize, class: Option<&'static str>) {
    if end > start {
        out.push(Span { start, end, class });
    }
}

/// Emit a matched region: one span per named capture, with everything else in
/// the region falling back to the rule's own scope.
fn emit(
    out: &mut Vec<Span>,
    line_start: usize,
    m: &ferroni::prelude::ScannerMatch,
    captures: &Captures,
    fallback: Option<&'static str>,
) {
    let whole = &m.capture_indices[0];
    let mut cursor = whole.start;
    for (group, scope) in captures {
        let Some(c) = m.capture_indices.get(*group) else { continue };
        if c.end <= c.start || c.start < cursor {
            continue;
        }
        push(out, line_start + cursor, line_start + c.start, fallback);
        push(out, line_start + c.start, line_start + c.end, class_of(Some(scope)));
        cursor = c.end;
    }
    push(out, line_start + cursor, line_start + whole.end, fallback);
}

/// Join neighbouring spans that share a class, and cover any gaps.
fn merge(spans: Vec<Span>, len: usize) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for s in spans {
        match out.last_mut() {
            Some(prev) if prev.class == s.class && prev.end == s.start => prev.end = s.end,
            _ => out.push(s),
        }
    }
    if let Some(last) = out.last() {
        if last.end < len {
            out.push(Span { start: out[out.len() - 1].end, end: len, class: None });
        }
    } else if len > 0 {
        out.push(Span { start: 0, end: len, class: None });
    }
    out
}

// ── Scope → CSS class ────────────────────────────────────────────────────────

/// Longest-prefix-wins, so the specific CSS entries beat the general ones. The
/// classes are deliberately few: a playground needs the code to be *readable*,
/// not to reproduce a full editor theme.
const SCOPES: &[(&str, &str)] = &[
    ("comment", "hl-comment"),
    ("string", "hl-string"),
    ("constant.character.escape", "hl-escape"),
    ("constant.numeric", "hl-number"),
    ("constant.other.color", "hl-number"),
    ("constant.language", "hl-const"),
    ("keyword.operator", "hl-operator"),
    ("keyword", "hl-keyword"),
    ("storage", "hl-keyword"),
    ("entity.name.tag", "hl-tag"),
    ("entity.name.function", "hl-function"),
    // `use components::header;`. The grammar gives this its own scope so a real
    // editor theme can colour a namespace as a namespace; here it takes the tag
    // colour, because what the path actually names is the tag `<header>`, and
    // this palette has no separate namespace colour worth adding one for.
    ("entity.name.namespace", "hl-tag"),
    ("entity.other.attribute-name.class", "hl-selector"),
    ("entity.other.attribute-name.id", "hl-selector"),
    ("entity.other.attribute-name.pseudo-class", "hl-selector"),
    ("entity.other.attribute-name.directive", "hl-directive"),
    ("entity.other.attribute-name", "hl-attr"),
    ("support.function", "hl-function"),
    ("support.type.property-name", "hl-property"),
    ("support.constant", "hl-value"),
    ("meta.interpolation", "hl-interp"),
    ("punctuation", "hl-punct"),
    // A comment's `//` or `<!--`, and a string's quotes, belong to the thing
    // they delimit, colouring them as generic punctuation makes a comment look
    // half-commented. These beat the bare `punctuation` entry on length.
    ("punctuation.definition.comment", "hl-comment"),
    ("punctuation.definition.string", "hl-string"),
    ("punctuation.section.interpolation", "hl-interp"),
    ("variable", "hl-var"),
];

fn class_of(scope: Option<&str>) -> Option<&'static str> {
    let scope = scope?;
    SCOPES
        .iter()
        .filter(|(prefix, _)| scope.starts_with(prefix))
        .max_by_key(|(prefix, _)| prefix.len())
        .map(|(_, class)| *class)
}

/// Render `text` as HTML, one `<span class="hl-…">` per scoped run.
pub fn to_html(grammar: &mut Grammar, text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for span in grammar.spans(text) {
        let slice = &text[span.start..span.end];
        match span.class {
            Some(class) => {
                out.push_str("<span class=\"");
                out.push_str(class);
                out.push_str("\">");
                escape_into(slice, &mut out);
                out.push_str("</span>");
            }
            None => escape_into(slice, &mut out),
        }
    }
    out
}

fn escape_into(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}
