//! Rux styling — milestone M2.
//!
//! Parses the `<style>` CSS with `lightningcss` (literal CSS, per Law 4), matches
//! rules against the template tree with our own small selector engine, applies
//! the cascade, and produces a styled `rux_layout::Node` tree. This is Stage 2
//! of `docs/04-architecture.md`, narrowed to the honored subset.
//!
//! Selector support (M2): tag, `.class`, `#id`, `[role="…"]`, compound
//! (`view.card`), and descendant combinators (`.a .b`). Specificity and source
//! order resolve conflicts, as in CSS.

use std::collections::HashMap;

use lightningcss::rules::CssRule;
use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
use lightningcss::traits::ToCss;
use rux_layout::{Axis, Node as LayoutNode, Rgba, Style, TextContent};
use rux_parser::{Element, Node as TplNode, Sfc};
use rux_reactive::Signals;

/// Default inherited text colour (`#cdd6f4`) and font size, used at the root
/// before any `color` / `font-size` rule applies. Text properties inherit.
const DEFAULT_COLOR: Rgba = Rgba::new(0.804, 0.839, 0.957, 1.0);
const DEFAULT_FONT_SIZE: f32 = 16.0;

/// Build the styled layout tree from a parsed SFC, interpolating `{{ }}` text
/// against the current signal values.
pub fn build_styled_tree(sfc: &Sfc, signals: &Signals) -> Result<LayoutNode, String> {
    let rules = parse_rules(&sfc.style);
    let mut ancestors: Vec<ElemDesc> = Vec::new();
    Ok(build_node(
        &sfc.template,
        &rules,
        &mut ancestors,
        (DEFAULT_COLOR, DEFAULT_FONT_SIZE),
        signals,
    ))
}

/// Replace `{{ expr }}` spans in `text` with evaluated signal values.
fn interpolate(text: &str, signals: &Signals) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                out.push_str(&eval_expr(after[..end].trim(), signals));
                rest = &after[end + 2..];
            }
            None => {
                out.push_str("{{");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// M5 expression eval: a bare signal name or a numeric literal. Unknown → empty.
fn eval_expr(expr: &str, signals: &Signals) -> String {
    if let Some(v) = signals.get(expr) {
        v.to_display()
    } else if let Ok(n) = expr.parse::<f64>() {
        rux_reactive::Value::Number(n).to_display()
    } else {
        String::new()
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

/// A full selector: a descendant chain of compounds, plus its specificity.
#[derive(Debug, Clone)]
struct Rule {
    chain: Vec<Compound>,
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
                    decls.push((
                        k.trim().to_lowercase(),
                        v.trim().trim_end_matches(';').trim().to_string(),
                    ));
                }
            }
        }

        // One Rule per selector in the list (they share the declarations).
        for selector in &style.selectors.0 {
            if let Ok(text) = selector.to_css_string(PrinterOptions::default()) {
                if let Some((chain, specificity)) = parse_selector(&text) {
                    rules.push(Rule {
                        chain,
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

/// Parse a selector string into a descendant chain and compute specificity.
fn parse_selector(text: &str) -> Option<(Vec<Compound>, (u32, u32, u32))> {
    let mut chain = Vec::new();
    let mut spec = (0u32, 0u32, 0u32);

    for token in text.split_whitespace() {
        if token == ">" || token == "+" || token == "~" {
            continue; // M2 treats all combinators as descendant
        }
        let compound = parse_compound(token, &mut spec)?;
        chain.push(compound);
    }
    if chain.is_empty() {
        return None;
    }
    Some((chain, spec))
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
        if Some(r.as_str()) != el.role.as_deref() {
            return false;
        }
    }
    true
}

/// Rightmost compound must match `el`; the rest must match ancestors in order.
fn matches(chain: &[Compound], ancestors: &[ElemDesc], el: &ElemDesc) -> bool {
    let Some((last, rest)) = chain.split_last() else {
        return false;
    };
    if !matches_compound(last, el) {
        return false;
    }
    // Walk ancestors nearest→root, consuming `rest` right→left as a subsequence.
    let mut remaining = rest.len();
    let mut a = ancestors.len();
    while remaining > 0 && a > 0 {
        a -= 1;
        if matches_compound(&rest[remaining - 1], &ancestors[a]) {
            remaining -= 1;
        }
    }
    remaining == 0
}

/// Collect the matching rules' declarations for an element, in cascade order.
fn matched_props(
    desc: &ElemDesc,
    ancestors: &[ElemDesc],
    rules: &[Rule],
) -> HashMap<String, String> {
    let mut matched: Vec<&Rule> = rules
        .iter()
        .filter(|r| matches(&r.chain, ancestors, desc))
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
fn collect_text(el: &Element, signals: &Signals) -> String {
    let mut parts = Vec::new();
    for child in &el.children {
        if let TplNode::Text(t) = child {
            parts.push(interpolate(t.trim(), signals));
        }
    }
    parts.join(" ")
}

/// `inherited` carries the resolved `(color, font_size)` down the tree, since
/// text properties inherit in CSS.
fn build_node(
    el: &Element,
    rules: &[Rule],
    ancestors: &mut Vec<ElemDesc>,
    inherited: (Rgba, f32),
    signals: &Signals,
) -> LayoutNode {
    let desc = ElemDesc::of(el);
    let props = matched_props(&desc, ancestors, rules);
    let style = interpret(&props);

    // Resolve inheritable text properties (own value, else inherited).
    let color = props
        .get("color")
        .and_then(|v| parse_color(v))
        .unwrap_or(inherited.0);
    let font_size = props
        .get("font-size")
        .and_then(|v| parse_px(first(v)))
        .unwrap_or(inherited.1);

    if el.tag == "text" {
        return LayoutNode::text(
            style,
            TextContent {
                text: collect_text(el, signals),
                font_size,
                color,
            },
        );
    }

    ancestors.push(desc);
    let children = el
        .children
        .iter()
        .filter_map(|n| match n {
            TplNode::Element(child) => {
                Some(build_node(child, rules, ancestors, (color, font_size), signals))
            }
            TplNode::Text(_) => None,
        })
        .collect();
    ancestors.pop();

    LayoutNode {
        style,
        text: None,
        children,
    }
}

// ── Value interpretation (honored subset) ───────────────────────────────────

fn interpret(p: &HashMap<String, String>) -> Style {
    let mut st = Style::default();
    if let Some(v) = p.get("width") {
        st.width = parse_px(first(v));
    }
    if let Some(v) = p.get("height") {
        st.height = parse_px(first(v));
    }
    if let Some(v) = p.get("padding") {
        if let Some(px) = parse_px(first(v)) {
            st.padding = px;
        }
    }
    if let Some(v) = p.get("gap") {
        if let Some(px) = parse_px(first(v)) {
            st.gap = px;
        }
    }
    if let Some(v) = p.get("flex-grow") {
        if let Ok(g) = first(v).parse::<f32>() {
            st.grow = g;
        }
    }
    if let Some(v) = p.get("flex-direction") {
        st.axis = if v.trim() == "row" { Axis::Row } else { Axis::Column };
    }
    if let Some(v) = p.get("background").or_else(|| p.get("background-color")) {
        st.background = parse_color(v);
    }
    if let Some(v) = p.get("border-radius") {
        if let Some(px) = parse_px(first(v)) {
            st.radius = px;
        }
    }
    st
}

fn first(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or(s)
}

fn parse_px(s: &str) -> Option<f32> {
    let s = s.trim();
    let s = s.strip_suffix("px").unwrap_or(s);
    s.parse::<f32>().ok()
}

fn parse_color(s: &str) -> Option<Rgba> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    if s.starts_with("rgb") {
        return parse_rgb(s);
    }
    match s {
        "transparent" => Some(Rgba::new(0.0, 0.0, 0.0, 0.0)),
        "white" => Some(Rgba::new(1.0, 1.0, 1.0, 1.0)),
        "black" => Some(Rgba::new(0.0, 0.0, 0.0, 1.0)),
        _ => None,
    }
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
    use super::interpolate;
    use rux_reactive::Signals;

    #[test]
    fn interpolates_signal_bindings() {
        let signals = Signals::from_script(r#"let level = signal(82); let who = signal("Cam");"#);
        assert_eq!(interpolate("{{ level }}%", &signals), "82%");
        assert_eq!(interpolate("Hi {{ who }}!", &signals), "Hi Cam!");
        assert_eq!(interpolate("plain text", &signals), "plain text");
        assert_eq!(interpolate("{{ missing }}!", &signals), "!"); // unknown → empty
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
