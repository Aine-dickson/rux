//! Rux styling, milestone M2.
//!
//! Parses the `<style>` CSS with `lightningcss` (literal CSS, per Law 4), matches
//! rules against the template tree with our own small selector engine, applies
//! the cascade, and produces a styled `rux_layout::Node` tree. This is Stage 2
//! of `docs/04-architecture.md`, narrowed to the honored subset.
//!
//! Selector support: tag, `.class`, `#id`, attribute selectors, compound
//! (`view.card`), and all four combinators, descendant (`.a .b`), child
//! (`.a > .b`), next-sibling (`.a + .b`) and subsequent-sibling (`.a ~ .b`).
//! Specificity and source order resolve conflicts, as in CSS.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use lightningcss::rules::CssRule;
use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
use lightningcss::traits::ToCss;
use rux_layout::{
    Access, AccessRole, Align, Axis, Background, BoxShadow, Cursor, Display, Gradient, GridPlace, ImageContent, Justify,
    Len, Node as LayoutNode, Overflow, Position, Rgba, SelectionStyle, Sides, Style, TextAlign, TextContent,
    TextWrap, TouchAction, Track, TrackSide,
};
use rux_layout::{AnimProp, BlendMode, BorderStyle, Easing, GradientKind, GridFlow, StepPosition, Transform, Transition};
use rux_layout::{FillRule, InputKind, LineCap, LineJoin, Outline, PathContent};
use rux_layout::{AnchorRef, AnchorSide, AnchorSize, PositionTry, ScrollbarWidth, TryInset, TryOrder};
use rux_layout::{ContentVisibility, IntrinsicSize};
use rux_layout::{AnimationDirection, AnimationFill, KeyframeAnimation, KeyframeStep, TransformFn};
use rux_layout::{DecorationStyle, ObjectFit, TextLook, TextTransform};
use rux_layout::{BgAttachment, BgBox, BgLayer, BgLayers, BgLen, BgPos, BgRepeat, BgSize};
use rux_layout::{GridAreas, GridName, GridNames, GridTrack, Repeat};
use rux_parser::{Element, Node as TplNode, Sfc};
use rux_reactive::Value;
/// Re-exported so the runtime and the shell can name a warning without
/// depending on `rux-reactive` directly, the same way `Viewport` travels.
pub use rux_reactive::Warning;
use rux_script::Engine;

/// Transitions, which live between a build and the layout rather than inside
/// either. See [`anim::Animator`].
mod anim;
pub use anim::{Animator, FRAME_MS};

/// A form field's built-in checks, shared by `:invalid` and submission.
pub mod checks;

/// The colour functions, `hsl()` to `oklch()` and `color-mix()`.
mod color;

/// Logical properties (`margin-inline-start` and the rest), put on a side.
mod layer;
mod logical;

/// `white-space`: which spaces in a `<text>` are drawn. It lives in the layout,
/// which joins the runs of a sentence and collapses the spaces between them.
pub use rux_layout::white_space;
pub use rux_layout::WhiteSpace;

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

thread_local! {
    /// The file the thing being built came from, when it is not the document.
    ///
    /// Set while an imported component's subtree is built, so a warning raised
    /// in there names the component's file. Coarser than [`AT_LINE`] on purpose:
    /// a line changes per attribute, a file changes only at a component
    /// boundary, so they are two scopes rather than one pair.
    static IN_FILE: std::cell::RefCell<Option<std::path::PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Run `f` with any warning it raises attributed to `file`.
///
/// Restores what was set before rather than clearing, so a component that uses
/// another component unwinds to the outer one rather than to the document.
///
/// Sets the script tier's scope as well as this one. Building a component
/// raises warnings from both sinks (a rule that does nothing, an expression
/// that failed) and they are equally the component's, so a reader would have no
/// way to know that only half of them named the right file.
pub fn in_file<T>(file: Option<std::path::PathBuf>, f: impl FnOnce() -> T) -> T {
    let previous = IN_FILE.with(|c| c.replace(file.clone()));
    let out = rux_script::in_file(file, f);
    IN_FILE.with(|c| c.replace(previous));
    out
}

thread_local! {
    /// How many times anything asked to warn or error, whether or not the sink
    /// kept it. [`interpreted`] reads it to tell a declaration set that never
    /// warns, which is safe to share, from one that does.
    static WARN_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn warn(message: String) {
    WARN_CALLS.with(|c| c.set(c.get() + 1));
    let file = IN_FILE.with(|c| c.borrow().clone());
    let warning = Warning::maybe_at(message, AT_LINE.with(|l| l.get())).in_file(file);
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

/// Raise something definitely wrong rather than merely dead, into the same sink.
///
/// `rux check` exits non-zero for these, and the dev overlay reds them. The
/// line is the same [`AT_LINE`] a warning uses, so `located` covers both.
fn error(message: String) {
    WARN_CALLS.with(|c| c.set(c.get() + 1));
    let file = IN_FILE.with(|c| c.borrow().clone());
    let raised = Warning::maybe_at(message, AT_LINE.with(|l| l.get())).in_file(file).as_error();
    WARNINGS.with(|w| {
        let mut w = w.borrow_mut();
        if !w.contains(&raised) {
            w.push(raised);
        }
    });
}

/// Raise a stylesheet warning from outside the cascade.
///
/// The runtime resolves `<style src="…">`, so it is the only layer that can
/// notice an include that cannot be read at all. The warning still belongs in
/// this sink, because to everyone downstream it is one more thing wrong with
/// the document's styling, and having two places to drain would mean the
/// overlay and `rux check` could disagree about what was said.
pub fn warn_stylesheet(message: impl Into<String>) {
    warn(message.into());
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

/// Stop (or resume) mirroring warnings to stderr.
///
/// On by default, which suits anyone running the window, where stderr was the
/// only place a warning could go before the overlay existed. A tool that drains
/// the sink and formats the warnings itself turns it off: printing each one
/// twice, once as prose and once as a diagnostic, is what makes machine-readable
/// output unpipeable.
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
    /// The text's `white-space`, applied to every new value as to the first.
    /// `None` for a run of a sentence, whose spaces are collapsed with the
    /// runs beside it when the layout joins them.
    pub white_space: Option<WhiteSpace>,
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
    /// The `r-key` of the row this input is in, when it is in one. With
    /// [`ValueBinding::locals`] it is what lets the value be read and written in
    /// the row's own scope: the expression is recorded as written, so a model
    /// like `items[item.at].note` means nothing without `item` in scope.
    pub row: Option<String>,
    /// The component instance this input was written in, the third part of its
    /// identity. Two instances of one component record the same model and the
    /// same row, so without this the first one's captured scope answered for
    /// every one of them.
    pub instance: Option<String>,
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
    /// `type="password"`: paint bullets instead of the text.
    ///
    /// **Only the painting.** The signal keeps the real value, and so does
    /// every read of it: an input method has to be handed the true text or
    /// editing breaks outright, and a handler that reads the model is reading
    /// what the person typed. Masking here, at the one place the shown string
    /// is computed, is what keeps that true on all three platforms at once.
    pub secret: bool,
}

use rux_layout::mask;

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
    /// What its children were built from, when they may be built again on
    /// their own. See [`rebuild_children`].
    pub ctx: Option<Rc<ListCtx>>,
}

/// Everything a parent's children are built from besides the template and
/// the state: the ancestors selectors match against (the parent last), what
/// they inherit, and the loop locals in scope. Kept so a reconcile can build
/// one list without building the whole document. See [`rebuild_children`].
pub struct ListCtx {
    /// The parent's template element, by address, to be sure the template
    /// path still finds the same one.
    el: usize,
    ancestors: Vec<AncNode>,
    inherited: Inherited,
    locals: Locals,
}

impl std::fmt::Debug for ListCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ListCtx")
    }
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
    /// Every built node as a selector sees it, for `query()` from script.
    ///
    /// Not reactivity, unlike everything above it, and it rides here because a
    /// build already threads this struct everywhere a node is produced. See
    /// [`ElementIndex`].
    pub elements: ElementIndex,
}


/// Bake the active `r-for` loop bindings into a handler as a `let` prelude, so it
/// still resolves them when it runs later in global scope (the loop variables are
/// gone by then). With no locals the handler is returned unchanged.
/// A boolean attribute, the way HTML has them: written means on, and what it
/// says does not matter. `:name="expr"` is the bound form, on when the
/// expression is truthy, and the signals it reads go into `deps` so a change
/// restyles the element.
///
/// **`disabled="false"` is on**, in HTML and here, and it is the classic trap:
/// the author wrote the word that switches it off and the attribute read only
/// that it was present. Warned rather than reinterpreted, because matching
/// HTML is the rule and the bound form is the right way to say it.
fn bool_attr(
    el: &Element,
    name: &str,
    engine: &mut Engine,
    locals: &Locals,
    deps: &mut HashSet<String>,
) -> bool {
    if let Some(written) = el.attr(name) {
        if written.eq_ignore_ascii_case("false") {
            located(el.attr_line(name), || {
                warn(format!(
                    "`{name}=\"false\"` still means {name}: a boolean attribute is on \
                     whenever it is written, whatever it says. Leave it off, or bind it \
                     with `:{name}=\"expr\"`"
                ))
            });
        }
        return true;
    }
    let bound = format!(":{name}");
    let Some(expr) = el.attr(&bound) else { return false };
    let (on, read) =
        located(el.attr_line(&bound), || engine.eval_bool_tracked(expr, locals));
    deps.extend(read);
    on
}

/// Refuse an `<input>` attribute value Rux cannot act on, the way an unknown
/// `type=` is refused: a keyboard hint that is misspelt is otherwise an
/// ordinary keyboard, and nothing says why.
fn check_field_attributes(el: &Element) {
    for (name, names) in [
        ("inputmode", &rux_layout::Keyboard::NAMES[..]),
        ("enterkeyhint", &rux_layout::EnterKey::NAMES[..]),
    ] {
        let Some(value) = el.attr(name) else { continue };
        if !names.contains(&value) {
            located(el.attr_line(name), || {
                error(format!(
                    "`{name}=\"{value}\"` is not a value Rux knows, so the field would get \
                     the ordinary keyboard. Use one of {}",
                    names.join(", ")
                ))
            });
        }
    }
    for name in ["maxlength", "minlength"] {
        let Some(value) = el.attr(name) else { continue };
        if value.trim().parse::<usize>().is_err() {
            located(el.attr_line(name), || {
                error(format!(
                    "`{name}=\"{value}\"` is not a whole number of characters, so the \
                     field would take any length"
                ))
            });
        }
    }
    // An `autocomplete` token Rux does not know is passed to nobody, so a
    // misspelt `new-pasword` would leave a password manager guessing.
    if let Some(value) = el.attr("autocomplete") {
        for token in value.split_whitespace() {
            let known = token.eq_ignore_ascii_case("off")
                || rux_layout::AUTOCOMPLETE_MODIFIERS.iter().any(|m| m.eq_ignore_ascii_case(token))
                || rux_layout::AUTOCOMPLETE.iter().any(|(n, _)| n.eq_ignore_ascii_case(token))
                || token.starts_with("section-");
            if !known {
                located(el.attr_line("autocomplete"), || {
                    warn(format!(
                        "`autocomplete=\"{value}\"`: `{token}` is not a field name Rux passes \
                         to autofill, so it is ignored. Rux knows {}, and `off`",
                        rux_layout::AUTOCOMPLETE
                            .iter()
                            .map(|(n, _)| *n)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                });
            }
        }
    }
    // A pattern that does not compile would accept everything, which is the
    // one answer nobody who wrote a pattern wanted.
    if let Some(pattern) = el.attr("pattern") {
        if let Err(why) = checks::check_pattern(pattern) {
            located(el.attr_line("pattern"), || {
                error(format!(
                    "`pattern=\"{pattern}\"` is not a regular expression Rux can read, so \
                     the field would accept anything: {why}"
                ))
            });
        }
    }
    // A date's range is two dates, and each would otherwise be dropped in
    // silence, leaving a picker that offers every day there is.
    if el.attr("type") == Some("date") {
        for name in ["min", "max"] {
            let Some(value) = el.attr(name) else { continue };
            if rux_layout::parse_date(value).is_none() {
                located(el.attr_line(name), || {
                    error(format!(
                        "`{name}=\"{value}\"` is not a date written `YYYY-MM-DD`, so the \
                         field would accept any day"
                    ))
                });
            }
        }
        return;
    }
    // A slider's range. Each would otherwise fall back to HTML's default in
    // silence, and a volume control reading 0 to 100 where 0 to 11 was
    // written is not something anyone would think to look for.
    let number = |name: &str| el.attr(name).map(|v| (v, v.trim().parse::<f64>().ok()));
    for name in ["min", "max"] {
        if let Some((value, None)) = number(name) {
            located(el.attr_line(name), || {
                error(format!(
                    "`{name}=\"{value}\"` is not a number, so the slider would use its \
                     default range of 0 to 100"
                ))
            });
        }
    }
    if let (Some((_, Some(min))), Some((max_text, Some(max)))) = (number("min"), number("max")) {
        if max <= min {
            located(el.attr_line("max"), || {
                error(format!(
                    "`max=\"{max_text}\"` is not above `min`, so the slider has no range \
                     to move along"
                ))
            });
        }
    }
    if let Some((value, parsed)) = number("step") {
        if value.trim() != "any" && !parsed.is_some_and(|s| s > 0.0) {
            located(el.attr_line("step"), || {
                error(format!(
                    "`step=\"{value}\"` is not a number above zero, so the slider would \
                     move in steps of 1. Write `step=\"any\"` for no steps at all"
                ))
            });
        }
    }
}

/// The part of a field every kind of input has: which form it is in, the name
/// it is sent under, what it holds and what its value is checked against.
fn form_part(el: &Element, required: bool, ancestors: &[AncNode]) -> rux_layout::Field {
    let ty = el.attr("type");
    let number = |name: &str| el.attr(name).and_then(|v| v.trim().parse::<f64>().ok());
    rux_layout::Field {
        name: el.attr("name").map(str::to_string),
        form: enclosing_form(ancestors),
        bind: el.attr("r-model").map(str::to_string),
        shape: match ty {
            Some("checkbox" | "switch") => rux_layout::Shape::Flag,
            Some("number" | "slider") => rux_layout::Shape::Number,
            _ => rux_layout::Shape::Text,
        },
        keyboard: el
            .attr("inputmode")
            .and_then(rux_layout::Keyboard::parse)
            .unwrap_or_default(),
        checks: rux_layout::Checks {
            required,
            minlength: el.attr("minlength").and_then(|v| v.trim().parse().ok()),
            pattern: el.attr("pattern").map(str::to_string),
            // A slider's `min` and `max` are its travel, which it cannot leave,
            // and a date's are the picker's: only a number field is checked
            // against them.
            low: (ty == Some("number")).then(|| number("min")).flatten(),
            high: (ty == Some("number")).then(|| number("max")).flatten(),
        },
        ..rux_layout::Field::default()
    }
}

/// What an `<input>` says about itself beyond its type. See
/// [`rux_layout::Field`]. Handlers are baked with the row's locals, like a
/// `@tap`, because they run long after the build that could still see the row.
fn field_of(
    el: &Element,
    disabled: bool,
    readonly: bool,
    locals: &[(String, Value)],
    part: rux_layout::Field,
) -> rux_layout::Field {
    let handler = |name: &str| el.attr(name).map(|h| bind_locals(h, locals));
    rux_layout::Field {
        disabled,
        readonly,
        maxlength: el.attr("maxlength").and_then(|v| v.trim().parse().ok()),
        keyboard: el
            .attr("inputmode")
            .and_then(rux_layout::Keyboard::parse)
            .unwrap_or_default(),
        enter_key: el
            .attr("enterkeyhint")
            .and_then(rux_layout::EnterKey::parse)
            .unwrap_or_default(),
        autofocus: el.attr("autofocus").is_some(),
        on_input: handler("@input"),
        on_change: handler("@change"),
        on_focus: handler("@focus"),
        on_blur: handler("@blur"),
        min: el.attr("min").and_then(rux_layout::parse_date),
        max: el.attr("max").and_then(rux_layout::parse_date),
        autocomplete: el.attr("autocomplete").map(str::to_string),
        ..part
    }
}

/// The part of `locals` a handler has to carry with it: what follows the
/// instance's own state and props, which means the `r-for` rows.
///
/// **The instance's own names must not be baked.** The runtime hands a handler
/// run in an instance that instance's state as it is *then*, and a baked
/// `let note = "";` shadows it with the value from the last build. A field
/// types without a build in between, so its `@input` was always one keystroke
/// behind, and the write-back put that stale value into the field: driven on a
/// phone, `@input="typed += 1"` in a routed page counted every key while the
/// field stayed empty.
fn handler_locals<'a>(
    locals: &'a [(String, Value)],
    instance: Option<&str>,
    instances: &Instances,
) -> &'a [(String, Value)] {
    let Some(entry) = instance.and_then(|k| instances.get(k)) else { return locals };
    let own = entry.state.iter().chain(entry.props.iter());
    // The locals start with exactly these names, in this order, where the
    // instance was expanded. Matched by name so a mismatch bakes everything,
    // which is the old behaviour, rather than dropping a row's variable.
    let skip = locals.iter().zip(own).take_while(|((a, _), (b, _))| a == b).count();
    &locals[skip..]
}

fn bind_locals(src: &str, locals: &[(String, Value)]) -> String {
    if locals.is_empty() {
        return src.to_string();
    }
    // The row's place is now written into a string no rewrite can reach. See
    // `RowCache`.
    BAKED.with(|b| b.set(b.get() + 1));
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

/// A value as an error about a prop shows it: short, and saying what kind of
/// value it is, since `3` and `"3"` look alike otherwise.
fn describe_value(value: &Value) -> String {
    match value {
        Value::Text(t) if t.is_empty() => "empty text".to_string(),
        Value::Text(t) if t.chars().count() > 30 => {
            format!("the text \"{}\u{2026}\"", t.chars().take(30).collect::<String>())
        }
        Value::Text(t) => format!("the text {t:?}"),
        Value::Number(_) => format!("the number {}", value.to_display()),
        Value::Bool(b) => format!("`{b}`"),
        Value::List(items) => format!("a list of {}", items.len()),
        Value::Map(_) => "a map".to_string(),
        Value::Null => "`null`".to_string(),
    }
}

/// What a component built without `prop name` has in its place, to finish the
/// sentence that says it was left out.
fn instead(component: &Component, name: &str) -> &'static str {
    match component.props.iter().find(|p| p.name == name) {
        Some(p) if p.default.is_some() => ", and it takes its default",
        _ => "; reading it inside fails",
    }
}

/// A compiled component: its template root, its own CSS rules, and the
/// top-level script that gives each instance its private state.
struct Component {
    /// The file it was read from, so a warning raised while expanding it names
    /// that file instead of whoever imported it. `None` for a component that
    /// came from somewhere with no filesystem, the browser build being the one
    /// that does.
    file: Option<std::path::PathBuf>,
    template: Element,
    /// Its own rules, with the document's merged in ahead of them unless
    /// either side said `<style scoped>`. Merged once at load; see
    /// `build_styled_tree_stateful`.
    rules: Vec<Rule>,
    /// The component's own `let` declarations, run once per instance. Its `fn`
    /// definitions are not here: those are shared, merged into the one engine,
    /// because a function is code and state is not.
    script: String,
    /// What it declared with `prop`: which attributes on its tag are props, and
    /// the default for any the caller leaves off.
    props: Vec<rux_parser::PropDecl>,
    /// What each of `props` takes, parsed once here rather than per tag.
    /// `None` for a prop declared without a type, or with one that does not
    /// parse, which the checker has already reported.
    prop_types: Vec<Option<rux_script::types::Type>>,
    /// What a declared name in one of those stands for.
    types: HashMap<String, rux_script::validate::Decl>,
}

impl Component {
    /// What `prop name` takes, when it says.
    fn prop_type(&self, name: &str) -> Option<&rux_script::types::Type> {
        let at = self.props.iter().position(|p| p.name == name)?;
        self.prop_types[at].as_ref()
    }
}

/// One component instance's private world: the state its own script declared,
/// and the props the caller passed in.
///
/// State persists across rebuilds, keyed by where the instance sits in the
/// *template* rather than in the tree, so it survives a list reordering around
/// it. Props are re-derived on every build, since they are the caller's to
/// decide.
#[derive(Clone, Debug, Default)]
pub struct Instance {
    /// Which component this is an instance of.
    ///
    /// Carried on the instance rather than looked up from the template, because
    /// the moment it is needed is the moment the template no longer mentions it:
    /// an instance that has just been pruned still has to say whose `unmounted`
    /// body to run.
    pub tag: String,
    pub state: Vec<(String, Value)>,
    pub props: Vec<(String, Value)>,
    /// What the caller wrote as `@event="…"` on the tag: the bodies to run when
    /// this instance emits. The caller's `r-for` locals are already baked in,
    /// the same treatment an `@tap` gets, since the body runs long after the
    /// build that read them.
    pub listeners: Vec<(String, String)>,
    /// The instance the caller itself sits in, `None` at document level. A
    /// listener body is the caller's code and must run in the caller's scope:
    /// anything else writes to the wrong variables and looks like it worked.
    pub caller: Option<String>,
    /// The path this instance was expanded under, when a `<router>` chose it.
    /// Leaving that route drops the instance, so a view visited a second time
    /// starts fresh rather than resuming where it was left. Anything meant to
    /// outlive a visit belongs in a document signal, which is the same rule
    /// components already follow for anything the caller needs to see.
    pub route: Option<String>,
    /// Whether the build now running has expanded this instance.
    ///
    /// Every build walks the whole template, including the one a reconcile does
    /// before splicing, so an instance the current build never reached is one
    /// that is no longer on screen: an `r-if` closed over it, or its `r-for` row
    /// went away. Cleared at the start of a build and checked at the end, which
    /// is the only moment both facts are known.
    pub touched: bool,
}

/// Every live component instance, by identity. Owned by the runtime so it
/// outlives the tree, which is rebuilt constantly.
pub type Instances = HashMap<String, Instance>;

// ── Enter/leave ─────────────────────────────────────────────────────────────

/// What identifies a swap across builds.
///
/// The **template** path, not the tree path. A leaving element's tree path moves
/// as siblings around it come and go, so keying on it would lose the swap the
/// moment a neighbour changed. The template path is fixed by the source file.
/// The second half is the `r-key` of the row, `None` for an `r-if`.
pub type SwapKey = (Vec<usize>, Option<String>);

/// The loop variables one `r-for` row was built with.
pub type RowLocals = Vec<(String, Value)>;

/// A row of an animated list as remembered: where it sat, and what built it.
type RememberedRow = (usize, RowLocals);

/// A row that has left the collection but is still on screen: its position, its
/// key, and enough to build it with now that its item is gone.
type DepartedRow = (usize, String, RowLocals);

/// Which side of a swap an element is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The build has just started asking for this element.
    Entering,
    /// The build has stopped asking for this element, and it is on screen only
    /// because the swap is holding it there.
    Leaving,
}

/// Which side of a swap one built element is on, and whether this is the first
/// build since it opened. Decided by [`Swaps::resolve`], since that is the only
/// place both facts are known.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwapSide {
    pub phase: Phase,
    pub first: bool,
}

/// Where a swap's progress comes from.
#[derive(Clone, Debug)]
pub enum Driver {
    /// Wall time. The build records only the *duration*, read from the
    /// element's own `transition` declarations, so there is one place a
    /// duration is written and it is the CSS. Whoever owns the clock turns that
    /// into `end_ms` the first frame it sees the swap: the build has no time
    /// source and must not grow one.
    Clock { duration: f32, end_ms: Option<f64> },
    /// An author expression, re-read every build, yielding 0..=1. Reaching 1
    /// commits and reaching 0 abandons. This is what binds a swap to a finger.
    Bound(String),
}

/// One swap in flight.
///
/// Its whole job is to make the build emit an element the build would otherwise
/// have dropped, and to say which of `:enter-from` / `:leave-to` is held while
/// it does. The interpolation itself is tier 1's: an entering element is styled
/// `:enter-from` for exactly one frame, so the frame after is an ordinary style
/// change and `transition` animates it with no knowledge that a swap happened.
#[derive(Clone, Debug)]
pub struct Swap {
    pub phase: Phase,
    pub driver: Driver,
    /// Progress 0..=1, only meaningful under [`Driver::Bound`]; the clock driver
    /// leaves the interpolation to the animator and never reads this.
    pub progress: f32,
    /// How many builds this swap has been through.
    ///
    /// `:enter-from` is held on the first and dropped on the second, and that
    /// asymmetry is the whole trick: the drop is an ordinary style change, which
    /// is what tier 1 already animates. Holding it a second build would mean the
    /// target never moves and nothing would ever animate at all.
    pub builds: u32,
    /// The row's loop variables, captured when a keyed `r-for` row starts
    /// leaving. By then the item is gone from the collection, so this is the
    /// only remaining way to build the row it used to render.
    pub locals: RowLocals,
    /// Whether the build now running has reached this swap. Same rule, and the
    /// same reason, as [`Instance::touched`].
    pub touched: bool,
    /// Whether a bound swap's progress has ever been off zero.
    ///
    /// Without it a bound swap abandons on the frame it opens: it starts at 0,
    /// and 0 is also where abandoning is decided. Nothing to do with a clock
    /// swap, which has a deadline instead of an end value.
    pub moved: bool,
}

/// Every swap in flight, and what the last build showed. Owned by the runtime
/// beside [`Instances`], because like an instance a swap outlives the tree it
/// appears in.
///
/// `shown` is the half that is easy to forget and impossible to do without: a
/// swap opens on the *difference* between what the last build emitted and what
/// this one asks for, and the tree cannot report the first of those, having been
/// thrown away. Nothing else in the build remembers a structural decision.
#[derive(Clone, Debug, Default)]
pub struct Swaps {
    shown: HashSet<SwapKey>,
    /// What the build now running has emitted, promoted to `shown` at the end.
    building: HashSet<SwapKey>,
    pending: HashMap<SwapKey, Swap>,
    /// The locals each keyed row of an animated `r-for` was last built with,
    /// and where in the list it sat.
    ///
    /// A departing row is the case an `r-if` does not have: by the time the row
    /// is leaving, its item is gone from the collection, so there is nothing
    /// left to build it from unless it was kept. Only lists that actually
    /// declare `r-transition` pay for this, since it is a clone per row per
    /// build and a list nobody is animating should not be charged for one.
    rows: HashMap<SwapKey, RememberedRow>,
}

impl Swaps {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether anything is mid-swap, so the caller can skip the work entirely.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn pending(&self) -> impl Iterator<Item = (&SwapKey, &Swap)> {
        self.pending.iter()
    }

    /// Start a build. Clears the per-build marks, the same bookkeeping
    /// [`Instance::touched`] does and for the same reason.
    pub fn begin(&mut self) {
        self.building.clear();
        for swap in self.pending.values_mut() {
            swap.touched = false;
        }
    }

    /// Finish a build: what it emitted becomes what is shown, and a swap the
    /// build never reached is one whose element is gone for a reason that has
    /// nothing to do with animation (an ancestor closed over it), so it goes
    /// too rather than holding a subtree nothing is drawing.
    pub fn end(&mut self) {
        self.shown = std::mem::take(&mut self.building);
        self.pending.retain(|_, s| s.touched);
    }

    /// [`begin`](Self::begin) for a build of one parent's children, which
    /// happens only when no swap is pending, so there is nothing to mark.
    fn begin_part(&mut self) {
        self.building.clear();
    }

    /// [`end`](Self::end) for a build of the children of the parent at
    /// template path `tpl`: what it emitted replaces what was shown under it,
    /// and everything else stays shown.
    fn end_part(&mut self, tpl: &[usize]) {
        self.shown.retain(|(path, _)| !(path.len() > tpl.len() && path.starts_with(tpl)));
        self.shown.extend(self.building.drain());
    }

    /// Remember what a row of an animated `r-for` was built with, so it can
    /// still be built after it has left the collection.
    fn remember_row(&mut self, key: &SwapKey, index: usize, locals: RowLocals) {
        self.rows.insert(key.clone(), (index, locals));
    }

    /// The rows of one animated `r-for` that the collection no longer has, as
    /// `(index, key, locals)` ready to build, oldest position first.
    ///
    /// Owned rather than borrowed because the caller needs `&mut self` again to
    /// build them, which is also what opens their swaps.
    fn departed_rows(
        &self,
        tpl: &[usize],
        present: &[String],
    ) -> Vec<DepartedRow> {
        let mut out: Vec<DepartedRow> = self
            .rows
            .iter()
            .filter(|((path, key), _)| {
                path == tpl && key.as_ref().is_some_and(|k| !present.contains(k))
            })
            .map(|((_, key), (index, locals))| {
                (*index, key.clone().unwrap_or_default(), locals.clone())
            })
            .collect();
        out.sort_by_key(|(index, _, _)| *index);
        out
    }

    /// Whether a swap is currently holding the page at `path` on screen.
    ///
    /// The router drops a route's instances as it leaves that route, which would
    /// take the state out from under a page that is still animating out. This is
    /// what tells it to wait.
    pub fn holds_route(&self, path: &str) -> bool {
        self.rows.iter().any(|(key, (_, locals))| {
            self.pending.contains_key(key)
                && locals.iter().any(|(name, value)| {
                    name == "path" && value.to_display() == path
                })
        })
    }

    /// Forget a row for good. Called when its leaving swap commits, so the
    /// remembered locals do not outlive the row they describe.
    fn forget_row(&mut self, key: &SwapKey) {
        self.rows.remove(key);
    }

    /// Whether an entering swap is still holding `:enter-from` and so needs one
    /// more build to let go of it.
    ///
    /// Easy to miss and fatal without: the frame that *drops* `:enter-from` is
    /// the one that starts the animation, and nothing else would ask for it. A
    /// condition change rebuilds once; without this the element would sit at its
    /// entering offset forever.
    pub fn needs_settling(&self) -> bool {
        self.pending.values().any(|s| s.phase == Phase::Entering && s.builds < 2)
    }

    /// The swap is over: drop it and let the build's decision stand.
    ///
    /// A leaving element also leaves `shown`. Forgetting that half is a loop
    /// rather than a glitch: the next build would compare "not wanted" against
    /// "was shown", see a difference, and open the very same swap again.
    fn finish(&mut self, key: &SwapKey) {
        if let Some(swap) = self.pending.remove(key) {
            if swap.phase == Phase::Leaving {
                self.shown.remove(key);
                self.forget_row(key);
            }
        }
    }

    /// Advance every swap against the clock and finish the ones that are done.
    ///
    /// Returns how long until the next one needs looking at again, and whether
    /// anything finished (which changes the tree, so the caller has to rebuild).
    /// A **delay**, not a deadline, because that is what the animator returns
    /// and the shell folds the two into one wait. A `None` means nothing is in
    /// flight and the app can sleep, the same property the animator protects:
    /// an idle Rux app must not burn frames.
    pub fn advance(&mut self, now_ms: f64) -> (Option<f64>, bool) {
        let mut done: Vec<SwapKey> = Vec::new();
        let mut wake: Option<f64> = None;
        for (key, swap) in self.pending.iter_mut() {
            match &mut swap.driver {
                Driver::Clock { duration, end_ms } => {
                    let end = *end_ms.get_or_insert(now_ms + *duration as f64);
                    if now_ms >= end {
                        done.push(key.clone());
                    } else {
                        let delay = end - now_ms;
                        wake = Some(wake.map_or(delay, |w: f64| w.min(delay)));
                    }
                }
                // The author owns progress, so there is no deadline to wait on:
                // the next build is what moves it, and the two ends are where
                // the swap is over. Reaching 1 commits, returning to 0 abandons.
                Driver::Bound(_) => {
                    if swap.progress >= 1.0 || (swap.moved && swap.progress <= 0.0) {
                        done.push(key.clone());
                    }
                }
            }
        }
        let finished = !done.is_empty();
        for key in &done {
            self.finish(key);
        }
        (wake, finished)
    }

    /// Decide what to do with one structural element this build, and record it.
    ///
    /// `wanted` is what the condition (or the collection) says. The return is
    /// whether to emit it at all, and if so which side it is on.
    fn resolve(&mut self, key: &SwapKey, wanted: bool, animated: bool) -> Option<Option<SwapSide>> {
        // A reused row would skip this bookkeeping. See `RowCache`.
        UNREPLAYABLE.with(|u| u.set(u.get() + 1));
        if !animated {
            // No `r-transition`: the old behaviour exactly, and no bookkeeping
            // beyond recording what was shown, so an element that gains the
            // attribute later does not think it has been there all along.
            if wanted {
                self.building.insert(key.clone());
            }
            self.pending.remove(key);
            return wanted.then_some(None);
        }
        let was = self.shown.contains(key);
        if let Some(swap) = self.pending.get_mut(key) {
            swap.touched = true;
            // The condition changing back mid-swap reverses it rather than
            // opening a second one. This is what makes a swap that is bound to
            // a finger able to change its mind.
            swap.phase = if wanted { Phase::Entering } else { Phase::Leaving };
            swap.builds = swap.builds.saturating_add(1);
            let side = SwapSide {
                phase: swap.phase,
                first: swap.builds == 1,
            };
            self.building.insert(key.clone());
            return Some(Some(side));
        }
        if wanted == was {
            if wanted {
                self.building.insert(key.clone());
            }
            return wanted.then_some(None);
        }
        // A change with no swap yet: open one. The caller fills in the driver,
        // which it can only do once it has built the element and can read its
        // `transition` declarations.
        let phase = if wanted { Phase::Entering } else { Phase::Leaving };
        self.pending.insert(
            key.clone(),
            Swap {
                phase,
                driver: Driver::Clock { duration: 0.0, end_ms: None },
                progress: 0.0,
                locals: Vec::new(),
                touched: true,
                moved: false,
                builds: 1,
            },
        );
        self.building.insert(key.clone());
        Some(Some(SwapSide { phase, first: true }))
    }
}

// ── Component lifecycle ─────────────────────────────────────────────────────

/// A component instance that appeared or disappeared during a build.
///
/// The build is the only place both facts are known, and it is the wrong place
/// to *act* on them: running a hook body here would run author code in the
/// middle of expanding a tree. So the build reports, and the runtime decides.
#[derive(Clone, Debug)]
pub struct Lifecycle {
    /// The instance's key, so the runtime can run a `mounted` body in the scope
    /// of an instance that is still in the map.
    pub key: String,
    /// Whose hook bodies to run.
    pub tag: String,
    /// The scope the body should run in, carried only for an unmount: by the
    /// time the runtime looks, the instance is gone from the map, and its
    /// `unmounted` body has to run against the names it was written for. State
    /// first and props after, so a prop of the same name wins, which is the
    /// order the instance itself was expanded with.
    pub locals: Vec<(String, Value)>,
}

thread_local! {
    /// Instances created and pruned by the current build, in build order.
    ///
    /// A sink rather than a return value: three public build entry points share
    /// this path, and threading a report out of all of them would change every
    /// signature for a thing only one caller wants. Drained by
    /// [`take_lifecycle`], on the same terms as the warning sink above.
    static MOUNTS: std::cell::RefCell<Vec<Lifecycle>> = const { std::cell::RefCell::new(Vec::new()) };
    static UNMOUNTS: std::cell::RefCell<Vec<Lifecycle>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Take what the last build mounted and unmounted, emptying both sinks.
///
/// Mounts come back outermost-first, the order the template expands in: an
/// inner component is only reached through its parent, so the parent's
/// `mounted` cannot usefully run second on the first build anyway. Unmounts are
/// in whatever order the map pruned them, which is not observable: a dead
/// instance's body cannot see another dead instance.
/// Queue an `unmounted` for an instance the *runtime* is dropping.
///
/// The build queues its own as it prunes, but the router drops a route's views
/// on the way out of that route, before any build runs, and that removal used
/// to bypass the sink entirely: a route view's `unmounted` never fired at all.
pub fn queue_unmount(entry: Lifecycle) {
    UNMOUNTS.with(|u| u.borrow_mut().push(entry));
}

pub fn take_lifecycle() -> (Vec<Lifecycle>, Vec<Lifecycle>) {
    (
        MOUNTS.with(|m| std::mem::take(&mut *m.borrow_mut())),
        UNMOUNTS.with(|u| std::mem::take(&mut *u.borrow_mut())),
    )
}

/// A component instance's identity: where it sits in the template, and which
/// `r-for` row it is in.
///
/// Deliberately not the tree path, which moves when a list reorders. Two
/// `<panel>` elements written in one file are two instances; the same `<panel>`
/// repeated by `r-for` is one instance per row.
fn instance_key(tpl_path: &[usize], row: Option<&str>) -> String {
    let mut key = String::new();
    for step in tpl_path {
        key.push_str(&step.to_string());
        key.push('.');
    }
    if let Some(row) = row {
        key.push('#');
        key.push_str(row);
    }
    key
}

/// A component's own top-level statements: its script with `fn` blocks and
/// `use` lines removed.
///
/// The functions are merged into the shared engine (code is shared; two
/// instances calling one function is right), while what is left declares the
/// state each instance gets its own copy of.
fn component_statements(script: &str) -> String {
    let mut out = String::new();
    let lines: Vec<&str> = script.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("use ") {
            i += 1;
            continue;
        }
        let bare = trimmed.strip_prefix("private ").map(str::trim_start).unwrap_or(trimmed);
        let bare = bare.strip_prefix("async ").map(str::trim_start).unwrap_or(bare);
        if bare.starts_with("fn ") {
            // Skip the whole definition by counting braces, so a function with
            // an `if` inside does not end early.
            let mut depth = 0i32;
            let mut seen = false;
            while i < lines.len() {
                for c in lines[i].chars() {
                    match c {
                        '{' => {
                            depth += 1;
                            seen = true;
                        }
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                i += 1;
                if seen && depth <= 0 {
                    break;
                }
            }
            continue;
        }
        out.push_str(lines[i]);
        out.push('\n');
        i += 1;
    }
    out
}

/// What a `<slot />` inside a component renders: the children written at the
/// call site, and the context they were written in.
///
/// The context travels because slot content belongs to the *caller*, not to the
/// component. It reads the caller's scope, including the caller's own instance
/// state, which the component has no access to, and
/// is styled by the caller's stylesheet (the component's rules are written for
/// markup the component itself wrote). Only its position in the tree comes from
/// the component.
#[derive(Clone, Copy)]
struct Slot<'a> {
    children: &'a [&'a Element],
    locals: &'a Locals,
    rules: &'a [Rule],
}

/// What a `<router-view />` deeper in a route's view still has to render.
///
/// Threaded like [`Slot`], and for the same reason: the element that decides the
/// content and the element that places it are in different files. A slot is
/// filled by the caller's markup; an outlet is filled by the route that matched
/// under this one.
#[derive(Clone, Copy)]
struct Outlet<'a> {
    /// The links below the one being rendered, outermost first.
    rest: &'a [Link<'a>],
    /// Every parameter the whole chain captured. The same set reaches every
    /// view in it, so a leaf sees what its parent matched.
    params: &'a Locals,
    /// The path that matched, for instance bookkeeping.
    current: &'a str,
    /// The template path of the `<router>` this chain belongs to, so each view
    /// in the chain gets an identity that survives a rebuild.
    router_tpl: &'a [usize],
    /// Set when an outlet actually renders a link. A route with children whose
    /// view never places a `<router-view />` would otherwise lose the child in
    /// silence, which is the failure this project keeps closing off.
    used: &'a std::cell::Cell<bool>,
    /// The namespace key of the file that wrote the `<router>`.
    ///
    /// A route's `view` is a name in **that** file, and a `<router-view />` is
    /// usually several components deep by the time it renders the next link. So
    /// the namespace travels with the chain rather than being whatever file the
    /// outlet happens to sit in, which is how a nested route came to report its
    /// own view as "not imported" the moment tags became per-file.
    router_ns: &'a str,
}

/// An element's element children, skipping text nodes. Text between tags is
/// handled by the text-binding path, not by the child builder.
fn element_children(el: &Element) -> Vec<&Element> {
    el.children
        .iter()
        .filter_map(|n| match n {
            TplNode::Element(child) => Some(child),
            TplNode::Text(..) => None,
        })
        .collect()
}

/// Registered components, keyed by whatever the loader made unique per file.
type Components = HashMap<String, Component>;

/// What one file's markup may name: tag, to the key of the component it means.
///
/// Per file rather than per document, because a tag is a **local** name. Two
/// files may each `use` a different `task.rux` and each write `<task>`, and
/// before this they shared one flat map, so the second import silently replaced
/// the first and one of the two files rendered the other's component.
pub type Namespace = HashMap<String, String>;

/// The namespace of the document itself, which is not a component and so has no
/// key of its own.
pub const DOCUMENT_NAMESPACE: &str = "";

thread_local! {
    /// Every file's namespace, refreshed at the top of each build.
    static NAMESPACES: std::cell::RefCell<HashMap<String, Namespace>> =
        std::cell::RefCell::new(HashMap::new());

    /// Whose markup is being built right now: a component's key, or
    /// [`DOCUMENT_NAMESPACE`].
    ///
    /// A thread-local for the same reason `IN_FILE` is one, and with exactly the
    /// same scope: it changes only at a component boundary, which is one place
    /// (`expand_component`), while the alternative is a parameter threaded
    /// through every function in the tree walk that has no other reason to know
    /// what a file is.
    static CURRENT_FILE: std::cell::RefCell<String> =
        const { std::cell::RefCell::new(String::new()) };
}

/// Build `f` as though its markup were written in the file keyed `key`.
///
/// Restores whatever was current before rather than clearing, so nesting
/// unwinds: a component that uses a component returns to its own namespace and
/// not to the document's.
fn in_namespace<T>(key: &str, f: impl FnOnce() -> T) -> T {
    let previous = CURRENT_FILE.with(|c| c.replace(key.to_string()));
    let out = f();
    CURRENT_FILE.with(|c| *c.borrow_mut() = previous);
    out
}

/// Look a component up by either spelling, and say which one it really is.
///
/// One component has two names and the author did not choose either of them.
/// `use pages::new_task;` **has** to be snake: it is a Rust-shaped path naming
/// `pages/new_task.rux`, and `new-task` there is a subtraction. The tag it
/// contributes **has** to be kebab: that is what a custom element looks like
/// everywhere else on the web. So the underscore and the hyphen are both
/// forced, at opposite ends of the same file, and the author is left holding
/// the difference — which showed up as `view="new_task"` being told to go and
/// write the other spelling of a name it had already given correctly.
///
/// So the template accepts either, and the script stays strict. Nothing is
/// ambiguous: `extract_imports` maps `_` to `-`, so no import can ever produce
/// a tag with an underscore in it, and the two spellings can never name two
/// different components.
///
/// The returned name is the **canonical** one, the key the component is
/// registered under. Everything an instance owns is filed by that name — its
/// `mounted` body, its `computed`s, its `effect`s — so handing back what the
/// author typed would give one component two identities and split its state
/// down the middle of a file.
/// Looked up in the namespace of the file being built, not in one flat map, so
/// the same tag may mean different components in two files.
fn find_component<'a>(comps: &'a Components, name: &str) -> Option<(&'a str, &'a Component)> {
    let kebab = name.replace('_', "-");
    let key = NAMESPACES.with(|all| {
        let all = all.borrow();
        CURRENT_FILE.with(|here| {
            let namespace = all.get(&*here.borrow())?;
            namespace.get(name).or_else(|| namespace.get(&kebab)).cloned()
        })
    })?;
    comps.get_key_value(&key).map(|(key, component)| (key.as_str(), component))
}

/// Default inherited text colour (`#cdd6f4`) and font size, used at the root
/// before any `color` / `font-size` rule applies. Text properties inherit.
const DEFAULT_COLOR: Rgba = Rgba::new(0.804, 0.839, 0.957, 1.0);
const DEFAULT_FONT_SIZE: f32 = 16.0;

/// The text properties that inherit down the tree: an element uses its own
/// `color`/`font-size`/`font-family` if set, else its parent's resolved value.
#[derive(Clone)]
struct Inherited {
    color: Rgba,
    /// The nearest `::selection` above, see [`matched_selection`].
    selection: SelectionStyle,
    font_size: f32,
    font_family: Option<String>,
    /// Custom properties (`--name`) in scope. They inherit like the text
    /// properties above, see [`Vars`].
    vars: Vars,
    /// Inside a `<text>`: what its runs take from it beyond inheritance.
    /// `None` everywhere else, which is also how a build knows it is not
    /// inside a sentence. See [`RunInherit`].
    run: Option<RunInherit>,
    /// `direction`, which inherits everywhere, as in CSS.
    direction: rux_layout::Direction,
    /// The rest of the inheriting text properties. See [`TextInherit`].
    text: TextInherit,
    /// `text-shadow`, which inherits; not in `TextInherit`, which is `Copy`.
    shadows: Option<Arc<Vec<rux_layout::TextShadow>>>,
    /// The parent's rarely set text properties, see [`text_look`].
    look: Option<Arc<TextLook>>,
}

/// The inheriting text properties beyond `color`, `font-size` and
/// `font-family`, which CSS passes from every box to everything inside it:
/// `font-weight` on a card makes its text bold. Rux passed only those three
/// until v0.9, and the rest had to be written on each `<text>`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TextInherit {
    weight: u16,
    italic: bool,
    letter_spacing: Option<f32>,
    word_spacing: Option<f32>,
    white_space: WhiteSpace,
    line_height: Option<LineHeight>,
    align: TextAlign,
    /// The used `color-scheme`, which `light-dark()` picks a side by.
    scheme: ColorScheme,
    /// `visibility: hidden` or `collapse`.
    invisible: bool,
    /// `pointer-events: none`.
    no_pointer: bool,
}

impl Default for TextInherit {
    fn default() -> Self {
        Self {
            weight: 400,
            italic: false,
            letter_spacing: None,
            word_spacing: None,
            white_space: WhiteSpace::Normal,
            line_height: None,
            align: TextAlign::Start,
            scheme: ColorScheme::Light,
            invisible: false,
            no_pointer: false,
        }
    }
}

/// `line-height` as it inherits. A bare number is inherited as the number and
/// multiplied by each element's own font size, which is why it is the form CSS
/// recommends; a length is inherited as the pixels it came to.
#[derive(Clone, Copy, Debug, PartialEq)]
enum LineHeight {
    Factor(f32),
    Px(f32),
}

impl LineHeight {
    fn px(self, font_size: f32) -> f32 {
        match self {
            LineHeight::Factor(f) => f * font_size,
            LineHeight::Px(p) => p,
        }
    }
}

/// What a run of a sentence takes from the `<text>` it is in beyond what
/// inherits: its decorations. CSS does not inherit `text-decoration`, but
/// draws it under every word of the box that has it, nested runs included,
/// which comes to the same thing.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RunInherit {
    underline: bool,
    strikethrough: bool,
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
    resolve_vars_in(value, &|name| vars.get(name).map(String::as_str), depth)
}

/// [`resolve_vars`] against any lookup, so an element's own custom
/// properties can be read before they are resolved. See [`take_vars`].
fn resolve_vars_in<'v>(value: &str, vars: &dyn Fn(&str) -> Option<&'v str>, depth: usize) -> String {
    if depth >= MAX_VAR_DEPTH || !value.contains("var(") {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = find_call(rest, "var(") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 4..];
        // The closing paren of *this* call, allowing nested parens in a
        // fallback (`var(--x, rgb(0, 0, 0))`).
        let Some(end) = closing_paren(after) else {
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
        match vars(name) {
            // The substituted value may itself contain var().
            Some(v) => out.push_str(&resolve_vars_in(v, vars, depth + 1)),
            None => match fallback {
                Some(f) => out.push_str(&resolve_vars_in(f, vars, depth + 1)),
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
    let registered = REGISTERED.with(|r| Rc::clone(&r.borrow().1));
    let inherited = &registered_over(inherited, &registered, &declared);
    if declared.is_empty() {
        return Rc::clone(inherited);
    }
    // Remove them: a custom property is not a real property, and leaving it in
    // would only be fed to `interpret` (which ignores it) as noise.
    let own: HashMap<String, String> =
        declared.into_iter().filter_map(|name| props.remove(&name).map(|v| (name, v))).collect();
    // Each is resolved against all of this element's own, as written, over
    // what it inherited, so `--a: var(--b); --b: red` on one element works
    // whichever order they come in. A theme declares dozens on one rule, in
    // terms of each other, and resolving them one by one in a map's order
    // made some of them undefined at random.
    let mut vars = (**inherited).clone();
    for (name, raw) in &own {
        let lookup = |key: &str| -> Option<&str> {
            match own.get(key) {
                Some(_) if key == name => inherited.get(key).map(String::as_str),
                Some(v) => Some(v.as_str()),
                None => inherited.get(key).map(String::as_str),
            }
        };
        let value = resolve_vars_in(raw, &lookup, 0);
        // `--pad: env(safe-area-inset-bottom)` is the way an author writes this
        // once and reads it everywhere, so it has to resolve where the variable
        // is declared and not only where a property is.
        let value = if value.contains("env(") { resolve_env(&value) } else { value };
        vars.insert(name.clone(), value);
    }
    Rc::new(vars)
}

/// A custom property an `@property` rule registered: what it is when nothing
/// sets it, and whether it inherits.
#[derive(Clone, Debug, PartialEq)]
struct Registered {
    initial: Option<String>,
    inherits: bool,
}

thread_local! {
    /// The document's `@property` registrations, with the print of the
    /// sheets they were read from, so a build reads them again only when the
    /// sheets changed. See [`register_properties`].
    static REGISTERED: std::cell::RefCell<(u64, Rc<HashMap<String, Registered>>)> =
        std::cell::RefCell::new((0, Rc::new(HashMap::new())));
    /// The document's `@keyframes`, by name, read with the `@property`
    /// rules. See [`register_properties`].
    static KEYFRAMES: std::cell::RefCell<Rc<HashMap<String, Rc<[Keyframe]>>>> =
        std::cell::RefCell::new(Rc::new(HashMap::new()));
}

/// One keyframe as written: where it sits, from 0 to 1, and its declarations.
#[derive(Clone, Debug)]
struct Keyframe {
    offset: f32,
    decls: Vec<(String, String)>,
}

/// Read the document's `@property` rules for the build about to run.
///
/// The document's sheets and what they include. A component's own
/// `@property` is not read yet.
fn register_properties(sfc: &Sfc) {
    let print = rules_print(sfc);
    if REGISTERED.with(|r| r.borrow().0 == print) {
        return;
    }
    let mut out = HashMap::new();
    let mut frames = HashMap::new();
    for css in sfc.style_includes.iter().map(|i| i.css.as_str()).chain(std::iter::once(sfc.style.as_str())) {
        if !css.contains("@property") && !css.contains("keyframes") {
            continue;
        }
        if let Ok(sheet) = StyleSheet::parse(css, ParserOptions::default()) {
            gather_registered(&sheet.rules.0, &mut out);
            gather_keyframes(&sheet.rules.0, &mut frames);
        }
    }
    REGISTERED.with(|r| *r.borrow_mut() = (print, Rc::new(out)));
    KEYFRAMES.with(|k| *k.borrow_mut() = Rc::new(frames));
}

/// Every `@keyframes` in the sheet, the last of a name winning, as in CSS.
/// A keyframe listing several places (`0%, 100%`) is one at each.
fn gather_keyframes(rules: &[CssRule], out: &mut HashMap<String, Rc<[Keyframe]>>) {
    use lightningcss::rules::keyframes::{KeyframeSelector, KeyframesName};
    for rule in rules {
        match rule {
            CssRule::Keyframes(k) => {
                let name = match &k.name {
                    KeyframesName::Ident(i) => i.0.to_string(),
                    KeyframesName::Custom(s) => s.to_string(),
                };
                let mut frames = Vec::new();
                for frame in &k.keyframes {
                    let decls: Vec<(String, String)> = frame
                        .declarations
                        .declarations
                        .iter()
                        .filter_map(|p| p.to_css_string(false, PrinterOptions::default()).ok())
                        .filter_map(|t| t.split_once(':').map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string())))
                        .collect();
                    for selector in &frame.selectors {
                        let offset = match selector {
                            KeyframeSelector::From => 0.0,
                            KeyframeSelector::To => 1.0,
                            KeyframeSelector::Percentage(p) => p.0,
                            // A scroll timeline's range: Rux's timelines are time.
                            KeyframeSelector::TimelineRangePercentage(_) => continue,
                        };
                        frames.push(Keyframe { offset: offset.clamp(0.0, 1.0), decls: decls.clone() });
                    }
                }
                frames.sort_by(|a, b| a.offset.total_cmp(&b.offset));
                out.insert(name, frames.into());
            }
            CssRule::LayerBlock(block) => gather_keyframes(&block.rules.0, out),
            CssRule::Supports(sup) if layer::supports(&sup.condition) => gather_keyframes(&sup.rules.0, out),
            _ => {}
        }
    }
}

fn gather_registered(rules: &[CssRule], out: &mut HashMap<String, Registered>) {
    for rule in rules {
        match rule {
            CssRule::Property(p) => {
                let initial = p.initial_value.as_ref().and_then(|v| v.to_css_string(PrinterOptions::default()).ok());
                out.insert(p.name.0.to_string(), Registered { initial, inherits: p.inherits });
            }
            CssRule::LayerBlock(block) => gather_registered(&block.rules.0, out),
            CssRule::Supports(sup) if layer::supports(&sup.condition) => gather_registered(&sup.rules.0, out),
            _ => {}
        }
    }
}

/// What an element inherits, with the registered custom properties put
/// right: one that does not inherit is back at its initial value here unless
/// this element sets it, and one nobody set anywhere is at its initial value.
///
/// That is what keeps a card's `--tw-shadow` out of a ring on a button
/// inside it: Tailwind registers its `--tw-*` with `inherits: false`, and
/// reads several of them into one `box-shadow`. Most elements find nothing
/// to change and share their parent's map.
fn registered_over(inherited: &Vars, registered: &HashMap<String, Registered>, own: &[String]) -> Vars {
    let target = |name: &String, reg: &Registered| -> Option<Option<String>> {
        if own.contains(name) || (reg.inherits && inherited.contains_key(name)) {
            return None;
        }
        (inherited.get(name) != reg.initial.as_ref()).then(|| reg.initial.clone())
    };
    if registered.iter().all(|(name, reg)| target(name, reg).is_none()) {
        return Rc::clone(inherited);
    }
    let mut vars = (**inherited).clone();
    for (name, reg) in registered {
        match target(name, reg) {
            Some(Some(initial)) => {
                vars.insert(name.clone(), initial);
            }
            Some(None) => {
                vars.remove(name);
            }
            None => {}
        }
    }
    Rc::new(vars)
}

thread_local! {
    /// What `env()` resolves against while a tree is being built.
    ///
    /// A thread-local rather than a parameter because `build_node_inner`
    /// already takes twenty of those, and this is read in one place: the pass
    /// that substitutes values before they are interpreted. It follows the
    /// pattern the source provider and the fragment flag already set, and
    /// `cargo test`'s threads each get their own.
    static BUILD_ENV: std::cell::Cell<Environment> =
        const { std::cell::Cell::new(Environment::HEADLESS) };
}

thread_local! {
    /// Whether this build keeps each ancestor's preceding siblings, because
    /// some rule in it could read them. See [`Rule::reads_ancestor_siblings`].
    /// Decided once per build over every rule, the components' included,
    /// since a component's rules match through its caller's ancestors. True
    /// until a build says otherwise, so a path that builds without deciding
    /// behaves as it always did.
    static KEEP_SIBLINGS: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// The environment `env()` resolves against for the rest of this build.
fn set_build_env(env: Environment) {
    BUILD_ENV.with(|e| e.set(env));
}

/// Substitute every `env(<name>[, fallback])` in `value`.
///
/// The only names are the four safe-area insets, which is what CSS's own
/// `env()` is overwhelmingly used for: the strip along an edge of a phone's
/// display that a notch, a rounded corner, a status bar or a home indicator
/// has already taken. On a desktop window every one of them is zero, which is
/// the honest answer rather than a placeholder: a window that owns its whole
/// surface has no unsafe edges.
///
/// **This is the surface that makes the inset mean anything.** The value has
/// been carried in [`Environment`] since the struct landed and nothing could
/// read it, so filling it in changed nothing that anyone could see.
///
/// An unknown name with a fallback takes the fallback, as in CSS. An unknown
/// name without one leaves the reference in place, which makes the declaration
/// unparseable and so ignored: CSS's "invalid at computed-value time", and the
/// same shape [`resolve_vars`] already uses, warning rather than dropping it
/// in silence.
fn resolve_env(value: &str) -> String {
    let env = BUILD_ENV.with(std::cell::Cell::get);
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = find_call(rest, "env(") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 4..];
        let Some(end) = closing_paren(after) else {
            // Unclosed, emit the rest verbatim rather than looping.
            out.push_str("env(");
            out.push_str(after);
            return out;
        };
        let inner = &after[..end];
        let (name, fallback) = match inner.split_once(',') {
            Some((n, f)) => (n.trim(), Some(f.trim())),
            None => (inner.trim(), None),
        };
        match safe_area_inset(name, env.safe_area) {
            Some(px) => out.push_str(&format!("{px}px")),
            None => match fallback {
                Some(f) => out.push_str(f),
                None => {
                    warn_unknown_env(name);
                    out.push_str("env(");
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

/// The four names, and nothing else. `safe-area-inset-*` only: the other
/// `env()` names in CSS (`titlebar-area-*`, `viewport-segment-*`) describe
/// surfaces Rux does not have, and answering for one would be inventing a
/// number rather than reporting one.
fn safe_area_inset(name: &str, insets: Insets) -> Option<f32> {
    match name {
        "safe-area-inset-top" => Some(insets.top),
        "safe-area-inset-right" => Some(insets.right),
        "safe-area-inset-bottom" => Some(insets.bottom),
        "safe-area-inset-left" => Some(insets.left),
        _ => None,
    }
}

/// Where a function call named `name` starts, ignoring a match that is really
/// the tail of a longer identifier.
///
/// `env(` inside `--my-env(` is not a call to `env()`, and a custom property
/// may legally be called anything at all.
fn find_call(haystack: &str, name: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(hit) = haystack[from..].find(name) {
        let at = from + hit;
        let before = haystack[..at].chars().next_back();
        if !before.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            return Some(at);
        }
        from = at + name.len();
    }
    None
}

/// The index of the `)` that closes a call whose `(` has already been passed,
/// allowing nested parens in a fallback (`env(x, calc(1px + 2px))`).
fn closing_paren(after: &str) -> Option<usize> {
    let mut depth = 1i32;
    for (i, c) in after.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Warn once per name that an `env()` named something Rux cannot answer.
fn warn_unknown_env(name: &str) {
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let message = format!(
        "`env({name})` is not an environment value Rux knows, so the declaration using it is \
         ignored. The four it knows are `safe-area-inset-top`, `-right`, `-bottom` and \
         `-left`, and any name at all takes a fallback: `env({name}, 0px)`"
    );
    warn(message.clone());
    let seen = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
    let Ok(mut seen) = seen.lock() else { return };
    if seen.insert(name.to_string()) {
        echo(&message);
    }
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

/// A slider's thumb diameter and bar thickness, and the colour of the bar
/// past the thumb.
const SLIDER_THUMB: f32 = 20.0;
const SLIDER_BAR: f32 = 4.0;
const SLIDER_REST: Rgba = Rgba::new(0.82, 0.84, 0.86, 1.0); // #d1d5db

/// A slider's `min`, `max` and `step`, with HTML's defaults for a range:
/// 0, 100 and 1. `step="any"` is continuous.
struct SliderRange {
    min: f64,
    max: f64,
    /// Zero for `any`.
    step: f64,
    /// Decimal places the step is written with, which is what the value is
    /// rounded to, so ten steps of `0.1` read `1` and not `0.9999999999999999`.
    places: i32,
}

impl SliderRange {
    fn of(el: &Element) -> Self {
        let number = |name: &str| el.attr(name).and_then(|v| v.trim().parse::<f64>().ok());
        let min = number("min").unwrap_or(0.0);
        let max = number("max").filter(|m| *m > min).unwrap_or(min.max(0.0) + 100.0);
        let (step, places) = match el.attr("step").map(str::trim) {
            Some("any") => (0.0, 12),
            Some(v) => match v.parse::<f64>() {
                Ok(s) if s > 0.0 => (s, v.split_once('.').map_or(0, |(_, f)| f.len() as i32)),
                _ => (1.0, 0),
            },
            None => (1.0, 0),
        };
        Self { min, max, step, places }
    }

    /// How far along the track `value` sits, 0 to 1.
    fn fraction(&self, value: f64) -> f32 {
        ((value - self.min) / (self.max - self.min)).clamp(0.0, 1.0) as f32
    }

    /// The script that writes `model` from where the pointer is: along the
    /// track between the thumb's two resting centres, snapped to the step
    /// and kept inside the range.
    fn assignment(&self, model: &str) -> String {
        let (min, max, span) = (self.min, self.max, self.max - self.min);
        let (half, thumb) = (SLIDER_THUMB as f64 / 2.0, SLIDER_THUMB as f64);
        let scale = 10f64.powi(self.places);
        let snapped = if self.step > 0.0 {
            let step = self.step;
            format!("{min:?} + (__f * {span:?} / {step:?}).round() * {step:?}")
        } else {
            format!("{min:?} + __f * {span:?}")
        };
        format!(
            "if event.width > {thumb:?} {{ \
             let __f = (event.x - {half:?}) / (event.width - {thumb:?}); \
             if __f < 0.0 {{ __f = 0.0; }} if __f > 1.0 {{ __f = 1.0; }} \
             let __v = (({snapped}) * {scale:?}).round() / {scale:?}; \
             if __v > {max:?} {{ __v = {max:?}; }} \
             {model} = __v; }}"
        )
    }
}

/// An unstyled switch's track, off and on.
const SWITCH_OFF: Rgba = Rgba::new(0.61, 0.64, 0.69, 1.0); // #9ca3af
const SWITCH_ON: Rgba = Rgba::new(0.15, 0.39, 0.92, 1.0); // #2563eb

/// An `<input type=checkbox|radio>`: whether it is currently checked, and the
/// signals its checked state reads (so a change can reconcile just this node).
#[derive(Clone)]
struct Toggle {
    radio: bool,
    /// `type="switch"`: a checkbox in every way but how it looks and what it
    /// is announced as.
    switch: bool,
    checked: bool,
    deps: HashSet<String>,
}

impl Toggle {
    fn of(el: &Element, engine: &mut Engine, locals: &Locals) -> Option<Self> {
        if el.tag != "input" {
            return None;
        }
        let (radio, switch) = match el.attr("type") {
            Some("radio") => (true, false),
            Some("checkbox") => (false, false),
            Some("switch") => (false, true),
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
        Some(Self { radio, switch, checked, deps })
    }
}

/// Build the styled layout tree from a parsed SFC. `components` maps a custom
/// element tag to the imported component's source; those are compiled and
/// expanded in place with their props bound. `{{ }}` and directive expressions
/// evaluate against the script engine's current state.
pub fn build_styled_tree(
    sfc: &Sfc,
    components: &HashMap<String, Sfc>,
    namespaces: &HashMap<String, Namespace>,
    engine: &mut Engine,
) -> Result<LayoutNode, String> {
    let mut instances = Instances::new();
    let mut swaps = Swaps::new();
    build_styled_tree_tracked(sfc, components, namespaces, engine, &mut instances, &mut swaps)
        .map(|(node, _)| node)
}

/// Recompute a text binding's string against the engine's current state, what
/// the runtime writes into the node at `binding.path` when a dependency changes.
pub fn eval_text_binding(binding: &TextBinding, engine: &mut Engine) -> String {
    let text = interpolate_tracked(&binding.template, engine, &binding.locals).0;
    match binding.white_space {
        Some(mode) => white_space::collapse(&text, mode),
        None => text,
    }
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
/// The declarations of an inline `style`, in the order written.
fn inline_decls(css: &str, out: &mut Vec<(String, String)>) {
    for decl in css.split(';') {
        if let Some((name, value)) = decl.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if !name.is_empty() && !value.is_empty() {
                out.push((name, value.to_string()));
            }
        }
    }
}

/// An element's `direction` and whether it is `dir="auto"`: the property
/// first, as CSS would have it (`dir` is a presentational hint an author's
/// rule overrides), then the attribute, then the parent's.
fn resolve_direction(el: &Element, props: &HashMap<String, String>, inherited: rux_layout::Direction) -> (rux_layout::Direction, bool) {
    if let Some(d) = props.get("direction").and_then(|v| rux_layout::Direction::parse(v)) {
        return (d, false);
    }
    match el.attr("dir").map(str::trim) {
        Some("auto") => (inherited, true),
        Some(v) => (rux_layout::Direction::parse(v).unwrap_or(inherited), false),
        None => (inherited, false),
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
        "link" => AccessRole::Link,
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
    // A disabled target is still a target, so a second element with its id
    // cannot take the label over, but it hands the label nothing to do.
    if let Some(id) = &node.id {
        targets.entry(id.clone()).or_insert_with(|| {
            if node.field().disabled {
                (None, None)
            } else {
                (node.on_tap.clone(), node.model.clone())
            }
        });
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
        // **The placeholder is not masked.** It is not the person's text, it is
        // the author's label, and a row of bullets where "Password" should be
        // reads as a field that already has something in it.
        (binding.placeholder.clone(), binding.placeholder_color)
    } else if binding.secret {
        (mask(&value), binding.color)
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
    namespaces: &HashMap<String, Namespace>,
    engine: &mut Engine,
    instances: &mut Instances,
    swaps: &mut Swaps,
) -> Result<(LayoutNode, BindingRegistry), String> {
    build_styled_tree_stateful(
        sfc,
        components,
        namespaces,
        engine,
        instances,
        swaps,
        &InteractionState::default(),
        Environment::sane(),
    )
}

/// Like [`build_styled_tree_tracked`], but matches pseudo-class selectors against
/// the shell's current [`InteractionState`], what is hovered, pressed, focused,
/// and `@media` queries against the current [`Environment`]. The runtime passes its
/// live state on every build so a reconcile reproduces the same styling.
pub fn build_styled_tree_stateful(
    sfc: &Sfc,
    components: &HashMap<String, Sfc>,
    // Which tags each file may write. See [`Namespace`]: a tag is a local name,
    // and two files may use the same one for different components.
    namespaces: &HashMap<String, Namespace>,
    engine: &mut Engine,
    instances: &mut Instances,
    swaps: &mut Swaps,
    state: &InteractionState,
    env: Environment,
) -> Result<(LayoutNode, BindingRegistry), String> {
    // The document's own `<style>` knows where it is in its file, so its
    // warnings get a line. A component's does not: its rules live in a
    // *different* file, and every consumer of a warning attributes it to the
    // document being built, so a line from the component's coordinate space
    // would point confidently at the wrong place. Unplaced is the honest answer
    // until warnings carry a file as well as a line.
    let rules = rux_script::profile::time(rux_script::profile::Phase::Css, || parse_document_rules(sfc, env));

    // What `env()` reads, for the length of this build. Set here rather than
    // threaded, beside the two other build-scoped registers below.
    set_build_env(env);
    register_properties(sfc);
    forget_interpreted();

    // What a `to=` is checked against. Refreshed per build rather than per load
    // because hot reload rewrites the routes as readily as anything else, and a
    // stale table would report a link to a route that now exists.
    ROUTER.with(|r| *r.borrow_mut() = find_router(&sfc.template).cloned());

    // Whose tags are whose, for the length of this build. The walk starts in the
    // document's own namespace and `expand_component` switches into each
    // component's as it enters one.
    NAMESPACES.with(|n| *n.borrow_mut() = namespaces.clone());
    CURRENT_FILE.with(|c| *c.borrow_mut() = DOCUMENT_NAMESPACE.to_string());

    // The document's stylesheet reaches the components it uses. Before this, a
    // component saw only its own `<style>`, so a shared look had to be
    // `<style src="theme.css">` in every single component file, and that is the
    // wall people hit first on a real app.
    //
    // Two opt-outs, both spelled `<style scoped>`: on the document, "my rules
    // stay in my markup"; on a component, "I own my appearance, do not style
    // me from outside".
    //
    // Merged here, once per load, rather than per build. A component is
    // expanded once per instance per frame, so concatenating rule lists at
    // expansion time would repeat the whole stylesheet for every row of a list
    // on every keystroke.
    //
    // Document rules come first so a component's own rules win a tie, which is
    // CSS's own order: same specificity, later wins.
    let cascading: &[Rule] = if sfc.style_scoped { &[] } else { &rules };
    let comps: Components = components
        .iter()
        .map(|(tag, c)| {
            let own = rux_script::profile::time(rux_script::profile::Phase::Css, || parse_component_rules(c, env));
            let merged = if c.style_scoped {
                own
            } else {
                cascading.iter().cloned().chain(own).collect()
            };
            (
                tag.clone(),
                Component {
                    file: c.file.clone(),
                    template: c.template.clone(),
                    rules: merged,
                    script: component_statements(&c.script),
                    props: c.props.clone(),
                    prop_types: c
                        .props
                        .iter()
                        .map(|p| p.ty.as_deref().and_then(|t| rux_script::types::parse_type(t).ok()))
                        .collect(),
                    types: c
                        .types
                        .iter()
                        .filter_map(|(n, t)| Some((n.clone(), rux_script::types::parse_decl(t).ok()?)))
                        .collect(),
                },
            )
        })
        .collect();
    KEEP_SIBLINGS.with(|k| {
        k.set(
            rules.iter().any(Rule::reads_ancestor_siblings)
                || comps.values().any(|c| c.rules.iter().any(Rule::reads_ancestor_siblings)),
        )
    });
    // A kept row is compared with everything but its siblings, so a build
    // whose rules can read siblings reuses nothing. See `RowCache`.
    let sibling_step = rules.iter().chain(comps.values().flat_map(|c| c.rules.iter())).any(Rule::has_sibling_step);
    SIBLING_STEP.with(|s| s.set(sibling_step));
    if sibling_step {
        ROWS.with(|r| {
            if let Some(b) = r.borrow_mut().as_mut() {
                b.off = true;
            }
        });
    }

    // An instance lives as long as it is on screen, and until now nothing ever
    // said it had left: the only removal anywhere was the router's, so an
    // `r-if` that closed over a component kept its state forever and gave it
    // back on the way in, and the map only ever grew. A build walks the whole
    // template, so what it does not reach is what is gone.
    for instance in instances.values_mut() {
        instance.touched = false;
    }
    // Same bookkeeping for swaps, and paired here rather than at the callers so
    // that no build can forget half of it.
    swaps.begin();

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
            selection: SelectionStyle::default(),
            font_size: DEFAULT_FONT_SIZE,
            font_family: None,
            vars: Vars::default(),
            run: None,
            direction: rux_layout::Direction::Ltr,
            text: TextInherit::default(),
            shadows: None,
            look: None,
        },
        engine,
        &locals,
        &[],
        &[],
        &mut reg,
        state,
        instances,
        swaps,
        None, // the root is not inside a component
        None, // the document root has no caller, so no slot content
        None, // and no route chain: the router itself starts one
        None, // and is not inside any row
        None, // and the root never swaps: there is no build without it
    );
    link_labels(&mut node);
    instances.retain(|key, instance| {
        if !instance.touched {
            // Its state goes with it: this is the last moment it exists, and an
            // `unmounted` body that could not read what the instance was holding
            // could not save it either, which is most of what the hook is for.
            let mut locals = std::mem::take(&mut instance.state);
            locals.extend(instance.props.iter().cloned());
            UNMOUNTS.with(|u| {
                u.borrow_mut().push(Lifecycle {
                    key: key.clone(),
                    tag: instance.tag.clone(),
                    locals,
                })
            });
        }
        instance.touched
    });
    // After the instance sweep: an instance pruned by this build has already
    // had its `unmounted` queued, and a swap holding an element on screen kept
    // that element's instance `touched`, which is exactly why the pair is
    // builder-owned rather than a snapshot the animator holds.
    swaps.end();
    // The safety net, applied last so nothing can add motion after it.
    //
    // A stylesheet that never mentions `prefers-reduced-motion` still animates,
    // and most will never mention it. v0.7 shipped a whole animation system
    // with no way to be told that the person watching it does not want one, and
    // for some people motion is a medical problem rather than a taste, so the
    // default has to be the safe one rather than the pretty one.
    //
    // Zeroing rather than removing, because `0` is already how CSS turns a
    // transition off without deleting the declaration, and it is a state every
    // consumer downstream already handles: `active` goes false, the value is
    // written straight to its target, and an `r-transition` whose length is the
    // sum of its element's durations becomes an instant swap for free.
    //
    // An author who wants motion back for a specific thing still has
    // `@media (prefers-reduced-motion: reduce)` to say so with, which is the
    // same escape hatch the web gives.
    if env.reduced_motion {
        still_the_motion(&mut node);
    }
    Ok((node, reg))
}

/// Take the time out of every transition and animation in the tree.
fn still_the_motion(node: &mut LayoutNode) {
    for transition in &mut node.style.transitions {
        transition.duration = 0.0;
        // The delay goes too. A delay with no duration is a change that waits
        // and then happens anyway, which is still something moving on a clock.
        transition.delay = 0.0;
    }
    // An animation with no time is over before it starts: the box shows its
    // own style, or its last keyframe where it is filled forwards.
    if let Some(rare) = node.style.rare.as_mut() {
        if let Some(list) = rare.animations.as_mut() {
            let mut stilled: Vec<KeyframeAnimation> = list.to_vec();
            for a in &mut stilled {
                a.duration = 0.0;
                a.delay = 0.0;
            }
            *list = stilled.into();
        }
    }
    for child in &mut node.children {
        still_the_motion(child);
    }
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
        match interpolation_end(after) {
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

/// The expression of each `{{ }}` in `text`, trimmed, in order: what
/// [`interpolate_tracked`] evaluates, for the type checker to read.
pub fn interpolations(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        match interpolation_end(after) {
            Some(end) => {
                out.push(after[..end].trim());
                rest = &after[end + 2..];
            }
            None => rest = after,
        }
    }
    out
}

/// Where the `}}` closing an interpolation starts, given the text just after its
/// `{{`. Braces are counted, and strings skipped, so a map inside it,
/// `{{ {a: {b: 1}}.a.b }}`, does not end at the map's own `}}`.
fn interpolation_end(after: &str) -> Option<usize> {
    let b = after.as_bytes();
    let mut depth = 0usize;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            q @ (b'"' | b'\'' | b'`') => {
                i += 1;
                while i < b.len() && b[i] != q {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'{' => depth += 1,
            b'}' if depth > 0 => depth -= 1,
            b'}' if b.get(i + 1) == Some(&b'}') => return Some(i),
            _ => (),
        }
        i += 1;
    }
    None
}

/// The raw concatenated text of an element's direct text children, `{{ }}` spans
/// The line a `{{ }}` failure inside this element should be reported on.
///
/// The first text child that interpolates, since that is what can fail. An
/// element with several text runs reports them all against the first one that
/// could go wrong, which is a small imprecision compared with reporting them
/// against the top of the file. With no interpolation anywhere, the element's
/// own line, which is what the caller would have used anyway.
fn text_line(el: &Element) -> usize {
    el.children
        .iter()
        .find_map(|c| match c {
            TplNode::Text(t, line) if t.contains("{{") => Some(*line),
            _ => None,
        })
        .unwrap_or(el.line)
}

/// left intact, the template a [`TextBinding`] re-interpolates on change.
///
/// The runs exactly as written, spaces included: the parser keeps them inside
/// a `<text>`, and [`white_space`] decides which are drawn once the `{{ }}`
/// values are in.
/// The words between two tags of a sentence, as the run the parser wrapped
/// them in (see `rux_parser::RUN_TAG`). It matches no rule, not even `*`, as
/// CSS's anonymous boxes do not: it is the sentence's own words, in the
/// sentence's own style, and takes all of it from the `<text>` around it.
fn build_run(
    el: &Element,
    inherited: &Inherited,
    engine: &mut Engine,
    locals: &Locals,
    path: &[usize],
    reg: &mut BindingRegistry,
) -> LayoutNode {
    let run = inherited.run.unwrap_or(RunInherit { underline: false, strikethrough: false });
    let up = inherited.text;
    let template = text_template(el);
    let (text, deps) =
        rux_script::located(Some(text_line(el)), || interpolate_tracked(&template, engine, locals));
    if template.contains("{{") {
        // Uncollapsed: the layout collapses it with the runs beside it.
        reg.text.push(TextBinding { path: path.to_vec(), template, locals: locals.clone(), deps, white_space: None });
    }
    let mut node = LayoutNode::text(
        rux_layout::Style::default(),
        TextContent {
            text,
            font_size: inherited.font_size,
            weight: up.weight,
            color: inherited.color,
            align: Default::default(),
            wrap: Default::default(),
            font_family: inherited.font_family.clone(),
            letter_spacing: up.letter_spacing,
            word_spacing: up.word_spacing,
            line_height: None,
            italic: up.italic,
            underline: run.underline,
            strikethrough: run.strikethrough,
            nowrap: !up.white_space.wraps(),
            caret: None,
            selection: None,
            preedit: None,
            shadows: None,
            selection_style: Default::default(),
            look: inherited.look.clone(),
            rich: None,
            white_space: up.white_space,
            direction: None,
            isolate: Default::default(),
        },
    );
    node.access = Access { role: AccessRole::None, ..Access::default() };
    node
}

/// What a sentence can hold: words, more `<text>`, and the two elements that
/// make sense as a box in a line, `<image>` and `<path>`. Anything else in a
/// `<text>` used to be dropped without a word; a `<view>` or a `<button>` in a
/// line is `inline-block`, which Rux does not have, so it is an error that
/// says so rather than a guess.
fn check_sentence_children(el: &Element) {
    for child in &el.children {
        let TplNode::Element(c) = child else { continue };
        if matches!(c.tag.as_str(), "text" | "image" | "path") || c.tag == rux_parser::RUN_TAG {
            continue;
        }
        located(Some(c.line), || {
            error(format!(
                "`<{}>` cannot be inside a `<text>`: a line of text holds words, more `<text>`, and \
                 `<image>` or `<path>` as a box in the line. Put the `<text>` and the `<{}>` side by side \
                 in a `<view>` instead",
                c.tag, c.tag
            ))
        });
    }
}

/// A text's words as a screen reader should hear them: its own, or for a
/// sentence all of its runs', in order and with their spaces collapsed.
fn sentence_words(node: &LayoutNode) -> String {
    fn gather(node: &LayoutNode, out: &mut Vec<(String, WhiteSpace)>) {
        for c in &node.children {
            if c.hidden {
                continue;
            }
            if let Some(tc) = &c.text {
                if c.children.is_empty() {
                    out.push((tc.text.clone(), tc.white_space));
                } else {
                    gather(c, out);
                }
            }
        }
    }
    let Some(tc) = &node.text else { return String::new() };
    if node.children.is_empty() {
        return tc.text.clone();
    }
    let mut runs = Vec::new();
    gather(node, &mut runs);
    let borrowed: Vec<(&str, WhiteSpace)> = runs.iter().map(|(t, w)| (t.as_str(), *w)).collect();
    white_space::collapse_runs(&borrowed).concat()
}

fn text_template(el: &Element) -> String {
    el.children
        .iter()
        .filter_map(|c| match c {
            TplNode::Text(t, _) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

// Entity decoding for text lives in `rux_parser::decode_entities`, the parser
// applies it to attribute values as it reads them, and text goes through the same
// function, so there is one table, not two.
use rux_parser::decode_entities;

// ── Selector model ──────────────────────────────────────────────────────────

/// An attribute selector, `[name]` or `[name op "value" i]`, as CSS has them.
///
/// Rux elements take a closed set of attributes, so one naming anything else
/// (`[data-state]`) is read and simply matches nothing, as it would against a
/// page whose elements never carry it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AttrSel {
    /// Lower-cased: attribute names are not case-sensitive in HTML.
    name: String,
    op: AttrOp,
    value: String,
    /// The `i` flag, or `role`, whose values Rux has always compared that way.
    ci: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttrOp {
    /// `[a]`
    Exists,
    /// `[a=v]`
    Equals,
    /// `[a~=v]`: one of its space-separated words.
    Word,
    /// `[a|=v]`: `v`, or `v-` and more, as `[lang|=en]` takes `en-GB`.
    Dash,
    /// `[a^=v]`
    Prefix,
    /// `[a$=v]`
    Suffix,
    /// `[a*=v]`
    Contains,
}

impl AttrSel {
    /// Read the inside of `[…]`. `None` is an attribute selector that is not
    /// one, which makes the whole selector invalid, as it does in CSS.
    fn parse(inner: &str) -> Option<Self> {
        let inner = inner.trim();
        let name_end = inner
            .find(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(inner.len());
        let name = inner[..name_end].to_ascii_lowercase();
        if name.is_empty() {
            return None;
        }
        let rest = inner[name_end..].trim_start();
        if rest.is_empty() {
            return Some(Self { ci: name == "role", name, op: AttrOp::Exists, value: String::new() });
        }
        let (op, rest) = [
            ("~=", AttrOp::Word),
            ("|=", AttrOp::Dash),
            ("^=", AttrOp::Prefix),
            ("$=", AttrOp::Suffix),
            ("*=", AttrOp::Contains),
            ("=", AttrOp::Equals),
        ]
        .into_iter()
        .find_map(|(token, op)| rest.strip_prefix(token).map(|r| (op, r.trim_start())))?;
        let (value, rest) = match rest.chars().next()? {
            q @ ('"' | '\'') => {
                let close = rest[1..].find(q)? + 1;
                (rest[1..close].to_string(), &rest[close + 1..])
            }
            _ => {
                let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                (rest[..end].to_string(), &rest[end..])
            }
        };
        let ci = match rest.trim().to_ascii_lowercase().as_str() {
            "" => name == "role",
            "i" => true,
            "s" => false,
            _ => return None,
        };
        Some(Self { name, op, value, ci })
    }

    fn holds(&self, el: &ElemDesc) -> bool {
        let joined;
        let found = match self.name.as_str() {
            "id" => el.id.as_deref(),
            "role" => el.role.as_deref(),
            "class" => {
                joined = el.classes.join(" ");
                Some(joined.as_str())
            }
            name => el.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str()),
        };
        let Some(found) = found else { return false };
        let (have, want) = if self.ci {
            (found.to_lowercase(), self.value.to_lowercase())
        } else {
            (found.to_string(), self.value.clone())
        };
        // A word, a prefix, a suffix or a part that is empty matches nothing,
        // as CSS has it: `[class^=""]` is not every element with a class.
        match self.op {
            AttrOp::Exists => true,
            AttrOp::Equals => have == want,
            AttrOp::Word => !want.is_empty() && have.split_whitespace().any(|w| w == want),
            AttrOp::Dash => have == want || have.strip_prefix(&want).is_some_and(|r| r.starts_with('-')),
            AttrOp::Prefix => !want.is_empty() && have.starts_with(&want),
            AttrOp::Suffix => !want.is_empty() && have.ends_with(&want),
            AttrOp::Contains => !want.is_empty() && have.contains(&want),
        }
    }
}

/// Attributes that are on or off, which a bound `true` writes as present and
/// empty, as Vue and the DOM do; anything else bound to `true` reads `"true"`.
const BOOLEAN_ATTRIBUTES: &[&str] = &["disabled", "readonly", "required", "checked", "autofocus", "multiple", "hidden", "open"];

/// One compound selector, e.g. `view.card#main[role="section"]:hover`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    /// `[type="checkbox"]`, `[disabled]`, `[role=heading]` and the rest.
    attrs: Vec<AttrSel>,
    pseudos: Vec<Pseudo>,
    /// Ends in `::selection`. Such a compound never matches an element: it
    /// names the element's highlighted text, which [`matched_selection`]
    /// styles. Legal only on the last compound; anywhere else the selector
    /// matches nothing, as a browser drops it.
    selection: bool,
    /// Ends in `::placeholder`: as `selection`, for an input's placeholder.
    placeholder: bool,
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
    /// `:focus-visible`: focused, and in a way the person should be shown.
    /// See [`ElemStates::focus_visible`].
    FocusVisible,
    Active,
    Checked,
    Current,
    /// The side an element starts from as it enters. Held for the element's
    /// first frame only, so the frame after is an ordinary style change and
    /// `transition` animates it without knowing a swap happened.
    EnterFrom,
    /// The side an element ends at as it leaves. Held from the moment the swap
    /// opens until it commits, so it is a target the whole way.
    LeaveTo,
    /// `:disabled`, a control with `disabled` on it.
    Disabled,
    /// `:enabled`, a control without.
    Enabled,
    /// `:valid` and `:invalid`: whether the field's value passes its checks.
    /// Matched from the start, as CSS does.
    Valid,
    Invalid,
    /// `:user-valid` and `:user-invalid`: the same answer, but only once the
    /// person has left the field or tried to submit its form, so a form does
    /// not open covered in complaints about fields nobody has touched.
    UserValid,
    UserInvalid,
    /// `:required` and `:optional`, on any input.
    Required,
    Optional,
    /// `:nth-child(an+b)`, and `:first-child` as `0n+1`: the element's place
    /// among its rendered siblings, counted from 1. Read from where it is,
    /// not from [`ElemStates`]; see [`matches_compound`].
    Nth { a: i32, b: i32 },
    /// `:nth-last-child(an+b)`, and `:last-child` as `0n+1`: counted from the
    /// end, which needs [`ElemDesc::total`].
    NthLast { a: i32, b: i32 },
    /// `:only-child`.
    Only,
    /// `:not(…)`: none of these compounds matches the element.
    Not(Vec<Compound>),
    /// `:root`: the element with no parent.
    Root,
    /// `:host`, which matches nothing: there is no shadow tree for it to be
    /// the host of. That is what a browser answers in a document's own
    /// sheet, and why it does not warn: Tailwind writes `:root, :host` for
    /// its theme, and only the first half can mean anything here.
    Host,
    /// `:is(…)` and `:where(…)`: any of these selectors matches the element,
    /// each one read from it as the subject, so `.x:is(.group:hover *)` is an
    /// `.x` inside a hovered `.group`. `spec` is what it adds: its most
    /// specific argument for `:is()`, nothing for `:where()`.
    Is { alts: Vec<Alt>, spec: (u32, u32, u32) },
    Unknown(String),
}

/// One selector in an `:is()` or `:where()` list.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Alt {
    chain: Vec<Compound>,
    combs: Vec<Combinator>,
}

/// Interaction state for one element, tested by the pseudo-classes above. It is
/// not part of the element's markup: `checked` is resolved at build time from the
/// toggle's `r-model`, while `hover`/`focus`/`active` are threaded in from the
/// shell (see [`InteractionState`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ElemStates {
    pub hover: bool,
    pub focus: bool,
    /// Focused, and the focus should be shown: a text field whenever it has
    /// focus, anything else only when focus got there from the keyboard. That
    /// is the rule browsers use, and the reason a button tapped with a finger
    /// shows no ring while the same button reached with Tab does.
    pub focus_visible: bool,
    pub active: bool,
    pub checked: bool,
    /// This element's `to` names the path we are on. Resolved at build time from
    /// the `route` signal, like `checked` and unlike the pointer states.
    pub current: bool,
    /// This element is entering and this is its first frame. Resolved at build
    /// time from [`Swaps`], like `current` and unlike the pointer states.
    pub enter_from: bool,
    /// This element is leaving: the build no longer asks for it, and it is on
    /// screen only because a swap is holding it there.
    pub leave_to: bool,
    /// A `disabled` `<input>` or `<button>`. Resolved at build time from the
    /// attribute, like `checked`.
    pub disabled: bool,
    /// An `<input>` or `<button>` that is not disabled: CSS's `:enabled`
    /// matches form controls only, never a plain box.
    pub enabled: bool,
    /// Whether an input's value passes its checks. `None` for anything that
    /// is not checked at all: not an input, or a disabled or readonly one,
    /// which HTML leaves out of validation.
    pub valid: Option<bool>,
    /// The person has left this field, or tried to submit its form.
    pub user: bool,
    /// `required`, on an input; `None` on anything else.
    pub required: Option<bool>,
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
    /// The `r-key` of the row that input is in, when it is inside an `r-for`.
    /// Without it `:focus` matches every row of a list at once, since they all
    /// carry the same `r-model` text.
    pub focused_row: Option<String>,
    /// The component instance that input is in, when it is inside one. Without
    /// it `:focus` lights the same input in *every* instance of a component,
    /// since they all carry the same `r-model` text as well.
    pub focused_instance: Option<String>,
    /// Path of the focused element when it is not a text field: a `@tap` box
    /// or a select, which Tab reaches and which have no `r-model` to be known
    /// by. A text field is known by the three fields above instead.
    pub focused_path: Option<Vec<usize>>,
    /// Whether focus got where it is from the keyboard, which is when
    /// anything but a text field shows it. See [`ElemStates::focus_visible`].
    pub focus_visible: bool,
    /// Fields the person has left, as `(r-model, row, instance)`, the same
    /// three-part identity focus uses. What `:user-invalid` waits for.
    pub touched: Vec<(String, Option<String>, Option<String>)>,
    /// Forms whose submission has been tried, by tree path. Every field in
    /// one counts as touched from then on.
    pub attempted: Vec<Vec<usize>>,
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
        match self {
            Self::Hover | Self::Active => true,
            _ => self.nested(Pseudo::is_pointer_state),
        }
    }

    /// Whether a pseudo-class inside this one's arguments answers `f`.
    fn nested(&self, f: fn(&Pseudo) -> bool) -> bool {
        match self {
            Self::Not(list) => list.iter().any(|c| c.pseudos.iter().any(f)),
            Self::Is { alts, .. } => alts.iter().any(|alt| alt.chain.iter().any(|c| c.pseudos.iter().any(f))),
            _ => false,
        }
    }

    /// An `:is()` that steps to a sibling inside.
    fn has_sibling_step(&self) -> bool {
        match self {
            Self::Is { alts, .. } => alts.iter().any(|alt| {
                alt.combs.iter().any(|c| matches!(c, Combinator::NextSibling | Combinator::SubsequentSibling))
                    || alt.chain.iter().any(|c| c.pseudos.iter().any(Pseudo::has_sibling_step))
            }),
            Self::Not(list) => list.iter().any(|c| c.pseudos.iter().any(Pseudo::has_sibling_step)),
            _ => false,
        }
    }

    /// Reads where the element sits among its siblings, so a row cannot be
    /// reused where its place may have changed. See `Rule::has_sibling_step`.
    fn reads_place(&self) -> bool {
        match self {
            Self::Nth { .. } | Self::NthLast { .. } | Self::Only => true,
            _ => self.nested(Pseudo::reads_place),
        }
    }

    /// Counts from the end, so its siblings have to be counted first.
    fn reads_total(&self) -> bool {
        match self {
            Self::NthLast { .. } | Self::Only => true,
            _ => self.nested(Pseudo::reads_total),
        }
    }

    fn parse(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "hover" => Self::Hover,
            "focus" => Self::Focus,
            "focus-visible" => Self::FocusVisible,
            "active" => Self::Active,
            "checked" => Self::Checked,
            "current" => Self::Current,
            "enter-from" => Self::EnterFrom,
            "leave-to" => Self::LeaveTo,
            "disabled" => Self::Disabled,
            "enabled" => Self::Enabled,
            "valid" => Self::Valid,
            "invalid" => Self::Invalid,
            "user-valid" => Self::UserValid,
            "user-invalid" => Self::UserInvalid,
            "required" => Self::Required,
            "optional" => Self::Optional,
            "first-child" => Self::Nth { a: 0, b: 1 },
            "last-child" => Self::NthLast { a: 0, b: 1 },
            "only-child" => Self::Only,
            "root" => Self::Root,
            "host" => Self::Host,
            other => match name.split_once('(').and_then(|(f, arg)| Some((f.to_ascii_lowercase(), arg.strip_suffix(')')?))) {
                Some((f, arg)) if f == "nth-child" => match nth(arg) {
                    Some((a, b)) => Self::Nth { a, b },
                    None => Self::Unknown(other.to_string()),
                },
                Some((f, arg)) if f == "nth-last-child" => match nth(arg) {
                    Some((a, b)) => Self::NthLast { a, b },
                    None => Self::Unknown(other.to_string()),
                },
                // A list of compounds. A combinator inside (`:not(.a .b)`)
                // is CSS too, but not here yet, and is refused rather than
                // read as its first compound.
                Some((f, arg)) if f == "not" => {
                    let simple = |c: &str| !c.is_empty() && !c.contains(|ch: char| ch.is_whitespace() || "+>~".contains(ch));
                    let mut spec = (0, 0, 0);
                    let list = arg
                        .split(',')
                        .map(str::trim)
                        .map(|c| simple(c).then(|| parse_compound(c, &mut spec)).flatten())
                        .collect::<Option<Vec<_>>>();
                    match list {
                        Some(list) if list.iter().all(|c| *c != Compound::default()) => Self::Not(list),
                        _ => Self::Unknown(other.to_string()),
                    }
                }
                Some((f, arg)) if matches!(f.as_str(), "is" | "where" | "matches" | "-webkit-any") => {
                    // A forgiving list, as in CSS: an argument that does not
                    // parse is dropped rather than taking the rest with it.
                    let mut spec = (0, 0, 0);
                    let alts: Vec<Alt> = split_selector_list(arg)
                        .into_iter()
                        .filter_map(|alt| parse_selector(alt.trim()))
                        .map(|(chain, combs, s)| {
                            spec = spec.max(s);
                            Alt { chain, combs }
                        })
                        .collect();
                    if f == "where" {
                        spec = (0, 0, 0);
                    }
                    Self::Is { alts, spec }
                }
                _ => Self::Unknown(other.to_string()),
            },
        }
    }

    fn holds(&self, s: &ElemStates) -> bool {
        match self {
            Self::Hover => s.hover,
            Self::Focus => s.focus,
            Self::FocusVisible => s.focus_visible,
            Self::Active => s.active,
            Self::Checked => s.checked,
            Self::Current => s.current,
            Self::EnterFrom => s.enter_from,
            Self::LeaveTo => s.leave_to,
            Self::Disabled => s.disabled,
            Self::Enabled => s.enabled,
            Self::Valid => s.valid == Some(true),
            Self::Invalid => s.valid == Some(false),
            Self::UserValid => s.user && s.valid == Some(true),
            Self::UserInvalid => s.user && s.valid == Some(false),
            Self::Required => s.required == Some(true),
            Self::Optional => s.required == Some(false),
            // Asked of the element in its place, by `matches_compound`.
            Self::Nth { .. } | Self::NthLast { .. } | Self::Only | Self::Not(_) | Self::Root | Self::Is { .. } => true,
            Self::Host => false,
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
    /// A `::selection` rule: its chain picks the element, and its
    /// declarations style that element's selected text, not the element.
    selection: bool,
    /// A `::placeholder` rule, the same way: its declarations style the
    /// placeholder of the input its chain picks. See [`matched_placeholder`].
    placeholder: bool,
    /// The cascade layer it was written in, empty for none. See [`layer`].
    layer: Vec<u32>,
    /// Its declarations were the `!important` ones of what was written. A
    /// rule with both kinds is collected as two.
    important: bool,
}

impl Rule {
    /// Whether matching this rule can look at an ancestor's preceding
    /// siblings: a `+` or `~` to the left of a descendant or child step, as in
    /// `.a ~ .b .c`. Only then does the build need to keep each ancestor's
    /// siblings. `combs[i]` links `chain[i]` to `chain[i + 1]`, and matching
    /// walks right to left, so a sibling step is reached from an ancestor
    /// exactly when a descendant or child step sits to its right.
    fn reads_ancestor_siblings(&self) -> bool {
        let hop = |c: &Combinator| matches!(c, Combinator::Descendant | Combinator::Child);
        let sibling = |c: &Combinator| matches!(c, Combinator::NextSibling | Combinator::SubsequentSibling);
        self.combs
            .iter()
            .enumerate()
            .any(|(i, c)| sibling(c) && self.combs[i + 1..].iter().any(hop))
            // Inside an `:is()`, kept whenever it steps across at all: rare,
            // and keeping siblings is only ever slower, never wrong.
            || self.chain.iter().any(|c| c.pseudos.iter().any(Pseudo::has_sibling_step))
    }

    /// Whether an attribute selector anywhere in this rule asks about `name`,
    /// so the build has to evaluate a bound form of it.
    fn reads_attribute(&self, name: &str) -> bool {
        fn compound(c: &Compound, name: &str) -> bool {
            c.attrs.iter().any(|a| a.name == name)
                || c.pseudos.iter().any(|p| match p {
                    Pseudo::Not(list) => list.iter().any(|c| compound(c, name)),
                    Pseudo::Is { alts, .. } => alts.iter().any(|alt| alt.chain.iter().any(|c| compound(c, name))),
                    _ => false,
                })
        }
        self.chain.iter().any(|c| compound(c, name))
    }

    /// Whether matching this rule counts siblings from the end, so they have
    /// to be counted before they are built. See [`count_rendered`].
    fn reads_total(&self) -> bool {
        self.chain.iter().any(|c| c.pseudos.iter().any(Pseudo::reads_total))
    }

    /// Whether matching this rule looks at siblings at all, `+` or `~`
    /// anywhere in it, or an element's place among them.
    fn has_sibling_step(&self) -> bool {
        self.combs
            .iter()
            .any(|c| matches!(c, Combinator::NextSibling | Combinator::SubsequentSibling))
            || self.chain.iter().any(|c| c.pseudos.iter().any(|p| p.reads_place() || p.has_sibling_step()))
    }
}

/// One built node, as a selector can see it: what it is, and where it sits.
///
/// Recorded during the build because that is the only moment both facts are
/// known together. The cascade computes an [`ElemDesc`] for every element and
/// then drops it, and [`crate::LayoutNode`] deliberately keeps only `id` and
/// `key`, the two it needs for `for=` and reconciliation. Without this, a
/// selector run after the build has nothing to match against: the tree has
/// forgotten its own tags and classes.
#[derive(Debug, Clone)]
struct ElementEntry {
    desc: ElemDesc,
    path: Vec<usize>,
}

/// One node a selector matched: where it is, and what it is.
///
/// The identity a caller acts on is the `path`; the rest is what script can
/// read back without needing the tree itself, which it has no access to.
#[derive(Clone, Debug, PartialEq)]
pub struct ElementMatch {
    pub path: Vec<usize>,
    pub tag: String,
    pub id: Option<String>,
    pub classes: Vec<String>,
}

/// Every built node in document order, enough to run a selector over the tree.
///
/// Ancestors and preceding siblings are *not* stored per entry. They are
/// recoverable from the paths alone, since a node's ancestors are exactly the
/// entries whose path is a strict prefix of its own, and reconstructing them on
/// demand costs nothing next to storing every ancestor chain twice over.
///
/// Built on every build and dropped with the tree. A document that never calls
/// `query` pays only for the push.
#[derive(Clone, Debug, Default)]
pub struct ElementIndex {
    entries: Vec<ElementEntry>,
}

impl ElementIndex {
    fn push(&mut self, desc: ElemDesc, path: &[usize]) {
        self.entries.push(ElementEntry { desc, path: path.to_vec() });
    }

    /// Every node matching `selector`, in document order.
    ///
    /// `None` when the selector does not parse, which the caller reports as a
    /// diagnostic: a selector that cannot be read matches nothing, and silently
    /// returning an empty list is the failure mode this project keeps killing.
    pub fn query(&self, selector: &str) -> Option<Vec<ElementMatch>> {
        let (chain, combs, _) = parse_selector(selector)?;
        let last = chain.last()?;
        // Found by path, and each parent's children in order, once per
        // query. Scanning every entry for every ancestor and sibling of every
        // entry made `query(".row .name")` over two thousand rows take two
        // seconds.
        let by_path: HashMap<&[usize], usize> =
            self.entries.iter().enumerate().map(|(i, e)| (&e.path[..], i)).collect();
        // Siblings matter only to `+` and `~`, and to an element's place
        // among them (`:nth-child()`, `:last-child`).
        let siblings = combs.iter().any(|c| matches!(c, Combinator::NextSibling | Combinator::SubsequentSibling))
            || chain.iter().any(|c| c.pseudos.iter().any(|p| p.reads_place() || p.has_sibling_step()));
        // Each parent's children: their places, and what they are, shared.
        let mut children: HashMap<&[usize], (Vec<usize>, Rc<[ElemDesc]>)> = HashMap::new();
        if siblings {
            let mut kids: HashMap<&[usize], Vec<usize>> = HashMap::new();
            for (i, e) in self.entries.iter().enumerate() {
                if let Some((_, parent)) = e.path.split_last() {
                    kids.entry(parent).or_default().push(i);
                }
            }
            for (parent, mut ids) in kids {
                ids.sort_by_key(|&i| self.entries[i].path.last().copied());
                let descs: Rc<[ElemDesc]> = ids.iter().map(|&i| self.entries[i].desc.clone()).collect();
                let places = ids.iter().map(|&i| self.entries[i].path.last().copied().unwrap_or(0)).collect();
                children.insert(parent, (places, descs));
            }
        }
        let none: Rc<[ElemDesc]> = Rc::from(Vec::new());
        let before = |path: &[usize]| -> Siblings {
            let empty = || Siblings { all: Rc::clone(&none), len: 0 };
            let Some((&at, parent)) = path.split_last() else {
                return empty();
            };
            match children.get(parent) {
                Some((places, all)) => Siblings { all: Rc::clone(all), len: places.partition_point(|&n| n < at) },
                None => empty(),
            }
        };
        // An element as a selector sees it here, with how many siblings it
        // has, which the build only counted where its own rules asked.
        let described = |path: &[usize], desc: &ElemDesc| -> ElemDesc {
            let total = match path.split_last() {
                Some((_, parent)) => children.get(parent).map(|(places, _)| places.len()),
                None => Some(1),
            };
            ElemDesc { total, ..desc.clone() }
        };
        let mut out = Vec::new();
        for entry in &self.entries {
            // The element itself first: most are not what was asked for, and
            // need no context to say so.
            if !matches_compound(last, &entry.desc, None) {
                continue;
            }
            // From 0, so the root is an ancestor like any other. The root's
            // own path is empty, and starting at 1 skipped exactly it:
            // `.app .row` matched nothing from a handler while the
            // stylesheet painted it, so the two disagreed about the same
            // document. A node at the root gets `0..0` and still has no
            // ancestors, which is correct.
            let ancestors: Vec<AncNode> = (0..entry.path.len())
                .map(|depth| {
                    let at = &entry.path[..depth];
                    let prev = before(at);
                    AncNode {
                        desc: by_path.get(at).map(|&i| described(at, &self.entries[i].desc)).unwrap_or_else(ElemDesc::unknown),
                        place: prev.len() + 1,
                        child_total: None,
                        prev,
                        form: None,
                    }
                })
                .collect();
            if matches_chain(&chain, &combs, &described(&entry.path, &entry.desc), &ancestors, &before(&entry.path)) {
                out.push(ElementMatch {
                    path: entry.path.clone(),
                    tag: entry.desc.tag.clone(),
                    id: entry.desc.id.clone(),
                    classes: entry.desc.classes.clone(),
                });
            }
        }
        Some(out)
    }
}

/// The matchable identity of a template element.
#[derive(Debug, Clone, PartialEq)]
struct ElemDesc {
    tag: String,
    id: Option<String>,
    classes: Vec<String>,
    role: Option<String>,
    /// The attributes an attribute selector can ask about, as written, and
    /// the bound ones the build evaluated. Not `class`, `id` or `role`,
    /// which are above, nor a directive or a handler.
    attrs: Vec<(String, String)>,
    states: ElemStates,
    /// How many rendered siblings it has, itself included, when a rule
    /// counts from the end (`:last-child`) and so had them counted before
    /// they were built. `None` otherwise, and then such a rule fails.
    total: Option<usize>,
}

/// An ancestor in the match context: its identity plus the identities of the
/// rendered siblings that precede it. The preceding siblings are needed so a
/// sibling combinator (`+`/`~`) sitting above a descendant/child hop
/// (e.g. `.a ~ .b .c`) can still be resolved correctly.
#[derive(Debug, Clone, PartialEq)]
struct AncNode {
    desc: ElemDesc,
    prev: Siblings,
    /// Where it sits among its rendered siblings, from 1, kept even when
    /// `prev` is not. See [`matches_at`].
    place: usize,
    /// How many children it renders, counted before they are built when a
    /// rule counts from the end. See [`count_rendered`].
    child_total: Option<usize>,
    /// This ancestor's tree path, when it is a `role="form"`: the form a
    /// field below it belongs to. Only the build fills it in; a query's
    /// match context has no use for it.
    form: Option<Vec<usize>>,
}

/// The siblings before an element, in order: the first `len` of its
/// parent's children. Shared, so a query over a long list copies each
/// parent's children once rather than once per child.
#[derive(Debug, Clone, PartialEq)]
struct Siblings {
    all: Rc<[ElemDesc]>,
    len: usize,
}

impl std::ops::Deref for Siblings {
    type Target = [ElemDesc];
    fn deref(&self) -> &[ElemDesc] {
        &self.all[..self.len]
    }
}

impl From<Vec<ElemDesc>> for Siblings {
    fn from(v: Vec<ElemDesc>) -> Self {
        let len = v.len();
        Siblings { all: v.into(), len }
    }
}

/// The form a field built here belongs to: the nearest `role="form"` above.
fn enclosing_form(ancestors: &[AncNode]) -> Option<Vec<usize>> {
    ancestors.iter().rev().find_map(|a| a.form.clone())
}

/// The form a `<button type="submit">` submits, if it is one.
///
/// A `type` Rux does not know, or a submit button with no form around it, is
/// reported: either way the button would do nothing it was written to do.
fn submit_target(el: &Element, ancestors: &[AncNode]) -> Option<Vec<usize>> {
    if el.tag != "button" {
        return None;
    }
    match el.attr("type")? {
        "submit" => {
            let form = enclosing_form(ancestors);
            if form.is_none() {
                located(el.attr_line("type"), || {
                    error(
                        "`<button type=\"submit\">` has no `<view role=\"form\">` around it, \
                         so there is nothing for it to submit"
                            .to_string(),
                    )
                });
            }
            form
        }
        "button" => None,
        other => {
            located(el.attr_line("type"), || {
                error(format!(
                    "`<button type=\"{other}\">` is not a kind of button Rux has. Use \
                     `type=\"submit\"` to submit the form around it, or leave `type` off"
                ))
            });
            None
        }
    }
}

impl ElemDesc {
    /// A stand-in for a path the index has no entry for. Its empty tag matches
    /// no tag selector and it carries no id or class, so it satisfies a bare
    /// descendant hop and nothing more specific. Reachable only if a built node
    /// sits under a path no element was recorded at, which no current build
    /// produces; it exists so a gap degrades a match rather than panicking.
    fn unknown() -> Self {
        Self {
            tag: String::new(),
            id: None,
            classes: Vec::new(),
            role: None,
            attrs: Vec::new(),
            states: ElemStates::default(),
            total: None,
        }
    }

    fn of(el: &Element) -> Self {
        Self {
            tag: el.tag.clone(),
            id: el.id().map(str::to_string),
            classes: el.classes().into_iter().map(str::to_string).collect(),
            role: el.role().map(str::to_string),
            attrs: el
                .attrs
                .iter()
                .filter(|a| selectable_attribute(&a.name))
                .map(|a| (a.name.to_ascii_lowercase(), a.value.clone()))
                .collect(),
            states: ElemStates::default(),
            total: None,
        }
    }
}

/// Whether `[name]` can ask about an attribute written this way: not one
/// Rux reads into the descriptor itself, a directive, a handler or a bound
/// form, which the build evaluates (see [`bound_attributes`]).
fn selectable_attribute(name: &str) -> bool {
    !(matches!(name, "class" | "id" | "role")
        || name.starts_with("r-")
        || name.starts_with('@')
        || name.starts_with(':'))
}

/// The bound attributes of `el` some rule asks about, evaluated, with what
/// they read put into `deps` so a change restyles the element. `false` and
/// `null` leave the attribute off, as the DOM does when Vue binds them.
fn bound_attributes(
    el: &Element,
    rules: &[Rule],
    engine: &mut Engine,
    locals: &Locals,
    deps: &mut HashSet<String>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for attr in &el.attrs {
        let Some(name) = attr.name.strip_prefix(':') else { continue };
        let name = name.to_ascii_lowercase();
        if !selectable_attribute(&name) || matches!(name.as_str(), "style" | "key") {
            continue;
        }
        if !rules.iter().any(|r| r.reads_attribute(&name)) {
            continue;
        }
        let (value, read) = located(Some(attr.line), || engine.eval_value_tracked(&attr.value, locals));
        deps.extend(read);
        let text = match value {
            None | Some(Value::Null) | Some(Value::Bool(false)) => continue,
            Some(Value::Bool(true)) if BOOLEAN_ATTRIBUTES.contains(&name.as_str()) => String::new(),
            Some(v) => v.to_display(),
        };
        out.push((name, text));
    }
    out
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

/// Whether the operating system is asking for light or dark surfaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColorScheme {
    #[default]
    Light,
    Dark,
}

/// The edges of the display a phone will not let a window use: a notch, a
/// rounded corner, a home indicator, a status bar.
///
/// Zero everywhere on a desktop, which is why it can be carried before anything
/// fills it in: a window that owns its whole surface has no unsafe edges.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// Everything the operating system knows that a stylesheet can ask about.
///
/// **This is one struct on purpose, and it carries fields nothing fills in
/// yet.** Every one of these is the same question asked of the same operating
/// system and surfaced as `@media`, and every one of them has to reach
/// `Feature::holds`, which means reaching every call site that evaluates a
/// query. Adding them one at a time would mean re-opening those call sites once
/// per answer. The fields cost nothing while unanswered: an inset of zero and a
/// density of one are exactly what a desktop window reports.
///
/// It also buys something that is not an environment query at all. Once the
/// answers are data rather than measurements, they can be supplied by hand, and
/// a desktop window can be told it is a phone: a device profile is this struct
/// with a viewport, a density and safe-area insets filled in from a table. That
/// is most of what mobile development needs before an emulator is involved.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Environment {
    pub viewport: Viewport,
    /// The person has asked for less movement. An accessibility requirement
    /// rather than a preference: for some people motion is a medical problem.
    pub reduced_motion: bool,
    pub color_scheme: ColorScheme,
    /// Filled in on a phone; zero on a desktop.
    pub safe_area: Insets,
    /// Physical pixels per logical pixel. The window's scale factor, named for
    /// what a stylesheet would call it.
    pub density: f32,
    /// What the person points with, which `@media (hover: hover)` and
    /// `(pointer: fine)` ask.
    pub pointer: Pointer,
}

/// What the person points with. A mouse or a trackpad is `Fine` and can
/// hover; a finger is `Coarse` and cannot. Tailwind puts every `hover:`
/// rule inside `@media (hover: hover)`, so that a tap on a phone does not
/// leave a button looking hovered, and this is the answer it asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Pointer {
    #[default]
    Fine,
    Coarse,
}

impl Environment {
    /// An environment that is only a window size, which is what every caller
    /// that predates this struct was passing.
    pub fn from_viewport(viewport: Viewport) -> Self {
        Self { viewport, ..Self::sane() }
    }

    /// The defaults a headless build evaluates against.
    ///
    /// Not `Default::default()` because `density` must be 1.0 and not 0.0: a
    /// derived default would give a zero scale factor, and anything dividing by
    /// it would produce infinity rather than an obviously wrong number.
    pub fn sane() -> Self {
        Self::HEADLESS
    }

    /// The same thing as a constant, for the places that need one: a
    /// `thread_local!` initialiser cannot call a function.
    ///
    /// The viewport is spelled out rather than derived, and it must stay the
    /// same numbers as `Viewport`'s own `Default`: a headless build evaluates
    /// `@media` the way the default window would, and writing a zero viewport
    /// here made every `max-width` query match, which is every narrow layout
    /// applying in every test that never mentioned a viewport.
    pub const HEADLESS: Self = Self {
        viewport: Viewport { width: 1280.0, height: 800.0 },
        reduced_motion: false,
        color_scheme: ColorScheme::Light,
        safe_area: Insets { top: 0.0, right: 0.0, bottom: 0.0, left: 0.0 },
        density: 1.0,
        pointer: Pointer::Fine,
    };
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
    /// `prefers-reduced-motion: reduce`, or its `no-preference` half.
    ReducedMotion(bool),
    /// `prefers-color-scheme: dark`, or its `light` half.
    Scheme(ColorScheme),
    /// `resolution`, in device pixels per CSS pixel.
    ///
    /// The environment has carried `density` since the struct was built, and
    /// this is the query that finally asks it. Compared in `dppx` whatever the
    /// author wrote, because a ratio is what the value means and `dpi` is that
    /// ratio times a constant.
    Resolution(Cmp, f32),
    /// `hover: hover`, or `hover: none` with `false`.
    Hover(bool),
    /// `pointer: fine` or `pointer: coarse`.
    PointerIs(Pointer),
    /// A media *type* we're always in (`screen`, `all`).
    Always,
    /// Unsupported, never matches.
    Never,
}

impl Feature {
    fn holds(&self, env: Environment) -> bool {
        let vp = env.viewport;
        match *self {
            Self::Width(cmp, v) => cmp.holds(vp.width, v),
            Self::Height(cmp, v) => cmp.holds(vp.height, v),
            Self::Portrait => vp.height >= vp.width,
            Self::Landscape => vp.width > vp.height,
            Self::Resolution(cmp, v) => cmp.holds(env.density, v),
            Self::ReducedMotion(want) => env.reduced_motion == want,
            Self::Scheme(want) => env.color_scheme == want,
            Self::Hover(want) => (env.pointer == Pointer::Fine) == want,
            Self::PointerIs(want) => env.pointer == want,
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
    fn holds(&self, env: Environment) -> bool {
        self.any.iter().any(|all| all.iter().all(|f| f.holds(env)))
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
        // A boolean feature: true unless its value would be `none`.
        return vec![match body.to_ascii_lowercase().as_str() {
            "hover" | "any-hover" => Feature::Hover(true),
            "pointer" | "any-pointer" => Feature::Always,
            _ => {
                warn_unsupported_media(body);
                Feature::Never
            }
        }];
    };
    let name = name.trim().to_ascii_lowercase();
    let value = value.trim();
    vec![match name.as_str() {
        "orientation" => match value.to_ascii_lowercase().as_str() {
            "portrait" => Feature::Portrait,
            "landscape" => Feature::Landscape,
            _ => Feature::Never,
        },
        // Both halves of each preference, not just the interesting one. CSS
        // lets an author write the `no-preference` or `light` side and expect
        // it to answer, and a query that silently never matches is the failure
        // this whole fail-closed design is trying to make visible elsewhere.
        "prefers-reduced-motion" => match value.to_ascii_lowercase().as_str() {
            "reduce" => Feature::ReducedMotion(true),
            "no-preference" => Feature::ReducedMotion(false),
            _ => Feature::Never,
        },
        // There is one pointer, so `any-` asks the same as the plain one.
        "hover" | "any-hover" => match value.to_ascii_lowercase().as_str() {
            "hover" => Feature::Hover(true),
            "none" => Feature::Hover(false),
            _ => Feature::Never,
        },
        "pointer" | "any-pointer" => match value.to_ascii_lowercase().as_str() {
            "fine" => Feature::PointerIs(Pointer::Fine),
            "coarse" => Feature::PointerIs(Pointer::Coarse),
            _ => Feature::Never,
        },
        "prefers-color-scheme" => match value.to_ascii_lowercase().as_str() {
            "dark" => Feature::Scheme(ColorScheme::Dark),
            "light" => Feature::Scheme(ColorScheme::Light),
            _ => Feature::Never,
        },
        // `resolution` is how CSS asks how dense the screen is, and the answer
        // is what an author reaches for to pick heavier artwork on a phone.
        // Prefixed forms are deliberately not accepted: `-webkit-` spellings
        // are what a stylesheet carries for browsers that needed them, and Rux
        // is not one.
        "resolution" | "min-resolution" | "max-resolution" => {
            let Some(dppx) = parse_resolution(value) else {
                warn_unsupported_media(&format!("{name}: {value}"));
                return vec![Feature::Never];
            };
            match name.as_str() {
                "min-resolution" => Feature::Resolution(Cmp::Ge, dppx),
                "max-resolution" => Feature::Resolution(Cmp::Le, dppx),
                _ => Feature::Resolution(Cmp::Eq, dppx),
            }
        }
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
        // **`min-resolution: 2dppx` arrives here, not at the `name: value`
        // path.** The stylesheet is parsed and re-serialized by lightningcss,
        // which normalizes the prefixed forms into modern range syntax, so what
        // this sees is `resolution >= 2dppx`. Written where it is answered
        // rather than where it was authored, and its value is a ratio rather
        // than a length, so it is read before the `parse_px` below.
        if axis == "resolution" {
            return match parse_resolution(value) {
                Some(dppx) => Feature::Resolution(cmp, dppx),
                None => {
                    warn_unsupported_media(&format!("resolution: {value}"));
                    Feature::Never
                }
            };
        }
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
            // Every axis name has to be listed here, not just the length ones:
            // an unlisted name is read as the *value* and the value as the
            // axis, so `resolution >= 2dppx` came back complaining that
            // `resolution` is not a length. The symptom names the axis, which
            // is exactly the wrong end to go looking at.
            if left == "width" || left == "height" || left == "resolution" {
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
         (supported: screen/all, min-/max-width, min-/max-height, orientation, resolution)"
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
pub fn media_matches(css: &str, env: Environment) -> Vec<bool> {
    let Ok(sheet) = StyleSheet::parse(css, ParserOptions::default()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_media_matches(&sheet.rules.0, env, &mut out);
    out
}

fn collect_media_matches(rules: &[CssRule], env: Environment, out: &mut Vec<bool>) {
    for rule in rules {
        match rule {
            CssRule::Media(media) => {
                let text = media
                    .query
                    .to_css_string(PrinterOptions::default())
                    .unwrap_or_default();
                out.push(MediaCond::parse(&text).holds(env));
                collect_media_matches(&media.rules.0, env, out);
            }
            // A `@media` inside a layer or a `@supports` is one too.
            CssRule::LayerBlock(block) => collect_media_matches(&block.rules.0, env, out),
            CssRule::Supports(sup) => collect_media_matches(&sup.rules.0, env, out),
            // And one nested in a rule.
            CssRule::Style(style) => collect_media_matches(&style.rules.0, env, out),
            _ => {}
        }
    }
}

// ── Parsing the stylesheet ──────────────────────────────────────────────────

/// `base` is the 1-based file line the `<style>` block's first character sits
/// on, used to lift lightningcss's section-relative positions onto the file's
/// own lines. `None` means "do not claim to know": see [`parse_rules_at`].
#[cfg(test)]
fn parse_rules(css: &str, env: Environment) -> Vec<Rule> {
    parse_rules_at(css, env, None, &position_try_rules(css, env), &mut layer::Layers::default())
}

/// Every `@position-try` rule in `css` by name, with its declarations as
/// written. A `position-try-fallbacks` naming one is expanded with them when
/// it is collected, so a box's style is still read from its declarations
/// alone. See [`expand_position_tries`].
fn position_try_rules(css: &str, env: Environment) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if !css.contains("@position-try") {
        return out;
    }
    if let Ok(sheet) = StyleSheet::parse(css, ParserOptions::default()) {
        gather_position_tries(&sheet.rules.0, env, &mut out);
    }
    out
}

fn gather_position_tries(rules: &[CssRule], env: Environment, out: &mut HashMap<String, String>) {
    for rule in rules {
        match rule {
            CssRule::Media(media) => {
                let text = media.query.to_css_string(PrinterOptions::default()).unwrap_or_default();
                if MediaCond::parse(&text).holds(env) {
                    gather_position_tries(&media.rules.0, env, out);
                }
            }
            CssRule::LayerBlock(block) => gather_position_tries(&block.rules.0, env, out),
            CssRule::Supports(sup) if layer::supports(&sup.condition) => {
                gather_position_tries(&sup.rules.0, env, out)
            }
            CssRule::Unknown(at) if at.name.eq_ignore_ascii_case("position-try") => {
                // Printed whole, `@position-try --name { body }`: the token
                // lists inside print only through their rule.
                let text = rule.to_css_string(PrinterOptions::default()).unwrap_or_default();
                let Some((head, body)) = text.split_once('{') else { continue };
                let name = head.trim().trim_start_matches("@position-try").trim();
                let body = body.trim().trim_end_matches('}').trim().to_string();
                if !name.starts_with("--") || name.contains(char::is_whitespace) {
                    warn(format!(
                        "`@position-try {name}` needs a name that starts with two dashes, as in `@position-try --above`"
                    ));
                    continue;
                }
                // A later rule with the same name replaces an earlier one.
                out.insert(name.to_string(), body);
            }
            _ => {}
        }
    }
}

/// Every sheet a document styles with: what it included, then its own
/// `<style>` body.
///
/// The included sheets come first so the document wins a tie, which is what
/// including a palette is for. That is source order doing the work, exactly as
/// it would if the file had been pasted in.
///
/// Included rules are parsed unplaced. They live in a different file, and every
/// consumer attributes a warning to the document being built, so a line number
/// from the include's coordinate space would point confidently at the wrong
/// place. Same reasoning as a component's `<style>` above.
fn parse_document_rules(sfc: &Sfc, env: Environment) -> Vec<Rule> {
    let tries = sheet_position_tries(sfc, env);
    // One set of layers across the sheets: a layer an include names is the
    // same layer when the document names it, as it would be pasted in.
    let mut layers = layer::Layers::default();
    if sfc.style_includes.is_empty() {
        // The overwhelmingly common case, and it keeps the `order` values
        // exactly as they were before includes existed.
        return parse_rules_at(&sfc.style, env, Some(sfc.style_line), &tries, &mut layers);
    }
    let mut rules = Vec::new();
    for include in &sfc.style_includes {
        rules.extend(parse_rules_at(&include.css, env, None, &tries, &mut layers));
    }
    rules.extend(parse_rules_at(&sfc.style, env, Some(sfc.style_line), &tries, &mut layers));
    renumber(&mut rules);
    rules
}

/// The `@position-try` rules of a document or component and what it
/// includes, which a fallback can name across, as one sheet.
fn sheet_position_tries(sfc: &Sfc, env: Environment) -> HashMap<String, String> {
    let mut tries = HashMap::new();
    for include in &sfc.style_includes {
        tries.extend(position_try_rules(&include.css, env));
    }
    tries.extend(position_try_rules(&sfc.style, env));
    tries
}

/// What [`parse_document_rules`] reads, hashed, so rules kept from an
/// earlier parse are known to still be the document's.
fn rules_print(sfc: &Sfc) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    sfc.style.hash(&mut h);
    sfc.style_line.hash(&mut h);
    for include in &sfc.style_includes {
        include.css.hash(&mut h);
    }
    h.finish()
}

/// [`parse_document_rules`] for a component, whose own `<style>` is unplaced
/// too.
fn parse_component_rules(sfc: &Sfc, env: Environment) -> Vec<Rule> {
    let tries = sheet_position_tries(sfc, env);
    let mut layers = layer::Layers::default();
    if sfc.style_includes.is_empty() {
        return parse_rules_at(&sfc.style, env, None, &tries, &mut layers);
    }
    let mut rules = Vec::new();
    for include in &sfc.style_includes {
        rules.extend(parse_rules_at(&include.css, env, None, &tries, &mut layers));
    }
    rules.extend(parse_rules_at(&sfc.style, env, None, &tries, &mut layers));
    renumber(&mut rules);
    rules
}

/// Restate `order` across concatenated sheets.
///
/// `order` is per-sheet and breaks specificity ties, so two sheets each
/// starting at 0 would make the tie-break meaningless and let an included rule
/// beat the document's own. Renumbering makes the concatenation read as one
/// sheet, which is what it is.
fn renumber(rules: &mut [Rule]) {
    for (i, rule) in rules.iter_mut().enumerate() {
        rule.order = i;
    }
}

fn parse_rules_at(
    css: &str,
    env: Environment,
    base: Option<usize>,
    tries: &HashMap<String, String>,
    layers: &mut layer::Layers,
) -> Vec<Rule> {
    let sheet = match StyleSheet::parse(css, ParserOptions::default()) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut rules = Vec::new();
    let mut order = 0usize;
    let mut at = Collecting { env, base, css, tries, layers, layer: Vec::new(), key: Vec::new(), parent: Vec::new() };
    collect_rules(&sheet.rules.0, &mut at, &mut rules, &mut order);
    rules
}

/// What [`collect_rules`] carries down the sheet: where it is, and which
/// cascade layer the rules it is reading belong to.
struct Collecting<'a> {
    env: Environment,
    base: Option<usize>,
    css: &'a str,
    tries: &'a HashMap<String, String>,
    layers: &'a mut layer::Layers,
    /// The layer being read, by name from the outermost, empty for none.
    layer: Vec<String>,
    /// Its key in `layers`.
    key: Vec<u32>,
    /// The selectors of the rule being read inside, when a rule is nested in
    /// another, with its own parent's already put in. Empty at the top.
    parent: Vec<String>,
}

impl Collecting<'_> {
    /// `name`, written inside the current layer, as a full path.
    fn path_of(&self, name: &lightningcss::rules::layer::LayerName) -> Vec<String> {
        self.layer.iter().cloned().chain(name.0.iter().map(|part| part.to_string())).collect()
    }
}

/// Walk the rule list, descending into `@media` blocks whose condition holds at
/// `vp`. A block that doesn't hold contributes nothing, so everything
/// downstream (matching, cascade, specificity) is untouched by media queries.
/// `order` keeps counting across blocks, which is what makes a later `@media`
/// rule win over an earlier plain rule of equal specificity, as in CSS.
///
/// `@layer` and `@supports` are descended the same way. A `@layer` block
/// puts its rules in a layer; `@layer a, b;` names layers so they stack in
/// that order before any rule is in them. See [`layer`].
fn collect_rules(rules: &[CssRule], at: &mut Collecting, out: &mut Vec<Rule>, order: &mut usize) {
    for rule in rules {
        match rule {
            CssRule::Media(media) => {
                let text = media
                    .query
                    .to_css_string(PrinterOptions::default())
                    .unwrap_or_default();
                // An unsupported condition is reported against the `@media` line.
                let env = at.env;
                let holds = located(file_line(at.base, media.loc.line), || {
                    MediaCond::parse(&text).holds(env)
                });
                if holds {
                    collect_rules(&media.rules.0, at, out, order);
                }
            }
            CssRule::Style(style) => collect_style_rule(style, at, out, order),
            // Declarations after a nested rule, or inside a `@media` or
            // `@supports` nested in a rule: the parent's, for the same
            // elements, in the order they were written.
            CssRule::NestedDeclarations(nested) if !at.parent.is_empty() => {
                let selectors = at.parent.clone();
                let line = nested.loc.line;
                located(file_line(at.base, line), || {
                    let normal = rule_decls(&nested.declarations.declarations, line, at.base, at.css, at.tries);
                    let important = rule_decls(&nested.declarations.important_declarations, line, at.base, at.css, at.tries);
                    push_rules(&selectors, &normal, &important, at, out, order);
                });
            }
            CssRule::LayerStatement(statement) => {
                for name in &statement.names {
                    let path = at.path_of(name);
                    at.layers.key(&path);
                }
            }
            CssRule::LayerBlock(block) => {
                let path = match &block.name {
                    Some(name) => at.path_of(name),
                    None => {
                        let anonymous = at.layers.anonymous();
                        at.layer.iter().cloned().chain(std::iter::once(anonymous)).collect()
                    }
                };
                let key = at.layers.key(&path);
                let outer = (std::mem::replace(&mut at.layer, path), std::mem::replace(&mut at.key, key));
                collect_rules(&block.rules.0, at, out, order);
                (at.layer, at.key) = outer;
            }
            CssRule::Supports(sup) => {
                if layer::supports(&sup.condition) {
                    collect_rules(&sup.rules.0, at, out, order);
                }
            }
            // Read by passes of their own: `position_try_rules` and
            // `registered_properties`.
            CssRule::Unknown(u) if u.name.eq_ignore_ascii_case("position-try") => {}
            CssRule::Property(_) => {}
            // Read by `gather_keyframes`, and run by what names it.
            CssRule::Keyframes(_) => {}
            // Says nothing about how anything looks.
            CssRule::Ignored | CssRule::Namespace(_) => {}
            other => located(file_line(at.base, at_rule_line(other)), || warn_at_rule(other)),
        }
    }
}

/// The section-relative line an at-rule starts on, for the warning that it
/// does nothing.
fn at_rule_line(rule: &CssRule) -> u32 {
    match rule {
        CssRule::Keyframes(r) => r.loc.line,
        CssRule::FontFace(r) => r.loc.line,
        CssRule::Import(r) => r.loc.line,
        CssRule::Container(r) => r.loc.line,
        CssRule::Scope(r) => r.loc.line,
        CssRule::StartingStyle(r) => r.loc.line,
        CssRule::Page(r) => r.loc.line,
        CssRule::CounterStyle(r) => r.loc.line,
        CssRule::Unknown(r) => r.loc.line,
        _ => 0,
    }
}

/// Say that an at-rule does nothing, once per name. Its rules would
/// otherwise vanish without a word, which is what `@layer` did until it was
/// read: every rule Tailwind emits sits in one, and a sheet of them did
/// nothing at all and said nothing.
fn warn_at_rule(rule: &CssRule) {
    let text = rule.to_css_string(PrinterOptions::default()).unwrap_or_default();
    let name: String = text
        .trim_start()
        .trim_start_matches('@')
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '-')
        .collect();
    if name.is_empty() {
        return;
    }
    warn_once(format!("`@{name}` is not supported yet, so what is inside it does nothing"));
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
    at: &mut Collecting,
    out: &mut Vec<Rule>,
    order: &mut usize,
) {
    let (base, css, tries) = (at.base, at.css, at.tries);
    let line = style.loc.line;
    let selectors: Vec<String> = style
        .selectors
        .0
        .iter()
        .filter_map(|selector| selector.to_css_string(PrinterOptions::default()).ok())
        .map(|text| nest(&at.parent, &text))
        .collect();
    located(file_line(base, line), || {
        // The declarations as written, and the `!important` ones, which the
        // cascade sorts apart: one Rule each.
        let normal = rule_decls(&style.declarations.declarations, line, base, css, tries);
        let important = rule_decls(&style.declarations.important_declarations, line, base, css, tries);
        push_rules(&selectors, &normal, &important, at, out, order);
    });
    // Rules nested inside, CSS nesting's: read against these selectors.
    if !style.rules.0.is_empty() {
        let outer = std::mem::replace(&mut at.parent, selectors);
        collect_rules(&style.rules.0, at, out, order);
        at.parent = outer;
    }
}

/// A nested selector with its parent put in, as CSS nesting reads it: `&` is
/// the parent, and a selector without one is a descendant of it (`> .b` a
/// child). A parent of more than one selector, or of more than one compound,
/// stands in as `:is(…)`, which is what CSS defines `&` to be; a single
/// compound is put in as written, which means the same and keeps a
/// pseudo-element (`::placeholder`) where `:is()` could not hold one.
fn nest(parent: &[String], text: &str) -> String {
    if parent.is_empty() {
        return text.to_string();
    }
    let simple = parent.len() == 1
        && !parent[0].contains(|c: char| c.is_whitespace() || matches!(c, '>' | '+' | '~'));
    let amp = if simple { parent[0].clone() } else { format!(":is({})", parent.join(", ")) };
    let text = text.trim();
    if text.contains('&') {
        text.replace('&', &amp)
    } else {
        format!("{amp} {text}")
    }
}

/// One Rule per selector in the list, which share the declarations, and a
/// second for each when some were `!important`.
fn push_rules(
    selectors: &[String],
    normal: &[(String, String)],
    important: &[(String, String)],
    at: &Collecting,
    out: &mut Vec<Rule>,
    order: &mut usize,
) {
    for text in selectors {
        if let Some((mut chain, combs, specificity)) = parse_selector(text) {
            // The flag moves from the last compound onto the rule, so
            // the chain can match the element the highlight belongs to.
            let selection = chain.last_mut().is_some_and(|last| std::mem::take(&mut last.selection));
            let placeholder = chain.last_mut().is_some_and(|last| std::mem::take(&mut last.placeholder));
            for (decls, important) in [(normal, false), (important, true)] {
                if decls.is_empty() && important {
                    continue;
                }
                out.push(Rule {
                    chain: chain.clone(),
                    combs: combs.clone(),
                    specificity,
                    order: *order,
                    decls: decls.to_vec(),
                    selection,
                    placeholder,
                    layer: at.key.clone(),
                    important,
                });
            }
        }
        *order += 1;
    }
}

/// One rule's declarations as `(property, value)`, warning about any
/// property that does nothing.
fn rule_decls(
    props: &[lightningcss::properties::Property],
    line: u32,
    base: Option<usize>,
    css: &str,
    tries: &HashMap<String, String>,
) -> Vec<(String, String)> {
    {
        // Serialize each declaration to "prop: value" and split it.
        let mut decls = Vec::new();
        for prop in props {
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
                    let at = decl_line(css, line, &key).unwrap_or(line);
                    located(file_line(base, at), || warn_if_unhonored(&key));
                    let mut v = v.trim().trim_end_matches(';').trim().to_string();
                    if key == "position-try-fallbacks" || key == "position-try" {
                        v = expand_position_tries(&v, tries);
                    }
                    decls.push((key, v));
                }
            }
        }
        decls
    }
}

/// The CSS properties the runtime actually interprets today. Anything outside
/// this set is parsed and then dropped, so [`warn_if_unhonored`] flags it. When
/// a new property is honored in `interpret` (or the text/border helpers), add it
/// here too, or authors will be told a working property does nothing.
const HONORED_PROPERTIES: &[&str] = &[
    // Controls: the colour a switch and a slider are drawn in.
    "accent-color",
    // Box / display
    "display", "width", "height", "gap",
    "min-width", "max-width", "min-height", "max-height",
    "padding", "padding-top", "padding-right", "padding-bottom", "padding-left",
    "margin", "margin-top", "margin-right", "margin-bottom", "margin-left",
    "border", "border-width", "border-color", "border-radius",
    "border-top-left-radius", "border-top-right-radius",
    "border-bottom-right-radius", "border-bottom-left-radius",
    "border-top", "border-right", "border-bottom", "border-left",
    "border-top-color", "border-right-color", "border-bottom-color", "border-left-color",
    "border-top-width", "border-right-width", "border-bottom-width", "border-left-width",
    "border-style", "border-top-style", "border-right-style", "border-bottom-style", "border-left-style",
    "overflow", "overflow-x", "overflow-y", "opacity", "mix-blend-mode", "clip-path", "mask", "mask-image", "mask-mode", "filter", "backdrop-filter", "text-shadow", "cursor", "touch-action", "box-shadow", "transform",
    "translate", "rotate", "scale",
    "transition", "transition-property", "transition-duration", "transition-timing-function",
    "transition-delay", "transition-behavior",
    // Scrolling
    "overscroll-behavior", "overscroll-behavior-x", "overscroll-behavior-y",
    "object-fit", "object-position", "pointer-events", "scroll-behavior",
    "scrollbar-width", "scrollbar-color",
    "outline", "outline-color", "outline-offset", "outline-style", "outline-width",
    // Flex / grid
    "flex", "flex-grow", "flex-shrink", "flex-basis", "flex-wrap", "flex-direction",
    "justify-content", "align-items", "align-self", "justify-self", "justify-items",
    "align-content", "row-gap", "column-gap", "place-items", "place-content", "place-self",
    "grid-template-columns", "grid-template-rows", "grid-template-areas", "grid-template",
    "grid-area", "grid-column", "grid-row",
    "grid-column-start", "grid-column-end", "grid-row-start", "grid-row-end",
    "grid-auto-flow", "grid-auto-rows", "grid-auto-columns",
    // Positioning
    "position", "top", "right", "bottom", "left", "inset", "aspect-ratio", "box-sizing", "z-index", "isolation",
    "anchor-name", "position-anchor",
    "position-try", "position-try-fallbacks", "position-try-order",
    "content-visibility", "contain-intrinsic-size", "contain-intrinsic-width",
    "contain-intrinsic-height", "contain-intrinsic-inline-size", "contain-intrinsic-block-size",
    // Logical sides, put on a physical side by `direction` (see `logical`).
    "margin-inline", "margin-inline-start", "margin-inline-end",
    "margin-block", "margin-block-start", "margin-block-end",
    "padding-inline", "padding-inline-start", "padding-inline-end",
    "padding-block", "padding-block-start", "padding-block-end",
    "inset-inline", "inset-inline-start", "inset-inline-end",
    "inset-block", "inset-block-start", "inset-block-end",
    // Background
    "background", "background-color", "background-image",
    "background-size", "background-position", "background-position-x", "background-position-y",
    "background-repeat", "background-attachment", "background-clip", "background-origin",
    // Text
    "color", "font", "font-size", "font-weight", "font-family", "font-style", "text-align",
    "letter-spacing", "word-spacing", "line-height", "white-space",
    "text-decoration", "text-decoration-line",
    "text-decoration-color", "text-decoration-style", "text-decoration-thickness",
    "text-underline-offset", "text-transform", "text-indent", "text-overflow", "line-clamp", "user-select", "transform-origin",
    "animation", "animation-name", "animation-duration", "animation-delay",
    "animation-timing-function", "animation-iteration-count", "animation-direction",
    "animation-fill-mode", "animation-play-state",
    "font-variant", "font-variant-caps", "font-variant-numeric", "font-variant-ligatures",
    "font-variant-position", "font-variant-east-asian", "font-feature-settings",
    "font-variation-settings", "font-stretch", "font-width", "caret-color",
    "overflow-wrap", "word-wrap", "word-break", "vertical-align", "direction",
    // Which side of `light-dark()` a colour takes.
    "color-scheme",
    // Inherited, so a hidden box's children are hidden unless one says
    // `visible`.
    "visibility",
    // <path>. Paint is CSS rather than attributes because that is where the
    // rest of an element's appearance is written, and it is what gives a path
    // `:hover`, `:class` and `transition` on the day it lands.
    "fill", "fill-rule", "stroke", "stroke-width", "stroke-linecap", "stroke-linejoin",
];

fn is_honored(property: &str) -> bool {
    HONORED_PROPERTIES.contains(&property)
}

/// Properties whose value is nothing but lengths, so every token in them can be
/// checked without tripping over a keyword, a colour or a duration.
///
/// Shorthands that mix kinds are deliberately absent. `border: 2px solid #f00`
/// and `transition: opacity 200ms ease` would each report their own non-length
/// tokens as broken, and a warning that cries wolf is worse than none.
const LENGTH_ONLY_PROPERTIES: &[&str] = &[
    "width", "height", "min-width", "max-width", "min-height", "max-height",
    "padding", "padding-top", "padding-right", "padding-bottom", "padding-left",
    "margin", "margin-top", "margin-right", "margin-bottom", "margin-left",
    "gap", "row-gap", "column-gap",
    "top", "right", "bottom", "left",
    "border-radius", "border-top-left-radius", "border-top-right-radius",
    "border-bottom-right-radius", "border-bottom-left-radius",
    "border-width", "border-top-width", "border-right-width",
    "border-bottom-width", "border-left-width",
    "letter-spacing", "word-spacing", "font-size",
];

/// Of those, the ones resolved to **plain pixels at cascade time**, where there
/// is no box to take a percentage of.
///
/// `width` and the insets become a [`Len`], which carries `Pct` and is resolved
/// during layout against a box that exists by then. These do not: they are
/// `f32` in [`Style`] by the time layout runs, so a percentage has nothing to
/// resolve against and `parse_px` returns `None` for it.
///
/// Before this list the two halves disagreed. The interpreter read these with
/// `parse_px`, which drops a percentage, while [`warn_unparseable_lengths`]
/// validated with `parse_len`, which accepts one. So `padding: 10%`,
/// `margin: 10%`, `gap: 5%` and `border-radius: 50%` were each dropped on the
/// floor and then pronounced fine, which is the exact failure shape the length
/// warning was added to remove. Found from a report that `border-radius` would
/// not take a percentage; it was four properties, not one.
///
/// Supporting percentages here means carrying a `Len` through to paint for the
/// radius and to layout for the rest. That is a feature, and it is scheduled.
/// Saying so is the patch. The radius was done first; `padding`, `margin` and
/// the gaps followed on 2026-10-13 ([`PCT_IN_LAYOUT`]).
const PX_ONLY_PROPERTIES: &[&str] = &[
    "padding", "padding-top", "padding-right", "padding-bottom", "padding-left",
    "margin", "margin-top", "margin-right", "margin-bottom", "margin-left",
    "gap", "row-gap", "column-gap",
    "border-width", "border-top-width", "border-right-width",
    "border-bottom-width", "border-left-width",
    "letter-spacing", "word-spacing", "font-size",
];

/// Of [`PX_ONLY_PROPERTIES`], those whose percentage is carried to the
/// layout as a percentage (`Style::padding_pct`, `margin_pct`, `gap_pct`)
/// and resolved there: of the containing block's width for a side, of the
/// box's own content size for a gap, as CSS has it.
const PCT_IN_LAYOUT: &[&str] = &[
    "padding", "padding-top", "padding-right", "padding-bottom", "padding-left",
    "margin", "margin-top", "margin-right", "margin-bottom", "margin-left",
    "gap", "row-gap", "column-gap",
];

/// Warn, once per property and value, that a length was written and not
/// understood.
///
/// This is the other half of the `rem` fix. Before it, a unit the runtime did
/// not honor made the whole declaration vanish with nothing said, so
/// `padding: 2rem` looked exactly like `padding` having no effect at all, and
/// the only way to find out was to try `px` and see the layout move. An
/// unhonored *property* has warned for a long time; an unhonored *value* did
/// not.
fn warn_unparseable_lengths(props: &HashMap<String, String>) {
    for (property, value) in props {
        if !LENGTH_ONLY_PROPERTIES.contains(&property.as_str()) {
            continue;
        }
        // Checked by their own parsers, which say what was wrong with them.
        if value.contains("anchor(") || value.contains("anchor-size(") {
            continue;
        }
        for token in split_top_level(value) {
            // Keywords (`auto`, `none`, `inherit`, `fit-content`) are legitimate
            // here and are not lengths.
            if token.chars().all(|c| c.is_ascii_alphabetic() || c == '-') {
                continue;
            }
            let px_only = PX_ONLY_PROPERTIES.contains(&property.as_str());
            // Checked with the same parser the property is actually read by, or
            // the check blesses values the interpreter then throws away.
            if (px_only && parse_px(token).is_some()) || (!px_only && parse_len(token).is_some()) {
                continue;
            }
            // Maths the cascade could not finish and this property cannot
            // take: it needs a box to take a percentage of, or it does not
            // parse.
            let head = token.as_bytes();
            let maths = rux_layout::calc::FUNCTIONS.iter().any(|f| {
                head.len() > f.len() && head[..f.len()].eq_ignore_ascii_case(f.as_bytes()) && head[f.len()] == b'('
            });
            if maths {
                let why = match rux_layout::calc::parse(token) {
                    Some(c) if c.needs().basis => {
                        "a percentage inside `calc()`, `min()`, `max()` or `clamp()` is worked out \
                         in `width`, `height`, their `min-` and `max-`, the insets and `flex-basis`, \
                         and not yet here"
                    }
                    _ => {
                        "it is not maths Rux can read: the units are px, rem, em, %, the viewport \
                         and font units, and plain numbers"
                    }
                };
                warn_stylesheet(format!("`{property}: {token}` is ignored: {why}."));
                continue;
            }
            // These carry a percentage through to the layout, which resolves it
            // (watchlist #6, 2026-10-13); the rest of the list still cannot.
            if PCT_IN_LAYOUT.contains(&property.as_str()) && side_length(token).is_some() {
                continue;
            }
            if px_only && token.ends_with('%') {
                // A radius has a workaround that gives exactly what the
                // percentage was reaching for, so the message names it. A
                // radius larger than the box is clamped to half the shorter
                // side, which is what `50%` means on a square, and the author
                // wanting a pill or a circle is who writes `%` here.
                //
                // There is no equivalent for `padding`, `margin` or `gap`, so
                // they get the plain sentence rather than an invented one.
                let workaround = if property.ends_with("radius") {
                    " A radius bigger than the box is clamped to it, so \
                     `9999px` is how to say \"as round as it goes\"."
                } else {
                    ""
                };
                warn_once(format!(
                    "`{property}: {token}` is ignored: Rux does not honor a percentage \
                     on `{property}` yet, because it is resolved to plain pixels before \
                     there is a box to take a percentage of. It takes px, rem or em.\
                     {workaround}"
                ));
                continue;
            }
            warn_once(format!(
                "`{property}: {token}` is not a length Rux understands, so the \
                 declaration is ignored. Use px, %, rem, em, vw or vh."
            ));
        }
    }
}

/// Warn at most once per distinct message, for the life of the process.
///
/// A stylesheet is reparsed on every tree rebuild, which is every keystroke in
/// an input, so an undeduplicated warning here would fill the overlay.
fn warn_once(message: String) {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let seen = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
    let Ok(mut seen) = seen.lock() else { return };
    if seen.insert(message.clone()) {
        warn(message);
    }
}

/// The property names [`warn_if_unhonored`] accepts, for anything outside the
/// runtime that needs to offer them: `rux vocab`, and through it the editor's
/// CSS completions. Exposing the same slice the warning consults is the point.
/// A completion list built from a second, hand-kept copy would eventually offer
/// a property the runtime then warns does nothing, which is worse than offering
/// nothing at all.
pub fn honored_properties() -> &'static [&'static str] {
    HONORED_PROPERTIES
}

/// The pseudo-classes a selector may name, for the same reason
/// [`honored_properties`] is public: the editor completes `:` in a selector
/// from this, and an unknown pseudo-class fails closed (it matches nothing),
/// so offering one that does not exist would be offering a rule that silently
/// never applies.
///
/// [`Pseudo::parse`] is the owner. `honored_pseudo_classes_all_parse` below
/// holds the two in step, which is the same arrangement `is_honored` and
/// `HONORED_PROPERTIES` have.
pub fn honored_pseudo_classes() -> &'static [&'static str] {
    HONORED_PSEUDO_CLASSES
}

const HONORED_PSEUDO_CLASSES: &[&str] =
    &[
        "hover", "focus", "focus-visible", "active", "checked", "current", "enter-from",
        "leave-to", "disabled", "enabled", "valid", "invalid", "user-valid", "user-invalid",
        "required", "optional", "first-child", "last-child", "only-child", "nth-child",
        "nth-last-child", "not", "is", "where", "root",
    ];

/// The properties `transition` can name, in the order `all` expands them.
///
/// Owned by [`rux_layout::AnimProp`], which is where the interpolation for each
/// one lives, so this is a projection of that list rather than a second copy.
/// `all` is prepended because it is legal to write and is not a variant of the
/// expansion.
pub fn animatable_properties() -> Vec<&'static str> {
    std::iter::once(AnimProp::All.name()).chain(AnimProp::EVERY.iter().map(|p| p.name())).collect()
}

/// The real CSS properties Rux knows of and does not honor yet, for the
/// generated reference page (`rux vocab --css-reference`). The same list the
/// warning reads, so the page cannot call a property pending that the
/// runtime calls unknown, or the other way round.
pub fn unimplemented_properties() -> &'static [&'static str] {
    UNIMPLEMENTED_PROPERTIES
}

/// The inset longhands, in `Style::inset` order (top, right, bottom, left).
const INSETS: [&str; 4] = ["top", "right", "bottom", "left"];

/// Real CSS properties Rux parses and does not honor.
///
/// This exists to tell two failures apart. `outline` is CSS an author has every
/// reason to expect, and the honest answer is that Rux has not built it yet;
/// `paddding` is a typo, and the honest answer is that no such property exists.
/// Before this list both produced the identical "parsed but not yet honored"
/// line, so a typo was indistinguishable from a pending feature and read as a
/// promise the roadmap had never made.
///
/// It does not have to be every property CSS defines, and deliberately is not:
/// a name in neither list is reported as unknown, which is true of the long
/// tail and useful for the typos. What belongs here is what an author actually
/// reaches for. Adding a name here is also the first half of honoring it, and
/// moving it to [`HONORED_PROPERTIES`] is the second.
const UNIMPLEMENTED_PROPERTIES: &[&str] = &[
    // Painting and compositing
    // Boxes
    "float", "clear", "order", "resize",
    // Text. Vertical writing is a layout of its own.
    "writing-mode",
    "appearance", "list-style", "list-style-type", "list-style-position",
    // Motion. A transform is flat, so it has no depth to look into.
    "perspective", "perspective-origin", "will-change",
    // Other layout models
    "columns", "column-count", "column-width", "table-layout",
    "border-collapse", "border-spacing", "content",
];

/// Levenshtein distance, for the did-you-mean below.
///
/// Two rows rather than a full matrix: the operands are property names, so this
/// runs on a warning path against a couple of hundred candidates and there is no
/// reason to allocate a grid for it.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        current[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let substitute = previous[j] + usize::from(ca != cb);
            current[j + 1] = substitute.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// The closest property name to `property`, if one is close enough to be worth
/// suggesting.
///
/// The budget scales with the length of the name written, because a fixed
/// distance of two is most of a short name and almost none of a long one:
/// `gap` must not be offered for `top`, while `bordr-radius` should still find
/// `border-radius`. An honored name beats an unimplemented one at the same
/// distance, since suggesting something that works beats suggesting something
/// else that also does nothing.
fn nearest_property(property: &str) -> Option<&'static str> {
    let budget = match property.chars().count() {
        0..=4 => 1,
        5..=8 => 2,
        _ => 3,
    };
    let mut best: Option<(usize, bool, &'static str)> = None;
    for (honored, name) in HONORED_PROPERTIES
        .iter()
        .map(|n| (true, *n))
        .chain(UNIMPLEMENTED_PROPERTIES.iter().map(|n| (false, *n)))
    {
        let distance = edit_distance(property, name);
        if distance > budget {
            continue;
        }
        // Ranked on `!honored`, so `false` (an honored name) sorts first and
        // wins a tie.
        let candidate = (distance, !honored);
        if best.is_none_or(|(d, unhonored, _)| candidate < (d, unhonored)) {
            best = Some((candidate.0, candidate.1, name));
        }
    }
    best.map(|(_, _, name)| name)
}

/// Warn, once per property name, for the life of the process, that a parsed
/// declaration is not honored. Deduped so a whole-tree rebuild (which reparses
/// every sheet) doesn't repeat the same line on every keystroke.
///
/// Three outcomes, not one. Honored says nothing; real-but-unbuilt says exactly
/// that and promises nothing beyond it; an unrecognised name says it is
/// unrecognised and offers the nearest thing that exists. The single message
/// this replaced told an author with a typo that their property was "not yet
/// honored", which sounds like a feature on its way and left them waiting for a
/// release that was never going to fix it.
fn warn_if_unhonored(property: &str) {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

    // A custom property is not a property we could fail to honor, it is storage
    // for `var()`, and any name is legal.
    if property.starts_with("--") || is_honored(property) || vendor_alias(property).is_some() {
        return;
    }
    // Another vendor's own property (`-webkit-tap-highlight-color`) belongs
    // to one engine, and the others pass over it without a word, as they do
    // its pseudo-elements. `-webkit-line-clamp` is read, by
    // [`line_clamp`], and `-webkit-box-orient` beside it says only that the
    // box is the vertical one it already is.
    if property.starts_with("-webkit-") || property.starts_with("-moz-") || property.starts_with("-ms-") {
        return;
    }
    let message = if UNIMPLEMENTED_PROPERTIES.contains(&property) {
        format!(
            "CSS property `{property}` is real CSS that Rux does not honor yet, so it \
             will have no effect"
        )
    } else {
        match nearest_property(property) {
            Some(nearest) => format!(
                "`{property}` is not a CSS property Rux knows, so it will have no effect. \
                 Did you mean `{nearest}`?"
            ),
            None => {
                format!("`{property}` is not a CSS property Rux knows, so it will have no effect")
            }
        }
    };
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
            if d == '\\' {
                // An escaped character is part of a name, even a space or a
                // `>`: `.\[\&\>\*\]` is one class.
                i += escape_len(&chars[i..]);
                continue;
            }
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
                let cls = ident(&chars, &mut i);
                if !cls.is_empty() {
                    c.classes.push(cls);
                    spec.1 += 1;
                }
            }
            '#' => {
                i += 1;
                let id = ident(&chars, &mut i);
                if !id.is_empty() {
                    c.id = Some(id);
                    spec.0 += 1;
                }
            }
            '[' => {
                // Up to the `]` that is not inside quotes.
                let mut end = i + 1;
                let mut quote = None;
                while end < chars.len() {
                    match (quote, chars[end]) {
                        (None, '"' | '\'') => quote = Some(chars[end]),
                        (Some(q), ch) if ch == q => quote = None,
                        (None, ']') => break,
                        _ => {}
                    }
                    end += 1;
                }
                let inner: String = chars.get(i + 1..end)?.iter().collect();
                let Some(sel) = AttrSel::parse(&inner) else {
                    warn_once(format!(
                        "`[{inner}]` is not an attribute selector CSS can read, so the rule `{token}` is in never applies"
                    ));
                    return None;
                };
                c.attrs.push(sel);
                spec.1 += 1;
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
                // `::selection`, the one pseudo-element Rux styles. Counted
                // like a type selector, which is where CSS puts it.
                if name.eq_ignore_ascii_case(":selection") {
                    c.selection = true;
                    spec.2 += 1;
                    continue;
                }
                if name.eq_ignore_ascii_case(":placeholder") {
                    c.placeholder = true;
                    spec.2 += 1;
                    continue;
                }
                if !name.is_empty() {
                    let pseudo = Pseudo::parse(&name);
                    if let Pseudo::Unknown(n) = &pseudo {
                        // A vendor's own (`::-webkit-inner-spin-button`,
                        // `:-moz-focusring`) belongs to one engine, and every
                        // other engine passes over it without a word. Rux is
                        // another engine. Reset sheets are full of them.
                        if !n.trim_start_matches(':').starts_with('-') {
                            warn_unknown_pseudo(n);
                        }
                    }
                    // A pseudo-class has class-level specificity, but `:not()`
                    // counts as its most specific argument, as in CSS.
                    let own = pseudo_specificity(&pseudo);
                    *spec = (spec.0 + own.0, spec.1 + own.1, spec.2 + own.2);
                    c.pseudos.push(pseudo);
                }
            }
            _ => break,
        }
    }
    Some(c)
}

/// A class or id name from `chars[*i..]`, escapes read: `md\:flex` is the
/// class `md:flex`, and `\31 0` is `10`, which is how a sheet writes a name
/// CSS would otherwise read as something else. Every Tailwind variant and
/// fraction is written this way (`.hover\:bg-x`, `.w-1\/2`, `.py-1\.5`).
fn ident(chars: &[char], i: &mut usize) -> String {
    let mut out = String::new();
    while *i < chars.len() {
        let c = chars[*i];
        if c == '\\' {
            let len = escape_len(&chars[*i..]);
            let body: String = chars[*i + 1..*i + len].iter().collect();
            let hex = body.trim_end();
            match u32::from_str_radix(hex, 16) {
                Ok(n) if !hex.is_empty() && hex.chars().all(|h| h.is_ascii_hexdigit()) => {
                    out.push(char::from_u32(n).filter(|&ch| ch != '\0').unwrap_or('\u{FFFD}'));
                }
                _ => out.extend(body.chars().next()),
            }
            *i += len;
        } else if c.is_alphanumeric() || c == '-' || c == '_' || !c.is_ascii() {
            out.push(c);
            *i += 1;
        } else {
            break;
        }
    }
    out
}

/// How many characters the escape at `chars[0]` (a backslash) takes: one
/// character after it, or up to six hex digits and one whitespace that ends
/// them.
fn escape_len(chars: &[char]) -> usize {
    let hex = chars[1..].iter().take(6).take_while(|c| c.is_ascii_hexdigit()).count();
    if hex == 0 {
        return (1 + 1).min(chars.len());
    }
    let ends_in_space = chars.get(1 + hex).is_some_and(|c| c.is_whitespace());
    1 + hex + ends_in_space as usize
}

/// What one pseudo-class adds to a selector's specificity: a class's worth,
/// except `:not()` and `:is()`, which count as their most specific argument,
/// and `:where()`, which counts nothing.
fn pseudo_specificity(p: &Pseudo) -> (u32, u32, u32) {
    match p {
        Pseudo::Not(list) => list.iter().map(specificity_of).max().unwrap_or_default(),
        Pseudo::Is { spec, .. } => *spec,
        _ => (0, 1, 0),
    }
}

/// `a, b(c, d), e` as `a`, `b(c, d)` and `e`.
fn split_selector_list(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    let mut chars = s.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// What one compound adds to a selector's specificity.
fn specificity_of(c: &Compound) -> (u32, u32, u32) {
    let pseudos = c.pseudos.iter().fold((0, 0, 0), |acc: (u32, u32, u32), p| {
        let own = pseudo_specificity(p);
        (acc.0 + own.0, acc.1 + own.1, acc.2 + own.2)
    });
    (
        c.id.is_some() as u32 + pseudos.0,
        (c.classes.len() + c.attrs.len()) as u32 + pseudos.1,
        (c.tag.is_some() as u32 + c.selection as u32 + c.placeholder as u32) + pseudos.2,
    )
}

/// `an+b` as `:nth-child()` takes it: `odd`, `even`, `3`, `2n`, `2n+1`,
/// `-n+3`, `n`. `None` for anything else.
fn nth(arg: &str) -> Option<(i32, i32)> {
    let arg: String = arg.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    match arg.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        _ => {}
    }
    let Some((a, b)) = arg.split_once('n') else { return Some((0, arg.parse().ok()?)) };
    let a = match a {
        "" | "+" => 1,
        "-" => -1,
        a => a.parse().ok()?,
    };
    let b = match b {
        "" => 0,
        b if b.starts_with('+') || b.starts_with('-') => b.parse().ok()?,
        _ => return None,
    };
    Some((a, b))
}

/// Whether the element at place `n` (from 1) is one `an+b` names: some
/// `k >= 0` with `a*k + b == n`.
fn nth_holds(a: i32, b: i32, n: i32) -> bool {
    match a {
        0 => n == b,
        _ => (n - b) % a == 0 && (n - b) / a >= 0,
    }
}

/// Whether every pseudo-class in `selector` is one Rux matches: what
/// `@supports selector(…)` asks. Read without parsing the selector for real,
/// which would warn about the very pseudo-class being tested for.
fn selector_is_supported(selector: &str) -> bool {
    let mut rest = selector;
    while let Some(at) = rest.find(':') {
        rest = &rest[at + 1..];
        if let Some(element) = rest.strip_prefix(':') {
            // A pseudo-element: `::selection` and `::placeholder` are the
            // ones Rux styles.
            let name: String = element.chars().take_while(|c| c.is_alphanumeric() || *c == '-').collect();
            if !name.eq_ignore_ascii_case("selection") && !name.eq_ignore_ascii_case("placeholder") {
                return false;
            }
            rest = element;
            continue;
        }
        let len = rest.find(|c: char| !(c.is_alphanumeric() || c == '-')).unwrap_or(rest.len());
        let mut name = rest[..len].to_string();
        if rest[len..].starts_with('(') {
            match closing_paren(&rest[len + 1..]) {
                Some(end) => name.push_str(&rest[len..len + end + 2]),
                None => return false,
            }
        }
        if matches!(Pseudo::parse(&name), Pseudo::Unknown(_)) {
            return false;
        }
    }
    true
}

/// Warn once per unknown pseudo-class. Same reasoning as `warn_if_unhonored`:
/// valid CSS that quietly does nothing is the worst failure mode we have, and
/// since an unknown pseudo now *fails closed*, the rule disappears entirely.
fn warn_unknown_pseudo(name: &str) {
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    // A second colon was kept in the name: a pseudo-element.
    let message = if let Some(element) = name.strip_prefix(':') {
        format!("pseudo-element `::{element}` is not supported, rules using it will never match (supported: ::selection, ::placeholder)")
    } else {
        format!(
            "pseudo-class `:{name}` is not supported, rules using it will never match \
             (supported: :{})",
            HONORED_PSEUDO_CLASSES.join(", :")
        )
    };
    warn(message.clone());
    let seen = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
    let Ok(mut seen) = seen.lock() else { return };
    if seen.insert(name.to_string()) {
        echo(&message);
    }
}

// ── Matching & cascade ──────────────────────────────────────────────────────

/// Whether `el` matches the compound `c`. `place` is where it sits among its
/// rendered siblings, from 1, which `:nth-child()` and `:first-child` read;
/// `None` where that is not known, and then they hold, so a caller asking
/// "could this match" is not told no.
fn matches_compound(c: &Compound, el: &ElemDesc, at: Option<Site>) -> bool {
    let place = at.map(|p| p.at);
    // From the end: a `None` place is "could this match", which holds; a
    // place with no total counted fails, since there is nothing to count
    // from.
    let from_end = |a: i32, b: i32| match (place, el.total) {
        (None, _) => true,
        (Some(n), Some(t)) => n <= t && nth_holds(a, b, (t - n + 1) as i32),
        (Some(_), None) => false,
    };
    // A highlight is not an element. See `Compound::selection`.
    if c.selection || c.placeholder {
        return false;
    }
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
    // Roles match case-insensitively (role="Heading" ~ [role="heading"]).
    if !c.attrs.iter().all(|a| a.holds(el)) {
        return false;
    }
    // Every pseudo-class in the compound must hold (`.btn:hover:active`).
    c.pseudos.iter().all(|p| match p {
        Pseudo::Nth { a, b } => place.is_none_or(|n| nth_holds(*a, *b, n as i32)),
        Pseudo::NthLast { a, b } => from_end(*a, *b),
        Pseudo::Only => place.is_none_or(|n| n == 1) && from_end(0, 1),
        Pseudo::Not(list) => !list.iter().any(|c| matches_compound(c, el, at)),
        // The root has no ancestors. Not known, it could be.
        Pseudo::Root => at.is_none_or(|p| p.ancestors.is_empty()),
        Pseudo::Is { alts, .. } => alts.iter().any(|alt| match at {
            Some(p) => matches_at(&alt.chain, &alt.combs, el, p.at, p.ancestors, p.prev),
            // Could it match: its last compound is the element.
            None => alt.chain.last().is_some_and(|last| matches_compound(last, el, None)),
        }),
        p => p.holds(&el.states),
    })
}

/// Where an element sits, for the pseudo-classes that ask: its place among
/// its rendered siblings from 1, its ancestors root first, and the siblings
/// before it.
#[derive(Clone, Copy)]
struct Site<'a> {
    at: usize,
    ancestors: &'a [AncNode],
    prev: &'a [ElemDesc],
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
    matches_at(chain, combs, el, prev.len() + 1, ancestors, prev)
}

/// [`matches_chain`] for an element at `place` among its siblings. Passed
/// down rather than read from `prev`, because an ancestor's `prev` is only
/// kept when a rule could read it (`KEEP_SIBLINGS`), and its place is wanted
/// whenever a rule says `:first-child`.
fn matches_at(
    chain: &[Compound],
    combs: &[Combinator],
    el: &ElemDesc,
    place: usize,
    ancestors: &[AncNode],
    prev: &[ElemDesc],
) -> bool {
    let Some((last, rest)) = chain.split_last() else {
        return false;
    };
    if !matches_compound(last, el, Some(Site { at: place, ancestors, prev })) {
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
            let a = &ancestors[i];
            matches_at(rest, rest_combs, &a.desc, a.place, &ancestors[..i], &a.prev)
        }),
        Combinator::Child => {
            let Some((parent, up)) = ancestors.split_last() else {
                return false;
            };
            matches_at(rest, rest_combs, &parent.desc, parent.place, up, &parent.prev)
        }
        // A sibling has as many siblings as this one, which its recorded
        // description may not carry.
        Combinator::NextSibling => {
            let Some((sib, earlier)) = prev.split_last() else {
                return false;
            };
            let sib = ElemDesc { total: el.total, ..sib.clone() };
            matches_at(rest, rest_combs, &sib, prev.len(), ancestors, earlier)
        }
        Combinator::SubsequentSibling => (0..prev.len()).rev().any(|i| {
            let sib = ElemDesc { total: el.total, ..prev[i].clone() };
            matches_at(rest, rest_combs, &sib, i + 1, ancestors, &prev[..i])
        }),
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
    // The compounds inside an `:is()` too: `.x:is(.group:hover *)` is
    // hovered through the group, so it is the group that needs a region.
    fn sensitive(chain: &[Compound], probe: &ElemDesc) -> bool {
        chain.iter().any(|compound| {
            // `:not(:hover)` is the one that holds with the pointer away, so
            // probing with it there would say no: such a compound counts.
            let own = compound.pseudos.iter().any(|p| matches!(p, Pseudo::Hover | Pseudo::Active | Pseudo::Not(_)) && p.is_pointer_state())
                && (matches_compound(compound, probe, None)
                    || compound.pseudos.iter().any(|p| matches!(p, Pseudo::Not(_)) && p.is_pointer_state()));
            own || compound.pseudos.iter().any(|p| match p {
                Pseudo::Is { alts, .. } => alts.iter().any(|alt| sensitive(&alt.chain, probe)),
                _ => false,
            })
        })
    }
    rules.iter().any(|rule| sensitive(&rule.chain, &probe))
}

/// Collect the matching rules' declarations for an element, in cascade order.
/// The rules that apply to an element, in the order they apply.
fn matched_rules<'r>(desc: &ElemDesc, ancestors: &[AncNode], prev: &[ElemDesc], rules: &'r [Rule]) -> Vec<&'r Rule> {
    let mut matched: Vec<&Rule> = rules
        .iter()
        .filter(|r| !r.selection && !r.placeholder && matches_chain(&r.chain, &r.combs, desc, ancestors, prev))
        .collect();
    matched.sort_by(|a, b| layer::cascade(a, b));
    matched
}

fn matched_props(
    desc: &ElemDesc,
    ancestors: &[AncNode],
    prev: &[ElemDesc],
    rules: &[Rule],
) -> HashMap<String, String> {
    let matched = matched_rules(desc, ancestors, prev, rules);
    let mut props: HashMap<String, String> = HashMap::new();
    for rule in matched {
        for (k, v) in &rule.decls {
            declare(&mut props, k, v);
        }
    }
    props
}

/// Put one declaration into an element's cascaded properties.
///
/// The map keeps the last value of each name, which loses the order a
/// shorthand and its longhands were written in. For `background` that
/// order matters, because the shorthand sets every longhand: written after
/// a `background-size`, it puts the size back to `auto`. So a `background`
/// takes away the longhands declared before it.
pub(crate) fn declare(props: &mut HashMap<String, String>, k: &str, v: &str) {
    // A shorthand Rux reads as the longhands it sets, put down here so they
    // take their place in the order everything else was written in.
    if let Some(longhands) = expand_shorthand(k, v) {
        for (lk, lv) in longhands {
            declare(props, lk, &lv);
        }
        return;
    }
    // A vendor's spelling of a property Rux honors is that property, as a
    // browser aliases `-webkit-backdrop-filter`.
    if let Some(unprefixed) = vendor_alias(k) {
        return declare(props, unprefixed, v);
    }
    if k == "background" {
        props.retain(|name, _| !BACKGROUND_LONGHANDS.contains(&name.as_str()));
    }
    if k == "transition" {
        props.retain(|name, _| !TRANSITION_LONGHANDS.contains(&name.as_str()));
    }
    props.insert(k.to_string(), v.to_string());
}

/// `inset` and the `place-*` pair shorthands, as their longhands. `None`
/// for anything else.
///
/// Split before `var()` is substituted, so a lone `var()` standing for
/// several values is copied to every side whole. A value written as one
/// token per side, which is what Tailwind writes (`inset: calc(var(--spacing)
/// * 0)`), is split the way it reads.
fn expand_shorthand(k: &str, v: &str) -> Option<Vec<(&'static str, String)>> {
    let parts = split_top_level(v);
    let part = |i: usize| parts.get(i).copied().unwrap_or(v).to_string();
    Some(match k {
        "inset" => {
            let (top, right, bottom, left) = match parts.len() {
                0 | 1 => (part(0), part(0), part(0), part(0)),
                2 => (part(0), part(1), part(0), part(1)),
                3 => (part(0), part(1), part(2), part(1)),
                _ => (part(0), part(1), part(2), part(3)),
            };
            vec![("top", top), ("right", right), ("bottom", bottom), ("left", left)]
        }
        "place-items" | "place-content" | "place-self" => {
            let (align, justify) = match k {
                "place-items" => ("align-items", "justify-items"),
                "place-content" => ("align-content", "justify-content"),
                _ => ("align-self", "justify-self"),
            };
            let second = if parts.len() > 1 { part(1) } else { part(0) };
            vec![(align, part(0)), (justify, second)]
        }
        "font" => return expand_font(v),
        _ => return None,
    })
}

/// The properties that inherit in CSS, of those Rux honors. What a
/// CSS-wide keyword means depends on it; see [`wide_keywords`].
const INHERITED_PROPERTIES: &[&str] = &[
    "color", "font-size", "font-weight", "font-family", "font-style", "font-variant",
    "font-variant-caps", "font-variant-numeric", "font-variant-ligatures", "font-variant-position",
    "font-variant-east-asian", "font-feature-settings", "font-variation-settings", "font-stretch",
    "font-width", "letter-spacing", "word-spacing", "line-height", "text-align", "text-indent",
    "text-transform", "text-shadow", "white-space", "word-break", "overflow-wrap", "word-wrap",
    "direction", "visibility", "cursor", "caret-color", "accent-color", "color-scheme",
    "pointer-events", "fill", "fill-rule", "stroke", "stroke-width", "stroke-linecap",
    "stroke-linejoin", "scrollbar-color",
];

/// `inherit`, `initial`, `unset`, `revert` and `revert-layer`, which every
/// property takes. Read as no declaration at all, which is what they come
/// to here: a property that inherits takes its parent's value, and one that
/// does not takes Rux's own default, which is what Rux's `revert` and
/// `initial` are, as it has no user-agent sheet apart from its defaults.
///
/// Two answers are not CSS's, and are said: `initial` on a property that
/// inherits gives the parent's value, not the property's initial one, and
/// `inherit` on one that does not inherit gives Rux's default, not the
/// parent's value.
fn wide_keywords(props: &mut HashMap<String, String>) {
    let keyword = |v: &str| matches!(v.trim().to_ascii_lowercase().as_str(), "inherit" | "initial" | "unset" | "revert" | "revert-layer");
    if !props.values().any(|v| keyword(v)) {
        return;
    }
    props.retain(|k, v| {
        if k.starts_with("--") || !keyword(v) {
            return true;
        }
        let inherits = INHERITED_PROPERTIES.contains(&k.as_str());
        match v.trim().to_ascii_lowercase().as_str() {
            "inherit" if !inherits => warn_once(format!(
                "`{k}: inherit` takes Rux's default, not the parent's value: `{k}` does not inherit, and \
                 taking it from the parent is not supported yet"
            )),
            "initial" if inherits => warn_once(format!(
                "`{k}: initial` takes the parent's value, not the initial one: `{k}` inherits, and its \
                 initial value is not known here yet"
            )),
            _ => {}
        }
        false
    });
}

/// `font: italic 600 1rem/1.5 Inter, sans-serif` as its longhands. The size
/// is the one token that has to be there; what comes before it is the style,
/// variant, weight and stretch in any order, and everything after it is the
/// family. A system font (`caption`, `menu`) is not a font Rux can name, so
/// it is left to warn as an unreadable `font`.
fn expand_font(v: &str) -> Option<Vec<(&'static str, String)>> {
    let v = v.trim();
    let lower = v.to_ascii_lowercase();
    if matches!(lower.as_str(), "inherit" | "initial" | "unset" | "revert" | "revert-layer") {
        return Some(
            ["font-style", "font-variant", "font-weight", "font-stretch", "font-size", "line-height", "font-family"]
                .into_iter()
                .map(|k| (k, v.to_string()))
                .collect(),
        );
    }
    let mut out = vec![
        ("font-style", "normal".to_string()),
        ("font-variant", "normal".to_string()),
        ("font-weight", "normal".to_string()),
        ("font-stretch", "normal".to_string()),
        ("line-height", "normal".to_string()),
    ];
    let mut rest = v;
    loop {
        let token_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let token = &rest[..token_end];
        if token.is_empty() {
            return None;
        }
        let lower = token.to_ascii_lowercase();
        let (size, line) = match token.split_once('/') {
            Some((s, l)) => (s, Some(l)),
            None => (token, None),
        };
        // A bare number is a weight; a size has a unit, or is zero.
        let is_size = size == "0"
            || (size.parse::<f32>().is_err() && (parse_px(size).is_some() || size.ends_with('%') || size.ends_with("vw") || size.ends_with("vh")))
            || ["calc(", "clamp(", "min(", "max("].iter().any(|f| size.starts_with(f))
            || matches!(size, "xx-small" | "x-small" | "small" | "medium" | "large" | "x-large" | "xx-large" | "xxx-large" | "smaller" | "larger");
        if is_size {
            out.push(("font-size", size.to_string()));
            if let Some(l) = line {
                out[4].1 = l.to_string();
            }
            let family = rest[token_end..].trim();
            if family.is_empty() {
                return None;
            }
            out.push(("font-family", family.to_string()));
            return Some(out);
        }
        match lower.as_str() {
            "normal" => {}
            "italic" | "oblique" => out[0].1 = token.to_string(),
            "small-caps" => out[1].1 = token.to_string(),
            "condensed" | "expanded" | "semi-condensed" | "semi-expanded" | "extra-condensed" | "extra-expanded"
            | "ultra-condensed" | "ultra-expanded" => out[3].1 = token.to_string(),
            _ if parse_weight(token).is_some() => out[2].1 = token.to_string(),
            _ => return None,
        }
        rest = rest[token_end..].trim_start();
    }
}

/// One `animation` as written, before its keyframes are looked up.
struct AnimationSpec {
    name: String,
    duration: f32,
    delay: f32,
    easing: Easing,
    iterations: f32,
    direction: AnimationDirection,
    fill: AnimationFill,
    paused: bool,
}

/// The element's `animation`s: the shorthand's list, with the longhands over
/// it, as `transition` is read. There is one animation per name, and each
/// other list is repeated to that length, as in CSS. `none` is no animation.
fn animation_specs(p: &HashMap<String, String>) -> Vec<AnimationSpec> {
    let list = |k: &str| p.get(k).map(|v| split_top_level_commas(v).into_iter().map(|s| s.trim().to_string()).collect::<Vec<_>>());
    let direction_of = |t: &str| match t {
        "normal" => Some(AnimationDirection::Normal),
        "reverse" => Some(AnimationDirection::Reverse),
        "alternate" => Some(AnimationDirection::Alternate),
        "alternate-reverse" => Some(AnimationDirection::AlternateReverse),
        _ => None,
    };
    let fill_of = |t: &str| match t {
        "none" => Some(AnimationFill::None),
        "forwards" => Some(AnimationFill::Forwards),
        "backwards" => Some(AnimationFill::Backwards),
        "both" => Some(AnimationFill::Both),
        _ => None,
    };
    let count_of = |t: &str| match t {
        "infinite" => Some(f32::INFINITY),
        n => n.parse::<f32>().ok().filter(|n| *n >= 0.0),
    };
    let mut specs: Vec<AnimationSpec> = Vec::new();
    for entry in list("animation").unwrap_or_default() {
        let mut spec = AnimationSpec {
            name: "none".to_string(),
            duration: 0.0,
            delay: 0.0,
            easing: Easing::EASE,
            iterations: 1.0,
            direction: AnimationDirection::Normal,
            fill: AnimationFill::None,
            paused: false,
        };
        let (mut times, mut named) = (0, false);
        for token in split_top_level(&entry) {
            let lower = token.to_ascii_lowercase();
            if let Some(ms) = parse_time(token) {
                if times == 0 { spec.duration = ms } else { spec.delay = ms }
                times += 1;
            } else if let Some(e) = parse_easing(token) {
                spec.easing = e;
            } else if let Some(n) = count_of(&lower) {
                spec.iterations = n;
            } else if let Some(d) = direction_of(&lower).filter(|_| lower != "normal" || named) {
                spec.direction = d;
            } else if let Some(f) = fill_of(&lower).filter(|_| lower != "none" || named) {
                spec.fill = f;
            } else if lower == "paused" || lower == "running" {
                spec.paused = lower == "paused";
            } else if !named {
                spec.name = token.trim_matches(|c| c == '"' || c == '\'').to_string();
                named = true;
            }
        }
        specs.push(spec);
    }
    if let Some(names) = list("animation-name") {
        let base = std::mem::take(&mut specs);
        specs = names
            .into_iter()
            .enumerate()
            .map(|(i, name)| {
                let from = base.get(i).or(base.last());
                AnimationSpec {
                    name: name.trim_matches(|c| c == '"' || c == '\'').to_string(),
                    duration: from.map_or(0.0, |s| s.duration),
                    delay: from.map_or(0.0, |s| s.delay),
                    easing: from.map_or(Easing::EASE, |s| s.easing),
                    iterations: from.map_or(1.0, |s| s.iterations),
                    direction: from.map_or_else(Default::default, |s| s.direction),
                    fill: from.map_or_else(Default::default, |s| s.fill),
                    paused: from.is_some_and(|s| s.paused),
                }
            })
            .collect();
    }
    let n = specs.len();
    let each = |k: &str| list(k).map(|l| (0..n).map(|i| l[i % l.len()].clone()).collect::<Vec<_>>());
    if let Some(l) = each("animation-duration") {
        for (s, v) in specs.iter_mut().zip(l) {
            s.duration = parse_time(&v).unwrap_or(0.0);
        }
    }
    if let Some(l) = each("animation-delay") {
        for (s, v) in specs.iter_mut().zip(l) {
            s.delay = parse_time(&v).unwrap_or(0.0);
        }
    }
    if let Some(l) = each("animation-timing-function") {
        for (s, v) in specs.iter_mut().zip(l) {
            s.easing = parse_easing(&v).unwrap_or(Easing::EASE);
        }
    }
    if let Some(l) = each("animation-iteration-count") {
        for (s, v) in specs.iter_mut().zip(l) {
            s.iterations = count_of(&v.to_ascii_lowercase()).unwrap_or(1.0);
        }
    }
    if let Some(l) = each("animation-direction") {
        for (s, v) in specs.iter_mut().zip(l) {
            s.direction = direction_of(&v.to_ascii_lowercase()).unwrap_or_default();
        }
    }
    if let Some(l) = each("animation-fill-mode") {
        for (s, v) in specs.iter_mut().zip(l) {
            s.fill = fill_of(&v.to_ascii_lowercase()).unwrap_or_default();
        }
    }
    if let Some(l) = each("animation-play-state") {
        for (s, v) in specs.iter_mut().zip(l) {
            s.paused = v.eq_ignore_ascii_case("paused");
        }
    }
    specs.retain(|s| !s.name.eq_ignore_ascii_case("none"));
    specs
}

/// A box's `animation`s with their keyframes worked out against it: each
/// keyframe's declarations over the box's own, resolved as the box's are
/// (`var()`, `em`, `currentcolor`), and read into a style. `None` when it has
/// none that name a `@keyframes`.
///
/// Its own function, not inline in [`build_node_inner`]: watchlist #41.
#[inline(never)]
fn keyframe_animations(
    props: &HashMap<String, String>,
    vars: &HashMap<String, String>,
    inherited_font_size: f32,
    inherited_color: Rgba,
    scheme: ColorScheme,
) -> Option<std::sync::Arc<[KeyframeAnimation]>> {
    let specs = animation_specs(props);
    if specs.is_empty() {
        return None;
    }
    let all = KEYFRAMES.with(|k| Rc::clone(&k.borrow()));
    let mut out = Vec::new();
    for spec in specs {
        let Some(frames) = all.get(&spec.name) else {
            warn_once(format!("`animation` names `{}`, and there is no `@keyframes {}`", spec.name, spec.name));
            continue;
        };
        let steps = frames
            .iter()
            .map(|frame| {
                let mut p = props.clone();
                let mut easing = None;
                let mut animated = Vec::new();
                for (k, v) in &frame.decls {
                    if k == "animation-timing-function" {
                        easing = parse_easing(v);
                        continue;
                    }
                    let v = resolve_env(&resolve_vars(v, vars, 0));
                    match parse_anim_prop(k) {
                        Some(prop) if prop != AnimProp::All => {
                            if !animated.contains(&prop) {
                                animated.push(prop);
                            }
                        }
                        _ if k.starts_with("--") => {}
                        _ => warn_once(format!(
                            "`{k}` in a `@keyframes` is not animated by Rux yet, so it is left as the box has it"
                        )),
                    }
                    p.insert(k.clone(), v);
                }
                wide_keywords(&mut p);
                let own_size = resolve_lengths(&mut p, inherited_font_size);
                resolve_colors(&mut p, inherited_color, scheme);
                let style = interpreted(&p);
                KeyframeStep {
                    offset: frame.offset,
                    easing,
                    color: p.get("color").and_then(|c| parse_color(c)),
                    font_size: p.contains_key("font-size").then_some(own_size),
                    transform: transform_fns(&p),
                    translate: translate_pct(&p),
                    props: animated,
                    style: Box::new(style),
                }
            })
            .collect();
        out.push(KeyframeAnimation {
            name: spec.name,
            duration: spec.duration,
            delay: spec.delay,
            easing: spec.easing,
            iterations: spec.iterations,
            direction: spec.direction,
            fill: spec.fill,
            paused: spec.paused,
            steps,
            base_transform: transform_fns(props),
            base_translate: translate_pct(props),
        });
    }
    (!out.is_empty()).then(|| out.into())
}

/// A box's transform as its functions, in the order CSS applies them: the
/// `translate`, `rotate` and `scale` properties, then the `transform` list.
/// Arguments in px, radians and plain numbers. Empty for `none`.
fn transform_fns(p: &HashMap<String, String>) -> Vec<TransformFn> {
    let value = |k: &str| p.get(k).map(|v| v.trim()).filter(|v| !v.eq_ignore_ascii_case("none"));
    let f = |name: &str, args: Vec<f32>| TransformFn { name: name.to_string(), args };
    let mut out = Vec::new();
    if let Some(v) = value("translate") {
        let parts = split_top_level(v);
        if let Some(tx) = parts.first().and_then(|s| parse_px(s)) {
            out.push(f("translate", vec![tx, parts.get(1).and_then(|s| parse_px(s)).unwrap_or(0.0)]));
        }
    }
    if let Some(angle) = value("rotate").and_then(|v| split_top_level(v).last().and_then(|s| parse_angle(s))) {
        out.push(f("rotate", vec![angle]));
    }
    if let Some(v) = value("scale") {
        let factor = |s: &str| match s.strip_suffix('%') {
            Some(pct) => pct.trim().parse::<f32>().ok().map(|n| n / 100.0),
            None => s.parse::<f32>().ok(),
        };
        let parts = split_top_level(v);
        if let Some(sx) = parts.first().and_then(|s| factor(s)) {
            out.push(f("scale", vec![sx, parts.get(1).and_then(|s| factor(s)).unwrap_or(sx)]));
        }
    }
    let mut rest = value("transform").unwrap_or("");
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().to_ascii_lowercase();
        let Some(close) = rest[open..].find(')').map(|c| c + open) else { break };
        let args: Vec<&str> = rest[open + 1..close].split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        let nums: Option<Vec<f32>> = match name.as_str() {
            "translate" | "translatex" | "translatey" => args.iter().map(|a| parse_px(a)).collect(),
            "rotate" => args.iter().map(|a| parse_angle(a)).collect(),
            "scale" | "scalex" | "scaley" => args.iter().map(|a| a.parse::<f32>().ok()).collect(),
            _ => None,
        };
        if let Some(nums) = nums.filter(|n| !n.is_empty()) {
            out.push(f(&name, nums));
        }
        rest = rest[close + 1..].trim_start();
    }
    out
}

/// The matrix of a list of transform functions, as [`transform_fns`] reads
/// them.
pub(crate) fn transform_fns_matrix(list: &[TransformFn]) -> Transform {
    let mut m = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    for f in list {
        let a = |i: usize| f.args.get(i).copied();
        let step = match f.name.as_str() {
            "translate" => [1.0, 0.0, 0.0, 1.0, a(0).unwrap_or(0.0), a(1).unwrap_or(0.0)],
            "translatex" => [1.0, 0.0, 0.0, 1.0, a(0).unwrap_or(0.0), 0.0],
            "translatey" => [1.0, 0.0, 0.0, 1.0, 0.0, a(0).unwrap_or(0.0)],
            "scale" => {
                let sx = a(0).unwrap_or(1.0);
                [sx, 0.0, 0.0, a(1).unwrap_or(sx), 0.0, 0.0]
            }
            "scalex" => [a(0).unwrap_or(1.0), 0.0, 0.0, 1.0, 0.0, 0.0],
            "scaley" => [1.0, 0.0, 0.0, a(0).unwrap_or(1.0), 0.0, 0.0],
            "rotate" => {
                let (sin, cos) = a(0).unwrap_or(0.0).sin_cos();
                [cos, sin, -sin, cos, 0.0, 0.0]
            }
            _ => continue,
        };
        m = mat_mul(m, step);
    }
    m
}

/// A translation in a percentage of the box, or maths with one in it, which
/// the matrix cannot hold: it is the box's size that a percentage is of, and
/// only the layout knows it. Read where CSS puts it first, so it can be put
/// in front of the matrix exactly: the `translate` property, which always
/// applies first, and a `translate()` leading a `transform` list with no
/// `rotate` or `scale` property before it. Anywhere else it is said.
fn translate_pct(p: &HashMap<String, String>) -> Option<[Len; 2]> {
    let lens = |x: &str, y: Option<&str>| -> Option<[Len; 2]> {
        let x = parse_len(x)?;
        let y = match y {
            Some(y) => parse_len(y)?,
            None => Len::Px(0.0),
        };
        let relative = |l: Len| matches!(l, Len::Pct(_) | Len::Calc(_));
        (relative(x) || relative(y)).then_some([x, y])
    };
    let mut out = p.get("translate").map(|v| v.trim()).filter(|v| !v.eq_ignore_ascii_case("none")).and_then(|v| {
        let parts = split_top_level(v);
        lens(parts.first()?, parts.get(1).copied())
    });
    let mut rest = p.get("transform").map(|v| v.trim()).unwrap_or("");
    let mut first = !["rotate", "scale"].iter().any(|k| p.get(*k).is_some_and(|v| !v.trim().eq_ignore_ascii_case("none")));
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().to_ascii_lowercase();
        let Some(close) = closing_paren(&rest[open + 1..]).map(|c| c + open + 1) else { break };
        let args: Vec<&str> = split_top_level_commas(&rest[open + 1..close]).into_iter().map(str::trim).collect();
        let pct = match name.as_str() {
            "translate" => args.first().and_then(|x| lens(x, args.get(1).copied())),
            "translatex" => args.first().and_then(|x| lens(x, None)),
            "translatey" => args.first().and_then(|y| lens("0", Some(y))),
            _ => None,
        };
        if let Some(pct) = pct {
            if first {
                // Two leading translations add up; a translation commutes
                // with another.
                out = Some(match out {
                    Some([x, y]) => [add_len(x, pct[0]), add_len(y, pct[1])],
                    None => pct,
                });
            } else {
                warn_once(format!(
                    "`{name}()` in a percentage after another transform is not drawn yet; put it first in the list"
                ));
            }
        }
        first = first && pct.is_some();
        rest = rest[close + 1..].trim_start();
    }
    out
}

/// Two lengths as one, where both are a percentage or both px; otherwise the
/// first, which is the one a translation was asked for in.
fn add_len(a: Len, b: Len) -> Len {
    match (a, b) {
        (Len::Pct(x), Len::Pct(y)) => Len::Pct(x + y),
        (Len::Px(x), Len::Px(y)) => Len::Px(x + y),
        (Len::Px(z), other) | (other, Len::Px(z)) if z == 0.0 => other,
        (a, _) => a,
    }
}

/// `transform-origin`: one or two positions, x then y, or keywords in either
/// order (`top left`), against the box's own size; a third value, the z
/// offset, means nothing to a flat transform and is read past. `None` for
/// what is not one, which is said.
fn parse_transform_origin(v: &str) -> Option<[Len; 2]> {
    let word = |t: &str| -> Option<(f32, Option<bool>)> {
        // The fraction, and whether the word can only be one axis: `Some(true)`
        // for x, `Some(false)` for y, `None` for `center`, which is either.
        Some(match t {
            "left" => (0.0, Some(true)),
            "right" => (1.0, Some(true)),
            "top" => (0.0, Some(false)),
            "bottom" => (1.0, Some(false)),
            "center" => (0.5, None),
            _ => return None,
        })
    };
    let parts: Vec<&str> = v.split_whitespace().collect();
    let one = |t: &str| word(t).map(|(f, _)| Len::Pct(f)).or_else(|| parse_len(t));
    let out = match parts[..] {
        [t] => match word(t) {
            Some((f, Some(false))) => Some([Len::Pct(0.5), Len::Pct(f)]),
            _ => one(t).map(|l| [l, Len::Pct(0.5)]),
        },
        [a, b, ..] => {
            // `top left` is `left top`.
            let swap = word(a).is_some_and(|(_, axis)| axis == Some(false)) || word(b).is_some_and(|(_, axis)| axis == Some(true));
            let (a, b) = if swap { (b, a) } else { (a, b) };
            one(a).zip(one(b)).map(|(x, y)| [x, y])
        }
        [] => None,
    };
    if out.is_none() {
        warn_once(format!("`transform-origin: {v}` is not a place Rux can read, so the box turns about its centre"));
    }
    out
}

/// `-webkit-backdrop-filter` as `backdrop-filter`: a vendor prefix on a
/// property Rux honors under its own name.
fn vendor_alias(k: &str) -> Option<&'static str> {
    let rest = k.strip_prefix("-webkit-").or_else(|| k.strip_prefix("-moz-"))?;
    // Not the same property: it needs `display: -webkit-box` beside it. See
    // [`line_clamp`].
    if rest == "line-clamp" {
        return None;
    }
    HONORED_PROPERTIES.iter().find(|p| **p == rest).copied()
}

/// Every longhand the `background` shorthand sets.
const BACKGROUND_LONGHANDS: &[&str] = &[
    "background-color",
    "background-image",
    "background-size",
    "background-position",
    "background-position-x",
    "background-position-y",
    "background-repeat",
    "background-clip",
    "background-origin",
    "background-attachment",
];

/// What `::selection` rules say about this element's selected text, over
/// what its parent's said.
///
/// **Inherited, both halves.** `::selection { background: gold }` on a page
/// has to reach the text inside every input on it, and a rule that stopped at
/// the element it was written on would reach none: the selected glyphs belong
/// to an input's text, several elements down. That is how browsers behaved for
/// the whole life of `::selection` before highlight inheritance, and it is
/// what authors write against.
fn matched_selection(
    desc: &ElemDesc,
    ancestors: &[AncNode],
    prev: &[ElemDesc],
    rules: &[Rule],
    inherited: SelectionStyle,
) -> SelectionStyle {
    let mut matched: Vec<&Rule> = rules
        .iter()
        .filter(|r| r.selection && matches_chain(&r.chain, &r.combs, desc, ancestors, prev))
        .collect();
    if matched.is_empty() {
        return inherited;
    }
    matched.sort_by(|a, b| layer::cascade(a, b));
    let mut out = inherited;
    for rule in matched {
        for (k, v) in &rule.decls {
            match k.as_str() {
                "color" => {
                    if let Some(c) = parse_color(v) {
                        out.color = Some(c);
                    }
                }
                // `background` counts only for its colour: a highlight has
                // no image.
                "background" | "background-color" => {
                    if let Some(c) = parse_color(v) {
                        out.background = Some(c);
                    }
                }
                other => warn_once(format!(
                    "`{other}` does nothing in `::selection`; a highlight honours `color` and \
                     `background-color` only"
                )),
            }
        }
    }
    out
}

/// The colour an input's placeholder is drawn in, by its `::placeholder`
/// rules: their `color`, faded by their `opacity`, with `var()` and
/// `currentcolor` read as the input reads them. `None` when no rule says,
/// and Rux's own dim colour stands.
///
/// Only the input's own rules count. A placeholder is part of the input, so
/// `::placeholder` does not reach down from an ancestor as `::selection` does.
fn matched_placeholder(
    desc: &ElemDesc,
    ancestors: &[AncNode],
    prev: &[ElemDesc],
    rules: &[Rule],
    vars: &HashMap<String, String>,
    current: Rgba,
    scheme: ColorScheme,
) -> Option<Rgba> {
    let mut matched: Vec<&Rule> = rules
        .iter()
        .filter(|r| r.placeholder && matches_chain(&r.chain, &r.combs, desc, ancestors, prev))
        .collect();
    if matched.is_empty() {
        return None;
    }
    matched.sort_by(|a, b| layer::cascade(a, b));
    let (mut color, mut opacity) = (None, 1.0f32);
    for rule in matched {
        for (k, v) in &rule.decls {
            let v = resolve_vars(v, vars, 0);
            let v = rewrite_colors(&v, current, scheme).unwrap_or(v);
            match k.as_str() {
                "color" => {
                    if let Some(c) = parse_color(&v) {
                        color = Some(c);
                    }
                }
                "opacity" => {
                    if let Ok(o) = v.trim().parse::<f32>() {
                        opacity = o.clamp(0.0, 1.0);
                    }
                }
                other => warn_once(format!(
                    "`{other}` does nothing in `::placeholder`; a placeholder honours `color` and `opacity` only"
                )),
            }
        }
    }
    let c = color.unwrap_or(PLACEHOLDER_COLOR);
    Some(Rgba { a: c.a * opacity, ..c })
}

/// The placeholder's colour when no `::placeholder` rule gives one.
const PLACEHOLDER_COLOR: Rgba = Rgba::new(0.42, 0.44, 0.52, 1.0); // #6c7086

/// Build one element into a layout node. Structural directives on the element
/// itself (`r-for`, `r-if`, `r-elif`, `r-else`) are handled by the parent in
/// [`build_children`]; this function handles per-node concerns (`r-show`) and
/// recurses into children.
///
/// `inherited` carries the resolved text properties (`color`/`font-size`/
/// `font-family`, which inherit); `locals` carries `r-for` loop bindings.
#[allow(clippy::too_many_arguments)]
/// Build one element, with every warning raised underneath it attributed to the
/// line the element was written on.
///
/// A wrapper rather than a `located` around the body, because the body has a
/// dozen early returns and each one would have to remember to restore the
/// position. Wrapping the call is the one place that cannot be forgotten.
///
/// The default is the element's own line, and the places that know better say
/// so: a handler is placed on its attribute's line, a `{{ }}` on its text
/// node's. Nesting works because [`rux_script::located`] restores what it
/// replaced, so a child's line does not outlive the child.
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
    instances: &mut Instances,
    swaps: &mut Swaps,
    instance: Option<&str>,
    slot: Option<Slot>,
    outlet: Option<Outlet>,
    row: Option<&str>,
    swap: Option<SwapSide>,
) -> LayoutNode {
    let mut node = rux_script::located(Some(el.line), || {
        build_node_inner(
            el, rules, comps, ancestors, prev, inherited, engine, locals, path, tpl_path, reg,
            state, instances, swaps, instance, slot, outlet, row, swap,
        )
    });
    // Anything keyboard focus can land on carries the path `:focus` is matched
    // by, the same path `:hover` uses. A component expands in place at the
    // element's own path, so its root may already carry it.
    if node.focus_path.is_none() && (node.on_tap.is_some() || node.model.is_some()) {
        node.focus_path = Some(path.to_vec());
    }
    // `@key` on one of Rux's own elements. On a component's tag it is a
    // listener for an `emit("key")`, as every `@name` there is. A component
    // expands in place, so its root may carry its own `@key` already, and the
    // tag's would then be the caller's listener, not this.
    if node.on_key.is_none() && rux_parser::is_element(&el.tag) {
        if let Some(h) = el.attr("@key") {
            let baked = handler_locals(locals, instance, instances);
            node.on_key = Some(bind_locals(h, &baked));
        }
    }
    node
}

/// Puts every relative unit in `props` into pixels and returns the element's own
/// font size. A viewport unit in a [`LAYOUT_LENGTHS`] property is left for the
/// layout, which resolves it against the window every frame.
///
/// Its own function for the reason [`build_text`] is: [`build_node_inner`]
/// recurses once per element, and with this inline a fresh project's
/// `rux check` overflowed again (watchlist #41).
#[inline(never)]
fn resolve_lengths(props: &mut HashMap<String, String>, inherited_font_size: f32) -> f32 {
    let viewport = BUILD_ENV.with(|e| {
        let v = e.get().viewport;
        (v.width, v.height)
    });
    // A percentage in `font-size`, `calc()`'s included, is of the inherited size.
    let own_font_size = props
        .get("font-size")
        .and_then(|v| {
            let v = resolve_units(v, inherited_font_size, Some(viewport));
            let v = fold_math(&v, Some(inherited_font_size)).unwrap_or(v);
            parse_px_len(first(&v), inherited_font_size)
        })
        .unwrap_or(inherited_font_size);
    for (property, value) in props.iter_mut() {
        if has_units(value) {
            // The layout reads a viewport unit itself, but not one inside
            // maths (`width: clamp(100px, 20vw, 300px)`), which is worked out
            // here and so has to be in pixels by now.
            let maths = value.contains('(')
                && rux_layout::calc::FUNCTIONS.iter().any(|f| contains_ignore_case(value, &format!("{f}(")));
            let viewport = (maths || !LAYOUT_LENGTHS.contains(&property.as_str())).then_some(viewport);
            *value = resolve_units(value, own_font_size, viewport);
        }
        if value.contains('(') {
            if let Some(folded) = fold_math(value, None) {
                *value = folded;
            }
        }
    }
    own_font_size
}

/// `value` with each maths function (`calc()`, `min()`, `clamp()`, …) that
/// can be worked out now put in pixels, or as a plain number when it holds
/// no length. One that needs a percentage's basis is left for the layout,
/// unless `basis` is given; a viewport unit has been put in pixels already
/// wherever the cascade is the one to resolve it. `None` when nothing
/// changed.
fn fold_math(value: &str, basis: Option<f32>) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    let mut changed = false;
    while i < value.len() {
        let boundary = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'-');
        let head = &bytes[i..];
        let called = boundary
            .then(|| {
                rux_layout::calc::FUNCTIONS.iter().find(|f| {
                    head.len() > f.len() && head[..f.len()].eq_ignore_ascii_case(f.as_bytes()) && head[f.len()] == b'('
                })
            })
            .flatten();
        if let Some(name) = called {
            let rest = &value[i..];
            if let Some(close) = closing_paren(&rest[name.len() + 1..]).map(|c| name.len() + 1 + c) {
                let worked = rux_layout::calc::parse(&rest[..=close]).and_then(|c| Some((c.eval(basis, None)?, c.needs())));
                if let Some((v, needs)) = worked.filter(|(v, _)| v.is_finite()) {
                    out.push_str(&if needs.length { format!("{v}px") } else { format!("{v}") });
                    i += close + 1;
                    changed = true;
                    continue;
                }
            }
        }
        let c = value[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    changed.then_some(out)
}

/// Puts the colours that need the element into `props`: `currentColor`
/// becomes its `color` (the inherited one, on `color` itself),
/// `light-dark()` takes the side its colour scheme picks, and each colour
/// function (`oklch()`, `hsl()`, `color-mix()`, …) becomes `#rrggbbaa`, so
/// a shorthand that splits on spaces (`border: 1px solid oklch(70% .1 200)`)
/// meets one word. Returns the element's colour scheme, which inherits.
///
/// Its own function, not inline in [`build_node_inner`]: watchlist #41.
#[inline(never)]
fn resolve_colors(props: &mut HashMap<String, String>, inherited: Rgba, inherited_scheme: ColorScheme) -> ColorScheme {
    // `color-scheme` is what `light-dark()` follows. Unset (`normal`) is
    // light whatever the system says, as in CSS; `light dark` follows the
    // system and `dark` alone is dark.
    let scheme = match props.get("color-scheme") {
        Some(v) => {
            let words: Vec<&str> = v.split_whitespace().collect();
            match (words.contains(&"light"), words.contains(&"dark")) {
                (true, true) => {
                    READS_SCHEME.with(|r| r.set(true));
                    BUILD_ENV.with(|e| e.get().color_scheme)
                }
                (false, true) => ColorScheme::Dark,
                _ => ColorScheme::Light,
            }
        }
        None => inherited_scheme,
    };
    let own = match props.get_mut("color") {
        Some(v) => {
            if let Some(out) = rewrite_colors(v, inherited, scheme) {
                *v = out;
            }
            parse_color(v).unwrap_or(inherited)
        }
        None => inherited,
    };
    for (property, value) in props.iter_mut() {
        if property != "color" {
            if let Some(out) = rewrite_colors(value, own, scheme) {
                *value = out;
            }
        }
        if property == "box-shadow" {
            if let Some(out) = shadow_colors(value, own) {
                *value = out;
            }
        }
        if property == "filter" || property == "backdrop-filter" {
            if let Some(out) = drop_shadow_colors(value, own) {
                *value = out;
            }
        }
    }
    scheme
}

/// A `box-shadow` list with `color` written into each shadow that has no
/// colour of its own, which CSS draws in `currentColor`. `None` when every
/// shadow has one.
fn shadow_colors(value: &str, color: Rgba) -> Option<String> {
    if value.trim() == "none" {
        return None;
    }
    let shadows = split_top_level_commas(value);
    let bare = |s: &str| !split_top_level(s).into_iter().any(|t| parse_color(t).is_some());
    if !shadows.iter().any(|s| bare(s)) {
        return None;
    }
    let hex = color::hex(color);
    let written: Vec<String> =
        shadows.iter().map(|s| if bare(s) { format!("{s} {hex}") } else { s.to_string() }).collect();
    Some(written.join(", "))
}

thread_local! {
    /// Whether a build has followed the system's light or dark setting
    /// through `color-scheme: light dark`, so a change to it has to cascade
    /// again. See [`reads_scheme`].
    static READS_SCHEME: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether a build has picked a side of `light-dark()` by the system's
/// setting, which goes stale when the setting changes. Never cleared, as
/// [`reads_viewport`] is not.
pub fn reads_scheme() -> bool {
    READS_SCHEME.with(std::cell::Cell::get)
}

/// The colour functions [`rewrite_colors`] resolves, longest first so
/// `color-mix(` is not read as `color(`.
const COLOR_FUNCTIONS: &[&str] =
    &["light-dark", "color-mix", "color", "rgba", "rgb", "hsla", "hsl", "hwb", "oklab", "oklch", "lab", "lch"];

/// `value` with its colour functions and `currentColor` resolved, or `None`
/// when it has neither, which is most values.
fn rewrite_colors(value: &str, current: Rgba, scheme: ColorScheme) -> Option<String> {
    if !value.contains('(') && !contains_ignore_case(value, "currentcolor") {
        return None;
    }
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    let mut changed = false;
    while i < value.len() {
        let boundary = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'-');
        if boundary {
            let rest = &value[i..];
            let head = rest.as_bytes();
            if head.len() >= 12 && head[..12].eq_ignore_ascii_case(b"currentcolor")
                && !head.get(12).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'-')
            {
                out.push_str(&color::hex(current));
                i += 12;
                changed = true;
                continue;
            }
            let called = COLOR_FUNCTIONS.iter().find(|f| {
                head.len() > f.len() && head[..f.len()].eq_ignore_ascii_case(f.as_bytes()) && head[f.len()] == b'('
            });
            if let Some(name) = called {
                if let Some(close) = closing_paren(&rest[name.len() + 1..]).map(|i| name.len() + 1 + i) {
                    let inner = &rest[name.len() + 1..close];
                    let inner = rewrite_colors(inner, current, scheme).unwrap_or_else(|| inner.to_string());
                    let resolved = if *name == "light-dark" {
                        let sides = color::split_top(&inner, ',');
                        match sides[..] {
                            [light, dark] => Some(if scheme == ColorScheme::Dark { dark } else { light }.trim().to_string()),
                            _ => None,
                        }
                    } else {
                        parse_color(&format!("{name}({inner})")).map(color::hex)
                    };
                    if let Some(r) = resolved {
                        out.push_str(&r);
                        i += close + 1;
                        changed = true;
                        continue;
                    }
                }
            }
        }
        let c = value[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    changed.then_some(out)
}

fn contains_ignore_case(hay: &str, needle: &str) -> bool {
    hay.as_bytes().windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// A `<text>`: plain words, a sentence of pieces, or a run inside one.
///
/// Its own function, not a branch of [`build_node_inner`], because that one
/// recurses once per element and every local it holds is held at every depth:
/// with this inline, a fresh project's `rux check` overflowed the 1 MB main
/// thread of a debug build (watchlist #41).
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn build_text(
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
    instances: &mut Instances,
    swaps: &mut Swaps,
    instance: Option<&str>,
    slot: Option<Slot>,
    outlet: Option<Outlet>,
    row: Option<&str>,
    style: Style,
    desc: ElemDesc,
    on_tap: Option<String>,
    to: Option<String>,
    hidden: bool,
    state_path: Option<Vec<usize>>,
    color: Rgba,
    selection: SelectionStyle,
    font_size: f32,
    font_family: Option<String>,
    letter_spacing: Option<f32>,
    word_spacing: Option<f32>,
    line_height: Option<f32>,
    italic: bool,
    underline: bool,
    strikethrough: bool,
    white_space: WhiteSpace,
    direction: rux_layout::Direction,
    dir_auto: bool,
    vars: Vars,
    weight: u16,
    align: TextAlign,
    own_text: TextInherit,
    // Whether this element sets a direction of its own, not one it inherits.
    dir_own: bool,
    shadows: Option<Arc<Vec<rux_layout::TextShadow>>>,
    look: Option<Arc<TextLook>>,
) -> LayoutNode {
    // Inside a sentence, the sentence's decorations are drawn under this run's
    // words too. See `RunInherit`.
    let outer = inherited.run;
    let underline = underline || outer.is_some_and(|r| r.underline);
    let strikethrough = strikethrough || outer.is_some_and(|r| r.strikethrough);
    let nowrap = !white_space.wraps();
    let wrap = style.text_wrap;
    // Recompute this text on change instead of rebuilding: record the raw
    // template, the locals, and the signals it reads, keyed by this node's
    // path. Only text that actually interpolates is registered.
    let template = text_template(el);
    // Placed on the text itself, which for a multi-line `<text>` is not the
    // line the tag opened on.
    let (text, deps) =
        rux_script::located(Some(text_line(el)), || interpolate_tracked(&template, engine, locals));
    // A run keeps its spaces for the layout, which collapses them with the
    // runs beside it; anything else is collapsed now.
    let collapse = outer.is_none().then_some(white_space);
    let text = match collapse {
        Some(mode) => white_space::collapse(&text, mode),
        None => text,
    };
    if template.contains("{{") {
        reg.text.push(TextBinding {
            path: path.to_vec(),
            template,
            locals: locals.clone(),
            deps,
            white_space: collapse,
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
            shadows: shadows.clone(),
            selection_style: Default::default(),
            look: look.clone(),
            rich: None,
            white_space,
            direction: (!dir_auto).then_some(direction),
            // A run with a `dir` of its own is set apart from its sentence.
            isolate: if inherited.run.is_none() {
                rux_layout::Isolate::None
            } else if dir_auto {
                rux_layout::Isolate::Auto
            } else if dir_own {
                match direction {
                    rux_layout::Direction::Ltr => rux_layout::Isolate::Ltr,
                    rux_layout::Direction::Rtl => rux_layout::Isolate::Rtl,
                }
            } else {
                rux_layout::Isolate::None
            },
        },
    );
    // A sentence: the pieces inside it are built as its children, each the
    // element it is, and the layout joins them into one text.
    if el.children.iter().any(|c| matches!(c, TplNode::Element(_))) {
        check_sentence_children(el);
        let run = RunInherit { underline, strikethrough };
        let child_inherited = Inherited {
            color,
            selection,
            font_size,
            font_family: font_family.clone(),
            vars: Rc::clone(&vars),
            run: Some(run),
            direction,
            text: own_text,
            shadows: shadows.clone(),
            look: look.clone(),
        };
        let siblings = if KEEP_SIBLINGS.with(std::cell::Cell::get) { prev.to_vec() } else { Vec::new() };
        ancestors.push(AncNode { desc, prev: siblings.into(), place: prev.len() + 1, child_total: None, form: None });
        let plain = instance.is_none() && slot.is_none() && outlet.is_none() && row.is_none();
        let (children, structural_deps) = build_children(
            &element_children(el),
            rules,
            comps,
            ancestors,
            &child_inherited,
            engine,
            locals,
            path,
            tpl_path,
            reg,
            state,
            instances,
            swaps,
            instance,
            slot,
            outlet,
            row,
        );
        // Rebuilt alone when an `r-if` or `r-for` inside it changes, as any
        // other parent's children are.
        let ctx = (plain && !structural_deps.is_empty()).then(|| {
            Rc::new(ListCtx {
                el: el as *const Element as usize,
                ancestors: ancestors.clone(),
                inherited: child_inherited.clone(),
                locals: locals.clone(),
            })
        });
        ancestors.pop();
        if !structural_deps.is_empty() {
            reg.structural_parents.push(StructuralParent {
                tree_path: path.to_vec(),
                tpl_path: tpl_path.to_vec(),
                deps: structural_deps,
                ctx,
            });
        }
        node.children = children;
    }
    node.on_tap = on_tap;
    // Inside a component a tap here runs against that instance's state,
    // as the general element branch below has always said; this branch
    // never did, so `<text @tap="n++">` in a component ran in the
    // document's scope, where `n` is not.
    node.instance = instance.map(str::to_string);
    node.hidden = hidden;
    node.id = el.attr("id").map(str::to_string);
    node.label_for = el.attr("for").map(str::to_string);
    node.state_path = state_path.clone();
    // Static text reads as a label; `role="heading"` promotes it. Text that is
    // itself tappable is a button whose name is its own words, unless it is a
    // link, which a screen reader announces differently and should.
    // A run that is only words is read as part of its sentence, so it is
    // nothing of its own; one that does something is a link or a button
    // inside it. A sentence is read by all of its words.
    let words = sentence_words(&node);
    node.access = Access {
        role: explicit_access_role(el).unwrap_or(if to.is_some() {
            AccessRole::Link
        } else if node.on_tap.is_some() {
            AccessRole::Button
        } else if outer.is_some() {
            AccessRole::None
        } else {
            AccessRole::Label
        }),
        label: authored_label(el).or_else(|| Some(words.trim().to_string()).filter(|t| !t.is_empty())),
        ..Access::default()
    };
    return node;
}

#[allow(clippy::too_many_arguments)]
fn build_node_inner(
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
    instances: &mut Instances,
    swaps: &mut Swaps,
    // The component instance this element is inside, None at document level.
    instance: Option<&str>,
    slot: Option<Slot>,
    outlet: Option<Outlet>,
    // The `r-key` of the `r-for` row this element is inside, `None` outside one.
    // Half the identity of anything in a list: `r-model` is recorded as written,
    // so every row of a list otherwise looks like the same input.
    row: Option<&str>,
    // Which side of a swap this element is on, `None` when it is not swapping.
    // Only ever `Some` for the element carrying `r-transition`; its descendants
    // inherit the visual effect through the cascade, not through this.
    swap: Option<SwapSide>,
) -> LayoutNode {
    // What a handler written here carries with it. See `handler_locals`.
    let baked = handler_locals(locals, instance, instances);
    // A tag that is neither one of Rux's own elements nor anything this file
    // imported can never render anything, so it is an error and not a warning.
    //
    // It was silent until 2026-09-15, and that silence is what hid a larger bug
    // for the whole of v0.7: a component's `use` lines were thrown away, so a
    // tag inside a component matched nothing, expanded to nothing, and left an
    // empty screen to be read as an empty list. A tag is the one name in a
    // template with no other way to fail.
    if el.tag == rux_parser::RUN_TAG {
        return build_run(el, inherited, engine, locals, path, reg);
    }
    if !rux_parser::is_element(&el.tag) && find_component(comps, &el.tag).is_none() {
        located(Some(el.line), || {
            error(format!(
                "there is no element or component called `<{}>`. Rux's own elements are {}; \
                 anything else is a component, and needs a `use` for it in this file's \
                 <script>",
                el.tag,
                rux_parser::element_tags()
                    .iter()
                    .map(|t| format!("<{t}>"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ))
        });
    }
    // A custom-element tag expands its imported component in place. Written
    // either way round: `<new-task>` and `<new_task>` are the same component,
    // see `find_component`.
    if let Some((tag, component)) = find_component(comps, &el.tag) {
        return expand_component(
            el, component, comps, ancestors, inherited, engine, locals, path, tpl_path, reg, state, rules,
            instances, swaps, instance, row, &Locals::new(), None,
            // An ordinary component tag is not a route view, so a
            // `<router-view />` inside it has no chain and says so.
            None,
            // Here the element *is* the component tag, under the name it is
            // registered by rather than the one this line happens to spell.
            tag,
            // A component tag carrying `r-if` and `r-transition` swaps like any
            // other element; the side lands on the component's root.
            swap,
        );
    }

    // **An `<input type=>` nobody recognises is a plain text field, and used to
    // be one in silence.** Four values are acted on, and anything else fell
    // through to the text path with nothing said. That is tolerable for a
    // misspelling and not tolerable for `type="password"`, which rendered a
    // working field that showed every character it was given. The author had
    // written the one thing that says "hide this" and the engine had ignored
    // it.
    //
    // Checked here rather than in the text branch below, because a checkbox, a
    // radio and a select never reach that branch and a typo in any of them
    // deserves the same answer.
    //
    // Reported 2026-09-21 by the user, asking whether the keyboard could tell
    // the types apart. It could not, and the deeper answer was that neither
    // could Rux.
    if el.tag == "input" {
        if let Some(kind) = el.attr("type") {
            const KNOWN: [&str; 11] = [
                "text", "textarea", "password", "search", "number", "date", "select", "checkbox",
                "radio", "switch", "slider",
            ];
            if !KNOWN.contains(&kind) {
                located(Some(el.line), || {
                    error(format!(
                        "`<input type=\"{kind}\">` is not a kind of input Rux has: it would \
                         render as a plain text field, which for something like `password` \
                         means showing every character. Use one of {}, or leave `type` off \
                         for a single-line text field",
                        KNOWN.join(", ")
                    ))
                });
            }
        }
    }

    // `disabled`, on the two elements that can be operated. Worked out before
    // the cascade, because `:disabled` is matched by it.
    let mut disabled_deps: HashSet<String> = HashSet::new();
    let disabled = matches!(el.tag.as_str(), "input" | "button")
        && bool_attr(el, "disabled", engine, locals, &mut disabled_deps);
    // `readonly` beside it, for the same reason and with the same bound form.
    let readonly =
        el.tag == "input" && bool_attr(el, "readonly", engine, locals, &mut disabled_deps);
    if el.tag == "input" {
        check_field_attributes(el);
    }
    // What an input says about its value, for its form and for `:invalid`.
    // Every kind of input, since a form sends them all.
    let form_part = (el.tag == "input").then(|| {
        let required = bool_attr(el, "required", engine, locals, &mut disabled_deps);
        form_part(el, required, ancestors)
    });

    let mut desc = ElemDesc::of(el);
    desc.attrs.extend(bound_attributes(el, rules, engine, locals, &mut disabled_deps));
    // Counted by the parent before its children were built, when a rule
    // counts from the end. The root has no siblings but itself.
    desc.total = match ancestors.last() {
        Some(parent) => parent.child_total,
        None => Some(1),
    };
    desc.states.disabled = disabled;
    desc.states.enabled = matches!(el.tag.as_str(), "input" | "button") && !disabled;
    if let Some(part) = &form_part {
        desc.states.required = Some(part.checks.required);
        // A disabled or readonly field is not checked, as HTML leaves both out
        // of validation: nobody could fix what it holds.
        if let Some(model) = part.bind.as_deref().filter(|_| !disabled && !readonly) {
            let (value, deps) = engine.eval_value_tracked(model, locals);
            disabled_deps.extend(deps);
            desc.states.valid = Some(checks::field_problem(part, value.as_ref()).is_none());
            let touched = state.touched.iter().any(|(m, r, i)| {
                m == model && r.as_deref() == row && i.as_deref() == instance
            });
            let attempted = part.form.as_ref().is_some_and(|f| state.attempted.contains(f));
            desc.states.user = touched || attempted;
        }
    }
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
    // The swap states. `:enter-from` is held for the first frame only, and that
    // asymmetry is the whole trick: dropping it next frame turns the swap into an
    // ordinary style change, which is exactly what tier 1 already animates.
    desc.states.enter_from =
        swap.is_some_and(|s| s.phase == Phase::Entering && s.first);
    desc.states.leave_to = swap.is_some_and(|s| s.phase == Phase::Leaving);
    // Both halves, or every row of a list matches at once: they all carry the
    // same `r-model` text, so the model alone cannot pick one out.
    let field_focused = match (&state.focused_model, el.attr("r-model")) {
        (Some(focused), Some(model)) => {
            focused == model
                && state.focused_row.as_deref() == row
                && state.focused_instance.as_deref() == instance
        }
        _ => false,
    };
    // Anything else is known by where it is. The element itself only: an
    // ancestor of the focused element is `:focus-within`, not `:focus`.
    let other_focused = state.focused_path.as_deref() == Some(path);
    desc.states.focus = field_focused || other_focused;
    // A text field shows its focus however it got it, since it is about to be
    // typed into; anything else only after the keyboard put it there.
    desc.states.focus_visible = field_focused || (other_focused && state.focus_visible);
    // `:class`: dynamic classes fed into the cascade (the `checked` pattern,
    // generalized). Signals it reads are collected for reconcile.
    let mut dyn_deps: HashSet<String> = disabled_deps;
    // Where this element links to, if anywhere. `:to` is the computed form, which
    // is what a list of rows needs: every row links somewhere different, and the
    // path is built from the row's own data.
    // A written-out `to` names a path the router is supposed to know, so it is
    // answerable here. `:to` is not: it is built from a row's own data and a
    // path that exists for row 3 and not for row 4 is a data problem, not a
    // markup one.
    if let Some(written) = el.attr("to") {
        located(el.attr_line("to"), || warn_if_no_such_route(written));
    }
    let to = el.attr("to").map(str::to_string).or_else(|| {
        let expr = el.attr(":to")?;
        let (value, deps) = engine.eval_value_tracked(expr, locals);
        dyn_deps.extend(deps);
        value.map(|v| v.to_display())
    });
    // `:current` is the link pointing at the path we are already on, so a nav bar
    // can show where you are. Reading the route here also subscribes this node to
    // it, which is what lets navigation restyle the link in place instead of
    // rebuilding: the same trick `:class` uses.
    if let Some(to) = &to {
        let (value, deps) = engine.eval_value_tracked(rux_script::ROUTE_SIGNAL, locals);
        dyn_deps.extend(deps);
        desc.states.current =
            value.is_some_and(|v| match_route(to, &v.to_display()).is_some());
    }
    if let Some(expr) = el.attr(":class") {
        let (value, deps) = rux_script::located(el.attr_line(":class"), || {
            engine.eval_value_tracked(expr, locals)
        });
        dyn_deps.extend(deps);
        if let Some(v) = value {
            desc.classes.extend(class_list(&v));
        }
    }

    // Record what this node is before the cascade consumes it. Placed after
    // `:class` and the pseudo-states for the same reason `state_path` is: a
    // dynamically applied class is a class, and `query(".open")` has to agree
    // with what the stylesheet matched a moment earlier or the two disagree
    // about the same document.
    reg.elements.push(desc.clone(), path);

    // Set on every node this build produces, so the shell gets a region to hit-test
    // (see `pointer_state_sensitive`). Computed after `:class`, so dynamically
    // applied classes count too.
    let state_path = pointer_state_sensitive(&desc, rules).then(|| path.to_vec());

    let mut props =
        rux_script::profile::time(rux_script::profile::Phase::Match, || matched_props(&desc, ancestors, prev, rules));
    // Inline styles override the cascade: static `style=` first, then dynamic
    // `:style` (which may interpolate, rhai backtick strings evaluate here).
    // Kept in order as well as merged, for the replay below.
    let mut inline: Vec<(String, String)> = Vec::new();
    if let Some(s) = el.attr("style") {
        inline_decls(s, &mut inline);
    }
    if let Some(expr) = el.attr(":style") {
        let (value, deps) = engine.eval_value_tracked(expr, locals);
        dyn_deps.extend(deps);
        match value {
            // Object form: `#{ background: c }` → each entry a declaration.
            Some(Value::Map(entries)) => {
                for (k, v) in entries {
                    inline.push((k.to_ascii_lowercase(), v.to_display()));
                }
            }
            // String form: `"background: red"` (possibly interpolated).
            Some(v) => inline_decls(&v.to_display(), &mut inline),
            None => {}
        }
    }
    for (k, v) in &inline {
        declare(&mut props, k, v);
    }
    // `direction`, from the property, else `dir`, else the parent. `dir="auto"`
    // leaves a text to its first letter with a direction, as HTML does.
    let (direction, dir_auto) = resolve_direction(el, &props, inherited.direction);
    let dir_own = el.attr("dir").is_some() || props.contains_key("direction");
    // A logical property names a side by the direction, and which of it and a
    // physical one wins is the order they were declared in, which the map
    // above has lost. So the few elements that use one are cascaded again, in
    // order, with each logical name put on its side as it is met.
    if props.keys().any(|k| logical::is_logical(k)) {
        props = logical::cascade_in_order(matched_rules(&desc, ancestors, prev, rules), &inline, direction);
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
        // After `var()`, so a variable holding an `env()` resolves, and so an
        // `env()` fallback may itself be a variable.
        if value.contains("env(") {
            *value = resolve_env(value);
        }
    }
    wide_keywords(&mut props);

    // Resolve `em` here, for the same reason `var()` is resolved here: every
    // length parser would otherwise have to learn about font size, and most of
    // them are reached with no element in hand. `interpret` takes a property map
    // and nothing else.
    //
    // `font-size` resolves first and against the *inherited* size, because that
    // is what `em` means on the property that defines it: `font-size: 1.5em` is
    // one and a half times the parent's, not its own.
    let own_font_size = resolve_lengths(&mut props, inherited.font_size);
    let scheme = resolve_colors(&mut props, inherited.color, inherited.text.scheme);
    // After `var()` and `em` are resolved, so a variable holding a bad length is
    // reported as the length it turned out to be rather than as `var(--x)`.
    warn_unparseable_lengths(&props);

    // **The focus ring is the default stylesheet's, and the author's rules
    // come after it.** Every browser ships `:focus-visible { outline: auto }`,
    // so any `outline` an author writes for the element replaces the ring and
    // `outline: none` removes it. Added only where the author said nothing
    // about the outline's style, which is exactly where a user-agent rule
    // would survive the cascade.
    if desc.states.focus_visible && !props.contains_key("outline") && !props.contains_key("outline-style")
    {
        props.insert("outline-style".to_string(), "auto".to_string());
    }

    let mut style = screen_fills_the_display(interpreted(&props), el, path, &props);
    style.direction = direction;
    if props.keys().any(|k| k.starts_with("animation")) {
        if let Some(list) = keyframe_animations(&props, &vars, inherited.font_size, inherited.color, inherited.text.scheme) {
            style.rare_mut().animations = Some(list);
        }
    }
    // A `@tap` handler runs later, in global scope, where the `r-for` loop
    // variable no longer exists, so `@tap="picked = item"` would see `item`
    // undefined and silently do nothing. Bake the current loop bindings into the
    // handler as a `let` prelude so it reproduces them when it runs.
    // `to="/path"` is a navigation intent, which is a tap like any other once it
    // is written as a handler: it then travels the one route a tap already
    // takes, through the hit region and `apply_handler`. An explicit `@tap`
    // wins, so a link can still do something else on the way.
    //
    // A disabled control answers nothing: no tap, no link, no gesture. Taken
    // away here rather than refused later, so that nothing downstream (the hit
    // regions, the keyboard's Enter, a label's `for=`) has a handler to find.
    let on_tap = el.attr("@tap").map(|h| bind_locals(h, baked)).or_else(|| {
        to.as_ref().map(|p| format!("navigate({})", Value::Text(p.clone()).to_rhai_literal()))
    }).filter(|_| !disabled);
    // `<button type="submit">` submits the form around it, after its own `@tap`
    // has run. Written onto the tap itself, so a finger, Enter on a focused
    // button and a script's `tap()` all submit alike: each of them already
    // runs the tap.
    let on_tap = match submit_target(el, ancestors) {
        Some(form) if !disabled => {
            let form: Vec<String> = form.iter().map(usize::to_string).collect();
            let submit = format!("{}(\"{}\")", rux_script::SUBMIT_FN, form.join("."));
            Some(match on_tap {
                Some(tap) => format!("{{ {tap} }} {submit}"),
                None => submit,
            })
        }
        _ => on_tap,
    };
    // The pointer handlers, in the order the vocabulary lists them rather than
    // the order they happen to be written, so two elements with the same set
    // dispatch in the same order.
    let gestures: Vec<(rux_layout::Gesture, String)> = [
        ("press", rux_layout::Gesture::Press),
        ("release", rux_layout::Gesture::Release),
        ("longpress", rux_layout::Gesture::LongPress),
        ("swipe", rux_layout::Gesture::Swipe),
        ("drag", rux_layout::Gesture::Drag),
    ]
    .into_iter()
    .filter_map(|(name, kind)| {
        // An `r-for` local is baked in the same way a `@tap`'s is: the body runs
        // long after the build that could still see the row.
        el.attr(&format!("@{name}")).map(|h| (kind, bind_locals(h, baked)))
    })
    .filter(|_| !disabled)
    .collect();
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
    // An outline with no colour of its own is drawn in the element's `color`,
    // which is only known now. An `auto` ring keeps the ring's colour.
    if let Some(o) = style.outline.as_mut().filter(|o| !o.auto) {
        o.color.get_or_insert(color);
    }
    // So is a border, whose colour is `currentColor` until one is written.
    // lightningcss drops it from `border: 1px solid currentColor` for being
    // the default, and a border with no colour was not drawn at all.
    if style.border_color.is_none() {
        style.border_color = Some(color);
    }
    let selection = matched_selection(&desc, ancestors, prev, rules, inherited.selection);
    let placeholder_color = if el.tag == "input" {
        matched_placeholder(&desc, ancestors, prev, rules, &vars, color, scheme).unwrap_or(PLACEHOLDER_COLOR)
    } else {
        PLACEHOLDER_COLOR
    };
    // Already resolved above, against the inherited size, so that the `em` pass
    // had something to resolve against.
    let font_size = own_font_size;
    // `font-family` is stored as the raw CSS list; parley parses it and does the
    // fallback. An empty/`inherit` value falls back to the inherited family.
    let font_family = props
        .get("font-family")
        .filter(|v| !v.trim().is_empty() && v.trim() != "inherit")
        .map(|v| v.trim().to_string())
        .or_else(|| inherited.font_family.clone());
    // The inheriting text properties: this element's own, else its parent's.
    // See `TextInherit`.
    let up = inherited.text;
    let own_text = TextInherit {
        weight: props.get("font-weight").and_then(|v| parse_weight(v)).unwrap_or(up.weight),
        italic: props.get("font-style").map_or(up.italic, |v| matches!(v.trim(), "italic" | "oblique")),
        letter_spacing: match props.get("letter-spacing") {
            Some(v) => parse_spacing(v),
            None => up.letter_spacing,
        },
        word_spacing: match props.get("word-spacing") {
            Some(v) => parse_spacing(v),
            None => up.word_spacing,
        },
        // `white-space`: `nowrap` and `pre` stop line breaking, and a `<text>`
        // collapses its spaces by it (see `white_space`).
        white_space: props.get("white-space").and_then(|v| WhiteSpace::parse(v)).unwrap_or(up.white_space),
        // `line-height`: a unitless number multiplies the font size; a length
        // is absolute; `normal` keeps the font metrics.
        line_height: match props.get("line-height") {
            Some(v) => line_height_spec(v),
            None => up.line_height,
        },
        align: props.get("text-align").map(|v| parse_text_align(v)).unwrap_or(up.align),
        scheme,
        invisible: match props.get("visibility").map(|v| v.trim()) {
            Some("hidden" | "collapse") => true,
            Some("visible") => false,
            _ => up.invisible,
        },
        // `pointer-events`: `none`, or `auto` back again. CSS's SVG values
        // name parts of a shape; on a box they come to `auto`.
        no_pointer: match props.get("pointer-events").map(|v| v.trim()) {
            Some("none") => true,
            Some(_) => false,
            None => up.no_pointer,
        },
    };
    style.invisible = own_text.invisible;
    style.no_pointer = own_text.no_pointer;
    style.scroll_smooth = match props.get("scroll-behavior").map(|v| v.trim()) {
        Some("smooth") => true,
        Some("auto") | None => false,
        Some(other) => {
            warn(format!("`scroll-behavior: {other}` is not a value Rux knows; use `auto` or `smooth`"));
            false
        }
    };
    let TextInherit { weight, italic, letter_spacing, word_spacing, white_space, align, .. } = own_text;
    // `text-shadow`: its own, else its parent's.
    let shadows = match props.get("text-shadow") {
        Some(v) => parse_text_shadows(v),
        None => inherited.shadows.clone(),
    };
    let line_height = own_text.line_height.map(|l| l.px(font_size));
    let look = text_look(&props, inherited.look.as_ref(), inherited.run.is_some());
    // `text-decoration[-line]`: underline / line-through (space-separated list).
    let decoration = props.get("text-decoration-line").or_else(|| props.get("text-decoration"));
    let underline = decoration.is_some_and(|v| v.split_whitespace().any(|t| t == "underline"));
    let strikethrough = decoration.is_some_and(|v| v.split_whitespace().any(|t| t == "line-through"));

    if el.tag == "text" {
        return build_text(el, rules, comps, ancestors, prev, inherited, engine, locals, path, tpl_path, reg, state, instances, swaps, instance, slot, outlet, row, style, desc, on_tap, to, hidden, state_path, color, selection, font_size, font_family, letter_spacing, word_spacing, line_height, italic, underline, strikethrough, white_space, direction, dir_auto, vars, weight, align, own_text, dir_own, shadows, look.clone());
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
        let mut image = ImageContent::new(src);
        if let Some(v) = props.get("object-fit") {
            image.fit = match v.trim() {
                "fill" => ObjectFit::Fill,
                "contain" => ObjectFit::Contain,
                "cover" => ObjectFit::Cover,
                "none" => ObjectFit::None,
                "scale-down" => ObjectFit::ScaleDown,
                other => {
                    warn(format!("`object-fit: {other}` is not a fit Rux knows; use `fill`, `contain`, `cover`, `none` or `scale-down`"));
                    ObjectFit::Fill
                }
            };
        }
        if let Some(v) = props.get("object-position") {
            let toks: Vec<String> = split_top_level(v).into_iter().map(str::to_string).collect();
            match parse_bg_position(&toks) {
                Some(p) => image.position = p,
                None => warn(format!("`object-position: {v}` is not a position Rux knows")),
            }
        }
        let mut node = LayoutNode::image(style, image);
        node.on_tap = on_tap;
        node.gestures = gestures;
        // As for text above: a tap or gesture here belongs to the instance.
        node.instance = instance.map(str::to_string);
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

    // <path d=…>: vector geometry, drawn with the `fill` and `stroke` the
    // cascade resolved. The geometry is an attribute and the paint is CSS, and
    // the split is not arbitrary: paint belongs in the cascade because that is
    // where every other appearance in Rux is written, and geometry belongs in
    // an attribute because a data-driven path is computed per row and the
    // cascade is the wrong place for a value that changes with the data.
    if el.tag == "path" {
        let d = el
            .attr(":d")
            .map(|e| {
                let (v, deps) = engine.eval_display_tracked(e, locals);
                // A rebuild rather than a patch. Changing `d` can change the
                // element's size, since a path given no CSS box takes the size
                // of its own geometry, so there is no in-place edit that is
                // always correct. It is also what lets a shape animate at all:
                // the animator compares a node against what it was on the
                // previous build, and a patched attribute never reaches it.
                reg.structural.extend(deps);
                v
            })
            .or_else(|| el.attr("d").map(str::to_string))
            .unwrap_or_default();
        let mut node = LayoutNode::path(style, PathContent::parse(&d));
        node.on_tap = on_tap;
        node.gestures = gestures;
        node.hidden = hidden;
        node.id = el.attr("id").map(str::to_string);
        node.instance = instance.map(|i| i.to_string());
        // A drawing is an image as far as assistive technology is concerned:
        // something to be described, not read. Without a description it is
        // decoration and is better left out of the tree than announced as an
        // unnamed graphic.
        node.access = Access {
            role: el
                .attr("alt")
                .map(|_| AccessRole::Image)
                .unwrap_or(AccessRole::None),
            label: el.attr("alt").map(str::to_string),
            ..Access::default()
        };
        return node;
    }

    // <input>: a box bound to a signal via r-model.
    //
    // `type=checkbox|radio` are tap-toggles, not text fields: they get no focus
    // and no keyboard, they just write the bound signal through the ordinary
    // handler path (`sig = !sig` / `sig = "value"`). An authored @tap wins.
    if let Some(Toggle { radio, switch, checked, deps }) = toggle {
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

        // A switch is a pill with a thumb that sits at the end it points to.
        // Everything it draws is overridable, and what is not written comes
        // from the defaults below, so an unstyled switch is still a switch
        // rather than a box with nothing in it.
        let mut thumb = color;
        if switch {
            if !props.contains_key("width") {
                style.width = Some(Len::Px(44.0));
            }
            if !props.contains_key("height") {
                style.height = Some(Len::Px(24.0));
            }
            if style.radius == [0.0; 4] {
                style.radius = [CIRCLE; 4];
            }
            if !props.contains_key("padding") {
                style.padding = Sides { top: 3.0, right: 3.0, bottom: 3.0, left: 3.0 };
            }
            if style.background.is_none() {
                style.background = Some(Background::Color(if checked {
                    props.get("accent-color").and_then(|v| parse_color(v)).unwrap_or(SWITCH_ON)
                } else {
                    SWITCH_OFF
                }));
            }
            if !props.contains_key("color") {
                thumb = Rgba::new(1.0, 1.0, 1.0, 1.0);
            }
            style.justify = Some(if checked { Justify::End } else { Justify::Start });
        }

        let mut node = LayoutNode::new(style);
        if switch {
            // The thumb: as tall as the track's content box, and round.
            node.children.push(LayoutNode::new(Style {
                display: Display::Flex,
                height: Some(Len::Pct(1.0)),
                aspect_ratio: Some(1.0),
                background: Some(Background::Color(thumb)),
                radius: [CIRCLE; 4],
                ..Default::default()
            }));
        } else if checked {
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
            let target = model_target(&model, locals);
            if model.is_empty() || disabled {
                None
            } else if radio {
                Some(format!("{target} = \"{value}\""))
            } else {
                Some(format!("{target} = !{target}"))
            }
        });
        // `@change` runs after the toggle, in the same handler, so it reads the
        // value the tap just wrote, and is handed it as `event.value` the way a
        // text field's is.
        if let (Some(tap), Some(change)) = (&mut node.on_tap, el.attr("@change")) {
            if !model.is_empty() {
                let change = bind_locals(change, baked);
                *tap = format!("{tap}; let event = #{{ value: {model} }}; {change}");
            }
        }
        node.field =
            rux_layout::Field { disabled, ..form_part.clone().unwrap_or_default() }.boxed();
        node.hidden = hidden;
        node.id = el.attr("id").map(str::to_string);
        node.label_for = el.attr("for").map(str::to_string);
        node.state_path = state_path.clone();
        // The checked state is what a screen reader announces alongside the name,
        // so it has to be the resolved boolean, not the class hack.
        node.access = Access {
            role: if radio {
                AccessRole::RadioButton
            } else if switch {
                AccessRole::Switch
            } else {
                AccessRole::CheckBox
            },
            label: authored_label(el),
            placeholder: None,
            checked: Some(checked),
            value: None,
        };
        return node;
    }

    // `type="slider"`: a number chosen along a track.
    //
    // **Built from parts the language already has, not from a region of its
    // own.** The finger is `@tap` and `@drag` on the element, whose events
    // carry the pointer's `x` and the element's `width`; the value is one
    // line of script computed from those. So a slider inside a scrolling page
    // gets the axis claim every `@drag` gets: a vertical swipe that starts on
    // it still scrolls the page, and only a sideways one moves the thumb.
    //
    // The track is three boxes in a row: the filled part, the thumb, and the
    // rest. The two bars grow in the proportion of the value, so the thumb
    // never overhangs either end, which a percentage width cannot promise.
    if el.tag == "input" && el.attr("type") == Some("slider") {
        let model = el.attr("r-model").unwrap_or_default().to_string();
        let range = SliderRange::of(el);
        let (value, deps) = if model.is_empty() {
            (range.min, HashSet::new())
        } else {
            let (v, deps) = engine.eval_value_tracked(&model, locals);
            (v.and_then(|v| v.as_number()).unwrap_or(range.min), deps)
        };
        // Reconciled like a toggle: a change moves the thumb and changes
        // nothing about the tree's shape.
        reg.toggles.push(ToggleBinding { path: path.to_vec(), deps });
        let fraction = range.fraction(value);
        let accent = props
            .get("accent-color")
            .and_then(|v| parse_color(v))
            .unwrap_or(SWITCH_ON);

        let mut style = style;
        if style.width.is_none() {
            style.width = Some(Len::Pct(1.0));
        }
        if !props.contains_key("height") {
            style.height = Some(Len::Px(32.0));
        }
        style.display = Display::Flex;
        style.align = Some(Align::Center);
        let bar = |grow: f32, color: Rgba| {
            LayoutNode::new(Style {
                display: Display::Flex,
                grow,
                basis: Some(Len::Px(0.0)),
                height: Some(Len::Px(SLIDER_BAR)),
                background: Some(Background::Color(color)),
                radius: [CIRCLE; 4],
                ..Default::default()
            })
        };
        let mut node = LayoutNode::new(style);
        node.children.push(bar(fraction, accent));
        node.children.push(LayoutNode::new(Style {
            display: Display::Flex,
            width: Some(Len::Px(SLIDER_THUMB)),
            height: Some(Len::Px(SLIDER_THUMB)),
            shrink: 0.0,
            background: Some(Background::Color(accent)),
            radius: [CIRCLE; 4],
            ..Default::default()
        }));
        node.children.push(bar(1.0 - fraction, SLIDER_REST));

        if !model.is_empty() && !disabled {
            let set = range.assignment(&model_target(&model, locals));
            let input = el.attr("@input").map(|h| bind_locals(h, baked));
            let change = el.attr("@change").map(|h| bind_locals(h, baked));
            let block = |h: &Option<String>| h.as_ref().map(|h| format!("{{ {h} }}")).unwrap_or_default();
            // `@input` as the value moves, `@change` when the hand lets go,
            // as HTML has them for a range, both handed the number as
            // `event.value`, like every field. A tap that lands on the value
            // already held fires neither.
            //
            // Guarded on `event.x`, because a `@tap` is also what Space and
            // `tap()` run, and neither has a pointer to place the thumb at.
            let (input, change) = (block(&input), block(&change));
            node.on_tap = Some(format!(
                "if event.contains(\"x\") {{ let __before = {model}; {set}; \
                 if {model} != __before {{ let event = #{{ value: {model} }}; {input} {change} }} }}"
            ));
            node.gestures.push((
                rux_layout::Gesture::Drag,
                format!(
                    "let __before = {model}; let __end = event.phase == \"end\"; {set}; \
                     let event = #{{ value: {model} }}; \
                     if {model} != __before {{ {input} }} if __end {{ {change} }}"
                ),
            ));
        }
        node.field =
            rux_layout::Field { disabled, ..form_part.clone().unwrap_or_default() }.boxed();
        node.hidden = hidden;
        node.id = el.attr("id").map(str::to_string);
        node.state_path = state_path.clone();
        node.access = Access {
            role: AccessRole::Slider,
            label: authored_label(el),
            placeholder: None,
            checked: None,
            value: Some(rux_reactive::Value::Number(value).to_display()),
        };
        return node;
    }

    // A text input: shows the bound value (or a dim placeholder when empty). The
    // shell focuses it on tap and edits the bound signal on keystrokes.
    // `type="textarea"` is the same, but `Enter` inserts a newline.
    if el.tag == "input" {
        let mut style = style;
        let kind = InputKind::from_type(el.attr("type"));
        let multiline = kind.multiline();
        // An input with nothing bound to it is inert, and was inert in silence.
        //
        // `r-model` is not decoration: it is the whole of an input's identity.
        // The layout only makes a focus region for an input that has one
        // (`if let Some(model) = &node.model` in `rux-layout`), the shell tracks
        // focus by the model text, and the value the field shows is read back
        // out of that signal. Without one the box paints, the placeholder
        // renders, and a tap reaches nothing: no caret, no keystrokes, no value.
        //
        // Reported 2026-09-15 as "my input elements are uninteractive", against
        // two `<input placeholder="…" />` with no binding. Nothing in the file
        // was wrong to look at, and nothing anywhere said what was missing.
        if el.attr("r-model").is_none() {
            located(Some(el.line), || {
                error(
                    "this `<input>` has no `r-model`, so nothing can be typed into it: the \
                     caret, the keystrokes and the value it shows are all addressed by the \
                     signal it binds. Write `r-model=\"name\"` and declare `let name = \
                     signal(\"\")` in <script>"
                        .to_string(),
                )
            });
        }
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
        let secret = kind.secret();
        let placeholder = el.attr("placeholder").unwrap_or_default().to_string();
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
                    row: row.map(str::to_string),
                    instance: instance.map(str::to_string),
                    placeholder: placeholder.clone(),
                    color,
                    placeholder_color,
                    locals: locals.clone(),
                    deps,
                    secret,
                });
                v
            })
            .unwrap_or_default();
        // The same three-way choice as `eval_value_binding`, which handles every
        // keystroke after this one. Both are here rather than shared because the
        // build has the colours to hand and the patch has the binding; they must
        // agree, and a password shown in full for one frame is a real leak.
        let (shown, shown_color) = if value.is_empty() {
            (placeholder.clone(), placeholder_color)
        } else if secret {
            (mask(&value), color)
        } else {
            (value, color)
        };
        let text_child = LayoutNode::text(
            Style::default(),
            TextContent {
                text: shown,
                font_size,
                weight,
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
                shadows: shadows.clone(),
                look: look.clone(),
                selection_style: SelectionStyle {
                    handle: props.get("accent-color").and_then(|v| parse_color(v)),
                    ..selection
                },
                rich: None,
                white_space: Default::default(),
                // A field follows what is typed into it, Arabic or English,
                // unless it is told a direction of its own: `dir` on it, or a
                // `direction` rule that reaches it.
                direction: (props.contains_key("direction")
                    || el.attr("dir").is_some_and(|d| d.trim() != "auto"))
                .then_some(direction),
                isolate: Default::default(),
            },
        );
        let mut node = LayoutNode::new(style);
        node.children.push(text_child);
        node.model = model;
        // An input built inside a component has to carry which one: its model
        // names that instance's own state, and without this the value was read
        // and written in the document's scope, where the name does not exist.
        // The general element branch below has always set it; this one never
        // did, which is exactly the branch every `r-model` goes through.
        node.instance = instance.map(str::to_string);
        node.kind = kind;
        node.field =
            field_of(el, disabled, readonly, baked, form_part.clone().unwrap_or_default()).boxed();
        node.options = options;
        node.on_tap = on_tap;
        node.gestures = gestures;
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
            } else if kind == InputKind::Date {
                AccessRole::DateInput
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

    // A form is only a grouping: the fields below it belong to it, and what a
    // submission does is the `@submit` written on it.
    // (`@submit` anywhere else is refused by the runtime's attribute check.)
    let is_form = el.role().is_some_and(|r| r.eq_ignore_ascii_case("form"));
    // An ancestor's preceding siblings are copied only when a rule could read
    // them. The copy was the build's largest cost: row `i` of a list copied
    // the `i` rows before it, so a 300-row list copied 45,000 of them a build.
    let siblings = if KEEP_SIBLINGS.with(std::cell::Cell::get) { prev.to_vec() } else { Vec::new() };
    ancestors.push(AncNode { desc, prev: siblings.into(), place: prev.len() + 1, child_total: None, form: is_form.then(|| path.to_vec()) });
    let element_children = element_children(el);
    let child_inherited = Inherited {
        color,
        selection,
        font_size,
        font_family,
        vars: Rc::clone(&vars),
        run: None,
        direction,
        text: own_text,
        shadows: shadows.clone(),
        look: look.clone(),
    };
    let plain = instance.is_none() && slot.is_none() && outlet.is_none() && row.is_none();
    let (children, structural_deps) = build_children(
        &element_children,
        rules,
        comps,
        ancestors,
        &child_inherited,
        engine,
        locals,
        path,
        tpl_path,
        reg,
        state,
        instances,
        swaps,
        instance,
        // A `<slot>` deeper inside a component still fills from the same call
        // site, so the slot travels down with everything else. So does the
        // outlet: a `<router-view />` may sit any number of boxes deep in the
        // view that places it.
        slot,
        outlet,
        row,
    );
    // What its children were built from, so they can be built again alone.
    // Only in the document's own markup, and never inside a keyed row: a
    // row can be replayed from the cache at another place, which would leave
    // this stale. See [`rebuild_children`].
    let ctx = (plain && !structural_deps.is_empty()).then(|| {
        Rc::new(ListCtx {
            el: el as *const Element as usize,
            ancestors: ancestors.clone(),
            inherited: child_inherited.clone(),
            locals: locals.clone(),
        })
    });
    ancestors.pop();
    // If any child carried a structural directive, this parent can be reconciled
    // in place (rebuild just its children) on a change to those signals.
    if !structural_deps.is_empty() {
        reg.structural_parents.push(StructuralParent {
            tree_path: path.to_vec(),
            tpl_path: tpl_path.to_vec(),
            deps: structural_deps,
            ctx,
        });
    }

    let mut node = LayoutNode {
        style,
        text: None,
        image: None,
        path: None,
        tick: None,
        children,
        on_tap,
        gestures,
        on_key: None,
        model: None,
        kind: InputKind::Text,
        field: None,
        options: None,
        hidden,
        id: el.attr("id").map(str::to_string),
        label_for: el.attr("for").map(str::to_string),
        focus_model: None,
        state_path,
        focus_path: None,
        access: Access::default(),
        // Set when this node is inside a component, so a handler on it knows
        // whose state it is running against.
        instance: instance.map(str::to_string),
        // Filled in by the `r-for` expansion, which is the only place that knows
        // an element is a row and which item it stands for.
        key: None,
        // Baked with the row's locals, like a `@tap`: a form inside an `r-for`
        // submits for its own row.
        form: is_form.then(|| rux_layout::Form {
            on_submit: el.attr("@submit").map(|h| bind_locals(h, baked)),
            on_invalid: el.attr("@invalid").map(|h| bind_locals(h, baked)),
        }),
    };
    // A tappable box is a button, named by the text inside it, that is how
    // `<view @tap><text>Save</text></view>` announces as "Save, button". A
    // scroller is worth exposing so its content can be reached; anything else is
    // structure, and only appears if the author gave it a `role=`.
    let role = explicit_access_role(el).unwrap_or(if to.is_some() {
        AccessRole::Link
    } else if node.on_tap.is_some() {
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
    // The chain above the component tag.
    //
    // Handed in rather than started empty, because a document's rules style the
    // components it uses and half a cascade is worse than none: `.page` reached
    // a component's root and `.stage.slow .page` silently did not, so a rule
    // that looked like it applied simply never fired. Found by turning on the
    // router example's slow motion and watching a navigation run at full speed.
    caller_ancestors: &[AncNode],
    inherited: &Inherited,
    engine: &mut Engine,
    parent_locals: &Locals,
    path: &[usize],
    tpl_path: &[usize],
    reg: &mut BindingRegistry,
    state: &InteractionState,
    // The caller's stylesheet, for whatever it wrote between the tags.
    caller_rules: &[Rule],
    instances: &mut Instances,
    swaps: &mut Swaps,
    // The instance the *caller* is in, which is where a listener body belongs.
    caller: Option<&str>,
    row: Option<&str>,
    // Props resolved by the caller rather than written as attributes: the route
    // parameters a `<router>` captured. Empty for an ordinary component tag.
    extra_props: &Locals,
    // The path this was expanded under, when a `<router>` chose it. Recorded so
    // leaving the route can drop the instance's state.
    route: Option<&str>,
    // The rest of the matched route chain, when this *is* a route's view. An
    // ordinary component tag gets `None` and its `<router-view />`, if it wrote
    // one, says so.
    outlet: Option<Outlet>,
    // What the component is *called*, which is not always `el.tag`.
    //
    // For a route's view the element standing in for the component tag is the
    // `<route>` itself, so recording `el.tag` filed every route view under the
    // tag `route`. Everything an instance looks up afterwards is keyed by this:
    // its `mounted` / `unmounted` bodies, its `computed`s and its `effect`s. So
    // a route view silently had none of them, which is the kind of failure this
    // project keeps closing off.
    comp_tag: &str,
    // Which side of a swap this expansion is on. For an ordinary component tag
    // it is whatever the *caller's* element carries; for a route's view there is
    // no caller element and the view itself is the thing swapping, so the
    // pseudo-classes land on the component's own root either way.
    swap: Option<SwapSide>,
) -> LayoutNode {
    // An instance is touched, mounted and given state here, none of which a
    // reused row would do. See `RowCache`.
    UNREPLAYABLE.with(|u| u.set(u.get() + 1));
    let mut props: Locals = Vec::new();
    let mut prop_deps: HashSet<String> = HashSet::new();
    let mut listeners: Vec<(String, String)> = Vec::new();
    for rux_parser::Attr { name: key, value: expr, .. } in &el.attrs {
        // `@event="body"` is a listener, not a prop: the body is the caller's
        // code to run *later*, so it is carried as text and never evaluated
        // here. Evaluating it would run the caller's statements at build time,
        // once per build, which is the opposite of an event.
        if let Some(name) = key.strip_prefix('@') {
            listeners.push((name.to_string(), bind_locals(expr, handler_locals(parent_locals, caller, instances))));
            continue;
        }
        if let Some(name) = key.strip_prefix(':') {
            // Props are evaluated in the caller's scope and become the component's
            // only locals, a prop change re-expands this subtree (a reconcile).
            //
            // Written kebab on a tag, read snake in script, the same mapping
            // tags have: `:id-of` is `prop id_of`. A hyphenated name was
            // unreadable by any script, so nothing depended on it staying one.
            let (value, deps) = engine.eval_value_tracked(expr, parent_locals);
            prop_deps.extend(deps);
            if let Some(value) = value {
                props.push((name.replace('-', "_"), value));
            }
            continue;
        }
        // A plain attribute naming a declared prop passes its text, as HTML
        // would: `<stat label="Battery">`. Anything else on a component tag is
        // either a directive or an error the checker has already reported.
        //
        // Not on a `<route>`, which stands in for the tag of its view: its plain
        // attributes are its own (`path`, `name`), and a view declaring
        // `prop name` must not be handed the route's name. It passes props
        // only in the bound form.
        let name = key.replace('-', "_");
        if el.tag != "route" && component.props.iter().any(|p| p.name == name) {
            props.push((name, Value::Text(expr.clone())));
        }
    }
    // A prop is a boundary, so what it was given is held to what it declared,
    // in release builds too: a value from a route, from a file checked on its
    // own, or from an `any` never met the checker. One that does not fit is
    // reported and left out, so the default below takes its place. See
    // `docs/10-types.md`, "Props".
    let named = |n: &str| component.types.get(n).cloned();
    // The tag as the caller wrote it: `comp_tag` is the component's key, which
    // is a path, and a route stands in for the view it names.
    let written_tag = if el.tag == "route" { el.attr("view").unwrap_or("route") } else { el.tag.as_str() };
    props.retain(|(name, value)| {
        let Some(ty) = component.prop_type(name) else { return true };
        if rux_script::validate::fits(value, ty, &named) {
            return true;
        }
        let written = el.attrs.iter().find(|a| a.name.trim_start_matches(':').replace('-', "_") == *name);
        let shown = written.map(|a| a.name.clone()).unwrap_or_else(|| name.clone());
        located(Some(written.map_or(el.line, |a| a.line)), || {
            error(format!(
                "`<{written_tag}>` was given {} for `{shown}`, which is not the `{ty}` `prop {name}` \
                 takes, so it is built without it{}",
                describe_value(value),
                instead(component, name),
            ))
        });
        false
    });
    // A route parameter is always text, so it becomes what its prop takes:
    // `"42"` is `42` for a `prop id: int`. One that cannot be is left out, as
    // a prop that does not fit is.
    let mut from_route: Locals = Vec::new();
    for (name, value) in extra_props {
        let (Some(ty), Value::Text(text)) = (component.prop_type(name), value) else {
            from_route.push((name.clone(), value.clone()));
            continue;
        };
        let converted = rux_script::validate::from_text(text, ty, &named).or_else(|| {
            (text == ROUTE_CHECK_PARAM).then(|| rux_script::validate::sample(ty, &named)).flatten()
        });
        match converted {
            Some(v) => from_route.push((name.clone(), v)),
            // `rux check` visiting a pattern, with a type nothing stands in for:
            // left out, and nothing said about a segment nobody wrote.
            None if text == ROUTE_CHECK_PARAM => {}
            None => located(Some(el.line), || {
                error(format!(
                    "the route gave `<{written_tag}>` {} for `{name}`, which cannot be the `{ty}` \
                     `prop {name}` takes, so it is built without it{}",
                    describe_value(value),
                    instead(component, name),
                ))
            }),
        }
    }
    // A declared prop the caller left off takes its default. Evaluated with no
    // caller locals: it was written in the component, where a caller's `r-for`
    // variable means nothing, so only the document's names reach it. One the
    // caller owed and did not pass is left out: the checker has said so at
    // the tag, and reading it fails inside with the prop's name.
    for decl in &component.props {
        if props.iter().any(|(n, _)| *n == decl.name) || from_route.iter().any(|(n, _)| *n == decl.name) {
            continue;
        }
        if let Some(default) = &decl.default {
            let (value, deps) = engine.eval_value_tracked(default, &Locals::new());
            prop_deps.extend(deps);
            if let Some(value) = value {
                props.push((decl.name.clone(), value));
            }
        }
    }
    // Route parameters are already values, so they are appended rather than
    // evaluated. Last, so on a name collision the captured segment wins: it is
    // what the path actually says, and a `:prop` of the same name is more likely
    // a leftover than an override.
    props.extend(from_route);
    // Reconcile this component instance in place when a prop's signals change.
    if !prop_deps.is_empty() {
        reg.components.push(ComponentBinding {
            path: path.to_vec(),
            deps: prop_deps,
        });
    }

    // Whatever was written between the tags becomes this instance's slot
    // content. It stays in the caller's rules and the caller's scope: the
    // component decides *where* it goes, not what it means.
    let supplied = element_children(el);
    let slot = Slot { children: &supplied, locals: parent_locals, rules: caller_rules };

    // This instance's own state. Created the first time it is expanded, by
    // running the component's script in a scope of its own, and kept across
    // rebuilds: the tree is thrown away constantly, and a component's state
    // must not be.
    let key = instance_key(tpl_path, row);
    // Asked before the entry, because `or_insert_with` will not say afterwards
    // whether it ran, and "did this instance exist a moment ago" is exactly what
    // a mount is.
    let fresh = !instances.contains_key(&key);
    let entry = instances.entry(key.clone()).or_insert_with(|| Instance {
        tag: comp_tag.to_string(),
        // In the component's own scope, which its state carries from here on.
        state: engine.init_component(comp_tag, &component.script),
        ..Instance::default()
    });
    entry.touched = true;
    if fresh {
        MOUNTS.with(|m| {
            m.borrow_mut().push(Lifecycle {
                key: key.clone(),
                tag: comp_tag.to_string(),
                // Still in the map, so the runtime reads its scope from there
                // and sees whatever the rest of this build did to it.
                locals: Vec::new(),
            })
        });
    }
    entry.props = props.clone();
    // Listeners and the calling instance are the caller's, so like props they
    // are re-derived on every build rather than kept.
    entry.listeners = listeners;
    entry.caller = caller.map(str::to_string);
    entry.route = route.map(str::to_string);
    // State first, so a prop of the same name wins: the caller's input is more
    // specific than the component's own default.
    let mut locals: Locals = entry.state.clone();
    locals.extend(props);

    // The component expands in place at this element's path, so its root node
    // takes the same path; its bindings are recorded relative to it, and it
    // inherits the caller's ancestor chain so a descendant selector written
    // outside can reach it.
    let mut ancestors: Vec<AncNode> = caller_ancestors.to_vec();
    // From here down the lines being reported are the *component's* lines, so
    // the file has to change with them. Only from here: the props above were
    // written on the caller's element, in the caller's file, and belong to it.
    // Slot content is the caller's too, but it is built through this subtree,
    // so it is attributed to the component. That is a smaller error than the
    // one being fixed and closing it needs a position on the slot's nodes.
    // And from here down the *tags* are the component's too: `<task>` inside it
    // means whatever its own `use` says, which is not necessarily what the same
    // word means in the file that called it.
    in_file(component.file.clone(), || {
        in_namespace(comp_tag, || {
        build_node(
            &component.template,
            &component.rules,
            comps,
            &mut ancestors,
            &[],
            inherited,
            engine,
            &locals,
            path,
            tpl_path,
            reg,
            state,
            instances,
            swaps,
            Some(key.as_str()),
            Some(slot),
            // Only a route's own view is handed the rest of the chain; an
            // ordinary component tag is given `None` by its caller.
            outlet,
            // A component expanded inside a row is still inside that row.
            row,
            swap,
        )
        })
    })
}

/// Match a `<route path="…">` pattern against the current path, capturing the
/// `:name` segments as parameters.
///
/// `None` means the pattern does not apply. `Some(params)` means it does, with
/// whatever it captured (often nothing). Segment counts have to agree, because a
/// pattern is a whole path and not a prefix: matching on a prefix would make
/// `/` match every path there is, and the first route would always win.
///
/// A trailing slash is not a difference, so `/settings` and `/settings/` are one
/// path. Anyone typing a URL by hand will produce both.
fn match_route(pattern: &str, path: &str) -> Option<Locals> {
    fn segments(s: &str) -> Vec<&str> {
        s.split('/').filter(|p| !p.is_empty()).collect()
    }
    let pat = segments(pattern);
    let cur = segments(path);
    if pat.len() != cur.len() {
        return None;
    }
    let mut params: Locals = Vec::new();
    for (p, c) in pat.iter().zip(cur.iter()) {
        match p.strip_prefix(':') {
            // A parameter takes whatever sits in that position, under its name,
            // decoded: a value that had to be escaped to go into the URL has to
            // come back out as what it was, or a view is handed `a%2Fb`.
            Some(name) => params.push((
                name.to_string(),
                Value::Text(rux_script::percent_decode(c)),
            )),
            // A literal segment has to be exactly itself.
            None if p == c => {}
            None => return None,
        }
    }
    Some(params)
}

/// One link of a matched route chain: which `<route>` it is, where it sits
/// among its siblings, and what its own pattern captured.
struct Link<'a> {
    index: usize,
    route: &'a Element,
    params: Locals,
}

/// The `<route>` children of a `<route>` or a `<router>`, in written order.
fn child_routes(el: &Element) -> Vec<&Element> {
    element_children(el).into_iter().filter(|r| r.tag == "route").collect()
}

/// Say that `view` names nothing this document imported.
///
/// An error rather than a warning: a route whose view is not imported can never
/// render anything, which is the definition [`error`] carries. It used to be a
/// warning that only the matched route raised, so `rux check` exited 0 on a
/// document with a page in it that was already unreachable.
///
/// The suggested import is spelled in **snake**, always. A `use` path names a
/// file, `page-a` is not a path segment anyone can write, and pasting
/// `use components::page-a;` turns a report into a hard error looking for a
/// `page-a.rux` that no convention here would ever produce.
///
/// There used to be a second message here, for a `view` written in the import's
/// own spelling: it said "a view is named the way its tag is, write
/// `view=\"page-a\"`". That message is gone because the case it described is
/// no longer a mistake — see [`find_component`], which takes either spelling.
fn report_view_not_imported(view: &str) {
    error(format!(
        "<route> names the view `{view}`, which is not imported; add \
         `use components::{file};` to the script",
        file = view.replace('-', "_")
    ));
}

thread_local! {
    /// The document's `<router>`, cloned at the top of each build.
    ///
    /// Held here rather than passed down because a `to=` is read where an
    /// element is described, which is nowhere near the `<router>` and has no
    /// reason to know a document has one. `None` means no router in this
    /// document, which is the honest answer for a component being checked on
    /// its own and the reason [`warn_if_no_such_route`] says nothing then.
    ///
    /// The subtree itself rather than a list of patterns, so the check runs
    /// through [`match_router`] and cannot disagree with the router about what
    /// a path reaches. The first draft compared against a flattened list and
    /// called every link in `examples/router.rux` dead, because a list of
    /// patterns has no idea what `<route fallback>` is.
    static ROUTER: std::cell::RefCell<Option<Element>> = const { std::cell::RefCell::new(None) };
}

/// Say that `to` names a path this document's router does not reach.
///
/// A dead link is silent by construction: tapping it navigates, the router
/// matches nothing, and the page goes blank with no more explanation than an
/// empty screen. The address is written in the markup and so are the routes, so
/// the two can be compared before anyone taps.
///
/// A query or fragment is cut off first: `to="/search?q=rust"` is a link to
/// `/search`, and the router has never matched on anything past the `?`.
fn warn_if_no_such_route(to: &str) {
    let path = to.split(['?', '#']).next().unwrap_or(to);
    let reachable = ROUTER.with(|r| match r.borrow().as_ref() {
        Some(router) => match_router(router, path).is_some(),
        // Nothing to check against. A component holds links and no router of
        // its own, and reporting every one of them would be a check nobody
        // could leave switched on.
        None => true,
    });
    if !reachable {
        warn(format!(
            "`to=\"{to}\"` matches no <route> in this document, so tapping it would leave \
             the router with nothing to render"
        ));
    }
}

/// Check the `view` of every route in a router, not only the one that matched.
///
/// A route is expanded when its path is the one you are on, so until this
/// existed a `view` naming nothing was silent on every page but its own: the
/// document loaded, `rux check` called it clean, and the mistake surfaced the
/// first time somebody navigated there. Whether a name is imported is a fact
/// about the file rather than about where you are standing in it, so it is
/// answerable at load, and is answered here.
///
/// A route with no `view` at all is left alone: its children are what render,
/// and a leaf without one already has a message of its own in `expand_chain`.
fn check_route_views(routes: &[&Element], comps: &Components) {
    for route in routes {
        if let Some(view) = route.attr("view") {
            if !view.trim().is_empty() && find_component(comps, view).is_none() {
                located(route.attr_line("view"), || report_view_not_imported(view));
            }
        }
        check_route_views(&child_routes(route), comps);
    }
}

/// Match one level of routes against `rest`, and every level below it.
///
/// The chain returned consumes `rest` **entirely**: a route with no children has
/// to account for every segment itself, exactly as before, and a route with
/// children may take a prefix and hand what is left to them. A branch whose
/// children cannot finish the job fails, and the next sibling is tried, which is
/// what makes `/crew/x/y` fall through to a fallback rather than half-matching
/// `/crew/:id`.
///
/// A child pattern is **relative** unless it begins with `/`, which is
/// vue-router's rule and the reason a section can be moved by editing one line.
/// An absolute child is matched against `whole` instead.
///
/// Order decides, not specificity. Routes are written in one template here
/// rather than assembled from several files, so what is read top to bottom is
/// what happens; ranking would be a second system to hold in your head.
fn match_level<'a>(
    routes: &[&'a Element],
    rest: &[&str],
    whole: &[&str],
) -> Option<Vec<Link<'a>>> {
    for (index, route) in routes.iter().enumerate() {
        // A fallback carries no pattern; it is tried only after everything else.
        let Some(pattern) = route.attr("path") else { continue };
        let absolute = pattern.starts_with('/');
        let against: &[&str] = if absolute { whole } else { rest };
        let Some((params, taken)) = take_prefix(pattern, against) else { continue };
        let remainder = &against[taken..];

        let children = child_routes(route);
        let link = Link { index, route: *route, params };
        if children.is_empty() {
            // A leaf must land exactly, which is the rule as it has always been.
            if remainder.is_empty() {
                return Some(vec![link]);
            }
            continue;
        }
        if let Some(mut deeper) = match_level(&children, remainder, whole) {
            let mut chain = vec![link];
            chain.append(&mut deeper);
            return Some(chain);
        }
        // Nothing below took the rest. With nothing left to take, the parent
        // alone is the match: `/crew` renders the list with an empty outlet,
        // and an index child (`path=""`) is what fills it when there is one.
        if remainder.is_empty() {
            return Some(vec![link]);
        }
    }
    // Only now: a fallback at this level, which is what a path that matched a
    // parent but nothing under it lands on.
    routes.iter().enumerate().find(|(_, r)| r.attr("fallback").is_some()).map(|(index, r)| {
        vec![Link { index, route: *r, params: Locals::new() }]
    })
}

/// Match `pattern` against the front of `segments`, returning what it captured
/// and how many segments it used. An empty pattern is the index route and takes
/// nothing.
fn take_prefix(pattern: &str, segments: &[&str]) -> Option<(Locals, usize)> {
    let pat: Vec<&str> = pattern.split('/').filter(|p| !p.is_empty()).collect();
    if pat.len() > segments.len() {
        return None;
    }
    let mut params: Locals = Vec::new();
    for (p, c) in pat.iter().zip(segments.iter()) {
        match p.strip_prefix(':') {
            // A parameter takes whatever sits in that position, decoded: a value
            // that had to be escaped to go into the URL has to come back out as
            // what it was, or a view is handed `a%2Fb`.
            Some(name) => {
                params.push((name.to_string(), Value::Text(rux_script::percent_decode(c))))
            }
            None if p == c => {}
            None => return None,
        }
    }
    Some((params, pat.len()))
}

/// Split a path into its segments, dropping the empties a leading or trailing
/// slash leaves behind.
fn path_segments(s: &str) -> Vec<&str> {
    s.split('/').filter(|p| !p.is_empty()).collect()
}

/// The chain of `<route>` elements the router renders for `path`, if any.
fn match_router<'a>(router: &'a Element, path: &str) -> Option<Vec<Link<'a>>> {
    let segments = path_segments(path);
    let routes = child_routes(router);
    match_level(&routes, &segments, &segments)
}

/// Every parameter the whole matched chain captured, merged outermost first.
///
/// Merged rather than per-level, because a child view routinely needs the `:id`
/// its parent captured, and having to thread it down by hand is what makes
/// nested routes annoying everywhere else.
fn chain_params(chain: &[Link]) -> Locals {
    let mut merged: Locals = Vec::new();
    for link in chain {
        for (name, value) in &link.params {
            // Deeper wins on a collision: it is the more specific of the two,
            // and a name repeated down a chain is almost always a mistake worth
            // resolving in favour of the thing nearest the leaf.
            merged.retain(|(n, _)| n != name);
            merged.push((name.clone(), value.clone()));
        }
    }
    merged
}

/// Expand the first link of `chain`, handing the rest to whatever
/// `<router-view />` its view places.
///
/// Shared by the `<router>` branch and by `<router-view />` itself, because from
/// the second link down they are the same operation: render this view, and pass
/// on what is left.
#[allow(clippy::too_many_arguments)]
fn expand_chain(
    chain: &[Link],
    // See `Outlet::router_ns`.
    router_ns: &str,
    params: &Locals,
    current: &str,
    comps: &Components,
    // The chain above the `<router>`, so a rule written outside it can reach a
    // page. See `expand_component`.
    caller_ancestors: &[AncNode],
    inherited: &Inherited,
    engine: &mut Engine,
    locals: &Locals,
    path: &[usize],
    router_tpl: &[usize],
    reg: &mut BindingRegistry,
    state: &InteractionState,
    rules: &[Rule],
    instances: &mut Instances,
    swaps: &mut Swaps,
    instance: Option<&str>,
    row: Option<&str>,
    swap: Option<SwapSide>,
) -> Option<LayoutNode> {
    let link = chain.first()?;
    let Some(view) = link.route.attr("view") else {
        warn(format!(
            "<route path=\"{current}\"> has no `view`, so there is nothing to render for it"
        ));
        return None;
    };
    // Either spelling; `view` is what the author wrote and `name` is what the
    // component is registered as. See `find_component`.
    let Some((name, component)) = in_namespace(router_ns, || find_component(comps, view)) else {
        // Reported against the `view=` it was written on, which is also what
        // lets this share a message with the load-time pass over every route:
        // warnings dedupe by message, line and file, so the route you happen to
        // be standing on is not reported twice.
        located(link.route.attr_line("view"), || report_view_not_imported(view));
        return None;
    };
    // Identity is where the route sits in the template, one step per level, so
    // two views at different depths never collide and a view keeps its instance
    // while a sibling below it changes.
    let mut tpl = router_tpl.to_vec();
    tpl.push(link.index);

    let rest = &chain[1..];
    let used = std::cell::Cell::new(false);
    let outlet = Outlet { rest, params, current, router_tpl: &tpl, used: &used, router_ns };

    // The `<route>` element stands in for the component tag, so any `:prop`
    // written on it is passed through as well, and the captured parameters of
    // the *whole* chain join them.
    let node = expand_component(
        link.route, component, comps, caller_ancestors, inherited, engine, locals, path, &tpl, reg, state, rules,
        instances, swaps, instance, row, params, Some(current), Some(outlet), name, swap,
    );

    // A route with children whose view never places an outlet would drop that
    // child in silence, which is the failure this project keeps closing off.
    if !rest.is_empty() && !used.get() {
        warn(format!(
            "`{view}` has a nested <route> but no <router-view /> to put it in, so nothing under \
             it can render"
        ));
    }
    Some(node)
}

/// What the routes in `template` capture from `path`, without building anything.
///
/// The matched view already receives its parameters as props, and that is
/// enough for the view itself. It is not enough for anything *around* the
/// router: a title bar or a breadcrumb sits in the document's own layout, is
/// not the matched view, and so had no way to see the `id` in `/crew/grace`.
/// This is what backs the `params` signal, which fills that gap.
///
/// It has to run *before* the build rather than fall out of it, or `{{ params.id }}`
/// written outside the router would render one navigation behind.
///
/// The first `<router>` in the template wins. Nested routers are not built, and
/// when they are, the parameters of an inner one belong to it rather than to
/// the document.
pub fn route_params(template: &Element, path: &str) -> Vec<(String, Value)> {
    let Some(router) = find_router(template) else { return Vec::new() };
    // The whole chain's parameters, not just the level that matched first: a
    // breadcrumb outside the router needs the `:id` a child captured as much as
    // the view does.
    match_router(router, path).map(|chain| chain_params(&chain)).unwrap_or_default()
}

/// The guards standing between the document and `path`, outermost first, each
/// with the parameters its own level captured.
///
/// The `<router>`'s own guard runs on every navigation; a `<route>`'s runs when
/// that route is part of what matched, so a guard on a section covers every page
/// inside it without being written on each one. Outermost first, because a
/// section's guard is the coarser question ("may you be in this section at all")
/// and answering it second would mean running the finer one for a place you were
/// never going to reach.
///
/// A path that matches nothing has no route guards, only the router's. There is
/// nothing to protect, and the fallback view is the answer.
pub fn route_guards(template: &Element, path: &str) -> Vec<(String, Vec<(String, Value)>)> {
    let Some(router) = find_router(template) else { return Vec::new() };
    let mut out = Vec::new();
    if let Some(g) = router.attr("guard") {
        out.push((g.to_string(), Vec::new()));
    }
    let Some(chain) = match_router(router, path) else { return out };
    for depth in 0..chain.len() {
        let Some(g) = chain[depth].route.attr("guard") else { continue };
        // What this level and everything above it captured, so a guard on
        // `/crew/:id` can read `id` and decide about that member rather than
        // about the section.
        out.push((g.to_string(), chain_params(&chain[..=depth])));
    }
    out
}

/// The document's `<router>`, wherever in the template it was written.
fn find_router(el: &Element) -> Option<&Element> {
    if el.tag == "router" {
        return Some(el);
    }
    element_children(el).into_iter().find_map(find_router)
}

/// Every `<route name="…" path="…">` in the template, in the order written.
///
/// What `path_for("crew-detail", #{ id: "grace" })` builds a path from. A name
/// is worth having because a path is written in every link that leads to it,
/// and a URL scheme that can never be changed afterwards is not much of a
/// scheme.
pub fn named_routes(template: &Element) -> Vec<(String, String)> {
    let Some(router) = find_router(template) else { return Vec::new() };
    let mut out = Vec::new();
    collect_named(&child_routes(router), "", &mut out);
    out
}

/// Walk a level of routes, building each one's **full** pattern from its
/// ancestors' as it goes.
///
/// Without this a name on a child route is unusable: `path_for` would look up
/// `:id` and build `/grace`, which is not where anything lives. Naming the leaf
/// and letting the resolver walk up is how the name survives the section being
/// moved, which is the whole reason a route has a name rather than a path
/// written out in every link that leads to it.
fn collect_named(routes: &[&Element], prefix: &str, out: &mut Vec<(String, String)>) {
    for route in routes {
        let Some(pattern) = route.attr("path") else { continue };
        let full = if pattern.starts_with('/') {
            pattern.to_string()
        } else if pattern.is_empty() {
            // The index child stands for its parent's own path.
            prefix.to_string()
        } else {
            format!("{}/{}", prefix.trim_end_matches('/'), pattern.trim_start_matches('/'))
        };
        if let Some(name) = route.attr("name") {
            out.push((name.to_string(), full.clone()));
        }
        collect_named(&child_routes(route), &full, out);
    }
}

/// What a route parameter is filled with when `rux check` visits a pattern.
/// Here rather than beside the visit, because building has to recognise it:
/// a `prop id: int` given it is the checker's stand-in, not a bad segment, and
/// gets a stand-in number instead of an error.
pub const ROUTE_CHECK_PARAM: &str = "rux-check";

/// Every route the document declares, as the full path a navigation uses.
///
/// [`named_routes`] answers a different question (what `path_for("name")`
/// resolves to) and so lists only the routes that carry a `name`. A checker
/// needs all of them, named or not, because an unnamed route still has a view
/// and that view is still a file nobody has ever looked at.
///
/// A `<route fallback>` has no path of its own, so it is reported with an empty
/// one and the flag set: the only way to reach it is to go somewhere no other
/// route matches.
pub fn route_patterns(template: &Element) -> Vec<RoutePattern> {
    let Some(router) = find_router(template) else { return Vec::new() };
    let mut out = Vec::new();
    collect_patterns(&child_routes(router), "", &mut out);
    out
}

/// One entry of [`route_patterns`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePattern {
    /// The full pattern, ancestors included: `/task/:id`, not `:id`.
    pub pattern: String,
    /// `<route fallback>`: reached by going where nothing else matches.
    pub fallback: bool,
    /// The `view=` it renders, for saying whose problem a finding is.
    pub view: Option<String>,
}

fn collect_patterns(routes: &[&Element], prefix: &str, out: &mut Vec<RoutePattern>) {
    for route in routes {
        let view = route.attr("view").map(str::to_string);
        let Some(pattern) = route.attr("path") else {
            if route.attr("fallback").is_some() {
                out.push(RoutePattern { pattern: String::new(), fallback: true, view });
            }
            continue;
        };
        let full = if pattern.starts_with('/') {
            pattern.to_string()
        } else if pattern.is_empty() {
            prefix.to_string()
        } else {
            format!("{}/{}", prefix.trim_end_matches('/'), pattern.trim_start_matches('/'))
        };
        out.push(RoutePattern { pattern: full.clone(), fallback: false, view });
        collect_patterns(&child_routes(route), &full, out);
    }
}

/// Whether the router remembers where each page was scrolled to.
///
/// `<router restore-scroll="false">` turns it off; anything else, including no
/// attribute at all, leaves it on. On by default because remembering is what a
/// user expects from Back, and a document with no `<router>` is unaffected
/// either way.
pub fn restore_scroll(template: &Element) -> bool {
    find_router(template)
        .and_then(|r| r.attr("restore-scroll"))
        .is_none_or(|v| v.trim() != "false")
}

/// The hidden local that says where a row's loop variable came from:
/// `_rux_at_item = "items[2]"` beside `item`. See [`model_target`].
pub const ROW_PLACE: &str = "_rux_at_";

/// Where the rows of `r-for="… in coll"` live, as a path a write can land on,
/// or `None` when the collection is computed (`items.filter(…)`) and a row
/// has nowhere to be written back to.
///
/// A collection that starts with an outer row's loop variable is placed
/// through that row, so a list nested in a list writes into the real inner
/// list and not a copy of it.
/// An `r-key` that names a field of the loop variable (`item.id`), read off
/// the row's record rather than evaluated: the same value, shown the way
/// `eval_display` shows it. A script evaluation per row was most of what
/// planning a 300-row list cost (docs/11-next.md, track (g)). `None` for any
/// other key, and for a field the record does not have, which the engine
/// then reports as it always has.
fn field_key(expr: &str, var: &str, locals: &Locals) -> Option<String> {
    let (head, field) = expr.trim().split_once('.')?;
    let ident = |s: &str| {
        s.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
            && s.chars().all(|c| c.is_alphanumeric() || c == '_')
    };
    if head != var || !ident(field) {
        return None;
    }
    let (_, item) = locals.iter().rev().find(|(name, _)| name == var)?;
    let Value::Map(fields) = item else { return None };
    fields.iter().find(|(name, _)| name == field).map(|(_, value)| value.to_display())
}

fn row_place(coll: &str, locals: &Locals) -> Option<String> {
    let coll = coll.trim();
    let path = coll.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && coll.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '[' | ']'));
    if !path {
        return None;
    }
    let root_len = coll.find(['.', '[']).unwrap_or(coll.len());
    let root = &coll[..root_len];
    let marker = format!("{ROW_PLACE}{root}");
    match locals.iter().rev().find(|(n, _)| *n == marker) {
        Some((_, Value::Text(place))) => Some(format!("{place}{}", &coll[root_len..])),
        // An outer loop variable with no place of its own: its list was
        // computed, so this one has nowhere to go either.
        _ if locals.iter().any(|(n, _)| n == root) => None,
        _ => Some(coll.to_string()),
    }
}

/// What a field's `r-model` really writes to.
///
/// **A loop variable is a copy of its row**, so inside `r-for="item in
/// items"` the model `item.name` wrote to the copy, and every edit, typed,
/// pasted or cut, was thrown away on the next build without a word. Only
/// `items[i].name` worked, and nobody writes that: `v-model="item.name"` is
/// what Vue authors write and what Vue makes work. So a model that starts
/// with a loop variable is rewritten through the row's place in its list,
/// `items[2].name`, which is the real thing.
pub fn model_target(model: &str, locals: &[(String, Value)]) -> String {
    let model = model.trim();
    let root_len = model.find(['.', '[']).unwrap_or(model.len());
    let marker = format!("{ROW_PLACE}{}", &model[..root_len]);
    match locals.iter().rev().find(|(n, _)| *n == marker) {
        Some((_, Value::Text(place))) => format!("{place}{}", &model[root_len..]),
        _ => model.to_string(),
    }
}

/// Parse `r-for="item in items"` into `(binding, collection_expr)`.
///
/// Public because the checker needs the same answer: a loop variable is a name
/// in scope inside anything a row's handler calls, and a second parser for one
/// `split_once` would eventually disagree with this one about what `r-for`
/// binds.
pub fn parse_for(expr: &str) -> Option<(&str, &str)> {
    let (var, coll) = expr.split_once(" in ")?;
    Some((var.trim(), coll.trim()))
}

/// Say so when `r-for` was written with a destructuring or index form.
///
/// `r-for` binds exactly **one** name, and [`parse_for`] takes everything left
/// of ` in ` as that name. So `r-for="(pot, index) in pots"` bound a local
/// literally called `(pot, index)`, and both `pot` and `index` were then
/// undefined. What the author saw was the undefined-variable warning for
/// `index`, advising them to declare it in `<script>` as a signal, advice that
/// is useless to someone reaching for a loop index and never once said that the
/// tuple form is not supported.
///
/// The name is left bound as written rather than rejected, because the row
/// still has to build so the rest of the document can be checked. The warning
/// is the fix: it names what was actually bound, so the second warning about
/// the undefined name reads as a consequence instead of a mystery.
fn warn_if_destructuring_for(expr: &str, var: &str) {
    let looks_like_a_tuple = var.starts_with('(') || var.contains(',');
    if !looks_like_a_tuple {
        return;
    }
    let names: Vec<&str> = var
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect();
    let first = names.first().copied().unwrap_or("item");
    warn(format!(
        "`r-for=\"{expr}\"` binds one name and `{var}` is being taken as that whole name, \
         so none of {} exists inside the row. `r-for` has no index or destructuring form: \
         write `r-for=\"{first} in …\"` and reach the rest through `{first}`",
        names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
    ));
}

/// Build a sequence of element children, applying the structural directives
/// How an element declares that its appearance and disappearance are animated.
///
/// The declaration says where progress comes from and nothing else. What the
/// two sides *look* like is CSS, on `:enter-from` and `:leave-to`, because that
/// is where the rest of an element's appearance is written and a transform is
/// not a different kind of thing.
#[derive(Clone, Copy)]
enum SwapDecl<'a> {
    /// `r-transition`: the swap runs on the clock, for as long as the element's
    /// own `transition` declarations say. Nothing else to write.
    Clock,
    /// `:r-transition="expr"`: the author owns progress, `expr` yielding 0..=1
    /// and re-read every build. Reaching 1 commits and reaching 0 abandons, so
    /// a swap bound to a finger can change its mind, which is the whole reason
    /// the pair is live rather than a snapshot.
    Bound(&'a str),
}

fn swap_decl(el: &Element) -> Option<SwapDecl<'_>> {
    if let Some(expr) = el.attr(":r-transition") {
        return Some(SwapDecl::Bound(expr));
    }
    el.attr("r-transition").map(|_| SwapDecl::Clock)
}

/// How long the longest declared transition on this element runs, delay
/// included. This is what a clock-driven swap lasts, so that an author who
/// writes `transition: transform 300ms` gets a 300ms swap without saying so
/// twice and without the two ever disagreeing.
fn swap_duration(style: &Style) -> f32 {
    style.transitions.iter().map(|t| t.delay + t.duration).fold(0.0, f32::max)
}

/// Fill in a freshly opened swap's driver, and refresh a bound one's progress.
///
/// Split out of [`Swaps::resolve`] because it needs the *built* node: a clock
/// swap's duration is read from the computed style, which does not exist until
/// the element has been through the cascade.
fn arm_swap(
    swaps: &mut Swaps,
    key: &SwapKey,
    node: &mut LayoutNode,
    decl: Option<SwapDecl<'_>>,
    engine: &mut Engine,
    locals: &Locals,
    deps: &mut HashSet<String>,
) {
    let Some(decl) = decl else { return };
    // Give the element a stable identity for the animator.
    //
    // The animator names an unkeyed node by its position among its siblings, so
    // anything appearing or disappearing *above* a swapping element renames it,
    // and a rename is a first sight: the track is dropped, the element settles
    // wherever the build put it, and a drag in progress jumps. Reported from the
    // window as the drag working with the panel open and reverting to the old
    // behaviour with it closed, which is exactly a sibling coming and going.
    //
    // The swap key is already stable across all of that, being a template path,
    // so it is the identity to lend. Only when the element has none of its own:
    // an `r-for` row's `r-key` is a better answer and is set by the caller.
    if node.key.is_none() {
        let (path, row) = key;
        let mut id = String::from("swap:");
        for seg in path {
            id.push_str(&seg.to_string());
            id.push('.');
        }
        if let Some(row) = row {
            id.push_str(row);
        }
        node.key = Some(id);
    }
    match decl {
        SwapDecl::Bound(expr) => {
            // Read outside the borrow of `swaps`: evaluating can run author code.
            let (value, read) = engine.eval_value_tracked(expr, locals);
            deps.extend(read);
            let Some(progress) = value.and_then(|v| v.as_number()) else {
                // Not a number: the author is handing the swap back to the
                // clock, which is how a released finger settles. It runs out
                // its declared duration from wherever the drag left it, rather
                // than snapping to an end the finger never reached.
                let duration = swap_duration(&node.style);
                if let Some(swap) = swaps.pending.get_mut(key) {
                    match &mut swap.driver {
                        // Already running on the clock: leave the deadline
                        // alone, or every build would push the end further
                        // away and it would never arrive.
                        Driver::Clock { end_ms: Some(_), .. } => {}
                        // A swap that opened while the expression was already
                        // holding `null` is a clock swap nobody has told the
                        // duration to: a swap opens at zero and the caller
                        // fills it in, and this is that caller. Without this it
                        // runs for 0ms and the change simply appears. That is
                        // the shape an author writes when one binding serves
                        // both drivers, `:r-transition="progress"` with
                        // `progress` null until a finger touches it, which is
                        // how a page swap animates on a tap and follows a hand
                        // on a drag without declaring the swap twice.
                        Driver::Clock { duration: d, .. } => *d = duration,
                        Driver::Bound(_) => {
                            swap.driver = Driver::Clock { duration, end_ms: None };
                        }
                    }
                }
                node.style.swap_progress = None;
                return;
            };
            let progress = progress as f32;
            if let Some(swap) = swaps.pending.get_mut(key) {
                swap.progress = progress.clamp(0.0, 1.0);
                swap.moved |= swap.progress > 0.0;
                swap.driver = Driver::Bound(expr.to_string());
                // Onto the node here rather than through the next build: the
                // value was read this build, and a frame of lag in something a
                // finger is dragging is a frame of lag you can see.
                node.style.swap_progress = Some(swap.progress);
            }
        }
        SwapDecl::Clock => {
            let duration = swap_duration(&node.style);
            if let Some(swap) = swaps.pending.get_mut(key) {
                match &mut swap.driver {
                    // Already running: leave the deadline alone, or every build
                    // during the swap would push the end further away and it
                    // would never arrive.
                    Driver::Clock { end_ms: Some(_), .. } => {}
                    Driver::Clock { duration: d, .. } => *d = duration,
                    Driver::Bound(_) => {
                        swap.driver = Driver::Clock { duration, end_ms: None };
                    }
                }
            }
        }
    }
}

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
    instances: &mut Instances,
    swaps: &mut Swaps,
    instance: Option<&str>,
    slot: Option<Slot>,
    outlet: Option<Outlet>,
    row: Option<&str>,
) -> (Vec<LayoutNode>, HashSet<String>) {
    let mut out = Vec::new();
    // Signals read by structural directives at this level, returned so the parent
    // can register itself as reconcilable.
    let mut structural_deps: HashSet<String> = HashSet::new();
    // The identities of the rendered siblings so far, so `+`/`~` combinators can
    // see the elements preceding the one being built. (The synthetic `checked`
    // class is not reflected here, sibling combinators don't see checked state.)
    let mut prev: Vec<ElemDesc> = Vec::new();
    // `:last-child` and the rest that count from the end need to know how
    // many children there will be before the first is built and styled, so
    // they are counted first, only when a rule here asks.
    let from_end = rules.iter().chain(slot.iter().flat_map(|s| s.rules)).any(Rule::reads_total);
    if from_end {
        let total = count_rendered(elements, engine, locals, slot, outlet);
        if let Some(parent) = ancestors.last_mut() {
            parent.child_total = Some(total);
        }
    }
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
        if el.attr("r-key").is_some() && el.attr("r-for").is_none() {
            // A key with nothing to identify. Silently ignoring it would let
            // someone believe their list was keyed when it was not.
            located(el.attr_line("r-key"), || {
                warn(format!(
                    "`r-key` on <{}> does nothing without `r-for` on the same element",
                    el.tag
                ))
            });
        }
        // `<slot />` is where the caller's children land. It is not an element
        // of its own: it renders them (or its own children as a fallback) and
        // leaves nothing behind, so a component adds no wrapper box the author
        // did not write.
        if el.tag == "slot" {
            in_chain = false;
            // The caller's markup, read in the caller's scope, which a row in
            // here does not compare. See `RowCache`.
            UNREPLAYABLE.with(|u| u.set(u.get() + 1));
            let ctp = child_tpl(ti);
            let filled = slot.filter(|s| !s.children.is_empty());
            match filled {
                Some(s) => {
                    for (si, child) in s.children.iter().enumerate() {
                        let cp = child_path(&out);
                        let mut ctp = ctp.clone();
                        ctp.push(si);
                        // The caller's rules and the caller's scope: this markup
                        // was written out there, and reads what is in scope out
                        // there. `slot: None` inside, since a component's own
                        // `<slot>` is not a place to put the outer one's content.
                        out.push(build_node(
                            child, s.rules, comps, ancestors, &prev, inherited, engine, s.locals,
                            &cp, &ctp, reg, state, instances, swaps, instance, None, None, row, None,
                        ));
                        prev.push(ElemDesc::of(child));
                    }
                }
                None => {
                    // Nothing supplied: the slot's own children are the default,
                    // and they belong to the component, so they build in its
                    // rules and its scope.
                    if slot.is_none() {
                        warn(
                            "`<slot>` outside a component renders its own children and nothing \
                             else; only a component has a caller to take content from"
                                .to_string(),
                        );
                    }
                    for (si, child) in element_children(el).iter().enumerate() {
                        let cp = child_path(&out);
                        let mut ctp = ctp.clone();
                        ctp.push(si);
                        out.push(build_node(
                            child, rules, comps, ancestors, &prev, inherited, engine, locals, &cp,
                            &ctp, reg, state, instances, swaps, instance, slot, outlet, row, None,
                        ));
                        prev.push(ElemDesc::of(child));
                    }
                }
            }
            continue;
        }

        // `<router-view />` is where a nested route's view lands. Like `<slot>`
        // it leaves no box of its own, and like `<slot>` the content comes from
        // somewhere else: from the route matched *under* the one whose view this
        // is, rather than from the caller's markup.
        if el.tag == "router-view" {
            in_chain = false;
            UNREPLAYABLE.with(|u| u.set(u.get() + 1));
            match outlet {
                Some(o) => {
                    if let Some(link) = o.rest.first() {
                        o.used.set(true);
                        let cp = child_path(&out);
                        if let Some(node) = expand_chain(
                            o.rest, o.router_ns, o.params, o.current, comps, ancestors, inherited, engine, locals, &cp,
                            o.router_tpl, reg, state, rules, instances, swaps, instance, row,
                            // A nested outlet is not itself a swap; the swap, if
                            // any, belongs to the `<router>` that chose the chain.
                            None,
                        ) {
                            prev.push(ElemDesc::of(link.route));
                            out.push(node);
                        }
                    }
                    // Nothing left in the chain is ordinary, not a mistake: it
                    // is what `/crew` means when the list has children but the
                    // path names none of them. An index route (`path=""`) is how
                    // an author fills that.
                }
                None => warn(
                    "`<router-view />` renders the route matched below this one, and this is not \
                     a route's view, so there is nothing for it to show"
                        .to_string(),
                ),
            }
            continue;
        }

        // `<router>` renders the one `<route>` whose path matches, and nothing
        // else. Like `<slot>` it leaves no box of its own behind, so a router
        // adds nothing to the layout: the matched view expands in its place.
        if el.tag == "router" {
            in_chain = false;
            // Reading the route here is what subscribes the enclosing parent to
            // it, so navigating reconciles this subtree the way a changed `r-for`
            // collection does, rather than forcing a whole rebuild.
            let (value, deps) = engine.eval_value_tracked(rux_script::ROUTE_SIGNAL, locals);
            structural_deps.extend(deps);
            let current = value.map(|v| v.to_display()).unwrap_or_default();

            let routes = element_children(el);
            // Whose file this `<router>` is written in. A route's `view` is a
            // name in that file, and by the time a nested link renders, the
            // `<router-view />` placing it is several components deep in files
            // that have never heard of it. See `Outlet::router_ns`.
            let router_ns = CURRENT_FILE.with(|c| c.borrow().clone());
            // Every route's `view`, not just the matched one's: see
            // `check_route_views`.
            check_route_views(&child_routes(el), comps);
            for r in &routes {
                if r.tag != "route" {
                    warn(format!(
                        "<{}> inside <router> is ignored; a router's children are <route> \
                         elements",
                        r.tag
                    ));
                }
            }
            // The whole chain at once: the outermost view is rendered here and
            // each one below it is rendered by the `<router-view />` in the view
            // above. First match wins at every level, so routes are tried in the
            // order they are written and a fallback can sit anywhere among them.
            // A route transition is the enter/leave problem with the matched
            // path as the identity, so it is the same machinery rather than a
            // second one: the page being left is held on screen by a swap while
            // the page being entered builds beside it.
            let anim = swap_decl(el);
            let matched = match_router(el, &current);
            // **Which route matched, not which path.** Two paths that match the
            // same route (`/user/7` and `/user/12`) are one page showing
            // different data, so they update in place rather than swapping,
            // which is what every router does with a reused component. It also
            // avoids a collision that would otherwise be silent: instance
            // identity is the route's position in the template, so two
            // simultaneous expansions of the *same* route would share one
            // instance and one set of state.
            let here = matched.as_ref().map(|c| c[0].index.to_string());
            if anim.is_some() {
                // The outgoing pages first, so the incoming one paints over
                // them. A page is rebuilt from the path it was on, which is the
                // one thing remembered about it while it was current.
                let present: Vec<String> = here.iter().cloned().collect();
                for (_, old_key, old_locals) in swaps.departed_rows(&ctp, &present) {
                    let key: SwapKey = (ctp.clone(), Some(old_key));
                    let old_path = old_locals
                        .iter()
                        .find(|(n, _)| n == "path")
                        .map(|(_, v)| v.to_display())
                        .unwrap_or_default();
                    let Some(chain) = match_router(el, &old_path) else {
                        // The route it was on no longer matches, which a hot
                        // reload can do mid-swap. Let it go rather than hold a
                        // page nothing can build.
                        swaps.finish(&key);
                        continue;
                    };
                    let Some(side) = swaps.resolve(&key, false, true) else { continue };
                    let params = chain_params(&chain);
                    let cp = child_path(&out);
                    if let Some(mut node) = expand_chain(
                        &chain, &router_ns, &params, &old_path, comps, ancestors, inherited, engine, locals, &cp, &ctp,
                        reg, state, rules, instances, swaps, instance, row, side,
                    ) {
                        arm_swap(swaps, &key, &mut node, anim, engine, locals, &mut structural_deps);
                        prev.push(ElemDesc::of(chain[0].route));
                        out.push(node);
                    }
                }
            }
            match matched {
                Some(chain) => {
                    let params = chain_params(&chain);
                    let cp = child_path(&out);
                    let key: SwapKey = (ctp.clone(), here);
                    let side = if anim.is_some() {
                        // Remember the path this page is on, so that when it
                        // stops being current there is still something to
                        // rebuild it from.
                        swaps.remember_row(
                            &key,
                            0,
                            vec![("path".to_string(), Value::Text(current.clone()))],
                        );
                        swaps.resolve(&key, true, true).flatten()
                    } else {
                        None
                    };
                    if let Some(mut node) = expand_chain(
                        &chain, &router_ns, &params, &current, comps, ancestors, inherited, engine, locals, &cp, &ctp,
                        reg, state, rules, instances, swaps, instance, row, side,
                    ) {
                        if anim.is_some() {
                            arm_swap(
                                swaps, &key, &mut node, anim, engine, locals,
                                &mut structural_deps,
                            );
                        }
                        prev.push(ElemDesc::of(chain[0].route));
                        out.push(node);
                    }
                }
                None => {
                    // Every path should render something. A router with nothing
                    // to show is far more likely to be a missing route than an
                    // intended blank screen.
                    warn(format!(
                        "no <route> matches `{current}`, and there is no `fallback` route, so \
                         the router rendered nothing"
                    ));
                }
            }
            continue;
        }

        // r-for expands the element once per collection item; it ends any chain.
        // The collection is a structural read, a change re-diffs the list.
        if let Some(for_expr) = el.attr("r-for") {
            in_chain = false;
            if let Some((var, coll)) = parse_for(for_expr) {
                // A directive is read by the *parent's* loop, so without this it
                // would be reported on the parent's line. It belongs on the line
                // the directive was written on.
                let at = el.attr_line("r-for");
                rux_script::located(at, || warn_if_destructuring_for(for_expr, var));
                // `r-if` on the same element as `r-for` is read by nobody: the
                // loop expands the element and the condition is never consulted,
                // so every row renders and the filter silently does nothing.
                // Said out loud rather than honored, because which one wins is a
                // real design question (Vue has answered it both ways across two
                // major versions) and quietly picking an answer here would change
                // what existing documents render.
                for name in ["r-if", "r-elif", "r-else", "r-show"] {
                    if el.attr(name).is_some() {
                        rux_script::located(el.attr_line(name).or(at), || {
                            warn(format!(
                                "`{name}` on the same element as `r-for` does nothing: the loop \
                                 expands the element and the condition is never read, so every \
                                 row renders. Filter the collection instead, with a `computed`, \
                                 or put the `{name}` on a child."
                            ))
                        });
                    }
                }
                // The collection is a reconcilable read, not a force-rebuild one:
                // it flows to the parent's structural deps (via the return), not to
                // `reg.structural`.
                let (value, deps) =
                    rux_script::located(at, || engine.eval_value_tracked(coll, locals));
                structural_deps.extend(deps);
                // Taken, not copied: the value is this build's own.
                let items = value.and_then(|v| match v {
                    Value::List(items) => Some(items),
                    _ => None,
                });
                if let Some(items) = items {
                    // `r-key` names what a row *is*, so the runtime can follow it
                    // when the list reorders instead of tracking slots.
                    let key_expr = el.attr("r-key");
                    let anim = swap_decl(el);
                    if anim.is_some() && key_expr.is_none() {
                        // Without a key there is no identity to hold a departing
                        // row by, and no way to tell a removal from a reorder.
                        // Animating anyway would animate whichever row happened
                        // to land in the vacated slot.
                        warn(format!(
                            "`r-transition` on <{}> needs `r-key` on the same element: a row \
                             that has left the collection can only be held on screen if there \
                             is something to identify it by",
                            el.tag
                        ));
                    }
                    let animated = anim.is_some() && key_expr.is_some();
                    let mut seen_keys: Vec<String> = Vec::new();
                    // The same keys as a set, asked once a row: `seen_keys`
                    // scanned was the square of the list's length.
                    let mut seen: HashSet<String> = HashSet::new();
                    // The key is evaluated *before* the row is built, because
                    // everything inside the row is identified by it: an input's
                    // focus, its `:focus` styling and the binding that reads its
                    // value all need to know which row they are in while they
                    // are being recorded.
                    let mut plan: Vec<(Option<String>, Locals)> = Vec::new();
                    let place = row_place(coll, locals);
                    for (index, item) in items.into_iter().enumerate() {
                        let mut child_locals = locals.clone();
                        child_locals.push((var.to_string(), item));
                        if let Some(place) = &place {
                            child_locals
                                .push((format!("{ROW_PLACE}{var}"), Value::Text(format!("{place}[{index}]"))));
                        }
                        let key = key_expr.map(|expr| {
                            let key = field_key(expr, var, &child_locals)
                                .unwrap_or_else(|| engine.eval_display(expr, &child_locals));
                            if key.is_empty() {
                                warn(format!(
                                    "`r-key=\"{expr}\"` evaluated to nothing on one row, so that \
                                     row has no identity and will be treated as a new one"
                                ));
                            } else if seen.contains(&key) {
                                // Two rows claiming one identity is worse than
                                // none: whatever follows a key follows the wrong
                                // row, silently.
                                warn(format!(
                                    "`r-key=\"{expr}\"` produced the duplicate key `{key}`; keys \
                                     must be unique within a list, or rows cannot be told apart"
                                ));
                            } else {
                                seen.insert(key.clone());
                                // Only an animated list asks which rows left.
                                if animated {
                                    seen_keys.push(key.clone());
                                }
                            }
                            key
                        });
                        plan.push((key, child_locals));
                    }
                    // Rows the collection has dropped are spliced back in at the
                    // position they held, not appended: a row leaving from the
                    // middle of a list has to animate out where it was.
                    if animated {
                        for (index, key, row_locals) in swaps.departed_rows(&ctp, &seen_keys) {
                            let at = index.min(plan.len());
                            plan.insert(at, (Some(key), row_locals));
                        }
                    }
                    out.reserve(plan.len());
                    // The local a row's place is written in, the same for every row.
                    let place_name = place.as_ref().map(|_| format!("{ROW_PLACE}{var}"));
                    // Every row is this element, so it is described once, and
                    // only kept as a sibling when a rule can read siblings: no
                    // `+` or `~` anywhere, and nothing reads `prev` at all.
                    let siblings_read = SIBLING_STEP.with(std::cell::Cell::get)
                        || KEEP_SIBLINGS.with(std::cell::Cell::get);
                    let desc = siblings_read.then(|| ElemDesc::of(el));
                    if siblings_read {
                        prev.reserve(plan.len());
                    }
                    for (index, (key, child_locals)) in plan.into_iter().enumerate() {
                        let skey: Option<SwapKey> = animated.then(|| (ctp.clone(), key.clone()));
                        // In the collection iff its key survived the diff; a row
                        // that is only in the plan because it was spliced back in
                        // is the one that is leaving.
                        let wanted = key.as_ref().is_some_and(|k| seen.contains(k));
                        let side = if let Some(skey) = &skey {
                            match swaps.resolve(skey, wanted, true) {
                                Some(side) => side,
                                // Its leaving swap has committed; the row goes.
                                None => continue,
                            }
                        } else {
                            None
                        };
                        if let Some(skey) = skey.as_ref().filter(|_| wanted) {
                            swaps.remember_row(skey, index, child_locals.clone());
                        }
                        let cp = child_path(&out);
                        // Only a keyed row can be found again, and an animated
                        // one is a swap's. See `RowCache`.
                        let reuse = key.as_ref().filter(|_| !animated).map(|k| {
                            let id: RowId = (
                                *el as *const Element as usize,
                                instance.map(str::to_string),
                                row.map(str::to_string),
                                k.clone(),
                            );
                            (id, place_name.clone())
                        });
                        let mut node = build_row(
                            el, rules, comps, ancestors, &prev, inherited, engine, &child_locals,
                            &cp, &ctp, reg, state, instances, swaps, instance, slot, outlet,
                            // An unkeyed row inherits whatever row it is nested
                            // in, which is normally nothing.
                            key.as_deref().or(row),
                            side,
                            reuse,
                        );
                        if let Some(skey) = &skey {
                            arm_swap(
                                swaps, skey, &mut node, anim, engine, &child_locals,
                                &mut structural_deps,
                            );
                        }
                        node.key = key;
                        out.push(node);
                        if let Some(desc) = &desc {
                            prev.push(desc.clone());
                        }
                    }
                }
            }
            continue;
        }

        // r-if / r-elif conditions are structural reads too.
        //
        // Each branch of a chain asks `Swaps` what to do rather than acting on
        // its condition directly, because a branch that has just become false
        // still has to be built while it animates out. Without `r-transition`
        // the answer is always the condition itself.
        if let Some(cond) = el.attr("r-if") {
            in_chain = true;
            let (v, deps) = rux_script::located(el.attr_line("r-if"), || {
                engine.eval_bool_tracked(cond, locals)
            });
            structural_deps.extend(deps);
            chain_satisfied = v;
            let key: SwapKey = (ctp.clone(), row.map(str::to_string));
            let anim = swap_decl(el);
            if let Some(side) = swaps.resolve(&key, chain_satisfied, anim.is_some()) {
                let cp = child_path(&out);
                let mut node = build_node(el, rules, comps, ancestors, &prev, inherited, engine, locals, &cp, &ctp, reg, state, instances, swaps, instance, slot, outlet, row, side);
                arm_swap(swaps, &key, &mut node, anim, engine, locals, &mut structural_deps);
                out.push(node);
                prev.push(ElemDesc::of(el));
            }
            continue;
        }
        if let Some(cond) = el.attr("r-elif") {
            let taken = if in_chain && !chain_satisfied {
                let (v, deps) = rux_script::located(el.attr_line("r-elif"), || {
                    engine.eval_bool_tracked(cond, locals)
                });
                structural_deps.extend(deps);
                v
            } else {
                false
            };
            chain_satisfied |= taken;
            let key: SwapKey = (ctp.clone(), row.map(str::to_string));
            let anim = swap_decl(el);
            if let Some(side) = swaps.resolve(&key, taken, anim.is_some()) {
                let cp = child_path(&out);
                let mut node = build_node(el, rules, comps, ancestors, &prev, inherited, engine, locals, &cp, &ctp, reg, state, instances, swaps, instance, slot, outlet, row, side);
                arm_swap(swaps, &key, &mut node, anim, engine, locals, &mut structural_deps);
                out.push(node);
                prev.push(ElemDesc::of(el));
            }
            continue;
        }
        if el.attr("r-else").is_some() {
            let taken = in_chain && !chain_satisfied;
            let key: SwapKey = (ctp.clone(), row.map(str::to_string));
            let anim = swap_decl(el);
            if let Some(side) = swaps.resolve(&key, taken, anim.is_some()) {
                let cp = child_path(&out);
                let mut node = build_node(el, rules, comps, ancestors, &prev, inherited, engine, locals, &cp, &ctp, reg, state, instances, swaps, instance, slot, outlet, row, side);
                arm_swap(swaps, &key, &mut node, anim, engine, locals, &mut structural_deps);
                out.push(node);
                prev.push(ElemDesc::of(el));
            }
            in_chain = false;
            continue;
        }

        // A plain element ends any active chain.
        in_chain = false;
        let cp = child_path(&out);
        out.push(build_node(el, rules, comps, ancestors, &prev, inherited, engine, locals, &cp, &ctp, reg, state, instances, swaps, instance, slot, outlet, row, None));
        if el.tag != rux_parser::RUN_TAG {
            prev.push(ElemDesc::of(el));
        }
    }
    (out, structural_deps)
}

/// How many elements `build_children` will render from `elements`, counted
/// the way it counts them into `prev`: the branch an `r-if` chain takes, each
/// row of an `r-for`, what a `<slot>` is filled with, the route a `<router>`
/// or `<router-view />` shows, and every other element but a run of text.
///
/// The conditions and collections are evaluated here and again when the
/// children are built, which is why this runs only when a rule counts from
/// the end. A row or a page held on screen while it animates out is not
/// counted, so for the length of a leave transition `:last-child` is the
/// last of the ones staying.
fn count_rendered(
    elements: &[&Element],
    engine: &mut Engine,
    locals: &Locals,
    slot: Option<Slot>,
    outlet: Option<Outlet>,
) -> usize {
    let mut n = 0;
    let (mut in_chain, mut satisfied) = (false, false);
    for el in elements {
        if el.tag == "slot" {
            in_chain = false;
            n += match slot.filter(|s| !s.children.is_empty()) {
                Some(s) => s.children.len(),
                None => element_children(el).len(),
            };
        } else if el.tag == "router-view" {
            in_chain = false;
            n += outlet.is_some_and(|o| !o.rest.is_empty()) as usize;
        } else if el.tag == "router" {
            in_chain = false;
            let current = engine
                .eval_value_tracked(rux_script::ROUTE_SIGNAL, locals)
                .0
                .map(|v| v.to_display())
                .unwrap_or_default();
            n += match_router(el, &current).is_some() as usize;
        } else if let Some(for_expr) = el.attr("r-for") {
            in_chain = false;
            if let Some((_, coll)) = parse_for(for_expr) {
                if let (Some(Value::List(items)), _) = engine.eval_value_tracked(coll, locals) {
                    n += items.len();
                }
            }
        } else if let Some(cond) = el.attr("r-if") {
            in_chain = true;
            satisfied = engine.eval_bool_tracked(cond, locals).0;
            n += satisfied as usize;
        } else if let Some(cond) = el.attr("r-elif") {
            let taken = in_chain && !satisfied && engine.eval_bool_tracked(cond, locals).0;
            satisfied |= taken;
            n += taken as usize;
        } else if el.attr("r-else").is_some() {
            n += (in_chain && !satisfied) as usize;
            in_chain = false;
        } else {
            in_chain = false;
            n += (el.tag != rux_parser::RUN_TAG) as usize;
        }
    }
    n
}

// ── Reusing list rows ───────────────────────────────────────────────────────

thread_local! {
    /// Bumped by everything a build does that a reused row would skip: an
    /// instance expanded, a swap consulted, a slot or an outlet filled. A row
    /// whose build moved it is never kept.
    static UNREPLAYABLE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// Bumped each time a handler is baked with the locals in scope. A row
    /// that baked one carries its place in its list inside a string, so it
    /// can only be reused where it was.
    static BAKED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// The rows the build now running may reuse and the ones it keeps. `None`
    /// outside [`build_styled_tree_cached`], so every other build is untouched.
    static ROWS: std::cell::RefCell<Option<BuildRows>> = const { std::cell::RefCell::new(None) };
    /// Whether the last whole build's rules had a sibling combinator.
    static SIBLING_STEP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The keyed rows of `r-for` lists, kept from one build for the next.
///
/// A reconcile builds the whole document to splice in the one list that
/// changed, so a list of 300 rows restyled all 300 when one moved. A row is
/// its template element, its loop variables, its ancestors, what it
/// inherited and the signals it read, and when none of those changed it
/// comes out the same, so it is taken from here instead of built.
///
/// **Everything a row depends on is compared, nothing is assumed.** Its loop
/// variables and ancestors by value, the signals it read by their values now
/// against their values then. What it cannot compare it refuses: a row that
/// warned, printed, emitted or failed a binding, expanded a component,
/// consulted a swap or filled a slot is never kept, since reusing it would
/// skip that; so is a row the pointer or focus is in, and so is every row of
/// a build with a sibling combinator in its rules, since a row's siblings are
/// not part of what is compared.
///
/// **Debug builds prove it.** A build that reused anything is built again
/// reusing nothing, and any difference panics, so every test that reconciles
/// a keyed list checks it.
///
/// **A row may move.** Its registry entries and the paths it carries are
/// rewritten to where it now is, and so is its place in its list, the hidden
/// local an `r-model` writes through. Its handlers are the exception: they
/// were baked with the place inside a string, so a row that has one is reused
/// only where it was.
///
/// Owned by the runtime, like [`Instances`]. Cleared by anything that changes
/// what a row is built from without a signal saying so: a stylesheet, a
/// component, the environment. Measured on a 300-row list moving one row a
/// frame: see `docs/05-as-built.md`.
#[derive(Default)]
pub struct RowCache {
    rows: RowMap,
    env: Option<Environment>,
    /// What the last whole build found about its rules, which a build of
    /// one list must repeat: `KEEP_SIBLINGS`, and whether any rule has a
    /// sibling combinator. `None` until a whole build has run.
    shape: Option<(bool, bool)>,
    /// The document's rules as the last build of one list parsed them, with
    /// what they were parsed from ([`rules_print`]) and for which environment:
    /// parsing them for every reconcile was 0.1 to 0.17 ms of a 300-row list's
    /// (track (g).7a).
    rules: Option<(u64, Environment, Vec<Rule>)>,
    /// How many rows the last build reused.
    pub hits: usize,
    /// How many keyed rows the last build had to build.
    pub misses: usize,
}

impl RowCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget every row, so the next build builds them all.
    pub fn clear(&mut self) {
        self.rows.clear();
        self.shape = None;
        self.rules = None;
    }

    /// Take what the whole build just finished, with no rows kept, found
    /// about its rules, so the next reconcile can build one list alone
    /// ([`rebuild_children`]) without waiting for a cached build first.
    pub fn after_whole_build(&mut self, env: Environment) {
        if self.env != Some(env) {
            self.rows.clear();
            self.env = Some(env);
        }
        self.shape = Some((KEEP_SIBLINGS.with(std::cell::Cell::get), SIBLING_STEP.with(std::cell::Cell::get)));
    }
}

/// A row's identity: its template element by address, the instance and the
/// outer row it is in, and its `r-key`.
type RowId = (usize, Option<String>, Option<String>, String);

/// The rows a build keeps, boxed: a row holds its node and registry
/// fragment, several kilobytes, and a map of them inline moved every one each
/// time it grew, 1.4 of a 300-row list's 4.9 ms reconcile (track (g).7a).
type RowMap = HashMap<RowId, Box<CachedRow>>;

/// One row as it was built, and everything that built it.
struct CachedRow {
    node: LayoutNode,
    /// Where `node` and `frag` were built, which their paths are relative to.
    path: Vec<usize>,
    /// Where the row is now, in the tree the last build made: where a build
    /// of one list finds it to adopt it. See [`LiveRow`].
    at: Vec<usize>,
    /// The row itself, taken from the live tree, during a build of one list.
    live: Option<LiveRow>,
    /// The registry entries the row's build recorded, at `path`.
    frag: BindingRegistry,
    /// The locals, without the place.
    locals: Locals,
    place: Option<Value>,
    baked: bool,
    ancestors: Vec<AncNode>,
    inherited: Inherited,
    /// Every signal the row read, with its value then.
    reads: Vec<(String, Option<Value>)>,
    /// Whether any locals `frag` captured hold a place other than the row's
    /// own: a list nested in the row, whose places are built from this one.
    /// Worked out once when the row is kept, since `frag` never changes;
    /// asked of every moved row on every reuse it was a tenth of a 300-row
    /// list's build (track (g).7b).
    other_place: bool,
}

struct BuildRows {
    old: RowMap,
    kept: RowMap,
    /// Each signal's value, read once per build however many rows ask.
    values: HashMap<String, Option<Value>>,
    off: bool,
    hits: usize,
    misses: usize,
}

/// A row of the live tree, taken out of it to be put back where the build
/// now wants it: its node, and the index it had under its parent, which is
/// where its registry entries are found by [`merge_rows`].
///
/// A reused row used to be copied out of the cache, node and registry
/// entries, 1.9 of a 300-row list's 5 ms (docs/11-next.md, track (g)).
/// A build of one list has the rows it is replacing in hand, and a row that
/// is reused is the same row, so it is moved instead.
struct LiveRow {
    node: LayoutNode,
    from: usize,
}

/// [`build_styled_tree_stateful`], reusing the rows `cache` kept from the
/// last build and keeping this build's for the next. See [`RowCache`].
#[allow(clippy::too_many_arguments)]
pub fn build_styled_tree_cached(
    sfc: &Sfc,
    components: &HashMap<String, Sfc>,
    namespaces: &HashMap<String, Namespace>,
    engine: &mut Engine,
    instances: &mut Instances,
    swaps: &mut Swaps,
    state: &InteractionState,
    env: Environment,
    cache: &mut RowCache,
) -> Result<(LayoutNode, BindingRegistry), String> {
    if cache.env != Some(env) {
        cache.rows.clear();
        cache.env = Some(env);
    }
    let old = std::mem::take(&mut cache.rows);
    ROWS.with(|r| {
        *r.borrow_mut() = Some(BuildRows {
            kept: HashMap::with_capacity(old.len()),
            old,
            values: HashMap::new(),
            // A tried submission marks fields by their form's path, which
            // the rows do not compare. Rare, and short-lived.
            off: !state.attempted.is_empty(),
            hits: 0,
            misses: 0,
        })
    });
    let out = build_styled_tree_stateful(sfc, components, namespaces, engine, instances, swaps, state, env);
    cache.shape = out.is_ok().then(|| {
        (KEEP_SIBLINGS.with(std::cell::Cell::get), SIBLING_STEP.with(std::cell::Cell::get))
    });
    if let Some(built) = ROWS.with(|r| r.borrow_mut().take()) {
        cache.rows = built.kept;
        cache.hits = built.hits;
        cache.misses = built.misses;
    }
    // Debug builds prove every reuse the way `Engine::tracking` proves its
    // writes: the same build again, reusing nothing, must come out the same,
    // and the test suite is what drives it. Skipped mid-swap, where a second
    // build would advance the swap and differ for that reason alone.
    #[cfg(debug_assertions)]
    if cache.hits > 0 && swaps.is_empty() {
        if let Ok((node, reg)) = &out {
            let mut instances = instances.clone();
            let mut swaps = swaps.clone();
            let plain = rux_script::without_effects(|| {
                build_styled_tree_stateful(sfc, components, namespaces, engine, &mut instances, &mut swaps, state, env)
            });
            if let Ok((plain, plain_reg)) = plain {
                assert_eq!(
                    format!("{node:#?}"),
                    format!("{plain:#?}"),
                    "a reused `r-for` row came out different from building it"
                );
                assert_eq!(
                    registry_digest(reg),
                    registry_digest(&plain_reg),
                    "a reused `r-for` row recorded different bindings from building it"
                );
            }
        }
    }
    out
}

/// One parent's children built again on their own, and what that build
/// recorded. See [`rebuild_children`].
pub struct Rebuilt {
    pub children: Vec<LayoutNode>,
    /// The registry entries of everything under the parent, at their paths.
    pub reg: BindingRegistry,
    /// What the parent's structural directives read now.
    pub deps: HashSet<String>,
}

/// Build the children of the reconcilable `parent` again, without building
/// the rest of the document, reusing rows from `cache` as a whole build does.
///
/// A reconcile used to build the whole document to splice in the one list
/// that changed: 5 of a 300-row list's 7 ms (docs/11-next.md, track (g)).
/// This is the same build started at the parent, from what the parent's
/// children were built from last time ([`ListCtx`]).
///
/// `None`, with nothing done, when that would not come out as a whole build
/// does, and the caller builds the whole document as before: no context kept
/// (the parent is in a component, a slot, a route view or a keyed row), no
/// whole build yet since the cache was cleared, a swap pending, the template
/// path no longer finding the same element, or anything under the parent
/// that a whole build does more for: a component, a `<slot>`, a router or
/// a route, a `<router-view>`, an `r-transition`, or a `for=` label. The caller answers
/// for what it knows better: the rest of the tree has no `for=` label, and
/// nothing else is to be reconciled with it.
///
/// Debug builds of the runtime check every one against a whole build.
#[allow(clippy::too_many_arguments)]
pub fn rebuild_children(
    sfc: &Sfc,
    namespaces: &HashMap<String, Namespace>,
    engine: &mut Engine,
    instances: &mut Instances,
    swaps: &mut Swaps,
    state: &InteractionState,
    env: Environment,
    cache: &mut RowCache,
    parent: &StructuralParent,
    // The parent's children as they are, and their registry entries
    // ([`BindingRegistry::take_under`]): a row that is reused is taken from
    // here rather than copied out of the cache. See [`LiveRow`].
    live_children: Vec<LayoutNode>,
    live_reg: BindingRegistry,
) -> Option<Rebuilt> {
    let el = rebuild_target(sfc, namespaces, swaps, env, cache, parent)?;
    let ctx = parent.ctx.as_ref()?;
    let (keep_siblings, sibling_step) = cache.shape?;

    // The registers a whole build sets up, as it sets them, except the
    // shared interpretations, which depend only on declarations and are kept.
    let print = rules_print(sfc);
    let rules = match cache.rules.take() {
        Some((p, e, rules)) if p == print && e == env => rules,
        _ => rux_script::profile::time(rux_script::profile::Phase::Css, || parse_document_rules(sfc, env)),
    };
    set_build_env(env);
    register_properties(sfc);
    ROUTER.with(|r| *r.borrow_mut() = find_router(&sfc.template).cloned());
    NAMESPACES.with(|n| *n.borrow_mut() = namespaces.clone());
    CURRENT_FILE.with(|c| *c.borrow_mut() = DOCUMENT_NAMESPACE.to_string());
    KEEP_SIBLINGS.with(|k| k.set(keep_siblings));
    SIBLING_STEP.with(|s| s.set(sibling_step));

    // Each child the cache knows as a row it kept, where the cache last saw
    // it, with the key to prove it is that row.
    let depth = parent.tree_path.len();
    let mut kids: Vec<Option<LayoutNode>> = live_children.into_iter().map(Some).collect();
    let olds = kids.len();
    ORIGINS.with(|o| o.borrow_mut().clear());
    for (id, row) in cache.rows.iter_mut() {
        if row.at.len() != depth + 1 || !row.at.starts_with(&parent.tree_path) {
            continue;
        }
        let i = row.at[depth];
        let Some(slot) = kids.get_mut(i) else { continue };
        if slot.as_ref().is_some_and(|n| n.key.as_deref() == Some(id.3.as_str())) {
            let node = slot.take().expect("checked just above");
            row.live = Some(LiveRow { node, from: i });
        }
    }

    let old = std::mem::take(&mut cache.rows);
    ROWS.with(|r| {
        *r.borrow_mut() = Some(BuildRows {
            kept: HashMap::with_capacity(old.len()),
            old,
            values: HashMap::new(),
            off: sibling_step || !state.attempted.is_empty(),
            hits: 0,
            misses: 0,
        })
    });
    swaps.begin_part();
    let comps = Components::new();
    let mut ancestors = ctx.ancestors.clone();
    let mut reg = BindingRegistry::default();
    let (mut children, deps) = build_children(
        &element_children(el),
        &rules,
        &comps,
        &mut ancestors,
        &ctx.inherited,
        engine,
        &ctx.locals,
        &parent.tree_path,
        &parent.tpl_path,
        &mut reg,
        state,
        instances,
        swaps,
        None,
        None,
        None,
        None,
    );
    swaps.end_part(&parent.tpl_path);
    let origins = ORIGINS.with(|o| std::mem::take(&mut *o.borrow_mut()));
    let reg = merge_rows(live_reg, reg, depth, olds, &origins, children.len());
    if let Some(built) = ROWS.with(|r| r.borrow_mut().take()) {
        // The rows under the parent that this build did not keep have left;
        // every other list's rows stay for the next build.
        let under = |p: &[usize]| p.len() > parent.tree_path.len() && p.starts_with(&parent.tree_path);
        let mut rows = built.kept;
        rows.extend(built.old.into_iter().filter(|(_, row)| !under(&row.path)));
        cache.rows = rows;
        cache.hits = built.hits;
        cache.misses = built.misses;
    }
    cache.rules = Some((print, env, rules));
    if env.reduced_motion {
        for child in &mut children {
            still_the_motion(child);
        }
    }
    Some(Rebuilt { children, reg, deps })
}

/// Whether [`rebuild_children`] would build `parent`'s children, without
/// building anything: so a caller with several to reconcile can find out
/// for all of them before it starts on one.
pub fn can_rebuild_children(
    sfc: &Sfc,
    namespaces: &HashMap<String, Namespace>,
    swaps: &Swaps,
    env: Environment,
    cache: &RowCache,
    parent: &StructuralParent,
) -> bool {
    rebuild_target(sfc, namespaces, swaps, env, cache, parent).is_some()
}

/// The template element of `parent`, when its children can be built alone.
fn rebuild_target<'a>(
    sfc: &'a Sfc,
    namespaces: &HashMap<String, Namespace>,
    swaps: &Swaps,
    env: Environment,
    cache: &RowCache,
    parent: &StructuralParent,
) -> Option<&'a Element> {
    let ctx = parent.ctx.as_ref()?;
    cache.shape?;
    if cache.env != Some(env) || !swaps.is_empty() {
        return None;
    }
    let mut el = &sfc.template;
    for &i in &parent.tpl_path {
        el = *element_children(el).get(i)?;
    }
    // The root is always the template itself, and is held inside the `Sfc`,
    // which moves (into its document, after the load's build), so its
    // address is not compared. Everything below it is on the heap.
    if !parent.tpl_path.is_empty() && el as *const Element as usize != ctx.el {
        return None;
    }
    let document = namespaces.get(DOCUMENT_NAMESPACE);
    element_children(el).into_iter().all(|c| plain_subtree(c, document)).then_some(el)
}

/// Whether a whole build does nothing for `el` or anything under it that a
/// build of one parent's children would not. See [`rebuild_children`].
fn plain_subtree(el: &Element, document: Option<&Namespace>) -> bool {
    let tag = el.tag.as_str();
    let component = document.is_some_and(|n| n.contains_key(tag) || n.contains_key(&tag.replace('_', "-")));
    !component
        // Each of these expands something a whole build keeps track of: a
        // slot's content, a route's view (a component), the outlet.
        && !matches!(tag, "slot" | "router" | "route" | "router-view")
        && el.attr("r-transition").is_none()
        && el.attr("for").is_none()
        && element_children(el).into_iter().all(|c| plain_subtree(c, document))
}

/// The entries of everything under one parent, taken out of a registry by
/// [`BindingRegistry::take_under`], and where each list had them.
pub struct Taken {
    pub reg: BindingRegistry,
    pub place: Place,
}

/// Where each list of a registry had the entries [`Taken`] from it.
pub struct Place([usize; 10]);

impl BindingRegistry {
    /// Take out the entries of everything under the parent at `parent`,
    /// remembering where they stood so [`put_under`](Self::put_under) can put
    /// a fresh build's in the same place. A build walks the tree depth first,
    /// so everything under one parent is together in each list.
    ///
    /// Where a list had none, the place is where a whole build would have
    /// put them: before the first entry that comes later in the tree (an
    /// ancestor is a prefix and sorts before), and for the parents' list,
    /// which records a parent after everything under it, before this
    /// parent's own entry.
    pub fn take_under(&mut self, parent: &[usize]) -> Taken {
        fn under(p: &[usize], parent: &[usize]) -> bool {
            p.len() > parent.len() && p.starts_with(parent)
        }
        fn after(p: &[usize], parent: &[usize]) -> bool {
            p > parent && !p.starts_with(parent)
        }
        fn take<T>(list: &mut Vec<T>, inside: impl Fn(&T) -> bool, later: impl Fn(&T) -> bool) -> (Vec<T>, usize) {
            match list.iter().position(&inside) {
                Some(s) => {
                    let end = s + list[s..].iter().take_while(|b| inside(b)).count();
                    debug_assert!(!list[end..].iter().any(&inside), "entries under one parent are together");
                    (list.drain(s..end).collect(), s)
                }
                None => (Vec::new(), list.iter().position(later).unwrap_or(list.len())),
            }
        }
        let mut reg = BindingRegistry::default();
        let mut at = [0; 10];
        (reg.text, at[0]) = take(&mut self.text, |b| under(&b.path, parent), |b| after(&b.path, parent));
        (reg.value, at[1]) = take(&mut self.value, |b| under(&b.path, parent), |b| after(&b.path, parent));
        (reg.show, at[2]) = take(&mut self.show, |b| under(&b.path, parent), |b| after(&b.path, parent));
        (reg.src, at[3]) = take(&mut self.src, |b| under(&b.path, parent), |b| after(&b.path, parent));
        (reg.options, at[4]) = take(&mut self.options, |b| under(&b.path, parent), |b| after(&b.path, parent));
        (reg.structural_parents, at[5]) = take(
            &mut self.structural_parents,
            |b| under(&b.tree_path, parent),
            |b| b.tree_path == parent || after(&b.tree_path, parent),
        );
        (reg.toggles, at[6]) = take(&mut self.toggles, |b| under(&b.path, parent), |b| after(&b.path, parent));
        (reg.components, at[7]) = take(&mut self.components, |b| under(&b.path, parent), |b| after(&b.path, parent));
        (reg.styled, at[8]) = take(&mut self.styled, |b| under(&b.path, parent), |b| after(&b.path, parent));
        (reg.elements.entries, at[9]) =
            take(&mut self.elements.entries, |e| under(&e.path, parent), |e| after(&e.path, parent));
        Taken { reg, place: Place(at) }
    }

    /// Put `fresh`, a build of the parent at `parent`'s children, where
    /// [`take_under`](Self::take_under) took the old entries from, and give
    /// the parent's own entry `deps` (or drop it, when nothing under it is
    /// structural any more): what a whole build would have recorded, in the
    /// order it would have recorded it.
    pub fn put_under(&mut self, parent: &[usize], place: &Place, fresh: BindingRegistry, deps: HashSet<String>) {
        let at = place.0;
        let BindingRegistry {
            text,
            value,
            show,
            src,
            options,
            structural_parents,
            toggles,
            components,
            styled,
            structural,
            elements,
        } = fresh;
        self.text.splice(at[0]..at[0], text);
        self.value.splice(at[1]..at[1], value);
        self.show.splice(at[2]..at[2], show);
        self.src.splice(at[3]..at[3], src);
        self.options.splice(at[4]..at[4], options);
        self.structural_parents.splice(at[5]..at[5], structural_parents);
        self.toggles.splice(at[6]..at[6], toggles);
        self.components.splice(at[7]..at[7], components);
        self.styled.splice(at[8]..at[8], styled);
        self.elements.entries.splice(at[9]..at[9], elements.entries);
        self.structural.extend(structural);
        if let Some(i) = self.structural_parents.iter().position(|b| b.tree_path == parent) {
            if deps.is_empty() {
                self.structural_parents.remove(i);
            } else {
                self.structural_parents[i].deps = deps;
            }
        }
    }
}

/// A registry as text that does not depend on hash order, for comparing two.
#[cfg(debug_assertions)]
pub fn registry_digest(reg: &BindingRegistry) -> String {
    let deps = |d: &HashSet<String>| {
        let mut d: Vec<&String> = d.iter().collect();
        d.sort();
        format!("{d:?}")
    };
    let mut out = String::new();
    for b in &reg.text {
        out += &format!("text {:?} {:?} {:?} {}\n", b.path, b.template, b.locals, deps(&b.deps));
    }
    for b in &reg.value {
        out += &format!(
            "value {:?} {:?} {:?} {:?} {:?} {}\n",
            b.path, b.model, b.row, b.instance, b.locals, deps(&b.deps)
        );
    }
    for b in &reg.show {
        out += &format!("show {:?} {:?} {:?} {}\n", b.path, b.cond, b.locals, deps(&b.deps));
    }
    for (kind, list) in [("src", &reg.src), ("options", &reg.options)] {
        for b in list {
            out += &format!("{kind} {:?} {:?} {:?} {}\n", b.path, b.expr, b.locals, deps(&b.deps));
        }
    }
    for b in &reg.structural_parents {
        out += &format!("parent {:?} {:?} {}\n", b.tree_path, b.tpl_path, deps(&b.deps));
    }
    for b in &reg.toggles {
        out += &format!("toggle {:?} {}\n", b.path, deps(&b.deps));
    }
    for b in &reg.components {
        out += &format!("component {:?} {}\n", b.path, deps(&b.deps));
    }
    for b in &reg.styled {
        out += &format!("styled {:?} {}\n", b.path, deps(&b.deps));
    }
    for e in &reg.elements.entries {
        out += &format!("element {:?} {:?}\n", e.path, e.desc);
    }
    out += &format!("structural {}\n", deps(&reg.structural));
    out
}

/// Build one keyed row of an `r-for`, or reuse it. `reuse` is its identity and
/// the name of its place local, `None` for a row that may not be reused.
#[allow(clippy::too_many_arguments)]
fn build_row(
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
    instances: &mut Instances,
    swaps: &mut Swaps,
    instance: Option<&str>,
    slot: Option<Slot>,
    outlet: Option<Outlet>,
    row: Option<&str>,
    side: Option<SwapSide>,
    reuse: Option<(RowId, Option<String>)>,
) -> LayoutNode {
    let active = ROWS.with(|r| r.borrow().as_ref().is_some_and(|b| !b.off));
    let Some((id, place_name)) = reuse.filter(|_| active) else {
        return build_node(
            el, rules, comps, ancestors, prev, inherited, engine, locals, path, tpl_path, reg, state,
            instances, swaps, instance, slot, outlet, row, side,
        );
    };
    let place_name = place_name.as_deref();
    let (rest, place) = match locals.last() {
        Some((name, value)) if Some(name.as_str()) == place_name => (&locals[..locals.len() - 1], Some(value)),
        _ => (&locals[..], None),
    };
    let touched = state_touches(state, path, &id.3);
    if !touched {
        let entry = ROWS.with(|r| r.borrow_mut().as_mut().and_then(|b| b.old.remove(&id)));
        if let Some(mut entry) = entry {
            if row_unchanged(&entry, rest, place, ancestors, inherited, engine) {
                let node = match entry.live.take() {
                    Some(live) => adopt_row(live, &entry.at, path, place_name, place),
                    None => replay_row(&entry, path, place_name, place, reg),
                };
                entry.at = path.to_vec();
                // Whatever encloses this row read what it read, and baked
                // what it baked, as surely as if it had just been built.
                rux_script::note_reads(entry.reads.iter().map(|(n, _)| n.as_str()));
                if entry.baked {
                    BAKED.with(|b| b.set(b.get() + 1));
                }
                ROWS.with(|r| {
                    if let Some(b) = r.borrow_mut().as_mut() {
                        b.hits += 1;
                        b.kept.insert(id, entry);
                    }
                });
                return node;
            }
        }
    }

    let marks = Marks::of(reg);
    let warned = WARN_CALLS.with(std::cell::Cell::get);
    let unreplayable = UNREPLAYABLE.with(std::cell::Cell::get);
    let baked = BAKED.with(std::cell::Cell::get);
    let effects = rux_script::effects();
    rux_script::begin_reads();
    let node = build_node(
        el, rules, comps, ancestors, prev, inherited, engine, locals, path, tpl_path, reg, state,
        instances, swaps, instance, slot, outlet, row, side,
    );
    let names = rux_script::end_reads();
    let keep = !touched
        && WARN_CALLS.with(std::cell::Cell::get) == warned
        && UNREPLAYABLE.with(std::cell::Cell::get) == unreplayable
        // A binding that failed, printed or emitted would go quiet if reused.
        && rux_script::effects() == effects
        && reg.structural.len() == marks.structural;
    ROWS.with(|r| {
        if let Some(b) = r.borrow_mut().as_mut() {
            b.misses += 1;
        }
    });
    if keep {
        let reads = names
            .into_iter()
            .filter(|n| engine.declares(n))
            .map(|n| {
                let value = signal_now(engine, &n);
                (n, value)
            })
            .collect();
        let frag = marks.fragment(reg);
        let other_place = frag_has_other_place(&frag, place_name);
        let entry = CachedRow {
            other_place,
            node: node.clone(),
            path: path.to_vec(),
            at: path.to_vec(),
            live: None,
            frag,
            locals: rest.to_vec(),
            place: place.cloned(),
            baked: BAKED.with(std::cell::Cell::get) != baked,
            ancestors: ancestors.clone(),
            inherited: inherited.clone(),
            reads,
        };
        ROWS.with(|r| {
            if let Some(b) = r.borrow_mut().as_mut() {
                b.kept.insert(id, Box::new(entry));
            }
        });
    }
    node
}

/// Whether the interaction state reaches into the row at `path` (keyed `key`):
/// something in it hovered, pressed or focused, or a field in it left. The
/// state is not compared, so a row it touches is built.
fn state_touches(state: &InteractionState, path: &[usize], key: &str) -> bool {
    let inside = |p: &Option<Vec<usize>>| p.as_ref().is_some_and(|p| p.starts_with(path));
    inside(&state.hovered)
        || inside(&state.active)
        || inside(&state.focused_path)
        || state.focused_row.as_deref() == Some(key)
        || state.touched.iter().any(|(_, r, _)| r.as_deref() == Some(key))
}

/// A signal's value now, read once per build.
fn signal_now(engine: &Engine, name: &str) -> Option<Value> {
    if let Some(value) = ROWS.with(|r| r.borrow().as_ref().and_then(|b| b.values.get(name).cloned())) {
        return value;
    }
    let value = engine.signal_value(name);
    ROWS.with(|r| {
        if let Some(b) = r.borrow_mut().as_mut() {
            b.values.insert(name.to_string(), value.clone());
        }
    });
    value
}

/// Whether a signal still holds `then`, compared where it is kept rather than
/// copied out, since a row may read a long list.
fn signal_still(engine: &Engine, name: &str, then: &Option<Value>) -> bool {
    let known = ROWS.with(|r| r.borrow().as_ref().and_then(|b| b.values.get(name).map(|v| v == then)));
    match known {
        Some(same) => same,
        None => signal_now(engine, name) == *then,
    }
}

/// Whether a kept row would be built the same way now.
fn row_unchanged(
    entry: &CachedRow,
    rest: &[(String, Value)],
    place: Option<&Value>,
    ancestors: &[AncNode],
    inherited: &Inherited,
    engine: &Engine,
) -> bool {
    let moved = entry.place.as_ref() != place;
    entry.locals.as_slice() == rest
        // A handler carries the place in a string.
        && !(moved && entry.baked)
        // A list nested in the row has its places built from this one.
        && !(moved && entry.other_place)
        && entry.ancestors.as_slice() == ancestors
        && same_inherited(&entry.inherited, inherited)
        && entry.reads.iter().all(|(n, v)| signal_still(engine, n, v))
}

/// Whether any locals the row captured hold a place other than its own.
fn frag_has_other_place(frag: &BindingRegistry, own: Option<&str>) -> bool {
    let other = |locals: &[(String, Value)]| {
        locals.iter().any(|(n, _)| n.starts_with(ROW_PLACE) && Some(n.as_str()) != own)
    };
    frag.text.iter().any(|b| other(&b.locals))
        || frag.value.iter().any(|b| other(&b.locals))
        || frag.show.iter().any(|b| other(&b.locals))
        || frag.src.iter().any(|b| other(&b.locals))
        || frag.options.iter().any(|b| other(&b.locals))
}

/// Destructured so that a field added to either struct fails to compile here
/// rather than being silently left out of the comparison.
fn same_inherited(a: &Inherited, b: &Inherited) -> bool {
    let Inherited { color, selection, font_size, font_family, vars, run, direction, text: text_style, shadows, look } = a;
    let SelectionStyle { background, color: text, handle } = selection;
    let rgba = |x: &Rgba, y: &Rgba| x.r == y.r && x.g == y.g && x.b == y.b && x.a == y.a;
    let opt = |x: &Option<Rgba>, y: &Option<Rgba>| match (x, y) {
        (Some(x), Some(y)) => rgba(x, y),
        (None, None) => true,
        _ => false,
    };
    rgba(color, &b.color)
        && opt(background, &b.selection.background)
        && opt(text, &b.selection.color)
        && opt(handle, &b.selection.handle)
        && *font_size == b.font_size
        && *font_family == b.font_family
        && (Rc::ptr_eq(vars, &b.vars) || **vars == *b.vars)
        && *run == b.run
        && *direction == b.direction
        && *text_style == b.text
        && *look == b.look
        && match (shadows, &b.shadows) {
            (Some(x), Some(y)) => Arc::ptr_eq(x, y),
            (None, None) => true,
            _ => false,
        }
}

/// Put a kept row into this build at `path`: its node, and its registry
/// entries rewritten to where it now is.
fn replay_row(
    entry: &CachedRow,
    path: &[usize],
    place_name: Option<&str>,
    place: Option<&Value>,
    reg: &mut BindingRegistry,
) -> LayoutNode {
    let from = entry.path.as_slice();
    let moved = from != path;
    let fix_path = |p: &mut Vec<usize>| {
        if moved {
            rebase(p, from, path);
        }
    };
    let re_place = place_name.zip(place).filter(|_| entry.place.as_ref() != place);
    let fix_locals = |locals: &mut Vec<(String, Value)>| {
        if let Some((name, value)) = re_place {
            for (n, v) in locals.iter_mut() {
                if n == name {
                    *v = value.clone();
                }
            }
        }
    };
    let mut node = entry.node.clone();
    if moved {
        rebase_node(&mut node, from, path);
    }
    let f = &entry.frag;
    for b in &f.text {
        let mut b = b.clone();
        fix_path(&mut b.path);
        fix_locals(&mut b.locals);
        reg.text.push(b);
    }
    for b in &f.value {
        let mut b = b.clone();
        fix_path(&mut b.path);
        fix_locals(&mut b.locals);
        reg.value.push(b);
    }
    for b in &f.show {
        let mut b = b.clone();
        fix_path(&mut b.path);
        fix_locals(&mut b.locals);
        reg.show.push(b);
    }
    for b in &f.src {
        let mut b = b.clone();
        fix_path(&mut b.path);
        fix_locals(&mut b.locals);
        reg.src.push(b);
    }
    for b in &f.options {
        let mut b = b.clone();
        fix_path(&mut b.path);
        fix_locals(&mut b.locals);
        reg.options.push(b);
    }
    for b in &f.structural_parents {
        let mut b = b.clone();
        fix_path(&mut b.tree_path);
        reg.structural_parents.push(b);
    }
    for b in &f.toggles {
        let mut b = b.clone();
        fix_path(&mut b.path);
        reg.toggles.push(b);
    }
    for b in &f.components {
        let mut b = b.clone();
        fix_path(&mut b.path);
        reg.components.push(b);
    }
    for b in &f.styled {
        let mut b = b.clone();
        fix_path(&mut b.path);
        reg.styled.push(b);
    }
    for e in &f.elements.entries {
        let mut e = e.clone();
        fix_path(&mut e.path);
        reg.elements.entries.push(e);
    }
    node
}

/// [`replay_row`] for a row taken from the live tree: moved into place
/// rather than copied, its node's paths rebased from where it was.
///
/// Its registry entries are not touched here. They stay with the entries
/// the build is replacing, and [`merge_rows`] puts them where the row now
/// is, in one pass over all of them once the build is done: moving each
/// row's entries out, into a registry of its own and back was 0.7 of a
/// 300-row list's 2.4 ms reconcile (track (g).7c).
fn adopt_row(
    live: LiveRow,
    from: &[usize],
    path: &[usize],
    place_name: Option<&str>,
    place: Option<&Value>,
) -> LayoutNode {
    let LiveRow { mut node, from: old } = live;
    if from != path {
        rebase_node(&mut node, from, path);
    }
    if let Some(&here) = path.last() {
        let place = place_name.zip(place).map(|(n, v)| (n.to_string(), v.clone()));
        ORIGINS.with(|o| o.borrow_mut().push(Origin { here, old, place }));
    }
    node
}

/// Where a row of a build of one list came from: its index under the parent
/// now and before, and the place its locals are to say it is in.
struct Origin {
    here: usize,
    old: usize,
    place: Option<(String, Value)>,
}

thread_local! {
    /// The rows a build of one list adopted, for [`merge_rows`].
    static ORIGINS: std::cell::RefCell<Vec<Origin>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The registry under a parent after a build of its children: the entries
/// of every adopted row taken from `live` (what was there before) and put at
/// the row's new index, the rest from `fresh` (what the build recorded), in
/// the order of the children, which is the order a whole build records.
fn merge_rows(
    live: BindingRegistry,
    fresh: BindingRegistry,
    depth: usize,
    olds: usize,
    origins: &[Origin],
    children: usize,
) -> BindingRegistry {
    let mut from: Vec<Option<&Origin>> = vec![None; children];
    for o in origins {
        if let Some(slot) = from.get_mut(o.here) {
            *slot = Some(o);
        }
    }
    let fix = |locals: &mut Vec<(String, Value)>, o: &Origin| {
        if let Some((name, value)) = &o.place {
            for (n, v) in locals.iter_mut() {
                if n == name && v != value {
                    *v = value.clone();
                }
            }
        }
    };
    fn merge<T>(
        live: Vec<T>,
        fresh: Vec<T>,
        depth: usize,
        olds: usize,
        from: &[Option<&Origin>],
        path: impl Fn(&mut T) -> &mut Vec<usize>,
        read: impl Fn(&T) -> &Vec<usize>,
        mut also: impl FnMut(&mut T, &Origin),
    ) -> Vec<T> {
        if live.is_empty() && from.iter().all(Option::is_none) {
            return fresh;
        }
        // Each old row's entries take the index its row has now, in place,
        // and a row nobody adopted goes; then the new entries join them and
        // all are put in the children's order by a stable sort, which keeps
        // each row's own entries in theirs. The sort finds the runs already
        // in order, so a list that moved one row costs about one pass.
        let mut now: Vec<Option<&Origin>> = vec![None; olds];
        for o in from.iter().flatten() {
            if let Some(slot) = now.get_mut(o.old) {
                *slot = Some(*o);
            }
        }
        let mut out = live;
        out.retain_mut(|entry| {
            let Some(&j) = path(entry).get(depth) else { return false };
            let Some(o) = now.get(j).copied().flatten() else { return false };
            path(entry)[depth] = o.here;
            also(entry, o);
            true
        });
        out.extend(fresh);
        out.sort_by_key(|entry| read(entry).get(depth).copied().unwrap_or(usize::MAX));
        out
    }
    let BindingRegistry { text, value, show, src, options, structural_parents, toggles, components, styled, structural: _, elements } = live;
    let mut f = fresh;
    BindingRegistry {
        text: merge(text, std::mem::take(&mut f.text), depth, olds, &from, |b| &mut b.path, |b| &b.path, |b, o| fix(&mut b.locals, o)),
        value: merge(value, std::mem::take(&mut f.value), depth, olds, &from, |b| &mut b.path, |b| &b.path, |b, o| fix(&mut b.locals, o)),
        show: merge(show, std::mem::take(&mut f.show), depth, olds, &from, |b| &mut b.path, |b| &b.path, |b, o| fix(&mut b.locals, o)),
        src: merge(src, std::mem::take(&mut f.src), depth, olds, &from, |b| &mut b.path, |b| &b.path, |b, o| fix(&mut b.locals, o)),
        options: merge(options, std::mem::take(&mut f.options), depth, olds, &from, |b| &mut b.path, |b| &b.path, |b, o| fix(&mut b.locals, o)),
        structural_parents: merge(structural_parents, std::mem::take(&mut f.structural_parents), depth, olds, &from, |b| &mut b.tree_path, |b| &b.tree_path, |_, _| {}),
        toggles: merge(toggles, std::mem::take(&mut f.toggles), depth, olds, &from, |b| &mut b.path, |b| &b.path, |_, _| {}),
        components: merge(components, std::mem::take(&mut f.components), depth, olds, &from, |b| &mut b.path, |b| &b.path, |_, _| {}),
        styled: merge(styled, std::mem::take(&mut f.styled), depth, olds, &from, |b| &mut b.path, |b| &b.path, |_, _| {}),
        structural: f.structural,
        elements: ElementIndex {
            entries: merge(elements.entries, std::mem::take(&mut f.elements.entries), depth, olds, &from, |e| &mut e.path, |e| &e.path, |_, _| {}),
        },
    }
}

/// Replace the prefix `from` of `p` with `to`, when `p` is under `from`.
fn rebase(p: &mut Vec<usize>, from: &[usize], to: &[usize]) {
    if p.starts_with(from) {
        if from.len() == to.len() {
            // A row moved within its list: the usual case, done in place.
            p[..to.len()].copy_from_slice(to);
        } else {
            p.splice(..from.len(), to.iter().copied());
        }
    }
}

/// Every path a built node carries, rebased from `from` to `to`.
fn rebase_node(node: &mut LayoutNode, from: &[usize], to: &[usize]) {
    for p in [&mut node.state_path, &mut node.focus_path].into_iter().flatten() {
        rebase(p, from, to);
    }
    if let Some(form) = node.field.as_mut().and_then(|f| f.form.as_mut()) {
        rebase(form, from, to);
    }
    for child in &mut node.children {
        rebase_node(child, from, to);
    }
}

/// How long each part of a registry was before a row's build, so what the row
/// added can be taken off the end.
struct Marks {
    text: usize,
    value: usize,
    show: usize,
    src: usize,
    options: usize,
    parents: usize,
    toggles: usize,
    components: usize,
    styled: usize,
    elements: usize,
    structural: usize,
}

impl Marks {
    fn of(reg: &BindingRegistry) -> Self {
        Self {
            text: reg.text.len(),
            value: reg.value.len(),
            show: reg.show.len(),
            src: reg.src.len(),
            options: reg.options.len(),
            parents: reg.structural_parents.len(),
            toggles: reg.toggles.len(),
            components: reg.components.len(),
            styled: reg.styled.len(),
            elements: reg.elements.entries.len(),
            structural: reg.structural.len(),
        }
    }

    fn fragment(&self, reg: &BindingRegistry) -> BindingRegistry {
        BindingRegistry {
            text: reg.text[self.text..].to_vec(),
            value: reg.value[self.value..].to_vec(),
            show: reg.show[self.show..].to_vec(),
            src: reg.src[self.src..].to_vec(),
            options: reg.options[self.options..].to_vec(),
            structural_parents: reg.structural_parents[self.parents..].to_vec(),
            toggles: reg.toggles[self.toggles..].to_vec(),
            components: reg.components[self.components..].to_vec(),
            styled: reg.styled[self.styled..].to_vec(),
            structural: HashSet::new(),
            elements: ElementIndex { entries: reg.elements.entries[self.elements..].to_vec() },
        }
    }
}

/// `<screen>` is the one element that means a *place* rather than a box: the
/// whole display, which nothing between it and the window may shrink, inset or
/// scroll away.
///
/// It used to mean nothing of the kind. It laid out exactly like a `<view>`,
/// and the only thing that filled the window was the **root**, forced to the
/// viewport by the layout whatever its tag. So a `<screen>` anywhere but the
/// root was an ordinary box, and the word was carrying a meaning the code did
/// not implement anywhere.
///
/// The root keeps being forced to the viewport, because the root *is* the
/// window: it is the initial containing block and everything `fixed` hoists to
/// it. That is why this only has to say anything about a `<screen>` that is not
/// the root, and why it says it as `position: fixed` with every inset at zero:
/// the four edges of the initial containing block are the display, whatever
/// box, padding or scroller the element happens to be written inside.
///
/// They are **defaults**, applied only where the author's own stylesheet is
/// silent, the way every other element's defaults work. A `<screen>` with a
/// `top` of its own is a `<screen>` the author has taken responsibility for.
fn screen_fills_the_display(
    mut st: Style,
    el: &Element,
    path: &[usize],
    props: &HashMap<String, String>,
) -> Style {
    if el.tag != "screen" || path.is_empty() {
        return st;
    }
    if !props.contains_key("position") {
        st.position = Position::Fixed;
    }
    for (i, edge) in INSETS.iter().enumerate() {
        if !props.contains_key(*edge) {
            st.inset[i] = Some(Len::Px(0.0));
        }
    }
    st
}

// ── Value interpretation (honored subset) ───────────────────────────────────

thread_local! {
    /// [`interpret`]'s answers for this build, by declaration set. The rows of
    /// a list carry the same declarations, and interpreting them again for
    /// each row was the largest single part of a build. Emptied when a build
    /// starts, so nothing outlives the stylesheet it came from.
    static INTERPRETED: std::cell::RefCell<HashMap<u64, Vec<(Vec<(String, String)>, Style)>>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Forget every shared interpretation. Called as a build starts.
fn forget_interpreted() {
    INTERPRETED.with(|m| m.borrow_mut().clear());
}

/// [`interpret`], shared between elements whose declarations are the same.
///
/// A result is kept only when interpreting it raised nothing: a warning names
/// the line of the element being built, so an answer shared from another
/// element would lose the warning this one owes. Those are interpreted every
/// time, as before.
fn interpreted(p: &HashMap<String, String>) -> Style {
    use std::hash::{Hash, Hasher};
    let mut pairs: Vec<(&String, &String)> = p.iter().collect();
    pairs.sort_unstable();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    pairs.hash(&mut hasher);
    let key = hasher.finish();
    let same = |kept: &[(String, String)]| {
        kept.len() == pairs.len() && kept.iter().zip(&pairs).all(|((a, b), (c, d))| a == *c && b == *d)
    };
    let hit = INTERPRETED.with(|m| {
        m.borrow().get(&key).and_then(|bucket| bucket.iter().find(|(kept, _)| same(kept)).map(|(_, st)| st.clone()))
    });
    if let Some(style) = hit {
        return style;
    }
    let before = WARN_CALLS.with(std::cell::Cell::get);
    let style = interpret(p);
    if WARN_CALLS.with(std::cell::Cell::get) == before {
        let owned = pairs.into_iter().map(|(a, b)| (a.clone(), b.clone())).collect();
        INTERPRETED.with(|m| m.borrow_mut().entry(key).or_default().push((owned, style.clone())));
    }
    style
}

fn interpret(p: &HashMap<String, String>) -> Style {
    let mut st = Style::default();
    if let Some(v) = p.get("display") {
        // Rux has no inline level for a box (see "Where Rux differs"), so the
        // inline forms are their block forms: `inline-flex` lays its
        // children out as a flex box, which was lost when it read as `block`.
        st.display = match v.trim() {
            "flex" | "inline-flex" => Display::Flex,
            "grid" | "inline-grid" => Display::Grid,
            "inline" => Display::Inline,
            "none" => Display::None,
            // `-webkit-box` is how `-webkit-line-clamp` is written; vertical,
            // it lays its children out as a block does.
            "block" | "inline-block" | "flow-root" | "list-item" | "-webkit-box" | "-webkit-inline-box" => {
                Display::Block
            }
            other => {
                warn_once(format!("`display: {other}` is not a layout Rux has, so the box is laid out as `block`"));
                Display::Block
            }
        };
    }
    for (i, side) in ["width", "height"].iter().enumerate() {
        let Some(v) = p.get(*side) else { continue };
        // `anchor-size()` is the anchor's size, known only once the anchor
        // is laid out: the layout resolves it in a second pass.
        if v.trim_start().starts_with("anchor-size(") {
            match parse_anchor_size(v) {
                Some(a) => st.anchor.get_or_insert_with(Default::default).size[i] = Some(a),
                None => warn(format!(
                    "`{side}: {}` is not an `anchor-size()` Rux understands; write `anchor-size(width)`, `anchor-size(--name height)` or `anchor-size(width, 120px)`",
                    v.trim()
                )),
            }
            continue;
        }
        let len = parse_len(first(v));
        if i == 0 {
            st.width = len;
        } else {
            st.height = len;
        }
    }
    (st.padding, st.padding_pct) = box_sides(p, "padding");
    (st.margin, st.margin_pct) = box_sides(p, "margin");
    interpret_border(p, &mut st);
    interpret_path_paint(p, &mut st);
    if let Some((px, pct)) = p.get("gap").and_then(|v| gap_length(v)) {
        st.gap = px;
        st.gap_pct = [pct; 2];
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
    interpret_grid_template(p, &mut st);
    if let Some(v) = p.get("grid-area") {
        let [rs, cs, re, ce] = parse_grid_area(v);
        st.grid_row = (rs, re);
        st.grid_column = (cs, ce);
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
    // Where an `<image>` or `<path>` inside a `<text>` sits against the line.
    if let Some(a) = p.get("vertical-align").and_then(|v| rux_layout::VerticalAlign::parse(v)) {
        st.vertical_align = a;
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
    if let Some((px, pct)) = p.get("row-gap").and_then(|v| gap_length(v)) {
        st.row_gap = Some(px);
        st.gap_pct[1] = pct;
    }
    if let Some((px, pct)) = p.get("column-gap").and_then(|v| gap_length(v)) {
        st.column_gap = Some(px);
        st.gap_pct[0] = pct;
    }
    if let Some(v) = p.get("position") {
        st.position = match v.trim() {
            "static" => Position::Static,
            "relative" => Position::Relative,
            "absolute" => Position::Absolute,
            "fixed" => Position::Fixed,
            "sticky" => Position::Sticky,
            other => {
                // Every value used to fall through to `relative`, so a typo was
                // a box that quietly stayed where it was and an author who could
                // not tell a misspelling from a rule that does nothing.
                warn(format!(
                    "`position: {other}` is not a value Rux knows; use `static`, `relative`, \
                     `sticky`, `absolute` or `fixed`"
                ));
                Position::Static
            }
        };
        // A fixed box with nothing to pin it to lands in the window's top-left
        // corner, which is a legal answer to what was written and never what was
        // meant. Absolute has the same shape and a real use for it: with no
        // insets it keeps its static position, which is what `:leave-to` wants.
        if matches!(st.position, Position::Fixed | Position::Sticky)
            && !p.keys().any(|k| INSETS.contains(&k.as_str()))
        {
            let what = if st.position == Position::Sticky {
                "`position: sticky` with no `top`, `right`, `bottom` or `left` has no edge to \
                 stick to, so it will never move; name at least one inset"
            } else {
                "`position: fixed` with no `top`, `right`, `bottom` or `left` pins this box to \
                 the top-left of the window; name at least one inset"
            };
            warn(what.to_string());
        }
    }
    for (i, side) in INSETS.iter().enumerate() {
        if let Some(v) = p.get(*side) {
            if v.trim_start().starts_with("anchor(") {
                // Placed at paint time against where the anchor is drawn, so
                // the layout sees no inset on this side. See `AnchorStyle::inset`.
                st.inset[i] = None;
                match parse_anchor(v, i % 2 == 0) {
                    Ok(a) => st.anchor.get_or_insert_with(Default::default).inset[i] = Some(a),
                    Err(why) => warn(format!("`{side}: {}` {why}", v.trim())),
                }
                continue;
            }
            st.inset[i] = if first(v) == "auto" { None } else { parse_len(first(v)) };
        }
    }
    for (property, own) in [("anchor-name", true), ("position-anchor", false)] {
        let Some(v) = p.get(property) else { continue };
        let named = match v.trim() {
            "none" | "auto" => None,
            name if name.starts_with("--") && name.len() > 2 => Some(name.to_string()),
            other => {
                warn(format!(
                    "`{property}: {other}` is not an anchor name; an anchor name starts with two dashes, as in `{property}: --menu`"
                ));
                None
            }
        };
        if let Some(name) = named {
            let a = st.anchor.get_or_insert_with(Default::default);
            if own {
                a.name = Some(name);
            } else {
                a.position_anchor = Some(name);
            }
        }
    }
    interpret_position_try(p, &mut st);
    interpret_content_visibility(p, &mut st);
    if let Some(v) = p.get("z-index") {
        st.z_index = match first(v) {
            "auto" => None,
            n => match n.parse::<i32>() {
                Ok(z) => Some(z),
                Err(_) => {
                    warn(format!(
                        "`z-index: {n}` is not a value Rux knows; use `auto` or a whole number \
                         such as `1` or `-1`"
                    ));
                    None
                }
            },
        };
    }
    if let Some(v) = p.get("clip-path") {
        st.clip_path = parse_clip_path(v).map(Box::new);
    }
    st.mask = interpret_mask(p).map(Box::new);
    let filter = p.get("filter").map(|v| parse_filter("filter", v)).unwrap_or_default();
    let backdrop = p.get("backdrop-filter").map(|v| parse_filter("backdrop-filter", v)).unwrap_or_default();
    if !filter.is_empty() || !backdrop.is_empty() {
        st.filters = Some(Box::new(rux_layout::filter::Filters { filter, backdrop }));
    }
    if let Some(v) = p.get("mix-blend-mode") {
        st.blend = match v.trim() {
            "normal" => BlendMode::Normal,
            "multiply" => BlendMode::Multiply,
            "screen" => BlendMode::Screen,
            "overlay" => BlendMode::Overlay,
            "darken" => BlendMode::Darken,
            "lighten" => BlendMode::Lighten,
            "color-dodge" => BlendMode::ColorDodge,
            "color-burn" => BlendMode::ColorBurn,
            "hard-light" => BlendMode::HardLight,
            "soft-light" => BlendMode::SoftLight,
            "difference" => BlendMode::Difference,
            "exclusion" => BlendMode::Exclusion,
            "hue" => BlendMode::Hue,
            "saturation" => BlendMode::Saturation,
            "color" => BlendMode::Color,
            "luminosity" => BlendMode::Luminosity,
            other => {
                warn_once(format!(
                    "`mix-blend-mode: {other}` is not a blend mode Rux draws, so the box blends `normal`."
                ));
                BlendMode::Normal
            }
        };
    }
    if let Some(v) = p.get("isolation") {
        st.isolate = match first(v) {
            "isolate" => true,
            "auto" => false,
            other => {
                warn(format!("`isolation: {other}` is not a value Rux knows; use `auto` or `isolate`"));
                false
            }
        };
    }
    if let Some(v) = p.get("aspect-ratio") {
        st.aspect_ratio = parse_aspect_ratio(v);
    }
    // Static text cannot be selected yet and a field always can, as in a
    // browser, so `none` and `auto` are what happens already; asking for
    // more is said.
    match p.get("user-select").map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        None | Some("none" | "auto") => {}
        Some(other) => warn_once(format!(
            "`user-select: {other}` has nothing to turn on: text outside a field cannot be selected yet"
        )),
    }
    match p.get("box-sizing").map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("content-box") => st.box_sizing = rux_layout::BoxSizing::ContentBox,
        Some("border-box") | None => {}
        Some(other) => warn_once(format!("`box-sizing: {other}` is not `border-box` or `content-box`, so the box keeps `border-box`")),
    }
    if let Some(bg) = interpret_background(p) {
        st.background = Some(bg);
    }
    if let Some(v) = p.get("transform") {
        st.transform = parse_transform(v);
    }
    if let Some(pct) = translate_pct(p) {
        st.rare_mut().translate = Some(pct);
    }
    if let Some(v) = p.get("transform-origin") {
        if let Some(origin) = parse_transform_origin(v) {
            st.rare_mut().transform_origin = Some(origin);
        }
    }
    if ["translate", "rotate", "scale"].iter().any(|k| p.contains_key(*k)) {
        st.transform = individual_transforms(p, st.transform);
    }
    if let Some(v) = transition_value(p) {
        st.transitions = parse_transitions(&v);
    }
    if let Some(v) = p.get("box-shadow") {
        st.box_shadow = parse_box_shadow(v);
    }
    st.outline = interpret_outline(p);
    // `border-radius` shorthand (1–4 values, CSS diagonal grouping), then the
    // per-corner longhands override.
    if let Some(v) = p.get("border-radius") {
        st.radius = parse_border_radius(v);
        st.radius_pct = parse_border_radius_pct(v);
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
        let Some(written) = p.get(*corner).map(|v| first(v)) else { continue };
        // A longhand replaces whatever the shorthand said for that corner, in
        // both units: writing one in px after a shorthand in % has to clear the
        // percentage, or the corner would keep resolving against the box and
        // the px would never be seen.
        if let Some(px) = parse_px(written) {
            st.radius[i] = px;
            st.radius_pct[i] = None;
        } else if let Some(pct) =
            written.strip_suffix('%').and_then(|n| n.trim().parse::<f32>().ok())
        {
            st.radius_pct[i] = Some(pct);
        }
    }
    overflow_axes(p, &mut st);
    scroll_style(p, &mut st);
    if let Some(v) = p.get("touch-action") {
        // Which axes a scroller may still take from a finger that starts here.
        // `manipulation` is accepted as a synonym for `auto`: on the web it only
        // turns off double-tap zoom, which Rux has no equivalent of, so the two
        // mean the same thing here rather than the keyword being an error.
        st.touch_action = match v.trim() {
            "none" => TouchAction::None,
            "pan-x" => TouchAction::PanX,
            "pan-y" => TouchAction::PanY,
            _ => TouchAction::Auto,
        };
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

/// A box's background from the `background` shorthand and its longhands.
///
/// A plain colour stays a [`Background::Color`], which is what a hover
/// animates. Anything with an image is [`Background::Layers`]: one layer per
/// image, the other longhands' lists repeated to match, as CSS does, and the
/// colour under them all. `None` when nothing is said, or `background: none`.
fn interpret_background(p: &HashMap<String, String>) -> Option<Background> {
    let mut color = None;
    let mut images: Vec<Option<Background>> = Vec::new();
    let mut layers: Vec<BgLayer> = Vec::new();
    let said = p.contains_key("background") || BACKGROUND_LONGHANDS.iter().any(|k| p.contains_key(*k));
    if !said {
        return None;
    }
    if let Some(v) = p.get("background") {
        let parts = split_top_level_commas(v);
        let last = parts.len().saturating_sub(1);
        for (i, part) in parts.iter().enumerate() {
            let (image, layer, c) = background_layer(part, i == last);
            images.push(image);
            layers.push(layer);
            color = color.or(c);
        }
    }
    if let Some(v) = p.get("background-color") {
        color = parse_color(v.trim());
    }
    if let Some(v) = p.get("background-image") {
        images = split_top_level_commas(v)
            .iter()
            .map(|part| match part.trim() {
                "none" => None,
                img => {
                    let parsed = parse_background(img).filter(|b| !matches!(b, Background::Color(_)));
                    if parsed.is_none() {
                        warn(format!("`background-image: {img}` is not an image Rux draws; use a gradient or `url(…)`"));
                    }
                    parsed
                }
            })
            .collect();
    }
    // As many layers as there are images; each other list is repeated (or
    // cut short) to that many.
    let n = images.len();
    layers.resize_with(n, || BgLayer::of(Background::Color(Rgba::new(0.0, 0.0, 0.0, 0.0))));
    let each = |name: &str| -> Option<Vec<String>> {
        let v = p.get(name)?;
        let list: Vec<String> = split_top_level_commas(v).iter().map(|s| s.trim().to_string()).collect();
        (!list.is_empty()).then_some(list)
    };
    let cycle = |list: &[String], i: usize| list[i % list.len()].clone();
    if let Some(list) = each("background-size") {
        for (i, l) in layers.iter_mut().enumerate() {
            if let Some(size) = parse_bg_size(&cycle(&list, i)) {
                l.size = size;
            }
        }
    }
    if let Some(list) = each("background-position") {
        for (i, l) in layers.iter_mut().enumerate() {
            let toks = split_top_level(&cycle(&list, i)).into_iter().map(str::to_string).collect::<Vec<_>>();
            if let Some(pos) = parse_bg_position(&toks) {
                l.position = pos;
            }
        }
    }
    for (axis, name) in [(0, "background-position-x"), (1, "background-position-y")] {
        if let Some(list) = each(name) {
            for (i, l) in layers.iter_mut().enumerate() {
                let toks = split_top_level(&cycle(&list, i)).into_iter().map(str::to_string).collect::<Vec<_>>();
                if let Some(pos) = parse_bg_axis(&toks, axis) {
                    l.position[axis] = pos;
                }
            }
        }
    }
    if let Some(list) = each("background-repeat") {
        for (i, l) in layers.iter_mut().enumerate() {
            let toks = cycle(&list, i);
            if let Some(r) = parse_bg_repeat(&toks.split_whitespace().collect::<Vec<_>>()) {
                l.repeat = r;
            }
        }
    }
    for (name, origin) in [("background-clip", false), ("background-origin", true)] {
        if let Some(list) = each(name) {
            for (i, l) in layers.iter_mut().enumerate() {
                let v = cycle(&list, i);
                match parse_bg_box(&v) {
                    Some(b) if origin => l.origin = b,
                    Some(b) => l.clip = b,
                    None => warn(format!("`{name}: {v}` is not a box Rux knows; use `border-box`, `padding-box` or `content-box`")),
                }
            }
        }
    }
    if let Some(list) = each("background-attachment") {
        for (i, l) in layers.iter_mut().enumerate() {
            let v = cycle(&list, i);
            match parse_bg_attachment(&v) {
                Some(a) => l.attachment = a,
                None => warn(format!("`background-attachment: {v}` is not a value Rux knows; use `scroll`, `fixed` or `local`")),
            }
        }
    }
    // A `none` image still took its place in the lists; it draws nothing.
    let layers: Vec<BgLayer> = images
        .into_iter()
        .zip(layers)
        .filter_map(|(image, layer)| Some(BgLayer { image: image?, ..layer }))
        .collect();
    if layers.is_empty() {
        return color.map(Background::Color);
    }
    // The colour is cut to the bottom layer's clip, whatever its image.
    Some(Background::Layers(std::sync::Arc::new(BgLayers { color, layers })))
}

/// One layer of the `background` shorthand: its image, the rest of its
/// longhands, and its colour, which only the last layer may have.
fn background_layer(part: &str, last: bool) -> (Option<Background>, BgLayer, Option<Rgba>) {
    let mut image = None;
    let mut color = None;
    let mut layer = BgLayer::of(Background::Color(Rgba::new(0.0, 0.0, 0.0, 0.0)));
    let mut boxes = Vec::new();
    let mut position: Vec<String> = Vec::new();
    let mut size: Vec<String> = Vec::new();
    let mut repeat: Vec<&str> = Vec::new();
    let mut after_slash = false;
    // lightningcss may write `center/cover` without spaces. Only a slash
    // outside brackets: the one in `url(assets/logo.png)` is the path's.
    let mut depth = 0;
    let spaced: String = part
        .chars()
        .flat_map(|c| {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            match (c, depth) {
                ('/', 0) => vec![' ', '/', ' '],
                _ => vec![c],
            }
        })
        .collect();
    for tok in split_top_level(&spaced) {
        let is_position = matches!(tok, "left" | "right" | "top" | "bottom" | "center")
            || tok.ends_with('%')
            || parse_px(tok).is_some();
        match tok {
            "/" => after_slash = true,
            "none" => {}
            "repeat" | "no-repeat" | "repeat-x" | "repeat-y" | "space" | "round" => repeat.push(tok),
            "scroll" | "fixed" | "local" => layer.attachment = parse_bg_attachment(tok).expect("matched"),
            "border-box" | "padding-box" | "content-box" | "text" => boxes.push(tok),
            "cover" | "contain" | "auto" if after_slash => size.push(tok.to_string()),
            _ if after_slash && is_position => size.push(tok.to_string()),
            _ if is_position => position.push(tok.to_string()),
            _ if tok.contains('(') && !tok.starts_with("rgb") && !tok.starts_with("hsl") => {
                image = parse_background(tok).filter(|b| !matches!(b, Background::Color(_)));
                if image.is_none() {
                    warn(format!("`{tok}` is not an image Rux draws in a background; use a gradient or `url(…)`"));
                }
            }
            _ => match parse_color(tok) {
                Some(c) if last => color = Some(c),
                Some(_) => warn(format!(
                    "`background: {part}` gives a colour to a layer that is not the last; CSS only takes one there"
                )),
                None => warn(format!("`{tok}` in `background` is not something Rux knows")),
            },
        }
    }
    if let Some(pos) = parse_bg_position(&position) {
        layer.position = pos;
    }
    if let Some(s) = parse_bg_size(&size.join(" ")) {
        layer.size = s;
    }
    if let Some(r) = parse_bg_repeat(&repeat) {
        layer.repeat = r;
    }
    // One box is the origin and the clip; two are the origin, then the clip.
    match boxes.as_slice() {
        [one] => {
            layer.origin = parse_bg_box(one).unwrap_or(BgBox::Padding);
            layer.clip = parse_bg_box(one).unwrap_or(BgBox::Border);
        }
        [origin, clip, ..] => {
            layer.origin = parse_bg_box(origin).unwrap_or(BgBox::Padding);
            layer.clip = parse_bg_box(clip).unwrap_or(BgBox::Border);
        }
        [] => {}
    }
    (image, layer, color)
}

/// `background-size`: `cover`, `contain`, or one or two of a length, a
/// percentage and `auto`. One value is the width, the height `auto`.
fn parse_bg_size(v: &str) -> Option<BgSize> {
    let toks: Vec<&str> = v.split_whitespace().collect();
    let len = |t: &str| -> Option<BgLen> {
        if t == "auto" {
            return Some(BgLen::Auto);
        }
        if let Some(n) = t.strip_suffix('%') {
            return n.trim().parse().ok().map(BgLen::Pct);
        }
        parse_px(t).map(BgLen::Px)
    };
    let size = match toks.as_slice() {
        [] => return None,
        ["cover"] => Some(BgSize::Cover),
        ["contain"] => Some(BgSize::Contain),
        [w] => len(w).map(|w| BgSize::Sized([w, BgLen::Auto])),
        [w, h] => len(w).zip(len(h)).map(|(w, h)| BgSize::Sized([w, h])),
        _ => None,
    };
    if size.is_none() {
        warn(format!("`background-size: {v}` is not a size Rux knows"));
    }
    size
}

/// A `background-position` of one to four values, as two axes.
fn parse_bg_position(toks: &[String]) -> Option<[BgPos; 2]> {
    let kw = |t: &str| matches!(t, "left" | "right" | "top" | "bottom" | "center");
    let vertical = |t: &str| matches!(t, "top" | "bottom");
    let horizontal = |t: &str| matches!(t, "left" | "right");
    let at = |t: &str| -> Option<BgPos> {
        Some(match t {
            "left" | "top" => BgPos { pct: 0.0, px: 0.0 },
            "center" => BgPos { pct: 50.0, px: 0.0 },
            "right" | "bottom" => BgPos { pct: 100.0, px: 0.0 },
            _ => match t.strip_suffix('%') {
                Some(n) => BgPos { pct: n.trim().parse().ok()?, px: 0.0 },
                None => BgPos { pct: 0.0, px: parse_px(t)? },
            },
        })
    };
    let toks: Vec<&str> = toks.iter().map(String::as_str).collect();
    let out = match toks.as_slice() {
        [] => return None,
        [a] if vertical(a) => [at("center")?, at(a)?],
        [a] => [at(a)?, at("center")?],
        [a, b] if vertical(a) || horizontal(b) => [at(b)?, at(a)?],
        [a, b] => [at(a)?, at(b)?],
        // Three or four: each keyword may take an offset from its edge.
        _ => {
            let mut axes = [None, None];
            let mut i = 0;
            while i < toks.len() {
                let k = toks[i];
                if !kw(k) {
                    warn(format!("`background-position: {}` is not a position Rux knows", toks.join(" ")));
                    return None;
                }
                let offset = toks.get(i + 1).filter(|t| !kw(t)).and_then(|t| at(t));
                i += if offset.is_some() { 2 } else { 1 };
                let mut pos = at(k)?;
                if let Some(o) = offset {
                    // From the far edge, an offset counts back towards the near one.
                    let sign = if matches!(k, "right" | "bottom") { -1.0 } else { 1.0 };
                    pos.pct += sign * o.pct;
                    pos.px += sign * o.px;
                }
                let axis = if vertical(k) { 1 } else if horizontal(k) { 0 } else if axes[0].is_none() { 0 } else { 1 };
                axes[axis] = Some(pos);
            }
            [axes[0].unwrap_or(BgPos { pct: 50.0, px: 0.0 }), axes[1].unwrap_or(BgPos { pct: 50.0, px: 0.0 })]
        }
    };
    Some(out)
}

/// `background-position-x` or `-y`: a keyword for the axis, an offset, or
/// both.
fn parse_bg_axis(toks: &[String], axis: usize) -> Option<BgPos> {
    // Read as the whole property, the other axis at its centre on its side.
    let centre = ["center".to_string()];
    let both = if axis == 0 { [toks, &centre].concat() } else { [&centre[..], toks].concat() };
    parse_bg_position(&both).map(|p| p[axis])
}

/// `background-repeat`: one keyword for both axes, or two, across then down.
fn parse_bg_repeat(toks: &[&str]) -> Option<[BgRepeat; 2]> {
    let one = |t: &str| match t {
        "repeat" => Some(BgRepeat::Repeat),
        "no-repeat" => Some(BgRepeat::NoRepeat),
        "space" => Some(BgRepeat::Space),
        "round" => Some(BgRepeat::Round),
        _ => None,
    };
    let r = match toks {
        [] => return None,
        ["repeat-x"] => [BgRepeat::Repeat, BgRepeat::NoRepeat],
        ["repeat-y"] => [BgRepeat::NoRepeat, BgRepeat::Repeat],
        [a] => [one(a)?; 2],
        [a, b] => [one(a)?, one(b)?],
        _ => return None,
    };
    Some(r)
}

/// `border-box`, `padding-box` or `content-box`. `text`, which clips a
/// background to the glyphs, is not drawn and says so.
fn parse_bg_box(v: &str) -> Option<BgBox> {
    match v.trim() {
        "border-box" => Some(BgBox::Border),
        "padding-box" => Some(BgBox::Padding),
        "content-box" => Some(BgBox::Content),
        _ => None,
    }
}

fn parse_bg_attachment(v: &str) -> Option<BgAttachment> {
    match v.trim() {
        "scroll" => Some(BgAttachment::Scroll),
        "fixed" => Some(BgAttachment::Fixed),
        "local" => Some(BgAttachment::Local),
        _ => None,
    }
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
    // A leading angle / `to <side>` sets the direction, with or without an
    // `in <space>` beside it; otherwise it's a stop.
    let (lead, space, hue) = interpolation_of(parts[0].trim());
    let angle = parse_gradient_angle(lead);
    if angle.is_some() || (lead.is_empty() && space.is_some()) {
        parts.remove(0);
    }
    let stops = parse_stops(&parts)?;
    Some(Gradient {
        kind: GradientKind::Linear {
            angle: angle.unwrap_or(std::f32::consts::PI), // default: to bottom
        },
        stops: stops.into(),
        space: space.unwrap_or_default(),
        hue,
    })
}

/// A gradient's first part with its `in <space> [<way> hue]` taken off: what
/// is left (the direction, or nothing), the space, and the way round. Tailwind
/// writes `in oklab` into every gradient it makes. A space Rux has no mixing
/// for is said, and mixes in sRGB.
fn interpolation_of(part: &str) -> (&str, Option<rux_layout::GradientSpace>, rux_layout::HueWay) {
    use rux_layout::{GradientSpace as S, HueWay as H};
    let lower = part.to_ascii_lowercase();
    let at = if lower.starts_with("in ") { Some(0) } else { lower.find(" in ").map(|i| i + 1) };
    let Some(at) = at else { return (part, None, H::Shorter) };
    let words: Vec<&str> = lower[at + 3..].split_whitespace().collect();
    let space = match words.first().copied() {
        Some("srgb") => S::Srgb,
        Some("srgb-linear") => S::LinearSrgb,
        Some("oklab") => S::Oklab,
        Some("oklch") => S::Oklch,
        Some("lab") => S::Lab,
        Some("lch") => S::Lch,
        Some("hsl") => S::Hsl,
        Some("hwb") => S::Hwb,
        other => {
            warn_once(format!("a gradient `in {}` is mixed in sRGB; Rux mixes in srgb, srgb-linear, oklab, oklch, lab, lch, hsl and hwb", other.unwrap_or("")));
            S::Srgb
        }
    };
    let hue = match words.get(1).copied() {
        Some("longer") => H::Longer,
        Some("increasing") => H::Increasing,
        Some("decreasing") => H::Decreasing,
        _ => H::Shorter,
    };
    (part[..at].trim(), Some(space), hue)
}

/// `radial-gradient([shape/size/at …,]? <stop>, <stop> …)`. The prelude before
/// the first stop (shape, `at …`) is accepted and ignored, we always draw a
/// centred circle to the nearest edge.
fn parse_radial_gradient(inner: &str) -> Option<Gradient> {
    let mut parts = split_top_level_commas(inner);
    if parts.is_empty() {
        return None;
    }
    // If the first segment isn't a colour stop, treat it as the (ignored) config,
    // all but the colour space it may name.
    let (_, space, hue) = interpolation_of(parts[0].trim());
    if parse_color(first(parts[0].trim())).is_none() && !parts[0].trim().is_empty() {
        parts.remove(0);
    }
    let stops = parse_stops(&parts)?;
    Some(Gradient { kind: GradientKind::Radial, stops: stops.into(), space: space.unwrap_or_default(), hue })
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
        // A bare `0` is a position too: it is how lightningcss writes the
        // second half of a hard stop, `red 50%, blue 0`.
        let at = |p: &str| {
            let pct = if p == "0" { Some(0.0) } else { p.strip_suffix('%').and_then(|n| n.trim().parse::<f32>().ok()) };
            if pct.is_none() {
                warn(format!(
                    "a gradient stop at `{p}` is placed as if it had no position; Rux places stops by percentage"
                ));
            }
            pct.map(|p| (p / 100.0).clamp(0.0, 1.0))
        };
        // `lime 25% 75%` is two stops of one colour, which lightningcss
        // writes for `lime 25%, lime 75%`.
        let pos: Vec<Option<f32>> = toks.map(at).collect();
        for p in if pos.is_empty() { vec![None] } else { pos } {
            colors.push(color);
            positions.push(p);
        }
    }
    if colors.len() < 2 {
        return None;
    }
    // A stop placed before the one ahead of it is moved up to it, as CSS
    // does, which is what makes `blue 0` after `red 50%` a hard stop at 50%.
    let mut furthest = 0.0f32;
    for p in positions.iter_mut().flatten() {
        furthest = furthest.max(*p);
        *p = furthest;
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

/// Parse a `transition` value: a comma-separated list of entries, each naming a
/// property and, in any order after it, a duration, an easing and a delay
/// (`opacity 150ms ease-out, transform .3s`).
///
/// CSS's own rule for the two times is positional, not semantic: the *first*
/// time is the duration and the second is the delay, which is why they are
/// counted rather than matched.
///
/// A property that cannot be animated is dropped with a warning naming it. The
/// alternative, silently ignoring it, produces an element that simply never
/// animates and no way to find out why.
fn parse_transitions(value: &str) -> Vec<Transition> {
    let mut out = Vec::new();
    for entry in split_top_level_commas(value) {
        let entry = entry.trim();
        if entry.is_empty() || entry.eq_ignore_ascii_case("none") {
            continue;
        }
        let mut property = None;
        let mut times = Vec::new();
        let mut easing = None;
        let mut unknown = None;
        for token in split_top_level(entry) {
            if let Some(ms) = parse_time(token) {
                times.push(ms);
            } else if let Some(e) = parse_easing(token) {
                easing = Some(e);
            } else if token.contains('(') {
                // An easing function Rux does not draw, or one written wrong.
                // Said rather than left to fall back to `ease` unexplained.
                warn_once(format!(
                    "`{token}` is not a timing function Rux knows, so the transition eases with `ease`. \
                     Use linear, ease, ease-in, ease-out, ease-in-out, cubic-bezier(), steps(), \
                     step-start or step-end."
                ));
            } else if property.is_none() && unknown.is_none() {
                match parse_anim_prop(token) {
                    Some(p) => property = Some(p),
                    None => unknown = Some(token.to_string()),
                }
            }
        }
        if let Some(name) = unknown {
            warn_stylesheet(anim_prop_help(&name));
            continue;
        }
        // A bare `transition: 200ms` is `all`, as in CSS.
        let property = property.unwrap_or(AnimProp::All);
        let duration = times.first().copied().unwrap_or(0.0);
        let delay = times.get(1).copied().unwrap_or(0.0);
        out.push(Transition {
            property,
            duration,
            delay,
            easing: easing.unwrap_or(Easing::EASE),
        });
    }
    out
}

/// The longhands `transition` sets. See [`declare`], which takes them away
/// when the shorthand comes after them.
const TRANSITION_LONGHANDS: &[&str] = &[
    "transition-property",
    "transition-duration",
    "transition-timing-function",
    "transition-delay",
    "transition-behavior",
];

/// The element's transitions as one `transition` value: the shorthand as
/// written, or, when any longhand is set, the lists put back together the
/// way CSS does. There are as many transitions as `transition-property`
/// names, and each other list is repeated to that length.
///
/// That is how Tailwind writes every `transition` utility, a list of
/// properties with the duration and the easing beside it, and a list of
/// that kind names properties Rux cannot animate alongside the ones it can.
/// A discrete property (`display`), a custom property, and the individual
/// transforms read as `transform`, are taken quietly; anything else is said
/// once for the list, since those change at once rather than not at all.
fn transition_value(p: &HashMap<String, String>) -> Option<String> {
    let longhand = |name: &str| p.get(name).map(|v| split_top_level_commas(v).into_iter().map(str::to_string).collect::<Vec<_>>());
    if !TRANSITION_LONGHANDS.iter().any(|n| p.contains_key(*n)) {
        return p.get("transition").cloned();
    }
    // What the shorthand set, as lists, under what the longhands say.
    let (mut props, mut durations, mut easings, mut delays) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for entry in p.get("transition").map(|v| split_top_level_commas(v)).unwrap_or_default() {
        let (mut prop, mut times, mut easing) = (None, Vec::new(), None);
        for token in split_top_level(entry) {
            if parse_time(token).is_some() {
                times.push(token.to_string());
            } else if parse_easing(token).is_some() || token.contains('(') {
                easing = Some(token.to_string());
            } else if !matches!(token, "normal" | "allow-discrete") {
                prop = Some(token.to_string());
            }
        }
        props.push(prop.unwrap_or_else(|| "all".into()));
        durations.push(times.first().cloned().unwrap_or_else(|| "0s".into()));
        delays.push(times.get(1).cloned().unwrap_or_else(|| "0s".into()));
        easings.push(easing.unwrap_or_else(|| "ease".into()));
    }
    let or = |list: Option<Vec<String>>, from: Vec<String>, default: &str| {
        let list = list.unwrap_or(from);
        if list.is_empty() { vec![default.to_string()] } else { list }
    };
    let props = or(longhand("transition-property"), props, "all");
    let durations = or(longhand("transition-duration"), durations, "0s");
    let easings = or(longhand("transition-timing-function"), easings, "ease");
    let delays = or(longhand("transition-delay"), delays, "0s");
    if props.len() == 1 && props[0].eq_ignore_ascii_case("none") {
        return Some("none".into());
    }
    let mut unanimated = Vec::new();
    let mut entries = Vec::new();
    for (i, name) in props.iter().enumerate() {
        let lower = name.trim().to_ascii_lowercase();
        let name = match lower.as_str() {
            "display" | "visibility" | "content-visibility" | "overlay" | "pointer-events" => continue,
            n if n.starts_with("--") => continue,
            n if parse_anim_prop(n).is_some() => n,
            n if props.len() > 1 => {
                unanimated.push(n.to_string());
                continue;
            }
            // Alone, it is said by `parse_transitions`, as the shorthand is.
            n => n,
        };
        let pick = |list: &[String]| list[i % list.len()].clone();
        entries.push(format!("{name} {} {} {}", pick(&durations), pick(&easings), pick(&delays)));
    }
    if !unanimated.is_empty() {
        warn_once(format!(
            "`transition-property` names {}, which Rux does not animate yet, so {} change at once",
            unanimated.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", "),
            if unanimated.len() == 1 { "it" } else { "they" }
        ));
    }
    Some(entries.join(", "))
}

/// A CSS `<time>`: `250ms`, `.3s`, `0`. Unitless is only legal as zero, and a
/// bare number that is not zero is not a time at all (it is a stray token), so
/// it parses as `None` and is ignored rather than being read as seconds.
fn parse_time(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(n) = s.strip_suffix("ms") {
        return n.trim().parse::<f32>().ok();
    }
    if let Some(n) = s.strip_suffix('s') {
        return n.trim().parse::<f32>().ok().map(|v| v * 1000.0);
    }
    (s == "0").then_some(0.0)
}

/// A timing-function keyword, `cubic-bezier(…)` or `steps(…)`.
fn parse_easing(s: &str) -> Option<Easing> {
    let s = s.trim().to_ascii_lowercase();
    match s.as_str() {
        "linear" => return Some(Easing::Linear),
        "ease" => return Some(Easing::EASE),
        "ease-in" => return Some(Easing::EASE_IN),
        "ease-out" => return Some(Easing::EASE_OUT),
        "ease-in-out" => return Some(Easing::EASE_IN_OUT),
        "step-start" => return Some(Easing::Steps(1, StepPosition::Start)),
        "step-end" => return Some(Easing::Steps(1, StepPosition::End)),
        _ => {}
    }
    if let Some(args) = s.strip_prefix("steps(").and_then(|a| a.strip_suffix(')')) {
        let mut parts = args.split(',').map(str::trim);
        let n: u32 = parts.next()?.parse().ok()?;
        let at = match parts.next() {
            None | Some("end" | "jump-end") => StepPosition::End,
            Some("start" | "jump-start") => StepPosition::Start,
            Some("jump-both") => StepPosition::Both,
            Some("jump-none") => StepPosition::None,
            Some(_) => return None,
        };
        // CSS refuses no steps at all, and `jump-none` with one: it would be
        // a single step that never jumps.
        let least = if at == StepPosition::None { 2 } else { 1 };
        return (n >= least && parts.next().is_none()).then_some(Easing::Steps(n, at));
    }
    let args = s.strip_prefix("cubic-bezier(")?.strip_suffix(')')?;
    let n: Vec<f32> = args.split(',').filter_map(|a| a.trim().parse::<f32>().ok()).collect();
    match n[..] {
        // The x coordinates are the parameter's own axis: outside 0..=1 the
        // curve is not a function of time any more and cannot be inverted.
        [x1, y1, x2, y2] if (0.0..=1.0).contains(&x1) && (0.0..=1.0).contains(&x2) => {
            Some(Easing::Bezier(x1, y1, x2, y2))
        }
        _ => None,
    }
}

/// A property name in a `transition`, or `None` if it is not animatable.
fn parse_anim_prop(s: &str) -> Option<AnimProp> {
    Some(match s.trim().to_ascii_lowercase().as_str() {
        "all" => AnimProp::All,
        "opacity" => AnimProp::Opacity,
        "background-color" | "background" => AnimProp::BackgroundColor,
        "color" => AnimProp::Color,
        "border-color" => AnimProp::BorderColor,
        "fill" => AnimProp::Fill,
        "stroke" => AnimProp::Stroke,
        "stroke-width" => AnimProp::StrokeWidth,
        "z-index" => AnimProp::ZIndex,
        // The geometry itself, spelled as SVG spells the attribute.
        "d" => AnimProp::PathData,
        "border-width" | "border" => AnimProp::BorderWidth,
        "border-radius" => AnimProp::BorderRadius,
        "width" => AnimProp::Width,
        "height" => AnimProp::Height,
        "padding" => AnimProp::Padding,
        "margin" => AnimProp::Margin,
        "gap" => AnimProp::Gap,
        "font-size" => AnimProp::FontSize,
        // The individual transforms are part of the one matrix.
        "transform" | "translate" | "rotate" | "scale" => AnimProp::Transform,
        "inset" | "top" | "right" | "bottom" | "left" => AnimProp::Inset,
        "box-shadow" => AnimProp::BoxShadow,
        "outline-color" | "outline" => AnimProp::OutlineColor,
        "text-decoration-color" => AnimProp::TextDecorationColor,
        "filter" => AnimProp::Filter,
        "backdrop-filter" | "-webkit-backdrop-filter" => AnimProp::BackdropFilter,
        _ => return None,
    })
}

/// The diagnostic for a `transition` naming something that cannot animate. A
/// longhand of a property that *can* gets pointed at the shorthand, because
/// that is the actual mistake and the fix is one word.
fn anim_prop_help(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let shorthand = [
        ("padding", "padding"),
        ("margin", "margin"),
        ("border-", "border-width"),
        ("row-gap", "gap"),
        ("column-gap", "gap"),
    ]
    .iter()
    .find(|(prefix, _)| lower.starts_with(prefix) && lower != *prefix)
    .map(|(_, whole)| *whole);
    match shorthand {
        Some(whole) => format!(
            "`transition: {name}` does nothing: the sides animate together, so \
             transition `{whole}` instead"
        ),
        None => {
            let animatable: Vec<&str> = AnimProp::EVERY.iter().map(|p| p.name()).collect();
            format!(
                "`transition: {name}` does nothing: `{name}` cannot be animated. \
                 Animatable properties are {}, and `all`",
                animatable.join(", ")
            )
        }
    }
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

/// `translate`, `rotate` and `scale`, the properties, put in front of
/// `transform` in the order CSS applies them: translate, then rotate, then
/// scale, then the transform list. `none` is no change; a 3D value is read
/// for its 2D part, a rotation about another axis than z as none.
fn individual_transforms(p: &HashMap<String, String>, transform: Option<Transform>) -> Option<Transform> {
    let value = |k: &str| p.get(k).map(|v| v.trim()).filter(|v| !v.eq_ignore_ascii_case("none"));
    let mut m: Transform = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut any = transform.is_some();
    if let Some(v) = value("translate") {
        // A percentage in either half makes it `translate_pct`'s.
        let parts = split_top_level(v);
        if let Some(px) = parts.iter().map(|s| parse_px(s)).collect::<Option<Vec<f32>>>() {
            let (tx, ty) = (px.first().copied().unwrap_or(0.0), px.get(1).copied().unwrap_or(0.0));
            m = mat_mul(m, [1.0, 0.0, 0.0, 1.0, tx, ty]);
            any = true;
        }
    }
    if let Some(v) = value("rotate") {
        let parts = split_top_level(v);
        // `45deg`, `z 45deg` or `0 0 1 45deg`.
        let about_z = match parts.len() {
            1 => true,
            2 => parts[0].eq_ignore_ascii_case("z"),
            4 => parts[..3].iter().map(|s| s.parse::<f32>().ok()).collect::<Option<Vec<_>>>()
                .is_some_and(|a| a[0] == 0.0 && a[1] == 0.0 && a[2] > 0.0),
            _ => false,
        };
        if let (true, Some(angle)) = (about_z, parts.last().and_then(|s| parse_angle(s))) {
            let (sin, cos) = angle.sin_cos();
            m = mat_mul(m, [cos, sin, -sin, cos, 0.0, 0.0]);
            any = true;
        }
    }
    if let Some(v) = value("scale") {
        let factor = |s: &str| match s.strip_suffix('%') {
            Some(pct) => pct.trim().parse::<f32>().ok().map(|n| n / 100.0),
            None => s.parse::<f32>().ok(),
        };
        let parts = split_top_level(v);
        if let Some(sx) = parts.first().and_then(|s| factor(s)) {
            let sy = parts.get(1).and_then(|s| factor(s)).unwrap_or(sx);
            m = mat_mul(m, [sx, 0.0, 0.0, sy, 0.0, 0.0]);
            any = true;
        }
    }
    any.then(|| mat_mul(m, transform.unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])))
}

/// One `transform` function to an affine, or `None` if unrecognised.
fn transform_fn(name: &str, args: &str) -> Option<Transform> {
    let nums: Vec<&str> = args.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    let num = |i: usize| nums.get(i).and_then(|s| s.parse::<f32>().ok());
    match name {
        // Not when either half is a percentage: that is `translate_pct`'s.
        "translate" => {
            let tx = parse_px(nums.first()?)?;
            let ty = match nums.get(1) {
                Some(s) => parse_px(s)?,
                None => 0.0,
            };
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

/// `box-shadow`: a comma-separated list of `<dx> <dy> <blur>? <spread>?
/// <color>?`, each optionally `inset`. `none` is no shadow, and so is a list
/// none of whose entries parse.
///
/// A shadow with no colour is `currentColor` in CSS, which is only known in
/// the cascade: [`resolve_colors`] writes it in before this runs, and black
/// here is for a value that never went through one.
fn parse_box_shadow(value: &str) -> rux_layout::Shadows {
    let list: Vec<BoxShadow> = split_top_level_commas(value).into_iter().filter_map(parse_one_shadow).collect();
    (!list.is_empty()).then(|| list.into())
}

fn parse_one_shadow(first: &str) -> Option<BoxShadow> {
    let first = first.trim();
    if first.is_empty() || first == "none" {
        return None;
    }
    let mut lengths = Vec::new();
    let mut color_parts = Vec::new();
    let mut inset = false;
    for tok in split_top_level(first) {
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
/// A `resolution` value to device pixels per CSS pixel.
///
/// `dppx` is the ratio itself, `x` is its shorthand, and `dpi` and `dpcm` are
/// that ratio against the CSS reference of 96 dots per inch. A bare number is
/// refused, as CSS refuses it: `resolution: 2` says nothing about what unit.
fn parse_resolution(v: &str) -> Option<f32> {
    let v = v.trim().to_ascii_lowercase();
    for (suffix, per_dppx) in [("dppx", 1.0), ("dpcm", 96.0 / 2.54), ("dpi", 96.0), ("x", 1.0)] {
        if let Some(number) = v.strip_suffix(suffix) {
            let n: f32 = number.trim().parse().ok()?;
            return (n > 0.0).then_some(n / per_dppx);
        }
    }
    None
}

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

/// `line-height` as it inherits: a bare number stays a number, anything else
/// is the pixels it comes to here (`em` is already pixels by now). `normal`
/// is `None`, which puts a child back on the font's own metrics.
fn line_height_spec(v: &str) -> Option<LineHeight> {
    let s = first(v);
    if let Ok(n) = s.parse::<f32>() {
        return Some(LineHeight::Factor(n));
    }
    if let Some(pct) = s.strip_suffix('%').and_then(|p| p.trim().parse::<f32>().ok()) {
        return Some(LineHeight::Factor(pct / 100.0));
    }
    parse_line_height(v, 1.0).map(LineHeight::Px)
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

/// `content-visibility` and `contain-intrinsic-size` (with its longhands,
/// the logical ones as horizontal text has them).
fn interpret_content_visibility(p: &HashMap<String, String>, st: &mut Style) {
    if let Some(v) = p.get("content-visibility") {
        st.content_visibility = match v.trim() {
            "visible" => ContentVisibility::Visible,
            "auto" => ContentVisibility::Auto,
            "hidden" => ContentVisibility::Hidden,
            other => {
                warn(format!("`content-visibility: {other}` is not one Rux knows; write `visible`, `auto` or `hidden`"));
                ContentVisibility::Visible
            }
        };
    }
    // One axis: `none`, a length, `auto <length>` or `auto none`.
    let axis = |property: &str, v: &str| -> Option<IntrinsicSize> {
        let words: Vec<&str> = v.split_whitespace().collect();
        let (auto, rest) = match words.as_slice() {
            ["auto", rest] => (true, *rest),
            [rest] => (false, *rest),
            _ => {
                warn(format!("`{property}: {v}` is not a size Rux understands; write `200px`, `auto 200px` or `none`"));
                return None;
            }
        };
        if rest == "none" {
            return Some(IntrinsicSize { auto, len: None });
        }
        match parse_px(rest) {
            Some(px) if px >= 0.0 => Some(IntrinsicSize { auto, len: Some(px) }),
            _ => {
                warn(format!("`{property}: {v}` is not a size Rux understands; write `200px`, `auto 200px` or `none`"));
                None
            }
        }
    };
    let mut sizes: [Option<IntrinsicSize>; 2] = [None, None];
    if let Some(v) = p.get("contain-intrinsic-size") {
        // One size for both, or width then height, each maybe `auto`.
        let words: Vec<&str> = v.split_whitespace().collect();
        let split = match words.as_slice() {
            [a] => Some((a.to_string(), a.to_string())),
            ["auto", a] => Some((format!("auto {a}"), format!("auto {a}"))),
            [a, b] if *a != "auto" => Some((a.to_string(), b.to_string())),
            ["auto", a, b] if *b != "auto" => Some((format!("auto {a}"), b.to_string())),
            [a, "auto", b] if *a != "auto" => Some((a.to_string(), format!("auto {b}"))),
            ["auto", a, "auto", b] => Some((format!("auto {a}"), format!("auto {b}"))),
            _ => None,
        };
        match split {
            Some((w, h)) => sizes = [axis("contain-intrinsic-size", &w), axis("contain-intrinsic-size", &h)],
            None => warn(format!(
                "`contain-intrinsic-size: {}` is not a size Rux understands; write `200px`, `auto 200px` or `auto 300px auto 200px`",
                v.trim()
            )),
        }
    }
    for (property, i) in [
        ("contain-intrinsic-width", 0),
        ("contain-intrinsic-inline-size", 0),
        ("contain-intrinsic-height", 1),
        ("contain-intrinsic-block-size", 1),
    ] {
        if let Some(v) = p.get(property) {
            sizes[i] = axis(property, v.trim());
        }
    }
    if sizes.iter().any(Option::is_some) {
        st.contain_intrinsic = Some(Box::new([sizes[0].unwrap_or_default(), sizes[1].unwrap_or_default()]));
    }
}

/// Split `value` where `at` says, outside any brackets or braces.
fn split_outside(value: &str, at: impl Fn(char) -> bool) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in value.char_indices() {
        match c {
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            c if depth == 0 && at(c) => {
                out.push(value[start..i].trim());
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(value[start..].trim());
    out.into_iter().filter(|t| !t.is_empty()).collect()
}

/// `position-try-fallbacks` (or the `position-try` shorthand) with each
/// `@position-try` name it uses followed by that rule's declarations in
/// braces, `--above{bottom: anchor(top)}`, so interpreting the box needs
/// nothing but its own declarations. A name with no rule is dropped from the
/// list and said, as CSS skips it.
fn expand_position_tries(value: &str, tries: &HashMap<String, String>) -> String {
    let options: Vec<String> = split_outside(value, |c| c == ',')
        .into_iter()
        .filter_map(|option| {
            let mut words = Vec::new();
            for word in split_outside(option, char::is_whitespace) {
                if word.starts_with("--") {
                    match tries.get(word) {
                        Some(body) => words.push(format!("{word}{{{}}}", body.trim())),
                        None => {
                            warn(format!(
                                "`position-try-fallbacks` names `{word}`, and there is no `@position-try {word}` rule, so that fallback is skipped"
                            ));
                            return None;
                        }
                    }
                } else {
                    words.push(word.to_string());
                }
            }
            Some(words.join(" "))
        })
        .collect();
    if options.is_empty() {
        "none".to_string()
    } else {
        options.join(", ")
    }
}

/// What a `@position-try` rule may set and Rux honors there: the insets,
/// the margins and `position-anchor`.
const POSITION_TRY_PROPERTIES: &[&str] = &[
    "top", "right", "bottom", "left",
    "margin", "margin-top", "margin-right", "margin-bottom", "margin-left",
    "position-anchor",
];

/// `position-try-fallbacks`, `position-try-order` and the `position-try`
/// shorthand, onto `st.anchor`.
fn interpret_position_try(p: &HashMap<String, String>, st: &mut Style) {
    let order_of = |w: &str| match w {
        "normal" => Some(TryOrder::Normal),
        "most-width" | "most-inline-size" => Some(TryOrder::MostWidth),
        "most-height" | "most-block-size" => Some(TryOrder::MostHeight),
        _ => None,
    };
    let mut order = None;
    let mut fallbacks = None;
    if let Some(v) = p.get("position-try") {
        let v = v.trim();
        let (head, rest) = v.split_once(char::is_whitespace).unwrap_or((v, ""));
        match order_of(head) {
            Some(o) => {
                order = Some(o);
                fallbacks = Some(rest.trim().to_string());
            }
            None => fallbacks = Some(v.to_string()),
        }
    }
    if let Some(v) = p.get("position-try-order") {
        match order_of(v.trim()) {
            Some(o) => order = Some(o),
            None => warn(format!(
                "`position-try-order: {}` is not one Rux knows; write `normal`, `most-width`, `most-height`, `most-inline-size` or `most-block-size`",
                v.trim()
            )),
        }
    }
    if let Some(v) = p.get("position-try-fallbacks") {
        fallbacks = Some(v.trim().to_string());
    }
    let tries: Vec<PositionTry> = match fallbacks.as_deref() {
        None | Some("") | Some("none") => Vec::new(),
        Some(v) => split_outside(v, |c| c == ',').into_iter().filter_map(parse_position_try).collect(),
    };
    if tries.is_empty() && order.is_none() {
        return;
    }
    let a = st.anchor.get_or_insert_with(Default::default);
    a.tries = tries;
    a.try_order = order.unwrap_or_default();
}

/// One entry of `position-try-fallbacks`, already expanded: an optional
/// `--name{declarations}` and any of the three flips.
fn parse_position_try(option: &str) -> Option<PositionTry> {
    let mut t = PositionTry::default();
    for word in split_outside(option, char::is_whitespace) {
        match word {
            "flip-block" => t.flip_block = true,
            "flip-inline" => t.flip_inline = true,
            "flip-start" => t.flip_start = true,
            w if w.starts_with("--") => {
                let body = w.split_once('{').map(|(_, b)| b.trim_end_matches('}')).unwrap_or("");
                position_try_rule(body, &mut t);
            }
            other => {
                warn(format!(
                    "`{other}` in `position-try-fallbacks` is not honored yet: Rux takes `flip-block`, `flip-inline`, `flip-start` and `@position-try` names, not `position-area` values, so that fallback is skipped"
                ));
                return None;
            }
        }
    }
    Some(t)
}

/// A `@position-try` rule's declarations onto `t`: only the sides it names
/// replace the box's own.
fn position_try_rule(body: &str, t: &mut PositionTry) {
    let mut decls = HashMap::new();
    for decl in split_outside(body, |c| c == ';') {
        let Some((k, v)) = decl.split_once(':') else { continue };
        let key = k.trim().to_ascii_lowercase();
        if !POSITION_TRY_PROPERTIES.contains(&key.as_str()) {
            warn(format!(
                "`{key}` in a `@position-try` rule is not honored yet; Rux takes the insets (`top`, `right`, `bottom`, `left`), the margins and `position-anchor` there"
            ));
            continue;
        }
        decls.insert(key, v.trim().to_string());
    }
    let st = interpret(&decls);
    for (i, side) in INSETS.iter().enumerate() {
        if decls.contains_key(*side) {
            t.inset[i] = Some(match (st.anchor.as_ref().and_then(|a| a.inset[i].clone()), st.inset[i]) {
                (Some(r), _) => TryInset::Anchor(r),
                (None, Some(l)) => TryInset::Len(l),
                (None, None) => TryInset::Auto,
            });
        }
    }
    let px = [st.margin.top, st.margin.right, st.margin.bottom, st.margin.left];
    for (i, side) in INSETS.iter().enumerate() {
        if decls.contains_key("margin") || decls.contains_key(&format!("margin-{side}")) {
            t.margin[i] = Some(px[i]);
            t.margin_pct[i] = st.margin_pct[i];
        }
    }
    if let Some(name) = st.anchor.as_ref().and_then(|a| a.position_anchor.clone()) {
        t.position_anchor = Some(name);
    }
}

/// `anchor(--name side, fallback)` in an inset. `vertical` is whether it is
/// in `top` or `bottom`, which decides which sides make sense: `top:
/// anchor(left)` measures along the wrong axis, and CSS calls it invalid.
fn parse_anchor(v: &str, vertical: bool) -> Result<AnchorRef, String> {
    let shape = "is not an `anchor()` Rux understands; write `anchor(bottom)`, `anchor(--name top)` or `anchor(bottom, 8px)`";
    let inner = v.trim().strip_prefix("anchor(").and_then(|r| r.strip_suffix(')')).ok_or(shape)?;
    let (main, fallback) = match inner.split_once(',') {
        Some((m, f)) => (m, Some(parse_len(f.trim()).ok_or(shape)?)),
        None => (inner, None),
    };
    let mut name = None;
    let mut side = None;
    for token in main.split_whitespace() {
        if token.starts_with("--") && name.is_none() {
            name = Some(token.to_string());
            continue;
        }
        if side.is_some() {
            return Err(shape.to_string());
        }
        side = Some(match token {
            "top" | "bottom" if !vertical => {
                return Err(format!(
                    "measures along the wrong axis: a `left` or `right` inset takes `anchor(left)`, `anchor(right)` or `anchor(center)`, not `{token}`"
                ))
            }
            "left" | "right" if vertical => {
                return Err(format!(
                    "measures along the wrong axis: a `top` or `bottom` inset takes `anchor(top)`, `anchor(bottom)` or `anchor(center)`, not `{token}`"
                ))
            }
            "top" => AnchorSide::Top,
            "bottom" => AnchorSide::Bottom,
            "left" => AnchorSide::Left,
            "right" => AnchorSide::Right,
            "center" => AnchorSide::Center,
            pct => match parse_len(pct) {
                Some(Len::Pct(f)) => AnchorSide::Frac(f),
                _ => return Err(shape.to_string()),
            },
        });
    }
    Ok(AnchorRef { name, side: side.ok_or(shape)?, fallback })
}

/// `anchor-size(--name width, fallback)` in `width` or `height`.
fn parse_anchor_size(v: &str) -> Option<AnchorSize> {
    let inner = v.trim().strip_prefix("anchor-size(")?.strip_suffix(')')?;
    let (main, fallback) = match inner.split_once(',') {
        Some((m, f)) => (m, Some(parse_len(f.trim())?)),
        None => (inner, None),
    };
    let mut name = None;
    let mut height = None;
    for token in main.split_whitespace() {
        match token {
            t if t.starts_with("--") && name.is_none() => name = Some(t.to_string()),
            "width" | "inline" if height.is_none() => height = Some(false),
            "height" | "block" if height.is_none() => height = Some(true),
            _ => return None,
        }
    }
    Some(AnchorSize { name, height: height?, fallback })
}

/// The first word of a value, a function call with spaces inside it
/// (`calc(100% - 20px)`) counting as one.
fn first(s: &str) -> &str {
    let s = s.trim_start();
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c.is_whitespace() && depth <= 0 => return &s[..i],
            _ => {}
        }
    }
    s
}

/// What a unit is worth, for [`resolve_units`].
enum Unit {
    /// So many pixels per unit.
    Px(f32),
    /// A viewport unit: `w` and `h` are how much of the width and the
    /// height it is, `min` and `max` pick the smaller or larger of the two.
    Viewport { name: &'static str, w: f32, h: f32, min: bool, max: bool },
}

/// The units [`resolve_units`] rewrites, against an element whose
/// `font-size` is `font_size`. `rem` is not one: every parser reads it.
fn unit_of(unit: &str, font_size: f32) -> Option<Unit> {
    let vp = |name, w, h, min, max| Unit::Viewport { name, w, h, min, max };
    Some(match unit {
        "em" => Unit::Px(font_size),
        // CSS's own fallback for a font Rux cannot ask: half an em for the
        // width of `0` and the height of `x`, and 1.2 em for a `normal`
        // line.
        "ch" | "ex" => Unit::Px(font_size * 0.5),
        "cap" => Unit::Px(font_size * 0.7),
        "ic" => Unit::Px(font_size),
        "lh" => Unit::Px(font_size * 1.2),
        "rlh" => Unit::Px(REM_PX * 1.2),
        "pt" => Unit::Px(96.0 / 72.0),
        "pc" => Unit::Px(16.0),
        "in" => Unit::Px(96.0),
        "cm" => Unit::Px(96.0 / 2.54),
        "mm" => Unit::Px(96.0 / 25.4),
        "q" => Unit::Px(96.0 / 101.6),
        // The small, large and dynamic viewports are one here: no browser
        // chrome comes and goes over a Rux window. `vi` and `vb` are `vw` and
        // `vh` in horizontal text.
        "vw" | "svw" | "lvw" | "dvw" | "vi" | "svi" | "lvi" | "dvi" => vp("vw", 1.0, 0.0, false, false),
        "vh" | "svh" | "lvh" | "dvh" | "vb" | "svb" | "lvb" | "dvb" => vp("vh", 0.0, 1.0, false, false),
        "vmin" | "svmin" | "lvmin" | "dvmin" => vp("vmin", 0.0, 0.0, true, false),
        "vmax" | "svmax" | "lvmax" | "dvmax" => vp("vmax", 0.0, 0.0, false, true),
        _ => return None,
    })
}

/// Rewrite the lengths in a declaration value that only the element can
/// resolve: `em` and the other font units against `font_size`, and the
/// absolute ones (`pt`, `in`, `cm`, …) into pixels.
///
/// Done as a pass over the value rather than inside each length parser, the
/// same way `var()` is, because `em` needs the element's font size and the
/// parsers are reached without one.
///
/// **Viewport units** are put in pixels against `viewport` when it is given:
/// for a property resolved here, during the cascade, which has a window size
/// to hand and is cascaded again when it changes (see [`reads_viewport`]).
/// Without one they are left as `vw`, `vh`, `vmin` or `vmax` for the layout,
/// which resolves `width: 50vw` against the window each frame.
///
/// `rem` is deliberately left alone: it is absolute against the root size,
/// and the parsers already understand it.
fn resolve_units(value: &str, font_size: f32, viewport: Option<(f32, f32)>) -> String {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    while i < value.len() {
        // A candidate starts at a digit or a `.` that begins a number, and only
        // when the previous character cannot make it part of a longer word.
        let starts_number = bytes[i].is_ascii_digit()
            || (bytes[i] == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit));
        let boundary = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'.' || bytes[i - 1] == b'#');
        if !(starts_number && boundary) {
            let c = value[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
            continue;
        }
        let mut j = i;
        while j < value.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'.') {
            j += 1;
        }
        let mut k = j;
        while k < value.len() && bytes[k].is_ascii_alphabetic() {
            k += 1;
        }
        // `2em` yes; `2ems` and `2em5` no: the unit is the whole word.
        let whole = !bytes.get(k).is_some_and(|c| c.is_ascii_digit());
        let unit = whole.then(|| unit_of(&value[j..k].to_ascii_lowercase(), font_size)).flatten();
        match (value[i..j].parse::<f32>(), unit) {
            (Ok(n), Some(Unit::Px(px))) => {
                out.push_str(&format!("{}px", n * px));
                i = k;
            }
            (Ok(n), Some(Unit::Viewport { name, w, h, min, max })) => {
                match viewport {
                    Some((vw, vh)) => {
                        READS_VIEWPORT.with(|r| r.set(true));
                        let per = if min {
                            vw.min(vh)
                        } else if max {
                            vw.max(vh)
                        } else {
                            vw * w + vh * h
                        };
                        out.push_str(&format!("{}px", n * per / 100.0));
                    }
                    None => out.push_str(&format!("{n}{name}")),
                }
                i = k;
            }
            _ => {
                out.push_str(&value[i..j]);
                i = j;
            }
        }
    }
    out
}

thread_local! {
    /// Whether a build on this thread has put a viewport unit in pixels
    /// during the cascade, so a resize has to cascade again. See
    /// [`resolve_units`] and [`reads_viewport`].
    static READS_VIEWPORT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether a build has resolved a viewport unit (`padding: 2vw`) during the
/// cascade, so its pixels go stale when the window is resized. Never
/// cleared: a document that has once asked costs a re-cascade per resize,
/// as a breakpoint does, rather than a stale padding.
pub fn reads_viewport() -> bool {
    READS_VIEWPORT.with(std::cell::Cell::get)
}

/// The properties whose lengths the layout resolves, which keep their
/// viewport units for it: everything else is in pixels by the end of the
/// cascade.
const LAYOUT_LENGTHS: &[&str] = &[
    "width", "height", "min-width", "max-width", "min-height", "max-height",
    "top", "right", "bottom", "left", "flex-basis", "flex",
];

/// Whether [`resolve_units`] has anything to do in `value`: a number
/// followed by a unit it rewrites. Most values have none (`12px`, `#fff`,
/// `flex`), and the pass would copy them for nothing.
#[cfg(test)]
fn resolve_units_em(value: &str, font_size: f32) -> String {
    resolve_units(value, font_size, None)
}

fn has_units(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() && bytes.get(i + 1).is_some_and(u8::is_ascii_alphabetic) {
            let mut k = i + 1;
            while k < bytes.len() && bytes[k].is_ascii_alphabetic() {
                k += 1;
            }
            if unit_of(&value[i + 1..k].to_ascii_lowercase(), 16.0).is_some() {
                return true;
            }
            i = k;
        } else {
            i += 1;
        }
    }
    false
}

/// A length that resolves to absolute pixels: `px`, `rem`, `em`, or a bare
/// number.
///
/// This exists because `padding: 1rem` used to be dropped on the floor. The
/// px-only parser was reached by padding, margin, gap, border widths, corner
/// radii, `letter-spacing`, `box-shadow` and `translate()`, while `width` and
/// `height` went through [`parse_len`], which has understood `rem` all along.
/// So `width: 2rem` worked, `padding: 2rem` silently did nothing, and there was
/// no warning to tell them apart.
///
/// `em` is relative to `font_size`, which is the element's own resolved size
/// (its `font-size` if it declares one, otherwise the inherited one). That is
/// why this takes a parameter at all, and why `font-size` itself must resolve
/// against the *inherited* size rather than its own.
///
/// The `filter` guards `rem`: `strip_suffix("em")` matches the tail of `rem`
/// too, and without it `1rem` would be read as `1r` em and fail to parse.
fn parse_px_len(s: &str, font_size: f32) -> Option<f32> {
    let s = s.trim();
    if let Some(n) = s.strip_suffix("rem") {
        return n.trim().parse::<f32>().ok().map(|v| v * REM_PX);
    }
    if let Some(n) = s.strip_suffix("em") {
        return n.trim().parse::<f32>().ok().map(|v| v * font_size);
    }
    let n = s.strip_suffix("px").unwrap_or(s);
    n.parse::<f32>().ok()
}

/// Lengths against the root font size, for the handful of places with no
/// element in hand (shorthand sub-values reached before the size is known).
fn parse_px(s: &str) -> Option<f32> {
    parse_px_len(s, REM_PX)
}

/// One `rem` in pixels (root font size).
const REM_PX: f32 = 16.0;

/// Parse a length: `px`, `%`, `rem`, `vw`, `vh`/`dvh`. (`rem` resolves to px;
/// `dvh` is treated as `vh` since we have no dynamic browser chrome.)
fn parse_len(s: &str) -> Option<Len> {
    let s = s.trim();
    // Maths the cascade could not finish has a percentage in it, which the
    // layout works out (see `rux_layout::calc`).
    if s.ends_with(')') && rux_layout::calc::FUNCTIONS.iter().any(|f| s.starts_with(f)) {
        let c = rux_layout::calc::intern(s)?;
        return match c.get().needs() {
            n if n.basis => Some(Len::Calc(c)),
            _ => c.get().eval(None, None).map(Len::Px),
        };
    }
    if let Some(pct) = s.strip_suffix('%') {
        return pct.trim().parse::<f32>().ok().map(|v| Len::Pct(v / 100.0));
    }
    if let Some(n) = s.strip_suffix("vmin") {
        return n.trim().parse::<f32>().ok().map(Len::Vmin);
    }
    if let Some(n) = s.strip_suffix("vmax") {
        return n.trim().parse::<f32>().ok().map(Len::Vmax);
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

/// `grid-column` / `grid-row` shorthand: `<start> [/ <end>]`. A missing end is
/// `auto` (span 1), or for a name, that name again (CSS's rule, so
/// `grid-column: sidebar` covers the `sidebar` area).
fn parse_grid_shorthand(value: &str) -> (GridPlace, GridPlace) {
    let mut parts = value.splitn(2, '/');
    let start = parts.next().map(parse_grid_place).unwrap_or_default();
    let end = match parts.next() {
        Some(e) => parse_grid_place(e),
        None => match start {
            GridPlace::Named(name, 0) => GridPlace::Named(name, 0),
            _ => GridPlace::Auto,
        },
    };
    (start, end)
}

/// One placement endpoint: `auto`, a (possibly negative) line, `span <n>`,
/// a line or area name with an optional count (`header`, `col 2`), or
/// `span <name>` (`span col`, `span 2 col`).
fn parse_grid_place(side: &str) -> GridPlace {
    let words: Vec<&str> = side.split_whitespace().collect();
    let span = words.first() == Some(&"span");
    let rest = if span { &words[1..] } else { &words[..] };
    let number = rest.iter().find_map(|w| w.parse::<i16>().ok());
    let name = rest.iter().find(|w| w.parse::<i16>().is_err() && **w != "auto").map(|w| GridName::new(w));
    match (span, name, number) {
        (_, None, None) => GridPlace::Auto,
        (true, None, Some(n)) if n > 0 => GridPlace::Span(n as u16),
        (true, Some(name), n) => GridPlace::NamedSpan(name, n.filter(|n| *n > 0).unwrap_or(1) as u16),
        (false, None, Some(i)) if i != 0 => GridPlace::Line(i),
        (false, Some(name), n) => GridPlace::Named(name, n.unwrap_or(0)),
        _ => GridPlace::Auto,
    }
}

/// `grid-area`: an area name for all four sides, or up to four placements,
/// row-start / column-start / row-end / column-end. A side left out is the
/// name of the side opposite it when that is a name, and `auto` otherwise.
fn parse_grid_area(value: &str) -> [GridPlace; 4] {
    let parts: Vec<GridPlace> = value.split('/').map(parse_grid_place).collect();
    let get = |i: usize| parts.get(i).copied();
    let named = |p: GridPlace| match p {
        GridPlace::Named(n, 0) => GridPlace::Named(n, 0),
        _ => GridPlace::Auto,
    };
    let row_start = get(0).unwrap_or_default();
    let col_start = get(1).unwrap_or_else(|| named(row_start));
    let row_end = get(2).unwrap_or_else(|| named(row_start));
    let col_end = get(3).unwrap_or_else(|| named(col_start));
    [row_start, col_start, row_end, col_end]
}

/// Split a grid template on whitespace outside brackets, so `[a b]`,
/// `minmax( … )` and `repeat( … )` stay whole.
fn split_template(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start: Option<usize> = None;
    for (i, c) in value.char_indices() {
        // A `[` starts a token of its own even with no space before it, which
        // is how lightningcss prints `repeat(…)[end]`.
        if c == '[' && depth == 0 {
            if let Some(s) = start.take() {
                out.push(value[s..i].trim());
            }
        }
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            if let Some(s) = start.take() {
                out.push(value[s..i].trim());
            }
        } else if start.is_none() {
            start = Some(i);
        }
        // And a `]` ends one.
        if c == ']' && depth == 0 {
            if let Some(s) = start.take() {
                out.push(value[s..=i].trim());
            }
        }
    }
    if let Some(s) = start {
        out.push(value[s..].trim());
    }
    out.into_iter().filter(|t| !t.is_empty()).collect()
}

/// A track list with its line names: each track, and the names of the lines
/// around them, one set more than tracks, or none when nothing is named.
fn parse_named_tracks(value: &str, allow_repeat: bool) -> (Vec<GridTrack>, Vec<Vec<GridName>>) {
    let mut tracks = Vec::new();
    let mut names: Vec<Vec<GridName>> = vec![Vec::new()];
    let mut any_name = false;
    for tok in split_template(value) {
        if let Some(inner) = tok.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
            any_name = true;
            names.last_mut().expect("a set").extend(inner.split_whitespace().map(GridName::new));
            continue;
        }
        if let Some(args) = tok.strip_prefix("repeat(").and_then(|t| t.strip_suffix(')')) {
            let Some((count, rest)) = args.split_once(',') else {
                warn(format!(
                    "`{tok}` is not a `repeat()` Rux understands; write `repeat(3, 1fr)` or `repeat(auto-fill, minmax(120px, 1fr))`"
                ));
                continue;
            };
            let count = match count.trim() {
                "auto-fill" => Repeat::AutoFill,
                "auto-fit" => Repeat::AutoFit,
                n => match n.parse::<u16>() {
                    Ok(n) if n > 0 => Repeat::Count(n),
                    _ => {
                        warn(format!(
                            "`{tok}` repeats `{n}` times; write a whole number above 0, `auto-fill` or `auto-fit`"
                        ));
                        continue;
                    }
                },
            };
            if !allow_repeat {
                warn(format!("`{tok}` cannot be inside another `repeat()`"));
                continue;
            }
            let (inner, inner_names) = parse_named_tracks(rest, false);
            let inner: Vec<Track> = inner
                .into_iter()
                .filter_map(|t| match t {
                    GridTrack::One(t) => Some(t),
                    GridTrack::Repeat(..) => None,
                })
                .collect();
            tracks.push(GridTrack::Repeat(count, inner, inner_names));
            names.push(Vec::new());
            continue;
        }
        tracks.push(GridTrack::One(parse_track(tok)));
        names.push(Vec::new());
    }
    if !any_name {
        names.clear();
    }
    (tracks, names)
}

/// One track: a length or percentage, `Nfr`, `auto`, `min-content`,
/// `max-content`, `fit-content(<length>)` or `minmax(min, max)`.
fn parse_track(tok: &str) -> Track {
    if let Some(args) = tok.strip_prefix("minmax(").and_then(|s| s.strip_suffix(')')) {
        let mut parts = args.split(',');
        let lo = parts.next().map(parse_track_side).unwrap_or(TrackSide::Auto);
        let hi = parts.next().map(parse_track_side).unwrap_or(TrackSide::Auto);
        return Track::MinMax(lo, hi);
    }
    if let Some(arg) = tok.strip_prefix("fit-content(").and_then(|s| s.strip_suffix(')')) {
        if let Some(px) = parse_px(arg.trim()) {
            return Track::FitContent(px);
        }
    }
    match parse_track_side(tok) {
        TrackSide::Px(v) => Track::Px(v),
        TrackSide::Pct(p) => Track::Pct(p),
        TrackSide::Fr(f) => Track::Fr(f),
        TrackSide::Auto => Track::Auto,
        TrackSide::MinContent => Track::MinContent,
        TrackSide::MaxContent => Track::MaxContent,
    }
}

/// The tracks of `grid-auto-rows` / `-columns`, which take no names or
/// `repeat()`.
fn parse_tracks(value: &str) -> Vec<Track> {
    split_template(value).into_iter().map(parse_track).collect()
}

/// A single track value: `Nfr`, `auto`, `min-content`, `max-content`, a
/// percentage or a length (default `auto`).
fn parse_track_side(tok: &str) -> TrackSide {
    let tok = tok.trim();
    if let Some(fr) = tok.strip_suffix("fr") {
        TrackSide::Fr(fr.trim().parse().unwrap_or(1.0))
    } else if let Some(pct) = tok.strip_suffix('%') {
        pct.trim().parse::<f32>().map(|p| TrackSide::Pct(p / 100.0)).unwrap_or(TrackSide::Auto)
    } else {
        match tok {
            "auto" => TrackSide::Auto,
            "min-content" => TrackSide::MinContent,
            "max-content" => TrackSide::MaxContent,
            _ => parse_px(tok).map(TrackSide::Px).unwrap_or(TrackSide::Auto),
        }
    }
}

/// `grid-template-areas`: one quoted string per row, one name per cell, `.`
/// for a cell no area takes. Each name must make a rectangle, or the whole
/// value is invalid, as in CSS.
fn parse_grid_areas(value: &str) -> Option<GridAreas> {
    let rows: Vec<Vec<&str>> = value
        .split('"')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, row)| row.split_whitespace().collect())
        .collect();
    let columns = rows.first()?.len();
    if columns == 0 || rows.iter().any(|r| r.len() != columns) {
        warn(format!("`grid-template-areas: {value}` needs the same number of cells in every row"));
        return None;
    }
    // Each name's first and last row and column.
    let mut seen: Vec<(&str, usize, usize, usize, usize)> = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            if cell.chars().all(|ch| ch == '.') {
                continue;
            }
            match seen.iter_mut().find(|a| a.0 == *cell) {
                Some(a) => {
                    a.1 = a.1.min(r);
                    a.2 = a.2.max(r);
                    a.3 = a.3.min(c);
                    a.4 = a.4.max(c);
                }
                None => seen.push((cell, r, r, c, c)),
            }
        }
    }
    for &(name, r0, r1, c0, c1) in &seen {
        let filled = (r0..=r1).all(|r| (c0..=c1).all(|c| rows[r][c] == name));
        let count = rows.iter().flatten().filter(|c| **c == name).count();
        if !filled || count != (r1 - r0 + 1) * (c1 - c0 + 1) {
            warn(format!(
                "`grid-template-areas`: the cells named `{name}` do not make a rectangle, so the value is ignored"
            ));
            return None;
        }
    }
    Some(GridAreas {
        areas: seen
            .into_iter()
            .map(|(name, r0, r1, c0, c1)| (GridName::new(name), r0 as u16 + 1, r1 as u16 + 2, c0 as u16 + 1, c1 as u16 + 2))
            .collect(),
        rows: rows.len() as u16,
        columns: columns as u16,
    })
}

/// The grid template properties onto `st`: the `grid-template` shorthand,
/// as `<rows> / <columns>` or with areas (`"a a" 40px "b c" / 1fr 2fr`),
/// then `grid-template-columns`, `-rows` and `-areas` over it.
fn interpret_grid_template(p: &HashMap<String, String>, st: &mut Style) {
    let mut names = GridNames::default();
    if let Some(v) = p.get("grid-template") {
        let v = v.trim();
        if v != "none" {
            let (rows, columns) = v.split_once('/').unwrap_or((v, ""));
            if rows.contains('"') {
                // Each row's string, then its size if one follows it.
                let mut areas = String::new();
                let mut sizes = String::new();
                let mut rest = rows;
                while let Some(open) = rest.find('"') {
                    let after = &rest[open + 1..];
                    let Some(close) = after.find('"') else { break };
                    areas.push_str(&format!("\"{}\" ", &after[..close]));
                    rest = &after[close + 1..];
                    let size = rest[..rest.find('"').unwrap_or(rest.len())].trim();
                    sizes.push_str(if size.is_empty() { "auto" } else { size });
                    sizes.push(' ');
                }
                names.areas = parse_grid_areas(&areas);
                (st.grid_rows, names.rows) = parse_named_tracks(&sizes, true);
            } else {
                (st.grid_rows, names.rows) = parse_named_tracks(rows, true);
            }
            (st.grid_columns, names.columns) = parse_named_tracks(columns, true);
        }
    }
    if let Some(v) = p.get("grid-template-columns") {
        (st.grid_columns, names.columns) = parse_named_tracks(v, true);
    }
    if let Some(v) = p.get("grid-template-rows") {
        (st.grid_rows, names.rows) = parse_named_tracks(v, true);
    }
    if let Some(v) = p.get("grid-template-areas") {
        names.areas = if v.trim() == "none" { None } else { parse_grid_areas(v) };
    }
    if names != GridNames::default() {
        st.grid_names = Some(Box::new(names));
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

/// The percentage half of `border-radius`, in the same diagonal grouping.
///
/// Returned separately from the pixels because the two resolve at different
/// times: pixels are final here, a percentage needs a box and is worked out at
/// paint. A corner written in px comes back `None` and keeps whatever
/// [`parse_border_radius`] gave it.
fn parse_border_radius_pct(value: &str) -> [Option<f32>; 4] {
    let horizontal = value.split('/').next().unwrap_or(value);
    let v: Vec<Option<f32>> = horizontal
        .split_whitespace()
        .map(|t| t.strip_suffix('%').and_then(|n| n.trim().parse::<f32>().ok()))
        .collect();
    match v.len() {
        1 => [v[0]; 4],
        2 => [v[0], v[1], v[0], v[1]],
        3 => [v[0], v[1], v[2], v[1]],
        n if n >= 4 => [v[0], v[1], v[2], v[3]],
        _ => [None; 4],
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
/// -left` longhand overrides: the px sides, and the sides written as a
/// percentage (top, right, bottom, left), which the layout resolves against
/// the containing block's width (watchlist #6). A side is one or the other.
///
/// A percentage keeps its place in the shorthand. It used to be dropped, which
/// moved the values after it: `padding: 10px 5%` came out as 10px all round.
fn box_sides(p: &HashMap<String, String>, prop: &str) -> (Sides, [Option<f32>; 4]) {
    let mut sides = Sides::default();
    let mut pct = [None; 4];
    if let Some(v) = p.get(prop) {
        let v: Vec<(f32, Option<f32>)> = v.split_whitespace().filter_map(side_length).collect();
        let at = |i: usize| v[i];
        let four = match v.len() {
            1 => [at(0); 4],
            2 => [at(0), at(1), at(0), at(1)],
            3 => [at(0), at(1), at(2), at(1)],
            n if n >= 4 => [at(0), at(1), at(2), at(3)],
            _ => [(0.0, None); 4],
        };
        sides = Sides { top: four[0].0, right: four[1].0, bottom: four[2].0, left: four[3].0 };
        pct = [four[0].1, four[1].1, four[2].1, four[3].1];
    }
    for (i, side) in ["top", "right", "bottom", "left"].into_iter().enumerate() {
        if let Some((px, p)) = p.get(&format!("{prop}-{side}")).and_then(|v| side_length(first(v))) {
            set_side(&mut sides, side, px);
            pct[i] = p;
        }
    }
    (sides, pct)
}

/// One side of a box shorthand: px, or a percentage (0 px, then the
/// percentage). `None` for anything else, which the shorthand skips.
fn side_length(token: &str) -> Option<(f32, Option<f32>)> {
    match token.strip_suffix('%') {
        Some(n) => n.trim().parse::<f32>().ok().map(|p| (0.0, Some(p))),
        None => parse_px(token).map(|px| (px, None)),
    }
}

/// `gap`, `row-gap` or `column-gap` as px, or as a percentage (0 px, then
/// the percentage).
fn gap_length(value: &str) -> Option<(f32, Option<f32>)> {
    side_length(first(value))
}

/// Parse the `<path>` paint properties.
///
/// `fill` defaults to opaque black and `stroke` to nothing, which is SVG's
/// pair of defaults and is why a `<path>` with geometry and no paint still
/// draws. `none` is spelled the way SVG spells it and is the only way to turn
/// a fill off; an unreadable colour leaves the property alone rather than
/// blanking it, so a typo shows the previous value rather than an empty box.
/// `overflow`, `overflow-x` and `overflow-y`, each axis on its own. The
/// shorthand takes one value for both or two (`x y`), and a longhand beside it
/// wins (the map has lost which came later, and the longhand is the one written
/// to narrow the other). `auto`/`scroll` scroll and clip, `hidden`/`clip` only
/// clip.
///
/// As CSS computes it, `visible` beside an axis that is `hidden`, `auto` or
/// `scroll` is `auto`: a box cannot both show what overflows on one axis and
/// clip on the other, so `overflow-y: auto` alone scrolls sideways too when
/// its content is wider. Beside `clip` it stays `visible`, but a clip in Rux is
/// the whole box, so it clips that axis as well: written down in
/// `docs/05-as-built.md`.
fn overflow_axes(p: &HashMap<String, String>, st: &mut Style) {
    let short: Vec<&str> = p.get("overflow").map(|v| v.split_whitespace().collect()).unwrap_or_default();
    let x = p.get("overflow-x").map(|v| v.trim()).or(short.first().copied());
    let y = p.get("overflow-y").map(|v| v.trim()).or(short.get(1).or(short.first()).copied());
    if x.is_none() && y.is_none() {
        return;
    }
    let (x, y) = (x.unwrap_or("visible"), y.unwrap_or("visible"));
    let lifts = |other: &str| matches!(other, "hidden" | "auto" | "scroll");
    let kind = |v: &str, other: &str| match v {
        "auto" | "scroll" => Overflow::Scroll,
        "hidden" | "clip" => Overflow::Clip,
        "visible" if lifts(other) => Overflow::Scroll,
        _ => Overflow::Visible,
    };
    let (kx, ky) = (kind(x, y), kind(y, x));
    st.overflow = if kx == Overflow::Scroll || ky == Overflow::Scroll {
        Overflow::Scroll
    } else if kx == Overflow::Clip || ky == Overflow::Clip {
        Overflow::Clip
    } else {
        Overflow::Visible
    };
    st.overflow_x = (kx != st.overflow).then_some(kx);
    st.overflow_y = (ky != st.overflow).then_some(ky);
}

/// `overscroll-behavior` (one value for both axes, or `x y`) and its `-x` and
/// `-y` longhands, `scrollbar-width` and `scrollbar-color`.
///
/// `contain` and `none` are the same here: they differ in whether the box
/// shows its own end-of-scroll effect, and Rux draws none yet.
fn scroll_style(p: &HashMap<String, String>, st: &mut Style) {
    let short: Vec<&str> =
        p.get("overscroll-behavior").map(|v| v.split_whitespace().collect()).unwrap_or_default();
    let x = p.get("overscroll-behavior-x").map(|v| v.trim()).or(short.first().copied());
    let y = p.get("overscroll-behavior-y").map(|v| v.trim()).or(short.get(1).or(short.first()).copied());
    let keeps = |v: Option<&str>| matches!(v, Some("contain" | "none"));
    st.overscroll_contain = (keeps(x), keeps(y));
    if let Some(v) = p.get("scrollbar-width") {
        st.scrollbar_width = match v.trim() {
            "thin" => ScrollbarWidth::Thin,
            "none" => ScrollbarWidth::None,
            _ => ScrollbarWidth::Auto,
        };
    }
    // Two colours, thumb then track; `auto` is the shell's own. A colour may
    // have spaces inside it (`rgb(0 0 0)`), so split outside parentheses.
    if let Some(v) = p.get("scrollbar-color") {
        let parts = split_top_level(v.trim());
        if let [thumb, track] = parts.as_slice() {
            if let (Some(a), Some(b)) = (parse_color(thumb), parse_color(track)) {
                st.scrollbar_color = Some(Box::new((a, b)));
            }
        }
    }
}

fn interpret_path_paint(p: &HashMap<String, String>, st: &mut Style) {
    if let Some(v) = p.get("fill") {
        st.fill = if v.trim().eq_ignore_ascii_case("none") {
            None
        } else {
            parse_color(v).or(st.fill)
        };
    }
    if let Some(v) = p.get("stroke") {
        st.stroke = if v.trim().eq_ignore_ascii_case("none") {
            None
        } else {
            parse_color(v).or(st.stroke)
        };
    }
    if let Some(v) = p.get("stroke-width") {
        if let Some(px) = parse_px(first(v)) {
            st.stroke_width = px;
        }
    }
    if let Some(v) = p.get("stroke-linecap") {
        st.stroke_linecap = match v.trim() {
            "round" => LineCap::Round,
            "square" => LineCap::Square,
            _ => LineCap::Butt,
        };
    }
    if let Some(v) = p.get("stroke-linejoin") {
        st.stroke_linejoin = match v.trim() {
            "round" => LineJoin::Round,
            "bevel" => LineJoin::Bevel,
            _ => LineJoin::Miter,
        };
    }
    if let Some(v) = p.get("fill-rule") {
        st.fill_rule = match v.trim() {
            "evenodd" | "even-odd" => FillRule::EvenOdd,
            _ => FillRule::NonZero,
        };
    }
}

/// `mask-image` and `mask-mode`, or the `mask` shorthand's image and mode,
/// the longhands winning. An image is a gradient or a `url()`; `none`, or a
/// colour, which is not an image, is no mask.
fn interpret_mask(p: &HashMap<String, String>) -> Option<rux_layout::Mask> {
    let image = |v: &str| match parse_background(v) {
        Some(Background::Color(_)) | None => None,
        some => some,
    };
    let mode = |v: &str| match v {
        "luminance" => Some(true),
        "alpha" | "match-source" => Some(false),
        _ => None,
    };
    let (mut picture, mut luminance) = (None, false);
    if let Some(v) = p.get("mask") {
        for token in split_top_level(v) {
            if let Some(m) = mode(token) {
                luminance = m;
            } else if let Some(i) = image(token) {
                picture = Some(i);
            }
        }
    }
    if let Some(v) = p.get("mask-image") {
        picture = image(v.trim());
    }
    if let Some(m) = p.get("mask-mode").and_then(|v| mode(v.trim())) {
        luminance = m;
    }
    Some(rux_layout::Mask { image: picture?, luminance })
}

/// `filter` or `backdrop-filter`: the functions in the order written, each
/// run of colour functions folded into one matrix. `none` is no filter, and
/// so is a list with a function Rux does not draw, which is said once: CSS
/// drops the whole declaration when one function is invalid.
fn parse_filter(property: &str, value: &str) -> Vec<rux_layout::filter::FilterOp> {
    use rux_layout::filter::{fold, FilterOp};
    let value = value.trim();
    if value == "none" {
        return Vec::new();
    }
    // A number or a percentage, `default` when left out.
    let amount = |args: &str, default: f32| -> Option<f32> {
        let a = args.trim();
        if a.is_empty() {
            return Some(default);
        }
        match a.strip_suffix('%') {
            Some(p) => p.trim().parse::<f32>().ok().map(|v| v / 100.0),
            None => a.parse::<f32>().ok(),
        }
        .filter(|v| *v >= 0.0)
    };
    let mut ops = Vec::new();
    for token in split_top_level(value) {
        let parsed = token.split_once('(').and_then(|(name, rest)| {
            let args = rest.strip_suffix(')')?;
            Some(match name.trim() {
                "blur" => FilterOp::Blur(if args.trim().is_empty() { 0.0 } else { parse_px(args.trim())?.max(0.0) }),
                "brightness" => FilterOp::Matrix(FilterOp::brightness(amount(args, 1.0)?)),
                "contrast" => FilterOp::Matrix(FilterOp::contrast(amount(args, 1.0)?)),
                "grayscale" => FilterOp::Matrix(FilterOp::grayscale(amount(args, 1.0)?)),
                "sepia" => FilterOp::Matrix(FilterOp::sepia(amount(args, 1.0)?)),
                "saturate" => FilterOp::Matrix(FilterOp::saturate(amount(args, 1.0)?)),
                "invert" => FilterOp::Matrix(FilterOp::invert(amount(args, 1.0)?)),
                "opacity" => FilterOp::Matrix(FilterOp::opacity(amount(args, 1.0)?)),
                "hue-rotate" => {
                    let a = args.trim();
                    let deg = if a.is_empty() { 0.0 } else { parse_angle(a)?.to_degrees() };
                    FilterOp::Matrix(FilterOp::hue_rotate(deg))
                }
                "drop-shadow" => {
                    let mut lengths = Vec::new();
                    let mut color = None;
                    for t in split_top_level(args) {
                        match parse_px(t) {
                            Some(px) => lengths.push(px),
                            None => color = Some(parse_color(t)?),
                        }
                    }
                    let [dx, dy, ref rest @ ..] = lengths[..] else { return None };
                    FilterOp::DropShadow {
                        dx,
                        dy,
                        blur: rest.first().copied().unwrap_or(0.0).max(0.0),
                        color: color.unwrap_or(Rgba::new(0.0, 0.0, 0.0, 1.0)),
                    }
                }
                _ => return None,
            })
        });
        match parsed {
            Some(op) => ops.push(op),
            None => {
                let why = if token.starts_with("url(") {
                    "an SVG `<filter>` is not something Rux has; the functions are blur(), brightness(), \
                     contrast(), drop-shadow(), grayscale(), hue-rotate(), invert(), opacity(), \
                     saturate() and sepia()"
                } else {
                    "it is not one of blur(), brightness(), contrast(), drop-shadow(), grayscale(), \
                     hue-rotate(), invert(), opacity(), saturate() or sepia(), written right"
                };
                warn_once(format!("`{property}: {value}` does nothing: `{token}` is the reason: {why}."));
                return Vec::new();
            }
        }
    }
    fold(ops)
}

/// A `filter` with its `drop-shadow()`s given `color` where they have no
/// colour of their own, which CSS draws in `currentColor`. `None` when
/// nothing changed.
fn drop_shadow_colors(value: &str, color: Rgba) -> Option<String> {
    if !value.contains("drop-shadow(") {
        return None;
    }
    let mut changed = false;
    let words: Vec<String> = split_top_level(value)
        .into_iter()
        .map(|token| match token.strip_prefix("drop-shadow(").and_then(|r| r.strip_suffix(')')) {
            Some(args) if !split_top_level(args).into_iter().any(|t| parse_color(t).is_some()) => {
                changed = true;
                format!("drop-shadow({args} {})", color::hex(color))
            }
            _ => token.to_string(),
        })
        .collect();
    changed.then(|| words.join(" "))
}

/// An element's rarely set text properties (see [`TextLook`]): the
/// inheriting ones its own or its parent's, the rest its own. Inside a
/// sentence a run takes the sentence's decoration too, as it takes its
/// underline (see [`RunInherit`]). `None` when nothing is set.
/// `line-clamp: 2`, or the way every engine has it today, Tailwind's
/// `line-clamp-2`: `-webkit-line-clamp: 2` beside `display: -webkit-box`.
/// The prefixed one does nothing in a browser without that display, so it
/// does nothing here either, and says so.
fn line_clamp(props: &HashMap<String, String>) -> Option<u32> {
    let count = |v: &str| match v.trim() {
        "none" => None,
        n => match n.parse::<u32>() {
            Ok(n) if n > 0 => Some(n),
            _ => {
                warn(format!("`{n}` is not a number of lines to clamp to"));
                None
            }
        },
    };
    if let Some(v) = props.get("line-clamp") {
        return count(v);
    }
    let v = props.get("-webkit-line-clamp")?;
    let boxed = props.get("display").is_some_and(|d| matches!(d.trim(), "-webkit-box" | "-webkit-inline-box"));
    if !boxed {
        warn("`-webkit-line-clamp` clamps only a box with `display: -webkit-box`, as in a browser".to_string());
        return None;
    }
    count(v)
}

fn text_look(props: &HashMap<String, String>, parent: Option<&Arc<TextLook>>, in_sentence: bool) -> Option<Arc<TextLook>> {
    let up = parent.map(|p| (**p).clone()).unwrap_or_default();
    let mut look = TextLook {
        transform: up.transform,
        indent: up.indent,
        indent_pct: up.indent_pct,
        indent_each_line: up.indent_each_line,
        indent_hanging: up.indent_hanging,
        variant: up.variant.clone(),
        feature_settings: up.feature_settings.clone(),
        variation_settings: up.variation_settings.clone(),
        stretch: up.stretch,
        caret_color: up.caret_color,
        ..Default::default()
    };
    if in_sentence {
        look.decoration_color = up.decoration_color;
        look.decoration_style = up.decoration_style;
        look.decoration_thickness = up.decoration_thickness;
        look.underline_offset = up.underline_offset;
        look.overline = up.overline;
    }
    // `text-decoration`: its lines, style, colour and thickness in any
    // order; the longhands after it.
    if let Some(v) = props.get("text-decoration") {
        for tok in split_top_level(v) {
            if let Some(s) = decoration_style(tok) {
                look.decoration_style = s;
            } else if tok == "overline" {
                look.overline = true;
            } else if matches!(tok, "underline" | "line-through" | "none" | "auto" | "from-font") {
            } else if let Some(px) = parse_px(tok) {
                look.decoration_thickness = Some(px);
            } else if let Some(c) = parse_color(tok) {
                look.decoration_color = Some(c);
            } else {
                warn(format!("`{tok}` in `text-decoration` is not something Rux knows"));
            }
        }
    }
    if let Some(v) = props.get("text-decoration-line") {
        look.overline = v.split_whitespace().any(|t| t == "overline");
    }
    if let Some(v) = props.get("text-decoration-style") {
        match decoration_style(v.trim()) {
            Some(s) => look.decoration_style = s,
            None => warn(format!("`text-decoration-style: {v}` is not a style Rux knows")),
        }
    }
    if let Some(v) = props.get("text-decoration-color") {
        look.decoration_color = parse_color(v.trim());
    }
    if let Some(v) = props.get("text-decoration-thickness") {
        look.decoration_thickness = match v.trim() {
            "auto" | "from-font" => None,
            t => parse_px(t),
        };
    }
    if let Some(v) = props.get("text-underline-offset") {
        look.underline_offset = match v.trim() {
            "auto" => None,
            t => parse_px(t),
        };
    }
    if let Some(v) = props.get("text-transform") {
        look.transform = match v.trim() {
            "none" => TextTransform::None,
            "uppercase" => TextTransform::Uppercase,
            "lowercase" => TextTransform::Lowercase,
            "capitalize" => TextTransform::Capitalize,
            other => {
                warn(format!(
                    "`text-transform: {other}` is not drawn; Rux has `uppercase`, `lowercase` and `capitalize`"
                ));
                TextTransform::None
            }
        };
    }
    if let Some(v) = props.get("text-indent") {
        // A length and, in any order, `hanging` and `each-line`.
        let toks: Vec<&str> = v.split_whitespace().collect();
        look.indent_hanging = toks.contains(&"hanging");
        look.indent_each_line = toks.contains(&"each-line");
        let len = toks.iter().find(|t| !matches!(**t, "hanging" | "each-line")).copied().unwrap_or("0");
        match len.strip_suffix('%').and_then(|n| n.parse::<f32>().ok()) {
            Some(pct) => (look.indent, look.indent_pct) = (0.0, Some(pct)),
            None => (look.indent, look.indent_pct) = (parse_px(len).unwrap_or(0.0), None),
        }
    }
    if let Some(v) = props.get("text-overflow") {
        look.ellipsis = match v.trim() {
            "ellipsis" => true,
            "clip" => false,
            other => {
                warn(format!("`text-overflow: {other}` is not drawn; Rux has `clip` and `ellipsis`"));
                false
            }
        };
        // As in CSS, only a box that clips cuts its text short: one that
        // lets it spill shows all of it.
        let clips = ["overflow-x", "overflow"]
            .iter()
            .find_map(|k| props.get(*k))
            .is_some_and(|v| first(v) != "visible");
        if look.ellipsis && !clips {
            warn("`text-overflow: ellipsis` cuts only a box that clips; add `overflow: hidden`".to_string());
            look.ellipsis = false;
        }
    }
    let variant_props = [
        "font-variant",
        "font-variant-caps",
        "font-variant-numeric",
        "font-variant-ligatures",
        "font-variant-position",
        "font-variant-east-asian",
    ];
    if variant_props.iter().any(|k| props.contains_key(*k)) {
        let mut features: Vec<&str> = Vec::new();
        for k in variant_props {
            if let Some(v) = props.get(k) {
                for tok in v.split_whitespace() {
                    match variant_features(tok) {
                        Some(f) => features.extend(f),
                        None => warn(format!("`{tok}` in `{k}` is not a variant Rux knows")),
                    }
                }
            }
        }
        look.variant = (!features.is_empty()).then(|| features.join(", "));
    }
    if let Some(v) = props.get("font-feature-settings") {
        look.feature_settings = (v.trim() != "normal").then(|| v.trim().to_string());
    }
    if let Some(v) = props.get("font-variation-settings") {
        look.variation_settings = (v.trim() != "normal").then(|| v.trim().to_string());
    }
    if let Some(v) = props.get("font-stretch").or_else(|| props.get("font-width")) {
        look.stretch = match font_stretch(v.trim()) {
            Some(pct) if pct == 100.0 => None,
            Some(pct) => Some(pct),
            None => {
                warn(format!("`font-stretch: {v}` is not a width Rux knows"));
                None
            }
        };
    }
    if let Some(v) = props.get("caret-color") {
        look.caret_color = match v.trim() {
            "auto" => None,
            c => parse_color(c),
        };
    }
    look.line_clamp = line_clamp(props);
    (look != TextLook::default()).then(|| Arc::new(look))
}

fn decoration_style(v: &str) -> Option<DecorationStyle> {
    Some(match v {
        "solid" => DecorationStyle::Solid,
        "double" => DecorationStyle::Double,
        "dotted" => DecorationStyle::Dotted,
        "dashed" => DecorationStyle::Dashed,
        "wavy" => DecorationStyle::Wavy,
        _ => return None,
    })
}

/// The OpenType features a `font-variant` keyword turns on or off, as CSS
/// Fonts 4 lists them. `normal` asks for none. A font without the feature
/// draws the text as it is: Rux does not make small capitals up, as a
/// browser does.
fn variant_features(v: &str) -> Option<&'static [&'static str]> {
    Some(match v {
        "normal" => &[],
        "none" => &["\"liga\" 0", "\"clig\" 0", "\"dlig\" 0", "\"hlig\" 0", "\"calt\" 0"],
        "small-caps" => &["\"smcp\""],
        "all-small-caps" => &["\"smcp\"", "\"c2sc\""],
        "petite-caps" => &["\"pcap\""],
        "all-petite-caps" => &["\"pcap\"", "\"c2pc\""],
        "unicase" => &["\"unic\""],
        "titling-caps" => &["\"titl\""],
        "lining-nums" => &["\"lnum\""],
        "oldstyle-nums" => &["\"onum\""],
        "proportional-nums" => &["\"pnum\""],
        "tabular-nums" => &["\"tnum\""],
        "diagonal-fractions" => &["\"frac\""],
        "stacked-fractions" => &["\"afrc\""],
        "ordinal" => &["\"ordn\""],
        "slashed-zero" => &["\"zero\""],
        "common-ligatures" => &["\"liga\"", "\"clig\""],
        "no-common-ligatures" => &["\"liga\" 0", "\"clig\" 0"],
        "discretionary-ligatures" => &["\"dlig\""],
        "no-discretionary-ligatures" => &["\"dlig\" 0"],
        "historical-ligatures" => &["\"hlig\""],
        "no-historical-ligatures" => &["\"hlig\" 0"],
        "contextual" => &["\"calt\""],
        "no-contextual" => &["\"calt\" 0"],
        "sub" => &["\"subs\""],
        "super" => &["\"sups\""],
        "jis78" => &["\"jp78\""],
        "jis83" => &["\"jp83\""],
        "jis90" => &["\"jp90\""],
        "jis04" => &["\"jp04\""],
        "simplified" => &["\"smpl\""],
        "traditional" => &["\"trad\""],
        "full-width" => &["\"fwid\""],
        "proportional-width" => &["\"pwid\""],
        "ruby" => &["\"ruby\""],
        _ => return None,
    })
}

/// `font-stretch` as a percentage: the keyword's, or the one written.
fn font_stretch(v: &str) -> Option<f32> {
    Some(match v {
        "ultra-condensed" => 50.0,
        "extra-condensed" => 62.5,
        "condensed" => 75.0,
        "semi-condensed" => 87.5,
        "normal" => 100.0,
        "semi-expanded" => 112.5,
        "expanded" => 125.0,
        "extra-expanded" => 150.0,
        "ultra-expanded" => 200.0,
        _ => v.strip_suffix('%')?.trim().parse().ok()?,
    })
}

/// `text-shadow`: a comma list of `<dx> <dy> <blur>? <color>?`. `none` is
/// none, and so is a list none of whose entries parse. A shadow with no
/// colour is `currentColor`, the colour of the text it is under, which is
/// only known where it is drawn.
fn parse_text_shadows(value: &str) -> Option<Arc<Vec<rux_layout::TextShadow>>> {
    if value.trim() == "none" {
        return None;
    }
    let list: Vec<rux_layout::TextShadow> = split_top_level_commas(value)
        .into_iter()
        .filter_map(|one| {
            let mut lengths = Vec::new();
            let mut color = None;
            for t in split_top_level(one) {
                match parse_px(t) {
                    Some(px) => lengths.push(px),
                    None => color = Some(parse_color(t)?),
                }
            }
            match lengths[..] {
                [dx, dy] => Some(rux_layout::TextShadow { dx, dy, blur: 0.0, color }),
                [dx, dy, blur] => Some(rux_layout::TextShadow { dx, dy, blur: blur.max(0.0), color }),
                _ => None,
            }
        })
        .collect();
    (!list.is_empty()).then(|| Arc::new(list))
}

/// `clip-path`: a basic shape and the box it is of, either alone, or `none`.
/// A shape that does not parse is said once, and clips nothing.
fn parse_clip_path(value: &str) -> Option<rux_layout::clip::ClipPath> {
    use rux_layout::clip::{ClipPath, RefBox};
    let value = value.trim();
    if value == "none" {
        return None;
    }
    let mut shape = None;
    let mut reference = None;
    for token in split_top_level(value) {
        let boxed = match token {
            "border-box" => Some(RefBox::Border),
            "padding-box" => Some(RefBox::Padding),
            "content-box" => Some(RefBox::Content),
            "margin-box" => Some(RefBox::Margin),
            // SVG's boxes, for a box that is not SVG, are the CSS ones.
            "fill-box" => Some(RefBox::Content),
            "stroke-box" | "view-box" => Some(RefBox::Border),
            _ => None,
        };
        if boxed.is_some() {
            reference = boxed;
            continue;
        }
        match parse_clip_shape(token) {
            Some(s) => shape = Some(s),
            None => {
                let why = if token.starts_with("url(") {
                    "Rux has no SVG `<clipPath>` to point at; use a shape such as \
                     `circle()`, `polygon()` or `path()`"
                } else {
                    "it is not a shape Rux reads: `inset()`, `circle()`, `ellipse()`, `polygon()`, \
                     `path()`, `xywh()` or `rect()`, and a box"
                };
                warn_once(format!("`clip-path: {value}` clips nothing: {why}."));
                return None;
            }
        }
    }
    Some(ClipPath { shape, reference: reference.unwrap_or_default() })
}

/// One basic shape: `inset()`, `xywh()`, `rect()`, `circle()`, `ellipse()`,
/// `polygon()` or `path()`.
fn parse_clip_shape(token: &str) -> Option<rux_layout::clip::ClipShape> {
    use rux_layout::clip::{ClipShape, Radius};
    let (name, rest) = token.split_once('(')?;
    let name = name.trim();
    let args = rest.strip_suffix(')')?.trim();
    let half = Len::Pct(0.5);
    // A `<radius>`: a keyword, or a length.
    let radius = |t: Option<&str>| -> Option<Radius> {
        Some(match t {
            None | Some("closest-side") => Radius::ClosestSide,
            Some("farthest-side") => Radius::FarthestSide,
            Some(t) => Radius::Len(parse_len(t)?),
        })
    };
    // One word of a position, as an x, a y, or neither (`center`).
    let word = |w: &str| -> Option<(Option<Len>, Option<Len>)> {
        Some(match w {
            "left" => (Some(Len::Pct(0.0)), None),
            "right" => (Some(Len::Pct(1.0)), None),
            "top" => (None, Some(Len::Pct(0.0))),
            "bottom" => (None, Some(Len::Pct(1.0))),
            "center" => (None, None),
            l => (Some(parse_len(l)?), None),
        })
    };
    // `at <position>`, centred unless said otherwise.
    let at = |words: &[&str]| -> Option<(Len, Len)> {
        Some(match words {
            [] => (half, half),
            [one] => {
                let (x, y) = word(one)?;
                (x.unwrap_or(half), y.unwrap_or(half))
            }
            [a, b] => {
                // `top left` is `left top`: a vertical word first is swapped.
                let swap = matches!(*a, "top" | "bottom") || matches!(*b, "left" | "right");
                let (a, b) = if swap { (*b, *a) } else { (*a, *b) };
                let x = word(a)?.0.unwrap_or(half);
                let y = match b {
                    "top" => Len::Pct(0.0),
                    "bottom" => Len::Pct(1.0),
                    "center" => half,
                    l => parse_len(l)?,
                };
                (x, y)
            }
            _ => return None,
        })
    };
    // The words before `at`, and the ones after it.
    let split_at = |args: &str| -> (Vec<String>, Vec<String>) {
        let words: Vec<String> = split_top_level(args).into_iter().map(str::to_string).collect();
        match words.iter().position(|w| w == "at") {
            Some(i) => (words[..i].to_vec(), words[i + 1..].to_vec()),
            None => (words, Vec::new()),
        }
    };
    // Up to four lengths after `round`, as `border-radius` takes them.
    let corners = |words: &[&str]| -> Option<[Len; 4]> {
        let l: Vec<Len> = words.iter().take_while(|w| **w != "/").map(|w| parse_len(w)).collect::<Option<_>>()?;
        Some(match l[..] {
            [] => [Len::Px(0.0); 4],
            [a] => [a; 4],
            [a, b] => [a, b, a, b],
            [a, b, c] => [a, b, c, b],
            [a, b, c, d] => [a, b, c, d],
            _ => return None,
        })
    };
    // `calc(100% - a - b)`: a far edge given by where the near ones are.
    let from_far = |parts: &[&str]| -> Option<Len> {
        let text = format!("calc(100% - {})", parts.join(" - "));
        rux_layout::calc::intern(&text).map(Len::Calc)
    };
    // An optional fill rule ahead of the rest, as `polygon()` and `path()` take one.
    let rule = |args: &str| -> (bool, String) {
        let parts = split_top_level_commas(args);
        match parts.first().copied() {
            Some("evenodd") => (true, parts[1..].join(",")),
            Some("nonzero") => (false, parts[1..].join(",")),
            _ => (false, args.to_string()),
        }
    };
    Some(match name {
        "inset" | "xywh" | "rect" => {
            let words = split_top_level(args);
            let round = words.iter().position(|w| *w == "round");
            let v = &words[..round.unwrap_or(words.len())];
            let sides = match (name, v) {
                ("inset", _) => {
                    let l: Vec<Len> = v.iter().map(|w| parse_len(w)).collect::<Option<_>>()?;
                    match l[..] {
                        [a] => [a; 4],
                        [a, b] => [a, b, a, b],
                        [a, b, c] => [a, b, c, b],
                        [a, b, c, d] => [a, b, c, d],
                        _ => return None,
                    }
                }
                // x y width height: the far edges are what is left over.
                ("xywh", [x, y, w, h]) => [parse_len(y)?, from_far(&[x, w])?, from_far(&[y, h])?, parse_len(x)?],
                // top right bottom left, each from the top or left edge.
                ("rect", [t, r, b, l]) => [parse_len(t)?, from_far(&[r])?, from_far(&[b])?, parse_len(l)?],
                _ => return None,
            };
            let round = corners(round.map_or(&[][..], |i| &words[i + 1..]))?;
            ClipShape::Inset { sides, round }
        }
        "circle" => {
            let (r, pos) = split_at(args);
            let pos: Vec<&str> = pos.iter().map(String::as_str).collect();
            ClipShape::Circle { r: radius(r.first().map(String::as_str))?, at: at(&pos)? }
        }
        "ellipse" => {
            let (r, pos) = split_at(args);
            let pos: Vec<&str> = pos.iter().map(String::as_str).collect();
            ClipShape::Ellipse {
                rx: radius(r.first().map(String::as_str))?,
                ry: radius(r.get(1).map(String::as_str))?,
                at: at(&pos)?,
            }
        }
        "polygon" => {
            let (evenodd, rest) = rule(args);
            let points = split_top_level_commas(&rest)
                .into_iter()
                .map(|pair| match split_top_level(pair)[..] {
                    [x, y] => Some((parse_len(x)?, parse_len(y)?)),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?;
            (points.len() >= 3).then_some(())?;
            ClipShape::Polygon { evenodd, points }
        }
        "path" => {
            let (evenodd, rest) = rule(args);
            let d = rest.trim().trim_matches(|c| c == '"' || c == '\'');
            let commands = rux_layout::path::parse_path(d);
            (!commands.is_empty()).then_some(())?;
            ClipShape::Path { evenodd, commands }
        }
        _ => return None,
    })
}

/// `outline` and its longhands, as CSS reads them: the shorthand resets all
/// three of its parts, the longhands then override one each, and
/// `outline-offset` is on its own.
///
/// `None` is no outline: the initial `outline-style: none`, `hidden`, or a
/// width of zero, which is how `outline: 0` removes one. The colour is left
/// `None` for the caller to fill with the element's `color`, CSS's
/// `currentColor`, which is not known yet here.
///
/// An `auto` style is the platform's ring and is 2px unless a width is
/// written. Every other style is a border's: `dashed`, `dotted`, `double`
/// and the 3D ones are drawn as a border with that style would be.
fn interpret_outline(p: &HashMap<String, String>) -> Option<Outline> {
    let mut width: Option<f32> = None;
    let mut style: Option<String> = None;
    let mut color: Option<Rgba> = None;
    if let Some(v) = p.get("outline") {
        for token in v.split_whitespace() {
            if let Some(w) = outline_width(token) {
                width = Some(w);
            } else if OUTLINE_STYLES.contains(&token) {
                style = Some(token.to_string());
            } else if let Some(c) = parse_color(token) {
                color = Some(c);
            }
        }
    }
    if let Some(w) = p.get("outline-width").and_then(|v| outline_width(first(v))) {
        width = Some(w);
    }
    if let Some(v) = p.get("outline-style") {
        style = Some(first(v).to_string());
    }
    if let Some(c) = p.get("outline-color").and_then(|v| parse_color(v)) {
        color = Some(c);
    }
    let style = style?;
    let auto = style == "auto";
    match style.as_str() {
        "none" | "hidden" => return None,
        other if OUTLINE_STYLES.contains(&other) => {}
        other => {
            warn_once(format!(
                "`outline-style: {other}` is not an outline style, so the outline is not \
                 drawn. Use solid, dashed, dotted, double, groove, ridge, inset, outset, \
                 auto or none."
            ));
            return None;
        }
    }
    // CSS's initial width is `medium`, 3px. The ring is the platform's own.
    let width = width.unwrap_or(if auto { 2.0 } else { 3.0 });
    if width <= 0.0 {
        return None;
    }
    let offset = p.get("outline-offset").and_then(|v| parse_px(first(v))).unwrap_or(0.0);
    // `auto` is the platform's ring, drawn solid.
    let style = BorderStyle::parse(&style).unwrap_or(BorderStyle::Solid);
    Some(Outline { width, color, offset, auto, style })
}

/// Every `outline-style` keyword CSS has. What Rux draws of each is decided
/// in [`interpret_outline`].
const OUTLINE_STYLES: &[&str] = &[
    "none", "hidden", "auto", "solid", "dotted", "dashed", "double", "groove", "ridge", "inset",
    "outset",
];

/// An `outline-width`: a length, or one of the three keywords with the sizes
/// browsers give them.
fn outline_width(token: &str) -> Option<f32> {
    match token {
        "thin" => Some(1.0),
        "medium" => Some(3.0),
        "thick" => Some(5.0),
        _ => parse_px(token),
    }
}

/// `border` and its longhands, as CSS reads them: per side, a width
/// (`medium`, 3px, until one is written), a style (`none` until one is
/// written) and the one colour Rux keeps for all four (`currentColor` until
/// one is written). The shorthands reset all three of what they cover and
/// the longhands then override one each.
///
/// **A side whose style is `none` or `hidden` has no width**, as in CSS:
/// `border: 2px red` draws nothing and takes no room. Rux drew it solid
/// until v0.9, so a border given a width or a colour and no style anywhere
/// is said once, with the word that was missing.
fn interpret_border(p: &HashMap<String, String>, st: &mut Style) {
    const SIDES: [&str; 4] = ["top", "right", "bottom", "left"];
    let mut widths = [3.0f32; 4];
    let mut styles = [BorderStyle::None; 4];
    // One `border` value: whichever of a width, a style and a colour it has.
    let shorthand = |v: &str, sides: &[usize], widths: &mut [f32; 4], styles: &mut [BorderStyle; 4], st: &mut Style| {
        let (mut width, mut style, mut color) = (3.0, BorderStyle::None, None);
        for token in split_top_level(v) {
            if let Some(s) = BorderStyle::parse(token) {
                style = s;
            } else if let Some(w) = outline_width(token) {
                width = w;
            } else if let Some(c) = parse_color(token) {
                color = Some(c);
            }
        }
        for &i in sides {
            widths[i] = width;
            styles[i] = style;
        }
        // One side's shorthand colours that side; `border` all of them.
        match sides {
            [i] => st.rare_mut().border_colors[*i] = color,
            _ => {
                st.border_color = color;
                if let Some(rare) = st.rare.as_mut() {
                    rare.border_colors = [None; 4];
                }
            }
        }
    };
    if let Some(v) = p.get("border") {
        shorthand(v, &[0, 1, 2, 3], &mut widths, &mut styles, st);
    }
    for (i, side) in SIDES.iter().enumerate() {
        if let Some(v) = p.get(&format!("border-{side}")) {
            shorthand(v, &[i], &mut widths, &mut styles, st);
        }
    }
    if let Some(v) = p.get("border-width") {
        let w: Vec<f32> = split_top_level(v).into_iter().filter_map(outline_width).collect();
        if let Some(w) = four_sides(&w) {
            widths = w;
        }
    }
    if let Some(v) = p.get("border-style") {
        let s: Vec<BorderStyle> = split_top_level(v).into_iter().filter_map(BorderStyle::parse).collect();
        if let Some(s) = four_sides(&s) {
            styles = s;
        }
    }
    if let Some(v) = p.get("border-color") {
        let colors: Vec<Rgba> = split_top_level(v).into_iter().filter_map(parse_color).collect();
        st.border_color = colors.first().copied();
        if let Some(rare) = st.rare.as_mut() {
            rare.border_colors = [None; 4];
        }
        if let Some(sides) = four_sides(&colors).filter(|_| colors.len() > 1) {
            st.rare_mut().border_colors = sides.map(Some);
        }
    }
    for (i, side) in SIDES.iter().enumerate() {
        if let Some(c) = p.get(&format!("border-{side}-color")).and_then(|v| parse_color(first(v))) {
            st.rare_mut().border_colors[i] = Some(c);
        }
        if let Some(w) = p.get(&format!("border-{side}-width")).and_then(|v| outline_width(first(v))) {
            widths[i] = w;
        }
        if let Some(s) = p.get(&format!("border-{side}-style")).and_then(|v| BorderStyle::parse(first(v))) {
            styles[i] = s;
        }
    }
    // Something drawn was asked for, and nothing says how.
    let off = |k: &str| p.get(k).is_some_and(|v| matches!(v.trim(), "none" | "0" | "0px" | "hidden"));
    let asked = ["border", "border-width", "border-color", "border-top", "border-right", "border-bottom", "border-left"]
        .iter()
        .any(|k| p.contains_key(*k) && !off(k))
        || SIDES.iter().any(|s| p.contains_key(&format!("border-{s}-width")));
    if asked && styles.iter().all(|s| *s == BorderStyle::None) {
        // Quoting the declaration, since this is said from the build, which
        // has no line to point at.
        let named = ["border", "border-top", "border-right", "border-bottom", "border-left", "border-width", "border-color"]
            .iter()
            .find_map(|k| p.get(*k).map(|v| format!("`{k}: {v}`")))
            .unwrap_or_else(|| "A border width".to_string());
        warn_once(format!(
            "{named} draws no border: with no style it is `none`, as in CSS. Add `solid` \
             (`border: 2px solid red`). Rux drew it solid before v0.9."
        ));
    }
    for i in 0..4 {
        if matches!(styles[i], BorderStyle::None | BorderStyle::Hidden) {
            widths[i] = 0.0;
        }
    }
    st.border = Sides { top: widths[0], right: widths[1], bottom: widths[2], left: widths[3] };
    st.border_style = styles;
}

/// One to four values to the four sides, as `margin` takes them.
fn four_sides<T: Copy>(v: &[T]) -> Option<[T; 4]> {
    Some(match *v {
        [a] => [a; 4],
        [a, b] => [a, b, a, b],
        [a, b, c] => [a, b, c, b],
        [a, b, c, d] => [a, b, c, d],
        _ => return None,
    })
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
        "end" => TextAlign::End,
        "left" => TextAlign::Left,
        "right" => TextAlign::Right,
        "justify" => TextAlign::Justify,
        _ => TextAlign::Start,
    }
}

fn parse_color(s: &str) -> Option<Rgba> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    // `rgb()`, `hsl()`, `oklch()`, `color()`, `color-mix()` and the rest.
    if let Some((name, rest)) = s.split_once('(') {
        return color::function(name, rest.strip_suffix(')')?, &parse_color);
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
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let (r, g, b, a) = match bytes.len() {
        3 => (expand(bytes[0]), expand(bytes[1]), expand(bytes[2]), 255),
        // `#rgba`, which is how lightningcss writes `transparent` (`#0000`)
        // and every short colour with an alpha. Refused until v0.9, so a
        // gradient to `transparent` was no gradient at all.
        4 => (expand(bytes[0]), expand(bytes[1]), expand(bytes[2]), expand(bytes[3])),
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

    /// A namespace for a test registry keyed by tag.
    ///
    /// The real loader keys components by file and gives each file its own
    /// namespace; a test that writes one map by hand wants the flat thing that
    /// used to exist, which is every tag visible to the document under its own
    /// name.
    fn flat(components: &HashMap<String, crate::Sfc>) -> HashMap<String, super::Namespace> {
        let namespace: super::Namespace =
            components.keys().map(|tag| (tag.clone(), tag.clone())).collect();
        HashMap::from([(super::DOCUMENT_NAMESPACE.to_string(), namespace)])
    }
    use rux_script::{Builder, Engine};

    /// **A password is painted as bullets, and nothing of the text survives.**
    ///
    /// The mask is display only: every read of the signal, and the text handed
    /// to an input method, stays the real thing, or editing breaks outright.
    #[test]
    fn a_password_field_paints_one_bullet_per_character() {
        let shown = super::mask("hunter2");
        assert_eq!(shown, "•••••••");
        assert_eq!(shown.chars().count(), "hunter2".chars().count(), "one bullet per char");
        assert!(!shown.contains("hunter"), "and none of the text is left");
    }

    /// An empty value masks to nothing, so the placeholder still wins.
    ///
    /// The placeholder is the author's label and not the person's secret, and a
    /// row of bullets where "Password" belongs reads as a field already filled.
    #[test]
    fn an_empty_password_masks_to_nothing() {
        assert_eq!(super::mask(""), "");
    }

    /// The mask counts the way the caret moves, which is by `char`.
    ///
    /// Deliberately not by grapheme. The shell steps the caret and Backspace by
    /// scalar, so a grapheme mask would paint one bullet where the caret has
    /// two places to stand. Written down as a test so that whoever makes text
    /// editing grapheme-aware finds this and moves it with them.
    #[test]
    fn the_mask_counts_by_char_because_the_caret_does() {
        // 'e' plus a combining acute: one thing to read, two to the caret.
        let combining = "e\u{301}";
        assert_eq!(combining.chars().count(), 2);
        assert_eq!(super::mask(combining).chars().count(), 2);
    }
    use std::collections::HashMap;

    /// `touch-action` decides who wins when a `@drag` and a scroller both want
    /// the same finger, so an unparsed value would silently hand every gesture
    /// to the element and reinstate the dead zone this property exists to fix.
    #[test]
    fn touch_action_parses() {
        use super::TouchAction;
        let of = |v: &str| {
            interpret(&HashMap::from([("touch-action".to_string(), v.to_string())])).touch_action
        };
        assert_eq!(of("none"), TouchAction::None);
        assert_eq!(of("pan-x"), TouchAction::PanX);
        assert_eq!(of("pan-y"), TouchAction::PanY);
        assert_eq!(of("auto"), TouchAction::Auto);
        // Whitespace is CSS-legal and must not change the meaning.
        assert_eq!(of("  none  "), TouchAction::None);
        // `manipulation` only disables double-tap zoom on the web, which has no
        // equivalent here, so it means `auto` rather than being an error.
        assert_eq!(of("manipulation"), TouchAction::Auto);
        // The default is CSS's, and is NOT "the element wins".
        assert_eq!(interpret(&HashMap::new()).touch_action, TouchAction::Auto);
    }

    /// `honored_pseudo_classes` is what the editor completes `:` from, and
    /// `Pseudo::parse` is what actually matches. A name in the first that the
    /// second does not know would be a completion for a rule that never fires,
    /// which is the one failure mode the whole vocabulary arrangement exists to
    /// prevent.
    #[test]
    fn honored_pseudo_classes_all_parse() {
        use super::{honored_pseudo_classes, Pseudo};
        for name in honored_pseudo_classes() {
            // The functional ones are offered by name and written with an
            // argument.
            let written = match *name {
                "nth-child" => "nth-child(2n+1)".to_string(),
                "nth-last-child" => "nth-last-child(2)".to_string(),
                "not" => "not(.a)".to_string(),
                "is" | "where" => format!("{name}(.a .b)"),
                n => n.to_string(),
            };
            assert!(
                !matches!(Pseudo::parse(&written), Pseudo::Unknown(_)),
                "`:{name}` is offered as a completion and `Pseudo::parse` does not know it"
            );
        }
        assert!(
            matches!(Pseudo::parse("first-of-type"), Pseudo::Unknown(_)),
            "a pseudo-class Rux does not implement must still parse as Unknown"
        );
    }

    /// A percentage on a property that cannot take one is reported, not dropped.
    ///
    /// The interpreter reads these with `parse_px`, which has no `%`, while the
    /// length check validated with `parse_len`, which does. So the value was
    /// thrown away and then pronounced fine, by two functions in this file that
    /// disagreed about what a length is. Reported as `border-radius` not taking
    /// a percentage; it was four properties.
    ///
    /// **`border-radius` has since left this list**, because it is the one of
    /// the four whose percentage has somewhere to resolve: it is not consumed
    /// by layout, only by paint, and paint knows the box. The other three
    /// resolve during the cascade and still have nothing to measure against.
    #[test]
    fn a_percentage_where_there_is_no_box_says_so() {
        use super::{parse_len, parse_px, LENGTH_ONLY_PROPERTIES, PX_ONLY_PROPERTIES};

        // The split is the whole fix: every px-only property must actually be
        // one the interpreter reads with `parse_px`, and `%` must fail there.
        for name in PX_ONLY_PROPERTIES {
            assert!(
                LENGTH_ONLY_PROPERTIES.contains(name),
                "`{name}` is px-only and not length-checked at all"
            );
        }
        assert!(parse_px("50%").is_none(), "px-only parsing has no percentage");
        assert!(parse_len("50%").is_some(), "the Len parser does");

        // `width` resolves during layout, against a box that exists by then, so
        // it keeps its percentage and must not be caught by this.
        assert!(!PX_ONLY_PROPERTIES.contains(&"width"));
        assert!(!PX_ONLY_PROPERTIES.contains(&"height"));
        assert!(!PX_ONLY_PROPERTIES.contains(&"top"));
        // Three of the four that were silently dropped. Still px-only, still
        // reported.
        for name in ["padding", "margin", "gap"] {
            assert!(PX_ONLY_PROPERTIES.contains(&name), "`{name}` takes no percentage");
        }
        // The fourth takes one now, and must not be warned about.
        for name in [
            "border-radius",
            "border-top-left-radius",
            "border-top-right-radius",
            "border-bottom-right-radius",
            "border-bottom-left-radius",
        ] {
            assert!(
                !PX_ONLY_PROPERTIES.contains(&name),
                "`{name}` resolves its percentage at paint and must not be reported"
            );
        }
    }

    /// A percentage radius is carried to paint rather than resolved here.
    #[test]
    fn a_percentage_radius_is_carried_as_a_percentage() {
        use super::{parse_border_radius, parse_border_radius_pct};

        // The diagonal grouping is the same in both halves.
        assert_eq!(parse_border_radius_pct("50%"), [Some(50.0); 4]);
        assert_eq!(
            parse_border_radius_pct("50% 0px"),
            [Some(50.0), None, Some(50.0), None],
            "a corner in px carries no percentage"
        );
        assert_eq!(
            parse_border_radius_pct("10% 20% 30% 40%"),
            [Some(10.0), Some(20.0), Some(30.0), Some(40.0)]
        );
        // Plain pixels carry none at all, so nothing changes for every document
        // that never writes a percentage.
        assert_eq!(parse_border_radius_pct("12px"), [None; 4]);
        assert_eq!(parse_border_radius("12px"), [12.0; 4]);
    }

    /// The three states an unhonored property can be in have to stay three.
    ///
    /// They were one message for a long time, so `filter` (real CSS, not
    /// built), `paddding` (a typo) and `florble` (invented) all read as "parsed
    /// but not yet honored", and an author with a typo waited for a release.
    #[test]
    fn unhonored_properties_report_three_different_things() {
        use super::{nearest_property, is_honored, UNIMPLEMENTED_PROPERTIES};

        // Real CSS, unbuilt: named as such, and never offered as a correction
        // for itself.
        assert!(UNIMPLEMENTED_PROPERTIES.contains(&"float"));
        assert!(UNIMPLEMENTED_PROPERTIES.contains(&"perspective"));

        // A typo finds the property it was reaching for.
        assert_eq!(nearest_property("paddding"), Some("padding"));
        assert_eq!(nearest_property("bordr-radius"), Some("border-radius"));
        assert_eq!(nearest_property("colour"), Some("color"));

        // Invented nonsense suggests nothing rather than reaching for the
        // nearest unrelated name.
        assert_eq!(nearest_property("florble"), None);

        // A short name gets a short budget, so one edit still suggests and two
        // does not. `tap` is a plausible slip for `gap`; `zzz` is not a slip
        // for anything and must stay silent rather than reach for `gap`.
        assert_eq!(nearest_property("tap"), Some("gap"));
        assert_eq!(nearest_property("zzz"), None);

        // Nothing honored should ever reach the warning at all.
        assert!(is_honored("padding"));
    }

    /// No name may claim to be both honored and unimplemented. The two lists
    /// drive different messages, so an overlap would make which one an author
    /// sees depend on the order the checks happen to run in.
    #[test]
    fn honored_and_unimplemented_never_overlap() {
        use super::{honored_properties, UNIMPLEMENTED_PROPERTIES};
        for name in UNIMPLEMENTED_PROPERTIES {
            assert!(
                !honored_properties().contains(name),
                "`{name}` is listed as both honored and unimplemented"
            );
        }
    }

    /// The same pairing for `transition`'s values.
    #[test]
    fn animatable_properties_all_parse() {
        use super::{animatable_properties, parse_anim_prop};
        for name in animatable_properties() {
            assert!(
                parse_anim_prop(name).is_some(),
                "`transition: {name}` is offered and `parse_anim_prop` rejects it"
            );
        }
        assert!(parse_anim_prop("float").is_none());
    }

    /// The pointer attributes reach the node, in the vocabulary's order rather
    /// than the order they happen to be written, and an `r-for` local is baked
    /// into the body the way a `@tap`'s is.
    #[test]
    fn gesture_attributes_reach_the_node() {
        use rux_layout::Gesture;
        let src = "<template><screen>                     <view @drag=\"a()\" @press=\"b()\" @longpress=\"c()\" />                   </screen></template>";
        let sfc = rux_parser::parse_sfc(src).expect("parses");
        let mut engine = Builder::new().build("").expect("engine");
        let tree = build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).expect("builds");
        let node = &tree.children[0];
        let kinds: Vec<Gesture> = node.gestures.iter().map(|(g, _)| *g).collect();
        assert_eq!(
            kinds,
            vec![Gesture::Press, Gesture::LongPress, Gesture::Drag],
            "declared order does not decide dispatch order"
        );
        assert!(node.on_tap.is_none(), "a pointer handler is not a tap");
    }

    /// Every kind of CSS warning must land on the line the reader can see, not
    /// on a line counted from the start of the `<style>` block. Getting this
    /// wrong sends someone confidently to the wrong part of their file, which is
    /// why the offset is carried rather than assumed to be zero.
    #[test]
    fn css_warnings_carry_the_line_of_the_file() {
        let src = "<template>\n  <screen class=\"a\"></screen>\n</template>\n\n<style>\n  .a { display: flex; }\n  .b { float: left; }\n\n  .c:nope { color: red; }\n\n  @media (scripting: none) { .a { gap: 4px; } }\n</style>\n";
        let sfc = rux_parser::parse_sfc(src).expect("parses");
        let mut engine = Builder::new().build("").expect("engine");

        let _ = super::take_warnings(); // start from a clean sink
        let _ = build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).expect("builds");
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
        let _ = build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).expect("builds");
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
        let _ = build_styled_tree(&main, &components, &flat(&components), &mut engine).expect("builds");
        let warnings = super::take_warnings();

        let float = warnings
            .iter()
            .find(|w| w.message.contains("float"))
            .expect("the component's unhonored property is still reported");
        assert_eq!(float.line, None, "but without a line from another file");
    }

    /// Build a document with one component and return the component's root node.
    fn component_root(doc: &str, comp: &str) -> super::LayoutNode {
        let main = rux_parser::parse_sfc(doc).expect("document parses");
        let component = rux_parser::parse_sfc(comp).expect("component parses");
        let mut components = HashMap::new();
        components.insert("chip".to_string(), component);
        let mut engine = Builder::new().build("").expect("engine");
        let root = build_styled_tree(&main, &components, &flat(&components), &mut engine).expect("builds");
        // <screen> -> the expanded component's root.
        root.children.into_iter().next().expect("the component rendered")
    }

    const DOC: &str = "<template>\n  <screen class=\"app\"><chip /></screen>\n</template>\n\
                       <style>\n  .chip { width: 120px; }\n</style>\n\
                       <script>\nuse components::chip;\n</script>\n";

    /// A document's stylesheet reaches the components it uses.
    ///
    /// Before this, a component saw only its own `<style>`, so sharing a look
    /// meant repeating `<style src="theme.css">` in every component file. That
    /// is the wall people hit first on a real app.
    #[test]
    fn a_documents_rules_reach_its_components() {
        let chip = "<template>\n  <view class=\"chip\"></view>\n</template>\n";
        let node = component_root(DOC, chip);
        assert_eq!(
            node.style.width,
            Some(Len::Px(120.0)),
            "the document's `.chip` rule did not reach the component"
        );
    }

    /// `<style scoped>` on the component means "I own my appearance".
    #[test]
    fn a_scoped_component_is_not_styled_from_outside() {
        let chip = "<template>\n  <view class=\"chip\"></view>\n</template>\n\
                    <style scoped>\n  .chip { height: 40px; }\n</style>\n";
        let node = component_root(DOC, chip);
        assert_eq!(node.style.height, Some(Len::Px(40.0)), "its own rule still applies");
        assert_eq!(node.style.width, None, "`scoped` did not keep the document out");
    }

    /// `<style scoped>` on the document means "my rules stay in my markup".
    #[test]
    fn a_scoped_document_does_not_style_its_components() {
        let doc = "<template>\n  <screen class=\"app\"><chip /></screen>\n</template>\n\
                   <style scoped>\n  .chip { width: 120px; }\n</style>\n\
                   <script>\nuse components::chip;\n</script>\n";
        let chip = "<template>\n  <view class=\"chip\"></view>\n</template>\n";
        assert_eq!(component_root(doc, chip).style.width, None);
    }

    /// Same specificity, later wins, which is CSS's own rule. The component's
    /// rules are appended after the document's, so a component can override the
    /// look it inherits without resorting to a more specific selector.
    #[test]
    fn a_components_own_rule_beats_the_one_it_inherits() {
        let chip = "<template>\n  <view class=\"chip\"></view>\n</template>\n\
                    <style>\n  .chip { width: 200px; }\n</style>\n";
        assert_eq!(component_root(DOC, chip).style.width, Some(Len::Px(200.0)));
    }

    /// `padding: 1rem` used to be dropped on the floor, with no warning.
    ///
    /// `width` went through `parse_len`, which has understood `rem` since it was
    /// written, while padding, margin, gap, border widths and corner radii went
    /// through a px-only parser. So `width: 2rem` worked and `padding: 2rem`
    /// silently did nothing, which is the worst possible pair of behaviours: the
    /// unit is obviously supported, so the failure reads as a layout bug.
    #[test]
    fn rem_is_honored_by_every_length_property() {
        let mut p = HashMap::new();
        p.insert("padding".to_string(), "1rem 2rem".to_string());
        p.insert("margin-left".to_string(), "0.5rem".to_string());
        p.insert("gap".to_string(), "1rem".to_string());
        p.insert("border-radius".to_string(), "0.5rem".to_string());
        p.insert("border-width".to_string(), "0.125rem".to_string());
        // A width draws only with a style, as in CSS.
        p.insert("border-style".to_string(), "solid".to_string());
        p.insert("width".to_string(), "10rem".to_string());

        let st = interpret(&p);
        assert_eq!((st.padding.top, st.padding.right), (16.0, 32.0), "padding shorthand");
        assert_eq!(st.margin.left, 8.0, "per-side longhand");
        assert_eq!(st.gap, 16.0);
        assert_eq!(st.radius, [8.0, 8.0, 8.0, 8.0]);
        assert_eq!(st.border.top, 2.0);
        assert_eq!(st.width, Some(Len::Px(160.0)), "width already worked; it must still");
    }

    /// `em` is resolved in a pass before `interpret`, against the element's own
    /// resolved font size, so it has to be tested through that pass rather than
    /// through `interpret` directly.
    #[test]
    fn em_resolves_against_the_font_size_and_leaves_rem_alone() {
        use super::resolve_units_em;
        assert_eq!(resolve_units_em("1em", 20.0), "20px");
        assert_eq!(resolve_units_em("1em 2em", 10.0), "10px 20px");
        assert_eq!(resolve_units_em("0.5em", 32.0), "16px");

        // The trap: `strip_suffix("em")` also matches the tail of `rem`, so a
        // careless pass turns `1rem` into `1r` + em and either fails or, worse,
        // resolves it against the font size.
        assert_eq!(resolve_units_em("1rem", 20.0), "1rem", "rem must survive untouched");
        assert_eq!(resolve_units_em("1rem 2em", 10.0), "1rem 20px");

        // Words that merely contain the letters are not lengths.
        assert_eq!(resolve_units_em("system-ui", 16.0), "system-ui");
        assert_eq!(resolve_units_em("2emu", 16.0), "2emu");
        assert_eq!(resolve_units_em("#12em34", 16.0), "#12em34", "not at a token boundary");
    }

    /// `font-size` in `em` means "times the inherited size", not its own, so it
    /// has to resolve before the pass that uses it.
    #[test]
    fn font_size_in_em_is_relative_to_the_inherited_size() {
        use super::parse_px_len;
        assert_eq!(parse_px_len("1.5em", 16.0), Some(24.0));
        assert_eq!(parse_px_len("2rem", 16.0), Some(32.0));
        assert_eq!(parse_px_len("2rem", 40.0), Some(32.0), "rem ignores the element");
        assert_eq!(parse_px_len("12px", 16.0), Some(12.0));
        assert_eq!(parse_px_len("12", 16.0), Some(12.0), "bare numbers still parse");
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
    fn transition_parses_a_list_in_any_order() {
        use super::{AnimProp, Easing};
        let ts = super::parse_transitions("opacity 150ms ease-out, transform .3s 50ms linear");
        assert_eq!(ts.len(), 2);
        assert_eq!(ts[0].property, AnimProp::Opacity);
        assert_eq!(ts[0].duration, 150.0);
        assert_eq!(ts[0].delay, 0.0);
        assert_eq!(ts[0].easing, Easing::EASE_OUT);
        // `.3s` is 300ms, and the *second* time is the delay however it is ordered.
        assert_eq!(ts[1].property, AnimProp::Transform);
        assert_eq!(ts[1].duration, 300.0);
        assert_eq!(ts[1].delay, 50.0);
        assert_eq!(ts[1].easing, Easing::Linear);

        // A bare time is `all`, and the default curve is `ease`, as in CSS.
        let ts = super::parse_transitions("200ms");
        assert_eq!(ts[0].property, AnimProp::All);
        assert_eq!(ts[0].easing, Easing::EASE);

        // A comma inside cubic-bezier() is not an entry separator.
        let ts = super::parse_transitions("width 1s cubic-bezier(0.2, 0, 0.1, 1)");
        assert_eq!(ts.len(), 1);
        assert_eq!(ts[0].easing, Easing::Bezier(0.2, 0.0, 0.1, 1.0));

        // `none` is not an entry, and an out-of-range bezier x falls back to ease.
        assert!(super::parse_transitions("none").is_empty());
        assert_eq!(
            super::parse_transitions("opacity 1s cubic-bezier(2, 0, 0.1, 1)")[0].easing,
            Easing::EASE
        );
    }

    #[test]
    fn an_unanimatable_transition_property_warns_instead_of_vanishing() {
        // The whole point: a property that cannot animate has to say so, or the
        // author is left with an element that just never moves.
        let _ = super::take_warnings();
        assert!(super::parse_transitions("display 1s").is_empty());
        let warnings = super::take_warnings();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("`display` cannot be animated"));

        // A longhand of an animatable shorthand is pointed at the shorthand.
        assert!(super::parse_transitions("padding-left 1s").is_empty());
        let warnings = super::take_warnings();
        assert!(warnings[0].message.contains("transition `padding` instead"));
    }

    #[test]
    fn easing_curves_are_monotonic_and_pinned_at_the_ends() {
        use super::Easing;
        for easing in [Easing::Linear, Easing::EASE, Easing::EASE_IN, Easing::EASE_OUT, Easing::EASE_IN_OUT] {
            assert_eq!(easing.eval(0.0), 0.0);
            assert_eq!(easing.eval(1.0), 1.0);
            let mut prev = 0.0;
            for i in 1..=100 {
                let y = easing.eval(i as f32 / 100.0);
                assert!(y >= prev - 1e-3, "{easing:?} went backwards at {i}");
                prev = y;
            }
        }
        // ease-out is ahead of linear in the first half: that is what "out" means.
        assert!(Easing::EASE_OUT.eval(0.25) > 0.25);
        assert!(Easing::EASE_IN.eval(0.25) < 0.25);
    }

    /// `steps()` holds each value for its interval and jumps where its
    /// position says, as CSS Easing's own examples do.
    #[test]
    fn steps_jump_where_their_position_says() {
        use super::{parse_easing as p, Easing, StepPosition};
        let at = |e: &str, t: f32| p(e).unwrap_or_else(|| panic!("{e} did not parse")).eval(t);
        assert_eq!(p("steps(4)"), Some(Easing::Steps(4, StepPosition::End)));
        assert_eq!(p("step-start"), Some(Easing::Steps(1, StepPosition::Start)));
        // jump-end: 0, .25, .5, .75 and 1 only at the very end.
        assert_eq!([at("steps(4, end)", 0.0), at("steps(4, end)", 0.3), at("steps(4, end)", 0.99), at("steps(4, end)", 1.0)], [0.0, 0.25, 0.75, 1.0]);
        // jump-start: already .25 at the start.
        assert_eq!([at("steps(4, jump-start)", 0.0), at("steps(4, start)", 0.5)], [0.25, 0.75]);
        // jump-both: five levels for four steps; jump-none: neither end jumps.
        assert_eq!([at("steps(4, jump-both)", 0.0), at("steps(4, jump-both)", 1.0)], [0.2, 1.0]);
        assert_eq!([at("steps(5, jump-none)", 0.0), at("steps(5, jump-none)", 0.5), at("steps(5, jump-none)", 1.0)], [0.0, 0.5, 1.0]);
        assert_eq!([at("step-end", 0.5), at("step-start", 0.5)], [0.0, 1.0]);
        // CSS refuses these, so Rux does.
        for bad in ["steps(0)", "steps(1, jump-none)", "steps(2, sideways)", "steps(1.5)"] {
            assert_eq!(p(bad), None, "{bad}");
        }
        // And a transition says so instead of easing quietly.
        let t = super::parse_transitions("opacity 1s steps(3, jump-start)");
        assert_eq!(t[0].easing, Easing::Steps(3, StepPosition::Start));
        let _ = super::take_warnings();
        let t = super::parse_transitions("opacity 1s linear(0, 0.5, 1)");
        assert_eq!(t[0].easing, Easing::EASE);
        assert!(super::take_warnings().iter().any(|w| w.message.contains("linear(0, 0.5, 1)")));
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
        // `#rgba`, lightningcss's spelling of `transparent`, and a gradient to it.
        assert_eq!(parse_color("#0000").map(|c| c.a), Some(0.0));
        assert_eq!(parse_color("#f008").map(|c| (c.r, c.a)), Some((1.0, 136.0 / 255.0)));
        assert!(parse_color("#ggg").is_none());
        assert!(super::parse_background("linear-gradient(#000, #0000)").is_some());
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
    fn resolution_reads_every_unit_css_allows() {
        use super::parse_resolution;
        assert_eq!(parse_resolution("2dppx"), Some(2.0));
        assert_eq!(parse_resolution("2x"), Some(2.0));
        // 96 dots per inch is the CSS reference, so 192dpi is exactly 2.
        assert_eq!(parse_resolution("192dpi"), Some(2.0));
        assert!((parse_resolution("75.5906dpcm").unwrap() - 2.0).abs() < 1e-3);

        // A bare number says nothing about which unit, and CSS refuses it too.
        assert_eq!(parse_resolution("2"), None);
        assert_eq!(parse_resolution("0dppx"), None, "a screen cannot have no density");
        assert_eq!(parse_resolution("nonsense"), None);
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
        let mut instances = super::Instances::new();
        let (_root, reg) =
            build_styled_tree_tracked(&sfc, &HashMap::new(), &HashMap::new(), &mut engine, &mut instances, &mut crate::Swaps::new()).unwrap();

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

        // A hard stop, in both spellings: a stop behind the one before it is
        // moved up to it, and `0` is how lightningcss writes the second half.
        for css in ["linear-gradient(to right, red 50%, blue 50%)", "linear-gradient(90deg, red 50%, blue 0)"] {
            let g = grad(css);
            assert_eq!([g.stops[0].1, g.stops[1].1], [0.5, 0.5], "{css}");
        }
        // A stop with two positions is two stops of one colour.
        let g = grad("linear-gradient(to right, red 25%, #0f0 25% 75%, #00f 75%)");
        let at: Vec<f32> = g.stops.iter().map(|s| s.1).collect();
        assert_eq!(at, [0.25, 0.25, 0.75, 0.75]);
        assert_eq!(g.stops[2].0.g, 1.0);

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
    fn maps_z_index_and_isolation() {
        let z = |v: &str| {
            let mut p = HashMap::new();
            p.insert("z-index".to_string(), v.to_string());
            p.insert("isolation".to_string(), "isolate".to_string());
            let st = interpret(&p);
            assert!(st.isolate);
            st.z_index
        };
        assert_eq!(z("3"), Some(3));
        assert_eq!(z("-1"), Some(-1));
        assert_eq!(z("auto"), None);
        let _ = super::take_warnings();
        assert_eq!(z("1.5"), None, "not a whole number");
        assert!(super::take_warnings().iter().any(|w| w.message.contains("z-index: 1.5")));
    }

    #[test]
    fn maps_grid_templates_through_the_stylesheet() {
        use rux_layout::{GridName, GridPlace, GridTrack, Repeat, Track};
        let css = r#".g { grid-template-columns: [full-start] 1fr repeat(auto-fill, minmax(100px, 1fr)) [full-end];
                         grid-template-areas: "head head" "side main"; }
                    .t { grid-template: "a a" 40px "b c" 1fr / 100px 1fr; }
                    .i { grid-area: main; grid-column: full-start / span 2; }
                    .j { grid-area: 2 / 1 / 3 / 3; }"#;
        let rules = parse_rules(css, Environment::sane());
        let style = |i: usize| {
            let p: HashMap<String, String> = rules[i].decls.iter().cloned().collect();
            interpret(&p)
        };
        let g = style(0);
        assert_eq!(g.grid_columns.len(), 2);
        assert!(matches!(&g.grid_columns[1], GridTrack::Repeat(Repeat::AutoFill, t, _) if t.len() == 1));
        let names = g.grid_names.as_deref().expect("named");
        assert_eq!(names.columns, vec![vec![GridName::new("full-start")], vec![], vec![GridName::new("full-end")]]);
        let areas = names.areas.as_ref().expect("areas");
        assert_eq!((areas.rows, areas.columns), (2, 2));
        assert!(areas.areas.contains(&(GridName::new("main"), 2, 3, 2, 3)));

        let t = style(1);
        assert_eq!(t.grid_rows, vec![GridTrack::One(Track::Px(40.0)), GridTrack::One(Track::Fr(1.0))]);
        assert_eq!(t.grid_columns.len(), 2);
        assert_eq!(t.grid_names.as_deref().and_then(|n| n.areas.as_ref()).map(|a| a.areas.len()), Some(3));

        let i = style(2);
        assert_eq!(i.grid_row, (GridPlace::Named(GridName::new("main"), 0), GridPlace::Named(GridName::new("main"), 0)));
        assert_eq!(i.grid_column, (GridPlace::Named(GridName::new("full-start"), 0), GridPlace::Span(2)));
        let j = style(3);
        assert_eq!((j.grid_row, j.grid_column), ((GridPlace::Line(2), GridPlace::Line(3)), (GridPlace::Line(1), GridPlace::Line(3))));
    }

    #[test]
    fn maps_position_try() {
        use rux_layout::{AnchorSide, TryInset, TryOrder};
        let css = ".menu { position: fixed; top: anchor(bottom); position-try: most-height --above, flip-inline flip-block, --nothing, top; }
                   @position-try --above { top: auto; bottom: anchor(top, 8px); margin-bottom: 4px; width: 10px }";
        let rules = parse_rules(css, Environment::sane());
        let p: HashMap<String, String> = rules[0].decls.iter().cloned().collect();
        let st = interpret(&p);
        let a = st.anchor.as_deref().expect("anchored");
        assert_eq!(a.try_order, TryOrder::MostHeight);
        // `--nothing` has no rule and `top` is a position-area: both skipped.
        assert_eq!(a.tries.len(), 2, "{:?}", a.tries);
        let above = &a.tries[0];
        assert_eq!(above.inset[0], Some(TryInset::Auto));
        assert!(matches!(&above.inset[2], Some(TryInset::Anchor(r)) if r.side == AnchorSide::Top && r.fallback == Some(Len::Px(8.0))));
        assert_eq!(above.inset[1], None, "a side it does not name stays the box's own");
        assert_eq!(above.margin, [None, None, Some(4.0), None]);
        assert!(a.tries[1].flip_block && a.tries[1].flip_inline && !a.tries[1].flip_start);

        let mut p = HashMap::new();
        p.insert("position-try-fallbacks".to_string(), "none".to_string());
        assert!(interpret(&p).anchor.is_none());
    }

    #[test]
    fn maps_anchor_positioning() {
        use rux_layout::{AnchorRef, AnchorSide, AnchorSize};
        let mut p = HashMap::new();
        p.insert("anchor-name".to_string(), "--btn".to_string());
        p.insert("position-anchor".to_string(), "--other".to_string());
        p.insert("top".to_string(), "anchor(bottom)".to_string());
        p.insert("left".to_string(), "anchor(--btn 25%, 8px)".to_string());
        p.insert("right".to_string(), "4px".to_string());
        p.insert("width".to_string(), "anchor-size(--btn width)".to_string());
        let _ = super::take_warnings();
        let st = interpret(&p);
        assert!(super::take_warnings().is_empty(), "nothing to say about any of it");
        let a = st.anchor.as_deref().expect("anchored");
        assert_eq!(a.name.as_deref(), Some("--btn"));
        assert_eq!(a.position_anchor.as_deref(), Some("--other"));
        assert_eq!(a.inset[0], Some(AnchorRef { name: None, side: AnchorSide::Bottom, fallback: None }));
        assert_eq!(
            a.inset[3],
            Some(AnchorRef { name: Some("--btn".into()), side: AnchorSide::Frac(0.25), fallback: Some(Len::Px(8.0)) })
        );
        assert!(st.inset[0].is_none() && st.inset[3].is_none(), "an anchored side is not the layout's");
        assert!(matches!(st.inset[1], Some(Len::Px(v)) if v == 4.0));
        assert_eq!(a.size[0], Some(AnchorSize { name: Some("--btn".into()), height: false, fallback: None }));
        assert!(st.width.is_none());

        // The wrong axis, and a name without its dashes, are said.
        let mut p = HashMap::new();
        p.insert("top".to_string(), "anchor(left)".to_string());
        p.insert("anchor-name".to_string(), "btn".to_string());
        let st = interpret(&p);
        assert!(st.anchor.is_none());
        let said: Vec<String> = super::take_warnings().into_iter().map(|w| w.message).collect();
        assert!(said.iter().any(|m| m.contains("wrong axis")), "{said:?}");
        assert!(said.iter().any(|m| m.contains("two dashes")), "{said:?}");
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
        let root = build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut e).unwrap();
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
        let root = build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).unwrap();

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
        let root = build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).unwrap();

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

        let root = build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).unwrap();
        let input = &root.children[0];
        assert_eq!(input.model.as_deref(), Some("name"), "r-model bound");
        // Empty signal → the placeholder is shown.
        assert_eq!(input.children[0].text.as_ref().unwrap().text, "Type here");

        // Simulate the shell editing the focused input, then rebuild.
        engine.set_string("name", "Cam");
        let root = build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).unwrap();
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
        let root = build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).unwrap();

        // The select evaluates :options to strings and shows the bound value.
        let select = &root.children[0];
        assert_eq!(select.model.as_deref(), Some("fruit"));
        assert_eq!(
            select.options.as_ref().expect("select has options"),
            &vec!["apple".to_string(), "pear".to_string(), "plum".to_string()]
        );
        assert_eq!(select.kind, rux_layout::InputKind::Text);
        assert_eq!(select.children[0].text.as_ref().unwrap().text, "pear");

        // The textarea is a multiline input (Enter → newline in the shell).
        let textarea = &root.children[1];
        assert_eq!(textarea.kind, rux_layout::InputKind::Textarea);
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
        let root = build_styled_tree(&main, &components, &flat(&components), &mut engine).unwrap();

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
            attrs: Vec::new(),
            states: ElemStates::default(),
            total: None,
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
        build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).unwrap()
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

    use super::{
        media_matches, parse_rules, ColorScheme, Environment, InteractionState, Viewport,
    };

    /// A window of this size, with every other answer left at its default.
    fn vp(width: f32, height: f32) -> Environment {
        Environment::from_viewport(Viewport { width, height })
    }

    /// Build at a given viewport and report the target's background.
    fn bg_at_vp(src: &str, env: Environment) -> Option<Background> {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let mut instances = super::Instances::new();
        let root = super::build_styled_tree_stateful(
            &sfc,
            &HashMap::new(),
            &HashMap::new(),
            &mut engine,
            &mut instances,
            &mut crate::Swaps::new(),
            &InteractionState::default(),
            env,
        )
        .unwrap();
        root.0.children[0].style.background.clone()
    }


    // ── units ───────────────────────────────────────────────────────────────

    #[test]
    fn absolute_and_font_units_become_pixels() {
        use super::resolve_units;
        assert_eq!(resolve_units("12pt", 16.0, None), "16px");
        assert_eq!(resolve_units("1in 1pc", 16.0, None), "96px 16px");
        assert_eq!(resolve_units("2.54cm", 16.0, None), "96px");
        assert_eq!(resolve_units("25.4mm", 16.0, None), "96px");
        assert_eq!(resolve_units("4ch 2ex", 20.0, None), "40px 20px");
        assert_eq!(resolve_units("1lh", 10.0, None), "12px");
        assert_eq!(resolve_units("2EM", 10.0, None), "20px", "units are not case-sensitive");
        assert_eq!(resolve_units("1rem 3s 45deg 2fr", 10.0, None), "1rem 3s 45deg 2fr", "left alone");
        assert_eq!(resolve_units("#12in34 url(a2in.png)", 10.0, None), "#12in34 url(a2in.png)");
    }

    #[test]
    fn viewport_units_are_pixels_in_the_cascade_and_kept_for_the_layout() {
        use super::resolve_units;
        let vp = Some((800.0, 600.0));
        assert_eq!(resolve_units("10vw 10vh", 16.0, vp), "80px 60px");
        assert_eq!(resolve_units("10vmin 10vmax", 16.0, vp), "60px 80px");
        assert_eq!(resolve_units("10svh 10dvw 10lvmin", 16.0, vp), "60px 80px 60px");
        assert_eq!(resolve_units("10svh 10vi 5dvmax", 16.0, None), "10vh 10vw 5vmax");
        assert_eq!(super::parse_len("50vmin"), Some(Len::Vmin(50.0)));
        assert_eq!(super::parse_len("20vmax"), Some(Len::Vmax(20.0)));

        // Through a build: `padding` is the cascade's, `width` the layout's.
        let src = "<template><screen><view class=\"a\"></view></screen></template>
<style>.a { padding: 2vw; width: 50vmin; font-size: 12pt; }</style>";
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let mut instances = super::Instances::new();
        let root = super::build_styled_tree_stateful(
            &sfc,
            &HashMap::new(),
            &HashMap::new(),
            &mut engine,
            &mut instances,
            &mut crate::Swaps::new(),
            &InteractionState::default(),
            Environment::from_viewport(Viewport { width: 1000.0, height: 500.0 }),
        )
        .unwrap();
        let st = &root.0.children[0].style;
        assert_eq!(st.padding.left, 20.0);
        assert_eq!(st.width, Some(Len::Vmin(50.0)));
        assert!(super::reads_viewport());
    }

    // ── env(safe-area-inset-*) ──────────────────────────────────────────────

    use super::{take_warnings, Insets};

    /// Build with these insets and return the padding of the first child.
    fn padding_with(src: &str, safe_area: Insets) -> rux_layout::Sides {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let mut instances = super::Instances::new();
        let root = super::build_styled_tree_stateful(
            &sfc,
            &HashMap::new(),
            &HashMap::new(),
            &mut engine,
            &mut instances,
            &mut crate::Swaps::new(),
            &InteractionState::default(),
            Environment { safe_area, ..Environment::sane() },
        )
        .unwrap();
        root.0.children[0].style.padding
    }

    /// `sane()` and `HEADLESS` are one thing said twice, and the constant is
    /// the copy that cannot be derived. The viewport in particular: a zero one
    /// makes every `max-width` query match, so every narrow layout applies in
    /// every test that never mentioned a window.
    #[test]
    fn the_headless_constant_is_the_sane_environment() {
        assert_eq!(Environment::HEADLESS, Environment::sane());
        assert_eq!(Environment::HEADLESS.viewport, Viewport::default());
        assert_eq!(Environment::HEADLESS.density, 1.0, "never zero: things divide by it");
    }

    const INSET_DOC: &str = r#"<template><screen><view class="bar" /></screen></template>
        <style>
          .bar {
            padding-top: env(safe-area-inset-top);
            padding-bottom: env(safe-area-inset-bottom);
          }
        </style>"#;

    /// The whole point: a stylesheet can read the strip of the display a notch
    /// or a home indicator has already taken, and lay itself out clear of it.
    #[test]
    fn env_reads_the_safe_area_insets() {
        let padding = padding_with(INSET_DOC, Insets { top: 47.0, bottom: 34.0, ..Insets::default() });
        assert_eq!((padding.top, padding.bottom), (47.0, 34.0));
    }

    /// On a desktop window every inset is zero, and zero is an answer rather
    /// than a failure: a window that owns its whole surface has no unsafe edge.
    /// The same stylesheet has to work there, unchanged.
    #[test]
    fn env_is_zero_where_nothing_is_taken() {
        let padding = padding_with(INSET_DOC, Insets::default());
        assert_eq!((padding.top, padding.bottom), (0.0, 0.0));
    }

    /// A fallback is used only when the name is one Rux cannot answer. A known
    /// name answering zero is still an answer, so the fallback must not win
    /// there: `env(safe-area-inset-top, 20px)` on a desktop is 0, not 20.
    #[test]
    fn a_fallback_covers_an_unknown_name_and_not_a_zero_answer() {
        let padding = padding_with(
            r#"<template><screen><view class="bar" /></screen></template>
            <style>
              .bar {
                padding-top: env(safe-area-inset-top, 20px);
                padding-bottom: env(titlebar-area-height, 12px);
              }
            </style>"#,
            Insets::default(),
        );
        assert_eq!(padding.top, 0.0, "a known inset answers zero");
        assert_eq!(padding.bottom, 12.0, "an unknown name takes the fallback");
    }

    /// An unknown name with no fallback leaves the declaration unparseable, so
    /// it is ignored, which is CSS's own "invalid at computed-value time", and
    /// it is said out loud rather than dropped in silence.
    #[test]
    fn an_unknown_env_with_no_fallback_is_ignored_and_reported() {
        let _ = take_warnings();
        let padding = padding_with(
            r#"<template><screen><view class="bar" /></screen></template>
            <style>.bar { padding-top: env(nonsense); }</style>"#,
            Insets { top: 47.0, ..Insets::default() },
        );
        assert_eq!(padding.top, 0.0, "the declaration is ignored, not guessed at");
        assert!(
            take_warnings().iter().any(|w| w.message.contains("nonsense")),
            "and the author is told which name it was"
        );
    }

    /// The way an author writes it once and reads it everywhere, so it has to
    /// resolve where the variable is declared, not only where a property is.
    #[test]
    fn a_custom_property_can_hold_an_env() {
        let padding = padding_with(
            r#"<template><screen class="app"><view class="bar" /></screen></template>
            <style>
              .app { --gutter: env(safe-area-inset-bottom); }
              .bar { padding-bottom: var(--gutter); }
            </style>"#,
            Insets { bottom: 34.0, ..Insets::default() },
        );
        assert_eq!(padding.bottom, 34.0);
    }

    /// `env(` is only a call at a token boundary. A custom property may be
    /// named anything, `--my-env` included, and substituting inside its name
    /// would corrupt a declaration that has nothing to do with this.
    #[test]
    fn a_property_whose_name_ends_in_env_is_not_a_call() {
        let padding = padding_with(
            r#"<template><screen class="app"><view class="bar" /></screen></template>
            <style>
              .app { --my-env: 9px; }
              .bar { padding-top: var(--my-env); }
            </style>"#,
            Insets { top: 47.0, ..Insets::default() },
        );
        assert_eq!(padding.top, 9.0);
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

    /// A later layer beats an earlier one whatever the specificity, and a
    /// rule in no layer beats them both. The order is the one `@layer a, b;`
    /// named, not the order the blocks come in.
    #[test]
    fn layers_decide_before_specificity() {
        let layered = r#"<template><screen><view class="target" id="t" /></screen></template>
            <style>
              @layer base, utilities;
              @layer utilities { .target { background: #00ff00; } }
              @layer base { #t { background: #ff0000; } }
            </style>"#;
        assert!(
            matches!(&bg_at_vp(layered, vp(800.0, 600.0)), Some(Background::Color(c)) if c.g == 1.0),
            "the utilities layer was named last, so its class beats the base layer's id"
        );
        let unlayered = r#"<template><screen><view class="target" id="t" /></screen></template>
            <style>
              .target { background: #ff0000; }
              @layer utilities { #t { background: #00ff00; } }
            </style>"#;
        assert!(is_red(&bg_at_vp(unlayered, vp(800.0, 600.0))), "a rule in no layer beats every layer");
    }

    /// `!important` beats every normal declaration, and turns layer order
    /// around: the earlier layer's important declaration wins.
    #[test]
    fn important_beats_normal_and_reverses_layers() {
        let src = r#"<template><screen><view class="target" id="t" /></screen></template>
            <style>
              @layer base, utilities;
              @layer base { .target { background: #ff0000 !important; } }
              @layer utilities { .target { background: #00ff00 !important; } }
              #t { background: #0000ff; }
            </style>"#;
        assert!(is_red(&bg_at_vp(src, vp(800.0, 600.0))));
    }

    /// `@supports` applies its rules when Rux honors what it asks about.
    #[test]
    fn supports_asks_whether_rux_honors_the_property() {
        let src = r#"<template><screen><view class="target" /></screen></template>
            <style>
              @supports (display: grid) { .target { background: #ff0000; } }
              @supports (-webkit-hyphens: none) { .target { background: #00ff00; } }
              @supports not (display: grid) { .target { background: #0000ff; } }
            </style>"#;
        assert!(is_red(&bg_at_vp(src, vp(800.0, 600.0))));
    }

    /// `hover` and `pointer` answer for what the person points with: a mouse
    /// hovers, a finger does not.
    #[test]
    fn hover_and_pointer_queries_ask_what_points() {
        let css = "@media (hover: hover) { .a { color: red } } @media (pointer: coarse) { .a { color: red } } \
                   @media (hover: none) { .a { color: red } } @media (any-hover) { .a { color: red } }";
        let mouse = vp(800.0, 600.0);
        let finger = Environment { pointer: super::Pointer::Coarse, ..mouse };
        assert_eq!(super::media_matches(css, mouse), vec![true, false, false, true]);
        assert_eq!(super::media_matches(css, finger), vec![false, true, true, false]);
    }

    /// A `@media` inside a layer still re-evaluates on a resize.
    #[test]
    fn media_inside_a_layer_is_reported_to_the_runtime() {
        let css = "@layer utilities { @media (min-width: 600px) { .a { color: red } } }";
        assert_eq!(super::media_matches(css, vp(800.0, 600.0)), vec![true]);
        assert_eq!(super::media_matches(css, vp(400.0, 600.0)), vec![false]);
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

    /// Both halves of both preferences answer, and answer against the
    /// environment rather than the window size.
    #[test]
    fn the_environment_queries_hold_and_fail_the_right_way_round() {
        let css = "@media (prefers-reduced-motion: reduce) { .a { color: red } }\
                   @media (prefers-reduced-motion: no-preference) { .b { color: red } }\
                   @media (prefers-color-scheme: dark) { .c { color: red } }\
                   @media (prefers-color-scheme: light) { .d { color: red } }";

        let calm = Environment { reduced_motion: true, ..Environment::sane() };
        assert_eq!(media_matches(css, calm), vec![true, false, false, true]);

        let dark = Environment { color_scheme: ColorScheme::Dark, ..Environment::sane() };
        assert_eq!(media_matches(css, dark), vec![false, true, true, false]);

        // The default is the ordinary case: full motion, light surfaces.
        assert_eq!(media_matches(css, Environment::sane()), vec![false, true, false, true]);
    }

    /// A stylesheet that never mentions the preference is stilled anyway.
    ///
    /// This is the half that matters. Most stylesheets will never write the
    /// query, so honouring only the query would leave almost every app
    /// animating at someone who asked it not to.
    #[test]
    fn reduced_motion_takes_the_time_out_of_a_transition_that_never_asked() {
        let src = r#"<template><screen><view class="target" /></screen></template>
        <style>
          .target { opacity: 1; transition: opacity 200ms ease 50ms; }
        </style>"#;

        let moving = transitions_of(src, Environment::sane());
        assert_eq!(moving.len(), 1, "the transition should be parsed at all");
        assert_eq!(moving[0], (200.0, 50.0), "the ordinary case still animates");

        let calm =
            transitions_of(src, Environment { reduced_motion: true, ..Environment::sane() });
        assert_eq!(
            calm,
            vec![(0.0, 0.0)],
            "reduced motion should zero the duration and the delay, not drop the declaration"
        );
    }

    /// Every `(duration, delay)` in the tree, in document order.
    fn transitions_of(src: &str, env: Environment) -> Vec<(f32, f32)> {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let mut instances = super::Instances::new();
        let (root, _) = super::build_styled_tree_stateful(
            &sfc,
            &HashMap::new(),
            &HashMap::new(),
            &mut engine,
            &mut instances,
            &mut crate::Swaps::new(),
            &InteractionState::default(),
            env,
        )
        .unwrap();
        let mut out = Vec::new();
        collect_transitions(&root, &mut out);
        out
    }

    fn collect_transitions(node: &rux_layout::Node, out: &mut Vec<(f32, f32)>) {
        for t in &node.style.transitions {
            out.push((t.duration, t.delay));
        }
        for child in &node.children {
            collect_transitions(child, out);
        }
    }

    /// A value neither half of a preference uses must not match either half.
    ///
    /// The fail-closed rule: an unsupported query hides its rules rather than
    /// applying them, so a typo loses a style instead of silently keeping one
    /// that was meant to be conditional.
    #[test]
    fn a_misspelled_preference_matches_nothing() {
        let css = "@media (prefers-reduced-motion: reduced) { .a { color: red } }\
                   @media (prefers-color-scheme: sepia) { .b { color: red } }";
        for env in [
            Environment::sane(),
            Environment { reduced_motion: true, ..Environment::sane() },
            Environment { color_scheme: ColorScheme::Dark, ..Environment::sane() },
        ] {
            assert_eq!(media_matches(css, env), vec![false, false]);
        }
    }

    /// The width queries must keep working now that they read through a struct
    /// rather than being handed one, and must ignore the rest of it.
    #[test]
    fn a_preference_does_not_disturb_a_width_query() {
        let css = "@media (max-width: 600px) { .a { color: red } }";
        let narrow = Environment::from_viewport(Viewport { width: 500.0, height: 800.0 });
        assert_eq!(media_matches(css, narrow), vec![true]);
        assert_eq!(
            media_matches(css, Environment { reduced_motion: true, ..narrow }),
            vec![true],
            "reduced motion is not a width"
        );
    }

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
        let root = build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).unwrap();
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
        let s = ElemStates {
            hover: false,
            focus: true,
            focus_visible: false,
            active: false,
            checked: true,
            current: false,
            enter_from: false,
            leave_to: false,
            disabled: true,
            enabled: false,
            valid: Some(false),
            user: false,
            required: Some(true),
        };
        assert!(hits_state("input:invalid", "input", s));
        assert!(!hits_state("input:user-invalid", "input", s), "nobody has been through it");
        assert!(hits_state("input:required", "input", s));
        assert!(!hits_state("input:optional", "input", s));
        assert!(hits_state("input:focus", "input", s));
        assert!(!hits_state("input:focus-visible", "input", s), "focused, but not to be shown");
        assert!(hits_state("input:disabled", "input", s));
        assert!(!hits_state("input:enabled", "input", s));
        assert!(hits_state("input:checked", "input", s));
        assert!(!hits_state("input:hover", "input", s));
        assert!(!hits_state("input:active", "input", s));
        assert!(!hits_state("input:current", "input", s));
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
        let all_on = ElemStates {
            hover: true,
            focus: true,
            focus_visible: true,
            active: true,
            checked: true,
            current: true,
            enter_from: true,
            leave_to: true,
            disabled: true,
            enabled: true,
            valid: Some(true),
            user: true,
            required: Some(true),
        };
        assert!(!hits_state(".box:last-child", ".box", all_on));
        assert!(!hits_state(".box:nth-last-child(1)", ".box", all_on));
        assert!(!hits_state(".box:not(.a .b)", ".box", all_on), "a combinator in :not() is refused");
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
        let root = build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).unwrap();

        let green = |n: &rux_layout::Node| {
            matches!(&n.style.background, Some(rux_layout::Background::Color(c)) if c.g == 1.0)
        };
        assert!(green(&root.children[0]), "ticked box matches .box:checked");
        assert!(!green(&root.children[1]), "unticked box does not");
    }

    fn anc(spec: &str, prev: &[&str]) -> AncNode {
        AncNode { desc: el(spec), prev: prev.iter().map(|s| el(s)).collect::<Vec<_>>().into(), place: prev.len() + 1, child_total: None, form: None }
    }

    /// `selector` against element `target` with the given ancestor chain
    /// (root-first) and preceding siblings (document order).
    fn hits(selector: &str, target: &str, ancestors: &[AncNode], prev: &[&str]) -> bool {
        let (chain, combs, _) = parse_selector(selector).expect("selector parses");
        let prev: Vec<ElemDesc> = prev.iter().map(|s| el(s)).collect();
        matches_chain(&chain, &combs, &el(target), ancestors, &prev)
    }

    /// `:first-child` and `:nth-child()` count the rendered siblings before
    /// an element, from 1.
    #[test]
    fn nth_child_counts_the_siblings_before() {
        let at = |selector: &str, n: usize| hits(selector, ".row", &[], &vec![".row"; n - 1]);
        assert!(at(".row:first-child", 1));
        assert!(!at(".row:first-child", 2));
        for (n, odd) in [(1, true), (2, false), (3, true), (4, false)] {
            assert_eq!(at(".row:nth-child(odd)", n), odd, "odd at {n}");
            assert_eq!(at(".row:nth-child(even)", n), !odd, "even at {n}");
            assert_eq!(at(".row:nth-child(2n+1)", n), odd, "2n+1 at {n}");
        }
        assert!(at(".row:nth-child(3)", 3) && !at(".row:nth-child(3)", 2));
        let first_three: Vec<bool> = (1..=5).map(|n| at(".row:nth-child(-n+3)", n)).collect();
        assert_eq!(first_three, [true, true, true, false, false]);
        assert!((1..=4).all(|n| at(".row:nth-child(n)", n)));
        assert!(at(".row:nth-child(3n + 2)", 5) && !at(".row:nth-child(3n + 2)", 4), "spaces");
    }

    /// An ancestor's place is kept even when its siblings are not, which is
    /// the usual case: no rule reads them.
    #[test]
    fn an_ancestors_place_is_its_own() {
        let second = AncNode { desc: el("li"), prev: Vec::<ElemDesc>::new().into(), place: 2, child_total: None, form: None };
        assert!(hits("li:nth-child(2) .x", ".x", &[second.clone()], &[]));
        assert!(!hits("li:first-child .x", ".x", &[second.clone()], &[]));
        assert!(hits("li:nth-child(2) > .x", ".x", &[second], &[]));
        // A sibling found by `+` or `~` has its own place too.
        assert!(hits(".a:first-child ~ .x", ".x", &[], &[".a", ".b"]));
        assert!(!hits(".b:first-child ~ .x", ".x", &[], &[".a", ".b"]));
        assert!(hits(".b:nth-child(2) + .x", ".x", &[], &[".a", ".b"]));
    }

    /// `:not()` matches what none of its compounds match, and counts as
    /// specific as the most specific of them.
    #[test]
    fn not_matches_what_none_of_its_list_does() {
        assert!(hits(".row:not(.done)", ".row", &[], &[]));
        assert!(!hits(".row:not(.done)", ".row.done", &[], &[]));
        assert!(!hits(".row:not(.done, .gone)", ".row.gone", &[], &[]));
        assert!(hits("view:not(button)", "view", &[], &[]));
        assert!(!hits(".row:not(:first-child)", ".row", &[], &[]));
        assert!(hits(".row:not(:first-child)", ".row", &[], &[".row"]));
        assert!(hits(".Row:not(.Done)", ".Row", &[], &[]), "case is kept");
        assert!(!hits(".Row:not(.Done)", ".Row.Done", &[], &[]));
        let spec = |sel: &str| parse_selector(sel).unwrap().2;
        assert_eq!(spec(".a:not(.b)"), (0, 2, 0));
        assert_eq!(spec(".a:not(.b, #c)"), (1, 1, 0));
        assert_eq!(spec("li:first-child"), (0, 1, 1));
        let disabled = ElemStates { disabled: true, ..ElemStates::default() };
        assert!(!hits_state("button:not(:disabled)", "button", disabled));
        assert!(hits_state("button:not(:disabled)", "button", ElemStates::default()));
    }

    #[test]
    fn lightningcss_serialization_round_trips_to_our_combinators() {
        // Guards the seam between lightningcss's selector serialization and our
        // `parse_selector`: if that serialization ever changes shape, this catches
        // it before it silently degrades matching back to descendant-only.
        use super::{parse_rules, Combinator};
        let css = ".card > text { color: #111 } .a + .b { color: #222 } .a ~ .b { color: #333 }";
        let rules = parse_rules(css, Environment::sane());
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
        let root = build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).unwrap();

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

    /// Elements with the same declarations share one interpretation, but a
    /// declaration set that warns is never shared, so every build still
    /// raises its warning: the overlay and `rux check` list what the latest
    /// build said, and a warning answered from a cache would vanish from both.
    #[test]
    fn a_shared_interpretation_does_not_swallow_a_warning() {
        let src = r#"<template><screen>
            <view class="x"></view><view class="x"></view>
          </screen></template>
          <style>.x { transition: bogus 1s }</style>"#;
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build("").unwrap();
        let _ = take_warnings();
        for build in 1..=2 {
            build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).unwrap();
            let warned = take_warnings().iter().any(|w| w.message.contains("bogus"));
            assert!(warned, "build {build} still says `bogus` cannot be animated");
        }
    }

    /// The build keeps an ancestor's siblings only for a rule that can read
    /// them, so this is what decides whether `.a ~ .b .c` still works.
    #[test]
    fn only_a_sibling_step_left_of_a_hop_reads_ancestor_siblings() {
        let reads = |css: &str| parse_rules(css, Environment::HEADLESS)[0].reads_ancestor_siblings();
        assert!(reads(".a ~ .b .c { color: #080808 }"));
        assert!(reads(".a + .b > .c { color: #080808 }"));
        assert!(reads(".a + .b .c .d { color: #080808 }"));
        assert!(!reads(".a .b ~ .c { color: #080808 }"));
        assert!(!reads(".a > .b + .c { color: #080808 }"));
        assert!(!reads(".a .b { color: #080808 }"));
        assert!(!reads(".a { color: #080808 }"));
    }

    /// End to end, through a real build: the rule that needs an ancestor's
    /// siblings still finds them, and one that does not is not changed by
    /// the build no longer keeping them.
    #[test]
    fn sibling_above_descendant_styles_the_right_element_end_to_end() {
        let src = r#"
            <template>
              <screen>
                <view class="a"></view>
                <view class="b"><text class="c">after a</text></view>
                <view class="b"><text class="c">also after a</text></view>
              </screen>
            </template>
            <style>
              .a ~ .b .c { color: #080808 }
            </style>
        "#;
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build("").unwrap();
        let root = build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).unwrap();
        for i in [1, 2] {
            let text = root.children[i].children[0].text.as_ref().unwrap();
            assert!(text.color.r < 0.1, "row {i} follows `.a`, so `.a ~ .b .c` colors it");
        }

        let src = r#"
            <template>
              <screen>
                <view class="b"><text class="c">no a before it</text></view>
                <view class="a"></view>
              </screen>
            </template>
            <style>
              .a ~ .b .c { color: #080808 }
            </style>
        "#;
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let root = build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).unwrap();
        let text = root.children[0].children[0].text.as_ref().unwrap();
        assert!(text.color.r > 0.5, "nothing `.a` precedes this `.b`");
    }

    // ── The element index ───────────────────────────────────────────────────

    /// Build a document and hand back the index a `query()` would run against.
    fn indexed(src: &str) -> super::ElementIndex {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let mut instances = super::Instances::new();
        let (_, reg) =
            super::build_styled_tree_tracked(&sfc, &HashMap::new(), &HashMap::new(), &mut engine, &mut instances, &mut crate::Swaps::new())
                .unwrap();
        reg.elements
    }

    const NESTED: &str = r#"<template><screen>
        <view class="card"><text class="title">One</text></view>
        <view class="card wide"><text class="title">Two</text></view>
    </screen></template>"#;

    /// The whole point of the index: after the build, the tree can still say
    /// what it is. `LayoutNode` keeps neither tag nor class, so without this
    /// there is nothing for a selector to match.
    #[test]
    fn a_class_selector_finds_every_matching_node() {
        let index = indexed(NESTED);
        assert_eq!(index.query(".card").unwrap().len(), 2);
        assert_eq!(index.query(".title").unwrap().len(), 2);
        // A compound narrows to the one element carrying both classes.
        assert_eq!(index.query(".card.wide").unwrap().len(), 1);
    }

    /// Tag and id come back too, and an id matches at most the one node.
    #[test]
    fn tag_and_id_selectors_work_against_the_built_tree() {
        let index = indexed(
            r#"<template><screen><view id="panel"><text>Hi</text></view></screen></template>"#,
        );
        assert_eq!(index.query("#panel").unwrap().len(), 1);
        assert_eq!(index.query("text").unwrap().len(), 1);
        assert!(index.query("#missing").unwrap().is_empty());
    }

    /// Combinators resolve, which is the part that needs ancestors and
    /// preceding siblings rebuilt from the paths rather than stored per node.
    #[test]
    fn combinators_resolve_from_paths_alone() {
        let index = indexed(NESTED);
        // Descendant and child both reach the titles through their card.
        assert_eq!(index.query(".card .title").unwrap().len(), 2);
        assert_eq!(index.query(".card > .title").unwrap().len(), 2);
        // The sibling combinator picks the second card only, since the first
        // has no preceding `.card`.
        let after = index.query(".card + .card").unwrap();
        assert_eq!(after.len(), 1);
        // …and it is genuinely the later one.
        assert!(after[0].path > index.query(".card").unwrap()[0].path);
    }

    /// The root is an ancestor like any other, which is where this diverged
    /// from the stylesheet.
    ///
    /// `query()` is billed as the stylesheet's own matcher, and a rule written
    /// `.app .row` painted while the identical selector handed to a handler
    /// came back empty. The ancestor chain was rebuilt from depth 1, and the
    /// root's path is empty, so the one node every selector is most likely to
    /// be anchored at was the one node that could never match.
    #[test]
    fn a_selector_anchored_at_the_root_matches() {
        let index = indexed(
            r#"<template><screen class="app">
                 <view class="wrap"><view class="row"><text>One</text></view></view>
               </screen></template>"#,
        );
        assert_eq!(index.query(".app").unwrap().len(), 1, "the root is in the index");
        assert_eq!(index.query(".app .row").unwrap().len(), 1, "as a descendant");
        assert_eq!(index.query(".app > .wrap").unwrap().len(), 1, "as a parent");
        assert_eq!(index.query("screen > .wrap").unwrap().len(), 1, "by tag too");
        // The root itself has no ancestors, so anchoring above it still fails.
        assert!(index.query("view > .app").unwrap().is_empty(), "nothing is above the root");
    }

    /// Results are in document order, which is what makes indexing into a query
    /// result mean something stable from one build to the next.
    #[test]
    fn results_come_back_in_document_order() {
        let index = indexed(NESTED);
        let hits: Vec<Vec<usize>> =
            index.query(".card").unwrap().into_iter().map(|m| m.path).collect();
        let mut sorted = hits.clone();
        sorted.sort();
        assert_eq!(hits, sorted);
    }

    /// A selector that does not parse is `None`, not an empty list. Silently
    /// matching nothing is the failure this project keeps closing off.
    #[test]
    fn an_unparseable_selector_is_reported_rather_than_empty() {
        let index = indexed(NESTED);
        assert!(index.query(">").is_none());
        // An ordinary selector that simply matches nothing is still `Some`.
        assert!(index.query(".nope").unwrap().is_empty());
    }

    /// A dynamically applied `:class` is in the index, so `query` and the
    /// stylesheet agree about the same document.
    #[test]
    fn dynamic_classes_are_indexed_too() {
        let index = indexed(
            r#"<template><screen><view :class="cls"><text>x</text></view></screen></template>
               <script>let cls = "open"</script>"#,
        );
        assert_eq!(index.query(".open").unwrap().len(), 1);
    }
}

#[cfg(test)]
mod white_space_tests {
    use std::collections::HashMap;

    use rux_script::Builder;

    fn texts(src: &str) -> (Vec<String>, super::BindingRegistry, rux_script::Engine) {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let mut instances = super::Instances::new();
        let (root, reg) = super::build_styled_tree_tracked(
            &sfc,
            &HashMap::new(),
            &HashMap::new(),
            &mut engine,
            &mut instances,
            &mut crate::Swaps::new(),
        )
        .unwrap();
        let texts = root.children.iter().map(|c| c.text.as_ref().unwrap().text.clone()).collect();
        (texts, reg, engine)
    }

    /// A sentence written over several indented lines reads as a sentence, the
    /// way it does in a browser. Until v0.9 the newline was a line break and
    /// the indentation after it was drawn.
    #[test]
    fn a_sentence_over_several_lines_is_one_line_of_words() {
        let (texts, ..) = texts(
            "<template><screen>\r\n  <text>\r\n    Read the\r\n    terms   first.\r\n  </text>\r\n</screen></template>",
        );
        assert_eq!(texts, ["Read the terms first."]);
    }

    /// A value's spaces collapse too: the rule is about what is drawn, not
    /// where it came from. `pre-line` is how data keeps its line breaks.
    #[test]
    fn a_value_collapses_unless_white_space_keeps_it() {
        let (texts, ..) = texts(
            r#"<template><screen>
                 <text>{{ notes }}</text>
                 <text class="keep">{{ notes }}</text>
                 <text class="pre">{{ notes }}</text>
               </screen></template>
               <style>.keep { white-space: pre-line; } .pre { white-space: pre-wrap; }</style>
               <script>let notes = signal("  one  \n  two");</script>"#,
        );
        assert_eq!(texts, ["one two", "one\ntwo", "  one  \n  two"]);
    }

    /// A value that changes later is collapsed by the same rule as the first.
    #[test]
    fn a_patched_value_keeps_the_texts_white_space() {
        let (_, reg, mut engine) = texts(
            r#"<template><screen>
                 <text>{{ notes }}</text>
                 <text class="keep">{{ notes }}</text>
               </screen></template>
               <style>.keep { white-space: pre-line; }</style>
               <script>let notes = signal("");</script>"#,
        );
        assert!(engine.run_handler(r#"notes = "a \n  b""#), "handler ran");
        let now: Vec<String> = reg.text.iter().map(|b| super::eval_text_binding(b, &mut engine)).collect();
        assert_eq!(now, ["a b", "a\nb"]);
    }

    /// `&nbsp;` is content, never a collapsible space, which is what makes it
    /// the way to ask for two spaces in a row.
    #[test]
    fn a_non_breaking_space_survives() {
        let (texts, ..) = texts("<template><screen><text> a&nbsp; b </text></screen></template>");
        assert_eq!(texts, ["a\u{a0} b"]);
    }
}

#[cfg(test)]
mod sentence_tests {
    use std::collections::HashMap;

    use rux_layout::{AccessRole, Node};
    use rux_script::Builder;

    fn built(src: &str) -> (Node, super::BindingRegistry) {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let mut instances = super::Instances::new();
        super::build_styled_tree_tracked(
            &sfc,
            &HashMap::new(),
            &HashMap::new(),
            &mut engine,
            &mut instances,
            &mut crate::Swaps::new(),
        )
        .unwrap()
    }

    fn text(n: &Node) -> &rux_layout::TextContent {
        n.text.as_ref().expect("a text")
    }

    /// The pieces of a sentence are its children, in order, each with its own
    /// words kept as written for the layout to join.
    #[test]
    fn a_sentence_is_built_as_its_pieces() {
        let (root, _) = built(
            r#"<template><screen>
                 <text>Read the <text class="b">terms</text> now</text>
               </screen></template>
               <style>.b { font-weight: 700; }</style>"#,
        );
        let s = &root.children[0];
        assert_eq!(text(s).text, "");
        let pieces: Vec<(&str, u16)> = s.children.iter().map(|c| (text(c).text.as_str(), text(c).weight)).collect();
        assert_eq!(pieces, [("Read the ", 400), ("terms", 700), (" now", 400)]);
    }

    /// A run takes the sentence's style where it says nothing: a word inside a
    /// bold, italic, underlined sentence is all three.
    #[test]
    fn a_run_takes_the_sentences_style() {
        let (root, _) = built(
            r#"<template><screen>
                 <text class="s">all <text class="red">of</text> this</text>
               </screen></template>
               <style>
                 .s { font-weight: 700; font-style: italic; text-decoration: underline; letter-spacing: 2px; }
                 .red { color: #ff0000; }
               </style>"#,
        );
        for piece in &root.children[0].children {
            let t = text(piece);
            assert_eq!((t.weight, t.italic, t.underline, t.letter_spacing), (700, true, true, Some(2.0)), "{:?}", t.text);
        }
        assert!(text(&root.children[0].children[1]).color.r > 0.9, "and keeps its own colour");
    }

    /// The words between tags are the sentence's own, so no rule reaches them,
    /// not even `*`: CSS never matches an anonymous box.
    #[test]
    fn no_rule_reaches_the_words_between_tags() {
        let (root, _) = built(
            r#"<template><screen>
                 <text class="s">plain <text>nested</text> plain</text>
               </screen></template>
               <style>* { color: #ff0000; } .s { color: #0000ff; }</style>"#,
        );
        let s = &root.children[0];
        assert!(text(&s.children[0]).color.b > 0.9, "the sentence's blue, not `*`'s red");
        assert!(text(&s.children[1]).color.r > 0.9, "a real <text> is matched by `*`");
    }

    /// A value in a run is bound to that run, and kept uncollapsed: the
    /// layout collapses it with the words beside it.
    #[test]
    fn a_value_in_a_run_is_bound_to_the_run() {
        let (root, reg) = built(
            r#"<template><screen>
                 <text>Taps: <text>{{ n }}</text> so far</text>
               </screen></template>
               <script>let n = signal(3);</script>"#,
        );
        assert_eq!(text(&root.children[0].children[1]).text, "3");
        let binding = reg.text.iter().find(|b| b.template.contains("{{ n }}")).expect("a binding");
        assert_eq!(binding.path, [0, 1]);
        assert_eq!(binding.white_space, None);
    }

    /// A sentence is read out as all its words; a run of plain words is part
    /// of it and nothing of its own; a run with `@tap` is a button in it.
    #[test]
    fn a_screen_reader_hears_the_sentence() {
        let (root, _) = built(
            r#"<template><screen>
                 <text>Read   the <text @tap="n++">terms</text> now</text>
               </screen></template>
               <script>let n = signal(0);</script>"#,
        );
        let s = &root.children[0];
        assert_eq!(s.access.role, AccessRole::Label);
        assert_eq!(s.access.label.as_deref(), Some("Read the terms now"));
        assert_eq!(s.children[0].access.role, AccessRole::None);
        assert_eq!(s.children[1].access.role, AccessRole::Button);
    }

    /// `<image>` and `<path>` are boxes in the line; a `<view>` in a sentence
    /// is an error that says what to do instead.
    #[test]
    fn only_words_and_boxes_go_in_a_sentence() {
        let _ = super::take_warnings();
        let (root, _) = built(
            r#"<template><screen>
                 <text>a <path d="M0 0 H8 V8 H0 Z" /> b <view></view></text>
               </screen></template>"#,
        );
        assert!(root.children[0].children[1].path.is_some());
        let said: Vec<String> = super::take_warnings().into_iter().map(|w| w.message).collect();
        assert!(said.iter().any(|m| m.contains("`<view>` cannot be inside a `<text>`")), "{said:?}");
    }

    /// A plain `<text>` is untouched: no children, its words collapsed.
    #[test]
    fn plain_text_is_still_plain() {
        let (root, _) = built("<template><screen><text>  just   words  </text></screen></template>");
        assert!(root.children[0].children.is_empty());
        assert_eq!(text(&root.children[0]).text, "just words");
    }
}

#[cfg(test)]
mod direction_tests {
    use std::collections::HashMap;

    use rux_layout::{Direction, Node, TextAlign};
    use rux_script::Builder;

    fn built(src: &str) -> Node {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        super::build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).unwrap()
    }

    /// `dir` reaches everything inside, the layout and the text alike.
    #[test]
    fn dir_is_inherited() {
        let root = built(r#"<template><screen><view dir="rtl"><view><text>a</text></view></view></screen></template>"#);
        let inner = &root.children[0].children[0];
        assert_eq!(inner.style.direction, Direction::Rtl);
        assert_eq!(inner.children[0].text.as_ref().unwrap().direction, Some(Direction::Rtl));
        assert_eq!(root.style.direction, Direction::Ltr, "and nothing outside");
        assert_eq!(root.children[0].text.as_ref().map(|_| ()), None);
    }

    /// A `direction` rule wins over the attribute, as an author's rule wins over
    /// a presentational hint in CSS.
    #[test]
    fn a_rule_beats_the_attribute() {
        let root = built(
            r#"<template><screen><text class="x" dir="rtl">a</text></screen></template>
               <style>.x { direction: ltr; }</style>"#,
        );
        assert_eq!(root.children[0].style.direction, Direction::Ltr);
    }

    /// `dir="auto"` leaves a text to its first letter.
    #[test]
    fn auto_leaves_the_text_to_its_letters() {
        let root = built(r#"<template><screen dir="rtl"><text dir="auto">a</text><text>b</text></screen></template>"#);
        assert_eq!(root.children[0].text.as_ref().unwrap().direction, None);
        assert_eq!(root.children[1].text.as_ref().unwrap().direction, Some(Direction::Rtl));
    }

    /// Between a logical and a physical declaration the later one wins, on the
    /// side the direction puts the logical one.
    #[test]
    fn logical_and_physical_resolve_in_order() {
        let src = |dir: &str| {
            format!(
                r#"<template><screen><view class="a" dir="{dir}"></view></screen></template>
                   <style>.a {{ margin-inline-start: 4px; margin-left: 8px; padding-inline: 1px 2px; }}</style>"#
            )
        };
        let ltr = built(&src("ltr"));
        let m = ltr.children[0].style.margin;
        assert_eq!((m.left, m.right), (8.0, 0.0), "ltr: the later margin-left wins the start side");
        let p = ltr.children[0].style.padding;
        assert_eq!((p.left, p.right), (1.0, 2.0));
        let rtl = built(&src("rtl"));
        let m = rtl.children[0].style.margin;
        assert_eq!((m.left, m.right), (8.0, 4.0), "rtl: the start is the right, untouched by margin-left");
        let p = rtl.children[0].style.padding;
        assert_eq!((p.left, p.right), (2.0, 1.0));
    }

    /// An inline `style` is part of the order too.
    #[test]
    fn an_inline_logical_declaration_comes_last() {
        let root = built(
            r#"<template><screen><view class="a" dir="rtl" style="margin-inline-end: 3px"></view></screen></template>
               <style>.a { margin-left: 9px; }</style>"#,
        );
        assert_eq!(root.children[0].style.margin.left, 3.0);
    }

    /// `left` and `right` are sides; `start` and `end` follow the direction.
    #[test]
    fn text_align_sides_and_ends() {
        let root = built(
            r#"<template><screen>
                 <text style="text-align: left">a</text>
                 <text style="text-align: right">b</text>
                 <text style="text-align: start">c</text>
                 <text style="text-align: end">d</text>
               </screen></template>"#,
        );
        let aligns: Vec<TextAlign> = root.children.iter().map(|c| c.text.as_ref().unwrap().align).collect();
        assert_eq!(aligns, [TextAlign::Left, TextAlign::Right, TextAlign::Start, TextAlign::End]);
    }
}

#[cfg(test)]
mod inheritance_tests {
    use std::collections::HashMap;

    use rux_layout::{Node, TextAlign, TextContent};
    use rux_script::Builder;

    fn built(src: &str) -> Node {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        super::build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).unwrap()
    }

    fn text(n: &Node) -> &TextContent {
        n.text.as_ref().expect("a text")
    }

    /// A box's text style reaches every text inside it, as in CSS.
    #[test]
    fn a_boxs_text_style_reaches_its_texts() {
        let root = built(
            r#"<template><screen><view class="card"><view><text>a</text></view></view></screen></template>
               <style>
                 .card { font-weight: 700; font-style: italic; letter-spacing: 1px; word-spacing: 3px;
                         text-align: center; white-space: pre-line; }
               </style>"#,
        );
        let t = text(&root.children[0].children[0].children[0]);
        assert_eq!((t.weight, t.italic, t.letter_spacing, t.word_spacing), (700, true, Some(1.0), Some(3.0)));
        assert_eq!(t.align, TextAlign::Center);
        assert_eq!(t.white_space, super::WhiteSpace::PreLine);
    }

    /// `visibility` inherits, and a child can be `visible` inside a hidden box.
    #[test]
    fn visibility_inherits_and_a_child_can_undo_it() {
        let root = built(
            r#"<template><screen><view class="p"><view></view><view class="c"></view></view></screen></template>
               <style>.p { visibility: hidden; } .c { visibility: visible; }</style>"#,
        );
        let p = &root.children[0];
        assert!(p.style.invisible && p.children[0].style.invisible);
        assert!(!p.children[1].style.invisible);
        assert!(!root.style.invisible);
    }

    /// A text's own rule wins, and `normal` takes it back.
    #[test]
    fn its_own_rule_wins() {
        let root = built(
            r#"<template><screen class="s"><text>a</text><text class="plain">b</text></screen></template>
               <style>.s { font-weight: 700; letter-spacing: 2px; } .plain { font-weight: 400; letter-spacing: normal; }</style>"#,
        );
        assert_eq!(text(&root.children[0]).weight, 700);
        assert_eq!((text(&root.children[1]).weight, text(&root.children[1]).letter_spacing), (400, None));
    }

    /// A bare `line-height` is inherited as the number and multiplied by each
    /// text's own size; a length is inherited as its pixels.
    #[test]
    fn line_height_inherits_as_css_says() {
        let root = built(
            r#"<template><screen>
                 <view class="factor"><text class="big">a</text></view>
                 <view class="px"><text class="big">b</text></view>
               </screen></template>
               <style>
                 .factor { line-height: 1.5; font-size: 10px; }
                 .px { line-height: 15px; font-size: 10px; }
                 .big { font-size: 20px; }
               </style>"#,
        );
        assert_eq!(text(&root.children[0].children[0]).line_height, Some(30.0), "1.5 of its own 20px");
        assert_eq!(text(&root.children[1].children[0]).line_height, Some(15.0), "the pixels it came to");
    }

    /// A field takes the weight around it.
    #[test]
    fn a_field_takes_the_weight_around_it() {
        let root = built(
            r#"<template><screen class="s"><input r-model="v" /></screen></template>
               <style>.s { font-weight: 600; }</style>
               <script>let v = signal("x");</script>"#,
        );
        assert_eq!(text(&root.children[0].children[0]).weight, 600);
    }
}

#[cfg(test)]
mod run_state_tests {
    use std::collections::HashMap;

    use rux_layout::{Isolate, Node};
    use rux_script::Builder;

    fn built(src: &str, hovered: Option<Vec<usize>>) -> Node {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let state = super::InteractionState { hovered, ..Default::default() };
        super::build_styled_tree_stateful(
            &sfc,
            &HashMap::new(),
            &HashMap::new(),
            &mut engine,
            &mut super::Instances::new(),
            &mut crate::Swaps::new(),
            &state,
            super::Environment::default(),
        )
        .unwrap()
        .0
    }

    const LINK: &str = r#"<template><screen><text>Read the <text class="link" @tap="n++">terms</text></text></screen></template>
        <style>.link { color: #0000ff; } .link:hover { color: #ff0000; }</style>
        <script>let n = signal(0);</script>"#;

    /// A run a `:hover` rule could style carries the path the shell's pointer
    /// regions report, and is styled by it when the pointer is over it.
    #[test]
    fn a_run_is_matched_by_hover() {
        let resting = built(LINK, None);
        let run = &resting.children[0].children[1];
        assert_eq!(run.state_path.as_deref(), Some(&[0, 1][..]), "a region for the pointer to report");
        assert!(run.text.as_ref().unwrap().color.b > 0.9, "blue at rest");
        let hovered = built(LINK, Some(vec![0, 1]));
        assert!(hovered.children[0].children[1].text.as_ref().unwrap().color.r > 0.9, "red under the pointer");
    }

    /// A run with a `dir` of its own is set apart from its sentence; one
    /// without is not, whatever direction it inherits.
    #[test]
    fn a_run_with_its_own_dir_is_isolated() {
        let root = built(
            r#"<template><screen dir="rtl"><text>a <text dir="ltr">b</text> <text dir="auto">c</text> <text>d</text></text></screen></template>"#,
            None,
        );
        let isolates: Vec<Isolate> = root.children[0]
            .children
            .iter()
            .filter(|c| c.text.is_some())
            .map(|c| c.text.as_ref().unwrap().isolate)
            .collect();
        // a, b, space, c, space, d.
        assert_eq!(
            isolates,
            [Isolate::None, Isolate::Ltr, Isolate::None, Isolate::Auto, Isolate::None, Isolate::None]
        );
    }
}

#[cfg(test)]
mod overflow_tests {
    use std::collections::HashMap;

    use rux_layout::{Overflow, Style};

    fn axes(decls: &[(&str, &str)]) -> (Overflow, Option<Overflow>, Option<Overflow>) {
        let p: HashMap<String, String> = decls.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let mut st = Style::default();
        super::overflow_axes(&p, &mut st);
        (st.overflow, st.overflow_x, st.overflow_y)
    }

    #[test]
    fn each_axis_on_its_own() {
        use Overflow::*;
        assert_eq!(axes(&[("overflow", "auto")]), (Scroll, None, None));
        assert_eq!(axes(&[("overflow", "hidden")]), (Clip, None, None));
        // One scrolls, the other clips: a scroller that does not scroll sideways.
        assert_eq!(axes(&[("overflow-x", "hidden"), ("overflow-y", "auto")]), (Scroll, Some(Clip), None));
        assert_eq!(axes(&[("overflow", "hidden auto")]), (Scroll, Some(Clip), None));
        // A longhand narrows the shorthand.
        assert_eq!(axes(&[("overflow", "auto"), ("overflow-x", "hidden")]), (Scroll, Some(Clip), None));
        // `visible` beside `hidden`/`auto` is `auto`, as CSS computes it.
        assert_eq!(axes(&[("overflow-y", "auto")]), (Scroll, None, None));
        assert_eq!(axes(&[("overflow-y", "hidden")]), (Scroll, None, Some(Clip)));
        // Beside `clip` it stays `visible`.
        assert_eq!(axes(&[("overflow-y", "clip")]), (Clip, Some(Visible), None));
        assert!(Style { overflow: Scroll, overflow_x: Some(Clip), ..Default::default() }.scrolls_y());
    }
}

#[cfg(test)]
mod color_tests {
    use std::collections::HashMap;

    use rux_layout::{Node, Rgba};
    use rux_script::Builder;

    use super::{ColorScheme, Environment, InteractionState, Instances, Swaps};

    fn built_in(src: &str, scheme: ColorScheme) -> Node {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let env = Environment { color_scheme: scheme, ..Environment::sane() };
        super::build_styled_tree_stateful(
            &sfc,
            &HashMap::new(),
            &HashMap::new(),
            &mut engine,
            &mut Instances::new(),
            &mut Swaps::new(),
            &InteractionState::default(),
            env,
        )
        .unwrap()
        .0
    }

    fn bytes(c: Option<Rgba>) -> [u8; 4] {
        let c = c.expect("a colour");
        let b = |v: f32| (v * 255.0).round() as u8;
        [b(c.r), b(c.g), b(c.b), b(c.a)]
    }

    /// A colour function inside a shorthand that is split on spaces, and one
    /// that only exists once a variable is put in.
    #[test]
    fn a_colour_function_reaches_every_property() {
        let root = built_in(
            r#"<template><screen><view class="a"></view><view class="b"></view></screen></template>
               <style>
                 .a { border: 2px solid oklch(62.8% 0.2577 29.23); }
                 .b { --h: 120; border-color: hsl(var(--h) 50% 50%); }
               </style>"#,
            ColorScheme::Light,
        );
        assert_eq!(bytes(root.children[0].style.border_color), [255, 0, 0, 255]);
        assert_eq!(root.children[0].style.border.top, 2.0);
        assert_eq!(bytes(root.children[1].style.border_color), [64, 191, 64, 255]);
    }

    /// `currentColor` is the element's own `color`, and on `color` itself the
    /// inherited one.
    #[test]
    fn current_color_is_the_elements_color() {
        let root = built_in(
            r#"<template><screen class="s"><view class="v"></view><view class="w"></view></screen></template>
               <style>
                 .s { color: #0000ff; }
                 .v { color: #ff0000; border: 1px solid currentColor; }
                 .w { border-color: color-mix(in srgb, currentcolor 50%, transparent); }
               </style>"#,
            ColorScheme::Light,
        );
        assert_eq!(bytes(root.children[0].style.border_color), [255, 0, 0, 255]);
        assert_eq!(bytes(root.children[1].style.border_color), [0, 0, 255, 128]);
    }

    /// Every shadow in the list is kept, and one with no colour is drawn in
    /// `currentColor`, not black.
    #[test]
    fn a_shadow_list_keeps_every_shadow() {
        let root = built_in(
            r#"<template><screen><view class="v"></view></screen></template>
               <style>.v { color: #ff0000; box-shadow: 0 2px 4px, inset 1px 1px 0 3px oklch(45.2% .313 264.05); }</style>"#,
            ColorScheme::Light,
        );
        let shadows = root.children[0].style.box_shadow.clone().expect("shadows");
        assert_eq!(shadows.len(), 2);
        assert_eq!(bytes(Some(shadows[0].color)), [255, 0, 0, 255]);
        assert!(!shadows[0].inset && shadows[0].dy == 2.0 && shadows[0].blur == 4.0);
        assert_eq!(bytes(Some(shadows[1].color)), [0, 0, 255, 255]);
        assert!(shadows[1].inset && shadows[1].spread == 3.0);
    }

    /// `light-dark()` follows `color-scheme`, which inherits, and with none
    /// it is light whatever the system says, as in CSS.
    #[test]
    fn light_dark_follows_the_color_scheme() {
        let src = r#"<template><screen class="s"><view class="v"></view></screen></template>
               <style>
                 .s { color-scheme: light dark; }
                 .v { border-color: light-dark(#ffffff, #000000); }
               </style>"#;
        let light = built_in(src, ColorScheme::Light);
        let dark = built_in(src, ColorScheme::Dark);
        assert_eq!(bytes(light.children[0].style.border_color), [255, 255, 255, 255]);
        assert_eq!(bytes(dark.children[0].style.border_color), [0, 0, 0, 255]);
        assert!(super::reads_scheme());

        let unset = built_in(
            r#"<template><screen><view class="v"></view></screen></template>
               <style>.v { border-color: light-dark(#ffffff, #000000); }</style>"#,
            ColorScheme::Dark,
        );
        assert_eq!(bytes(unset.children[0].style.border_color), [255, 255, 255, 255]);
        let only_dark = built_in(
            r#"<template><screen><view class="v"></view></screen></template>
               <style>.v { color-scheme: dark; border-color: light-dark(#ffffff, #000000); }</style>"#,
            ColorScheme::Light,
        );
        assert_eq!(bytes(only_dark.children[0].style.border_color), [0, 0, 0, 255]);
    }
}

#[cfg(test)]
mod math_tests {
    use std::collections::HashMap;

    use rux_layout::Node;
    use rux_script::Builder;

    use super::{Environment, InteractionState, Instances, Swaps, Viewport};

    fn built(src: &str) -> Node {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let env = Environment { viewport: Viewport { width: 1000.0, height: 600.0 }, ..Environment::sane() };
        super::build_styled_tree_stateful(
            &sfc,
            &HashMap::new(),
            &HashMap::new(),
            &mut engine,
            &mut Instances::new(),
            &mut Swaps::new(),
            &InteractionState::default(),
            env,
        )
        .unwrap()
        .0
    }

    /// Maths with nothing left to know is worked out in the cascade, in any
    /// property, through a variable or not.
    #[test]
    fn maths_the_cascade_can_finish_is_finished_there() {
        let root = built(
            r#"<template><screen class="s"><view class="v"><text class="t">a</text></view></screen></template>
               <style>
                 .s { font-size: 20px; }
                 .v { --o: 1; padding: calc(1rem + 2px) min(4vw, 20px); gap: calc(4px * 2);
                      opacity: calc(var(--o) / 2); font-size: calc(50% + 2px); }
                 .t { font-size: clamp(1rem, 2.5vw, 2rem); }
               </style>"#,
        );
        let v = &root.children[0];
        assert_eq!((v.style.padding.top, v.style.padding.right), (18.0, 20.0));
        assert_eq!(v.style.gap, 8.0);
        assert_eq!(v.style.opacity, 0.5);
        // 2.5vw of a 1000px window, between 16px and 32px.
        assert_eq!(v.children[0].text.as_ref().unwrap().font_size, 25.0);
    }

    /// A percentage in `font-size`, maths or not, is of the inherited size.
    #[test]
    fn a_percentage_in_font_size_maths_is_of_the_parents() {
        let root = built(
            r#"<template><screen class="s"><text class="t">a</text></screen></template>
               <style>.s { font-size: 20px; } .t { font-size: calc(50% + 2px); }</style>"#,
        );
        assert_eq!(root.children[0].text.as_ref().unwrap().font_size, 12.0);
    }

    /// Viewport units in a size's maths are worked out in the cascade, which
    /// then cascades again on a resize; a percentage is left, and said.
    #[test]
    fn a_size_with_viewport_maths_is_finished_too() {
        let root = built(
            r#"<template><screen><view class="v"></view></screen></template>
               <style>.v { width: clamp(100px, 20vw, 300px); height: calc(10vh + 4px); }</style>"#,
        );
        let st = &root.children[0].style;
        assert_eq!((st.width, st.height), (Some(rux_layout::Len::Px(200.0)), Some(rux_layout::Len::Px(64.0))));
        assert!(super::reads_viewport());
    }
}

#[cfg(test)]
mod clip_tests {
    use rux_layout::clip::{ClipShape, Radius, RefBox};
    use rux_layout::Len;

    use super::parse_clip_path as p;

    #[test]
    fn every_shape_parses_as_lightningcss_prints_it() {
        let c = p("circle(40px at 0 0) padding-box").unwrap();
        assert_eq!(c.reference, RefBox::Padding);
        assert_eq!(c.shape, Some(ClipShape::Circle { r: Radius::Len(Len::Px(40.0)), at: (Len::Px(0.0), Len::Px(0.0)) }));
        let c = p("circle()").unwrap();
        assert_eq!(c.shape, Some(ClipShape::Circle { r: Radius::ClosestSide, at: (Len::Pct(0.5), Len::Pct(0.5)) }));
        let c = p("ellipse(50% 30% at right top)").unwrap();
        assert_eq!(
            c.shape,
            Some(ClipShape::Ellipse {
                rx: Radius::Len(Len::Pct(0.5)),
                ry: Radius::Len(Len::Pct(0.3)),
                at: (Len::Pct(1.0), Len::Pct(0.0))
            })
        );
        let c = p("inset(10px 20px round 8px)").unwrap();
        assert_eq!(
            c.shape,
            Some(ClipShape::Inset { sides: [Len::Px(10.0), Len::Px(20.0), Len::Px(10.0), Len::Px(20.0)], round: [Len::Px(8.0); 4] })
        );
        let Some(ClipShape::Polygon { evenodd, points }) = p("polygon(evenodd, 0 0, 10px 0, 10px 10px)").unwrap().shape
        else {
            panic!("not a polygon")
        };
        assert!(evenodd && points.len() == 3);
        let Some(ClipShape::Path { evenodd, commands }) = p(r#"path(evenodd, "M0 0 h10 v10 Z")"#).unwrap().shape else {
            panic!("not a path")
        };
        assert!(evenodd && !commands.is_empty());
        assert_eq!(p("content-box").unwrap().shape, None);
        assert!(p("none").is_none());
        // xywh() and rect() are insets whose far edges are what is left.
        let Some(ClipShape::Inset { sides, .. }) = p("xywh(10px 20px 50% 30px)").unwrap().shape else {
            panic!("not an inset")
        };
        assert_eq!((sides[0], sides[3]), (Len::Px(20.0), Len::Px(10.0)));
        assert_eq!(sides[1].px(200.0, (0.0, 0.0)), 200.0 - 10.0 - 100.0);
    }

    #[test]
    fn what_rux_cannot_clip_to_is_said() {
        let _ = super::take_warnings();
        assert!(p(r##"url("#x")"##).is_none());
        assert!(super::take_warnings().iter().any(|w| w.message.contains("SVG `<clipPath>`")));
    }
}

#[cfg(test)]
mod mask_tests {
    use std::collections::HashMap;

    use rux_layout::Background;

    fn mask(pairs: &[(&str, &str)]) -> Option<rux_layout::Mask> {
        let p: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        super::interpret_mask(&p)
    }

    #[test]
    fn a_mask_is_an_image_and_a_mode() {
        let m = mask(&[("mask", "linear-gradient(to right, #000, #0000)")]).unwrap();
        assert!(matches!(m.image, Background::Gradient(_)) && !m.luminance);
        let m = mask(&[("mask-image", "url(a.png)"), ("mask-mode", "luminance")]).unwrap();
        assert!(matches!(m.image, Background::Image(ref s) if s == "a.png") && m.luminance);
        // The longhand wins over the shorthand, and `none` is no mask.
        assert!(mask(&[("mask", "url(a.png)"), ("mask-image", "none")]).is_none());
        // A colour is not an image.
        assert!(mask(&[("mask-image", "red")]).is_none());
    }
}

#[cfg(test)]
mod filter_tests {
    use rux_layout::filter::FilterOp;
    use rux_layout::Rgba;

    use super::parse_filter as p;

    #[test]
    fn filters_parse_in_order_with_colour_runs_folded() {
        let ops = p("filter", "blur(4px) brightness(1.2) contrast(50%) drop-shadow(2px 3px 4px #0008)");
        assert_eq!(ops.len(), 3);
        assert!(matches!(ops[0], FilterOp::Blur(b) if b == 4.0));
        assert!(matches!(ops[1], FilterOp::Matrix(_)));
        let FilterOp::DropShadow { dx, dy, blur, color } = ops[2].clone() else { panic!("{ops:?}") };
        assert_eq!((dx, dy, blur), (2.0, 3.0, 4.0));
        assert!((color.a - 136.0 / 255.0).abs() < 1e-6);
        // Left out, each function takes CSS's default: grayscale() is all of it.
        let FilterOp::Matrix(m) = p("filter", "grayscale()")[0].clone() else { panic!() };
        let g = FilterOp::apply(&m, Rgba::new(1.0, 0.0, 0.0, 1.0));
        assert!((g.r - g.b).abs() < 1e-6);
        assert!(p("filter", "none").is_empty());
    }

    #[test]
    fn a_bad_function_drops_the_whole_filter_and_says_so() {
        let _ = super::take_warnings();
        assert!(p("filter", r##"blur(2px) url("#f")"##).is_empty());
        assert!(super::take_warnings().iter().any(|w| w.message.contains("SVG `<filter>`")));
        assert!(p("backdrop-filter", "brightness(-1)").is_empty());
    }

    #[test]
    fn a_drop_shadow_with_no_colour_is_current_color() {
        let out = super::drop_shadow_colors("blur(1px) drop-shadow(2px 2px)", Rgba::new(1.0, 0.0, 0.0, 1.0)).unwrap();
        assert_eq!(out, "blur(1px) drop-shadow(2px 2px #ff0000ff)");
        assert!(super::drop_shadow_colors("drop-shadow(2px 2px red)", Rgba::new(1.0, 0.0, 0.0, 1.0)).is_none());
    }
}

#[cfg(test)]
mod text_shadow_tests {
    use std::collections::HashMap;

    use rux_script::Builder;

    #[test]
    fn a_text_shadow_list_parses_and_inherits() {
        let list = super::parse_text_shadows("1px 2px 3px red, 0 0 4px").unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!((list[0].dx, list[0].dy, list[0].blur), (1.0, 2.0, 3.0));
        assert!(list[0].color.is_some() && list[1].color.is_none(), "no colour is currentColor");
        assert!(super::parse_text_shadows("none").is_none());

        let sfc = rux_parser::parse_sfc(
            r#"<template><screen class="s"><text>a</text><text class="plain">b</text></screen></template>
               <style>.s { text-shadow: 2px 2px #000; } .plain { text-shadow: none; }</style>"#,
        )
        .unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let root = super::build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).unwrap();
        assert!(root.children[0].text.as_ref().unwrap().shadows.is_some(), "inherited");
        assert!(root.children[1].text.as_ref().unwrap().shadows.is_none(), "and taken back");
    }
}

#[cfg(test)]
mod border_style_tests {
    use std::collections::HashMap;

    use rux_layout::BorderStyle::{self, *};

    fn border(pairs: &[(&str, &str)]) -> ([f32; 4], [BorderStyle; 4]) {
        let p: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let st = super::interpret(&p);
        ([st.border.top, st.border.right, st.border.bottom, st.border.left], st.border_style)
    }

    #[test]
    fn a_border_needs_a_style_as_in_css() {
        let _ = super::take_warnings();
        assert_eq!(border(&[("border", "2px #ff0000")]), ([0.0; 4], [None; 4]));
        assert!(super::take_warnings().iter().any(|w| w.message.contains("`border: 2px #ff0000` draws no border")));
        // A style with no width is `medium`, 3px.
        assert_eq!(border(&[("border", "dashed")]), ([3.0; 4], [Dashed; 4]));
        assert_eq!(border(&[("border", "thin solid red")]).0, [1.0; 4]);
        // `border: none` says nothing.
        let _ = super::take_warnings();
        assert_eq!(border(&[("border", "none")]).0, [0.0; 4]);
        assert!(super::take_warnings().is_empty());
    }

    #[test]
    fn each_side_has_its_own_style() {
        let (w, s) = border(&[("border-bottom", "1px dotted")]);
        assert_eq!((w, s), ([0.0, 0.0, 1.0, 0.0], [None, None, Dotted, None]));
        let (w, s) = border(&[("border-width", "2px"), ("border-style", "solid double")]);
        assert_eq!((w, s), ([2.0; 4], [Solid, Double, Solid, Double]));
        let (_, s) = border(&[("border", "4px groove red"), ("border-left-style", "inset")]);
        assert_eq!(s, [Groove, Groove, Groove, Inset]);
        // `hidden` is no border, as `none` is.
        assert_eq!(border(&[("border", "2px solid red"), ("border-top-style", "hidden")]).0[0], 0.0);
    }
}

#[cfg(test)]
mod background_tests {
    use std::collections::HashMap;

    use rux_layout::{Background, BgAttachment, BgBox, BgLayers, BgLen, BgPos, BgRepeat, BgSize};

    fn bg(pairs: &[(&str, &str)]) -> Option<Background> {
        let mut p = HashMap::new();
        for (k, v) in pairs {
            super::declare(&mut p, k, v);
        }
        super::interpret_background(&p)
    }

    fn layers(pairs: &[(&str, &str)]) -> BgLayers {
        match bg(pairs) {
            Some(Background::Layers(l)) => (*l).clone(),
            other => panic!("expected layers, got {other:?}"),
        }
    }

    fn pos(pct: f32, px: f32) -> BgPos {
        BgPos { pct, px }
    }

    #[test]
    fn a_colour_alone_stays_a_colour() {
        assert!(matches!(bg(&[("background", "#123456")]), Some(Background::Color(_))));
        assert!(matches!(bg(&[("background-color", "red")]), Some(Background::Color(_))));
        assert!(bg(&[("background", "none")]).is_none());
        assert!(bg(&[]).is_none());
    }

    #[test]
    fn an_image_is_tiled_at_its_own_size_by_default() {
        let l = layers(&[("background", "url(sky.png)")]);
        let one = &l.layers[0];
        assert!(matches!(&one.image, Background::Image(s) if s == "sky.png"));
        assert_eq!(one.size, BgSize::Sized([BgLen::Auto, BgLen::Auto]));
        assert_eq!(one.repeat, [BgRepeat::Repeat; 2]);
        assert_eq!(one.position, [pos(0.0, 0.0); 2]);
        assert_eq!((one.origin, one.clip, one.attachment), (BgBox::Padding, BgBox::Border, BgAttachment::Scroll));
        assert!(l.color.is_none());
    }

    #[test]
    fn the_shorthand_takes_every_part() {
        let l = layers(&[("background", "#fff url(a.png) right 10px bottom 20px / 50% auto no-repeat fixed content-box padding-box")]);
        let one = &l.layers[0];
        assert!(l.color.is_some());
        assert_eq!(one.position, [pos(100.0, -10.0), pos(100.0, -20.0)]);
        assert_eq!(one.size, BgSize::Sized([BgLen::Pct(50.0), BgLen::Auto]));
        assert_eq!(one.repeat, [BgRepeat::NoRepeat; 2]);
        assert_eq!(one.attachment, BgAttachment::Fixed);
        assert_eq!((one.origin, one.clip), (BgBox::Content, BgBox::Padding));
        // lightningcss writes the slash without spaces.
        let l = layers(&[("background", "url(assets/a.png) 50%/cover")]);
        assert_eq!(l.layers[0].size, BgSize::Cover);
        // ...and the slash in a path is the path's.
        assert!(matches!(&l.layers[0].image, Background::Image(s) if s == "assets/a.png"));
        assert_eq!(l.layers[0].position, [pos(50.0, 0.0), pos(50.0, 0.0)]);
    }

    #[test]
    fn longhands_repeat_their_lists_to_the_number_of_images() {
        let l = layers(&[
            ("background-image", "linear-gradient(red, blue), url(a.png), none"),
            ("background-size", "10px 20px, contain"),
            ("background-repeat", "repeat-x"),
            ("background-color", "red"),
        ]);
        // `none` keeps its place in the lists but draws nothing.
        assert_eq!(l.layers.len(), 2);
        assert_eq!(l.layers[0].size, BgSize::Sized([BgLen::Px(10.0), BgLen::Px(20.0)]));
        assert_eq!(l.layers[1].size, BgSize::Contain);
        assert_eq!(l.layers[1].repeat, [BgRepeat::Repeat, BgRepeat::NoRepeat]);
        assert_eq!(l.color.map(|c| c.r), Some(1.0));
    }

    #[test]
    fn a_shorthand_written_after_a_longhand_resets_it() {
        let l = layers(&[("background-size", "cover"), ("background", "url(a.png)")]);
        assert_eq!(l.layers[0].size, BgSize::Sized([BgLen::Auto, BgLen::Auto]));
        let l = layers(&[("background", "url(a.png)"), ("background-size", "cover")]);
        assert_eq!(l.layers[0].size, BgSize::Cover);
        // A colour and an image together, which the shorthand used to keep
        // apart: `background-image` replaced the colour.
        let l = layers(&[("background", "red"), ("background-image", "url(a.png)")]);
        assert!(l.color.is_some());
    }

    #[test]
    fn positions_read_as_css_reads_them() {
        let at = |v: &str| layers(&[("background", "url(a.png)"), ("background-position", v)]).layers[0].position;
        assert_eq!(at("top"), [pos(50.0, 0.0), pos(0.0, 0.0)]);
        assert_eq!(at("right"), [pos(100.0, 0.0), pos(50.0, 0.0)]);
        assert_eq!(at("bottom left"), [pos(0.0, 0.0), pos(100.0, 0.0)]);
        assert_eq!(at("10px 25%"), [pos(0.0, 10.0), pos(25.0, 0.0)]);
        assert_eq!(at("left 5px top"), [pos(0.0, 5.0), pos(0.0, 0.0)]);
        let x = |v: &str| layers(&[("background", "url(a.png)"), ("background-position-x", v)]).layers[0].position;
        assert_eq!(x("right 8px"), [pos(100.0, -8.0), pos(0.0, 0.0)]);
        let y = |v: &str| layers(&[("background", "url(a.png)"), ("background-position-y", v)]).layers[0].position;
        assert_eq!(y("bottom"), [pos(0.0, 0.0), pos(100.0, 0.0)]);
    }

    #[test]
    fn repeat_takes_one_or_two_keywords() {
        let r = |v: &str| layers(&[("background", "url(a.png)"), ("background-repeat", v)]).layers[0].repeat;
        assert_eq!(r("repeat-y"), [BgRepeat::NoRepeat, BgRepeat::Repeat]);
        assert_eq!(r("space round"), [BgRepeat::Space, BgRepeat::Round]);
    }
}

#[cfg(test)]
mod text_look_tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use rux_layout::{DecorationStyle, TextLook, TextTransform};

    fn look(pairs: &[(&str, &str)], parent: Option<&Arc<TextLook>>, in_sentence: bool) -> Option<Arc<TextLook>> {
        let p: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        super::text_look(&p, parent, in_sentence)
    }

    #[test]
    fn nothing_set_is_no_look() {
        assert!(look(&[("color", "red")], None, false).is_none());
    }

    #[test]
    fn the_decoration_shorthand_takes_style_colour_and_thickness() {
        let l = look(&[("text-decoration", "underline overline wavy #f00 3px")], None, false).unwrap();
        assert_eq!(l.decoration_style, DecorationStyle::Wavy);
        assert!(l.overline);
        assert_eq!(l.decoration_thickness, Some(3.0));
        assert_eq!(l.decoration_color.map(|c| c.r), Some(1.0));
        let l = look(&[("text-underline-offset", "4px"), ("text-decoration-style", "double")], None, false).unwrap();
        assert_eq!((l.underline_offset, l.decoration_style), (Some(4.0), DecorationStyle::Double));
    }

    #[test]
    fn an_indent_takes_a_length_or_a_percentage_and_its_keywords() {
        let l = look(&[("text-indent", "2em hanging each-line")], None, false).unwrap();
        assert_eq!((l.indent_hanging, l.indent_each_line), (true, true));
        assert!(l.indent > 0.0);
        let l = look(&[("text-indent", "10%")], None, false).unwrap();
        assert_eq!((l.indent, l.indent_pct), (0.0, Some(10.0)));
    }

    #[test]
    fn an_ellipsis_needs_a_box_that_clips() {
        let cut = |pairs: &[(&str, &str)]| look(pairs, None, false).is_some_and(|l| l.ellipsis);
        assert!(cut(&[("text-overflow", "ellipsis"), ("overflow", "hidden")]));
        assert!(cut(&[("text-overflow", "ellipsis"), ("overflow-x", "clip")]));
        assert!(!cut(&[("text-overflow", "ellipsis")]));
    }

    /// `line-clamp`, and Tailwind's spelling of it, which a browser honours
    /// only beside `display: -webkit-box`.
    #[test]
    fn line_clamp_in_both_spellings() {
        let clamp = |pairs: &[(&str, &str)]| look(pairs, None, false).and_then(|l| l.line_clamp);
        assert_eq!(clamp(&[("line-clamp", "3")]), Some(3));
        assert_eq!(clamp(&[("line-clamp", "none")]), None);
        assert_eq!(clamp(&[("-webkit-line-clamp", "2"), ("display", "-webkit-box")]), Some(2));
        assert_eq!(clamp(&[("-webkit-line-clamp", "2")]), None);
        assert!(look(&[("line-clamp", "2")], None, false).is_some_and(|l| l.line_clamp == Some(2)));
        let parent = look(&[("line-clamp", "2")], None, false);
        assert_eq!(look(&[], parent.as_ref(), false).and_then(|l| l.line_clamp), None, "not inherited");
    }

    #[test]
    fn variants_become_font_features() {
        let l = look(&[("font-variant", "small-caps tabular-nums"), ("font-feature-settings", "\"zero\"")], None, false).unwrap();
        assert_eq!(l.features().as_deref(), Some("\"smcp\", \"tnum\", \"zero\""));
        let l = look(&[("font-stretch", "condensed")], None, false).unwrap();
        assert_eq!(l.stretch, Some(75.0));
        assert!(look(&[("font-stretch", "100%")], None, false).is_none());
    }

    /// `text-transform`, `text-indent`, the font settings and `caret-color`
    /// inherit; decoration does not, except into a run of a sentence.
    #[test]
    fn what_inherits_and_what_does_not() {
        let parent = look(
            &[("text-transform", "uppercase"), ("text-decoration-color", "red"), ("caret-color", "blue")],
            None,
            false,
        );
        let child = look(&[], parent.as_ref(), false).unwrap();
        assert_eq!(child.transform, TextTransform::Uppercase);
        assert!(child.caret_color.is_some());
        assert!(child.decoration_color.is_none());
        let run = look(&[], parent.as_ref(), true).unwrap();
        assert!(run.decoration_color.is_some());
    }
}

#[cfg(test)]
mod from_the_end_tests {
    use rux_script::Builder;
    use std::collections::HashMap;

    fn widths(src: &str) -> Vec<Option<rux_layout::Len>> {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let root = super::build_styled_tree(&sfc, &HashMap::new(), &HashMap::new(), &mut engine).unwrap();
        root.children[0].children.iter().map(|c| c.style.width).collect()
    }

    fn index(src: &str) -> super::ElementIndex {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let mut instances = super::Instances::new();
        let (_, reg) = super::build_styled_tree_tracked(
            &sfc, &HashMap::new(), &HashMap::new(), &mut engine, &mut instances, &mut crate::Swaps::new(),
        )
        .unwrap();
        reg.elements
    }

    /// An `r-if` that holds, one that does not with an `r-else` taken in its
    /// place, then an `r-for` of three: five rows, and the count has to read
    /// every one of those.
    const ROWS: &str = r#"<template><screen>
        <view class="rows">
          <view class="row" r-if="true"></view>
          <view class="row" r-if="false"></view>
          <view class="row" r-else></view>
          <view class="row" r-for="n in items"></view>
        </view>
      </screen></template>
      <style>
        .row:last-child { width: 1px; }
        .row:nth-last-child(2) { width: 2px; }
        .row:only-child { width: 9px; }
      </style>
      <script>let items = signal([1, 2, 3]);</script>"#;

    /// `:last-child` and `:nth-last-child()` are counted before the rows are
    /// built: the branch an `r-if` chain takes, and each row of an `r-for`.
    #[test]
    fn the_last_child_is_known_before_it_is_built() {
        use rux_layout::Len::Px;
        assert_eq!(widths(ROWS), [None, None, None, Some(Px(2.0)), Some(Px(1.0))]);
        // A row on its own is the only child.
        let one = ROWS.replace("[1, 2, 3]", "[]").replace(r#"r-if="true""#, r#"r-if="false""#);
        assert_eq!(widths(&one), [Some(Px(9.0))]);
    }

    /// `query()` counts from the end too, from the tree as built, whether or
    /// not the stylesheet asked for a count.
    #[test]
    fn a_query_counts_from_the_end() {
        let plain = ROWS.replace(".row:last-child", ".unused").replace(".row:nth-last-child(2)", ".unused2").replace(".row:only-child", ".unused3");
        for src in [ROWS.to_string(), plain] {
            let index = index(&src);
            let paths = |sel: &str| index.query(sel).unwrap().into_iter().map(|m| m.path).collect::<Vec<_>>();
            assert_eq!(paths(".row:last-child"), [vec![0, 4]]);
            assert_eq!(paths(".row:nth-child(2)"), [vec![0, 1]]);
            assert_eq!(paths(".row:nth-last-child(-n+2)"), [vec![0, 3], vec![0, 4]]);
            assert!(paths(".row:only-child").is_empty());
            assert_eq!(paths(".rows:only-child"), [vec![0]]);
        }
    }

    /// Matching itself: from the end needs the total, and a sibling found by
    /// `+` or `~` shares this element's.
    #[test]
    fn from_the_end_reads_the_total() {
        let (chain, combs, _) = super::parse_selector(".a:last-child").unwrap();
        let mut el = super::ElemDesc { tag: "view".into(), id: None, classes: vec!["a".into()], role: None, attrs: Vec::new(), states: Default::default(), total: Some(3) };
        let prev = vec![el.clone(), el.clone()];
        assert!(super::matches_chain(&chain, &combs, &el, &[], &prev));
        assert!(!super::matches_chain(&chain, &combs, &el, &[], &prev[..1]));
        el.total = None;
        assert!(!super::matches_chain(&chain, &combs, &el, &[], &prev), "nothing counted: fails closed");
        el.total = Some(3);
        let (chain, combs, _) = super::parse_selector(".a:nth-last-child(3) ~ .a").unwrap();
        assert!(super::matches_chain(&chain, &combs, &el, &[], &prev));
        let (chain, combs, _) = super::parse_selector(".a:not(:last-child)").unwrap();
        assert!(super::matches_chain(&chain, &combs, &el, &[], &prev[..1]));
        assert!(!super::matches_chain(&chain, &combs, &el, &[], &prev));
    }
}

#[cfg(test)]
mod selector_list_tests {
    use std::collections::HashMap;

    use rux_layout::{Background, Node};
    use rux_script::Builder;

    fn built(src: &str, hovered: Option<Vec<usize>>) -> Node {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let state = super::InteractionState { hovered, ..Default::default() };
        super::build_styled_tree_stateful(
            &sfc,
            &HashMap::new(),
            &HashMap::new(),
            &mut engine,
            &mut super::Instances::new(),
            &mut crate::Swaps::new(),
            &state,
            super::Environment::default(),
        )
        .unwrap()
        .0
    }

    fn red(node: &Node) -> bool {
        matches!(&node.style.background, Some(Background::Color(c)) if c.r == 1.0 && c.g == 0.0)
    }

    /// `:root` is the element with no parent, and a variable declared on it
    /// reaches everything.
    #[test]
    fn root_is_the_top_element() {
        let root = built(
            r#"<template><screen><view class="a" /></screen></template>
               <style>:root { --c: #ff0000; } :root, :host { --d: 1; } .a { background: var(--c); }</style>"#,
            None,
        );
        assert!(red(&root.children[0]));
    }

    /// `:where()` adds nothing to specificity, so a plain class written after
    /// it wins; `:is()` adds its most specific argument, so it does not.
    #[test]
    fn where_counts_nothing_and_is_counts_its_argument() {
        let src = |pseudo: &str| {
            format!(
                r#"<template><screen><view class="a b" /></screen></template>
                   <style>.a:{pseudo}(.b, #nope) {{ background: #00ff00; }} .a {{ background: #ff0000; }}</style>"#
            )
        };
        assert!(red(&built(&src("where"), None).children[0]), ":where() lost to the later .a");
        assert!(!red(&built(&src("is"), None).children[0]), ":is() carried an id's weight");
    }

    /// Every attribute operator CSS has. Until 2026-10-23 only `[role=…]` was
    /// read, and any other attribute test was dropped from its compound, so
    /// `input[type=checkbox]` styled every input.
    #[test]
    fn attribute_selectors_test_the_attribute() {
        let red_for = |selector: &str| {
            let root = built(
                &format!(
                    r#"<template><screen>
                         <input type="text" placeholder="Search lessons" r-model="q" />
                         <input type="checkbox" r-model="on" />
                       </screen></template>
                       <script>let q = signal(""); let on = signal(false);</script>
                       <style>{selector} {{ background: #ff0000; }}</style>"#
                ),
                None,
            );
            (red(&root.children[0]), red(&root.children[1]))
        };
        assert_eq!(red_for("input[type=checkbox]"), (false, true));
        assert_eq!(red_for("[type='text']"), (true, false));
        assert_eq!(red_for("[placeholder]"), (true, false));
        assert_eq!(red_for("[placeholder~=lessons]"), (true, false));
        assert_eq!(red_for("[placeholder^=Sea]"), (true, false));
        assert_eq!(red_for("[placeholder$=\"sons\"]"), (true, false));
        assert_eq!(red_for("[placeholder*=h\\ l]"), (true, false), "an escaped space is part of the value");
        assert_eq!(red_for("[placeholder*='h l']"), (true, false));
        assert_eq!(red_for("[type=TEXT]"), (false, false), "values are case-sensitive");
        assert_eq!(red_for("[type=TEXT i]"), (true, false));
        assert_eq!(red_for("[placeholder^='']"), (false, false), "an empty prefix matches nothing");
        assert_eq!(red_for("input:not([type=checkbox])"), (true, false));
        assert_eq!(red_for("input:where([type='button'], [type='checkbox'])"), (false, true));
        assert_eq!(red_for("[data-state]"), (false, false), "no Rux element carries it");
        assert_eq!(red_for("[type=text"), (false, false), "an unclosed bracket is no selector");
    }

    /// CSS nesting: `&`, a selector with none (a descendant), `> .b`, and a
    /// `@media` or `@supports` inside a rule, whose declarations are the
    /// rule's own.
    #[test]
    fn nested_rules_read_against_their_parent() {
        let red_for = |css: &str| {
            let root = built(
                &format!(
                    r#"<template><screen>
                         <view class="a b"><view class="c" /></view>
                         <view class="d"><view><view class="c" /></view></view>
                       </screen></template>
                       <style>{css}</style>"#
                ),
                None,
            );
            (red(&root.children[0]), red(&root.children[0].children[0]), red(&root.children[1].children[0].children[0]))
        };
        let on = "{ background: #ff0000; }";
        assert_eq!(red_for(&format!(".a {{ &.b {on} }}")), (true, false, false));
        assert_eq!(red_for(&format!(".a {{ .c {on} }}")), (false, true, false));
        assert_eq!(red_for(&format!(".a, .d {{ .c {on} }}")), (false, true, true));
        assert_eq!(red_for(&format!(".d {{ > .c {on} }}")), (false, false, false), "the .c under .d is a grandchild");
        assert_eq!(red_for(&format!(".a {{ @media (min-width: 1px) {{ background: #ff0000; }} }}")), (true, false, false));
        assert_eq!(red_for(&format!(".a {{ @media (max-width: 1px) {{ background: #ff0000; }} }}")), (false, false, false));
        assert_eq!(red_for(&format!(".a {{ @supports (display: grid) {{ .c {on} }} }}")), (false, true, false));
        // Declarations after a nested rule are still the parent's, and come
        // later than it, so they win over what came before.
        assert_eq!(red_for(".a { background: #0000ff; .c { color: red; } background: #ff0000; }"), (true, false, false));
    }

    /// `::placeholder` colours an empty input's placeholder, with `var()`,
    /// `currentcolor` and `opacity` read, and leaves the input itself alone.
    /// Tailwind's preflight is the case: `currentcolor` mixed to half, inside
    /// an `@supports` nested in the rule.
    #[test]
    fn placeholder_rules_colour_the_placeholder() {
        let shown = |css: &str| {
            let root = built(
                &format!(
                    r#"<template><screen><input class="f" placeholder="Search" r-model="q" /></screen></template>
                       <script>let q = signal("");</script>
                       <style>.f {{ color: #0000ff; }} {css}</style>"#
                ),
                None,
            );
            let input = &root.children[0];
            fn text(n: &Node) -> Option<rux_layout::Rgba> {
                n.text.as_ref().map(|t| t.color).or_else(|| n.children.iter().find_map(text))
            }
            (text(input).expect("the placeholder is drawn"), red(input))
        };
        let (dim, _) = shown("");
        assert!((dim.r - 0.42).abs() < 0.01, "Rux's own placeholder colour with no rule: {dim:?}");
        let (c, input_red) = shown(".f::placeholder { color: #ff0000; background: #ff0000; }");
        assert_eq!((c.r, c.b), (1.0, 0.0));
        assert!(!input_red, "a ::placeholder rule styled the input");
        let (c, _) = shown(":root { --p: #00ff00; } .f::placeholder { color: var(--p); opacity: 0.5; }");
        assert_eq!((c.g, c.a), (1.0, 0.5));
        let (c, _) = shown(
            "::placeholder { opacity: 1; } \
             @supports (display: grid) { ::placeholder { color: currentcolor; \
               @supports (color: color-mix(in lab, red, red)) { color: color-mix(in srgb, currentcolor 50%, transparent); } } }",
        );
        assert!(c.b > 0.99 && (c.a - 0.5).abs() < 0.02, "half the input's own blue: {c:?}");
    }

    /// `[lang|=en]` is `en` and `en-` anything; `[dir=rtl]` reads the
    /// attribute as written.
    #[test]
    fn dash_and_dir_attributes() {
        let root = built(
            r#"<template><screen><view dir="rtl" class="a" /><view class="a" /></screen></template>
               <style>.a[dir|=rtl] { background: #ff0000; }</style>"#,
            None,
        );
        assert!(red(&root.children[0]));
        assert!(!red(&root.children[1]));
    }

    /// A bound attribute is matched by its value now, and `false` takes it
    /// off, as binding it does in the DOM.
    #[test]
    fn a_bound_attribute_is_matched_by_its_value() {
        let src = |on: bool| {
            format!(
                r#"<template><screen><button :disabled="off" class="b"><text>Go</text></button></screen></template>
                   <script>let off = signal({on});</script>
                   <style>.b[disabled] {{ background: #ff0000; }}</style>"#
            )
        };
        assert!(red(&built(&src(true), None).children[0]));
        assert!(!red(&built(&src(false), None).children[0]));
    }

    /// Tailwind's `group-hover:`: the element is styled while an ancestor
    /// with the group class is hovered, and that ancestor gets the pointer
    /// region the shell reports.
    #[test]
    fn is_with_a_descendant_step_reads_the_ancestors() {
        let src = r#"<template><screen><view class="group"><view class="x" /></view><view class="x" /></screen></template>
            <style>.x:is(:where(.group):hover *) { background: #ff0000; }</style>"#;
        let resting = built(src, None);
        assert!(!red(&resting.children[0].children[0]));
        assert_eq!(resting.children[0].state_path.as_deref(), Some(&[0][..]), "the group needs a region");
        let hovered = built(src, Some(vec![0]));
        assert!(red(&hovered.children[0].children[0]), "inside the hovered group");
        assert!(!red(&hovered.children[1]), "outside any group");
    }

    /// Tailwind's `space-y-*`: a `:where()` that is the whole selector.
    #[test]
    fn where_can_be_the_whole_selector() {
        let root = built(
            r#"<template><screen><view class="s"><view /><view /><view /></view></screen></template>
               <style>:where(.s > :not(:last-child)) { background: #ff0000; }</style>"#,
            None,
        );
        let kids = &root.children[0].children;
        assert!(red(&kids[0]) && red(&kids[1]) && !red(&kids[2]));
    }

    /// An escaped class name is the name with the escapes read.
    #[test]
    fn escaped_class_names_match() {
        let root = built(
            r#"<template><screen><view class="md:flex" /><view class="py-1.5" /><view class="w-1/2" /><view class="10" /></screen></template>
               <style>.md\:flex, .py-1\.5, .w-1\/2, .\31 0 { background: #ff0000; }</style>"#,
            None,
        );
        assert!(root.children.iter().all(red));
    }

    /// A vendor's pseudo-element matches nothing and says nothing.
    #[test]
    fn vendor_pseudo_elements_are_passed_over_quietly() {
        let _ = super::take_warnings();
        built(
            r#"<template><screen /></template>
               <style>::-webkit-inner-spin-button { height: auto; } :-moz-focusring { outline: auto; }</style>"#,
            None,
        );
        let warnings = super::take_warnings();
        assert!(warnings.iter().all(|w| !w.message.contains("webkit") && !w.message.contains("moz")), "{warnings:?}");
    }
}

#[cfg(test)]
mod custom_property_tests {
    use std::collections::HashMap;

    use rux_layout::{Background, Node};
    use rux_script::Builder;

    fn built(src: &str) -> Node {
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        super::build_styled_tree_stateful(
            &sfc,
            &HashMap::new(),
            &HashMap::new(),
            &mut engine,
            &mut super::Instances::new(),
            &mut crate::Swaps::new(),
            &super::InteractionState::default(),
            super::Environment::default(),
        )
        .unwrap()
        .0
    }

    fn red(node: &Node) -> bool {
        matches!(&node.style.background, Some(Background::Color(c)) if c.r == 1.0 && c.g == 0.0)
    }

    /// Custom properties on one element read each other whatever order they
    /// are in. A map's order decided it before, so a theme of dozens lost
    /// some at random.
    #[test]
    fn own_custom_properties_read_each_other() {
        for _ in 0..8 {
            let root = built(
                r#"<template><screen class="t"><view class="a" /></screen></template>
                   <style>.t { --a: var(--b); --b: var(--c); --c: var(--d); --d: #ff0000; } .a { background: var(--a); }</style>"#,
            );
            assert!(red(&root.children[0]));
        }
    }

    /// A registered property nobody sets is its initial value.
    #[test]
    fn a_registered_property_starts_at_its_initial_value() {
        let root = built(
            r#"<template><screen><view class="a" /></screen></template>
               <style>@property --c { syntax: "<color>"; inherits: false; initial-value: #ff0000; }
                 .a { background: var(--c); }</style>"#,
        );
        assert!(red(&root.children[0]));
    }

    /// One registered with `inherits: false` is back at its initial value on
    /// a child, whatever its parent set.
    #[test]
    fn a_registered_property_that_does_not_inherit_resets_on_children() {
        let root = built(
            r#"<template><screen><view class="p"><view class="a" /></view></screen></template>
               <style>@property --c { syntax: "<color>"; inherits: false; initial-value: #ff0000; }
                 .p { --c: #00ff00; } .a { background: var(--c); }</style>"#,
        );
        assert!(red(&root.children[0].children[0]));
    }
}

#[cfg(test)]
mod tailwind_property_tests {
    use std::collections::HashMap;

    use rux_layout::{AnimProp, Len, Node};
    use rux_script::Builder;

    fn first(css: &str) -> Node {
        let src = format!(r#"<template><screen><view class="a" /></screen></template><style>{css}</style>"#);
        let sfc = rux_parser::parse_sfc(&src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let root = super::build_styled_tree_stateful(
            &sfc,
            &HashMap::new(),
            &HashMap::new(),
            &mut engine,
            &mut super::Instances::new(),
            &mut crate::Swaps::new(),
            &super::InteractionState::default(),
            super::Environment::default(),
        )
        .unwrap()
        .0;
        root.children[0].clone()
    }

    /// Tailwind's `transition`: a list of properties, with one duration and
    /// one easing beside it, repeated for each.
    #[test]
    fn transition_longhands_make_one_transition_per_property() {
        let n = first(
            ".a { transition-property: color, background-color, display, --tw-x, opacity; \
                  transition-duration: 150ms; transition-timing-function: linear; }",
        );
        let props: Vec<AnimProp> = n.style.transitions.iter().map(|t| t.property).collect();
        assert_eq!(props, vec![AnimProp::Color, AnimProp::BackgroundColor, AnimProp::Opacity]);
        assert!(n.style.transitions.iter().all(|t| t.duration == 150.0));
    }

    /// The shorthand written after a longhand puts it back.
    #[test]
    fn the_transition_shorthand_resets_its_longhands() {
        let n = first(".a { transition-duration: 900ms; transition: opacity 100ms; }");
        assert_eq!(n.style.transitions.len(), 1);
        assert_eq!(n.style.transitions[0].duration, 100.0);
        let n = first(".a { transition: opacity 100ms; transition-duration: 900ms; }");
        assert_eq!(n.style.transitions[0].duration, 900.0, "a longhand after it overrides");
    }

    /// `translate`, `rotate` and `scale` go in front of `transform`.
    #[test]
    fn individual_transforms_compose_before_transform() {
        let n = first(".a { translate: 10px 4px; scale: 50%; }");
        let m = n.style.transform.unwrap();
        assert_eq!([m[0], m[3], m[4], m[5]], [0.5, 0.5, 10.0, 4.0]);
        let n = first(".a { transform: translate(2px, 0); scale: 2; }");
        let m = n.style.transform.unwrap();
        assert_eq!([m[0], m[4]], [2.0, 4.0], "scale applies outside the transform list");
        assert!(first(".a { translate: none; }").style.transform.is_none());
    }

    /// Tailwind's `animate-spin`: `@keyframes`, the `animation` shorthand
    /// through a variable, and the keyframe's transform kept as functions.
    #[test]
    fn keyframes_and_the_animation_shorthand() {
        let n = first(
            ":root { --animate-spin: spin 1s linear infinite; }              @keyframes spin { to { transform: rotate(360deg); } }              .a { animation: var(--animate-spin); }",
        );
        let list = n.style.animations().expect("an animation");
        let a = &list[0];
        assert_eq!((a.name.as_str(), a.duration, a.iterations), ("spin", 1000.0, f32::INFINITY));
        assert_eq!(a.easing, rux_layout::Easing::Linear);
        assert_eq!(a.steps.len(), 1);
        assert_eq!(a.steps[0].offset, 1.0);
        assert_eq!(a.steps[0].props, vec![AnimProp::Transform]);
        let turn = &a.steps[0].transform[0];
        assert_eq!(turn.name, "rotate");
        assert!((turn.args[0] - std::f32::consts::TAU).abs() < 1e-4);
        assert!(a.base_transform.is_empty());

        // Longhands over the shorthand, and a percentage keyframe.
        let n = first(
            "@keyframes pulse { 50% { opacity: .5; } }              .a { animation: pulse 2s infinite; animation-delay: 100ms; animation-direction: alternate; animation-fill-mode: both; }",
        );
        let a = &n.style.animations().unwrap()[0];
        assert_eq!((a.duration, a.delay, a.steps[0].offset), (2000.0, 100.0, 0.5));
        assert_eq!(a.direction, rux_layout::AnimationDirection::Alternate);
        assert_eq!(a.fill, rux_layout::AnimationFill::Both);
        assert!(first(".a { animation: nothing-here 1s; }").style.animations().is_none());
        assert!(first(".a { animation: none; }").style.animations().is_none());
    }

    /// A translation in a percentage of the box: the property, a leading
    /// `translate()`, and Tailwind's `-translate-x-1/2`; px stays in the
    /// matrix.
    #[test]
    fn a_percentage_translation_is_kept_for_the_layout() {
        let pct = |css: &str| first(css).style.translate_pct();
        let size = |l: [Len; 2]| (l[0].px(200.0, (0.0, 0.0)), l[1].px(100.0, (0.0, 0.0)));
        assert_eq!(pct(".a { translate: -50% -50%; }").map(size), Some((-100.0, -50.0)));
        assert_eq!(pct(".a { translate: 10px 50%; }").map(size), Some((10.0, 50.0)));
        assert!(first(".a { translate: 10px 50%; }").style.transform.is_none(), "not counted twice");
        let n = first(".a { transform: translate(-50%, 0) rotate(90deg); }");
        assert_eq!(n.style.translate_pct().map(size), Some((-100.0, 0.0)));
        assert!(n.style.transform.is_some_and(|m| m[0].abs() < 1e-4), "the rotation is still in the matrix");
        let n = first(".a { --tw-translate-x: calc(calc(1/2 * 100%) * -1); --tw-translate-y: 0; translate: var(--tw-translate-x) var(--tw-translate-y); }");
        assert_eq!(n.style.translate_pct().map(size), Some((-100.0, 0.0)));
        assert!(pct(".a { translate: 10px 4px; }").is_none());
    }

    /// `transform-origin`: keywords either way round, lengths, and a single
    /// value that names only one axis.
    #[test]
    fn transform_origin_reads_as_css_does() {
        // Where it lands on a 100 by 50 box: the sheet parser may respell a
        // keyword as a length, so the place is what is compared.
        let at = |v: &str| {
            first(&format!(".a {{ transform-origin: {v}; }}"))
                .style
                .transform_origin()
                .map(|b| (b[0].px(100.0, (0.0, 0.0)), b[1].px(50.0, (0.0, 0.0))))
        };
        assert_eq!(at("top left"), Some((0.0, 0.0)));
        assert_eq!(at("left top"), Some((0.0, 0.0)));
        assert_eq!(at("bottom"), Some((50.0, 50.0)));
        assert_eq!(at("right"), Some((100.0, 25.0)));
        assert_eq!(at("10px 25%"), Some((10.0, 12.5)));
        assert_eq!(at("10px 20px 30px"), Some((10.0, 20.0)), "z is read past");
        assert_eq!(at("sideways"), None);
    }

    /// `inset` is the four sides; a later side wins over it.
    #[test]
    fn inset_sets_four_sides() {
        let n = first(".a { position: absolute; inset: 1px 2px; left: 9px; }");
        let px = |l: Option<Len>| match l {
            Some(Len::Px(v)) => v,
            other => panic!("{other:?}"),
        };
        let i = n.style.inset;
        assert_eq!([px(i[0]), px(i[1]), px(i[2]), px(i[3])], [1.0, 2.0, 1.0, 9.0]);
    }

    /// A side's colour overrides the box's, and a side's shorthand colours
    /// that side alone.
    #[test]
    fn border_sides_take_their_own_colours() {
        let n = first(".a { border: 2px solid #ff0000; border-top-color: transparent; }");
        assert!(n.style.border_color.is_some_and(|c| c.r == 1.0));
        let sides = n.style.border_colors();
        assert!(sides[0].is_some_and(|c| c.a == 0.0));
        assert!(sides[1].is_none());
        let n = first(".a { border-bottom: 1px solid #00ff00; }");
        let sides = n.style.border_colors();
        assert!(sides[0].is_none(), "only the bottom was coloured; the rest are currentcolor");
        assert!(sides[2].is_some_and(|c| c.g == 1.0));
    }

    /// The `font` shorthand, in its usual shapes.
    #[test]
    fn font_shorthand_is_its_longhands() {
        let font = |v: &str| super::expand_font(v).map(|l| l.into_iter().collect::<HashMap<_, _>>());
        let f = font("italic 600 20px/1.5 Inter, 'Segoe UI', sans-serif").unwrap();
        assert_eq!(f["font-style"], "italic");
        assert_eq!(f["font-weight"], "600");
        assert_eq!(f["font-size"], "20px");
        assert_eq!(f["line-height"], "1.5");
        assert_eq!(f["font-family"], "Inter, 'Segoe UI', sans-serif");
        assert_eq!(font("1rem serif").unwrap()["font-weight"], "normal");
        assert!(font("caption").is_none(), "a system font is not one Rux can name");
        assert_eq!(font("inherit").unwrap()["font-size"], "inherit");
    }

    /// `font: inherit` on a field takes the font around it, which is what
    /// every reset sheet writes.
    #[test]
    fn font_inherit_takes_the_parents_font() {
        let src = r#"<template><screen class="s"><input class="i" r-model="v" /></screen></template>
            <style>.s { font-size: 30px; font-weight: 700; } .i { font: inherit; color: inherit; }</style>
            <script>let v = signal("x");</script>"#;
        let sfc = rux_parser::parse_sfc(src).unwrap();
        let mut engine = Builder::new().build(&sfc.script).unwrap();
        let _ = super::take_warnings();
        let root = super::build_styled_tree_stateful(
            &sfc,
            &HashMap::new(),
            &HashMap::new(),
            &mut engine,
            &mut super::Instances::new(),
            &mut crate::Swaps::new(),
            &super::InteractionState::default(),
            super::Environment::default(),
        )
        .unwrap()
        .0;
        let warnings = super::take_warnings();
        assert!(warnings.is_empty(), "{warnings:?}");
        let text = root.children[0].children[0].text.as_ref().unwrap();
        assert_eq!((text.font_size, text.weight), (30.0, 700));
    }

    /// `-webkit-backdrop-filter` is `backdrop-filter`.
    #[test]
    fn a_vendor_alias_is_the_property() {
        let a = first(".a { -webkit-backdrop-filter: blur(4px); }");
        let b = first(".a { backdrop-filter: blur(4px); }");
        assert_eq!(format!("{:?}", a.style.filters), format!("{:?}", b.style.filters));
        assert!(format!("{:?}", a.style.filters) != "None");
    }
}

