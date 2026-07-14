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
use rux_layout::{
    Align, Axis, Display, ImageContent, Justify, Len, Node as LayoutNode, Overflow, Rgba, Sides,
    Style, TextAlign, TextContent, TextWrap, Track,
};
use rux_parser::{Element, Node as TplNode, Sfc};
use rux_reactive::Value;
use rux_script::Engine;

/// Loop-variable bindings introduced by `r-for`, layered as a scope stack and
/// injected into the script engine for each evaluation.
type Locals = Vec<(String, Value)>;

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

/// A radius larger than any sane box; kurbo clamps it to half the shorter side,
/// which makes the box a circle/pill whatever its size.
const CIRCLE: f32 = 9999.0;

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

    let mut ancestors: Vec<ElemDesc> = Vec::new();
    let locals = Locals::new();
    Ok(build_node(
        &sfc.template,
        &rules,
        &comps,
        &mut ancestors,
        (DEFAULT_COLOR, DEFAULT_FONT_SIZE),
        engine,
        &locals,
    ))
}

/// Replace `{{ expr }}` spans in `text` with values evaluated by the engine.
fn interpolate(text: &str, engine: &mut Engine, locals: &Locals) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
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
    out.push_str(rest);
    out
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
        // Roles match case-insensitively (role="Heading" ~ [role="heading"]).
        if !el.role.as_deref().is_some_and(|er| er.eq_ignore_ascii_case(r)) {
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
/// `inherited` carries the resolved `(color, font_size)` (text properties
/// inherit); `locals` carries `r-for` loop bindings.
#[allow(clippy::too_many_arguments)]
fn build_node(
    el: &Element,
    rules: &[Rule],
    comps: &Components,
    ancestors: &mut Vec<ElemDesc>,
    inherited: (Rgba, f32),
    engine: &mut Engine,
    locals: &Locals,
) -> LayoutNode {
    // A custom-element tag expands its imported component in place.
    if let Some(component) = comps.get(&el.tag) {
        return expand_component(el, component, comps, inherited, engine, locals);
    }

    let desc = ElemDesc::of(el);
    let props = matched_props(&desc, ancestors, rules);
    let style = interpret(&props);
    let on_tap = el.attr("@tap").map(str::to_string);
    // r-show="false" keeps the layout slot but paints nothing.
    let hidden = el
        .attr("r-show")
        .is_some_and(|e| !engine.eval_bool(e, locals));

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
                caret: None,
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
    if el.tag == "input" && matches!(el.attr("type"), Some("checkbox") | Some("radio")) {
        let radio = el.attr("type") == Some("radio");
        let model = el.attr("r-model").unwrap_or_default().to_string();
        let value = el.attr("value").unwrap_or_default().to_string();

        let checked = if model.is_empty() {
            false
        } else if radio {
            engine.eval_display(&model, locals) == value
        } else {
            engine.eval_bool(&model, locals)
        };

        let mut style = style;
        // Centre the indicator inside the box unless the author says otherwise.
        if style.display == Display::Block {
            style.display = Display::Flex;
        }
        style.justify.get_or_insert(Justify::Center);
        style.align.get_or_insert(Align::Center);
        // A radio is round unless it was given its own radius.
        if radio && style.radius == 0.0 {
            style.radius = CIRCLE;
        }

        let mut node = LayoutNode::new(style);
        if checked {
            // The mark: a smaller box in the text colour (a dot for a radio).
            node.children.push(LayoutNode::new(Style {
                display: Display::Flex,
                width: Some(Len::Pct(0.6)),
                height: Some(Len::Pct(0.6)),
                background: Some(color),
                radius: if radio { CIRCLE } else { 2.0 },
                ..Default::default()
            }));
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
    if el.tag == "input" {
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
                caret: None, // the runtime marks the focused input's caret
            },
        );
        let mut node = LayoutNode::new(style);
        node.children.push(text_child);
        node.model = model;
        node.on_tap = on_tap;
        node.hidden = hidden;
        return node;
    }

    ancestors.push(desc);
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
        (color, font_size),
        engine,
        locals,
    );
    ancestors.pop();

    LayoutNode {
        style,
        text: None,
        image: None,
        children,
        on_tap,
        model: None,
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
    inherited: (Rgba, f32),
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

    let mut ancestors: Vec<ElemDesc> = Vec::new();
    build_node(
        &component.template,
        &component.rules,
        comps,
        &mut ancestors,
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
    ancestors: &mut Vec<ElemDesc>,
    inherited: (Rgba, f32),
    engine: &mut Engine,
    locals: &Locals,
) -> Vec<LayoutNode> {
    let mut out = Vec::new();
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
                        out.push(build_node(el, rules, comps, ancestors, inherited, engine, &child_locals));
                    }
                }
            }
            continue;
        }

        if let Some(cond) = el.attr("r-if") {
            in_chain = true;
            chain_satisfied = engine.eval_bool(cond, locals);
            if chain_satisfied {
                out.push(build_node(el, rules, comps, ancestors, inherited, engine, locals));
            }
            continue;
        }
        if let Some(cond) = el.attr("r-elif") {
            if in_chain && !chain_satisfied && engine.eval_bool(cond, locals) {
                chain_satisfied = true;
                out.push(build_node(el, rules, comps, ancestors, inherited, engine, locals));
            }
            continue;
        }
        if el.attr("r-else").is_some() {
            if in_chain && !chain_satisfied {
                out.push(build_node(el, rules, comps, ancestors, inherited, engine, locals));
            }
            in_chain = false;
            continue;
        }

        // A plain element ends any active chain.
        in_chain = false;
        out.push(build_node(el, rules, comps, ancestors, inherited, engine, locals));
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
        st.justify = match v.trim() {
            "center" => Some(Justify::Center),
            "flex-end" | "end" => Some(Justify::End),
            "space-between" => Some(Justify::SpaceBetween),
            "space-around" => Some(Justify::SpaceAround),
            "flex-start" | "start" => Some(Justify::Start),
            _ => None,
        };
    }
    if let Some(v) = p.get("align-items") {
        st.align = match v.trim() {
            "center" => Some(Align::Center),
            "flex-end" | "end" => Some(Align::End),
            "stretch" => Some(Align::Stretch),
            "flex-start" | "start" => Some(Align::Start),
            _ => None,
        };
    }
    if let Some(v) = p.get("background").or_else(|| p.get("background-color")) {
        st.background = parse_color(v);
    }
    if let Some(v) = p.get("border-radius") {
        if let Some(px) = parse_px(first(v)) {
            st.radius = px;
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

/// Parse a `grid-template-columns`/`-rows` value into tracks (`1fr`, `100px`, `auto`).
fn parse_tracks(value: &str) -> Vec<Track> {
    value
        .split_whitespace()
        .map(|tok| {
            if let Some(fr) = tok.strip_suffix("fr") {
                Track::Fr(fr.parse().unwrap_or(1.0))
            } else if tok == "auto" {
                Track::Auto
            } else {
                parse_px(tok).map(Track::Px).unwrap_or(Track::Auto)
            }
        })
        .collect()
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
