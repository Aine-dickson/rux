//! Rux styling, milestone M2.
//!
//! Parses the `<style>` CSS with `lightningcss` (literal CSS, per Law 4), matches
//! rules against the template tree with our own small selector engine, applies
//! the cascade, and produces a styled `rux_layout::Node` tree. This is Stage 2
//! of `docs/04-architecture.md`, narrowed to the honored subset.
//!
//! Selector support: tag, `.class`, `#id`, `[role="…"]`, compound
//! (`view.card`), and all four combinators, descendant (`.a .b`), child
//! (`.a > .b`), next-sibling (`.a + .b`) and subsequent-sibling (`.a ~ .b`).
//! Specificity and source order resolve conflicts, as in CSS.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use lightningcss::rules::CssRule;
use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
use lightningcss::traits::ToCss;
use rux_layout::{
    Access, AccessRole, Align, Axis, Background, BoxShadow, Cursor, Display, Gradient, GridPlace, ImageContent, Justify,
    Len, Node as LayoutNode, Overflow, Position, Rgba, Sides, Style, TextAlign, TextContent,
    TextWrap, Track, TrackSide,
};
use rux_layout::{GradientKind, GridFlow, Transform};
use rux_parser::{Element, Node as TplNode, Sfc};
use rux_reactive::Value;
/// Re-exported so the runtime and the shell can name a warning without
/// depending on `rux-reactive` directly, the same way `Viewport` travels.
pub use rux_reactive::Warning;
use rux_script::Engine;

/// Loop-variable bindings introduced by `r-for`, layered as a scope stack and
/// injected into the script engine for each evaluation.
type Locals = Vec<(String, Value)>;

// ── Warning collection ──────────────────────────────────────────────────────

thread_local! {
    /// Warnings raised while building the current tree, unhonored properties,
    /// unknown pseudo-classes, undefined `var()`s, unsupported `@media`.
    ///
    /// Collected per build so the runtime can show them *in the window*. The
    /// stderr lines keep their own process-wide dedupe (a rebuild shouldn't spam
    /// the terminal on every keystroke), but the overlay must list everything the
    /// current document has wrong, every build, so this sink dedupes only within
    /// itself and is drained by [`take_warnings`].
    static WARNINGS: std::cell::RefCell<Vec<Warning>> = const { std::cell::RefCell::new(Vec::new()) };

    /// The file line currently being cascaded, when it is known.
    ///
    /// Set around one rule's collection, so a warning raised anywhere beneath
    /// can say where it came from without every function in between carrying a
    /// line it does not otherwise care about. The alternative was threading a
    /// parameter through selector parsing and pseudo-class parsing, neither of
    /// which has any other reason to know what a file is.
    static AT_LINE: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Run `f` with any warnings it raises attributed to `line`. Restores whatever
/// was set before, so nesting (a rule inside an `@media`) unwinds correctly.
fn located<T>(line: Option<usize>, f: impl FnOnce() -> T) -> T {
    let previous = AT_LINE.with(|l| l.replace(line));
    let out = f();
    AT_LINE.with(|l| l.set(previous));
    out
}

fn warn(message: String) {
    let warning = Warning::maybe_at(message, AT_LINE.with(|l| l.get()));
    WARNINGS.with(|w| {
        let mut w = w.borrow_mut();
        // Deduped by message *and* line: the same unhonored property on two
        // different rules is two places to go and fix, and an editor wants a
        // squiggle on each. Twice on one line is still once.
        if !w.contains(&warning) {
            w.push(warning);
        }
    });
}

/// Take the warnings raised since the last call, emptying the sink.
pub fn take_warnings() -> Vec<Warning> {
    WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()))
}

thread_local! {
    /// Whether to mirror warnings to stderr as they are raised.
    ///
    /// On for anyone running the window, where stderr was the only place a
    /// warning could go before the overlay existed. Off for a tool that drains
    /// the sink and formats it itself: printing each warning twice, once as
    /// prose and once as a diagnostic, is what makes machine-readable output
    /// unpipeable.
    static ECHO: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Stop (or resume) mirroring warnings to stderr. See [`ECHO`].
pub fn set_stderr_echo(on: bool) {
    ECHO.with(|e| e.set(on));
}

/// Mirror one already-deduped warning to stderr, unless that has been turned off.
fn echo(message: &str) {
    if ECHO.with(|e| e.get()) {
        eprintln!("rux: {message}");
    }
}

/// A text `{{ }}` binding recorded during build: where its node lives in the
/// final tree, the raw text (with `{{ }}` intact) to re-interpolate, the
/// `r-for` locals in scope when it was built, and the signals it reads. Lets the
/// runtime recompute just this node's text when one of `deps` changes, instead of
/// rebuilding the whole tree.
#[derive(Clone, Debug)]
pub struct TextBinding {
    /// Child-index path from the tree root to the text node.
    pub path: Vec<usize>,
    /// The element's raw text template, still containing `{{ expr }}` spans.
    pub template: String,
    /// `r-for` loop locals captured at build time (empty for ordinary text).
    pub locals: Vec<(String, Value)>,
    /// Signals this binding reads, its subscription set.
    pub deps: HashSet<String>,
}

/// An `<input>`'s displayed value, patchable in place: the input node's path, the
/// bound signal, and the placeholder/colors needed to render an empty vs filled
/// field. The shown text lives in the input's first child; a change to `model`
/// rewrites it without a rebuild, so keystrokes don't throw the tree away.
#[derive(Clone, Debug)]
pub struct ValueBinding {
    /// Path to the `<input>` node; its first child holds the shown text.
    pub path: Vec<usize>,
    /// The `r-model` signal expression.
    pub model: String,
    /// Shown (dim) when the value is empty.
    pub placeholder: String,
    /// Text colour when the field has a value.
    pub color: Rgba,
    /// Text colour for the placeholder.
    pub placeholder_color: Rgba,
    /// `r-for` locals captured at build (empty for ordinary inputs).
    pub locals: Vec<(String, Value)>,
    /// Signals the value reads, normally just `model`.
    pub deps: HashSet<String>,
}

/// An `r-show` condition, patchable in place: it only flips the node's `hidden`
/// flag (paint on/off), never the tree shape, so a change rewrites one bool with
/// no rebuild and no path invalidation.
#[derive(Clone, Debug)]
pub struct ShowBinding {
    /// Path to the node whose `hidden` flag this controls.
    pub path: Vec<usize>,
    /// The `r-show` condition expression; the node is hidden when it is falsy.
    pub cond: String,
    /// `r-for` locals captured at build.
    pub locals: Vec<(String, Value)>,
    /// Signals the condition reads.
    pub deps: HashSet<String>,
}

/// A parent element that holds structural directives (`r-if`/`r-elif`/`r-else`/
/// `r-for`) among its children. Recorded so a change to one of `deps` can rebuild
/// just this parent's children and splice them in, instead of rebuilding the whole
/// tree (the reconciliation engine, slice 2b). `tpl_path` re-finds the parent
/// element in the template; `tree_path` locates its node in the built tree.
#[derive(Clone, Debug)]
pub struct StructuralParent {
    /// Child-index path from the tree root to the parent node.
    pub tree_path: Vec<usize>,
    /// Element-child index path from the template root to the parent element.
    pub tpl_path: Vec<usize>,
    /// Signals read by the structural directives directly under this parent.
    pub deps: HashSet<String>,
}

/// A checkbox/radio `<input>` node, reconcilable in place: toggling its bound
/// signal changes only this node (its `checked`-class style + mark child), never
/// the tree shape, so the node is re-built and spliced at `path` on a change.
#[derive(Clone, Debug)]
pub struct ToggleBinding {
    /// Path to the toggle `<input>` node.
    pub path: Vec<usize>,
    /// Signals the checked state reads, normally just the `r-model` signal.
    pub deps: HashSet<String>,
}

/// An expanded component instance, reconcilable in place: a change to a prop's
/// signals re-expands this subtree. The node's whole subtree is re-built and
/// spliced at `path` (with focus re-applied, since a component may hold inputs).
#[derive(Clone, Debug)]
pub struct ComponentBinding {
    /// Path to the component's root node.
    pub path: Vec<usize>,
    /// Signals its props read.
    pub deps: HashSet<String>,
}

/// A node with a dynamic `:class` / `:style` that reads signals: a change
/// re-cascades/re-interprets it, so it reconciles in place (node splice) like a
/// component. (A `:style` that reads only an `r-for` local has no signal deps and
/// is handled by the loop's own reconcile, so it isn't recorded here.)
#[derive(Clone, Debug)]
pub struct StyledBinding {
    /// Path to the node.
    pub path: Vec<usize>,
    /// Signals `:class` / `:style` read.
    pub deps: HashSet<String>,
}

/// A reactive attribute whose change rewrites one field of a node in place, with
/// no shape change: `:src` on an `<image>` or `:options` on a `<select>`.
#[derive(Clone, Debug)]
pub struct AttrBinding {
    /// Path to the node the attribute is on.
    pub path: Vec<usize>,
    /// The attribute expression.
    pub expr: String,
    /// `r-for` locals captured at build.
    pub locals: Vec<(String, Value)>,
    /// Signals the expression reads.
    pub deps: HashSet<String>,
}

/// What a build discovered about reactivity: the patchable bindings (text `{{ }}`,
/// input values, `r-show` visibility, `:src`/`:options` attributes), the parents
/// that hold structural directives and the toggle nodes (for reconciliation), and
/// the signals whose change can *not* be handled in place at all (component props)
/// and so require a full rebuild.
#[derive(Clone, Debug, Default)]
pub struct BindingRegistry {
    pub text: Vec<TextBinding>,
    pub value: Vec<ValueBinding>,
    pub show: Vec<ShowBinding>,
    /// `:src` on `<image>`, rewrites the image source.
    pub src: Vec<AttrBinding>,
    /// `:options` on `<select>`, rewrites the option list.
    pub options: Vec<AttrBinding>,
    pub structural_parents: Vec<StructuralParent>,
    pub toggles: Vec<ToggleBinding>,
    pub components: Vec<ComponentBinding>,
    pub styled: Vec<StyledBinding>,
    /// Signals read by any non-patchable, non-reconcilable site. A change touching
    /// one of these means the runtime must rebuild rather than patch. (Empty now,
    /// kept as a safety net for any future non-reconcilable binding.)
    pub structural: HashSet<String>,
}


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
    /// Custom properties (`--name`) in scope. They inherit like the text
    /// properties above, see [`Vars`].
    vars: Vars,
}

/// CSS custom properties (`--name: value`) in scope for an element: its own
/// declarations layered over everything it inherited. Custom properties inherit
/// like `color` does, which is what lets a palette be declared once on the root
/// and read by `var()` anywhere below.
///
/// Shared by `Rc` because the overwhelmingly common case is a subtree that
/// declares none of its own, those nodes hand the very same map to their
/// children instead of copying it.
type Vars = Rc<HashMap<String, String>>;

/// How many `var()` hops to follow before giving up. A custom property may be
/// defined in terms of another (`--accent: var(--blue)`), so resolution
/// recurses; this is what stops `--a: var(--b); --b: var(--a)` from hanging.
const MAX_VAR_DEPTH: usize = 16;

/// Substitute every `var(--name[, fallback])` in `value`.
///
/// An undefined variable with no fallback leaves the reference in place, which
/// makes the declaration unparseable and so ignored, which is CSS's own "invalid at
/// computed-value time" behaviour. [`warn_undefined_var`] says so out loud,
/// because a silently dropped declaration is the failure mode this project keeps
/// trying to design away.
fn resolve_vars(value: &str, vars: &HashMap<String, String>, depth: usize) -> String {
    if depth >= MAX_VAR_DEPTH || !value.contains("var(") {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("var(") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 4..];
        // Find this var()'s closing paren, allowing nested parens in a fallback
        // (`var(--x, rgb(0, 0, 0))`).
        let mut depth_parens = 1i32;
        let mut end = None;
        for (i, c) in after.char_indices() {
            match c {
                '(' => depth_parens += 1,
                ')' => {
                    depth_parens -= 1;
                    if depth_parens == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            // Unclosed `var(`, emit the rest verbatim rather than looping.
            out.push_str("var(");
            out.push_str(after);
            return out;
        };
        let inner = &after[..end];
        let (name, fallback) = match inner.split_once(',') {
            Some((n, f)) => (n.trim(), Some(f.trim())),
            None => (inner.trim(), None),
        };
        match vars.get(name) {
            // The substituted value may itself contain var().
            Some(v) => out.push_str(&resolve_vars(v, vars, depth + 1)),
            None => match fallback {
                Some(f) => out.push_str(&resolve_vars(f, vars, depth + 1)),
                None => {
                    warn_undefined_var(name);
                    out.push_str("var(");
                    out.push_str(inner);
                    out.push(')');
                }
            },
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Pull this element's `--name` declarations out of `props` and layer them over
/// the inherited ones, returning what `var()` resolves against here and in the
/// subtree below.
///
/// Declaring none, the common case, returns the inherited map by `Rc` clone, so
/// only elements that actually define a variable pay for a copy. A custom
/// property's own value may reference other variables, so it is resolved as it is
/// inserted; that also means a `--x: var(--x)` self-reference resolves against
/// the *outer* scope, as in CSS.
fn take_vars(props: &mut HashMap<String, String>, inherited: &Vars) -> Vars {
    let declared: Vec<String> = props.keys().filter(|k| k.starts_with("--")).cloned().collect();
    if declared.is_empty() {
        return Rc::clone(inherited);
    }
    let mut vars = (**inherited).clone();
    for name in declared {
        // Remove it: a custom property is not a real property, and leaving it in
        // would only be fed to `interpret` (which ignores it) as noise.
        let Some(value) = props.remove(&name) else { continue };
        let value = resolve_vars(&value, &vars, 0);
        vars.insert(name, value);
    }
    Rc::new(vars)
}

/// Warn once per name that a `var()` referenced an undefined custom property.
fn warn_undefined_var(name: &str) {
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let message = format!(
        "custom property `{name}` is not defined, the declaration using var({name}) is \
         ignored (give it a fallback: `var({name}, …)`)"
    );
    warn(message.clone());
    let seen = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
    let Ok(mut seen) = seen.lock() else { return };
    if seen.insert(name.to_string()) {
        echo(&message);
    }
}

/// A radius larger than any sane box; kurbo clamps it to half the shorter side,
/// which makes the box a circle/pill whatever its size.
const CIRCLE: f32 = 9999.0;

/// An `<input type=checkbox|radio>`: whether it is currently checked, and the
/// signals its checked state reads (so a change can reconcile just this node).
#[derive(Clone)]
struct Toggle {
    radio: bool,
    checked: bool,
    deps: HashSet<String>,
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
        // The checked state reads the model signal; a change to it reconciles this
        // toggle node in place (the `checked` class flips its style + mark).
        let (checked, deps) = if model.is_empty() {
            (false, HashSet::new())
        } else if radio {
            let (v, deps) = engine.eval_display_tracked(model, locals);
            (v == el.attr("value").unwrap_or_default(), deps)
        } else {
            engine.eval_bool_tracked(model, locals)
        };
        Some(Self { radio, checked, deps })
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
    build_styled_tree_tracked(sfc, components, engine).map(|(node, _)| node)
}

/// Recompute a text binding's string against the engine's current state, what
/// the runtime writes into the node at `binding.path` when a dependency changes.
pub fn eval_text_binding(binding: &TextBinding, engine: &mut Engine) -> String {
    interpolate_tracked(&binding.template, engine, &binding.locals).0
}

/// The class names a `:class` value contributes: a string splits on whitespace; a
/// list contributes each item (also whitespace-split). Object/conditional form
/// (`#{ active: cond }`) needs a `Value::Map` and isn't handled yet.
fn class_list(value: &Value) -> Vec<String> {
    match value {
        Value::Text(s) => s.split_whitespace().map(str::to_string).collect(),
        Value::List(items) => items
            .iter()
            .flat_map(|i| i.to_display().split_whitespace().map(str::to_string).collect::<Vec<_>>())
            .collect(),
        // Object/conditional form: keys whose value is truthy.
        Value::Map(entries) => entries
            .iter()
            .filter(|(_, v)| v.is_truthy())
            .flat_map(|(k, _)| k.split_whitespace().map(str::to_string).collect::<Vec<_>>())
            .collect(),
        _ => Vec::new(),
    }
}

/// Merge an inline CSS declaration string (`"background: red; color: white"`) into
/// the resolved props at highest priority (inline wins over the cascade). A simple
/// `;`/`:` split, enough for the flat declaration lists inline styles carry.
fn merge_inline_style(props: &mut HashMap<String, String>, css: &str) {
    for decl in css.split(';') {
        if let Some((name, value)) = decl.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if !name.is_empty() && !value.is_empty() {
                props.insert(name, value.to_string());
            }
        }
    }
}

/// Resolve `for=` labels: a node with `label_for` and no `@tap` of its own inherits
/// the `@tap` of the input whose `id` it targets, so tapping the label activates
/// that input (toggles a checkbox/radio) exactly as tapping the input would. A
/// build-time link, no shell plumbing, and it survives a reconcile (which rebuilds).
/// A `for=` target's tap handler (toggles/buttons) or bound model (text inputs).
type LabelTarget = (Option<String>, Option<String>);

/// An explicit `role="…"` mapped to an accessibility role. `role=` already drives
/// selector matching (`[role="heading"]`); this makes it mean something to a
/// screen reader too, which is what it was always for.
///
/// An unrecognised role still counts as a meaningful grouping rather than being
/// dropped, the author said this element *is* something.
fn explicit_access_role(el: &Element) -> Option<AccessRole> {
    let role = el.role()?.to_ascii_lowercase();
    Some(match role.as_str() {
        "heading" => AccessRole::Heading,
        "button" => AccessRole::Button,
        "label" | "text" | "paragraph" => AccessRole::Label,
        "checkbox" => AccessRole::CheckBox,
        "radio" => AccessRole::RadioButton,
        "textbox" | "textfield" => AccessRole::TextInput,
        "combobox" | "listbox" | "select" => AccessRole::ComboBox,
        "image" | "img" => AccessRole::Image,
        _ => AccessRole::Group,
    })
}

/// The author-supplied accessible name, if any: `label="…"` (or `alt="…"` on an
/// image). When absent the name comes from the element's own text, or from a
/// `<text for="…">` label pointing at it (see [`link_labels`]).
fn authored_label(el: &Element) -> Option<String> {
    el.attr("label")
        .or_else(|| el.attr("alt"))
        .filter(|v| !v.trim().is_empty())
        .map(str::to_string)
}

/// All the text under a node, joined, the accessible name for a control whose
/// label is its own content, like a `<view @tap>` acting as a button.
fn subtree_text(node: &LayoutNode) -> String {
    let mut out = String::new();
    collect_subtree_text(node, &mut out);
    out
}

fn collect_subtree_text(node: &LayoutNode, out: &mut String) {
    if let Some(text) = &node.text {
        if !text.text.trim().is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(text.text.trim());
        }
    }
    for child in &node.children {
        collect_subtree_text(child, out);
    }
}

fn link_labels(root: &mut LayoutNode) {
    let mut targets: HashMap<String, LabelTarget> = HashMap::new();
    collect_label_targets(root, &mut targets);
    if !targets.is_empty() {
        apply_label_targets(root, &targets);
    }
    // A `<text for="email">Email</text>` names the control it points at, which is
    // the accessible name a screen reader announces for it. Collected in the same
    // pass that makes such a label tappable, so the two can't drift apart.
    let mut names: HashMap<String, String> = HashMap::new();
    collect_label_names(root, &mut names);
    if !names.is_empty() {
        apply_label_names(root, &names);
    }
}

/// Map each `for=` target id to the labelling element's text.
fn collect_label_names(node: &LayoutNode, names: &mut HashMap<String, String>) {
    if let Some(target) = &node.label_for {
        let text = subtree_text(node);
        if !text.is_empty() {
            names.entry(target.clone()).or_insert(text);
        }
    }
    for child in &node.children {
        collect_label_names(child, names);
    }
}

/// Give each labelled control its label's text as an accessible name. An
/// authored `label=` on the control itself wins, it is the more specific
/// statement of intent.
fn apply_label_names(node: &mut LayoutNode, names: &HashMap<String, String>) {
    if node.access.label.is_none() {
        if let Some(name) = node.id.as_ref().and_then(|id| names.get(id)) {
            node.access.label = Some(name.clone());
        }
    }
    for child in &mut node.children {
        apply_label_names(child, names);
    }
}

fn collect_label_targets(node: &LayoutNode, targets: &mut HashMap<String, LabelTarget>) {
    if let Some(id) = &node.id {
        targets
            .entry(id.clone())
            .or_insert_with(|| (node.on_tap.clone(), node.model.clone()));
    }
    for child in &node.children {
        collect_label_targets(child, targets);
    }
}

fn apply_label_targets(node: &mut LayoutNode, targets: &HashMap<String, LabelTarget>) {
    if node.on_tap.is_none() && node.focus_model.is_none() {
        if let Some((tap, model)) = node.label_for.as_ref().and_then(|t| targets.get(t)) {
            if let Some(tap) = tap {
                // A tappable target (checkbox/radio/button): tap the label to run it.
                node.on_tap = Some(tap.clone());
            } else if let Some(model) = model {
                // A text input: tap the label to focus it.
                node.focus_model = Some(model.clone());
            }
        }
    }
    for child in &mut node.children {
        apply_label_targets(child, targets);
    }
}

/// Recompute a `:src` attribute to the (unresolved) image source string.
pub fn eval_src_binding(binding: &AttrBinding, engine: &mut Engine) -> String {
    engine.eval_display(&binding.expr, &binding.locals)
}

/// Recompute a `:options` attribute to the option strings.
pub fn eval_options_binding(binding: &AttrBinding, engine: &mut Engine) -> Vec<String> {
    engine
        .eval_value(&binding.expr, &binding.locals)
        .and_then(|v| v.as_list().map(|items| items.iter().map(Value::to_display).collect()))
        .unwrap_or_default()
}

/// Recompute an input's shown text and colour against the engine's current state:
/// the value in the normal colour, or the placeholder in the dim colour when empty.
pub fn eval_value_binding(binding: &ValueBinding, engine: &mut Engine) -> (String, Rgba) {
    let value = engine.eval_display(&binding.model, &binding.locals);
    if value.is_empty() {
        (binding.placeholder.clone(), binding.placeholder_color)
    } else {
        (value, binding.color)
    }
}

/// Like [`build_styled_tree`], but also returns the [`BindingRegistry`], where
/// each patchable text binding lives and which signals force a rebuild. The
/// runtime uses it to update in place instead of rebuilding the whole tree.
pub fn build_styled_tree_tracked(
    sfc: &Sfc,
    components: &HashMap<String, Sfc>,
    engine: &mut Engine,
) -> Result<(LayoutNode, BindingRegistry), String> {
    build_styled_tree_stateful(
        sfc,
        components,
        engine,
        &InteractionState::default(),
        Viewport::default(),
    )
}

/// Like [`build_styled_tree_tracked`], but matches pseudo-class selectors against
/// the shell's current [`InteractionState`], what is hovered, pressed, focused,
/// and `@media` queries against the current [`Viewport`]. The runtime passes its
/// live state on every build so a reconcile reproduces the same styling.
pub fn build_styled_tree_stateful(
    sfc: &Sfc,
    components: &HashMap<String, Sfc>,
    engine: &mut Engine,
    state: &InteractionState,
    viewport: Viewport,
) -> Result<(LayoutNode, BindingRegistry), String> {
    // The document's own `<style>` knows where it is in its file, so its
    // warnings get a line. A component's does not: its rules live in a
    // *different* file, and every consumer of a warning attributes it to the
    // document being built, so a line from the component's coordinate space
    // would point confidently at the wrong place. Unplaced is the honest answer
    // until warnings carry a file as well as a line.
    let rules = parse_rules_at(&sfc.style, viewport, Some(sfc.style_line));
    let comps: Components = components
        .iter()
        .map(|(tag, c)| {
            (
                tag.clone(),
                Component {
                    template: c.template.clone(),
                    rules: parse_rules(&c.style, viewport),
                },
            )
        })
        .collect();

    let mut ancestors: Vec<AncNode> = Vec::new();
    let locals = Locals::new();
    let mut reg = BindingRegistry::default();
    let mut node = build_node(
        &sfc.template,
        &rules,
        &comps,
        &mut ancestors,
        &[],
        &Inherited {
            color: DEFAULT_COLOR,
            font_size: DEFAULT_FONT_SIZE,
            font_family: None,
            vars: Vars::default(),
        },
        engine,
        &locals,
        &[],
        &[],
        &mut reg,
        state,
    );
    link_labels(&mut node);
    Ok((node, reg))
}

/// Replace `{{ expr }}` spans in `text` with values evaluated by the engine, and
/// return the union of signals read across all spans, the text binding's
/// dependency set. Literal text has its HTML entities (`&amp;`, `&lt;`, …)
/// decoded; interpolated values are inserted verbatim (already runtime strings).
fn interpolate_tracked(
    text: &str,
    engine: &mut Engine,
    locals: &Locals,
) -> (String, HashSet<String>) {
    let mut out = String::new();
    let mut deps = HashSet::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&decode_entities(&rest[..start]));
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let (value, d) = engine.eval_display_tracked(after[..end].trim(), locals);
                out.push_str(&value);
                deps.extend(d);
                rest = &after[end + 2..];
            }
            None => {
                out.push_str("{{");
                rest = after;
            }
        }
    }
    out.push_str(&decode_entities(rest));
    (out, deps)
}

/// The raw concatenated text of an element's direct text children, `{{ }}` spans
/// left intact, the template a [`TextBinding`] re-interpolates on change.
fn text_template(el: &Element) -> String {
    el.children
        .iter()
        .filter_map(|c| match c {
            TplNode::Text(t) => Some(t.trim()),
            _ => None,
        })
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

// Entity decoding for text lives in `rux_parser::decode_entities`, the parser
// applies it to attribute values as it reads them, and text goes through the same
// function, so there is one table, not two.
use rux_parser::decode_entities;

// ── Selector model ──────────────────────────────────────────────────────────

/// One compound selector, e.g. `view.card#main[role="section"]:hover`.
#[derive(Debug, Clone, Default)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    role: Option<String>,
    pseudos: Vec<Pseudo>,
}

/// A pseudo-class in a compound selector. Each one tests a bit of interaction
/// state carried on [`ElemStates`], *not* the element's markup.
///
/// An unrecognised pseudo-class becomes [`Pseudo::Unknown`], which **never
/// matches**. That is deliberate: before pseudo-classes existed, `parse_compound`
/// stopped at the `:` and dropped it, so `.box:hover` parsed as plain `.box` and
/// the rule applied *unconditionally*. Failing closed means an unsupported
/// pseudo-class does nothing instead of styling everything.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pseudo {
    Hover,
    Focus,
    Active,
    Checked,
    Unknown(String),
}

/// Interaction state for one element, tested by the pseudo-classes above. It is
/// not part of the element's markup: `checked` is resolved at build time from the
/// toggle's `r-model`, while `hover`/`focus`/`active` are threaded in from the
/// shell (see [`InteractionState`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ElemStates {
    pub hover: bool,
    pub focus: bool,
    pub active: bool,
    pub checked: bool,
}

/// The interaction state the *shell* owns, handed to the build so pseudo-class
/// selectors can match against it. Elements are identified by their tree path,
/// the same child-index path the [`BindingRegistry`] uses, because that is what
/// survives a reconcile and what the layout's state regions report back.
///
/// `checked` is not here: it is resolved from the toggle's `r-model` during the
/// build, not tracked by the shell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InteractionState {
    /// Path of the innermost element under the pointer.
    pub hovered: Option<Vec<usize>>,
    /// Path of the element currently pressed (pointer down on it).
    pub active: Option<Vec<usize>>,
    /// `r-model` of the focused input, the shell tracks focus by model, not path.
    pub focused_model: Option<String>,
}

impl InteractionState {
    /// Does `path` name the hovered element or one of its ancestors? CSS `:hover`
    /// matches the whole chain from the root down to the pointer, not just the
    /// innermost element, a hovered button inside a hovered card leaves both
    /// hovered.
    fn hovers(&self, path: &[usize]) -> bool {
        self.hovered.as_ref().is_some_and(|h| h.starts_with(path))
    }

    /// Same containment rule as [`Self::hovers`], for `:active`.
    fn activates(&self, path: &[usize]) -> bool {
        self.active.as_ref().is_some_and(|a| a.starts_with(path))
    }
}

impl Pseudo {
    /// Is this a state the *shell* supplies (as opposed to `:checked`, resolved
    /// during the build)? Such an element needs a layout region so the shell can
    /// tell when the pointer enters or leaves it.
    fn is_pointer_state(&self) -> bool {
        matches!(self, Self::Hover | Self::Active)
    }

    fn parse(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "hover" => Self::Hover,
            "focus" => Self::Focus,
            "active" => Self::Active,
            "checked" => Self::Checked,
            other => Self::Unknown(other.to_string()),
        }
    }

    fn holds(&self, s: &ElemStates) -> bool {
        match self {
            Self::Hover => s.hover,
            Self::Focus => s.focus,
            Self::Active => s.active,
            Self::Checked => s.checked,
            // Fails closed, see the type docs.
            Self::Unknown(_) => false,
        }
    }
}

/// How one compound relates to the compound on its left in a selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    /// `a b`: b is any descendant of a.
    Descendant,
    /// `a > b`: b is a direct child of a.
    Child,
    /// `a + b`: b is the element immediately following sibling a.
    NextSibling,
    /// `a ~ b`: b is any following sibling of a.
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
    states: ElemStates,
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
            states: ElemStates::default(),
        }
    }
}

// ── Media queries ───────────────────────────────────────────────────────────

/// The viewport `@media` queries are evaluated against, the window's logical
/// size. It reaches the build the same way interaction state does, because a
/// resize can change which rules apply, not just where boxes land.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub width: f32,
    pub height: f32,
}

impl Default for Viewport {
    /// A desktop-ish window, so headless builds evaluate `@media` the way the
    /// default window would.
    fn default() -> Self {
        Self { width: 1280.0, height: 800.0 }
    }
}

/// A comparison in a media feature. `min-width: 600px` is `Ge(600)`, and the
/// Level-4 range spelling `(width < 600px)` is `Lt(600)`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Cmp {
    Le,
    Lt,
    Ge,
    Gt,
    Eq,
}

impl Cmp {
    fn holds(self, actual: f32, bound: f32) -> bool {
        match self {
            Self::Le => actual <= bound,
            Self::Lt => actual < bound,
            Self::Ge => actual >= bound,
            Self::Gt => actual > bound,
            Self::Eq => (actual - bound).abs() < f32::EPSILON,
        }
    }

    /// The same comparison read right-to-left, for `(600px >= width)`.
    fn flipped(self) -> Self {
        match self {
            Self::Le => Self::Ge,
            Self::Lt => Self::Gt,
            Self::Ge => Self::Le,
            Self::Gt => Self::Lt,
            Self::Eq => Self::Eq,
        }
    }
}

/// One media feature we understand. Anything else parses to [`Feature::Never`],
/// so an unsupported query hides its rules rather than applying them
/// unconditionally, the same fail-closed choice as an unknown pseudo-class.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Feature {
    Width(Cmp, f32),
    Height(Cmp, f32),
    Portrait,
    Landscape,
    /// A media *type* we're always in (`screen`, `all`).
    Always,
    /// Unsupported, never matches.
    Never,
}

impl Feature {
    fn holds(&self, vp: Viewport) -> bool {
        match *self {
            Self::Width(cmp, v) => cmp.holds(vp.width, v),
            Self::Height(cmp, v) => cmp.holds(vp.height, v),
            Self::Portrait => vp.height >= vp.width,
            Self::Landscape => vp.width > vp.height,
            Self::Always => true,
            Self::Never => false,
        }
    }
}

/// A parsed media condition: a comma-separated list of alternatives (OR), each a
/// chain of `and`-ed features.
#[derive(Debug, Clone, Default)]
struct MediaCond {
    any: Vec<Vec<Feature>>,
}

impl MediaCond {
    fn holds(&self, vp: Viewport) -> bool {
        self.any.iter().any(|all| all.iter().all(|f| f.holds(vp)))
    }

    /// Parse a serialized media query list, e.g.
    /// `screen and (width <= 600px), (orientation: portrait)`.
    fn parse(text: &str) -> Self {
        let any = text
            .split(',')
            .map(|alternative| {
                alternative
                    .split(" and ")
                    .flat_map(|token| parse_media_feature(token.trim()))
                    .collect()
            })
            .collect();
        Self { any }
    }
}

/// Parse one media feature. Returns several when the source is a double-ended
/// range (`(400px <= width <= 600px)` is two bounds `and`-ed).
///
/// **Both spellings have to work.** An author writes `(min-width: 600px)`, but
/// lightningcss normalizes it to the Media Queries Level 4 range form
/// `(width >= 600px)` before we ever see it, so the range form is in fact the
/// one that arrives in practice, and the `min-`/`max-` arm is the compatibility
/// path, not the other way round.
fn parse_media_feature(token: &str) -> Vec<Feature> {
    let inner = token.trim();
    // A bare media type.
    if !inner.starts_with('(') {
        return vec![match inner.to_ascii_lowercase().as_str() {
            "screen" | "all" => Feature::Always,
            // `print`/`speech` never apply to a window; `not …` and `only …` are
            // unsupported rather than wrong.
            other => {
                warn_unsupported_media(other);
                Feature::Never
            }
        }];
    }
    let body = inner.trim_start_matches('(').trim_end_matches(')').trim();

    // Range syntax: `width <= 600px`, `600px >= width`, `400px <= width <= 600px`.
    let parts = split_on_comparators(body);
    if parts.len() >= 3 {
        return parse_range(&parts);
    }

    let Some((name, value)) = body.split_once(':') else {
        // A boolean feature like `(hover)`, not something we can answer.
        warn_unsupported_media(body);
        return vec![Feature::Never];
    };
    let name = name.trim().to_ascii_lowercase();
    let value = value.trim();
    vec![match name.as_str() {
        "orientation" => match value.to_ascii_lowercase().as_str() {
            "portrait" => Feature::Portrait,
            "landscape" => Feature::Landscape,
            _ => Feature::Never,
        },
        "min-width" | "max-width" | "min-height" | "max-height" => {
            // Media lengths are absolute; the viewport-relative units a
            // stylesheet can use elsewhere would be circular here.
            let Some(px) = parse_px(value) else {
                warn_unsupported_media(&format!("{name}: {value}"));
                return vec![Feature::Never];
            };
            match name.as_str() {
                "min-width" => Feature::Width(Cmp::Ge, px),
                "max-width" => Feature::Width(Cmp::Le, px),
                "min-height" => Feature::Height(Cmp::Ge, px),
                _ => Feature::Height(Cmp::Le, px),
            }
        }
        other => {
            warn_unsupported_media(other);
            Feature::Never
        }
    }]
}

/// One token of a range-syntax feature: an operand or a comparator.
enum RangePart {
    Operand(String),
    Op(Cmp),
}

/// Split `width <= 600px` into operands and comparators. Returns an empty vec
/// when there is no comparator, so the caller falls through to `name: value`.
fn split_on_comparators(body: &str) -> Vec<RangePart> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = body.chars().peekable();
    let mut saw_op = false;
    while let Some(c) = chars.next() {
        let op = match c {
            '<' if chars.peek() == Some(&'=') => {
                chars.next();
                Some(Cmp::Le)
            }
            '>' if chars.peek() == Some(&'=') => {
                chars.next();
                Some(Cmp::Ge)
            }
            '<' => Some(Cmp::Lt),
            '>' => Some(Cmp::Gt),
            '=' => Some(Cmp::Eq),
            _ => None,
        };
        match op {
            Some(op) => {
                parts.push(RangePart::Operand(current.trim().to_string()));
                parts.push(RangePart::Op(op));
                current = String::new();
                saw_op = true;
            }
            None => current.push(c),
        }
    }
    if !saw_op {
        return Vec::new();
    }
    parts.push(RangePart::Operand(current.trim().to_string()));
    parts
}

/// Turn a split range into features: `[operand, op, operand]` or the double-ended
/// `[operand, op, operand, op, operand]`.
fn parse_range(parts: &[RangePart]) -> Vec<Feature> {
    // Which side is the axis name decides how the comparison reads.
    let feature = |axis: &str, cmp: Cmp, value: &str| -> Feature {
        let Some(px) = parse_px(value) else {
            warn_unsupported_media(value);
            return Feature::Never;
        };
        match axis {
            "width" => Feature::Width(cmp, px),
            "height" => Feature::Height(cmp, px),
            other => {
                warn_unsupported_media(other);
                Feature::Never
            }
        }
    };
    let operand = |i: usize| match &parts[i] {
        RangePart::Operand(s) => s.to_ascii_lowercase(),
        RangePart::Op(_) => String::new(),
    };
    let op = |i: usize| match &parts[i] {
        RangePart::Op(c) => *c,
        RangePart::Operand(_) => Cmp::Eq,
    };

    match parts.len() {
        3 => {
            let (left, right) = (operand(0), operand(2));
            if left == "width" || left == "height" {
                vec![feature(&left, op(1), &right)]
            } else {
                // `600px >= width`: same relation, read the other way.
                vec![feature(&right, op(1).flipped(), &left)]
            }
        }
        // `400px <= width <= 600px`: both bounds, `and`-ed.
        5 => {
            let axis = operand(2);
            vec![
                feature(&axis, op(1).flipped(), &operand(0)),
                feature(&axis, op(3), &operand(4)),
            ]
        }
        _ => vec![Feature::Never],
    }
}

/// Warn once per unsupported media feature, an `@media` block that silently
/// never applies is exactly the failure mode the unhonored-property warning
/// exists to prevent.
fn warn_unsupported_media(what: &str) {
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let message = format!(
        "`@media` condition `{what}` is not supported, its rules will never apply \
         (supported: screen/all, min-/max-width, min-/max-height, orientation)"
    );
    warn(message.clone());
    let seen = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
    let Ok(mut seen) = seen.lock() else { return };
    if seen.insert(what.to_string()) {
        echo(&message);
    }
}

/// Whether each `@media` block in `css` applies at `vp`, in source order. The
/// runtime compares this across a resize: if it is unchanged, no rule set changed
/// and the tree does not need re-cascading.
pub fn media_matches(css: &str, vp: Viewport) -> Vec<bool> {
    let Ok(sheet) = StyleSheet::parse(css, ParserOptions::default()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_media_matches(&sheet.rules.0, vp, &mut out);
    out
}

fn collect_media_matches(rules: &[CssRule], vp: Viewport, out: &mut Vec<bool>) {
    for rule in rules {
        if let CssRule::Media(media) = rule {
            let text = media
                .query
                .to_css_string(PrinterOptions::default())
                .unwrap_or_default();
            out.push(MediaCond::parse(&text).holds(vp));
            collect_media_matches(&media.rules.0, vp, out);
        }
    }
}

// ── Parsing the stylesheet ──────────────────────────────────────────────────

/// `base` is the 1-based file line the `<style>` block's first character sits
/// on, used to lift lightningcss's section-relative positions onto the file's
/// own lines. `None` means "do not claim to know": see [`parse_rules_at`].
fn parse_rules(css: &str, vp: Viewport) -> Vec<Rule> {
    parse_rules_at(css, vp, None)
}

fn parse_rules_at(css: &str, vp: Viewport, base: Option<usize>) -> Vec<Rule> {
    let sheet = match StyleSheet::parse(css, ParserOptions::default()) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut rules = Vec::new();
    let mut order = 0usize;
    collect_rules(&sheet.rules.0, vp, &mut rules, &mut order, base, css);
    rules
}

/// Walk the rule list, descending into `@media` blocks whose condition holds at
/// `vp`. A block that doesn't hold contributes nothing, so everything
/// downstream (matching, cascade, specificity) is untouched by media queries.
/// `order` keeps counting across blocks, which is what makes a later `@media`
/// rule win over an earlier plain rule of equal specificity, as in CSS.
fn collect_rules(
    rules: &[CssRule],
    vp: Viewport,
    out: &mut Vec<Rule>,
    order: &mut usize,
    base: Option<usize>,
    css: &str,
) {
    for rule in rules {
        match rule {
            CssRule::Media(media) => {
                let text = media
                    .query
                    .to_css_string(PrinterOptions::default())
                    .unwrap_or_default();
                // An unsupported condition is reported against the `@media` line.
                let holds = located(file_line(base, media.loc.line), || {
                    MediaCond::parse(&text).holds(vp)
                });
                if holds {
                    collect_rules(&media.rules.0, vp, out, order, base, css);
                }
            }
            CssRule::Style(style) => collect_style_rule(style, out, order, base, css),
            _ => {}
        }
    }
}

/// Lift a section-relative line (lightningcss counts from 0) onto the file's
/// own 1-based numbering. `None` in, `None` out: a document whose base is not
/// known reports no line rather than a wrong one.
fn file_line(base: Option<usize>, relative: u32) -> Option<usize> {
    base.map(|b| b + relative as usize)
}

/// The section-relative line `property` is declared on, scanning forward from
/// `rule_line` (the line the rule's selector sits on).
///
/// lightningcss records a location for a *rule* but none for the declarations
/// inside it, so this is the only way back to the line a reader can see. The
/// scan stops at the brace that closes the rule, so a property absent from this
/// rule reports `None` rather than borrowing a line from the next one.
///
/// Matching is deliberately loose: the property name at the start of a line,
/// then optional whitespace, then a colon. That is how a declaration is written
/// in practice, and a miss costs the rule's line, which is what the caller
/// would have used anyway.
fn decl_line(css: &str, rule_line: u32, property: &str) -> Option<u32> {
    let mut depth = 0usize;
    let mut entered = false;
    for (offset, text) in css.lines().enumerate().skip(rule_line as usize) {
        // Inside the block, a line whose first token is the property is it.
        if entered {
            let trimmed = text.trim_start();
            if let Some(rest) = trimmed.strip_prefix(property) {
                if rest.trim_start().starts_with(':') {
                    return u32::try_from(offset).ok();
                }
            }
        }
        for ch in text.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    entered = true;
                }
                '}' => {
                    depth = depth.saturating_sub(1);
                    // The rule has closed without the property turning up.
                    if entered && depth == 0 {
                        return None;
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn collect_style_rule(
    style: &lightningcss::rules::style::StyleRule,
    out: &mut Vec<Rule>,
    order: &mut usize,
    base: Option<usize>,
    css: &str,
) {
    located(file_line(base, style.loc.line), || {
        // Serialize each declaration to "prop: value" and split it.
        let mut decls = Vec::new();
        for prop in &style.declarations.declarations {
            if let Ok(text) = prop.to_css_string(false, PrinterOptions::default()) {
                if let Some((k, v)) = text.split_once(':') {
                    let key = k.trim().to_lowercase();
                    // Silent ignoring is the worst failure mode we have: valid CSS
                    // that does nothing with no explanation. Say so, once per name.
                    //
                    // Against the declaration's own line, not the rule's: a
                    // selector and the property under it can be many lines
                    // apart, and a warning that points at the selector sends
                    // the reader somewhere the named property does not appear.
                    let at = decl_line(css, style.loc.line, &key).unwrap_or(style.loc.line);
                    located(file_line(base, at), || warn_if_unhonored(&key));
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
                    out.push(Rule {
                        chain,
                        combs,
                        specificity,
                        order: *order,
                        decls: decls.clone(),
                    });
                }
            }
            *order += 1;
        }
    });
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

/// Warn, once per property name, for the life of the process, that a parsed
/// declaration is not honored. Deduped so a whole-tree rebuild (which reparses
/// every sheet) doesn't repeat the same line on every keystroke.
fn warn_if_unhonored(property: &str) {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

    // A custom property is not a property we could fail to honor, it is storage
    // for `var()`, and any name is legal.
    if property.starts_with("--") || is_honored(property) {
        return;
    }
    let message =
        format!("CSS property `{property}` is parsed but not yet honored, it will have no effect");
    warn(message.clone());
    let seen = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
    let Ok(mut seen) = seen.lock() else { return };
    if seen.insert(property.to_string()) {
        echo(&message);
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
            if d == '[' || d == '(' {
                depth += 1;
            } else if d == ']' || d == ')' {
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
            ':' => {
                // `:hover`: and `::selection`, whose second colon just falls into
                // the name and makes it an Unknown (never-matching) pseudo, which
                // is the right answer for a pseudo-*element* we don't support.
                i += 1;
                let mut name = String::new();
                // A second colon means a pseudo-*element* (`::selection`). Keep it
                // in the name so it stays Unknown rather than colliding with the
                // same-named pseudo-class.
                if i < chars.len() && chars[i] == ':' {
                    name.push(':');
                    i += 1;
                }
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '-') {
                    name.push(chars[i]);
                    i += 1;
                }
                // A functional pseudo (`:not(…)`) keeps its argument in the name so
                // it stays Unknown rather than matching as a bare `:not`.
                if i < chars.len() && chars[i] == '(' {
                    let mut depth = 0i32;
                    while i < chars.len() {
                        if chars[i] == '(' {
                            depth += 1;
                        } else if chars[i] == ')' {
                            depth -= 1;
                        }
                        name.push(chars[i]);
                        i += 1;
                        if depth == 0 {
                            break;
                        }
                    }
                }
                if !name.is_empty() {
                    let pseudo = Pseudo::parse(&name);
                    if let Pseudo::Unknown(n) = &pseudo {
                        warn_unknown_pseudo(n);
                    }
                    c.pseudos.push(pseudo);
                    // A pseudo-class has class-level specificity.
                    spec.1 += 1;
                }
            }
            _ => break,
        }
    }
    Some(c)
}

/// Warn once per unknown pseudo-class. Same reasoning as `warn_if_unhonored`:
/// valid CSS that quietly does nothing is the worst failure mode we have, and
/// since an unknown pseudo now *fails closed*, the rule disappears entirely.
fn warn_unknown_pseudo(name: &str) {
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let message = format!(
        "pseudo-class `:{name}` is not supported, rules using it will never match \
         (supported: :hover, :focus, :active, :checked)"
    );
    warn(message.clone());
    let seen = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
    let Ok(mut seen) = seen.lock() else { return };
    if seen.insert(name.to_string()) {
        echo(&message);
    }
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
    // Every pseudo-class in the compound must hold (`.btn:hover:active`).
    if !c.pseudos.iter().all(|p| p.holds(&el.states)) {
        return false;
    }
    true
}

/// Does the selector `chain` (joined by `combs`) match the element `el`, whose
/// ancestors are `ancestors` (root-first) and whose preceding rendered siblings
/// are `prev` (document order)?
///
/// Matches right-to-left with backtracking: the rightmost compound must match
/// `el`, then the combinator to its left dictates where the remaining prefix is
/// sought, up the ancestor chain (descendant/child) or across the preceding
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

/// Could any `:hover` / `:active` rule apply to this element? If so the layout
/// emits a [`rux_layout::StateRegion`] for it, which is how the shell learns the
/// pointer entered or left it.
///
/// Only the *pseudo-carrying compound* is tested, and only against this element,
/// the rest of the chain is ignored, so this over-approximates (a `.card:hover`
/// rule flags every `.card`, even one no full selector reaches). Over-flagging
/// costs one region; under-flagging would mean a `:hover` rule that silently never
/// fires, so the bias is deliberate.
///
/// Note `.card:hover .icon` flags the **card**, not the icon: the card is what the
/// pointer is over, and its subtree, icon included, is re-cascaded when its hover
/// state flips.
fn pointer_state_sensitive(desc: &ElemDesc, rules: &[Rule]) -> bool {
    // Probe with both pointer states on: we're asking "could this ever match",
    // not "does it match now".
    let probe = ElemDesc {
        states: ElemStates { hover: true, active: true, ..desc.states },
        ..desc.clone()
    };
    rules.iter().any(|rule| {
        rule.chain.iter().any(|compound| {
            compound.pseudos.iter().any(Pseudo::is_pointer_state)
                && matches_compound(compound, &probe)
        })
    })
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
    path: &[usize],
    tpl_path: &[usize],
    reg: &mut BindingRegistry,
    state: &InteractionState,
) -> LayoutNode {
    // A custom-element tag expands its imported component in place.
    if let Some(component) = comps.get(&el.tag) {
        return expand_component(
            el, component, comps, inherited, engine, locals, path, tpl_path, reg, state,
        );
    }

    let mut desc = ElemDesc::of(el);
    // A ticked checkbox / selected radio is matched by `:checked`. It *also* still
    // carries the synthetic `checked` class, the pre-pseudo-class hack, so
    // stylesheets written against `.box.checked` keep working for one release.
    // Deprecated: prefer `.box:checked`.
    let toggle = Toggle::of(el, engine, locals);
    if toggle.as_ref().is_some_and(|t| t.checked) {
        desc.states.checked = true;
        desc.classes.push("checked".to_string());
    }
    // Pointer state comes from the shell, keyed by this node's tree path. Focus is
    // keyed by `r-model` instead, that is how the shell tracks it, and it is what
    // survives a reconcile that moves nodes around.
    desc.states.hover = state.hovers(path);
    desc.states.active = state.activates(path);
    desc.states.focus = match (&state.focused_model, el.attr("r-model")) {
        (Some(focused), Some(model)) => focused == model,
        _ => false,
    };
    // `:class`: dynamic classes fed into the cascade (the `checked` pattern,
    // generalized). Signals it reads are collected for reconcile.
    let mut dyn_deps: HashSet<String> = HashSet::new();
    if let Some(expr) = el.attr(":class") {
        let (value, deps) = engine.eval_value_tracked(expr, locals);
        dyn_deps.extend(deps);
        if let Some(v) = value {
            desc.classes.extend(class_list(&v));
        }
    }

    // Set on every node this build produces, so the shell gets a region to hit-test
    // (see `pointer_state_sensitive`). Computed after `:class`, so dynamically
    // applied classes count too.
    let state_path = pointer_state_sensitive(&desc, rules).then(|| path.to_vec());

    let mut props = matched_props(&desc, ancestors, prev, rules);
    // Inline styles override the cascade: static `style=` first, then dynamic
    // `:style` (which may interpolate, rhai backtick strings evaluate here).
    if let Some(s) = el.attr("style") {
        merge_inline_style(&mut props, s);
    }
    if let Some(expr) = el.attr(":style") {
        let (value, deps) = engine.eval_value_tracked(expr, locals);
        dyn_deps.extend(deps);
        match value {
            // Object form: `#{ background: c }` → each entry a declaration.
            Some(Value::Map(entries)) => {
                for (k, v) in entries {
                    props.insert(k.to_ascii_lowercase(), v.to_display());
                }
            }
            // String form: `"background: red"` (possibly interpolated).
            Some(v) => merge_inline_style(&mut props, &v.to_display()),
            None => {}
        }
    }
    // A `:class`/`:style` that reads a signal reconciles this node on change.
    if !dyn_deps.is_empty() {
        reg.styled.push(StyledBinding { path: path.to_vec(), deps: dyn_deps });
    }

    // Custom properties: this element's own `--name` declarations (from the
    // cascade and from inline styles alike) layer over what it inherited, and the
    // result is what `var()` sees here *and* below. A node that declares none,
    // most of them, passes the inherited map straight down without copying it.
    let vars = take_vars(&mut props, &inherited.vars);
    // Substitute var() everywhere before interpreting, so every property gets it
    // for free rather than each parser learning about variables. Runs even with no
    // variables in scope: `var(--x, 12px)` is a legitimate way to write a default,
    // and skipping the pass would leave the fallback unresolved.
    for value in props.values_mut() {
        if value.contains("var(") {
            *value = resolve_vars(value, &vars, 0);
        }
    }

    let style = interpret(&props);
    // A `@tap` handler runs later, in global scope, where the `r-for` loop
    // variable no longer exists, so `@tap="picked = item"` would see `item`
    // undefined and silently do nothing. Bake the current loop bindings into the
    // handler as a `let` prelude so it reproduces them when it runs.
    let on_tap = el.attr("@tap").map(|h| bind_locals(h, locals));
    // r-show="false" keeps the layout slot but paints nothing. It only flips
    // `hidden`, never the shape, so it's patchable: record it and a change rewrites
    // the bool in place.
    let hidden = el.attr("r-show").is_some_and(|e| {
        let (v, deps) = engine.eval_bool_tracked(e, locals);
        reg.show.push(ShowBinding {
            path: path.to_vec(),
            cond: e.to_string(),
            locals: locals.clone(),
            deps,
        });
        !v
    });

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
        // Recompute this text on change instead of rebuilding: record the raw
        // template, the locals, and the signals it reads, keyed by this node's
        // path. Only text that actually interpolates is registered.
        let template = text_template(el);
        let (text, deps) = interpolate_tracked(&template, engine, locals);
        if template.contains("{{") {
            reg.text.push(TextBinding {
                path: path.to_vec(),
                template,
                locals: locals.clone(),
                deps,
            });
        }
        let mut node = LayoutNode::text(
            style,
            TextContent {
                text,
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
                preedit: None,
            },
        );
        node.on_tap = on_tap;
        node.hidden = hidden;
        node.id = el.attr("id").map(str::to_string);
        node.label_for = el.attr("for").map(str::to_string);
        node.state_path = state_path.clone();
        // Static text reads as a label; `role="heading"` promotes it. Text that is
        // itself tappable is a button whose name is its own words.
        node.access = Access {
            role: explicit_access_role(el).unwrap_or(if node.on_tap.is_some() {
                AccessRole::Button
            } else {
                AccessRole::Label
            }),
            label: authored_label(el).or_else(|| {
                node.text.as_ref().map(|t| t.text.trim().to_string()).filter(|t| !t.is_empty())
            }),
            ..Access::default()
        };
        return node;
    }

    // <image src=…>: a leaf that paints its pixels. The `src` here is still the
    // author's string; the runtime resolves it against the .rux file's directory
    // and fills in the intrinsic size.
    if el.tag == "image" {
        let src = el
            .attr(":src")
            .map(|e| {
                // `:src` rewrites the image source in place on change, no shape
                // change, so it's patchable rather than a rebuild.
                let (v, deps) = engine.eval_display_tracked(e, locals);
                reg.src.push(AttrBinding {
                    path: path.to_vec(),
                    expr: e.to_string(),
                    locals: locals.clone(),
                    deps,
                });
                v
            })
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
        node.id = el.attr("id").map(str::to_string);
        node.label_for = el.attr("for").map(str::to_string);
        node.state_path = state_path.clone();
        // An image with no `alt` has no accessible name, deliberately left None
        // rather than announcing a file path, which is noise, not information.
        node.access = Access {
            role: explicit_access_role(el).unwrap_or(AccessRole::Image),
            label: authored_label(el),
            ..Access::default()
        };
        return node;
    }

    // <input>: a box bound to a signal via r-model.
    //
    // `type=checkbox|radio` are tap-toggles, not text fields: they get no focus
    // and no keyboard, they just write the bound signal through the ordinary
    // handler path (`sig = !sig` / `sig = "value"`). An authored @tap wins.
    if let Some(Toggle { radio, checked, deps }) = toggle {
        // Recorded so a change to the bound signal reconciles just this node.
        reg.toggles.push(ToggleBinding { path: path.to_vec(), deps });
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
        node.id = el.attr("id").map(str::to_string);
        node.label_for = el.attr("for").map(str::to_string);
        node.state_path = state_path.clone();
        // The checked state is what a screen reader announces alongside the name,
        // so it has to be the resolved boolean, not the class hack.
        node.access = Access {
            role: if radio { AccessRole::RadioButton } else { AccessRole::CheckBox },
            label: authored_label(el),
            placeholder: None,
            checked: Some(checked),
            value: None,
        };
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
                    .and_then(|e| {
                        // `:options` rewrites the option list in place on change.
                        let (v, deps) = engine.eval_value_tracked(e, locals);
                        reg.options.push(AttrBinding {
                            path: path.to_vec(),
                            expr: e.to_string(),
                            locals: locals.clone(),
                            deps,
                        });
                        v
                    })
                    .and_then(|v| v.as_list().map(|items| items.iter().map(Value::to_display).collect()))
                    .unwrap_or_default()
            });
        let model = el.attr("r-model").map(str::to_string);
        let placeholder = el.attr("placeholder").unwrap_or_default().to_string();
        const PLACEHOLDER_COLOR: Rgba = Rgba::new(0.42, 0.44, 0.52, 1.0); // #6c7086
        // The value display is patchable: record where it lives and how to render
        // it, so a keystroke rewrites this input's text in place instead of
        // rebuilding. (If `model` is *also* read structurally, e.g. by an `r-if`,
        // that read marks it structural elsewhere, and the change rebuilds anyway.)
        let value = model
            .as_deref()
            .map(|m| {
                let (v, deps) = engine.eval_display_tracked(m, locals);
                reg.value.push(ValueBinding {
                    path: path.to_vec(),
                    model: m.to_string(),
                    placeholder: placeholder.clone(),
                    color,
                    placeholder_color: PLACEHOLDER_COLOR,
                    locals: locals.clone(),
                    deps,
                });
                v
            })
            .unwrap_or_default();
        let (shown, shown_color) = if value.is_empty() {
            (placeholder.clone(), PLACEHOLDER_COLOR)
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
                preedit: None,
            },
        );
        let mut node = LayoutNode::new(style);
        node.children.push(text_child);
        node.model = model;
        node.multiline = multiline;
        node.options = options;
        node.on_tap = on_tap;
        node.hidden = hidden;
        node.id = el.attr("id").map(str::to_string);
        node.label_for = el.attr("for").map(str::to_string);
        node.state_path = state_path.clone();
        // The *value* is the signal's text, never the placeholder, a placeholder
        // is a hint, and announcing it as the content would be a lie. It becomes
        // the fallback *name* instead, when nothing else labels the field.
        node.access = Access {
            role: explicit_access_role(el).unwrap_or(if node.options.is_some() {
                AccessRole::ComboBox
            } else if multiline {
                AccessRole::MultilineTextInput
            } else {
                AccessRole::TextInput
            }),
            label: authored_label(el),
            // Only a fallback name, a `<text for="…">` label linked after the
            // build must outrank it.
            placeholder: (!placeholder.is_empty()).then(|| placeholder.clone()),
            value: node
                .model
                .as_deref()
                .map(|m| engine.eval_display(m, locals))
                .filter(|v| !v.is_empty()),
            checked: None,
        };
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
    let (children, structural_deps) = build_children(
        &element_children,
        rules,
        comps,
        ancestors,
        &Inherited { color, font_size, font_family, vars: Rc::clone(&vars) },
        engine,
        locals,
        path,
        tpl_path,
        reg,
        state,
    );
    ancestors.pop();
    // If any child carried a structural directive, this parent can be reconciled
    // in place (rebuild just its children) on a change to those signals.
    if !structural_deps.is_empty() {
        reg.structural_parents.push(StructuralParent {
            tree_path: path.to_vec(),
            tpl_path: tpl_path.to_vec(),
            deps: structural_deps,
        });
    }

    let mut node = LayoutNode {
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
        id: el.attr("id").map(str::to_string),
        label_for: el.attr("for").map(str::to_string),
        focus_model: None,
        state_path,
        access: Access::default(),
    };
    // A tappable box is a button, named by the text inside it, that is how
    // `<view @tap><text>Save</text></view>` announces as "Save, button". A
    // scroller is worth exposing so its content can be reached; anything else is
    // structure, and only appears if the author gave it a `role=`.
    let role = explicit_access_role(el).unwrap_or(if node.on_tap.is_some() {
        AccessRole::Button
    } else if node.style.overflow == Overflow::Scroll {
        AccessRole::ScrollView
    } else {
        AccessRole::None
    });
    if role.is_meaningful() {
        let label = authored_label(el).or_else(|| {
            let text = subtree_text(&node);
            (!text.is_empty()).then_some(text)
        });
        node.access = Access { role, label, ..Access::default() };
    }
    node
}

/// Expand a `<custom-element :prop="expr" …>` into its component's tree. Props
/// (attributes prefixed `:`) are evaluated in the caller's scope and become the
/// only locals visible inside the component (component instances are isolated).
#[allow(clippy::too_many_arguments)]
fn expand_component(
    el: &Element,
    component: &Component,
    comps: &Components,
    inherited: &Inherited,
    engine: &mut Engine,
    parent_locals: &Locals,
    path: &[usize],
    tpl_path: &[usize],
    reg: &mut BindingRegistry,
    state: &InteractionState,
) -> LayoutNode {
    let mut props: Locals = Vec::new();
    let mut prop_deps: HashSet<String> = HashSet::new();
    for (key, expr) in &el.attrs {
        if let Some(name) = key.strip_prefix(':') {
            // Props are evaluated in the caller's scope and become the component's
            // only locals, a prop change re-expands this subtree (a reconcile).
            let (value, deps) = engine.eval_value_tracked(expr, parent_locals);
            prop_deps.extend(deps);
            if let Some(value) = value {
                props.push((name.to_string(), value));
            }
        }
    }
    // Reconcile this component instance in place when a prop's signals change.
    if !prop_deps.is_empty() {
        reg.components.push(ComponentBinding {
            path: path.to_vec(),
            deps: prop_deps,
        });
    }

    // The component expands in place at this element's path, so its root node
    // takes the same path; its bindings are recorded relative to it.
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
        path,
        tpl_path,
        reg,
        state,
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
    path: &[usize],
    tpl_path: &[usize],
    reg: &mut BindingRegistry,
    state: &InteractionState,
) -> (Vec<LayoutNode>, HashSet<String>) {
    let mut out = Vec::new();
    // Signals read by structural directives at this level, returned so the parent
    // can register itself as reconcilable.
    let mut structural_deps: HashSet<String> = HashSet::new();
    // The identities of the rendered siblings so far, so `+`/`~` combinators can
    // see the elements preceding the one being built. (The synthetic `checked`
    // class is not reflected here, sibling combinators don't see checked state.)
    let mut prev: Vec<ElemDesc> = Vec::new();
    // Tracks an active r-if/r-elif/r-else chain and whether a branch was taken.
    let mut in_chain = false;
    let mut chain_satisfied = false;

    // The tree path to the child about to be pushed, its index is its position in
    // `out`. The template path uses the element's index `ti`, shared by r-for items.
    let child_path = |out: &Vec<LayoutNode>| -> Vec<usize> {
        path.iter().copied().chain(std::iter::once(out.len())).collect()
    };
    let child_tpl = |ti: usize| -> Vec<usize> {
        tpl_path.iter().copied().chain(std::iter::once(ti)).collect()
    };

    for (ti, el) in elements.iter().enumerate() {
        let ctp = child_tpl(ti);
        // r-for expands the element once per collection item; it ends any chain.
        // The collection is a structural read, a change re-diffs the list.
        if let Some(for_expr) = el.attr("r-for") {
            in_chain = false;
            if let Some((var, coll)) = parse_for(for_expr) {
                // The collection is a reconcilable read, not a force-rebuild one:
                // it flows to the parent's structural deps (via the return), not to
                // `reg.structural`.
                let (value, deps) = engine.eval_value_tracked(coll, locals);
                structural_deps.extend(deps);
                let items = value.and_then(|v| v.as_list().map(<[Value]>::to_vec));
                if let Some(items) = items {
                    for item in items {
                        let mut child_locals = locals.clone();
                        child_locals.push((var.to_string(), item));
                        let cp = child_path(&out);
                        out.push(build_node(el, rules, comps, ancestors, &prev, inherited, engine, &child_locals, &cp, &ctp, reg, state));
                        prev.push(ElemDesc::of(el));
                    }
                }
            }
            continue;
        }

        // r-if / r-elif conditions are structural reads too.
        if let Some(cond) = el.attr("r-if") {
            in_chain = true;
            let (v, deps) = engine.eval_bool_tracked(cond, locals);
            structural_deps.extend(deps);
            chain_satisfied = v;
            if chain_satisfied {
                let cp = child_path(&out);
                out.push(build_node(el, rules, comps, ancestors, &prev, inherited, engine, locals, &cp, &ctp, reg, state));
                prev.push(ElemDesc::of(el));
            }
            continue;
        }
        if let Some(cond) = el.attr("r-elif") {
            let taken = if in_chain && !chain_satisfied {
                let (v, deps) = engine.eval_bool_tracked(cond, locals);
                structural_deps.extend(deps);
                v
            } else {
                false
            };
            if taken {
                chain_satisfied = true;
                let cp = child_path(&out);
                out.push(build_node(el, rules, comps, ancestors, &prev, inherited, engine, locals, &cp, &ctp, reg, state));
                prev.push(ElemDesc::of(el));
            }
            continue;
        }
        if el.attr("r-else").is_some() {
            if in_chain && !chain_satisfied {
                let cp = child_path(&out);
                out.push(build_node(el, rules, comps, ancestors, &prev, inherited, engine, locals, &cp, &ctp, reg, state));
                prev.push(ElemDesc::of(el));
            }
            in_chain = false;
            continue;
        }

        // A plain element ends any active chain.
        in_chain = false;
        let cp = child_path(&out);
        out.push(build_node(el, rules, comps, ancestors, &prev, inherited, engine, locals, &cp, &ctp, reg, state));
        prev.push(ElemDesc::of(el));
    }
    (out, structural_deps)
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
    // word-break: break-all is stronger, it breaks anywhere, not just to avoid
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
    // saying so is enough, we have no per-axis overflow yet.
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
/// the first stop (shape, `at …`) is accepted and ignored, we always draw a
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

/// Split a track list on whitespace, but keep a `minmax( … )` group, which
/// contains its own spaces and comma, together as one token.
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
    use super::{build_styled_tree, build_styled_tree_tracked, interpolate_tracked, interpret, Len, Locals};
    use rux_script::{Builder, Engine};
    use std::collections::HashMap;

    /// Every kind of CSS warning must land on the line the reader can see, not
    /// on a line counted from the start of the `<style>` block. Getting this
    /// wrong sends someone confidently to the wrong part of their file, which is
    /// why the offset is carried rather than assumed to be zero.
    #[test]
    fn css_warnings_carry_the_line_of_the_file() {
        let src = "<template>\n  <screen class=\"a\"></screen>\n</template>\n\n<style>\n  .a { display: flex; }\n  .b { float: left; }\n\n  .c:nope { color: red; }\n\n  @media (hover: hover) { .a { gap: 4px; } }\n</style>\n";
        let sfc = rux_parser::parse_sfc(src).expect("parses");
        let mut engine = Builder::new().build("").expect("engine");

        let _ = super::take_warnings(); // start from a clean sink
        let _ = build_styled_tree(&sfc, &HashMap::new(), &mut engine).expect("builds");
        let warnings = super::take_warnings();

        let line_for = |needle: &str| {
            warnings
                .iter()
                .find(|w| w.message.contains(needle))
                .unwrap_or_else(|| panic!("no warning mentioning {needle}: {warnings:?}"))
                .line
        };
        assert_eq!(line_for("float"), Some(7));
        assert_eq!(line_for(":nope"), Some(9));
        assert_eq!(line_for("@media"), Some(11));

        // And each reported line really does contain what was complained about.
        let line_of = |n: usize| src.lines().nth(n - 1).unwrap();
        assert!(line_of(7).contains("float"));
        assert!(line_of(9).contains(":nope"));
        assert!(line_of(11).contains("@media"));
    }

    /// The fixture above writes every rule on one line, which makes the rule's
    /// line and its declarations' lines the same and hides the difference. A
    /// real stylesheet is written expanded, and a warning must name the line the
    /// property is actually on, not the line the selector is on.
    #[test]
    fn a_warning_in_an_expanded_rule_names_the_declaration_not_the_selector() {
        let src = concat!(
            "<template>\n",
            "  <screen class=\"a\"></screen>\n",
            "</template>\n",
            "\n",
            "<style>\n",
            "  .a {\n",
            "    display: flex;\n",
            "    padding: 8px;\n",
            "    float: left;\n",
            "  }\n",
            "\n",
            "  .b {\n",
            "    color: red;\n",
            "    zoom: 2;\n",
            "  }\n",
            "</style>\n",
        );
        let sfc = rux_parser::parse_sfc(src).expect("parses");
        let mut engine = Builder::new().build("").expect("engine");

        let _ = super::take_warnings();
        let _ = build_styled_tree(&sfc, &HashMap::new(), &mut engine).expect("builds");
        let warnings = super::take_warnings();

        let line_for = |needle: &str| {
            warnings
                .iter()
                .find(|w| w.message.contains(needle))
                .unwrap_or_else(|| panic!("no warning mentioning {needle}: {warnings:?}"))
                .line
        };
        // `float` is on line 9; `.a` opens on line 6, which is what the rule's
        // own location would have reported.
        assert_eq!(line_for("float"), Some(9));
        // And the scan must not run past the closing brace into the next rule.
        assert_eq!(line_for("zoom"), Some(14));

        let line_of = |n: usize| src.lines().nth(n - 1).unwrap();
        assert!(line_of(9).contains("float"));
        assert!(line_of(14).contains("zoom"));
    }

    /// A component's CSS lives in a different file, and a warning carries no
    /// file, so claiming a line would point into whichever document happened to
    /// import it. Unplaced is the honest answer until warnings carry a file too.
    #[test]
    fn a_components_css_warning_is_left_unplaced() {
        let main = rux_parser::parse_sfc(
            "<template>\n  <screen><my-row /></screen>\n</template>\n<script>\nuse components::row;\n</script>\n",
        )
        .expect("parses");
        let component = rux_parser::parse_sfc(
            "<template>\n  <view class=\"r\"></view>\n</template>\n<style>\n  .r { float: left; }\n</style>\n",
        )
        .expect("parses");
        let mut components = HashMap::new();
        components.insert("my-row".to_string(), component);
        let mut engine = Builder::new().build("").expect("engine");

        let _ = super::take_warnings();
        let _ = build_styled_tree(&main, &components, &mut engine).expect("builds");
        let warnings = super::take_warnings();

        let float = warnings
            .iter()
            .find(|w| w.message.contains("float"))
            .expect("the component's unhonored property is still reported");
        assert_eq!(float.line, None, "but without a line from another file");
    }

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
        // The shorthand's omitted basis is 0, not auto, a bare `flex: 1` sizes
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

    /// A parent holding structural directives is recorded (with its tree path,
    /// template path, and the signals its directives read) so the runtime can
    /// reconcile just that parent instead of rebuilding the whole tree.
    #[test]
    fn records_structural_parent_for_reconcile() {
        let src = r#"
            <template>
              <screen>
                <text>title</text>
                <view r-for="n in nums"><text>{{ n }}</text></view>
                <text r-if="level < 5">low</text>
              </screen>
            </template>
            <script> let nums = signal([1, 2, 3]); let level = signal(10); </script>
        "#;
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let (_root, reg) = build_styled_tree_tracked(&sfc, &HashMap::new(), &mut engine).unwrap();

        assert_eq!(reg.structural_parents.len(), 1, "the screen is the one structural parent");
        let sp = &reg.structural_parents[0];
        assert_eq!(sp.tree_path, Vec::<usize>::new(), "screen is the root");
        assert_eq!(sp.tpl_path, Vec::<usize>::new());
        let mut deps: Vec<&str> = sp.deps.iter().map(String::as_str).collect();
        deps.sort_unstable();
        assert_eq!(deps, ["level", "nums"], "both directive signals are captured");
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
        let interp = |e: &mut Engine, s: &str| interpolate_tracked(s, e, &locals).0;
        assert_eq!(interp(&mut e, "{{ level }}%"), "82%");
        assert_eq!(interp(&mut e, "Hi {{ who }}!"), "Hi Cam!");
        assert_eq!(interp(&mut e, "plain text"), "plain text");
        assert_eq!(interp(&mut e, "{{ missing }}!"), "!"); // unknown → empty
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
    use super::{matches_chain, parse_selector, AncNode, ElemDesc, ElemStates};

    fn el(spec: &str) -> ElemDesc {
        // "tag.class.class#id", tag optional, order flexible enough for tests.
        let mut d = ElemDesc {
            tag: String::new(),
            id: None,
            classes: Vec::new(),
            role: None,
            states: ElemStates::default(),
        };
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

    // ── Accessibility ───────────────────────────────────────────────────────

    use super::AccessRole;

    fn built(src: &str) -> rux_layout::Node {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        build_styled_tree(&sfc, &HashMap::new(), &mut engine).unwrap()
    }

    /// Every control gets a role a screen reader can announce, derived from what
    /// it is rather than guessed from how it paints.
    #[test]
    fn controls_get_their_implicit_roles() {
        let root = built(
            r#"<template><screen>
                 <text>a heading</text>
                 <input r-model="name" />
                 <input type="textarea" r-model="notes" />
                 <input type="checkbox" r-model="agree" />
                 <input type="radio" r-model="plan" value="pro" />
                 <view @tap="n = n + 1"><text>Save</text></view>
                 <image src="logo.png" alt="the logo" />
               </screen></template>
               <script>let name = signal(""); let notes = signal(""); let agree = signal(false);
                       let plan = signal("free"); let n = signal(0);</script>"#,
        );
        let roles: Vec<AccessRole> = root.children.iter().map(|c| c.access.role).collect();
        assert_eq!(
            roles,
            vec![
                AccessRole::Label,
                AccessRole::TextInput,
                AccessRole::MultilineTextInput,
                AccessRole::CheckBox,
                AccessRole::RadioButton,
                AccessRole::Button,
                AccessRole::Image,
            ]
        );
    }

    /// A tappable box is named by the text inside it, so it announces as
    /// "Save, button" rather than as an anonymous control.
    #[test]
    fn a_tappable_box_is_named_by_its_content() {
        let root = built(
            r#"<template><screen><view @tap="n = n + 1"><text>Save</text></view></screen></template>
               <script>let n = signal(0);</script>"#,
        );
        let button = &root.children[0];
        assert_eq!(button.access.role, AccessRole::Button);
        assert_eq!(button.access.label.as_deref(), Some("Save"));
    }

    /// A `for=` label names the control it points at, the same link that already
    /// makes the label tappable.
    #[test]
    fn a_for_label_names_its_control() {
        let root = built(
            r#"<template><screen>
                 <text for="email">Email address</text>
                 <input id="email" r-model="email" />
               </screen></template>
               <script>let email = signal("");</script>"#,
        );
        let input = &root.children[1];
        assert_eq!(input.access.role, AccessRole::TextInput);
        assert_eq!(input.access.label.as_deref(), Some("Email address"));
    }

    /// An explicit `role=` wins over the implicit one, and an authored `label=`
    /// wins over content, both are the author being specific.
    #[test]
    fn explicit_role_and_label_win() {
        let root = built(
            r#"<template><screen>
                 <text role="heading">Dashboard</text>
                 <view @tap="n = n + 1" label="Save changes"><text>OK</text></view>
               </screen></template>
               <script>let n = signal(0);</script>"#,
        );
        assert_eq!(root.children[0].access.role, AccessRole::Heading);
        assert_eq!(root.children[1].access.label.as_deref(), Some("Save changes"));
    }

    /// A toggle reports its checked state, and reports it *live*, that is what a
    /// screen reader announces alongside the name.
    #[test]
    fn a_toggle_reports_its_checked_state() {
        let root = built(
            r#"<template><screen>
                 <input type="checkbox" r-model="on" />
                 <input type="checkbox" r-model="off" />
               </screen></template>
               <script>let on = signal(true); let off = signal(false);</script>"#,
        );
        assert_eq!(root.children[0].access.checked, Some(true));
        assert_eq!(root.children[1].access.checked, Some(false));
    }

    /// An input exposes its value, but a placeholder is a hint, not content, so
    /// it names the field instead of pretending to be what's typed in it.
    #[test]
    fn an_input_exposes_value_but_not_its_placeholder_as_value() {
        let root = built(
            r#"<template><screen>
                 <input r-model="name" placeholder="Your name" />
                 <input r-model="city" placeholder="Your city" />
               </screen></template>
               <script>let name = signal("Ada"); let city = signal("");</script>"#,
        );
        let filled = &root.children[0];
        assert_eq!(filled.access.value.as_deref(), Some("Ada"));
        assert_eq!(
            filled.access.name(),
            Some("Your name"),
            "an unlabelled field falls back to its placeholder for a name"
        );

        let empty = &root.children[1];
        assert_eq!(empty.access.value, None, "an empty field has no value");
        assert_eq!(empty.access.name(), Some("Your city"));
    }

    /// A real label outranks a placeholder: the placeholder is only the fallback
    /// name for a field nobody labelled.
    #[test]
    fn a_for_label_outranks_a_placeholder() {
        let root = built(
            r#"<template><screen>
                 <text for="notes">Notes</text>
                 <input id="notes" r-model="notes" placeholder="Type a few lines…" />
               </screen></template>
               <script>let notes = signal("");</script>"#,
        );
        let input = &root.children[1];
        assert_eq!(input.access.name(), Some("Notes"), "the label wins");
        assert_eq!(
            input.access.placeholder.as_deref(),
            Some("Type a few lines…"),
            "the placeholder is still available as a hint"
        );
    }

    /// Plain layout boxes stay out of the tree, an assistive tree full of
    /// anonymous groups is worse than a short one.
    #[test]
    fn plain_boxes_are_not_exposed() {
        let root = built(
            r#"<template><screen><view class="row"><view class="col" /></view></screen></template>"#,
        );
        assert_eq!(root.children[0].access.role, AccessRole::None);
        assert_eq!(root.children[0].children[0].access.role, AccessRole::None);
        assert!(!AccessRole::None.is_meaningful());
    }

    // ── @media ──────────────────────────────────────────────────────────────

    use super::{media_matches, parse_rules, InteractionState, Viewport};

    fn vp(width: f32, height: f32) -> Viewport {
        Viewport { width, height }
    }

    /// Build at a given viewport and report the target's background.
    fn bg_at_vp(src: &str, viewport: Viewport) -> Option<Background> {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let root = super::build_styled_tree_stateful(
            &sfc,
            &HashMap::new(),
            &mut engine,
            &InteractionState::default(),
            viewport,
        )
        .unwrap();
        root.0.children[0].style.background.clone()
    }

    const MEDIA_DOC: &str = r#"<template><screen><view class="target" /></screen></template>
        <style>
          .target { background: #00ff00; }
          @media (max-width: 600px) { .target { background: #ff0000; } }
        </style>"#;

    /// The rules inside a matching `@media` apply; outside it they don't exist.
    #[test]
    fn media_query_gates_its_rules_on_the_viewport() {
        assert!(is_red(&bg_at_vp(MEDIA_DOC, vp(480.0, 800.0))), "narrow → the @media rule");
        let wide = bg_at_vp(MEDIA_DOC, vp(1200.0, 800.0));
        assert!(
            matches!(&wide, Some(Background::Color(c)) if c.g == 1.0),
            "wide → the base rule, as if the block weren't there"
        );
    }

    /// A media rule beats an equally-specific earlier rule by source order, and
    /// loses to a more specific one, media adds no specificity, as in CSS.
    #[test]
    fn media_rules_cascade_by_order_not_by_being_in_a_block() {
        let src = r#"<template><screen><view class="target" id="t" /></screen></template>
            <style>
              #t { background: #00ff00; }
              @media (max-width: 600px) { .target { background: #ff0000; } }
            </style>"#;
        let narrow = bg_at_vp(src, vp(480.0, 800.0));
        assert!(
            matches!(&narrow, Some(Background::Color(c)) if c.g == 1.0),
            "#id still beats a .class inside @media"
        );
    }

    /// `and`, comma alternatives, orientation, and a media type all evaluate.
    #[test]
    fn media_conditions_evaluate() {
        let and = r#"<template><screen><view class="target" /></screen></template>
            <style>@media screen and (min-width: 400px) and (max-width: 600px) {
              .target { background: #ff0000; } }</style>"#;
        assert!(is_red(&bg_at_vp(and, vp(500.0, 800.0))), "inside the band");
        assert!(bg_at_vp(and, vp(700.0, 800.0)).is_none(), "outside the band");

        let either = r#"<template><screen><view class="target" /></screen></template>
            <style>@media (max-width: 400px), (min-width: 1000px) {
              .target { background: #ff0000; } }</style>"#;
        assert!(is_red(&bg_at_vp(either, vp(300.0, 800.0))), "first alternative");
        assert!(is_red(&bg_at_vp(either, vp(1200.0, 800.0))), "second alternative");
        assert!(bg_at_vp(either, vp(600.0, 800.0)).is_none(), "neither");

        let portrait = r#"<template><screen><view class="target" /></screen></template>
            <style>@media (orientation: portrait) { .target { background: #ff0000; } }</style>"#;
        assert!(is_red(&bg_at_vp(portrait, vp(400.0, 800.0))), "taller than wide");
        assert!(bg_at_vp(portrait, vp(800.0, 400.0)).is_none(), "wider than tall");
    }

    /// An unsupported condition hides its rules rather than applying them, the
    /// same fail-closed rule as an unknown pseudo-class.
    #[test]
    fn unsupported_media_condition_never_applies() {
        let src = r#"<template><screen><view class="target" /></screen></template>
            <style>@media (min-resolution: 2dppx) { .target { background: #ff0000; } }</style>"#;
        assert!(bg_at_vp(src, vp(800.0, 600.0)).is_none());
    }

    /// `media_matches` is what lets the runtime skip work on a resize that crosses
    /// no breakpoint: same answers, no re-cascade.
    #[test]
    fn media_matches_reports_each_block() {
        let css = "@media (max-width: 600px) { .a { color: red } } \
                   @media (min-width: 1000px) { .b { color: red } }";
        assert_eq!(media_matches(css, vp(500.0, 800.0)), vec![true, false]);
        assert_eq!(media_matches(css, vp(800.0, 800.0)), vec![false, false]);
        assert_eq!(media_matches(css, vp(1200.0, 800.0)), vec![false, true]);
        // Two sizes on the same side of every breakpoint look identical, which is
        // exactly the "don't rebuild" signal.
        assert_eq!(media_matches(css, vp(700.0, 800.0)), media_matches(css, vp(900.0, 800.0)));
        assert!(media_matches(".a { color: red }", vp(800.0, 600.0)).is_empty());
    }

    /// A rule outside any block is unaffected by the viewport.
    #[test]
    fn plain_rules_are_viewport_independent() {
        let css = ".a { color: red }";
        assert_eq!(parse_rules(css, vp(320.0, 480.0)).len(), parse_rules(css, vp(1600.0, 900.0)).len());
    }

    // ── Custom properties + var() ───────────────────────────────────────────

    use super::{Background, Vars};

    /// Build a document and return the background of the node at `path`.
    fn bg_at(src: &str, path: &[usize]) -> Option<Background> {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let root = build_styled_tree(&sfc, &HashMap::new(), &mut engine).unwrap();
        let mut node = &root;
        for i in path {
            node = &node.children[*i];
        }
        node.style.background.clone()
    }

    fn is_red(bg: &Option<Background>) -> bool {
        matches!(bg, Some(Background::Color(c)) if c.r == 1.0 && c.g == 0.0 && c.b == 0.0)
    }

    /// A variable declared on an ancestor is visible to `var()` far below it,
    /// the whole point of a palette declared once at the root.
    #[test]
    fn custom_property_inherits_down_the_tree() {
        let bg = bg_at(
            r#"<template><screen class="app"><view><view class="target" /></view></screen></template>
               <style>
                 .app { --brand: #ff0000; }
                 .target { background: var(--brand); }
               </style>"#,
            &[0, 0],
        );
        assert!(is_red(&bg), "var() resolved from an ancestor's declaration");
    }

    /// A nearer declaration wins over a farther one, and only within its subtree.
    #[test]
    fn nearer_declaration_shadows_the_inherited_one() {
        let src = r#"<template><screen class="app">
                       <view class="panel"><view class="target" /></view>
                       <view><view class="target" /></view>
                     </screen></template>
                     <style>
                       .app { --brand: #00ff00; }
                       .panel { --brand: #ff0000; }
                       .target { background: var(--brand); }
                     </style>"#;
        assert!(is_red(&bg_at(src, &[0, 0])), "inside .panel the nearer value wins");
        let outside = bg_at(src, &[1, 0]);
        assert!(
            matches!(&outside, Some(Background::Color(c)) if c.g == 1.0),
            "outside .panel the root value still applies, the override didn't leak"
        );
    }

    /// A variable may be defined in terms of another.
    #[test]
    fn custom_property_can_reference_another() {
        let bg = bg_at(
            r#"<template><screen class="app"><view class="target" /></screen></template>
               <style>
                 .app { --red: #ff0000; --brand: var(--red); }
                 .target { background: var(--brand); }
               </style>"#,
            &[0],
        );
        assert!(is_red(&bg));
    }

    /// An undefined variable falls back when given one, including a fallback that
    /// itself contains parentheses.
    #[test]
    fn var_falls_back_when_undefined() {
        let bg = bg_at(
            r#"<template><screen><view class="target" /></screen></template>
               <style>.target { background: var(--nope, #ff0000); }</style>"#,
            &[0],
        );
        assert!(is_red(&bg), "the fallback is used");

        let bg = bg_at(
            r#"<template><screen><view class="target" /></screen></template>
               <style>.target { background: var(--nope, rgb(255, 0, 0)); }</style>"#,
            &[0],
        );
        assert!(is_red(&bg), "a fallback with its own parens survives");
    }

    /// An undefined variable with no fallback leaves the declaration invalid, so
    /// it is dropped, it must not paint something arbitrary.
    #[test]
    fn undefined_var_without_fallback_drops_the_declaration() {
        let bg = bg_at(
            r#"<template><screen><view class="target" /></screen></template>
               <style>.target { background: var(--nope); }</style>"#,
            &[0],
        );
        assert!(bg.is_none(), "no background, rather than a wrong one");
    }

    /// A cycle must terminate rather than hang.
    #[test]
    fn circular_variables_terminate() {
        let bg = bg_at(
            r#"<template><screen class="app"><view class="target" /></screen></template>
               <style>
                 .app { --a: var(--b); --b: var(--a); }
                 .target { background: var(--a); }
               </style>"#,
            &[0],
        );
        assert!(bg.is_none(), "a cycle resolves to nothing, and returns");
    }

    /// `var()` works in inline `style=` too, since substitution happens after the
    /// cascade and inline styles are merged.
    #[test]
    fn var_resolves_in_inline_style() {
        let bg = bg_at(
            r#"<template><screen class="app"><view style="background: var(--brand)" /></screen></template>
               <style>.app { --brand: #ff0000; }</style>"#,
            &[0],
        );
        assert!(is_red(&bg));
    }

    /// A custom property is not a real property: it must not reach `interpret`,
    /// and must not be reported as an unhonored one.
    #[test]
    fn custom_property_is_not_treated_as_a_property() {
        assert!(!super::is_honored("--brand"));
        let mut props: HashMap<String, String> = HashMap::new();
        props.insert("--brand".into(), "#ff0000".into());
        props.insert("background".into(), "var(--brand)".into());
        let vars = super::take_vars(&mut props, &Vars::default());
        assert!(!props.contains_key("--brand"), "stripped out of the property map");
        assert_eq!(vars.get("--brand").map(String::as_str), Some("#ff0000"));
    }

    // ── Pseudo-classes ──────────────────────────────────────────────────────
    //
    // The negative case matters most here. Before pseudo-classes existed,
    // `parse_compound` stopped at the `:` and threw it away, so `.box:hover`
    // parsed as `.box` and matched *always*. Every test below that asserts a
    // rule does NOT match is guarding that regression.

    /// `selector` against an element with the given states and no ancestors.
    fn hits_state(selector: &str, target: &str, states: ElemStates) -> bool {
        let (chain, combs, _) = parse_selector(selector).expect("selector parses");
        let mut d = el(target);
        d.states = states;
        matches_chain(&chain, &combs, &d, &[], &[])
    }

    fn hovered() -> ElemStates {
        ElemStates { hover: true, ..ElemStates::default() }
    }

    #[test]
    fn pseudo_class_matches_only_in_that_state() {
        assert!(hits_state(".box:hover", ".box", hovered()));
        assert!(
            !hits_state(".box:hover", ".box", ElemStates::default()),
            "an unhovered element must NOT match :hover (it used to match always)"
        );
        // The un-suffixed rule still matches in either state.
        assert!(hits_state(".box", ".box", hovered()));
    }

    #[test]
    fn each_pseudo_reads_its_own_state() {
        let s = ElemStates { hover: false, focus: true, active: false, checked: true };
        assert!(hits_state("input:focus", "input", s));
        assert!(hits_state("input:checked", "input", s));
        assert!(!hits_state("input:hover", "input", s));
        assert!(!hits_state("input:active", "input", s));
    }

    #[test]
    fn stacked_pseudos_all_have_to_hold() {
        let hover_only = hovered();
        let both = ElemStates { hover: true, active: true, ..ElemStates::default() };
        assert!(!hits_state(".btn:hover:active", ".btn", hover_only));
        assert!(hits_state(".btn:hover:active", ".btn", both));
    }

    /// An unsupported pseudo-class fails *closed*, the rule never matches,
    /// rather than being dropped and matching everything.
    #[test]
    fn unknown_pseudo_never_matches() {
        let all_on = ElemStates { hover: true, focus: true, active: true, checked: true };
        assert!(!hits_state(".box:disabled", ".box", all_on));
        assert!(!hits_state(".box:nth-child(2)", ".box", all_on));
        assert!(!hits_state(".box::selection", ".box", all_on));
    }

    /// A pseudo-class carries class-level specificity, so `.box:hover` beats
    /// `.box` regardless of source order.
    #[test]
    fn pseudo_class_adds_class_specificity() {
        let (_, _, plain) = parse_selector(".box").unwrap();
        let (_, _, with_pseudo) = parse_selector(".box:hover").unwrap();
        assert_eq!(plain, (0, 1, 0));
        assert_eq!(with_pseudo, (0, 2, 0));
        assert!(with_pseudo > plain);
    }

    /// A pseudo-class sits inside one compound, it must not be mistaken for a
    /// new compound or split the selector.
    #[test]
    fn pseudo_class_stays_within_its_compound() {
        let (chain, combs, _) = parse_selector(".card > .btn:hover").unwrap();
        assert_eq!(chain.len(), 2, "two compounds, not three");
        assert_eq!(combs.len(), 1);
        // The state is on the *right* compound: a hovered .btn inside .card.
        let hover = hovered();
        let mut btn = el(".btn");
        btn.states = hover;
        let card = anc(".card", &[]);
        assert!(matches_chain(&chain, &combs, &btn, &[card.clone()], &[]));
        let plain_btn = el(".btn");
        assert!(!matches_chain(&chain, &combs, &plain_btn, &[card], &[]));
    }

    /// End-to-end: a ticked checkbox is styled by `:checked` through the real
    /// cascade, and an unticked one is not.
    #[test]
    fn checked_pseudo_styles_a_ticked_toggle() {
        let src = r#"
            <template>
              <screen>
                <input type="checkbox" class="box" r-model="on" />
                <input type="checkbox" class="box" r-model="off" />
              </screen>
            </template>
            <style>
              .box { background: #000000; }
              .box:checked { background: #00ff00; }
            </style>
            <script> let on = signal(true); let off = signal(false); </script>
        "#;
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let root = build_styled_tree(&sfc, &HashMap::new(), &mut engine).unwrap();

        let green = |n: &rux_layout::Node| {
            matches!(&n.style.background, Some(rux_layout::Background::Color(c)) if c.g == 1.0)
        };
        assert!(green(&root.children[0]), "ticked box matches .box:checked");
        assert!(!green(&root.children[1]), "unticked box does not");
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
        let rules = parse_rules(css, Viewport::default());
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
        // Descendant (`.card text`) still matches the nested one, the control.
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
