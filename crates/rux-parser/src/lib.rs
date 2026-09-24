//! Rux template parser, milestone M2.
//!
//! Two jobs, both hand-rolled (the one piece of the pipeline with no off-the-shelf
//! answer, see `docs/04-architecture.md`, Stage 1):
//!
//! 1. Split a `.rux` single-file component into its `<template>`, `<style>`, and
//!    `<script>` sections.
//! 2. Parse the template, an XML-shaped grammar that, unlike XML, must accept our
//!    attribute spellings (`@tap`, `:device`, `r-for`) and `{{ }}` interpolations.
//!
//! M2 keeps interpolations and directives as raw attribute/text strings; binding
//! compilation arrives with reactivity (M5).

use std::fmt;

/// Tags that hold no children, so an opening tag closes itself and a closing
/// tag is not merely unnecessary but wrong.
///
/// `<input type="text">` is the one an author writes by hand, and until this
/// list reached the parser it was a parse error: the template demanded an
/// `</input>` that Rux has no such thing as, and the message named the *next*
/// closing tag it found instead, so `<view><input></view>` was reported as
/// "expected </input>, found </view>" against a document whose only mistake was
/// being written the way HTML is.
///
/// The same set the formatter indents by and the editor auto-closes by. It
/// lived in `rux-fmt` first, which is why that crate now re-exports this one
/// rather than keeping a second copy: the list drifted once already between
/// Rust and JavaScript (HTML's `img` against Rux's `<image>`), and a third copy
/// is how it would drift again.
const VOID_TAGS: &[&str] = &[
    // `<router-view />` never nests: what goes in it comes from the route
    // matched below, not from anything written between the tags. `<path>` holds
    // its geometry in an attribute, so it has nothing to nest.
    "image", "input", "path", "router-view", //
    "area", "base", "br", "col", "embed", "hr", "img", "link", "meta", "param", "source", "track",
    "wbr",
];

/// Every tag Rux itself defines. Anything else in a template is a component,
/// which is to say somebody's file.
///
/// Held here, beside [`VOID_TAGS`], because both answer the same kind of
/// question about a name and both have to agree with `rux vocab`: the editor's
/// completion list and the runtime's unknown-tag error are two readings of one
/// list, and a second copy is how `<image>` came to be missing from one of them.
/// `rux-cli` has a test that the vocabulary it prints matches this exactly.
///
/// `router-view` is here and is not in the vocabulary's element list, because it
/// is documented with the router rather than on its own. It is still a tag Rux
/// defines, which is what this list is for.
const ELEMENT_TAGS: &[&str] = &[
    "screen", "view", "text", "image", "path", "button", "input", //
    "slot", "router", "route", "router-view",
];

/// Whether `tag` is one of Rux's own elements rather than a component.
pub fn is_element(tag: &str) -> bool {
    ELEMENT_TAGS.contains(&tag)
}

/// The tags in [`is_element`], for anything that has to agree about what Rux
/// defines: `rux vocab`, and the runtime's "no such tag" error.
pub fn element_tags() -> &'static [&'static str] {
    ELEMENT_TAGS
}

/// Attributes every element takes: identity, styling, accessibility, a link,
/// and the structural directives.
///
/// `label` is the accessible name. It was honored all along (the accessibility
/// tree reads it before an element's own text) and advertised nowhere.
const GLOBAL_ATTRIBUTES: &[&str] = &[
    "class", "id", "style", "role", "label", "to", //
    "r-for", "r-key", "r-if", "r-elif", "r-else", "r-show", "r-transition",
];

/// Attributes that mean something on one element only. `rux vocab` advertises
/// the same set to the editor, and `rux-cli` has a test that the two agree.
const ELEMENT_ATTRIBUTES: &[(&str, &[&str])] = &[
    ("image", &["src", "alt"]),
    ("path", &["d", "alt"]),
    (
        "input",
        &[
            "type", "placeholder", "value", "checked", "name", "required", "minlength",
            "pattern", "options", "disabled", "readonly", "min", "max", "step", "maxlength",
            "inputmode", "enterkeyhint", "autocomplete", "autofocus", "r-model",
        ],
    ),
    ("button", &["disabled", "type"]),
    ("route", &["path", "view", "fallback", "guard", "name"]),
    ("router", &["restore-scroll", "guard"]),
    ("text", &["for"]),
];

/// The attributes with a bound `:name` form, and where. Everything else is read
/// once, as written, so `:id="row + n"` used to be accepted and thrown away.
const BOUND_GLOBAL: &[&str] = &["class", "style", "to", "r-transition"];
const BOUND_ELEMENT: &[(&str, &[&str])] = &[
    ("image", &["src"]),
    ("path", &["d"]),
    ("input", &["options", "disabled", "readonly", "required"]),
    ("button", &["disabled"]),
];

/// Whether `<tag name>` means something to Rux, `name` written as it appears
/// (so `:src` is asked as `:src`). `@` listeners are not answered here: the
/// runtime checks those against the events it dispatches.
pub fn is_known_attribute(tag: &str, name: &str) -> bool {
    let of = |table: &[(&str, &'static [&'static str])], n: &str| {
        table.iter().any(|(t, names)| *t == tag && names.contains(&n))
    };
    match name.strip_prefix(':') {
        Some(bound) => BOUND_GLOBAL.contains(&bound) || of(BOUND_ELEMENT, bound),
        None => GLOBAL_ATTRIBUTES.contains(&name) || of(ELEMENT_ATTRIBUTES, name),
    }
}

/// Every attribute `<tag>` takes, globals first, for an error to list and for
/// `rux vocab` to agree with.
pub fn attributes_of(tag: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = GLOBAL_ATTRIBUTES.to_vec();
    if let Some((_, own)) = ELEMENT_ATTRIBUTES.iter().find(|(t, _)| *t == tag) {
        out.extend_from_slice(own);
    }
    out
}

/// Whether `name` has a bound `:name` form on `<tag>`.
pub fn has_bound_form(tag: &str, name: &str) -> bool {
    is_known_attribute(tag, &format!(":{name}"))
}

/// A `prop name;` or `prop name = default;` line from a component's `<script>`.
///
/// Carried on the [`Sfc`] rather than left in the script: a prop is not state
/// the component owns, it is an input its caller owes, so it cannot be a `let`
/// that a handler could write. The runtime fills this in when it strips the
/// lines; the parser leaves it empty, like [`Sfc::file`].
#[derive(Debug, Clone, PartialEq)]
pub struct PropDecl {
    /// As a script reads it, which is snake: `id_of`, written on a tag as
    /// either `:id-of` or `:id_of`.
    pub name: String,
    /// The type it was declared with, `prop label: string;`, as text. `None`
    /// when it was declared by name alone.
    pub ty: Option<String>,
    /// The expression used when the caller passes nothing. `None` means the
    /// caller must pass it.
    pub default: Option<String>,
    /// 1-based line within the `<script>` body.
    pub line: usize,
}

/// Whether `tag` is one that never takes children or a closing tag.
pub fn is_void(tag: &str) -> bool {
    VOID_TAGS.contains(&tag)
}

/// The tags in [`is_void`], for anything that has to agree with the parser
/// about what never nests: the formatter's indenter, and through `rux vocab`
/// the editor's tag auto-closing.
pub fn void_tags() -> &'static [&'static str] {
    VOID_TAGS
}

/// Decode the HTML entities an author might write: the named ones (`&amp;`,
/// `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;`) and numeric (`&#38;`, `&#x26;`).
/// An unrecognised `&…;` is left as written.
///
/// Applied to **attribute values as they are parsed**, and by later stages to
/// text. Attributes need it because an attribute is delimited by the same `"` a
/// script expression uses for its string literals, so
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
    /// The file this was parsed from, once somebody who has a filesystem says
    /// so. Parsing does no IO and is handed a string, so the parser always
    /// leaves this `None`; `rux-runtime` fills it in, the same arrangement
    /// [`Sfc::style_includes`] already uses.
    ///
    /// It exists so a warning raised while building an imported component can
    /// name the component's file rather than the document's. Errors have said
    /// so since components landed (`LoadError::file`); warnings did not, and a
    /// warning that names the wrong file is worse than one that names none,
    /// because it comes with a line number the reader will trust.
    pub file: Option<std::path::PathBuf>,
    pub template: Element,
    pub style: String,
    pub script: String,
    /// 1-based line in the *file* where `style`'s first character sits.
    ///
    /// The later stages parse `style` on its own, so everything they know a
    /// position for is relative to the section. Without this a warning would
    /// point at the wrong line of the file, which is worse than pointing
    /// nowhere: the reader trusts it and looks in the wrong place.
    pub style_line: usize,
    /// The same, for `script`.
    pub script_line: usize,
    /// Paths from `<style src="a.css, b.css">`, in the order written.
    ///
    /// Parsing does no IO, so this is only the request. Whoever has a
    /// filesystem (`rux-runtime`) reads the files and fills in
    /// [`Sfc::style_includes`]; a browser, which has neither a filesystem nor a
    /// path to be relative to, warns instead.
    pub style_src: Vec<String>,
    /// `<style scoped>`: keep these rules to this file's own markup.
    ///
    /// Without it, a document's rules also reach the components it uses, so a
    /// look can be written once at the top instead of imported into every
    /// component. `scoped` is the opt-out for a component that wants to own its
    /// appearance completely.
    pub style_scoped: bool,
    /// Included stylesheets, filled in after parsing by whoever resolved
    /// [`Sfc::style_src`]. Empty when nothing was included, which is the usual
    /// case.
    ///
    /// These cascade **before** the inline `<style>` body, so a document can
    /// override the palette it included without reaching for `!important`, the
    /// same way source order works in CSS.
    pub style_includes: Vec<StyleInclude>,
    /// The props this file declares with `prop`, when it is a component. See
    /// [`PropDecl`].
    pub props: Vec<PropDecl>,
    /// The file has a `<script>` and nothing else: it declares types for
    /// other files to `use`, and its template is an empty `<view>` standing in
    /// for the one it does not have.
    pub types_only: bool,
    /// Every declared type a prop of this file may name, as name and text:
    /// its own `type`s and what it brings in with `use types::X`, the types
    /// beside those included. Parsing does no IO and cannot follow a `use`,
    /// so this is empty until `rux-runtime` fills it in, the arrangement
    /// [`Sfc::style_includes`] uses. A prop's type is checked when the
    /// program runs, and a name in it needs these to mean anything.
    pub types: Vec<(String, String)>,
}

/// One resolved external stylesheet: where it came from, and what it said.
#[derive(Debug, Clone)]
pub struct StyleInclude {
    /// The path as written in `src`, for error messages. Not reopened.
    pub path: String,
    pub css: String,
}

/// An element node: a tag, its attributes (in source order), and its children.
#[derive(Debug, Clone)]
pub struct Element {
    pub tag: String,
    pub attrs: Vec<Attr>,
    pub children: Vec<Node>,
    /// The 1-based **file** line this element's `<` sits on.
    ///
    /// File-relative, not section-relative, so it lines up with the editor
    /// gutter without every reader having to know where `<template>` started.
    /// [`parse_sfc`] shifts the whole tree once, after parsing, the same way it
    /// already shifts a [`ParseError`].
    pub line: usize,
}

/// One attribute, with the line it was written on.
///
/// The line is carried per attribute rather than per element because elements
/// here are routinely written across several lines:
///
/// ```text
/// <view class="tile"
///       r-for="d in devices" @tap="select(d)">
/// ```
///
/// A warning about that `@tap` belongs on the second line, and an element-level
/// line would put it on the first. That is a smaller lie than the one this
/// replaces, but it is still a lie, and the cost of not telling it is one
/// `usize`.
#[derive(Debug, Clone)]
pub struct Attr {
    pub name: String,
    pub value: String,
    /// The 1-based **file** line, as [`Element::line`].
    pub line: usize,
    /// Whether an `=` was written at all.
    ///
    /// `r-else` and `r-else=""` both leave [`Attr::value`] empty, so without
    /// this the two are indistinguishable and a value given to an attribute
    /// that takes none cannot be reported. The directives this matters for
    /// (`r-else`, and `fallback` on a `<route>`) are exactly the ones an editor
    /// is most likely to complete *with* an `=""` it should not.
    pub has_value: bool,
}

/// A node in the template tree.
#[derive(Debug, Clone)]
pub enum Node {
    Element(Element),
    /// Text, and the 1-based **file** line it starts on. A `{{ }}` that fails
    /// is reported against this, which is why the text keeps its position even
    /// though nothing else about a text node needs one.
    Text(String, usize),
}

impl Element {
    /// Value of an attribute by exact name, if present.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|a| a.name == name).map(|a| a.value.as_str())
    }

    /// The line an attribute was written on, for a warning that is about it.
    pub fn attr_line(&self, name: &str) -> Option<usize> {
        self.attrs.iter().find(|a| a.name == name).map(|a| a.line)
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
    // A file holding only a `<script>` declares types for other files to
    // `use`, and has nothing to show. The runtime holds its script to that.
    let absent = |name| matches!(find_section(src, name), Err(SectionProblem::Absent));
    if absent("template") && absent("style") && find_section(src, "script").is_ok() {
        let (script, script_line) = trimmed_section(src, "script");
        return Ok(Sfc {
            file: None,
            template: Element { tag: "view".to_string(), attrs: Vec::new(), children: Vec::new(), line: 1 },
            style: String::new(),
            style_line: 1,
            script,
            script_line,
            style_src: Vec::new(),
            style_scoped: false,
            style_includes: Vec::new(),
            props: Vec::new(),
            types_only: true,
            types: Vec::new(),
        });
    }
    let (template_src, template_start, _) =
        find_section(src, "template").map_err(|why| section_error(src, "template", why))?;
    // A `<style>` or `<script>` that is absent is fine and common. One that is
    // *present and broken* is not: before this, an unclosed `<style>` dropped
    // every rule in the file with nothing said, which looks exactly like CSS
    // that does not work.
    for name in ["style", "script"] {
        if let Err(why) = find_section(src, name) {
            if why != SectionProblem::Absent {
                return Err(section_error(src, name, why));
            }
        }
    }
    let (style, style_line) = trimmed_section(src, "style");
    let (script, script_line) = trimmed_section(src, "script");
    let style_open = section_with_open(src, "style").map(|(_, _, open)| open);
    let style_src = style_open
        .as_deref()
        .and_then(|open| open_tag_attr(open, "src"))
        .map(|v| split_src(&v))
        .unwrap_or_default();
    // `<style scoped>` keeps a document's rules to its own markup. Without it
    // they reach the components it uses, which is the default because a shared
    // look is the common case and repeating an import in every component was
    // the thing people actually hit.
    let style_scoped = style_open.as_deref().is_some_and(|open| open_tag_flag(open, "scoped"));

    // Positions inside the template are relative to the section; shift them onto
    // the file's lines so a reported line matches the editor's gutter.
    let lines_before = src[..template_start].matches('\n').count();

    let mut parser = Parser::new(&template_src);
    let nodes = parser.parse_nodes(None).map_err(|e| e.offset_lines(lines_before))?;
    // Every element at the top of the template, not just the first.
    //
    // Taking the first and dropping the rest is what this did, in silence. A
    // component written with four siblings rendered only the opening one, and
    // when that one carried an `r-if` that happened to be false, the component
    // rendered *nothing at all*: a blank screen, no warning, and `rux check`
    // clean. That is the silent-drop shape this project keeps hunting, and it
    // cost somebody an afternoon of looking at an empty window.
    //
    // The one-root rule itself is real and stays. It was only ever written down
    // in `docs/02-spec.md`, which is design history and checked against nothing,
    // so an author reading the actual reference had no way to learn it.
    let mut roots: Vec<Element> = nodes
        .into_iter()
        .filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(..) => None,
        })
        .collect();
    if roots.len() > 1 {
        let extra = &roots[1];
        return Err(ParseError::at(
            format!(
                "<template> has {} root elements, and it takes exactly one. \
                 `<{}>` and everything after it would be dropped without a word. \
                 Wrap them in a single `<view>`.",
                roots.len(),
                extra.tag
            ),
            extra.line,
            1,
        )
        .offset_lines(lines_before));
    }
    let mut template = roots
        .pop()
        .ok_or_else(|| ParseError::new("<template> has no root element"))?;
    // The parser counts from the start of the section it was handed, so every
    // line in the tree is short by however many lines came before `<template>`.
    // Shifted once here rather than threaded through the parser, which is the
    // same trade `offset_lines` already makes for a ParseError.
    offset_element_lines(&mut template, lines_before);
    wrap_button_text(&mut template);

    Ok(Sfc {
        file: None,
        template,
        style,
        style_line,
        script,
        script_line,
        style_src,
        style_scoped,
        style_includes: Vec::new(),
        props: Vec::new(),
        types_only: false,
        types: Vec::new(),
    })
}

/// `<button>Join</button>` means `<button><text>Join</text></button>`.
///
/// Only a `<text>` draws words, so a button's bare text used to be dropped:
/// the button drew, empty, and nothing said why. Wrapped here, once, so every
/// later stage (the cascade, the paths, `{{ }}` bindings, the accessibility
/// name) sees exactly the tree the author would have written by hand. Text
/// that is only whitespace is left alone, as it is everywhere else. The
/// formatter reads the source through its own parser, so what is written on
/// disk is never rewritten.
fn wrap_button_text(el: &mut Element) {
    let is_button = el.tag == "button";
    for child in &mut el.children {
        match child {
            Node::Element(inner) => wrap_button_text(inner),
            Node::Text(text, line) if is_button && !text.trim().is_empty() => {
                let wrapped = Element {
                    tag: "text".to_string(),
                    attrs: Vec::new(),
                    children: vec![Node::Text(std::mem::take(text), *line)],
                    line: *line,
                };
                *child = Node::Element(wrapped);
            }
            Node::Text(..) => {}
        }
    }
}

/// Move every line in a parsed subtree onto the file's numbering.
///
/// See the call site: the parser is handed the `<template>` body alone and so
/// counts from 1 at its first line, while everything downstream (the overlay,
/// `rux check --format json`, the editor gutter) means file lines.
fn offset_element_lines(el: &mut Element, by: usize) {
    el.line += by;
    for attr in &mut el.attrs {
        attr.line += by;
    }
    for child in &mut el.children {
        match child {
            Node::Element(child) => offset_element_lines(child, by),
            Node::Text(_, line) => *line += by,
        }
    }
}

/// A section's contents with the surrounding blank space removed, and the
/// 1-based file line its first remaining character sits on.
///
/// The trim is what makes the line number necessary rather than obvious: a
/// `<style>` tag on line 8 usually has its first rule on line 9, and counting
/// the newlines that were trimmed away is the only way to say which.
fn trimmed_section(src: &str, name: &str) -> (String, usize) {
    let Some((raw, start)) = section(src, name) else {
        return (String::new(), 1);
    };
    let leading = raw.len() - raw.trim_start().len();
    let line = src[..start + leading].matches('\n').count() + 1;
    (raw.trim().to_string(), line)
}

/// Extract the inner text of a top-level `<name> … </name>` section, with the
/// byte offset it starts at (so errors inside it can be reported against the
/// file's own line numbers).
fn section(src: &str, name: &str) -> Option<(String, usize)> {
    section_with_open(src, name).map(|(body, at, _)| (body, at))
}

/// [`section`], and the opening tag itself, so its attributes can be read.
///
/// Only `<style src="…">` needs this today. The sections are found by scanning
/// rather than by the element parser, because they are the frame the parser
/// runs inside: `<script>` holds rhai and `<style>` holds CSS, and neither is
/// the XML-shaped grammar.
fn section_with_open(src: &str, name: &str) -> Option<(String, usize, String)> {
    find_section(src, name).ok()
}

/// Why a section could not be read, for the three cases that are not the same
/// problem.
///
/// They produced one message for a long time, and it was the wrong one twice
/// out of three: a file whose `<template>` was simply never closed reported as
/// having **no** `<template>` at all. That is the worst shape a parse error
/// takes, because the author is looking at the tag it says is missing. It shows
/// up constantly while editing, since a half-typed section is unclosed by
/// definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionProblem {
    /// No `<name` anywhere.
    Absent,
    /// `<name` is there and its opening tag never ends: no `>` after it.
    UnterminatedOpenTag { at: usize },
    /// The section opens and never closes.
    Unclosed { at: usize },
}

/// Find one section, or say precisely what is wrong with it.
fn find_section(src: &str, name: &str) -> Result<(String, usize, String), SectionProblem> {
    let open = format!("<{name}");
    let start = src.find(&open).ok_or(SectionProblem::Absent)?;
    let open_end = start
        + src[start..]
            .find('>')
            .ok_or(SectionProblem::UnterminatedOpenTag { at: start })?;
    let after_open = open_end + 1;
    let close = format!("</{name}>");
    let end = src[after_open..]
        .find(&close)
        .ok_or(SectionProblem::Unclosed { at: start })?
        + after_open;
    Ok((
        src[after_open..end].to_string(),
        after_open,
        src[start..open_end].to_string(),
    ))
}

/// Turn a [`SectionProblem`] into the error an author reads.
///
/// `required` separates `<template>`, whose absence is a real error, from
/// `<style>` and `<script>`, which are optional and whose absence is not. A
/// section that is *present and broken* is an error either way: silently
/// dropping every rule in an unclosed `<style>` is the same failure wearing a
/// quieter coat.
fn section_error(src: &str, name: &str, problem: SectionProblem) -> ParseError {
    let at = |offset: usize| {
        let line = src[..offset].matches('\n').count() + 1;
        let col = offset - src[..offset].rfind('\n').map_or(0, |i| i + 1) + 1;
        (line, col)
    };
    match problem {
        SectionProblem::Absent => ParseError::new(format!(
            "missing <{name}> section: every .rux file needs one, holding a single root element"
        )),
        SectionProblem::UnterminatedOpenTag { at: offset } => {
            let (line, col) = at(offset);
            ParseError::at(
                format!("the opening <{name}> tag is never finished: no `>` after it"),
                line,
                col,
            )
        }
        SectionProblem::Unclosed { at: offset } => {
            let (line, col) = at(offset);
            ParseError::at(
                format!(
                    "<{name}> is opened here and never closed: add `</{name}>`. \
                     The section is not missing, it has no end"
                ),
                line,
                col,
            )
        }
    }
}

/// Read one attribute's value out of a raw opening tag, `<style src="a.css"`.
///
/// Deliberately small: a section's opening tag is one line of a handful of
/// attributes, not a document. Quotes are required, because an unquoted path
/// cannot contain a space and silently truncating one at the space is the kind
/// of failure that gets blamed on the file being missing.
/// Whether a valueless attribute is present on an open tag: `<style scoped>`.
///
/// Separate from [`open_tag_attr`], which requires an `=` and a quoted value.
/// A boolean attribute has neither, the same way `<route fallback />` does not.
fn open_tag_flag(open: &str, name: &str) -> bool {
    let mut rest = open;
    while let Some(at) = rest.find(name) {
        let before_ok = rest[..at].chars().next_back().is_none_or(char::is_whitespace);
        let after = &rest[at + name.len()..];
        // Not `scopedish`, and not `scoped="…"`, which is a different thing that
        // this deliberately does not claim.
        let after_ok = after
            .chars()
            .next()
            .is_none_or(|c| c.is_whitespace() || c == '>' || c == '/');
        if before_ok && after_ok {
            return true;
        }
        rest = &rest[at + name.len()..];
    }
    false
}

fn open_tag_attr(open: &str, name: &str) -> Option<String> {
    let mut rest = open;
    loop {
        let at = rest.find(name)?;
        let before_ok = rest[..at].chars().next_back().is_none_or(char::is_whitespace);
        let after = &rest[at + name.len()..];
        let value = after.trim_start();
        if before_ok && value.starts_with('=') {
            let value = value[1..].trim_start();
            let quote = value.chars().next()?;
            if quote == '"' || quote == '\'' {
                let value = &value[1..];
                let end = value.find(quote)?;
                return Some(decode_entities(&value[..end]));
            }
            return None;
        }
        rest = &rest[at + name.len()..];
    }
}

/// Split a `src` attribute into paths. Comma-separated, so one `<style>` can
/// pull in a palette and a component sheet without needing a second section.
fn split_src(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
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

    /// An error carrying the position the parser has reached, which is where the
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
                break; // closing tag, caller consumes it
            }
            if self.peek() == Some('<') {
                let el = self.parse_element()?;
                nodes.push(Node::Element(el));
                continue;
            }
            // Text run up to the next '<'.
            let at = self.line_col(self.pos).0;
            let text = self.read_text();
            if !text.trim().is_empty() {
                // The line of the run's first character, not of its first
                // non-space one. A `{{ }}` sitting on its own line after the
                // tag is the common shape, and the leading newline is part of
                // the run, so the trim is counted back out.
                let skipped = text.chars().take_while(|c| c.is_whitespace()).filter(|c| *c == '\n').count();
                nodes.push(Node::Text(text.trim().to_string(), at + skipped));
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
        // Taken before the `<` is consumed, so the line is the one an author
        // sees the tag begin on.
        let line = self.line_col(self.pos).0;
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
                    // A void tag closes itself. `<input type="text">` is HTML's
                    // shape and the shape every author reaches for, and Rux has
                    // no `</input>` for it to be missing.
                    if is_void(&tag) {
                        return Ok(Element { tag, attrs, children: Vec::new(), line });
                    }
                    let children = self.parse_nodes(Some(&tag))?;
                    self.expect_closing(&tag)?;
                    return Ok(Element { tag, attrs, children, line });
                }
                Some('/') if self.starts_with("/>") => {
                    self.pos += 2;
                    return Ok(Element { tag, attrs, children: Vec::new(), line });
                }
                _ => {
                    let attr_line = self.line_col(self.pos).0;
                    let name = self.read_attr_name();
                    if name.is_empty() {
                        return Err(self.err(format!("malformed attribute in <{tag}>")));
                    }
                    self.skip_ws();
                    let has_value = self.peek() == Some('=');
                    let value = if has_value {
                        self.bump();
                        self.skip_ws();
                        // Decoded here: an attribute is quoted with the same `"`
                        // a script expression needs for its own string literals,
                        // so `&quot;` is how you write one.
                        decode_entities(&self.read_attr_value())
                    } else {
                        String::new() // valueless attribute, e.g. `disabled`
                    };
                    attrs.push(Attr { name, value, line: attr_line, has_value });
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
            // A closing tag for something that never takes one. Saying
            // "expected </view>, found </input>" here would point at the
            // enclosing element and describe the wrong mistake: the `</input>`
            // is not a tag in the wrong place, it is a tag that does not exist.
            if is_void(&close) {
                return Err(self.err(format!(
                    "<{close}> holds nothing, so it has no closing tag; delete </{close}>"
                )));
            }
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

    /// `<input type="text">` is how anyone writes a text field, and Rux has no
    /// `</input>` for it to be missing. Until void tags reached the parser this
    /// was a parse error that named the *enclosing* element's closing tag, so a
    /// correct document was reported as a mismatch.
    #[test]
    fn a_void_tag_closes_itself() {
        let src = "<template>
  <view>
    <input type=\"text\">
  </view>
</template>";
        let sfc = parse_sfc(src).expect("parses without a closing </input>");
        let view = &sfc.template;
        assert_eq!(view.tag, "view");
        let Node::Element(input) = &view.children[0] else { panic!("an element") };
        assert_eq!(input.tag, "input");
        assert_eq!(input.attr("type"), Some("text"));
        assert!(input.children.is_empty(), "a void tag holds nothing");
    }

    /// The slash stays legal: every file in `examples/` is written that way.
    #[test]
    fn a_void_tag_may_still_be_written_self_closing() {
        let src = "<template><view><image src=\"a.png\" /></view></template>";
        let sfc = parse_sfc(src).expect("parses");
        let Node::Element(image) = &sfc.template.children[0] else { panic!("an element") };
        assert_eq!(image.tag, "image");
    }

    /// And a closing tag for one is named as the mistake it is, rather than
    /// reported against whatever element happened to enclose it.
    #[test]
    fn a_closing_void_tag_says_what_is_wrong_with_it() {
        let src = "<template><view><input></input></view></template>";
        let err = parse_sfc(src).expect_err("rejected");
        assert!(
            err.message.contains("no closing tag") && err.message.contains("</input>"),
            "names the tag that does not exist: {}",
            err.message
        );
    }


    /// The line a section's content starts on, which is what lets a later stage
    /// report a CSS warning against the file's own gutter rather than against an
    /// offset into a string nobody can see.
    #[test]
    fn sections_record_the_file_line_they_start_on() {
        // Deliberately not aligned: the leading blank lines inside `<style>` are
        // trimmed, and the count has to survive that.
        let src = "<template>\n  <screen></screen>\n</template>\n\n<style>\n\n  .a { color: red; }\n</style>\n\n<script>\nlet n = signal(1);\n</script>\n";
        let sfc = parse_sfc(src).expect("parses");

        assert_eq!(sfc.style_line, 7, "`.a` is on line 7");
        assert_eq!(sfc.script_line, 11, "`let n` is on line 11");
        // And the recorded line really is where that text is in the file.
        let line_of = |n: usize| src.lines().nth(n - 1).unwrap().trim();
        assert!(line_of(sfc.style_line).starts_with(".a"));
        assert!(line_of(sfc.script_line).starts_with("let n"));
    }

    /// `src` on the `<style>` tag is read, and reading it must not disturb the
    /// line the body starts on: the attribute lives on the opening tag, which
    /// was already being skipped past before includes existed.
    #[test]
    fn a_style_tag_can_name_external_sheets() {
        let src = "<template>\n  <screen></screen>\n</template>\n\n<style src=\"theme.css, layout.css\">\n  .a { color: red; }\n</style>\n";
        let sfc = parse_sfc(src).expect("parses");

        assert_eq!(sfc.style_src, vec!["theme.css", "layout.css"]);
        assert_eq!(sfc.style, ".a { color: red; }");
        assert_eq!(sfc.style_line, 6, "the body still starts on line 6");
        // Parsing does no IO, so nothing is resolved yet.
        assert!(sfc.style_includes.is_empty());
    }

    #[test]
    fn a_style_tag_without_src_asks_for_nothing() {
        let sfc = parse_sfc("<template><screen></screen></template>\n<style>.a{color:red}</style>")
            .expect("parses");
        assert!(sfc.style_src.is_empty());
    }

    /// Single quotes work, and a path is allowed to contain a space. An
    /// unquoted value is refused rather than truncated at the space, because a
    /// half-read path fails later as "no such file" and sends the reader
    /// looking for a missing file instead of a missing quote.
    #[test]
    fn src_values_are_quoted_and_may_contain_spaces() {
        let quoted = parse_sfc(
            "<template><screen></screen></template>\n<style src='my theme.css'>.a{color:red}</style>",
        )
        .expect("parses");
        assert_eq!(quoted.style_src, vec!["my theme.css"]);

        let bare = parse_sfc(
            "<template><screen></screen></template>\n<style src=theme.css>.a{color:red}</style>",
        )
        .expect("parses");
        assert!(bare.style_src.is_empty(), "unquoted is not half-read");
    }

    /// A file with no `<style>` must not claim line 0, which is not a line.
    #[test]
    fn a_missing_section_reports_a_usable_line() {
        let sfc = parse_sfc("<template><screen></screen></template>").expect("parses");
        assert_eq!(sfc.style, "");
        assert_eq!(sfc.style_line, 1);
    }

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

    /// The three ways a section can fail are three different problems, and one
    /// of them used to report as another. A `<template>` that is opened and
    /// never closed said "missing <template> section", which is the worst shape
    /// an error takes: the author is looking straight at the tag it says is
    /// absent, and a half-typed section is unclosed by definition, so it showed
    /// up constantly while editing.
    #[test]
    fn a_broken_section_says_which_way_it_is_broken() {
        let unclosed = parse_sfc("<template>\n  <screen></screen>\n").unwrap_err();
        assert!(
            unclosed.message.contains("never closed"),
            "not `missing`: {}",
            unclosed.message
        );
        assert_eq!(unclosed.line, Some(1), "and it points at the opening tag");

        let absent = parse_sfc("<style>\n.a { color: red; }\n</style>\n").unwrap_err();
        assert!(absent.message.contains("missing <template>"), "{}", absent.message);

        let unterminated = parse_sfc("<template").unwrap_err();
        assert!(
            unterminated.message.contains("never finished"),
            "{}",
            unterminated.message
        );
    }

    /// An unclosed `<style>` used to drop every rule in the file in silence,
    /// which looks exactly like CSS that does not work. A section that is
    /// *present and broken* is an error even when the section is optional.
    #[test]
    fn an_unclosed_optional_section_is_still_an_error() {
        let err = parse_sfc("<template>\n  <screen></screen>\n</template>\n<style>\n.a{color:red}\n")
            .unwrap_err();
        assert!(err.message.contains("<style> is opened here and never closed"), "{}", err.message);
        assert_eq!(err.line, Some(4));

        // And a file with no `<style>` at all is still perfectly fine.
        assert!(parse_sfc("<template>\n  <screen></screen>\n</template>\n").is_ok());
    }
    /// A template with more than one root element says so, and says where.
    ///
    /// It used to take the first element and drop the rest without a word. A
    /// component written as four siblings rendered only the first, and when
    /// that one carried an `r-if` that was false it rendered nothing at all:
    /// a blank window, no warning, and a clean `rux check`. Reported as "rux run
    /// doesn't give me the expected UI", which is exactly what a silent drop
    /// looks like from the outside.
    #[test]
    fn a_template_with_two_roots_says_so_instead_of_dropping_one() {
        let src = "<template>\n  <view><text>one</text></view>\n  <view><text>two</text></view>\n</template>";
        let err = parse_sfc(src).expect_err("two roots is not a document");
        assert!(err.message.contains("root elements"), "names the problem: {}", err.message);
        assert!(err.message.contains("dropped"), "and what it used to cost: {}", err.message);
        assert_eq!(err.line, Some(3), "points at the second root, not the first");
    }

    /// The count and the offending tag are both in the message, because "more
    /// than one" leaves the author counting and the tag is what they search for.
    #[test]
    fn the_multi_root_message_names_the_count_and_the_tag() {
        let src = "<template>\n  <view />\n  <text>x</text>\n  <input />\n</template>";
        let err = parse_sfc(src).expect_err("three roots");
        assert!(err.message.contains('3'), "the count: {}", err.message);
        assert!(err.message.contains("`<text>`"), "the tag that starts the dropped run: {}", err.message);
    }

    /// One root is still one root, including with comments and stray text
    /// around it, which are not elements and must not be counted.
    #[test]
    fn a_single_root_with_comments_around_it_is_still_one_root() {
        let src = "<template>\n  <!-- a note -->\n  <view><text>one</text></view>\n</template>";
        let sfc = parse_sfc(src).expect("one root");
        assert_eq!(sfc.template.tag, "view");
    }

}
