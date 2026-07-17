//! Rux styling — milestone M2.
//!
//! Parses the `<style>` CSS with `lightningcss` (literal CSS, per Law 4), matches
//! rules against the template tree with our own small selector engine, applies
//! the cascade, and produces a styled `rux_layout::Node` tree. This is Stage 2
//! of `docs/04-architecture.md`, narrowed to the honored subset.
//!
//! Selector support: tag, `.class`, `#id`, `[role="…"]`, compound
//! (`view.card`), and all four combinators — descendant (`.a .b`), child
//! (`.a > .b`), next-sibling (`.a + .b`) and subsequent-sibling (`.a ~ .b`).
//! Specificity and source order resolve conflicts, as in CSS.

use std::collections::HashMap;

use lightningcss::rules::CssRule;
use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
use lightningcss::traits::ToCss;
use rux_layout::{
    Align, Axis, Background, BoxShadow, Cursor, Display, Gradient, GridPlace, ImageContent, Justify,
    Len, Node as LayoutNode, Overflow, Position, Rgba, Sides, Style, TextAlign, TextContent,
    TextWrap, Track, TrackSide,
};
use rux_layout::{GradientKind, GridFlow, Transform};
use rux_parser::{Element, Node as TplNode, Sfc};
use rux_reactive::Value;
use rux_script::Engine;

/// Loop-variable bindings introduced by `r-for`, layered as a scope stack and
/// injected into the script engine for each evaluation.
type Locals = Vec<(String, Value)>;

/// Bake the active `r-for` loop bindings into a handler as a `let` prelude, so it
/// still resolves them when it runs later in global scope (the loop variables are
/// gone by then). With no locals the handler is returned unchanged.
fn bind_locals(src: &str, locals: &Locals) -> String {
    if locals.is_empty() {
        return src.to_string();
    }
    let mut out = String::new();
    for (name, value) in locals {
        out.push_str("let ");
        out.push_str(name);
        out.push_str(" = ");
        out.push_str(&value.to_rhai_literal());
        out.push_str("; ");
    }
    out.push_str(src);
    out
}

/// A compiled component: its template root and its own CSS rules.
struct Component {
    template: Element,
    rules: Vec<Rule>,
}

/// Registered components, keyed by custom-element tag.
type Components = HashMap<String, Component>;

/// Default inherited text colour (`#cdd6f4`) and font size, used at the root
/// before any `color` / `font-size` rule applies. Text properties inherit.
const DEFAULT_COLOR: Rgba = Rgba::new(0.804, 0.839, 0.957, 1.0);
const DEFAULT_FONT_SIZE: f32 = 16.0;

/// The text properties that inherit down the tree: an element uses its own
/// `color`/`font-size`/`font-family` if set, else its parent's resolved value.
#[derive(Clone)]
struct Inherited {
    color: Rgba,
    font_size: f32,
    font_family: Option<String>,
}

/// A radius larger than any sane box; kurbo clamps it to half the shorter side,
/// which makes the box a circle/pill whatever its size.
const CIRCLE: f32 = 9999.0;

/// An `<input type=checkbox|radio>` and whether it is currently checked.
#[derive(Clone, Copy)]
struct Toggle {
    radio: bool,
    checked: bool,
}

impl Toggle {
    fn of(el: &Element, engine: &mut Engine, locals: &Locals) -> Option<Self> {
        if el.tag != "input" {
            return None;
        }
        let radio = match el.attr("type") {
            Some("radio") => true,
            Some("checkbox") => false,
            _ => return None,
        };
        let model = el.attr("r-model").unwrap_or_default();
        let checked = if model.is_empty() {
            false
        } else if radio {
            engine.eval_display(model, locals) == el.attr("value").unwrap_or_default()
        } else {
            engine.eval_bool(model, locals)
        };
        Some(Self { radio, checked })
    }
}

/// Build the styled layout tree from a parsed SFC. `components` maps a custom
/// element tag to the imported component's source; those are compiled and
/// expanded in place with their props bound. `{{ }}` and directive expressions
/// evaluate against the script engine's current state.
pub fn build_styled_tree(
    sfc: &Sfc,
    components: &HashMap<String, Sfc>,
    engine: &mut Engine,
) -> Result<LayoutNode, String> {
    let rules = parse_rules(&sfc.style);
    let comps: Components = components
        .iter()
        .map(|(tag, c)| {
            (
                tag.clone(),
                Component {
                    template: c.template.clone(),
                    rules: parse_rules(&c.style),
                },
            )
        })
        .collect();

    let mut ancestors: Vec<AncNode> = Vec::new();
    let locals = Locals::new();
    Ok(build_node(
        &sfc.template,
        &rules,
        &comps,
        &mut ancestors,
        &[],
        &Inherited { color: DEFAULT_COLOR, font_size: DEFAULT_FONT_SIZE, font_family: None },
        engine,
        &locals,
    ))
}

/// Replace `{{ expr }}` spans in `text` with values evaluated by the engine.
/// Literal text has its HTML entities (`&amp;`, `&lt;`, …) decoded; interpolated
/// values are inserted verbatim (they're already runtime strings).
fn interpolate(text: &str, engine: &mut Engine, locals: &Locals) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&decode_entities(&rest[..start]));
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                out.push_str(&engine.eval_display(after[..end].trim(), locals));
                rest = &after[end + 2..];
            }
            None => {
                out.push_str("{{");
                rest = after;
            }
        }
    }
    out.push_str(&decode_entities(rest));
    out
}

/// Decode the HTML entities an author might write in text: the named ones
/// (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;`) and numeric
/// (`&#38;`, `&#x26;`). An unrecognised `&…;` is left as written.
fn decode_entities(s: &str) -> String {
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

// ── Selector model ──────────────────────────────────────────────────────────

/// One compound selector, e.g. `view.card#main[role="section"]`.
#[derive(Debug, Clone, Default)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    role: Option<String>,
}

/// How one compound relates to the compound on its left in a selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    /// `a b` — b is any descendant of a.
    Descendant,
    /// `a > b` — b is a direct child of a.
    Child,
    /// `a + b` — b is the element immediately following sibling a.
    NextSibling,
    /// `a ~ b` — b is any following sibling of a.
    SubsequentSibling,
}

/// A full selector: a chain of compounds joined by combinators, plus its
/// specificity. `combs[i]` links `chain[i]` to `chain[i + 1]`, so it always has
/// one fewer entry than `chain`.
#[derive(Debug, Clone)]
struct Rule {
    chain: Vec<Compound>,
    combs: Vec<Combinator>,
    specificity: (u32, u32, u32),
    order: usize,
    decls: Vec<(String, String)>,
}

/// The matchable identity of a template element.
#[derive(Debug, Clone)]
struct ElemDesc {
    tag: String,
    id: Option<String>,
    classes: Vec<String>,
    role: Option<String>,
}

/// An ancestor in the match context: its identity plus the identities of the
/// rendered siblings that precede it. The preceding siblings are needed so a
/// sibling combinator (`+`/`~`) sitting above a descendant/child hop
/// (e.g. `.a ~ .b .c`) can still be resolved correctly.
#[derive(Debug, Clone)]
struct AncNode {
    desc: ElemDesc,
    prev: Vec<ElemDesc>,
}

impl ElemDesc {
    fn of(el: &Element) -> Self {
        Self {
            tag: el.tag.clone(),
            id: el.id().map(str::to_string),
            classes: el.classes().into_iter().map(str::to_string).collect(),
            role: el.role().map(str::to_string),
        }
    }
}

// ── Parsing the stylesheet ──────────────────────────────────────────────────

fn parse_rules(css: &str) -> Vec<Rule> {
    let sheet = match StyleSheet::parse(css, ParserOptions::default()) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut rules = Vec::new();
    let mut order = 0usize;

    for rule in &sheet.rules.0 {
        let CssRule::Style(style) = rule else { continue };

        // Serialize each declaration to "prop: value" and split it.
        let mut decls = Vec::new();
        for prop in &style.declarations.declarations {
            if let Ok(text) = prop.to_css_string(false, PrinterOptions::default()) {
                if let Some((k, v)) = text.split_once(':') {
                    let key = k.trim().to_lowercase();
                    // Silent ignoring is the worst failure mode we have: valid CSS
                    // that does nothing with no explanation. Say so, once per name.
                    warn_if_unhonored(&key);
                    decls.push((
                        key,
                        v.trim().trim_end_matches(';').trim().to_string(),
                    ));
                }
            }
        }

        // One Rule per selector in the list (they share the declarations).
        for selector in &style.selectors.0 {
            if let Ok(text) = selector.to_css_string(PrinterOptions::default()) {
                if let Some((chain, combs, specificity)) = parse_selector(&text) {
                    rules.push(Rule {
                        chain,
                        combs,
                        specificity,
                        order,
                        decls: decls.clone(),
                    });
                }
            }
            order += 1;
        }
    }
    rules
}

/// The CSS properties the runtime actually interprets today. Anything outside
/// this set is parsed and then dropped, so [`warn_if_unhonored`] flags it. When
/// a new property is honored in `interpret` (or the text/border helpers), add it
/// here too, or authors will be told a working property does nothing.
const HONORED_PROPERTIES: &[&str] = &[
    // Box / display
    "display", "width", "height", "gap",
    "min-width", "max-width", "min-height", "max-height",
    "padding", "padding-top", "padding-right", "padding-bottom", "padding-left",
    "margin", "margin-top", "margin-right", "margin-bottom", "margin-left",
    "border", "border-width", "border-color", "border-radius",
    "border-top-left-radius", "border-top-right-radius",
    "border-bottom-right-radius", "border-bottom-left-radius",
    "border-top", "border-right", "border-bottom", "border-left",
    "border-top-width", "border-right-width", "border-bottom-width", "border-left-width",
    "overflow", "overflow-x", "overflow-y", "opacity", "cursor", "box-shadow", "transform",
    // Flex / grid
    "flex", "flex-grow", "flex-shrink", "flex-basis", "flex-wrap", "flex-direction",
    "justify-content", "align-items", "align-self", "justify-self", "justify-items",
    "align-content", "row-gap", "column-gap",
    "grid-template-columns", "grid-template-rows",
    "grid-column", "grid-row",
    "grid-column-start", "grid-column-end", "grid-row-start", "grid-row-end",
    "grid-auto-flow", "grid-auto-rows", "grid-auto-columns",
    // Positioning
    "position", "top", "right", "bottom", "left", "aspect-ratio",
    // Background
    "background", "background-color", "background-image",
    // Text
    "color", "font-size", "font-weight", "font-family", "font-style", "text-align",
    "letter-spacing", "word-spacing", "line-height", "white-space",
    "text-decoration", "text-decoration-line",
    "overflow-wrap", "word-wrap", "word-break",
];

fn is_honored(property: &str) -> bool {
    HONORED_PROPERTIES.contains(&property)
}

/// Warn — once per property name, for the life of the process — that a parsed
/// declaration is not honored. Deduped so a whole-tree rebuild (which reparses
/// every sheet) doesn't repeat the same line on every keystroke.
fn warn_if_unhonored(property: &str) {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

    if is_honored(property) {
        return;
    }
    let seen = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
    let Ok(mut seen) = seen.lock() else { return };
    if seen.insert(property.to_string()) {
        eprintln!(
            "rux: CSS property `{property}` is parsed but not yet honored — it will have no effect"
        );
    }
}

/// Parse a selector string into a chain of compounds, the combinators joining
/// them, and its specificity. Combinator tokens (`>`, `+`, `~`) are recognised
/// with or without surrounding whitespace; a bare space is the descendant
/// combinator. `[…]` attribute segments are skipped so a `~=` inside one is not
/// mistaken for a combinator.
fn parse_selector(text: &str) -> Option<(Vec<Compound>, Vec<Combinator>, (u32, u32, u32))> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut chain = Vec::new();
    let mut combs = Vec::new();
    let mut spec = (0u32, 0u32, 0u32);
    // A combinator waiting to be attached to the next compound we read.
    let mut pending: Option<Combinator> = None;

    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if let Some(comb) = combinator_of(c) {
            pending = Some(comb);
            i += 1;
            continue;
        }
        // Read one compound: everything up to the next top-level whitespace or
        // combinator, treating `[…]` as opaque.
        let start = i;
        let mut depth = 0i32;
        while i < chars.len() {
            let d = chars[i];
            if d == '[' {
                depth += 1;
            } else if d == ']' {
                depth -= 1;
            } else if depth == 0 && (d.is_whitespace() || combinator_of(d).is_some()) {
                break;
            }
            i += 1;
        }
        let token: String = chars[start..i].iter().collect();
        let compound = parse_compound(&token, &mut spec)?;
        if !chain.is_empty() {
            // A space with no explicit combinator is the descendant combinator.
            combs.push(pending.take().unwrap_or(Combinator::Descendant));
        }
        pending = None;
        chain.push(compound);
    }
    if chain.is_empty() {
        return None;
    }
    Some((chain, combs, spec))
}

fn combinator_of(c: char) -> Option<Combinator> {
    match c {
        '>' => Some(Combinator::Child),
        '+' => Some(Combinator::NextSibling),
        '~' => Some(Combinator::SubsequentSibling),
        _ => None,
    }
}

fn parse_compound(token: &str, spec: &mut (u32, u32, u32)) -> Option<Compound> {
    let mut c = Compound::default();
    let chars: Vec<char> = token.chars().collect();
    let mut i = 0;

    // Optional leading type/universal selector.
    let mut tag = String::new();
    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '-' || chars[i] == '*') {
        tag.push(chars[i]);
        i += 1;
    }
    if !tag.is_empty() && tag != "*" {
        c.tag = Some(tag);
        spec.2 += 1;
    }

    while i < chars.len() {
        match chars[i] {
            '.' => {
                i += 1;
                let mut cls = String::new();
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '-' || chars[i] == '_') {
                    cls.push(chars[i]);
                    i += 1;
                }
                if !cls.is_empty() {
                    c.classes.push(cls);
                    spec.1 += 1;
                }
            }
            '#' => {
                i += 1;
                let mut id = String::new();
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '-' || chars[i] == '_') {
                    id.push(chars[i]);
                    i += 1;
                }
                if !id.is_empty() {
                    c.id = Some(id);
                    spec.0 += 1;
                }
            }
            '[' => {
                // Only `[role="…"]` / `[role=…]` is understood in M2.
                let end = token.find(']')?;
                let inner = &token[i + 1..end];
                if let Some(rest) = inner.strip_prefix("role") {
                    let val = rest
                        .trim_start_matches('=')
                        .trim_matches(|ch| ch == '"' || ch == '\'');
                    c.role = Some(val.to_string());
                    spec.1 += 1;
                }
                i = end + 1;
            }
            _ => break,
        }
    }
    Some(c)
}

// ── Matching & cascade ──────────────────────────────────────────────────────

fn matches_compound(c: &Compound, el: &ElemDesc) -> bool {
    if let Some(t) = &c.tag {
        if *t != el.tag {
            return false;
        }
    }
    if let Some(id) = &c.id {
        if Some(id.as_str()) != el.id.as_deref() {
            return false;
        }
    }
    for cls in &c.classes {
        if !el.classes.iter().any(|x| x == cls) {
            return false;
        }
    }
    if let Some(r) = &c.role {
        // Roles match case-insensitively (role="Heading" ~ [role="heading"]).
        if !el.role.as_deref().is_some_and(|er| er.eq_ignore_ascii_case(r)) {
            return false;
        }
    }
    true
}

/// Does the selector `chain` (joined by `combs`) match the element `el`, whose
/// ancestors are `ancestors` (root-first) and whose preceding rendered siblings
/// are `prev` (document order)?
///
/// Matches right-to-left with backtracking: the rightmost compound must match
/// `el`, then the combinator to its left dictates where the remaining prefix is
/// sought — up the ancestor chain (descendant/child) or across the preceding
/// siblings (`+`/`~`). Siblings share `el`'s ancestors; an ancestor's own
/// preceding siblings ride along in [`AncNode::prev`], so a sibling combinator
/// above a descendant hop still resolves.
fn matches_chain(
    chain: &[Compound],
    combs: &[Combinator],
    el: &ElemDesc,
    ancestors: &[AncNode],
    prev: &[ElemDesc],
) -> bool {
    let Some((last, rest)) = chain.split_last() else {
        return false;
    };
    if !matches_compound(last, el) {
        return false;
    }
    if rest.is_empty() {
        return true;
    }
    // `combs` has one fewer entry than `chain`; the last one links `last` to the
    // compound now at the end of `rest`.
    let (comb, rest_combs) = combs.split_last().expect("combs matches chain length");
    match comb {
        Combinator::Descendant => (0..ancestors.len()).rev().any(|i| {
            matches_chain(rest, rest_combs, &ancestors[i].desc, &ancestors[..i], &ancestors[i].prev)
        }),
        Combinator::Child => {
            let Some((parent, up)) = ancestors.split_last() else {
                return false;
            };
            matches_chain(rest, rest_combs, &parent.desc, up, &parent.prev)
        }
        Combinator::NextSibling => {
            let Some((sib, earlier)) = prev.split_last() else {
                return false;
            };
            matches_chain(rest, rest_combs, sib, ancestors, earlier)
        }
        Combinator::SubsequentSibling => (0..prev.len())
            .rev()
            .any(|i| matches_chain(rest, rest_combs, &prev[i], ancestors, &prev[..i])),
    }
}

/// Collect the matching rules' declarations for an element, in cascade order.
fn matched_props(
    desc: &ElemDesc,
    ancestors: &[AncNode],
    prev: &[ElemDesc],
    rules: &[Rule],
) -> HashMap<String, String> {
    let mut matched: Vec<&Rule> = rules
        .iter()
        .filter(|r| matches_chain(&r.chain, &r.combs, desc, ancestors, prev))
        .collect();
    matched.sort_by(|a, b| a.specificity.cmp(&b.specificity).then(a.order.cmp(&b.order)));

    let mut props: HashMap<String, String> = HashMap::new();
    for rule in matched {
        for (k, v) in &rule.decls {
            props.insert(k.clone(), v.clone());
        }
    }
    props
}

/// Concatenate the direct text children of an element, interpolating `{{ }}`.
fn collect_text(el: &Element, engine: &mut Engine, locals: &Locals) -> String {
    let mut parts = Vec::new();
    for child in &el.children {
        if let TplNode::Text(t) = child {
            parts.push(interpolate(t.trim(), engine, locals));
        }
    }
    parts.join(" ")
}

/// Build one element into a layout node. Structural directives on the element
/// itself (`r-for`, `r-if`, `r-elif`, `r-else`) are handled by the parent in
/// [`build_children`]; this function handles per-node concerns (`r-show`) and
/// recurses into children.
///
/// `inherited` carries the resolved text properties (`color`/`font-size`/
/// `font-family`, which inherit); `locals` carries `r-for` loop bindings.
#[allow(clippy::too_many_arguments)]
fn build_node(
    el: &Element,
    rules: &[Rule],
    comps: &Components,
    ancestors: &mut Vec<AncNode>,
    prev: &[ElemDesc],
    inherited: &Inherited,
    engine: &mut Engine,
    locals: &Locals,
) -> LayoutNode {
    // A custom-element tag expands its imported component in place.
    if let Some(component) = comps.get(&el.tag) {
        return expand_component(el, component, comps, inherited, engine, locals);
    }

    let mut desc = ElemDesc::of(el);
    // A ticked checkbox / selected radio carries a synthetic `checked` class, so
    // its checked look is plain CSS (`.box.checked { background: … }`) — we have
    // no `:checked` pseudo-class and this needs no new selector machinery.
    let toggle = Toggle::of(el, engine, locals);
    if toggle.is_some_and(|t| t.checked) {
        desc.classes.push("checked".to_string());
    }
    let props = matched_props(&desc, ancestors, prev, rules);
    let style = interpret(&props);
    // A `@tap` handler runs later, in global scope, where the `r-for` loop
    // variable no longer exists — so `@tap="picked = item"` would see `item`
    // undefined and silently do nothing. Bake the current loop bindings into the
    // handler as a `let` prelude so it reproduces them when it runs.
    let on_tap = el.attr("@tap").map(|h| bind_locals(h, locals));
    // r-show="false" keeps the layout slot but paints nothing.
    let hidden = el
        .attr("r-show")
        .is_some_and(|e| !engine.eval_bool(e, locals));

    // Resolve inheritable text properties (own value, else inherited).
    let color = props
        .get("color")
        .and_then(|v| parse_color(v))
        .unwrap_or(inherited.color);
    let font_size = props
        .get("font-size")
        .and_then(|v| parse_px(first(v)))
        .unwrap_or(inherited.font_size);
    // `font-family` is stored as the raw CSS list; parley parses it and does the
    // fallback. An empty/`inherit` value falls back to the inherited family.
    let font_family = props
        .get("font-family")
        .filter(|v| !v.trim().is_empty() && v.trim() != "inherit")
        .map(|v| v.trim().to_string())
        .or_else(|| inherited.font_family.clone());
    // Non-inheriting shaping props, resolved from this node's own rules (as
    // `font-weight`/`text-align` already are).
    let letter_spacing = props.get("letter-spacing").and_then(|v| parse_spacing(v));
    let word_spacing = props.get("word-spacing").and_then(|v| parse_spacing(v));
    // `line-height`: a unitless number multiplies the font size; a length is
    // absolute; `normal` keeps the font metrics.
    let line_height = props.get("line-height").and_then(|v| parse_line_height(v, font_size));
    let italic = props
        .get("font-style")
        .is_some_and(|v| matches!(v.trim(), "italic" | "oblique"));
    // `text-decoration[-line]`: underline / line-through (space-separated list).
    let decoration = props.get("text-decoration-line").or_else(|| props.get("text-decoration"));
    let underline = decoration.is_some_and(|v| v.split_whitespace().any(|t| t == "underline"));
    let strikethrough = decoration.is_some_and(|v| v.split_whitespace().any(|t| t == "line-through"));
    // `white-space: nowrap|pre` stops line breaking. (We don't preserve `pre`
    // whitespace runs yet; the no-wrap half is what matters for layout.)
    let nowrap = props
        .get("white-space")
        .is_some_and(|v| matches!(v.trim(), "nowrap" | "pre"));

    if el.tag == "text" {
        let weight = props.get("font-weight").and_then(|v| parse_weight(v)).unwrap_or(400);
        let align = props
            .get("text-align")
            .map(|v| parse_text_align(v))
            .unwrap_or_default();
        let wrap = style.text_wrap;
        let mut node = LayoutNode::text(
            style,
            TextContent {
                text: collect_text(el, engine, locals),
                font_size,
                weight,
                color,
                align,
                wrap,
                font_family: font_family.clone(),
                letter_spacing,
                word_spacing,
                line_height,
                italic,
                underline,
                strikethrough,
                nowrap,
                caret: None,
                selection: None,
            },
        );
        node.on_tap = on_tap;
        node.hidden = hidden;
        return node;
    }

    // <image src=…>: a leaf that paints its pixels. The `src` here is still the
    // author's string; the runtime resolves it against the .rux file's directory
    // and fills in the intrinsic size.
    if el.tag == "image" {
        let src = el
            .attr(":src")
            .map(|e| engine.eval_display(e, locals))
            .or_else(|| el.attr("src").map(str::to_string))
            .unwrap_or_default();
        let mut node = LayoutNode::image(
            style,
            ImageContent {
                src,
                intrinsic: (0.0, 0.0),
            },
        );
        node.on_tap = on_tap;
        node.hidden = hidden;
        return node;
    }

    // <input>: a box bound to a signal via r-model.
    //
    // `type=checkbox|radio` are tap-toggles, not text fields: they get no focus
    // and no keyboard, they just write the bound signal through the ordinary
    // handler path (`sig = !sig` / `sig = "value"`). An authored @tap wins.
    if let Some(Toggle { radio, checked }) = toggle {
        let model = el.attr("r-model").unwrap_or_default().to_string();
        let value = el.attr("value").unwrap_or_default().to_string();

        let mut style = style;
        // Centre the mark inside the box unless the author says otherwise.
        if style.display == Display::Block {
            style.display = Display::Flex;
        }
        style.justify.get_or_insert(Justify::Center);
        style.align.get_or_insert(Align::Center);
        // A radio is round unless it was given its own radius.
        if radio && style.radius == [0.0; 4] {
            style.radius = [CIRCLE; 4];
        }

        let mut node = LayoutNode::new(style);
        if checked {
            node.children.push(if radio {
                // A dot, in the box's text colour.
                LayoutNode::new(Style {
                    display: Display::Flex,
                    width: Some(Len::Pct(0.5)),
                    height: Some(Len::Pct(0.5)),
                    background: Some(Background::Color(color)),
                    radius: [CIRCLE; 4],
                    ..Default::default()
                })
            } else {
                // A stroked checkmark, in the box's text colour. Style the checked
                // box itself with `.yourclass.checked { … }`.
                let mut mark = LayoutNode::new(Style {
                    display: Display::Flex,
                    width: Some(Len::Pct(0.68)),
                    height: Some(Len::Pct(0.68)),
                    ..Default::default()
                });
                mark.tick = Some(color);
                mark
                        });
        }
        node.on_tap = on_tap.or_else(|| {
            if model.is_empty() {
                None
            } else if radio {
                Some(format!("{model} = \"{value}\""))
            } else {
                Some(format!("{model} = !{model}"))
            }
        });
        node.hidden = hidden;
        return node;
    }

    // A text input: shows the bound value (or a dim placeholder when empty). The
    // shell focuses it on tap and edits the bound signal on keystrokes.
    // `type="textarea"` is the same, but `Enter` inserts a newline.
    if el.tag == "input" {
        let mut style = style;
        let multiline = el.attr("type") == Some("textarea");
        // Inputs are form controls: they fill their slot rather than hug their
        // text (else the box would shrink as you type). A single line clips; a
        // textarea scrolls, so text past the bottom stays reachable.
        if style.width.is_none() {
            style.width = Some(Len::Pct(1.0));
        }
        if style.overflow == Overflow::Visible {
            style.overflow = if multiline { Overflow::Scroll } else { Overflow::Clip };
        }
        // `type="select"`: evaluate the bound `:options` collection to strings so
        // the shell can render a dropdown.
        let options = (el.attr("type") == Some("select"))
            .then(|| {
                el.attr(":options")
                    .and_then(|e| engine.eval_value(e, locals))
                    .and_then(|v| v.as_list().map(|items| items.iter().map(Value::to_display).collect()))
                    .unwrap_or_default()
            });
        let model = el.attr("r-model").map(str::to_string);
        let value = model
            .as_deref()
            .map(|m| engine.eval_display(m, locals))
            .unwrap_or_default();
        let (shown, shown_color) = if value.is_empty() {
            let placeholder = el.attr("placeholder").unwrap_or_default().to_string();
            (placeholder, Rgba::new(0.42, 0.44, 0.52, 1.0)) // #6c7086
        } else {
            (value, color)
        };
        let text_child = LayoutNode::text(
            Style::default(),
            TextContent {
                text: shown,
                font_size,
                weight: 400,
                color: shown_color,
                align: TextAlign::Start,
                wrap: style.text_wrap,
                font_family: font_family.clone(),
                letter_spacing,
                word_spacing,
                line_height,
                italic,
                underline,
                strikethrough,
                // A single-line input never wraps; a textarea does.
                nowrap: !multiline,
                // The runtime marks the focused input's caret and selection.
                caret: None,
                selection: None,
            },
        );
        let mut node = LayoutNode::new(style);
        node.children.push(text_child);
        node.model = model;
        node.multiline = multiline;
        node.options = options;
        node.on_tap = on_tap;
        node.hidden = hidden;
        return node;
    }

    ancestors.push(AncNode { desc, prev: prev.to_vec() });
    let element_children: Vec<&Element> = el
        .children
        .iter()
        .filter_map(|n| match n {
            TplNode::Element(child) => Some(child),
            TplNode::Text(_) => None,
        })
        .collect();
    let children = build_children(
        &element_children,
        rules,
        comps,
        ancestors,
        &Inherited { color, font_size, font_family },
        engine,
        locals,
    );
    ancestors.pop();

    LayoutNode {
        style,
        text: None,
        image: None,
        tick: None,
        children,
        on_tap,
        model: None,
        multiline: false,
        options: None,
        hidden,
    }
}

/// Expand a `<custom-element :prop="expr" …>` into its component's tree. Props
/// (attributes prefixed `:`) are evaluated in the caller's scope and become the
/// only locals visible inside the component (component instances are isolated).
fn expand_component(
    el: &Element,
    component: &Component,
    comps: &Components,
    inherited: &Inherited,
    engine: &mut Engine,
    parent_locals: &Locals,
) -> LayoutNode {
    let mut props: Locals = Vec::new();
    for (key, expr) in &el.attrs {
        if let Some(name) = key.strip_prefix(':') {
            if let Some(value) = engine.eval_value(expr, parent_locals) {
                props.push((name.to_string(), value));
            }
        }
    }

    let mut ancestors: Vec<AncNode> = Vec::new();
    build_node(
        &component.template,
        &component.rules,
        comps,
        &mut ancestors,
        &[],
        inherited,
        engine,
        &props,
    )
}

/// Parse `r-for="item in items"` into `(binding, collection_expr)`.
fn parse_for(expr: &str) -> Option<(&str, &str)> {
    let (var, coll) = expr.split_once(" in ")?;
    Some((var.trim(), coll.trim()))
}

/// Build a sequence of element children, applying the structural directives
/// `r-for` (repeat) and `r-if`/`r-elif`/`r-else` (conditional chains).
#[allow(clippy::too_many_arguments)]
fn build_children(
    elements: &[&Element],
    rules: &[Rule],
    comps: &Components,
    ancestors: &mut Vec<AncNode>,
    inherited: &Inherited,
    engine: &mut Engine,
    locals: &Locals,
) -> Vec<LayoutNode> {
    let mut out = Vec::new();
    // The identities of the rendered siblings so far, so `+`/`~` combinators can
    // see the elements preceding the one being built. (The synthetic `checked`
    // class is not reflected here — sibling combinators don't see checked state.)
    let mut prev: Vec<ElemDesc> = Vec::new();
    // Tracks an active r-if/r-elif/r-else chain and whether a branch was taken.
    let mut in_chain = false;
    let mut chain_satisfied = false;

    for el in elements {
        // r-for expands the element once per collection item; it ends any chain.
        if let Some(for_expr) = el.attr("r-for") {
            in_chain = false;
            if let Some((var, coll)) = parse_for(for_expr) {
                let items = engine
                    .eval_value(coll, locals)
                    .and_then(|v| v.as_list().map(<[Value]>::to_vec));
                if let Some(items) = items {
                    for item in items {
                        let mut child_locals = locals.clone();
                        child_locals.push((var.to_string(), item));
                        out.push(build_node(el, rules, comps, ancestors, &prev, inherited, engine, &child_locals));
                        prev.push(ElemDesc::of(el));
                    }
                }
            }
            continue;
        }

        if let Some(cond) = el.attr("r-if") {
            in_chain = true;
            chain_satisfied = engine.eval_bool(cond, locals);
            if chain_satisfied {
                out.push(build_node(el, rules, comps, ancestors, &prev, inherited, engine, locals));
                prev.push(ElemDesc::of(el));
            }
            continue;
        }
        if let Some(cond) = el.attr("r-elif") {
            if in_chain && !chain_satisfied && engine.eval_bool(cond, locals) {
                chain_satisfied = true;
                out.push(build_node(el, rules, comps, ancestors, &prev, inherited, engine, locals));
                prev.push(ElemDesc::of(el));
            }
            continue;
        }
        if el.attr("r-else").is_some() {
            if in_chain && !chain_satisfied {
                out.push(build_node(el, rules, comps, ancestors, &prev, inherited, engine, locals));
                prev.push(ElemDesc::of(el));
            }
            in_chain = false;
            continue;
        }

        // A plain element ends any active chain.
        in_chain = false;
        out.push(build_node(el, rules, comps, ancestors, &prev, inherited, engine, locals));
        prev.push(ElemDesc::of(el));
    }
    out
}

// ── Value interpretation (honored subset) ───────────────────────────────────

fn interpret(p: &HashMap<String, String>) -> Style {
    let mut st = Style::default();
    if let Some(v) = p.get("display") {
        st.display = match v.trim() {
            "flex" => Display::Flex,
            "grid" => Display::Grid,
            "inline" => Display::Inline,
            "none" => Display::None,
            _ => Display::Block,
        };
    }
    if let Some(v) = p.get("width") {
        st.width = parse_len(first(v));
    }
    if let Some(v) = p.get("height") {
        st.height = parse_len(first(v));
    }
    st.padding = box_sides(p, "padding");
    st.margin = box_sides(p, "margin");
    interpret_border(p, &mut st);
    if let Some(v) = p.get("gap") {
        if let Some(px) = parse_px(first(v)) {
            st.gap = px;
        }
    }
    if let Some(v) = p.get("min-width") {
        st.min_width = parse_len(first(v));
    }
    if let Some(v) = p.get("max-width") {
        st.max_width = parse_len(first(v));
    }
    if let Some(v) = p.get("min-height") {
        st.min_height = parse_len(first(v));
    }
    if let Some(v) = p.get("max-height") {
        st.max_height = parse_len(first(v));
    }
    if let Some(v) = p.get("grid-template-columns") {
        st.grid_columns = parse_tracks(v);
    }
    if let Some(v) = p.get("grid-template-rows") {
        st.grid_rows = parse_tracks(v);
    }
    // Grid item placement: `grid-column: 1 / 3`, `grid-row: span 2`, and the
    // -start/-end longhands.
    if let Some(v) = p.get("grid-column") {
        st.grid_column = parse_grid_shorthand(v);
    }
    if let Some(v) = p.get("grid-row") {
        st.grid_row = parse_grid_shorthand(v);
    }
    if let Some(v) = p.get("grid-column-start") {
        st.grid_column.0 = parse_grid_place(v);
    }
    if let Some(v) = p.get("grid-column-end") {
        st.grid_column.1 = parse_grid_place(v);
    }
    if let Some(v) = p.get("grid-row-start") {
        st.grid_row.0 = parse_grid_place(v);
    }
    if let Some(v) = p.get("grid-row-end") {
        st.grid_row.1 = parse_grid_place(v);
    }
    if let Some(v) = p.get("grid-auto-flow") {
        let v = v.trim();
        let dense = v.contains("dense");
        st.grid_auto_flow = if v.contains("column") {
            if dense { GridFlow::ColumnDense } else { GridFlow::Column }
        } else if dense {
            GridFlow::RowDense
        } else {
            GridFlow::Row
        };
    }
    if let Some(v) = p.get("grid-auto-rows") {
        st.grid_auto_rows = parse_tracks(v);
    }
    if let Some(v) = p.get("grid-auto-columns") {
        st.grid_auto_columns = parse_tracks(v);
    }
    // `flex: grow [shrink [basis]]` first, so the longhands can override it.
    if let Some(v) = p.get("flex") {
        interpret_flex_shorthand(v.trim(), &mut st);
    }
    if let Some(v) = p.get("flex-grow") {
        if let Ok(g) = first(v).parse::<f32>() {
            st.grow = g;
        }
    }
    if let Some(v) = p.get("flex-shrink") {
        if let Ok(s) = first(v).parse::<f32>() {
            st.shrink = s.max(0.0);
        }
    }
    if let Some(v) = p.get("flex-basis") {
        st.basis = match first(v) {
            "auto" | "content" => None,
            l => parse_len(l),
        };
    }
    if let Some(v) = p.get("flex-wrap") {
        st.wrap = matches!(v.trim(), "wrap" | "wrap-reverse");
    }
    if let Some(v) = p.get("overflow-wrap").or_else(|| p.get("word-wrap")) {
        st.text_wrap = match v.trim() {
            "break-word" | "anywhere" => TextWrap::BreakWord,
            _ => TextWrap::Normal,
        };
    }
    // word-break: break-all is stronger — it breaks anywhere, not just to avoid
    // an overflow.
    if let Some(v) = p.get("word-break") {
        if v.trim() == "break-all" {
            st.text_wrap = TextWrap::Anywhere;
        }
    }
    if let Some(v) = p.get("opacity") {
        if let Ok(o) = first(v).parse::<f32>() {
            st.opacity = o.clamp(0.0, 1.0);
        }
    }
    if let Some(v) = p.get("flex-direction") {
        st.axis = if v.trim() == "column" { Axis::Column } else { Axis::Row };
    }
    if let Some(v) = p.get("justify-content") {
        st.justify = parse_justify(v);
    }
    if let Some(v) = p.get("align-items") {
        st.align = parse_align(v);
    }
    // Cross-/inline-axis self and content alignment (flex + grid).
    if let Some(v) = p.get("align-self") {
        st.align_self = parse_align(v);
    }
    if let Some(v) = p.get("justify-self") {
        st.justify_self = parse_align(v);
    }
    if let Some(v) = p.get("justify-items") {
        st.justify_items = parse_align(v);
    }
    if let Some(v) = p.get("align-content") {
        st.align_content = parse_justify(v);
    }
    // `row-gap` / `column-gap` override the `gap` shorthand per axis.
    if let Some(px) = p.get("row-gap").and_then(|v| parse_px(first(v))) {
        st.row_gap = Some(px);
    }
    if let Some(px) = p.get("column-gap").and_then(|v| parse_px(first(v))) {
        st.column_gap = Some(px);
    }
    if let Some(v) = p.get("position") {
        st.position = match v.trim() {
            "absolute" | "fixed" => Position::Absolute,
            _ => Position::Relative,
        };
    }
    for (i, side) in ["top", "right", "bottom", "left"].iter().enumerate() {
        if let Some(v) = p.get(*side) {
            st.inset[i] = if first(v) == "auto" { None } else { parse_len(first(v)) };
        }
    }
    if let Some(v) = p.get("aspect-ratio") {
        st.aspect_ratio = parse_aspect_ratio(v);
    }
    // `background` (shorthand, may be a gradient) → `background-image` (gradient
    // or url, url not yet supported) → `background-color` (colour only).
    if let Some(v) = p
        .get("background")
        .or_else(|| p.get("background-image"))
        .or_else(|| p.get("background-color"))
    {
        st.background = parse_background(v);
    }
    if let Some(v) = p.get("transform") {
        st.transform = parse_transform(v);
    }
    if let Some(v) = p.get("box-shadow") {
        st.box_shadow = parse_box_shadow(v);
    }
    // `border-radius` shorthand (1–4 values, CSS diagonal grouping), then the
    // per-corner longhands override.
    if let Some(v) = p.get("border-radius") {
        st.radius = parse_border_radius(v);
    }
    for (i, corner) in [
        "border-top-left-radius",
        "border-top-right-radius",
        "border-bottom-right-radius",
        "border-bottom-left-radius",
    ]
    .iter()
    .enumerate()
    {
        if let Some(px) = p.get(*corner).and_then(|v| parse_px(first(v))) {
            st.radius[i] = px;
        }
    }
    // `auto`/`scroll` scroll (and clip); `hidden`/`clip` only clip. Any axis
    // saying so is enough — we have no per-axis overflow yet.
    let values = ["overflow", "overflow-x", "overflow-y"]
        .iter()
        .filter_map(|k| p.get(*k))
        .map(|v| v.trim());
    for v in values {
        match v {
            "auto" | "scroll" => st.overflow = Overflow::Scroll,
            "hidden" | "clip" if st.overflow != Overflow::Scroll => st.overflow = Overflow::Clip,
            _ => {}
        }
    }
    if let Some(v) = p.get("cursor") {
        // Only `pointer` maps to a distinct shape today; everything else keeps
        // the default arrow. The shell applies this on hover for tappable boxes.
        st.cursor = match v.trim() {
            "pointer" => Cursor::Pointer,
            _ => Cursor::Default,
        };
    }
    st
}

/// `flex: <grow> [<shrink> [<basis>]]`, plus the CSS keywords. Note the
/// shorthand's defaults differ from the initial values: `flex: 1` means
/// `1 1 0%`, not `1 1 auto`.
fn interpret_flex_shorthand(v: &str, st: &mut Style) {
    match v {
        "none" => {
            st.grow = 0.0;
            st.shrink = 0.0;
            st.basis = None;
            return;
        }
        "auto" => {
            st.grow = 1.0;
            st.shrink = 1.0;
            st.basis = None;
            return;
        }
        "initial" => {
            st.grow = 0.0;
            st.shrink = 1.0;
            st.basis = None;
            return;
        }
        _ => {}
    }

    let parts: Vec<&str> = v.split_whitespace().collect();
    let Some(grow) = parts.first().and_then(|g| g.parse::<f32>().ok()) else {
        return;
    };
    st.grow = grow;
    st.shrink = parts
        .get(1)
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(1.0)
        .max(0.0);
    st.basis = match parts.get(2) {
        Some(&"auto") | Some(&"content") => None,
        Some(b) => parse_len(b),
        // A bare `flex: 1` sizes purely from the free space.
        None => Some(Len::Px(0.0)),
    };
}

/// `align-items` / `align-self` / `justify-self` / `justify-items` keyword.
fn parse_align(v: &str) -> Option<Align> {
    match v.trim() {
        "center" => Some(Align::Center),
        "flex-end" | "end" => Some(Align::End),
        "stretch" => Some(Align::Stretch),
        "flex-start" | "start" => Some(Align::Start),
        _ => None,
    }
}

/// `justify-content` / `align-content` keyword.
fn parse_justify(v: &str) -> Option<Justify> {
    match v.trim() {
        "center" => Some(Justify::Center),
        "flex-end" | "end" => Some(Justify::End),
        "space-between" => Some(Justify::SpaceBetween),
        "space-around" => Some(Justify::SpaceAround),
        "flex-start" | "start" => Some(Justify::Start),
        _ => None,
    }
}

/// Parse a `background` / `background-image` / `background-color` value into a
/// solid colour or a gradient. `url(…)` and other image sources aren't handled.
fn parse_background(value: &str) -> Option<Background> {
    let v = value.trim();
    if let Some(inner) = gradient_args(v, "linear-gradient") {
        return parse_linear_gradient(inner).map(Background::Gradient);
    }
    if let Some(inner) = gradient_args(v, "radial-gradient") {
        return parse_radial_gradient(inner).map(Background::Gradient);
    }
    if let Some(inner) = gradient_args(v, "url") {
        // Strip surrounding quotes from the url; the runtime resolves the path.
        let src = inner.trim().trim_matches(|c| c == '"' || c == '\'');
        if !src.is_empty() {
            return Some(Background::Image(src.to_string()));
        }
    }
    parse_color(v).map(Background::Color)
}

/// The comma-separated argument text inside `<name>( … )`, if `v` is that call.
fn gradient_args<'a>(v: &'a str, name: &str) -> Option<&'a str> {
    v.strip_prefix(name)?.trim_start().strip_prefix('(')?.strip_suffix(')')
}

/// `linear-gradient([<angle> | to <side>,]? <stop>, <stop> …)`. Defaults to
/// `to bottom` (180°). Stops without a position are spread evenly.
fn parse_linear_gradient(inner: &str) -> Option<Gradient> {
    let mut parts = split_top_level_commas(inner);
    if parts.is_empty() {
        return None;
    }
    // A leading angle / `to <side>` sets the direction; otherwise it's a stop.
    let angle = parse_gradient_angle(parts[0].trim());
    if angle.is_some() {
        parts.remove(0);
    }
    let stops = parse_stops(&parts)?;
    Some(Gradient {
        kind: GradientKind::Linear {
            angle: angle.unwrap_or(std::f32::consts::PI), // default: to bottom
        },
        stops,
    })
}

/// `radial-gradient([shape/size/at …,]? <stop>, <stop> …)`. The prelude before
/// the first stop (shape, `at …`) is accepted and ignored — we always draw a
/// centred circle to the nearest edge.
fn parse_radial_gradient(inner: &str) -> Option<Gradient> {
    let mut parts = split_top_level_commas(inner);
    if parts.is_empty() {
        return None;
    }
    // If the first segment isn't a colour stop, treat it as the (ignored) config.
    if parse_color(first(parts[0].trim())).is_none() && !parts[0].trim().is_empty() {
        parts.remove(0);
    }
    let stops = parse_stops(&parts)?;
    Some(Gradient { kind: GradientKind::Radial, stops })
}

/// Parse the direction of a linear gradient: `<n>deg` (CSS: 0 = to top,
/// clockwise) or `to <side>`. Returns radians, or `None` if it's not a direction.
fn parse_gradient_angle(tok: &str) -> Option<f32> {
    if let Some(deg) = tok.strip_suffix("deg") {
        return deg.trim().parse::<f32>().ok().map(f32::to_radians);
    }
    if tok == "turn" {
        return None;
    }
    if let Some(rest) = tok.strip_suffix("turn") {
        return rest.trim().parse::<f32>().ok().map(|t| t * std::f32::consts::TAU);
    }
    let side = tok.strip_prefix("to ")?.trim();
    // CSS angles: to top = 0, to right = 90, to bottom = 180, to left = 270.
    let deg = match side {
        "top" => 0.0,
        "right" => 90.0,
        "bottom" => 180.0,
        "left" => 270.0,
        "top right" | "right top" => 45.0,
        "bottom right" | "right bottom" => 135.0,
        "bottom left" | "left bottom" => 225.0,
        "top left" | "left top" => 315.0,
        _ => return None,
    };
    Some(f32::to_radians(deg))
}

/// Parse `<color> [<pos>%]` stops. Missing positions are filled by spreading the
/// unspecified stops evenly between their specified neighbours (ends default to
/// 0% and 100%).
fn parse_stops(parts: &[&str]) -> Option<Vec<(Rgba, f32)>> {
    let mut colors = Vec::new();
    let mut positions: Vec<Option<f32>> = Vec::new();
    for part in parts {
        let part = part.trim();
        let mut toks = part.split_whitespace();
        let color = parse_color(toks.next()?)?;
        let pos = toks
            .next()
            .and_then(|p| p.strip_suffix('%'))
            .and_then(|p| p.trim().parse::<f32>().ok())
            .map(|p| (p / 100.0).clamp(0.0, 1.0));
        colors.push(color);
        positions.push(pos);
    }
    if colors.len() < 2 {
        return None;
    }
    // Fill missing positions: ends anchor to 0 and 1, interior gaps interpolate.
    let n = positions.len();
    positions[0].get_or_insert(0.0);
    positions[n - 1].get_or_insert(1.0);
    let mut i = 0;
    while i < n {
        if positions[i].is_some() {
            i += 1;
            continue;
        }
        let start = i - 1;
        let mut j = i;
        while j < n && positions[j].is_none() {
            j += 1;
        }
        let p0 = positions[start].unwrap();
        let p1 = positions[j].unwrap();
        let gap = j - start;
        for (k, slot) in (start + 1..j).enumerate() {
            positions[slot] = Some(p0 + (p1 - p0) * (k as f32 + 1.0) / gap as f32);
        }
        i = j;
    }
    Some(colors.into_iter().zip(positions.into_iter().map(Option::unwrap)).collect())
}

/// Split on top-level commas (ignoring commas inside `rgb( … )` etc.).
fn split_top_level_commas(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in value.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(value[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = value[start..].trim();
    if !last.is_empty() {
        out.push(last);
    }
    out
}

/// Parse a `transform` function list (`rotate(15deg) translate(4px, 0)`) into a
/// single affine `[a, b, c, d, e, f]`. Functions compose left-to-right (the
/// leftmost is outermost). `translate` percentages aren't supported. `None` if
/// nothing parsed.
fn parse_transform(value: &str) -> Option<Transform> {
    let mut m = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]; // identity
    let mut any = false;
    let mut rest = value.trim();
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().to_ascii_lowercase();
        let close = rest[open..].find(')')? + open;
        let args = &rest[open + 1..close];
        if let Some(f) = transform_fn(&name, args) {
            m = mat_mul(m, f);
            any = true;
        }
        rest = rest[close + 1..].trim_start();
    }
    any.then_some(m)
}

/// One `transform` function to an affine, or `None` if unrecognised.
fn transform_fn(name: &str, args: &str) -> Option<Transform> {
    let nums: Vec<&str> = args.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    let num = |i: usize| nums.get(i).and_then(|s| s.parse::<f32>().ok());
    match name {
        "translate" => {
            let tx = parse_px(nums.first()?)?;
            let ty = nums.get(1).and_then(|s| parse_px(s)).unwrap_or(0.0);
            Some([1.0, 0.0, 0.0, 1.0, tx, ty])
        }
        "translatex" => Some([1.0, 0.0, 0.0, 1.0, parse_px(nums.first()?)?, 0.0]),
        "translatey" => Some([1.0, 0.0, 0.0, 1.0, 0.0, parse_px(nums.first()?)?]),
        "scale" => {
            let sx = num(0)?;
            let sy = num(1).unwrap_or(sx);
            Some([sx, 0.0, 0.0, sy, 0.0, 0.0])
        }
        "scalex" => Some([num(0)?, 0.0, 0.0, 1.0, 0.0, 0.0]),
        "scaley" => Some([1.0, 0.0, 0.0, num(0)?, 0.0, 0.0]),
        "rotate" => {
            let (sin, cos) = parse_angle(nums.first()?)?.sin_cos();
            Some([cos, sin, -sin, cos, 0.0, 0.0])
        }
        _ => None,
    }
}

/// Multiply two affines (`a` applied after `b`): `mat_mul(a, b)(p) = a(b(p))`.
fn mat_mul(a: Transform, b: Transform) -> Transform {
    let [a1, b1, c1, d1, e1, f1] = a;
    let [a2, b2, c2, d2, e2, f2] = b;
    [
        a1 * a2 + c1 * b2,
        b1 * a2 + d1 * b2,
        a1 * c2 + c1 * d2,
        b1 * c2 + d1 * d2,
        a1 * e2 + c1 * f2 + e1,
        b1 * e2 + d1 * f2 + f1,
    ]
}

/// An angle in `deg` (default), `rad`, `turn`, or `grad`, returned in radians.
fn parse_angle(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(v) = s.strip_suffix("deg") {
        return v.trim().parse::<f32>().ok().map(f32::to_radians);
    }
    if let Some(v) = s.strip_suffix("grad") {
        return v.trim().parse::<f32>().ok().map(|g| g * std::f32::consts::PI / 200.0);
    }
    if let Some(v) = s.strip_suffix("turn") {
        return v.trim().parse::<f32>().ok().map(|t| t * std::f32::consts::TAU);
    }
    if let Some(v) = s.strip_suffix("rad") {
        return v.trim().parse::<f32>().ok();
    }
    s.parse::<f32>().ok().map(f32::to_radians)
}

/// `box-shadow: <dx> <dy> <blur>? <spread>? <color>?`, optionally `inset`.
/// A single shadow only; multiple comma-separated shadows take the first.
/// `none` yields no shadow.
fn parse_box_shadow(value: &str) -> Option<BoxShadow> {
    let first = value.split(',').next().unwrap_or(value).trim();
    if first.is_empty() || first == "none" {
        return None;
    }
    let mut lengths = Vec::new();
    let mut color_parts = Vec::new();
    let mut inset = false;
    for tok in first.split_whitespace() {
        if tok == "inset" {
            inset = true;
        } else if let Some(px) = parse_px(tok) {
            lengths.push(px);
        } else {
            color_parts.push(tok);
        }
    }
    // Offsets are required; blur and spread default to 0, colour to black.
    if lengths.len() < 2 {
        return None;
    }
    let color = parse_color(&color_parts.join(" ")).unwrap_or(Rgba::new(0.0, 0.0, 0.0, 1.0));
    Some(BoxShadow {
        dx: lengths[0],
        dy: lengths[1],
        blur: lengths.get(2).copied().unwrap_or(0.0),
        spread: lengths.get(3).copied().unwrap_or(0.0),
        color,
        inset,
    })
}

/// `line-height`: a unitless number (× font size), a length, or `normal` (→
/// `None`, keep the font's own metrics).
fn parse_line_height(v: &str, font_size: f32) -> Option<f32> {
    let s = first(v);
    if s == "normal" {
        return None;
    }
    if s.ends_with("px") || s.ends_with("rem") || s.ends_with("em") {
        // `em` is relative to font size; `parse_len` handles px/rem.
        if let Some(em) = s.strip_suffix("em").filter(|e| !e.ends_with('r')) {
            return em.parse::<f32>().ok().map(|n| n * font_size);
        }
        return parse_len(s).and_then(|l| match l {
            Len::Px(px) => Some(px),
            _ => None,
        });
    }
    // A bare number multiplies the font size (the usual CSS form).
    s.parse::<f32>().ok().map(|n| n * font_size)
}

/// `letter-spacing` / `word-spacing`: a px length, or `normal` (→ no extra).
fn parse_spacing(v: &str) -> Option<f32> {
    match first(v) {
        "normal" => None,
        s => parse_px(s),
    }
}

/// `aspect-ratio`: a plain number, or a `<w> / <h>` ratio.
fn parse_aspect_ratio(v: &str) -> Option<f32> {
    if let Some((w, h)) = v.split_once('/') {
        let (w, h) = (w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?);
        return (h != 0.0).then_some(w / h);
    }
    v.trim().parse::<f32>().ok().filter(|r| *r > 0.0)
}

fn first(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or(s)
}

fn parse_px(s: &str) -> Option<f32> {
    let s = s.trim();
    let s = s.strip_suffix("px").unwrap_or(s);
    s.parse::<f32>().ok()
}

/// One `rem` in pixels (root font size).
const REM_PX: f32 = 16.0;

/// Parse a length: `px`, `%`, `rem`, `vw`, `vh`/`dvh`. (`rem` resolves to px;
/// `dvh` is treated as `vh` since we have no dynamic browser chrome.)
fn parse_len(s: &str) -> Option<Len> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        return pct.trim().parse::<f32>().ok().map(|v| Len::Pct(v / 100.0));
    }
    if let Some(n) = s.strip_suffix("dvh").or_else(|| s.strip_suffix("vh")) {
        return n.trim().parse::<f32>().ok().map(Len::Vh);
    }
    if let Some(n) = s.strip_suffix("vw") {
        return n.trim().parse::<f32>().ok().map(Len::Vw);
    }
    if let Some(n) = s.strip_suffix("rem") {
        return n.trim().parse::<f32>().ok().map(|v| Len::Px(v * REM_PX));
    }
    let n = s.strip_suffix("px").unwrap_or(s);
    n.parse::<f32>().ok().map(Len::Px)
}

/// Parse a `grid-template-columns`/`-rows` value into tracks: `1fr`, `100px`,
/// `auto`, and `minmax(min, max)` (e.g. `minmax(0, 1fr)`, which lets a track
/// shrink below its content instead of overflowing the grid).
/// `grid-column` / `grid-row` shorthand: `<start> [/ <end>]`. A missing end is
/// `auto` (span 1). Each side is a line index, `span <n>`, or `auto`.
fn parse_grid_shorthand(value: &str) -> (GridPlace, GridPlace) {
    let mut parts = value.splitn(2, '/');
    let start = parts.next().map(parse_grid_place).unwrap_or_default();
    let end = parts.next().map(parse_grid_place).unwrap_or_default();
    (start, end)
}

/// One placement endpoint: `auto`, a (possibly negative) line index, or
/// `span <n>`. Named lines aren't supported.
fn parse_grid_place(side: &str) -> GridPlace {
    let s = side.trim();
    if let Some(rest) = s.strip_prefix("span") {
        return rest.trim().parse::<u16>().ok().map_or(GridPlace::Auto, GridPlace::Span);
    }
    match s.parse::<i16>() {
        Ok(i) if i != 0 => GridPlace::Line(i),
        _ => GridPlace::Auto,
    }
}

fn parse_tracks(value: &str) -> Vec<Track> {
    split_top_level(value)
        .into_iter()
        .map(|tok| {
            if let Some(args) = tok
                .strip_prefix("minmax(")
                .and_then(|s| s.strip_suffix(')'))
            {
                let mut parts = args.split(',');
                let lo = parts.next().map(parse_track_side).unwrap_or(TrackSide::Auto);
                let hi = parts.next().map(parse_track_side).unwrap_or(TrackSide::Auto);
                Track::MinMax(lo, hi)
            } else {
                match parse_track_side(tok) {
                    TrackSide::Px(v) => Track::Px(v),
                    TrackSide::Fr(f) => Track::Fr(f),
                    TrackSide::Auto => Track::Auto,
                }
            }
        })
        .collect()
}

/// A single track value: `Nfr`, `auto`, or a length (default `auto`).
fn parse_track_side(tok: &str) -> TrackSide {
    let tok = tok.trim();
    if let Some(fr) = tok.strip_suffix("fr") {
        TrackSide::Fr(fr.trim().parse().unwrap_or(1.0))
    } else if tok == "auto" {
        TrackSide::Auto
    } else {
        parse_px(tok).map(TrackSide::Px).unwrap_or(TrackSide::Auto)
    }
}

/// Split a track list on whitespace, but keep a `minmax( … )` group — which
/// contains its own spaces and comma — together as one token.
fn split_top_level(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start: Option<usize> = None;
    for (i, c) in value.char_indices() {
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
        }
        if c.is_whitespace() && depth == 0 {
            if let Some(s) = start.take() {
                out.push(value[s..i].trim());
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(value[s..].trim());
    }
    out.into_iter().filter(|t| !t.is_empty()).collect()
}

/// Expand a 1–4 value shorthand (`5px`, `5px 10px`, `1 2 3`, `1 2 3 4`) into
/// per-side lengths, CSS order (top, right, bottom, left).
fn parse_shorthand_sides(value: &str) -> Sides {
    let v: Vec<f32> = value
        .split_whitespace()
        .filter_map(parse_px)
        .collect();
    match v.len() {
        1 => Sides::uniform(v[0]),
        2 => Sides {
            top: v[0],
            right: v[1],
            bottom: v[0],
            left: v[1],
        },
        3 => Sides {
            top: v[0],
            right: v[1],
            bottom: v[2],
            left: v[1],
        },
        n if n >= 4 => Sides {
            top: v[0],
            right: v[1],
            bottom: v[2],
            left: v[3],
        },
        _ => Sides::default(),
    }
}

/// Parse the `border-radius` shorthand into `[TL, TR, BR, BL]`. Unlike the box
/// shorthands, border-radius groups by diagonal: 1 value = all; 2 = TL/BR, TR/BL;
/// 3 = TL, TR/BL, BR; 4 = TL, TR, BR, BL. An elliptical `h / v` form is reduced
/// to its horizontal radii (we only draw circular corners).
fn parse_border_radius(value: &str) -> [f32; 4] {
    let horizontal = value.split('/').next().unwrap_or(value);
    let v: Vec<f32> = horizontal.split_whitespace().filter_map(parse_px).collect();
    match v.len() {
        1 => [v[0]; 4],
        2 => [v[0], v[1], v[0], v[1]],
        3 => [v[0], v[1], v[2], v[1]],
        n if n >= 4 => [v[0], v[1], v[2], v[3]],
        _ => [0.0; 4],
    }
}

/// Resolve `padding`/`margin` from the shorthand plus any `-top/-right/-bottom/
/// -left` longhand overrides.
fn box_sides(p: &HashMap<String, String>, prop: &str) -> Sides {
    let mut sides = p
        .get(prop)
        .map(|v| parse_shorthand_sides(v))
        .unwrap_or_default();
    for side in ["top", "right", "bottom", "left"] {
        if let Some(v) = p.get(&format!("{prop}-{side}")) {
            if let Some(px) = parse_px(first(v)) {
                set_side(&mut sides, side, px);
            }
        }
    }
    sides
}

/// Parse `border` box-model props: `border`, `border-width`, `border-color`,
/// `border-<side>`, `border-<side>-width`.
fn interpret_border(p: &HashMap<String, String>, st: &mut Style) {
    // `border: <width> <style> <color>` shorthand.
    if let Some(v) = p.get("border") {
        let (w, c) = parse_border(v);
        st.border = Sides::uniform(w);
        if c.is_some() {
            st.border_color = c;
        }
    }
    if let Some(v) = p.get("border-width") {
        st.border = parse_shorthand_sides(v);
    }
    if let Some(v) = p.get("border-color") {
        st.border_color = parse_color(v);
    }
    for side in ["top", "right", "bottom", "left"] {
        if let Some(v) = p.get(&format!("border-{side}")) {
            let (w, c) = parse_border(v);
            set_side(&mut st.border, side, w);
            if c.is_some() {
                st.border_color = c;
            }
        }
        if let Some(v) = p.get(&format!("border-{side}-width")) {
            if let Some(px) = parse_px(first(v)) {
                set_side(&mut st.border, side, px);
            }
        }
    }
}

fn set_side(sides: &mut Sides, side: &str, value: f32) {
    match side {
        "top" => sides.top = value,
        "right" => sides.right = value,
        "bottom" => sides.bottom = value,
        "left" => sides.left = value,
        _ => {}
    }
}

/// Parse a `border` value into `(width, color)`; the line style token is ignored.
fn parse_border(value: &str) -> (f32, Option<Rgba>) {
    let mut width = 0.0;
    let mut color = None;
    for token in value.split_whitespace() {
        if let Some(px) = parse_px(token) {
            width = px;
        } else if let Some(c) = parse_color(token) {
            color = Some(c);
        }
    }
    (width, color)
}

/// Parse `font-weight`: keywords or a numeric 100–900.
fn parse_weight(s: &str) -> Option<u16> {
    match s.trim() {
        "normal" => Some(400),
        "bold" => Some(700),
        "lighter" => Some(300),
        "bolder" => Some(800),
        other => other.parse::<u16>().ok(),
    }
}

/// Parse `text-align`.
fn parse_text_align(s: &str) -> TextAlign {
    match s.trim() {
        "center" => TextAlign::Center,
        "right" | "end" => TextAlign::End,
        "justify" => TextAlign::Justify,
        _ => TextAlign::Start,
    }
}

fn parse_color(s: &str) -> Option<Rgba> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    if s.starts_with("rgb") {
        return parse_rgb(s);
    }
    if s.eq_ignore_ascii_case("transparent") {
        return Some(Rgba::new(0.0, 0.0, 0.0, 0.0));
    }
    // Named colors. This matters more than it looks: lightningcss *minifies* hex
    // to the shorter keyword (`#ff0000` → `red`), so without this table a plain
    // `color: #ff0000` would silently fall back to the default.
    named_color(&s.to_ascii_lowercase()).and_then(parse_hex)
}

/// The CSS named colors, as their hex value (without `#`). Covers the full CSS
/// Color Level 4 keyword list so any keyword lightningcss emits round-trips.
fn named_color(name: &str) -> Option<&'static str> {
    let hex = match name {
        "aliceblue" => "f0f8ff", "antiquewhite" => "faebd7", "aqua" => "00ffff",
        "aquamarine" => "7fffd4", "azure" => "f0ffff", "beige" => "f5f5dc",
        "bisque" => "ffe4c4", "black" => "000000", "blanchedalmond" => "ffebcd",
        "blue" => "0000ff", "blueviolet" => "8a2be2", "brown" => "a52a2a",
        "burlywood" => "deb887", "cadetblue" => "5f9ea0", "chartreuse" => "7fff00",
        "chocolate" => "d2691e", "coral" => "ff7f50", "cornflowerblue" => "6495ed",
        "cornsilk" => "fff8dc", "crimson" => "dc143c", "cyan" => "00ffff",
        "darkblue" => "00008b", "darkcyan" => "008b8b", "darkgoldenrod" => "b8860b",
        "darkgray" | "darkgrey" => "a9a9a9", "darkgreen" => "006400",
        "darkkhaki" => "bdb76b", "darkmagenta" => "8b008b", "darkolivegreen" => "556b2f",
        "darkorange" => "ff8c00", "darkorchid" => "9932cc", "darkred" => "8b0000",
        "darksalmon" => "e9967a", "darkseagreen" => "8fbc8f", "darkslateblue" => "483d8b",
        "darkslategray" | "darkslategrey" => "2f4f4f", "darkturquoise" => "00ced1",
        "darkviolet" => "9400d3", "deeppink" => "ff1493", "deepskyblue" => "00bfff",
        "dimgray" | "dimgrey" => "696969", "dodgerblue" => "1e90ff",
        "firebrick" => "b22222", "floralwhite" => "fffaf0", "forestgreen" => "228b22",
        "fuchsia" => "ff00ff", "gainsboro" => "dcdcdc", "ghostwhite" => "f8f8ff",
        "gold" => "ffd700", "goldenrod" => "daa520", "gray" | "grey" => "808080",
        "green" => "008000", "greenyellow" => "adff2f", "honeydew" => "f0fff0",
        "hotpink" => "ff69b4", "indianred" => "cd5c5c", "indigo" => "4b0082",
        "ivory" => "fffff0", "khaki" => "f0e68c", "lavender" => "e6e6fa",
        "lavenderblush" => "fff0f5", "lawngreen" => "7cfc00", "lemonchiffon" => "fffacd",
        "lightblue" => "add8e6", "lightcoral" => "f08080", "lightcyan" => "e0ffff",
        "lightgoldenrodyellow" => "fafad2", "lightgray" | "lightgrey" => "d3d3d3",
        "lightgreen" => "90ee90", "lightpink" => "ffb6c1", "lightsalmon" => "ffa07a",
        "lightseagreen" => "20b2aa", "lightskyblue" => "87cefa", "lightslategray" | "lightslategrey" => "778899",
        "lightsteelblue" => "b0c4de", "lightyellow" => "ffffe0", "lime" => "00ff00",
        "limegreen" => "32cd32", "linen" => "faf0e6", "magenta" => "ff00ff",
        "maroon" => "800000", "mediumaquamarine" => "66cdaa", "mediumblue" => "0000cd",
        "mediumorchid" => "ba55d3", "mediumpurple" => "9370db", "mediumseagreen" => "3cb371",
        "mediumslateblue" => "7b68ee", "mediumspringgreen" => "00fa9a", "mediumturquoise" => "48d1cc",
        "mediumvioletred" => "c71585", "midnightblue" => "191970", "mintcream" => "f5fffa",
        "mistyrose" => "ffe4e1", "moccasin" => "ffe4b5", "navajowhite" => "ffdead",
        "navy" => "000080", "oldlace" => "fdf5e6", "olive" => "808000",
        "olivedrab" => "6b8e23", "orange" => "ffa500", "orangered" => "ff4500",
        "orchid" => "da70d6", "palegoldenrod" => "eee8aa", "palegreen" => "98fb98",
        "paleturquoise" => "afeeee", "palevioletred" => "db7093", "papayawhip" => "ffefd5",
        "peachpuff" => "ffdab9", "peru" => "cd853f", "pink" => "ffc0cb",
        "plum" => "dda0dd", "powderblue" => "b0e0e6", "purple" => "800080",
        "rebeccapurple" => "663399", "red" => "ff0000", "rosybrown" => "bc8f8f",
        "royalblue" => "4169e1", "saddlebrown" => "8b4513", "salmon" => "fa8072",
        "sandybrown" => "f4a460", "seagreen" => "2e8b57", "seashell" => "fff5ee",
        "sienna" => "a0522d", "silver" => "c0c0c0", "skyblue" => "87ceeb",
        "slateblue" => "6a5acd", "slategray" | "slategrey" => "708090", "snow" => "fffafa",
        "springgreen" => "00ff7f", "steelblue" => "4682b4", "tan" => "d2b48c",
        "teal" => "008080", "thistle" => "d8bfd8", "tomato" => "ff6347",
        "turquoise" => "40e0d0", "violet" => "ee82ee", "wheat" => "f5deb3",
        "white" => "ffffff", "whitesmoke" => "f5f5f5", "yellow" => "ffff00",
        "yellowgreen" => "9acd32",
        _ => return None,
    };
    Some(hex)
}

fn parse_hex(hex: &str) -> Option<Rgba> {
    let expand = |c: char| -> u8 { u8::from_str_radix(&format!("{c}{c}"), 16).unwrap_or(0) };
    let bytes: Vec<char> = hex.chars().collect();
    let (r, g, b, a) = match bytes.len() {
        3 => (expand(bytes[0]), expand(bytes[1]), expand(bytes[2]), 255),
        6 => (
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            255,
        ),
        8 => (
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            u8::from_str_radix(&hex[6..8], 16).ok()?,
        ),
        _ => return None,
    };
    Some(Rgba::new(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ))
}

#[cfg(test)]
mod tests {
    use super::{build_styled_tree, interpolate, interpret, Len, Locals};
    use rux_script::Builder;
    use std::collections::HashMap;

    #[test]
    fn box_model_shorthand_sides_and_border() {
        let mut p = HashMap::new();
        p.insert("padding".to_string(), "4px 8px".to_string()); // vertical | horizontal
        p.insert("padding-left".to_string(), "20px".to_string()); // longhand override
        p.insert("margin".to_string(), "10px".to_string());
        p.insert("border".to_string(), "2px solid #ff0000".to_string());
        p.insert("border-bottom-width".to_string(), "5px".to_string());

        let st = interpret(&p);
        assert_eq!((st.padding.top, st.padding.right, st.padding.bottom, st.padding.left), (4.0, 8.0, 4.0, 20.0));
        assert_eq!(st.margin.top, 10.0);
        assert_eq!(st.border.top, 2.0);
        assert_eq!(st.border.bottom, 5.0); // per-side width override
        assert_eq!(st.border_color.map(|c| c.r), Some(1.0)); // #ff0000 → red
    }

    #[test]
    fn flex_longhands_and_shorthand() {
        let flex = |v: &str| {
            let mut p = HashMap::new();
            p.insert("flex".to_string(), v.to_string());
            let st = interpret(&p);
            (st.grow, st.shrink, st.basis)
        };
        // The shorthand's omitted basis is 0, not auto — a bare `flex: 1` sizes
        // purely from the free space.
        assert_eq!(flex("1"), (1.0, 1.0, Some(Len::Px(0.0))));
        assert_eq!(flex("1 0 auto"), (1.0, 0.0, None));
        assert_eq!(flex("2 3 120px"), (2.0, 3.0, Some(Len::Px(120.0))));
        assert_eq!(flex("none"), (0.0, 0.0, None));

        let mut p = HashMap::new();
        p.insert("flex".to_string(), "1".to_string());
        p.insert("flex-shrink".to_string(), "0".to_string()); // longhand wins
        p.insert("flex-wrap".to_string(), "wrap".to_string());
        p.insert("opacity".to_string(), "0.45".to_string());
        let st = interpret(&p);
        assert_eq!(st.shrink, 0.0);
        assert!(st.wrap);
        assert_eq!(st.opacity, 0.45);
    }

    #[test]
    fn border_radius_shorthand_diagonal_grouping_and_longhands() {
        // border-radius groups by diagonal, unlike padding/margin: 2 values are
        // TL/BR then TR/BL; 3 are TL, TR/BL, BR.
        assert_eq!(super::parse_border_radius("8px"), [8.0, 8.0, 8.0, 8.0]);
        assert_eq!(super::parse_border_radius("8px 4px"), [8.0, 4.0, 8.0, 4.0]);
        assert_eq!(super::parse_border_radius("1px 2px 3px"), [1.0, 2.0, 3.0, 2.0]);
        assert_eq!(super::parse_border_radius("1px 2px 3px 4px"), [1.0, 2.0, 3.0, 4.0]);
        // Elliptical `h / v` reduces to the horizontal radii.
        assert_eq!(super::parse_border_radius("10px / 20px"), [10.0, 10.0, 10.0, 10.0]);

        // A per-corner longhand overrides just its corner (index 1 = top-right).
        let mut p = HashMap::new();
        p.insert("border-radius".to_string(), "5px".to_string());
        p.insert("border-top-right-radius".to_string(), "12px".to_string());
        assert_eq!(interpret(&p).radius, [5.0, 12.0, 5.0, 5.0]);
    }

    #[test]
    fn grid_placement_parses_lines_and_spans() {
        use super::GridPlace;
        let place = |css: &str| {
            let mut p = HashMap::new();
            p.insert("grid-column".to_string(), css.to_string());
            interpret(&p).grid_column
        };
        assert_eq!(place("1 / 3"), (GridPlace::Line(1), GridPlace::Line(3)));
        assert_eq!(place("2"), (GridPlace::Line(2), GridPlace::Auto));
        assert_eq!(place("span 2"), (GridPlace::Span(2), GridPlace::Auto));
        assert_eq!(place("1 / span 2"), (GridPlace::Line(1), GridPlace::Span(2)));
        assert_eq!(place("-1"), (GridPlace::Line(-1), GridPlace::Auto));

        // The -end longhand overrides just the end of the shorthand.
        let mut p = HashMap::new();
        p.insert("grid-row".to_string(), "1 / 2".to_string());
        p.insert("grid-row-end".to_string(), "span 3".to_string());
        assert_eq!(interpret(&p).grid_row, (GridPlace::Line(1), GridPlace::Span(3)));
    }

    #[test]
    fn named_and_hex_colors_resolve() {
        use super::parse_color;
        // The landmine: lightningcss minifies `#ff0000` to `red`, so the keyword
        // path has to work or a plain red silently falls back to the default.
        assert_eq!(parse_color("red").map(|c| (c.r, c.g, c.b)), Some((1.0, 0.0, 0.0)));
        assert!(parse_color("REBECCApurple").is_some()); // case-insensitive
        assert_eq!(parse_color("#000000").map(|c| c.r), Some(0.0));
        assert_eq!(parse_color("transparent").map(|c| c.a), Some(0.0));
        assert!(parse_color("notacolor").is_none());
    }

    #[test]
    fn decodes_html_entities_in_text() {
        use super::decode_entities;
        assert_eq!(decode_entities("A &amp; B"), "A & B");
        assert_eq!(decode_entities("&lt;tag&gt; &quot;q&quot;"), "<tag> \"q\"");
        assert_eq!(decode_entities("&#38; &#x26;"), "& &");
        assert_eq!(decode_entities("plain text"), "plain text");
        // An unrecognised or malformed entity is left as written.
        assert_eq!(decode_entities("R&D, AT&T"), "R&D, AT&T");
        assert_eq!(decode_entities("&notanentity;"), "&notanentity;");
    }

    #[test]
    fn parses_and_composes_transforms() {
        use super::parse_transform;
        assert_eq!(parse_transform("translate(10px, 20px)").unwrap(), [1.0, 0.0, 0.0, 1.0, 10.0, 20.0]);
        assert_eq!(parse_transform("scale(2, 3)").unwrap(), [2.0, 0.0, 0.0, 3.0, 0.0, 0.0]);

        // rotate(90deg) maps (x, y) → (-y, x): a≈0, b≈1, c≈-1, d≈0.
        let r = parse_transform("rotate(90deg)").unwrap();
        assert!(r[0].abs() < 1e-4 && (r[1] - 1.0).abs() < 1e-4);
        assert!((r[2] + 1.0).abs() < 1e-4 && r[3].abs() < 1e-4);

        // Left-to-right composition: rotate(90) ∘ translate(10,0) moves the
        // translation into the rotated frame, so it ends up as (0, 10).
        let c = parse_transform("rotate(90deg) translate(10px, 0)").unwrap();
        assert!(c[4].abs() < 1e-3 && (c[5] - 10.0).abs() < 1e-3);

        assert!(parse_transform("none").is_none());
    }

    #[test]
    fn parses_gradients_direction_and_stops() {
        use super::parse_background;
        use rux_layout::{Background, GradientKind};
        use std::f32::consts::{FRAC_PI_2, PI};

        let grad = |css: &str| match parse_background(css) {
            Some(Background::Gradient(g)) => g,
            other => panic!("expected a gradient, got {other:?}"),
        };

        // 90deg → to the right; two stops anchor to 0 and 1.
        let g = grad("linear-gradient(90deg, red, blue)");
        assert!(matches!(g.kind, GradientKind::Linear { angle } if (angle - FRAC_PI_2).abs() < 1e-4));
        assert_eq!(g.stops.len(), 2);
        assert_eq!(g.stops[0].1, 0.0);
        assert_eq!(g.stops[1].1, 1.0);
        assert_eq!(g.stops[0].0.r, 1.0); // red
        assert_eq!(g.stops[1].0.b, 1.0); // blue

        // No direction → default `to bottom` (π); the middle stop spreads to 50%.
        let g = grad("linear-gradient(red, lime, blue)");
        assert!(matches!(g.kind, GradientKind::Linear { angle } if (angle - PI).abs() < 1e-4));
        assert!((g.stops[1].1 - 0.5).abs() < 1e-4);

        // Explicit positions are honoured; `to right` is 90°.
        let g = grad("linear-gradient(to right, red 10%, blue 80%)");
        assert!(matches!(g.kind, GradientKind::Linear { angle } if (angle - FRAC_PI_2).abs() < 1e-4));
        assert!((g.stops[0].1 - 0.1).abs() < 1e-4);
        assert!((g.stops[1].1 - 0.8).abs() < 1e-4);

        // Radial: the shape prelude is ignored; stops still parse.
        let g = grad("radial-gradient(circle, red, blue)");
        assert!(matches!(g.kind, GradientKind::Radial));
        assert_eq!(g.stops.len(), 2);

        // A plain colour is still a colour, not a gradient.
        assert!(matches!(parse_background("#123456"), Some(Background::Color(_))));

        // `url(…)` is an image background; quotes are stripped.
        assert!(matches!(parse_background("url(assets/logo.png)"), Some(Background::Image(s)) if s == "assets/logo.png"));
        assert!(matches!(parse_background("url('a b.png')"), Some(Background::Image(s)) if s == "a b.png"));
    }

    #[test]
    fn maps_alignment_gap_position_and_aspect_ratio() {
        use super::{Align, Justify, Len, Position};
        let mut p = HashMap::new();
        p.insert("align-self".to_string(), "center".to_string());
        p.insert("justify-self".to_string(), "end".to_string());
        p.insert("align-content".to_string(), "space-between".to_string());
        p.insert("row-gap".to_string(), "8px".to_string());
        p.insert("column-gap".to_string(), "12px".to_string());
        p.insert("position".to_string(), "absolute".to_string());
        p.insert("top".to_string(), "10px".to_string());
        p.insert("left".to_string(), "auto".to_string());
        p.insert("aspect-ratio".to_string(), "16 / 9".to_string());

        let st = interpret(&p);
        assert!(matches!(st.align_self, Some(Align::Center)));
        assert!(matches!(st.justify_self, Some(Align::End)));
        assert!(matches!(st.align_content, Some(Justify::SpaceBetween)));
        assert_eq!(st.row_gap, Some(8.0));
        assert_eq!(st.column_gap, Some(12.0));
        assert!(matches!(st.position, Position::Absolute));
        assert!(matches!(st.inset[0], Some(Len::Px(v)) if v == 10.0)); // top
        assert!(st.inset[3].is_none()); // left: auto
        assert!(st.aspect_ratio.is_some_and(|r| (r - 16.0 / 9.0).abs() < 1e-4));
    }

    #[test]
    fn parses_grid_tracks_including_minmax() {
        use super::{parse_tracks, Track, TrackSide};
        let tracks = parse_tracks("minmax(0, 1fr) 100px auto minmax(120px, 1fr)");
        assert_eq!(tracks.len(), 4);
        assert!(matches!(
            tracks[0],
            Track::MinMax(TrackSide::Px(0.0), TrackSide::Fr(f)) if f == 1.0
        ));
        assert!(matches!(tracks[1], Track::Px(v) if v == 100.0));
        assert!(matches!(tracks[2], Track::Auto));
        assert!(matches!(
            tracks[3],
            Track::MinMax(TrackSide::Px(v), TrackSide::Fr(_)) if v == 120.0
        ));
    }

    #[test]
    fn image_element_carries_its_src() {
        let src = r#"<template><screen><image src="assets/logo.png" /></screen></template>"#;
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut e = Builder::new().build("").unwrap();
        let root = build_styled_tree(&sfc, &HashMap::new(), &mut e).unwrap();
        let img = root.children[0].image.as_ref().expect("image node");
        assert_eq!(img.src, "assets/logo.png");
    }

    #[test]
    fn interpolates_bindings() {
        let mut e = Builder::new()
            .build(r#"let level = signal(82); let who = signal("Cam");"#)
            .unwrap();
        let locals = Locals::new();
        assert_eq!(interpolate("{{ level }}%", &mut e, &locals), "82%");
        assert_eq!(interpolate("Hi {{ who }}!", &mut e, &locals), "Hi Cam!");
        assert_eq!(interpolate("plain text", &mut e, &locals), "plain text");
        assert_eq!(interpolate("{{ missing }}!", &mut e, &locals), "!"); // unknown → empty
    }

    #[test]
    fn expands_r_for_and_r_if_chain() {
        let src = r#"
            <template>
              <screen>
                <view r-for="n in nums"><text>{{ n }}</text></view>
                <text r-if="level < 5">low</text>
                <text r-elif="level < 50">mid</text>
                <text r-else>high</text>
              </screen>
            </template>
            <script> let nums = signal([1, 2, 3]); let level = signal(10); </script>
        "#;
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let root = build_styled_tree(&sfc, &HashMap::new(), &mut engine).unwrap();

        // 3 views from r-for + exactly one branch (level=10 → the r-elif "mid").
        assert_eq!(root.children.len(), 4);
        let mid = root.children[3].text.as_ref().unwrap();
        assert_eq!(mid.text, "mid");
    }

    #[test]
    fn r_for_tap_handler_captures_the_loop_variable() {
        let src = r#"
            <template>
              <screen>
                <view r-for="item in items" @tap="picked = item">
                  <text>{{ item }}</text>
                </view>
              </screen>
            </template>
            <script> let items = signal(["Alpha", "Bravo", "Charlie"]); let picked = signal(""); </script>
        "#;
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let root = build_styled_tree(&sfc, &HashMap::new(), &mut engine).unwrap();

        // The second row's handler must carry its own loop value baked in, not a
        // bare `item` that resolves to nothing when it runs in global scope.
        let handler = root.children[1].on_tap.clone().expect("row has @tap");
        assert!(
            handler.contains("let item = \"Bravo\""),
            "loop value not baked into handler: {handler}"
        );

        // End to end: picked starts empty, running the third row's handler sets
        // it to that row's item (the bug was that it stayed empty forever).
        assert_eq!(engine.get_string("picked"), "");
        let third = root.children[2].on_tap.clone().unwrap();
        assert!(engine.run_handler(&third), "handler ran");
        assert_eq!(engine.get_string("picked"), "Charlie");
    }

    #[test]
    fn input_binds_model_and_shows_placeholder_then_value() {
        let src = r#"<template><screen>
                       <input r-model="name" placeholder="Type here" />
                     </screen></template>
                     <script> let name = signal(""); </script>"#;
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();

        let root = build_styled_tree(&sfc, &HashMap::new(), &mut engine).unwrap();
        let input = &root.children[0];
        assert_eq!(input.model.as_deref(), Some("name"), "r-model bound");
        // Empty signal → the placeholder is shown.
        assert_eq!(input.children[0].text.as_ref().unwrap().text, "Type here");

        // Simulate the shell editing the focused input, then rebuild.
        engine.set_string("name", "Cam");
        let root = build_styled_tree(&sfc, &HashMap::new(), &mut engine).unwrap();
        let input = &root.children[0];
        assert_eq!(input.children[0].text.as_ref().unwrap().text, "Cam");
    }

    #[test]
    fn select_carries_options_and_textarea_is_multiline() {
        let src = r#"<template><screen>
                       <input type="select" r-model="fruit" :options="fruits" />
                       <input type="textarea" r-model="notes" />
                     </screen></template>
                     <script>
                       let fruit = signal("pear");
                       let fruits = signal(["apple", "pear", "plum"]);
                       let notes = signal("");
                     </script>"#;
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let root = build_styled_tree(&sfc, &HashMap::new(), &mut engine).unwrap();

        // The select evaluates :options to strings and shows the bound value.
        let select = &root.children[0];
        assert_eq!(select.model.as_deref(), Some("fruit"));
        assert_eq!(
            select.options.as_ref().expect("select has options"),
            &vec!["apple".to_string(), "pear".to_string(), "plum".to_string()]
        );
        assert!(!select.multiline);
        assert_eq!(select.children[0].text.as_ref().unwrap().text, "pear");

        // The textarea is a multiline input (Enter → newline in the shell).
        let textarea = &root.children[1];
        assert!(textarea.multiline);
        assert!(textarea.options.is_none());
    }

    #[test]
    fn expands_component_with_props() {
        let main = rux_parser::parse_sfc(
            r#"<template>
                 <screen><stat :label="title" :value="level" /></screen>
               </template>
               <script> let level = signal(82); let title = signal("Battery"); </script>"#,
        )
        .unwrap();
        let stat = rux_parser::parse_sfc(
            r#"<template>
                 <view><text>{{ label }}: {{ value }}</text></view>
               </template>"#,
        )
        .unwrap();

        let mut components = HashMap::new();
        components.insert("stat".to_string(), stat);

        let mut engine = Builder::new().build(&main.script).unwrap();
        let root = build_styled_tree(&main, &components, &mut engine).unwrap();

        // screen → (expanded stat) view → text "Battery: 82"
        let view = &root.children[0];
        let text = view.children[0].text.as_ref().unwrap();
        assert_eq!(text.text, "Battery: 82");
    }

    // ── Combinators ─────────────────────────────────────────────────────────
    //
    // These test `matches_chain` directly so both the positive and the negative
    // case are asserted: the bug being fixed here made `>`, `+` and `~` behave
    // as descendant, i.e. match elements they must NOT match.
    use super::{matches_chain, parse_selector, AncNode, ElemDesc};

    fn el(spec: &str) -> ElemDesc {
        // "tag.class.class#id" — tag optional, order flexible enough for tests.
        let mut d = ElemDesc { tag: String::new(), id: None, classes: Vec::new(), role: None };
        let mut rest = spec;
        while let Some(pos) = rest.find(['.', '#']) {
            if pos > 0 {
                d.tag = rest[..pos].to_string();
            }
            let marker = rest.as_bytes()[pos];
            let after = &rest[pos + 1..];
            let end = after.find(['.', '#']).unwrap_or(after.len());
            let name = after[..end].to_string();
            if marker == b'.' {
                d.classes.push(name);
            } else {
                d.id = Some(name);
            }
            rest = &after[end..];
        }
        if !rest.is_empty() && d.tag.is_empty() {
            d.tag = rest.to_string();
        }
        d
    }

    fn anc(spec: &str, prev: &[&str]) -> AncNode {
        AncNode { desc: el(spec), prev: prev.iter().map(|s| el(s)).collect() }
    }

    /// `selector` against element `target` with the given ancestor chain
    /// (root-first) and preceding siblings (document order).
    fn hits(selector: &str, target: &str, ancestors: &[AncNode], prev: &[&str]) -> bool {
        let (chain, combs, _) = parse_selector(selector).expect("selector parses");
        let prev: Vec<ElemDesc> = prev.iter().map(|s| el(s)).collect();
        matches_chain(&chain, &combs, &el(target), ancestors, &prev)
    }

    #[test]
    fn lightningcss_serialization_round_trips_to_our_combinators() {
        // Guards the seam between lightningcss's selector serialization and our
        // `parse_selector`: if that serialization ever changes shape, this catches
        // it before it silently degrades matching back to descendant-only.
        use super::{parse_rules, Combinator};
        let css = ".card > text { color: #111 } .a + .b { color: #222 } .a ~ .b { color: #333 }";
        let rules = parse_rules(css);
        let combs: Vec<&[Combinator]> = rules.iter().map(|r| r.combs.as_slice()).collect();
        assert_eq!(combs[0], &[Combinator::Child]);
        assert_eq!(combs[1], &[Combinator::NextSibling]);
        assert_eq!(combs[2], &[Combinator::SubsequentSibling]);
    }

    #[test]
    fn child_combinator_styles_the_right_element_end_to_end() {
        // `> text` must reach the direct child, not the grandchild. Before the
        // fix both were colored; now only the direct child is.
        // `#080808` is used because lightningcss minifies e.g. `#ff0000` to the
        // keyword `red`, which `parse_color` doesn't (yet) resolve; this hex has
        // no shorter form and survives serialization unchanged.
        let src = r#"
            <template>
              <screen>
                <text>direct</text>
                <view><text>nested</text></view>
              </screen>
            </template>
            <style>
              screen > text { color: #080808 }
            </style>
        "#;
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build("").unwrap();
        let root = build_styled_tree(&sfc, &HashMap::new(), &mut engine).unwrap();

        let direct = root.children[0].text.as_ref().unwrap();
        let nested = root.children[1].children[0].text.as_ref().unwrap();
        assert!(direct.color.r < 0.1, "direct child of screen got the #080808 color");
        assert!(nested.color.r > 0.5, "grandchild is NOT matched by `screen > text`");
    }

    #[test]
    fn child_combinator_only_matches_direct_children() {
        // The bug's own example: `.card > text` must select a text that is a
        // direct child of `.card`, and must NOT select one nested a level deeper.
        assert!(hits("*.card > text", "text", &[anc("view.card", &[])], &[]));
        assert!(!hits(
            "*.card > text",
            "text",
            &[anc("view.card", &[]), anc("view.inner", &[])],
            &[],
        ));
        // Descendant (`.card text`) still matches the nested one — the control.
        assert!(hits(
            "*.card text",
            "text",
            &[anc("view.card", &[]), anc("view.inner", &[])],
            &[],
        ));
    }

    #[test]
    fn next_sibling_combinator_needs_immediate_predecessor() {
        // `.a + .b`: matches only when `.a` is the element right before `.b`.
        assert!(hits("*.a + *.b", "view.b", &[], &["view.a"]));
        assert!(hits("*.a + *.b", "view.b", &[], &["view.x", "view.a"]));
        // `.a` present but not immediately before → no match (was matched by bug).
        assert!(!hits("*.a + *.b", "view.b", &[], &["view.a", "view.x"]));
        assert!(!hits("*.a + *.b", "view.b", &[], &[]));
    }

    #[test]
    fn subsequent_sibling_combinator_matches_any_earlier_sibling() {
        // `.a ~ .b`: any preceding sibling `.a`, not just the immediate one.
        assert!(hits("*.a ~ *.b", "view.b", &[], &["view.a", "view.x"]));
        assert!(hits("*.a ~ *.b", "view.b", &[], &["view.a"]));
        assert!(!hits("*.a ~ *.b", "view.b", &[], &["view.x"]));
    }

    #[test]
    fn combinators_compose() {
        // `.card > .a + .b`: `.b` is a child of `.card`, right after sibling `.a`.
        let ancestors = [anc("view.card", &[])];
        assert!(hits("*.card > *.a + *.b", "view.b", &ancestors, &["view.a"]));
        // A sibling combinator sitting above a descendant hop resolves via the
        // ancestor's own preceding siblings: `.a ~ .b .c`.
        let ancestors = [anc("view.b", &["view.a"])];
        assert!(hits("*.a ~ *.b *.c", "view.c", &ancestors, &[]));
        // …and fails when that ancestor has no preceding `.a`.
        let ancestors = [anc("view.b", &["view.x"])];
        assert!(!hits("*.a ~ *.b *.c", "view.c", &ancestors, &[]));
    }
}

fn parse_rgb(s: &str) -> Option<Rgba> {
    let inner = s.trim_start_matches("rgba").trim_start_matches("rgb");
    let inner = inner.trim().trim_start_matches('(').trim_end_matches(')');
    let parts: Vec<&str> = inner.split([',', ' ', '/']).filter(|p| !p.is_empty()).collect();
    if parts.len() < 3 {
        return None;
    }
    let r = parts[0].parse::<f32>().ok()? / 255.0;
    let g = parts[1].parse::<f32>().ok()? / 255.0;
    let b = parts[2].parse::<f32>().ok()? / 255.0;
    let a = parts.get(3).and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
    Some(Rgba::new(r, g, b, a))
}
