//! Rux runtime, milestones M2–M9.
//!
//! The document model: loads a `.rux` file, resolves its `use` component imports
//! (loading each imported `.rux`), builds the script [`Engine`] (merging the main
//! and component scripts, registering host functions), and builds the renderable
//! tree with bindings, directives, and component expansions resolved. Running an
//! `@tap` handler mutates engine state; `rebuild` refreshes the tree.
//!
//! [`Document`] is the unit everything else works in terms of. `rux-shell` owns
//! one and asks it for a tree each frame; `rux check` builds one and throws the
//! tree away, which is why checking a file needs no window and no GPU and runs
//! in CI.
//!
//! Handling an event does not rebuild the document. `rux-script` records which
//! signals each binding reads and which ones a handler writes, so a state change
//! patches the nodes that the written signals actually reach and reconciles the
//! lists that changed shape. A full rebuild is the fallback, not the path.
//!
//! Two things are collected rather than printed, because the same document is
//! loaded by a CLI, a window and a browser, and only the caller knows where
//! output belongs. [`Diagnostics`] carries the errors and warnings a load
//! produced; [`take_warnings`] drains the ones raised out of band. Loading a
//! document that cannot be parsed is not a panic: it is a [`LoadError`], so the
//! window can keep the last good tree on screen and show the overlay instead of
//! dying on a half-typed edit.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rux_layout::Node as LayoutNode;
use rux_parser::{Sfc, StyleInclude};
use rux_script::{Builder, Engine};
use rux_style::{BindingRegistry, Instances};
/// Re-exported so the shell can hand pointer/focus state and the window size in
/// without depending on `rux-style` directly.
pub use rux_reactive::json_string;
pub use rux_style::{InteractionState, Viewport, Warning};

/// A loaded `.rux` document: parsed source, imported components (by tag), the
/// script engine, and the current tree.
pub struct Document {
    sfc: Sfc,
    components: HashMap<String, Sfc>,
    engine: Engine,
    /// Directory the document was loaded from, `<image src>` resolves against it.
    base: PathBuf,
    /// The focused input, with its caret and selection, if any. Re-applied on
    /// every rebuild so both survive a state change.
    focus: Option<Focus>,
    /// Where each patchable text binding lives and which signals force a rebuild,
    /// refreshed on every full build. Lets [`Document::patch`] update value
    /// bindings in place instead of throwing the tree away.
    registry: BindingRegistry,
    /// What the pointer is over / pressing, and which input has focus, the state
    /// `:hover`, `:active` and `:focus` match against. Owned here so every build
    /// (rebuild, reconcile, hot-reload) reproduces the same styling.
    state: InteractionState,
    /// The window size `@media` queries are evaluated against.
    viewport: Viewport,
    /// What is currently wrong with this document, for the dev overlay.
    diagnostics: Diagnostics,
    /// Every component instance's private state, kept here because the tree it
    /// belongs to is rebuilt constantly and the state must not be.
    instances: Instances,
    /// `computed` declarations, in declaration order, so one may read another
    /// declared above it and a single pass refreshes them all.
    computeds: Vec<Computed>,
    /// `effect` blocks, with what each read when it last ran.
    effects: Vec<Effect>,
    /// Where navigation has been, and where it is in that. See [`History`].
    history: History,
    pub root: LayoutNode,
}

/// The paths visited, and where along them we are.
///
/// A cursor into one list rather than two stacks, because that is the model a
/// user already has: going back then somewhere new drops the forward entries,
/// and going back and forward again returns to exactly where it was. Two stacks
/// express the same thing and make the truncation easy to get wrong.
#[derive(Clone, Debug)]
struct History {
    entries: Vec<String>,
    at: usize,
}

impl Default for History {
    fn default() -> Self {
        Self { entries: vec![ROOT_PATH.to_string()], at: 0 }
    }
}

impl History {
    /// Where we are now. Never empty, so this always answers.
    fn current(&self) -> &str {
        &self.entries[self.at]
    }

    /// Go somewhere new, dropping anything that was ahead. Navigating to where
    /// we already are is not a visit: it would otherwise fill the history with
    /// repeats of whatever link a user tapped twice, and make Back do nothing
    /// visible the first time it was pressed.
    fn push(&mut self, path: &str) -> bool {
        if self.current() == path {
            return false;
        }
        self.entries.truncate(self.at + 1);
        self.entries.push(path.to_string());
        self.at = self.entries.len() - 1;
        true
    }

    /// Step back, if there is anywhere to step back to.
    fn back(&mut self) -> bool {
        if self.at == 0 {
            return false;
        }
        self.at -= 1;
        true
    }

    /// Step forward into somewhere already visited and stepped back from.
    fn forward(&mut self) -> bool {
        if self.at + 1 >= self.entries.len() {
            return false;
        }
        self.at += 1;
        true
    }
}

/// Where a document starts before anything navigates.
pub const ROOT_PATH: &str = "/";

/// What is wrong with the document right now, the model behind the dev overlay.
///
/// An **error** means the file could not be loaded at all: there is no tree to
/// show, so the window would otherwise be blank (or, on hot-reload, silently
/// stale). A **warning** means the document built, but something in it does
/// nothing, an unhonored property, an unknown pseudo-class, an undefined
/// `var()`, an unsupported `@media`.
///
/// Both used to go only to stderr, which nobody running a GUI app is watching.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diagnostics {
    /// The load/parse failure, if the document is currently broken.
    pub error: Option<String>,
    /// Whether the tree on screen predates that error (a failed hot-reload keeps
    /// the last good UI rather than blanking the window).
    pub stale: bool,
    pub warnings: Vec<Warning>,
}

impl Diagnostics {
    pub fn is_empty(&self) -> bool {
        self.error.is_none() && self.warnings.is_empty()
    }
}

/// Which input has keyboard focus, and where its caret and selection are.
///
/// The selection is the range between `anchor` (where it started) and `caret`
/// (where it has been dragged/extended to); `anchor == caret` means no selection,
/// just a caret. Either may be the smaller, dragging leftwards puts the caret
/// before the anchor, so consumers normalize with [`Focus::range`].
#[derive(Clone, Debug, PartialEq)]
pub struct Focus {
    pub model: String,
    /// The `r-key` of the `r-for` row holding this input, when it is in one.
    ///
    /// `model` is the `r-model` expression as written, so every row of a list
    /// shares it and cannot identify which row the caret is in. The row's key
    /// is the other half. It also makes the caret follow its row when the list
    /// reorders: the identity is the row, not the position, so nothing has to
    /// be remapped afterwards.
    pub row: Option<String>,
    pub caret: usize,
    pub anchor: usize,
    /// The byte range of an in-progress IME composition, if one is running. The
    /// composed text is already in the bound value; this only marks which part
    /// of it is provisional, so the painter can underline it.
    pub preedit: Option<(usize, usize)>,
}

impl Focus {
    /// A plain caret with nothing selected, in an input that is not in a list.
    pub fn at(model: impl Into<String>, caret: usize) -> Self {
        Self::at_row(model, None, caret)
    }

    /// The same, in a known `r-for` row.
    pub fn at_row(model: impl Into<String>, row: Option<String>, caret: usize) -> Self {
        Self { model: model.into(), row, caret, anchor: caret, preedit: None }
    }

    /// Whether this focus is the input bound to `model` in row `row`. Both
    /// halves matter: a list's rows all share one model.
    pub fn is(&self, model: &str, row: Option<&str>) -> bool {
        self.model == model && self.row.as_deref() == row
    }

    /// The selected range, low to high.
    pub fn range(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    pub fn is_collapsed(&self) -> bool {
        self.caret == self.anchor
    }
}

/// Mark the focused input's text child with the caret position and selection, so
/// it paints them, and clear every other input's.
///
/// Clearing matters: this runs against the *existing* tree when focus moves, not
/// only against a freshly built one. Setting without clearing left the caret
/// showing in the input you just left, until some unrelated rebuild wiped it.
/// The selection is one more thing that can be left behind the same way.
fn apply_focus(node: &mut LayoutNode, focus: Option<&Focus>) {
    apply_focus_in(node, focus, None);
}

/// [`apply_focus`], carrying the `r-key` of the row being walked.
///
/// A splice starts partway down the tree, so the row a subtree sits in cannot be
/// recovered from the subtree itself; `row` is what the caller already knew. It
/// is `None` at the root and everywhere outside a keyed list.
fn apply_focus_in(node: &mut LayoutNode, focus: Option<&Focus>, row: Option<&str>) {
    let row = node.key.as_deref().or(row);
    if node.model.is_some() {
        if let Some(text) = node.children.first_mut().and_then(|c| c.text.as_mut()) {
            let mine = focus.filter(|f| {
                node.model.as_deref().is_some_and(|m| f.is(m, row))
            });
            // An empty input shows its placeholder; the caret still sits at 0.
            text.caret = mine.map(|f| f.caret.min(text.text.len()));
            text.selection = mine.filter(|f| !f.is_collapsed()).map(|f| {
                let (start, end) = f.range();
                (start.min(text.text.len()), end.min(text.text.len()))
            });
            text.preedit = mine.and_then(|f| f.preedit).map(|(start, end)| {
                (start.min(text.text.len()), end.min(text.text.len()))
            });
        }
    }
    for child in &mut node.children {
        apply_focus_in(child, focus, row);
    }
}

/// The deepest node whose subtree covers both paths, where the old and new
/// pointer targets diverge. Re-cascading from here restyles every element that
/// gained or lost the state and nothing else, because `:hover`/`:active` hold for
/// the whole chain from the root down to the pointer, and the two chains are
/// identical above the divergence.
///
/// When one side is `None` the pointer entered from (or left to) nothing, and the
/// entire chain changed state, including ancestors, so the splice starts at the
/// root. That is a full re-cascade, but only on entering/leaving all interactive
/// boxes, and only in documents that use pointer-state rules at all.
fn divergence(a: Option<&[usize]>, b: Option<&[usize]>) -> Vec<usize> {
    match (a, b) {
        (Some(a), Some(b)) => a.iter().zip(b).take_while(|(x, y)| x == y).map(|(x, _)| *x).collect(),
        _ => Vec::new(),
    }
}

/// Drain both warning sinks, the cascade's (unhonored properties, unknown
/// pseudo-classes, undefined `var()`s, unsupported `@media`) and the script's
/// (expressions that failed to compile or evaluate).
fn collect_warnings() -> Vec<Warning> {
    let mut warnings = rux_style::take_warnings();
    warnings.extend(rux_script::take_warnings());
    warnings
}

/// Drain the warning sinks without building anything.
///
/// The sinks are global and are only emptied by a *successful* build, so a load
/// that fails partway leaves whatever it managed to warn about sitting there,
/// ready to be misattributed to the next file. Anything checking more than one
/// document in a row needs to be able to clear them between files.
pub fn take_warnings() -> Vec<Warning> {
    collect_warnings()
}

/// Stop mirroring warnings to stderr as they are raised. Covers both sinks, so a
/// tool that formats them itself does not have to know there are two.
pub fn set_stderr_echo(on: bool) {
    rux_script::set_stderr_echo(on);
    rux_style::set_stderr_echo(on);
}

/// Whether this file is a document in its own right, rather than a component
/// meant to be used by one. `None` means the question could not be answered,
/// because the file would not read or parse.
///
/// The test is the one the spec already sets: "the application entry point is a
/// component whose root is `<screen>`". Anything else is a fragment expecting a
/// parent.
///
/// A checker needs the distinction. A component's `{{ prop }}` bindings are
/// supplied by whoever uses it, so loading one on its own reports every prop as
/// an undefined variable: failures that say nothing about whether the file is
/// correct. Going by the root rather than by who imports what also catches a
/// component that nothing currently uses.
pub fn is_entry_point(path: impl AsRef<Path>) -> Option<bool> {
    let src = std::fs::read_to_string(path.as_ref()).ok()?;
    let sfc = rux_parser::parse_sfc(&src).ok()?;
    Some(sfc.template.tag == "screen")
}

/// Resolve every `<image src>` in the tree against `base` and read its intrinsic
/// size, so a sizeless `<image>` lays out at its natural pixel dimensions. Only
/// the file header is read, not the pixels; the painter decodes and caches those.
fn resolve_images(node: &mut LayoutNode, base: &Path) {
    if let Some(img) = &mut node.image {
        if !img.src.is_empty() {
            let path = base.join(&img.src);
            if let Ok((w, h)) = image::image_dimensions(&path) {
                img.intrinsic = (w as f32, h as f32);
            } else {
                eprintln!("rux: cannot read image {}", path.display());
            }
            img.src = path.to_string_lossy().into_owned();
        }
    }
    // `background-image: url(…)` resolves against the .rux file too. The painter
    // sizes it to the box, so no intrinsic size is needed here.
    if let Some(rux_layout::Background::Image(src)) = &mut node.style.background {
        if !src.is_empty() {
            *src = base.join(&*src).to_string_lossy().into_owned();
        }
    }
    for child in &mut node.children {
        resolve_images(child, base);
    }
}

/// What went wrong loading a document, with the position kept when there is one.
///
/// [`Document::load`] flattens this to a string, which is what the dev overlay
/// wants: prose in a panel. A checker wants the parts separately, because an
/// editor cannot put a squiggle under a sentence. Same failure, two audiences,
/// so the structure is preserved here and thrown away at the last moment.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadError {
    pub message: String,
    /// The file the error is actually in, which is not always the file that was
    /// asked for: a component is reached through its parent's `use`.
    pub file: Option<PathBuf>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Whether this came from the parser, which is the only stage that knows a
    /// position. Kept so the flattened form reads the way it always has.
    parse: bool,
}

impl LoadError {
    fn plain(message: String) -> Self {
        Self { message, file: None, line: None, column: None, parse: false }
    }

    /// `file` is `None` when the source did not come from one, which is the
    /// playground's case: it has a buffer, not a path.
    fn parse(err: rux_parser::ParseError, file: Option<&Path>) -> Self {
        Self {
            message: err.message,
            file: file.map(Path::to_path_buf),
            line: err.line,
            column: err.column,
            parse: true,
        }
    }
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.parse {
            return write!(f, "{}", self.message);
        }
        match (self.line, self.column) {
            (Some(l), Some(c)) => {
                write!(f, "parse error at line {l}, column {c}: {}", self.message)
            }
            _ => write!(f, "parse error: {}", self.message),
        }
    }
}

impl std::error::Error for LoadError {}

impl Document {
    /// Load a document, flattening any failure to a sentence.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        Self::load_checked(path).map_err(|e| e.to_string())
    }

    /// Load a document, keeping the failure's position so a checker can point at
    /// it. [`Document::load`] is this with the structure discarded.
    pub fn load_checked(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let path = path.as_ref();
        let src = std::fs::read_to_string(path)
            .map_err(|e| LoadError::plain(format!("reading {}: {e}", path.display())))?;
        let mut sfc = rux_parser::parse_sfc(&src).map_err(|e| LoadError::parse(e, Some(path)))?;

        // Resolve `use module::component;` imports relative to this file.
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        resolve_style_includes(&mut sfc, base)?;
        let (main_script, imports) = extract_imports(&sfc.script);
        let (main_script, computeds, effects) = extract_reactives(&main_script);

        let mut components = HashMap::new();
        let mut combined_script = main_script;
        for import in imports {
            let comp_path = base.join(&import.file);
            let comp_src = std::fs::read_to_string(&comp_path).map_err(|e| {
                LoadError::plain(format!("reading component {}: {e}", comp_path.display()))
            })?;
            let mut comp_sfc =
                rux_parser::parse_sfc(&comp_src).map_err(|e| LoadError::parse(e, Some(&comp_path)))?;
            // A component's `src` is relative to the component, not to whoever
            // imported it. Anything else would make a component unusable from a
            // second directory, which is the whole point of having one.
            let comp_base = comp_path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
            resolve_style_includes(&mut comp_sfc, &comp_base)?;
            let (comp_script, _nested) = extract_imports(&comp_sfc.script);
            // A component's own computed/effect declarations are not
            // supported yet; strip them so the merged script still compiles.
            let (comp_script, _c, _e) = extract_reactives(&comp_script);
            // Only its *functions* join the shared engine. Its `let`s do not:
            // they are the state each instance gets a private copy of, so
            // merging them here would put one shared variable behind every
            // instance, which is exactly the bug this is fixing. `rux-style`
            // runs the same split and hands the statements to `init_scope`.
            combined_script.push('\n');
            combined_script.push_str(&component_functions(&comp_script));
            components.insert(import.tag, comp_sfc);
        }

        let mut engine = build_engine(&combined_script).map_err(LoadError::plain)?;
        let mut instances = Instances::new();
        let (mut root, registry) = rux_style::build_styled_tree_tracked(&sfc, &components, &mut engine, &mut instances)
            .map_err(LoadError::plain)?;
        resolve_images(&mut root, base);
        let mut doc = Self {
            sfc,
            components,
            engine,
            base: base.to_path_buf(),
            focus: None,
            registry,
            state: InteractionState::default(),
            viewport: Viewport::default(),
            // Whatever the build just complained about, ready for the overlay.
            diagnostics: Diagnostics {
                warnings: collect_warnings(),
                ..Diagnostics::default()
            },
            instances,
            computeds,
            effects,
            history: History::default(),
            root,
        };
        doc.init_reactive();
        Ok(doc)
    }

    /// Process `.rux` source with no import resolution (used for fallbacks/tests).
    pub fn from_source(src: &str) -> Result<Self, String> {
        Self::from_source_checked(src).map_err(|e| e.to_string())
    }

    /// [`Document::from_source`], keeping the failure's position.
    ///
    /// The playground needs this: it has no file to point at, so a parse error
    /// with no line was all it could ever show, and "something is wrong
    /// somewhere" is not much of an editor.
    pub fn from_source_checked(src: &str) -> Result<Self, LoadError> {
        let sfc = rux_parser::parse_sfc(src).map_err(|e| LoadError::parse(e, None))?;
        // There is no file here, so there is nothing for `src` to be relative
        // to and nothing to read it from. Warn rather than fail: the document
        // still renders, just without the sheet it asked for, and a playground
        // that refused to show anything would be worse than one that shows the
        // document and says what is missing.
        for path in &sfc.style_src {
            warn_unresolvable_include(path);
        }
        let (main_script, _imports) = extract_imports(&sfc.script);
        let (main_script, computeds, effects) = extract_reactives(&main_script);
        let mut engine = build_engine(&main_script).map_err(LoadError::plain)?;
        let mut instances = Instances::new();
        let (mut root, registry) =
            rux_style::build_styled_tree_tracked(&sfc, &HashMap::new(), &mut engine, &mut instances)
                .map_err(LoadError::plain)?;
        let base = PathBuf::from(".");
        resolve_images(&mut root, &base);
        let mut doc = Self {
            sfc,
            components: HashMap::new(),
            engine,
            base,
            focus: None,
            registry,
            state: InteractionState::default(),
            viewport: Viewport::default(),
            diagnostics: Diagnostics {
                warnings: collect_warnings(),
                ..Diagnostics::default()
            },
            instances,
            computeds,
            effects,
            history: History::default(),
            root,
        };
        doc.init_reactive();
        Ok(doc)
    }

    /// The script engine, for running `@tap` handlers.
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    /// What is currently wrong with this document, for the dev overlay.
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// Record that a re-load failed. The tree on screen stays as it was, so the
    /// app keeps working while the file is broken; the overlay says so, and marks
    /// what you're looking at as stale.
    pub fn set_load_error(&mut self, error: impl Into<String>) {
        self.diagnostics.error = Some(error.into());
        self.diagnostics.stale = true;
    }

    /// Mark the visible tree as *not* a leftover from before the error, used when
    /// the very first load failed, so there is no earlier version being shown.
    pub fn clear_stale(&mut self) {
        self.diagnostics.stale = false;
    }

    /// Adopt a freshly loaded document's tree and state, keeping this one's
    /// identity. Used by hot-reload so a successful load clears the error.
    pub fn replace_with(&mut self, mut fresh: Document) {
        // A reload rebuilds from scratch, so focus is legitimately reset, but the
        // viewport and pointer state belong to the window, not the file.
        fresh.viewport = self.viewport;
        fresh.state = self.state.clone();
        fresh.rebuild();
        *self = fresh;
    }

    /// Focus an input (by `r-model`), with its caret and selection. `None` clears.
    pub fn set_focus(&mut self, focus: Option<Focus>) {
        self.focus = focus;
        apply_focus(&mut self.root, self.focus.as_ref());
    }

    /// The pointer/focus state pseudo-class selectors match against.
    pub fn interaction(&self) -> &InteractionState {
        &self.state
    }

    /// Update the interaction state (`:hover` / `:active` / `:focus`) and restyle
    /// what it affects. Returns whether anything was restyled, so the shell knows
    /// whether to repaint, `false` for the overwhelmingly common case of the
    /// pointer moving within the same element.
    ///
    /// Only the affected subtree is spliced, not the whole tree: hover moving
    /// between two siblings re-cascades their common parent's subtree, so a caret
    /// or selection anywhere else survives by node identity, the same reconcile
    /// discipline signal changes use.
    pub fn set_interaction(&mut self, next: InteractionState) -> bool {
        if next == self.state {
            return false;
        }
        // A focus move can restyle anything (`:focus` is matched by model, not by
        // path, and `.field:focus .hint` reaches elsewhere), so it re-cascades from
        // the root. It is rare, one click or Tab, and the caret is being moved
        // anyway, so there is no ephemeral state left to preserve.
        let mut roots: Vec<Vec<usize>> = Vec::new();
        if next.focused_model == self.state.focused_model
            && next.focused_row == self.state.focused_row
        {
            roots.push(divergence(self.state.hovered.as_deref(), next.hovered.as_deref()));
            roots.push(divergence(self.state.active.as_deref(), next.active.as_deref()));
        } else {
            roots.push(Vec::new());
        }
        self.state = next;
        self.restyle(&roots);
        true
    }

    /// Tell the document the window size, for `@media`. Returns whether any query
    /// changed answer, i.e. whether the rule set moved and the tree had to be
    /// re-cascaded.
    ///
    /// A resize fires continuously, and almost every one crosses no breakpoint, so
    /// the common case must be free: the media conditions are evaluated at the old
    /// and new size and compared, and the tree is only rebuilt when that vector
    /// actually differs. A document with no `@media` at all compares two empty
    /// vectors and never rebuilds.
    pub fn set_viewport(&mut self, viewport: Viewport) -> bool {
        if viewport == self.viewport {
            return false;
        }
        let before = self.media_state(self.viewport);
        let after = self.media_state(viewport);
        self.viewport = viewport;
        if before == after {
            return false;
        }
        // A breakpoint was crossed: re-cascade everything. Focus is re-applied by
        // `rebuild`, and scroll offsets live in the shell, so nothing is lost.
        self.rebuild();
        true
    }

    /// Whether each `@media` block, in the document and in every component,
    /// applies at `viewport`.
    fn media_state(&self, viewport: Viewport) -> Vec<bool> {
        let mut out = rux_style::media_matches(&self.sfc.style, viewport);
        // Components are keyed by tag in a HashMap, so sort for a stable order.
        let mut tags: Vec<&String> = self.components.keys().collect();
        tags.sort();
        for tag in tags {
            out.extend(rux_style::media_matches(&self.components[tag].style, viewport));
        }
        out
    }

    /// Rebuild the given subtrees against the current interaction state and splice
    /// them into the live tree, re-applying focus scoped to each.
    fn restyle(&mut self, roots: &[Vec<usize>]) {
        let Ok((mut fresh_root, fresh_reg)) = rux_style::build_styled_tree_stateful(
            &self.sfc,
            &self.components,
            &mut self.engine,
            &mut self.instances,
            &self.state,
            self.viewport,
        ) else {
            return;
        };
        resolve_images(&mut fresh_root, &self.base);
        for path in roots {
            let Some(fresh) = node_at(&fresh_root, path) else { continue };
            let fresh_node = fresh.clone();
            let row = row_at(&fresh_root, path);
            if let Some(live) = node_at_mut(&mut self.root, path) {
                *live = fresh_node;
                apply_focus_in(live, self.focus.as_ref(), row.as_deref());
            }
        }
        self.registry = fresh_reg;
    }

    /// Rebuild the layout tree from the engine's current state.
    pub fn rebuild(&mut self) {
        if let Ok((mut root, registry)) = rux_style::build_styled_tree_stateful(
            &self.sfc,
            &self.components,
            &mut self.engine,
            &mut self.instances,
            &self.state,
            self.viewport,
        ) {
            resolve_images(&mut root, &self.base);
            apply_focus(&mut root, self.focus.as_ref());
            self.registry = registry;
            self.root = root;
            // Refresh what the overlay lists: a rebuild re-runs the cascade and
            // every binding, so it re-raises exactly what this document still has
            // wrong.
            self.diagnostics.warnings = collect_warnings();
        }
    }

    /// Apply a set of changed signals *in place* where possible: re-evaluate the
    /// text bindings that read them and write the new strings into their nodes,
    /// without rebuilding the tree (so ephemeral state, caret, scroll, survives
    /// untouched). Returns `false` when the change can't be patched, it touched a
    /// signal that drives structure, an attribute, an input value, or a component
    /// prop, in which case the caller must [`rebuild`](Self::rebuild). Nothing is
    /// mutated on the `false` path.
    #[must_use]
    pub fn patch(&mut self, changed: &HashSet<String>) -> bool {
        if changed.is_empty() {
            return true; // nothing changed → nothing to do, and no rebuild needed
        }
        // A non-reconcilable structural read (component prop, `:src`/`:options`, or
        // a toggle's `checked` class) still needs a full rebuild.
        if !self.registry.structural.is_disjoint(changed) {
            return false;
        }
        // r-if/r-for: reconcile just the affected subtrees in place.
        self.reconcile(changed);
        // Text, input values, and r-show update in place.
        self.patch_values(changed);
        true
    }

    /// Re-evaluate the value bindings (text `{{ }}`, input values, `r-show`) whose
    /// deps changed and write them into their nodes, no shape change.
    fn patch_values(&mut self, changed: &HashSet<String>) {
        for binding in &self.registry.text {
            if binding.deps.is_disjoint(changed) {
                continue;
            }
            let text = rux_style::eval_text_binding(binding, &mut self.engine);
            if let Some(node) = node_at_mut(&mut self.root, &binding.path) {
                if let Some(content) = node.text.as_mut() {
                    content.text = text;
                }
            }
        }
        // Input values live in the input's first child; patch their text + colour
        // so a keystroke doesn't rebuild. The caret/selection on that child are
        // left untouched (the shell sets them via `set_focus`).
        for binding in &self.registry.value {
            if binding.deps.is_disjoint(changed) {
                continue;
            }
            let (text, color) = rux_style::eval_value_binding(binding, &mut self.engine);
            if let Some(node) = node_at_mut(&mut self.root, &binding.path) {
                if let Some(content) = node.children.first_mut().and_then(|c| c.text.as_mut()) {
                    content.text = text;
                    content.color = color;
                }
            }
        }
        // `r-show` only flips paint on/off, rewrite the `hidden` bool in place.
        for binding in &self.registry.show {
            if binding.deps.is_disjoint(changed) {
                continue;
            }
            let visible = self.engine.eval_bool(&binding.cond, &binding.locals);
            if let Some(node) = node_at_mut(&mut self.root, &binding.path) {
                node.hidden = !visible;
            }
        }
        // `:src`: rewrite the image source and re-resolve it (path + intrinsic size).
        for binding in &self.registry.src {
            if binding.deps.is_disjoint(changed) {
                continue;
            }
            let raw = rux_style::eval_src_binding(binding, &mut self.engine);
            if let Some(node) = node_at_mut(&mut self.root, &binding.path) {
                if let Some(img) = node.image.as_mut() {
                    img.src = raw;
                }
                resolve_images(node, &self.base);
            }
        }
        // `:options`: rewrite a select's option list in place.
        for binding in &self.registry.options {
            if binding.deps.is_disjoint(changed) {
                continue;
            }
            let opts = rux_style::eval_options_binding(binding, &mut self.engine);
            if let Some(node) = node_at_mut(&mut self.root, &binding.path) {
                node.options = Some(opts);
            }
        }
    }

    /// Reconcile the `r-if`/`r-for` parents whose deps changed: build a fresh tree,
    /// splice its affected subtrees into the live one, and re-apply focus *scoped*
    /// to those subtrees. Unaffected subtrees keep their live node identity, so a
    /// caret (or any ephemeral state) elsewhere survives with no whole-tree
    /// restore. Refreshes the registry to the fresh build. No-op if nothing
    /// structural changed.
    fn reconcile(&mut self, changed: &HashSet<String>) {
        // Outermost affected structural parents (a nested one is regenerated by its
        // ancestor's splice), and the toggle nodes whose bound signal changed.
        let mut affected: Vec<Vec<usize>> = self
            .registry
            .structural_parents
            .iter()
            .filter(|p| !p.deps.is_disjoint(changed))
            .map(|p| p.tree_path.clone())
            .collect();
        let toggles: Vec<Vec<usize>> = self
            .registry
            .toggles
            .iter()
            .filter(|t| !t.deps.is_disjoint(changed))
            .map(|t| t.path.clone())
            .collect();
        // Components and `:class`/`:style` nodes both reconcile by node-splice with
        // scoped focus (their subtrees may hold inputs).
        let mut node_splices: Vec<Vec<usize>> = self
            .registry
            .components
            .iter()
            .filter(|c| !c.deps.is_disjoint(changed))
            .map(|c| c.path.clone())
            .collect();
        node_splices.extend(
            self.registry
                .styled
                .iter()
                .filter(|s| !s.deps.is_disjoint(changed))
                .map(|s| s.path.clone()),
        );
        if affected.is_empty() && toggles.is_empty() && node_splices.is_empty() {
            return;
        }
        affected.sort_by_key(Vec::len);
        let mut roots: Vec<Vec<usize>> = Vec::new();
        for p in affected {
            if !roots.iter().any(|r| p.starts_with(r.as_slice())) {
                roots.push(p);
            }
        }

        let Ok((mut fresh_root, fresh_reg)) = rux_style::build_styled_tree_stateful(
            &self.sfc,
            &self.components,
            &mut self.engine,
            &mut self.instances,
            &self.state,
            self.viewport,
        ) else {
            return;
        };
        resolve_images(&mut fresh_root, &self.base);
        // Structural parents: replace the affected parent's children wholesale.
        for p in &roots {
            let Some(fresh) = node_at(&fresh_root, p) else { continue };
            let fresh_children = fresh.children.clone();
            let row = row_at(&fresh_root, p);
            if let Some(live) = node_at_mut(&mut self.root, p) {
                live.children = fresh_children;
                // Put the caret back only within this rebuilt subtree. The rows
                // carry their own keys, so a caret in a row that moved lands in
                // that row rather than in the position it used to hold.
                apply_focus_in(live, self.focus.as_ref(), row.as_deref());
            }
        }
        // Toggles: replace just the single node (its checked style + mark). No
        // shape change and no caret on a toggle, so no scoped focus is needed.
        for p in &toggles {
            if roots.iter().any(|r| p.starts_with(r.as_slice())) {
                continue; // already covered by a parent splice above
            }
            if let Some(fresh) = node_at(&fresh_root, p) {
                let fresh_node = fresh.clone();
                if let Some(live) = node_at_mut(&mut self.root, p) {
                    *live = fresh_node;
                }
            }
        }
        // Components and :class/:style nodes: replace the whole node subtree,
        // re-applying focus scoped to it (the subtree may hold inputs).
        for p in &node_splices {
            if roots.iter().any(|r| p.starts_with(r.as_slice())) {
                continue;
            }
            if let Some(fresh) = node_at(&fresh_root, p) {
                let fresh_node = fresh.clone();
                let row = row_at(&fresh_root, p);
                if let Some(live) = node_at_mut(&mut self.root, p) {
                    *live = fresh_node;
                    apply_focus_in(live, self.focus.as_ref(), row.as_deref());
                }
            }
        }
        self.registry = fresh_reg;
    }

    /// Apply an input edit (a keystroke's new value for `model`) and reflect it the
    /// cheapest correct way: patch the input's shown value in place, falling back
    /// to a rebuild only when `model` is also read structurally. The caller sets
    /// the caret afterward via [`set_focus`](Self::set_focus).
    pub fn apply_edit(&mut self, model: &str, value: &str) {
        self.apply_edit_in(model, None, value);
    }

    /// [`apply_edit`](Self::apply_edit) for an input in a known `r-for` row.
    ///
    /// The row matters because an `r-model` is recorded as written: inside a
    /// list it can mention the loop variable, which only exists in that row's
    /// scope. The scope was captured when the input was built, so this looks it
    /// up rather than reconstructing it.
    pub fn apply_edit_in(&mut self, model: &str, row: Option<&str>, value: &str) {
        let locals = self.locals_for(model, row);
        let changed = self.engine.assign_string(model, value, &locals);
        if changed.is_empty() {
            return;
        }
        self.apply_change(&changed);
    }

    /// An input's current value, read in its own row's scope.
    pub fn value_in(&mut self, model: &str, row: Option<&str>) -> String {
        let locals = self.locals_for(model, row);
        self.engine.get_string_in(model, &locals)
    }

    /// The loop variables that were in scope where this input was built.
    ///
    /// Matched on model *and* row, since a list's rows all record the same
    /// model. Empty for an input outside a list, which is the common case and
    /// needs nothing.
    fn locals_for(&self, model: &str, row: Option<&str>) -> Vec<(String, rux_reactive::Value)> {
        self.registry
            .value
            .iter()
            .find(|b| b.model == model && b.row.as_deref() == row)
            .map(|b| b.locals.clone())
            .unwrap_or_default()
    }

    /// Run an `@tap` handler and reflect its effect the cheapest correct way:
    /// patch the changed bindings in place, falling back to a full rebuild only
    /// when the change is structural. Returns whether anything changed, so the
    /// shell knows whether to repaint.
    pub fn apply_handler(&mut self, src: &str) -> bool {
        self.apply_handler_in(src, None)
    }

    /// Run a handler that was written inside a component instance.
    ///
    /// The instance's state and props are in scope, and whatever the handler
    /// leaves them at is written back: that is what makes a component's own
    /// state writable, and what keeps two instances of one component apart.
    ///
    /// A change to instance state rebuilds rather than patches. The state is
    /// not a signal, so the binding registry has nothing to look it up by, and
    /// claiming otherwise would mean bindings quietly missing updates. A
    /// component is a subtree, so the rebuild is bounded in practice.
    pub fn apply_handler_in(&mut self, src: &str, instance: Option<&str>) -> bool {
        // Anything emitted before this handler was not emitted *by* it: a build
        // evaluates every binding, and a stray `emit` in one of those would
        // otherwise be delivered to the first tap that happened afterwards. The
        // same goes for a `navigate` evaluated during a build.
        let _ = rux_script::take_emissions();
        let _ = rux_script::take_navigations();
        let ran = self.dispatch_handler(src, instance, 0);
        // Navigation is applied once, after the handler and everything it set
        // off have finished. A handler that navigates and then writes a signal
        // would otherwise render the old route with the new state in it.
        self.apply_navigations() || ran
    }

    /// Apply whatever `navigate`/`back`/`forward` the last run asked for.
    ///
    /// Later calls win: a handler that navigates twice ends up where the second
    /// one pointed, and the intermediate path is not a place the user visited.
    fn apply_navigations(&mut self) -> bool {
        let mut moved = false;
        for nav in rux_script::take_navigations() {
            moved |= match nav {
                rux_script::Nav::To(path) => self.navigate(&path),
                rux_script::Nav::Back => self.back(),
                rux_script::Nav::Forward => self.forward(),
            };
        }
        moved
    }

    /// The path the document is on.
    pub fn route(&self) -> &str {
        self.history.current()
    }

    /// Go to `path`, recording it in the history.
    ///
    /// Returns whether anything changed, so the shell knows whether to repaint.
    /// Navigating to where we already are changes nothing, which is what makes
    /// tapping the current link in a nav bar a no-op rather than a rebuild.
    pub fn navigate(&mut self, path: &str) -> bool {
        if !self.history.push(path) {
            return false;
        }
        self.show_current_route()
    }

    /// Step back through the history. Returns whether there was anywhere to go.
    pub fn back(&mut self) -> bool {
        self.history.back() && self.show_current_route()
    }

    /// Step forward again. Returns whether there was anywhere to go.
    pub fn forward(&mut self) -> bool {
        self.history.forward() && self.show_current_route()
    }

    /// Move the document to whatever path the history now points at.
    fn show_current_route(&mut self) -> bool {
        let path = self.history.current().to_string();
        // A route's views are dropped when we leave it, so visiting one a second
        // time starts fresh instead of resuming where it was left. That is what
        // every router does, and the alternative here would be accidental:
        // instance state is keyed by template position and would otherwise
        // simply still be sitting there.
        self.instances.retain(|_, i| i.route.as_deref().is_none_or(|r| r == path));
        if !self.engine.set_route(&path) {
            return false;
        }
        // The route is a signal like any other, so the ordinary change path
        // decides between a reconcile and a rebuild. A `<router>` records itself
        // as a structural read, so this reconciles the router's subtree.
        self.apply_change(&HashSet::from([rux_script::ROUTE_SIGNAL.to_string()]));
        true
    }

    /// One handler run, plus whatever its `emit` calls set off. `depth` is the
    /// length of that chain: a component listened to by a component that emits
    /// back is a cycle, and stopping it at a bound is better than a window that
    /// never repaints.
    fn dispatch_handler(&mut self, src: &str, instance: Option<&str>, depth: usize) -> bool {
        const MAX_EVENT_DEPTH: usize = 8;
        if depth > MAX_EVENT_DEPTH {
            rux_script::warn_script(format!(
                "an event chain is still going after {MAX_EVENT_DEPTH} rounds and has been \
                 stopped; a component is probably emitting an event that comes back to it"
            ));
            return false;
        }

        let Some(key) = instance.filter(|k| self.instances.contains_key(*k)) else {
            let changed = self.engine.run_handler_tracked(src);
            // An `emit` outside a component has nobody to tell: the document is
            // the top of the tree. Say so rather than dropping it, since the
            // author plainly expected something to happen.
            for (event, _) in rux_script::take_emissions() {
                rux_script::warn_script(format!(
                    "`emit(\"{event}\")` outside a component has no caller to receive it"
                ));
            }
            if changed.is_empty() {
                return false;
            }
            self.apply_change(&changed);
            return true;
        };

        let entry = &self.instances[key];
        let mut locals = entry.state.clone();
        locals.extend(entry.props.iter().cloned());
        let (after, changed) = self.engine.run_scoped_handler(src, &locals);

        // Only the component's own names are written back. A prop belongs to the
        // caller: assigning to one inside a component would look like it worked
        // and be forgotten on the next build, which is worse than not allowing it.
        let state_names: Vec<String> =
            self.instances[key].state.iter().map(|(n, _)| n.clone()).collect();
        let mut moved = false;
        for (name, value) in after {
            if !state_names.contains(&name) {
                continue;
            }
            let slot = self
                .instances
                .get_mut(key)
                .and_then(|i| i.state.iter_mut().find(|(n, _)| *n == name));
            if let Some(slot) = slot {
                if slot.1 != value {
                    slot.1 = value;
                    moved = true;
                }
            }
        }

        // Deliver whatever the handler emitted. After the state write-back
        // above, so a listener that rebuilds rebuilds against the state the
        // handler left, not the state it started from.
        let mut fired = false;
        for (event, payload) in rux_script::take_emissions() {
            let listener = self.instances[key]
                .listeners
                .iter()
                .find(|(name, _)| *name == event)
                .map(|(_, body)| body.clone());
            // An event nobody listens to is ordinary, not a mistake: a
            // component offers events and a caller takes the ones it wants.
            let Some(body) = listener else { continue };
            let caller = self.instances[key].caller.clone();
            fired |= self.dispatch_handler(&with_event(&body, payload.as_ref()), caller.as_deref(), depth + 1);
        }

        if !changed.is_empty() {
            self.apply_change(&changed);
            return true;
        }
        if moved {
            self.rebuild();
            return true;
        }
        fired
    }

    /// Reflect a set of changed signals: patch in place, or rebuild when the change
    /// is structural. `RUX_TRACE=1` prints which path was taken, so the reactivity
    /// behavior is observable while driving (the pixels are identical either way).
    /// Record what the computeds and effects read, and run every effect once.
    ///
    /// Effects run on load rather than only on the first change, which is the
    /// only way an effect can *establish* something (a title, a saved value)
    /// rather than merely react to it. It is also where their dependency sets
    /// come from: an effect subscribes to what it read, so it has to read first.
    fn init_reactive(&mut self) {
        for i in 0..self.computeds.len() {
            let (name, expr) = (self.computeds[i].name.clone(), self.computeds[i].expr.clone());
            let (_, deps) = self.engine.recompute(&name, &expr);
            self.computeds[i].deps = deps;
        }
        let mut writes: HashSet<String> = HashSet::new();
        for i in 0..self.effects.len() {
            let body = self.effects[i].body.clone();
            let (mut reads, wrote) = self.engine.run_effect_tracked(&body);
            // An effect is never woken by its own writes. Assigning to a signal
            // resolves its name, so the tracker sees a write as a read too, and
            // every effect that wrote anything would immediately re-trigger
            // itself: harmless when the result settles, an eight-round pile-up
            // when it does not. The cost is that an effect which writes X will
            // not re-run when someone *else* changes X, which is the right way
            // round: that effect is the one deciding what X is.
            reads.retain(|n| !wrote.contains(n));
            self.effects[i].deps = reads;
            writes.extend(wrote);
        }
        self.diagnostics.warnings.extend(collect_warnings());
        if !writes.is_empty() {
            // An effect that set something on load has to be reflected, or the
            // first frame shows the state it was written to replace.
            self.apply_change_depth(&writes, 1);
        }
    }

    /// Bring the computeds up to date, adding any that actually changed to
    /// `changed` so the bindings reading them are patched too.
    ///
    /// One pass in declaration order, which is enough for a computed that reads
    /// another declared above it, and is where the ordering rule comes from: a
    /// computed may only read computeds declared before it. The alternative is
    /// iterating to a fixpoint, which turns a typo into a hang.
    fn refresh_computed(&mut self, changed: &mut HashSet<String>) {
        for i in 0..self.computeds.len() {
            let stale = !self.computeds[i].deps.is_disjoint(changed);
            if !stale {
                continue;
            }
            let (name, expr) = (self.computeds[i].name.clone(), self.computeds[i].expr.clone());
            let (moved, deps) = self.engine.recompute(&name, &expr);
            self.computeds[i].deps = deps;
            if moved {
                changed.insert(name);
            }
        }
    }

    /// Run the effects whose dependencies changed, and report what they wrote.
    fn run_effects(&mut self, changed: &HashSet<String>) -> HashSet<String> {
        let mut writes = HashSet::new();
        for i in 0..self.effects.len() {
            if self.effects[i].deps.is_disjoint(changed) {
                continue;
            }
            let body = self.effects[i].body.clone();
            let (mut reads, wrote) = self.engine.run_effect_tracked(&body);
            // An effect is never woken by its own writes. Assigning to a signal
            // resolves its name, so the tracker sees a write as a read too, and
            // every effect that wrote anything would immediately re-trigger
            // itself: harmless when the result settles, an eight-round pile-up
            // when it does not. The cost is that an effect which writes X will
            // not re-run when someone *else* changes X, which is the right way
            // round: that effect is the one deciding what X is.
            reads.retain(|n| !wrote.contains(n));
            self.effects[i].deps = reads;
            writes.extend(wrote);
        }
        writes
    }

    fn apply_change(&mut self, changed: &HashSet<String>) {
        self.apply_change_depth(changed, 0);
    }

    /// [`apply_change`](Self::apply_change), counting how many times an effect
    /// has caused another round.
    ///
    /// An effect that writes a signal it also reads is a loop. It is stopped and
    /// reported rather than followed: a window that hangs tells you nothing,
    /// while a warning naming the effect is the whole diagnosis.
    fn apply_change_depth(&mut self, changed: &HashSet<String>, depth: u32) {
        const MAX_EFFECT_ROUNDS: u32 = 8;
        let mut changed = changed.clone();
        self.refresh_computed(&mut changed);
        let patched = self.patch(&changed);
        if !patched {
            self.rebuild();
        }
        let writes = self.run_effects(&changed);
        if !writes.is_empty() {
            if depth + 1 >= MAX_EFFECT_ROUNDS {
                let mut names: Vec<&str> = writes.iter().map(String::as_str).collect();
                names.sort_unstable();
                rux_style::warn_stylesheet(format!(
                    "an `effect` keeps re-triggering itself (still writing {names:?} after \
                     {MAX_EFFECT_ROUNDS} rounds); it was stopped. An effect must not write a \
                     signal it also reads."
                ));
                self.diagnostics.warnings.extend(collect_warnings());
                return;
            }
            self.apply_change_depth(&writes, depth + 1);
            return;
        }
        if std::env::var_os("RUX_TRACE").is_some() {
            let mut names: Vec<&str> = changed.iter().map(String::as_str).collect();
            names.sort_unstable();
            eprintln!(
                "rux: change {names:?} → {}",
                if patched { "patched in place (no rebuild)" } else { "rebuilt (structural)" }
            );
        }
    }
}

/// Follow a child-index path from the root to a node.
fn node_at<'a>(root: &'a LayoutNode, path: &[usize]) -> Option<&'a LayoutNode> {
    let mut node = root;
    for &i in path {
        node = node.children.get(i)?;
    }
    Some(node)
}

/// The `r-key` of the row `path` lands inside, if any.
///
/// A splice re-applies focus to a subtree, and the subtree cannot tell you which
/// row it is in: the key is on an ancestor that the splice never looks at. This
/// walks down from the root to recover it.
fn row_at(root: &LayoutNode, path: &[usize]) -> Option<String> {
    let mut node = root;
    let mut row = node.key.clone();
    for &i in path {
        node = node.children.get(i)?;
        if node.key.is_some() {
            row = node.key.clone();
        }
    }
    row
}

/// Follow a child-index path from the root to a node, mutably.
fn node_at_mut<'a>(root: &'a mut LayoutNode, path: &[usize]) -> Option<&'a mut LayoutNode> {
    let mut node = root;
    for &i in path {
        node = node.children.get_mut(i)?;
    }
    Some(node)
}

/// Read the stylesheets a document asked for with `<style src="…">`, relative
/// to the file that asked.
///
/// A missing stylesheet is a load failure, not a warning, and deliberately the
/// same kind of failure as a missing component: both are a file naming another
/// file that is not there, and a document that silently renders unstyled looks
/// like a layout bug rather than a typo in a path. The window keeps the last
/// good tree on screen and puts the message in the overlay, so the cost of
/// being strict is a red panel and not a closed window.
fn resolve_style_includes(sfc: &mut Sfc, base: &Path) -> Result<(), LoadError> {
    if sfc.style_src.is_empty() {
        return Ok(());
    }
    let mut includes = Vec::with_capacity(sfc.style_src.len());
    for relative in &sfc.style_src {
        let path = base.join(relative);
        let css = std::fs::read_to_string(&path).map_err(|e| {
            LoadError::plain(format!("reading stylesheet {}: {e}", path.display()))
        })?;
        includes.push(StyleInclude { path: relative.clone(), css });
    }
    sfc.style_includes = includes;
    Ok(())
}

/// Say that an include could not be resolved because there is no file to be
/// relative to. Only the browser reaches this.
fn warn_unresolvable_include(path: &str) {
    rux_style::warn_stylesheet(format!(
        "`<style src=\"{path}\">` was ignored: this document was loaded from source, \
         not from a file, so there is nothing for the path to be relative to"
    ));
}

/// A `computed name = expr;` declaration.
///
/// Kept beside the script rather than inside it because it has to be
/// *re-evaluated*, and rhai has no lazy value: the line is rewritten to a plain
/// `let` so the name becomes an ordinary signal, and the expression is kept here
/// so the runtime can run it again when something it reads changes.
#[derive(Clone, Debug)]
struct Computed {
    name: String,
    expr: String,
    /// Signals the expression read when it last ran.
    deps: HashSet<String>,
}

/// An `effect { … }` block: statements to run when what they read changes.
#[derive(Clone, Debug)]
struct Effect {
    body: String,
    /// Signals the body read when it last ran. Recorded per run, so an effect
    /// whose reads depend on a condition subscribes to what it actually touched.
    deps: HashSet<String>,
}

/// Pull `computed` and `effect` declarations out of a script.
///
/// Returns the script rhai should see, with every consumed line replaced by a
/// blank one so line numbers still match the file: a warning pointing at the
/// wrong line is worse than one pointing nowhere.
///
/// `computed x = expr;` becomes `let x = expr;`, which is what makes a computed
/// an ordinary signal, initialised in declaration order alongside the rest.
fn extract_reactives(script: &str) -> (String, Vec<Computed>, Vec<Effect>) {
    let mut cleaned = String::new();
    let mut computeds = Vec::new();
    let mut effects = Vec::new();

    let lines: Vec<&str> = script.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("computed ") {
            if let Some((name, expr)) = rest.split_once('=') {
                let name = name.trim();
                let expr = expr.trim().trim_end_matches(';').trim();
                if is_identifier(name) && !expr.is_empty() {
                    computeds.push(Computed {
                        name: name.to_string(),
                        expr: expr.to_string(),
                        deps: HashSet::new(),
                    });
                    // Declared, not stripped: the value has to exist before any
                    // binding reads it, and being a `let` is what makes it a
                    // signal the rest of the pipeline already understands.
                    cleaned.push_str(&format!("let {name} = {expr};\n"));
                    i += 1;
                    continue;
                }
            }
        }

        if trimmed == "effect {" || trimmed.starts_with("effect {") {
            // Take the block by counting braces, so an effect can hold an `if`.
            let mut depth = 0i32;
            let mut body = String::new();
            let mut j = i;
            let mut closed = false;
            while j < lines.len() {
                let l = lines[j];
                for c in l.chars() {
                    match c {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                let start = if j == i { l.find('{').map(|p| p + 1).unwrap_or(0) } else { 0 };
                body.push_str(&l[start..]);
                body.push('\n');
                cleaned.push('\n'); // keep the file's line numbering
                j += 1;
                if depth <= 0 {
                    closed = true;
                    break;
                }
            }
            if closed {
                // Drop the trailing `}` the loop consumed with the last line.
                let body = body.trim_end();
                let body = body.strip_suffix('}').unwrap_or(body).to_string();
                effects.push(Effect { body, deps: HashSet::new() });
                i = j;
                continue;
            }
            // Unterminated: leave it to rhai to complain about, with its lines.
            rux_style::warn_stylesheet(
                "an `effect {` block is never closed; it was ignored".to_string(),
            );
            i = j;
            continue;
        }

        cleaned.push_str(line);
        cleaned.push('\n');
        i += 1;
    }
    (cleaned, computeds, effects)
}

/// Whether `s` is a plain identifier, so `computed 2 + 2 = x;` is left for rhai
/// to reject rather than quietly becoming a declaration.
fn is_identifier(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// A listener body with the emitted payload bound to `event`.
///
/// Baked in as a `let` prelude rather than passed as an argument, the same
/// trick that carries an `r-for` row into an `@tap`: the body is statements the
/// caller wrote inline, not a function, so there is no parameter list to put it
/// in. `emit("change")` with no payload leaves `event` undeclared, so reading it
/// is a lookup failure rather than a silent empty value.
fn with_event(body: &str, payload: Option<&rux_reactive::Value>) -> String {
    match payload {
        Some(value) => format!("let event = {}; {body}", value.to_rhai_literal()),
        None => body.to_string(),
    }
}

/// Just the `fn` definitions from a component's script.
///
/// The complement of `rux-style`'s `component_statements`: functions are code
/// and are shared across instances, everything else is state and is not.
fn component_functions(script: &str) -> String {
    let mut out = String::new();
    let lines: Vec<&str> = script.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if !lines[i].trim().starts_with("fn ") {
            i += 1;
            continue;
        }
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
            out.push_str(lines[i]);
            out.push('\n');
            i += 1;
            if seen && depth <= 0 {
                break;
            }
        }
    }
    out
}

/// A resolved component import.
struct Import {
    /// Custom-element tag (last path segment, `_` → `-`).
    tag: String,
    /// File path relative to the importing document (`a::b` → `a/b.rux`).
    file: String,
}

/// Split `use a::b;` lines out of a script, returning the cleaned script (which
/// `rhai` can parse) and the resolved imports.
fn extract_imports(script: &str) -> (String, Vec<Import>) {
    let mut cleaned = String::new();
    let mut imports = Vec::new();

    for line in script.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("use ") {
            // A `use` must be its own statement on its own line; a path with
            // spaces or extra `;` is malformed, leave it for rhai to reject.
            if let Some(path) = rest.strip_suffix(';').map(str::trim).filter(|p| {
                !p.is_empty() && !p.contains(char::is_whitespace) && !p.contains(';')
            }) {
                let segments: Vec<&str> = path.split("::").collect();
                let file = format!("{}.rux", segments.join("/"));
                let tag = segments
                    .last()
                    .map(|s| s.replace('_', "-"))
                    .unwrap_or_default();
                imports.push(Import { tag, file });
                continue; // strip the import line
            }
        }
        cleaned.push_str(line);
        cleaned.push('\n');
    }
    (cleaned, imports)
}

/// Build the script engine and register host functions (the native-capability
/// boundary; a real app registers its own here).
fn build_engine(script: &str) -> Result<Engine, String> {
    let mut builder = Builder::new();
    builder.host_number("full", || 100.0);
    let mut engine = builder.build(script)?;
    // The route has to be in scope before the first build, because a `<router>`
    // reads it during that build. A document that declared `route` itself is
    // told rather than quietly overwritten on the first navigation.
    if engine.declares_route() {
        rux_script::warn_script(
            "`route` is the router's signal and is provided for you; a `let route` of your own \
             is overwritten on every navigation",
        );
    }
    engine.set_route(ROOT_PATH);
    Ok(engine)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every string the tree renders, for a failure message that says what was
    /// actually on screen rather than dumping the whole node.
    fn text_of(node: &LayoutNode) -> Vec<String> {
        let mut out: Vec<String> = node.text.iter().map(|t| t.text.clone()).collect();
        for child in &node.children {
            out.extend(text_of(child));
        }
        out
    }

    fn find_text(node: &LayoutNode, needle: &str) -> bool {
        if let Some(t) = &node.text {
            if t.text.contains(needle) {
                return true;
            }
        }
        node.children.iter().any(|c| find_text(c, needle))
    }

    #[test]
    fn loads_document_and_expands_imported_component() {
        // Self-contained fixtures (not the mutable examples): exercises import
        // resolution, component file loading, engine merge, and expansion.
        use std::fs;
        let dir = std::env::temp_dir().join(format!("rux_test_{}", std::process::id()));
        let comp_dir = dir.join("components");
        fs::create_dir_all(&comp_dir).unwrap();
        fs::write(
            comp_dir.join("stat.rux"),
            r#"<template><view><text>{{ label }}: {{ value }}</text></view></template>"#,
        )
        .unwrap();
        fs::write(
            dir.join("app.rux"),
            "<template><screen><stat :label=\"title\" :value=\"n\" /></screen></template>\n\
             <script>\n\
             use components::stat;\n\
             let title = signal(\"Battery\");\n\
             let n = signal(82);\n\
             </script>",
        )
        .unwrap();

        let doc = Document::load(dir.join("app.rux")).expect("load app");
        assert!(find_text(&doc.root, "Battery"), "component label prop rendered");
        assert!(find_text(&doc.root, "82"), "component value prop rendered");

        let _ = fs::remove_dir_all(&dir);
    }

    /// An included sheet styles the document, and the document's own `<style>`
    /// wins a tie. That order is the whole point: you include a palette in
    /// order to override part of it, and needing `!important` to do so would
    /// mean the include had been bolted on top rather than cascaded under.
    #[test]
    fn included_stylesheets_cascade_under_the_document() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!("rux_css_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("theme.css"),
            ".card { background: #ff0000; } .plain { background: #0000ff; }",
        )
        .unwrap();
        fs::write(
            dir.join("app.rux"),
            "<template><screen><view class=\"card\" /><view class=\"plain\" /></screen></template>\n\
             <style src=\"theme.css\">\n  .card { background: #00ff00; }\n</style>",
        )
        .unwrap();

        let doc = Document::load(dir.join("app.rux")).expect("load app");
        let bg = |i: usize| doc.root.children[i].style.background.clone();
        assert!(
            matches!(bg(0), Some(rux_layout::Background::Color(c)) if c.g == 1.0),
            "same specificity, so the document's own rule wins on source order"
        );
        assert!(
            matches!(bg(1), Some(rux_layout::Background::Color(c)) if c.b == 1.0),
            "and what the document says nothing about still comes from the include"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// A stylesheet that is not there fails the load, the same way a missing
    /// component does. A document that renders unstyled reads as a layout bug,
    /// which is a much longer walk back to the typo in the path.
    #[test]
    fn a_missing_stylesheet_fails_the_load() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!("rux_css_missing_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("app.rux"),
            "<template><screen /></template>\n<style src=\"nope.css\">.a{color:red}</style>",
        )
        .unwrap();

        let Err(err) = Document::load(dir.join("app.rux")) else {
            panic!("a document naming a stylesheet that is not there must not load");
        };
        assert!(err.contains("nope.css"), "names the file that is missing: {err}");

        let _ = fs::remove_dir_all(&dir);
    }

    /// From source there is no file, so there is nothing for the path to be
    /// relative to. The document still renders; it just says what it lost.
    #[test]
    fn an_include_from_source_warns_instead_of_failing() {
        let _ = take_warnings(); // the sinks are global; start from a known state
        let doc = Document::from_source(
            "<template><screen /></template>\n<style src=\"theme.css\">.a{color:red}</style>",
        )
        .expect("renders anyway");
        assert!(
            doc.diagnostics.warnings.iter().any(|w| w.message.contains("theme.css")),
            "the warning names the sheet that was ignored: {:?}",
            doc.diagnostics.warnings
        );
    }

    /// `<image src>` is relative to the .rux file, not the working directory,
    /// and the intrinsic size comes from the file itself so a sizeless image
    /// lays out at its natural dimensions.
    #[test]
    fn resolves_image_src_and_intrinsic_size() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!("rux_img_{}", std::process::id()));
        fs::create_dir_all(dir.join("assets")).unwrap();

        // A 2x1 PNG, written by the same decoder the painter uses.
        let png = dir.join("assets/dot.png");
        image::RgbaImage::from_pixel(2, 1, image::Rgba([255, 0, 0, 255]))
            .save(&png)
            .unwrap();
        fs::write(
            dir.join("app.rux"),
            r#"<template><screen><image src="assets/dot.png" /></screen></template>"#,
        )
        .unwrap();

        let doc = Document::load(dir.join("app.rux")).expect("load app");
        let img = doc.root.children[0].image.as_ref().expect("image node");
        assert_eq!(img.intrinsic, (2.0, 1.0));
        assert_eq!(Path::new(&img.src), png, "src resolved against the .rux dir");

        let _ = fs::remove_dir_all(&dir);
    }

    fn caret_of(node: &LayoutNode, model: &str) -> Option<usize> {
        if node.model.as_deref() == Some(model) {
            return node.children.first()?.text.as_ref()?.caret;
        }
        node.children.iter().find_map(|c| caret_of(c, model))
    }

    /// Moving focus must clear the caret in the input you left. It used to only
    /// ever *set* one, so the old input kept painting a caret until some
    /// unrelated rebuild happened to wipe it.
    #[test]
    fn focus_moves_the_caret_out_of_the_old_input() {
        let mut doc = Document::from_source(
            "<template><screen>             <input r-model=\"name\" /><input r-model=\"city\" />             </screen></template>
             <script>let name = signal(\"abc\"); let city = signal(\"xyz\");</script>",
        )
        .expect("load");

        doc.set_focus(Some(Focus::at("name", 2)));
        assert_eq!(caret_of(&doc.root, "name"), Some(2));
        assert_eq!(caret_of(&doc.root, "city"), None);

        // Focus the other field: the first one must lose its caret immediately,
        // with no rebuild in between.
        doc.set_focus(Some(Focus::at("city", 1)));
        assert_eq!(caret_of(&doc.root, "name"), None, "old input kept its caret");
        assert_eq!(caret_of(&doc.root, "city"), Some(1));

        // Tapping outside clears both.
        doc.set_focus(None);
        assert_eq!(caret_of(&doc.root, "name"), None);
        assert_eq!(caret_of(&doc.root, "city"), None);
    }

    fn selection_of(node: &LayoutNode, model: &str) -> Option<(usize, usize)> {
        if node.model.as_deref() == Some(model) {
            return node.children.first()?.text.as_ref()?.selection;
        }
        node.children.iter().find_map(|c| selection_of(c, model))
    }

    fn preedit_of(node: &LayoutNode, model: &str) -> Option<(usize, usize)> {
        if node.model.as_deref() == Some(model) {
            return node.children.first()?.text.as_ref()?.preedit;
        }
        node.children.iter().find_map(|c| preedit_of(c, model))
    }

    fn two_inputs() -> Document {
        Document::from_source(
            "<template><screen>             <input r-model=\"name\" /><input r-model=\"city\" />             </screen></template>
             <script>let name = signal(\"abc\"); let city = signal(\"xyz\");</script>",
        )
        .expect("load")
    }

    /// The selection is the range between anchor and caret, either way round, and
    /// only the focused input has one.
    #[test]
    fn selection_paints_only_in_the_focused_input() {
        let mut doc = two_inputs();

        doc.set_focus(Some(Focus { model: "name".into(), row: None, caret: 3, anchor: 1, preedit: None }));
        assert_eq!(selection_of(&doc.root, "name"), Some((1, 3)));
        assert_eq!(selection_of(&doc.root, "city"), None);

        // Dragging leftwards puts the caret *before* the anchor; same range.
        doc.set_focus(Some(Focus { model: "name".into(), row: None, caret: 1, anchor: 3, preedit: None }));
        assert_eq!(selection_of(&doc.root, "name"), Some((1, 3)));
    }

    /// The negative case, which is where the caret bug lived: moving focus must
    /// *clear* the old input's selection, not just set the new one's. A rebuild
    /// isn't required to notice.
    #[test]
    fn focus_moves_the_selection_out_of_the_old_input() {
        let mut doc = two_inputs();

        doc.set_focus(Some(Focus { model: "name".into(), row: None, caret: 3, anchor: 0, preedit: None }));
        assert_eq!(selection_of(&doc.root, "name"), Some((0, 3)));

        doc.set_focus(Some(Focus { model: "city".into(), row: None, caret: 2, anchor: 0, preedit: None }));
        assert_eq!(selection_of(&doc.root, "name"), None, "old input kept its selection");
        assert_eq!(selection_of(&doc.root, "city"), Some((0, 2)));

        doc.set_focus(None);
        assert_eq!(selection_of(&doc.root, "name"), None);
        assert_eq!(selection_of(&doc.root, "city"), None);
    }

    /// `r-key` stamps each row with what it stands for, and the stamp survives
    /// a reorder: after reversing the data, the row carrying key `b` is the one
    /// at the front. Nothing consumes this yet (see the note on
    /// `LayoutNode::key`), but it is the only thing in a document that says a
    /// row is the same row.
    #[test]
    fn a_key_identifies_a_row_across_a_reorder() {
        let mut doc = Document::from_source(
            "<template><screen>\
               <text r-for=\"row in rows\" r-key=\"row.id\">{{ row.text }}</text>\
             </screen></template>
             <script>\
               let rows = signal([\
                 #{ id: \"a\", text: \"alpha\" },\
                 #{ id: \"b\", text: \"bravo\" }\
               ]);\
               </script>",
        )
        .expect("load");
        let keys = |d: &Document| -> Vec<Option<String>> {
            d.root.children.iter().map(|c| c.key.clone()).collect()
        };
        assert_eq!(keys(&doc), vec![Some("a".into()), Some("b".into())]);

        assert!(
            doc.apply_handler(
                "rows = [#{ id: \"b\", text: \"bravo\" }, #{ id: \"a\", text: \"alpha\" }];"
            ),
            "the reorder changed a signal"
        );
        assert_eq!(
            keys(&doc),
            vec![Some("b".into()), Some("a".into())],
            "the keys moved with their rows"
        );
        assert!(find_text(&doc.root, "bravo"));
    }

    /// Every row of a list is bound to the same `r-model` text, so a model on
    /// its own cannot say which row the caret is in. Before the row was part of
    /// focus, setting the caret in one row put a caret in *every* row of the
    /// list at once.
    fn keyed_inputs() -> Document {
        Document::from_source(
            "<template><screen>\
               <view r-for=\"row in rows\" r-key=\"row.id\">\
                 <input r-model=\"draft\" />\
               </view>\
             </screen></template>
             <script>\
               let rows = signal([#{ id: \"a\" }, #{ id: \"b\" }]);\
               let draft = signal(\"hello\");\
             </script>",
        )
        .expect("load")
    }

    /// The caret inside a keyed row belongs to that row alone.
    #[test]
    fn only_the_focused_row_gets_a_caret() {
        let mut doc = keyed_inputs();
        doc.set_focus(Some(Focus::at_row("draft", Some("b".into()), 2)));

        let carets: Vec<Option<usize>> = doc
            .root
            .children
            .iter()
            .map(|row| caret_of(row, "draft"))
            .collect();
        assert_eq!(
            carets,
            vec![None, Some(2)],
            "the caret is in row b only, not in every row bound to `draft`"
        );
    }

    /// And it stays with its row when the list is reordered, rather than with
    /// the position the row used to hold. Nothing is remapped to achieve this:
    /// the identity *is* the row, so the caret lands wherever that row went.
    #[test]
    fn the_caret_follows_its_row_across_a_reorder() {
        let mut doc = keyed_inputs();
        doc.set_focus(Some(Focus::at_row("draft", Some("b".into()), 2)));

        assert!(
            doc.apply_handler("rows = [#{ id: \"b\" }, #{ id: \"a\" }];"),
            "the reorder changed a signal"
        );
        assert_eq!(doc.root.children[0].key.as_deref(), Some("b"), "row b is first now");

        let carets: Vec<Option<usize>> = doc
            .root
            .children
            .iter()
            .map(|row| caret_of(row, "draft"))
            .collect();
        assert_eq!(
            carets,
            vec![Some(2), None],
            "the caret moved with row b instead of staying in the first slot"
        );
    }

    /// A row's field can be read and written, which needs that row's loop
    /// variable in scope: the `r-model` is recorded as written, so
    /// `rows[row.at.to_int()].note` is not an expression without `row`. Reading
    /// it raw returned "" and warned `Variable not found`, and writing it set a
    /// scope variable *named* `rows[row.at.to_int()].note`, leaving the real
    /// target untouched. Typing into a row's field did nothing at all.
    #[test]
    fn a_rows_field_reads_and_writes_in_its_own_scope() {
        let mut doc = Document::from_source(
            "<template><screen>\
               <input r-for=\"row in rows\" r-key=\"row.id\" r-model=\"rows[row.at.to_int()].note\" />\
             </screen></template>
             <script>\
               let rows = signal([\
                 #{ id: \"a\", at: 0, note: \"alpha\" },\
                 #{ id: \"b\", at: 1, note: \"bravo\" }\
               ]);\
             </script>",
        )
        .expect("load");
        let model = "rows[row.at.to_int()].note";

        assert_eq!(doc.value_in(model, Some("a")), "alpha");
        assert_eq!(doc.value_in(model, Some("b")), "bravo", "each row reads its own value");

        doc.apply_edit_in(model, Some("b"), "bravo!");
        assert_eq!(doc.value_in(model, Some("b")), "bravo!", "the edit landed");
        assert_eq!(doc.value_in(model, Some("a")), "alpha", "and only in that row");
    }

    /// An `r-model` that is a path rather than a bare signal is written through
    /// too. This never worked, in or out of a list.
    #[test]
    fn a_path_model_is_assigned_not_shadowed() {
        let mut doc = Document::from_source(
            "<template><screen><input r-model=\"user.name\" /></screen></template>
             <script>let user = signal(#{ name: \"ada\" });</script>",
        )
        .expect("load");

        doc.apply_edit("user.name", "grace");
        assert_eq!(doc.value_in("user.name", None), "grace");
    }

    /// A value containing quotes and backslashes survives being written, since
    /// the write runs as script and the text is a person's typing.
    #[test]
    fn an_awkward_value_survives_the_round_trip() {
        let mut doc = two_inputs();
        let awkward = "she said \"hi\" \\ then left";
        doc.apply_edit("name", awkward);
        assert_eq!(doc.value_in("name", None), awkward);
    }

    /// Write a component with a `<slot />` and a document that fills it, in a
    /// temp dir, then load it. Returns the document.
    fn with_component(component: &str, app: &str) -> Document {
        use std::fs;
        let dir = std::env::temp_dir().join(format!(
            "rux_slot_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("components")).unwrap();
        fs::write(dir.join("components/card.rux"), component).unwrap();
        fs::write(dir.join("app.rux"), app).unwrap();
        let doc = Document::load(dir.join("app.rux")).expect("load");
        let _ = fs::remove_dir_all(&dir);
        doc
    }

    /// The point of scoping: two instances of one component are two separate
    /// sets of state. Their scripts used to be merged into the single shared
    /// script, so both `<counter>` elements counted the same number.
    #[test]
    fn two_instances_keep_their_own_state() {
        let doc = with_component(
            "<template><view class=\"card\">\
               <text>{{ count }}</text>\
               <view @tap=\"count = count + 1\"><text>add</text></view>\
             </view></template>\n\
             <script>\nlet count = signal(0);\n</script>",
            "<template><screen><card /><card /></screen></template>\n\
             <script>\nuse components::card;\n</script>",
        );
        // Two instances, two keys, and neither is a document signal.
        assert_eq!(doc.instances.len(), 2, "one entry per instance: {:?}", doc.instances);
        assert!(
            doc.instances.values().all(|i| i.state.iter().any(|(n, _)| n == "count")),
            "each holds its own `count`: {:?}",
            doc.instances
        );
    }

    /// Tapping inside one instance moves that instance's state and no other's.
    #[test]
    fn a_handler_moves_only_its_own_instance() {
        let mut doc = with_component(
            "<template><view>\
               <text>{{ label }}:{{ count }}</text>\
               <view @tap=\"count = count + 1\"><text>add</text></view>\
             </view></template>\n\
             <script>\nlet count = signal(0);\n</script>",
            "<template><screen>\
               <card :label=\"&quot;a&quot;\" /><card :label=\"&quot;b&quot;\" />\
             </screen></template>\n\
             <script>\nuse components::card;\n</script>",
        );
        assert!(find_text(&doc.root, "a:0"), "{:?}", text_of(&doc.root));
        assert!(find_text(&doc.root, "b:0"), "{:?}", text_of(&doc.root));

        // The second card's handler, found the way the shell finds it: by the
        // instance recorded on the node it was tapped on.
        let second = doc.root.children[1].clone();
        let button = second.children.iter().find(|c| c.on_tap.is_some()).expect("a tappable box");
        let (src, instance) = (button.on_tap.clone().unwrap(), button.instance.clone());
        assert!(instance.is_some(), "the node knows which instance it is in");

        assert!(doc.apply_handler_in(&src, instance.as_deref()), "the tap changed state");
        assert!(find_text(&doc.root, "b:1"), "the tapped card counted: {:?}", text_of(&doc.root));
        assert!(
            find_text(&doc.root, "a:0"),
            "and the other one did not: {:?}",
            text_of(&doc.root)
        );
    }

    /// Tap the one tappable box in a subtree, the way the shell would: with the
    /// instance recorded on the node it was tapped on.
    fn tap(doc: &mut Document, node: &LayoutNode) -> bool {
        fn find(node: &LayoutNode) -> Option<&LayoutNode> {
            if node.on_tap.is_some() {
                return Some(node);
            }
            node.children.iter().find_map(find)
        }
        let button = find(node).expect("a tappable box").clone();
        doc.apply_handler_in(&button.on_tap.clone().unwrap(), button.instance.as_deref())
    }

    /// The other half of props: something comes back out. Without this a
    /// component can only ever be told things, so every piece of state a caller
    /// cares about has to be hoisted out of the component that owns it.
    #[test]
    fn an_emitted_event_runs_the_callers_handler() {
        let mut doc = with_component(
            "<template><view>\
               <view @tap=\"emit(&quot;bumped&quot;)\"><text>add</text></view>\
             </view></template>",
            "<template><screen>\
               <text>total {{ total }}</text>\
               <card @bumped=\"total = total + 1\" />\
             </screen></template>\n\
             <script>\nuse components::card;\nlet total = signal(0);\n</script>",
        );
        let card = doc.root.children[1].clone();
        assert!(tap(&mut doc, &card), "the tap reached the caller");
        assert!(find_text(&doc.root, "total 1"), "{:?}", text_of(&doc.root));
        let card = doc.root.children[1].clone();
        assert!(tap(&mut doc, &card), "and again");
        assert!(find_text(&doc.root, "total 2"), "{:?}", text_of(&doc.root));
    }

    /// A payload arrives as `event`, so a component can say *what* happened
    /// rather than only that something did.
    #[test]
    fn an_event_carries_its_payload() {
        let mut doc = with_component(
            "<template><view>\
               <view @tap=\"emit(&quot;picked&quot;, label)\"><text>pick</text></view>\
             </view></template>",
            "<template><screen>\
               <text>chose {{ chosen }}</text>\
               <card :label=\"&quot;blue&quot;\" @picked=\"chosen = event\" />\
             </screen></template>\n\
             <script>\nuse components::card;\nlet chosen = signal(\"nothing\");\n</script>",
        );
        let card = doc.root.children[1].clone();
        assert!(tap(&mut doc, &card), "the tap reached the caller");
        assert!(find_text(&doc.root, "chose blue"), "{:?}", text_of(&doc.root));
    }

    /// The listener body is the *caller's* code and must run in the caller's
    /// scope. Run in the instance's, it would write to a variable of the same
    /// name inside the component, or to nothing at all, and look like it worked.
    #[test]
    fn a_listener_runs_in_the_callers_scope() {
        let mut doc = with_component(
            "<template><view>\
               <text>inner {{ total }}</text>\
               <view @tap=\"emit(&quot;bumped&quot;)\"><text>add</text></view>\
             </view></template>\n\
             <script>\nlet total = signal(100);\n</script>",
            "<template><screen>\
               <text>outer {{ total }}</text>\
               <card @bumped=\"total = total + 1\" />\
             </screen></template>\n\
             <script>\nuse components::card;\nlet total = signal(0);\n</script>",
        );
        let card = doc.root.children[1].clone();
        assert!(tap(&mut doc, &card));
        assert!(find_text(&doc.root, "outer 1"), "the caller's own: {:?}", text_of(&doc.root));
        assert!(
            find_text(&doc.root, "inner 100"),
            "the component's like-named state is untouched: {:?}",
            text_of(&doc.root)
        );
    }

    /// A component offers events; a caller takes the ones it wants. Emitting
    /// one nobody listens to is ordinary, and must not fail or warn.
    #[test]
    fn an_event_with_no_listener_is_ignored() {
        let _ = take_warnings();
        let mut doc = with_component(
            "<template><view>\
               <view @tap=\"count = count + 1; emit(&quot;bumped&quot;)\"><text>add</text></view>\
               <text>{{ count }}</text>\
             </view></template>\n\
             <script>\nlet count = signal(0);\n</script>",
            "<template><screen><card /></screen></template>\n\
             <script>\nuse components::card;\n</script>",
        );
        let card = doc.root.children[0].clone();
        assert!(tap(&mut doc, &card), "the handler's own work still happened");
        assert!(find_text(&doc.root, "1"), "{:?}", text_of(&doc.root));
        assert!(take_warnings().is_empty(), "an unheard event is not a mistake");
    }

    /// `emit` outside a component has nobody to tell. Silence there would be a
    /// handler that plainly expected something to happen and did nothing.
    #[test]
    fn emit_outside_a_component_warns() {
        let _ = take_warnings();
        let mut doc = Document::from_source(
            "<template><screen><view @tap=\"emit(&quot;bumped&quot;)\"><text>go</text></view></screen></template>",
        )
        .expect("loads");
        let button = doc.root.children[0].clone();
        doc.apply_handler_in(&button.on_tap.clone().unwrap(), None);
        let warnings = take_warnings();
        assert!(
            warnings.iter().any(|w| w.message.contains("no caller")),
            "the warning says why nothing happened: {warnings:?}"
        );
    }

    /// Write a document with a router over three views, in a temp dir.
    ///
    /// `home` and `settings` are plain; `user` reads an `:id` captured from the
    /// path and counts taps, so a test can tell whether a view's state survived
    /// a navigation.
    fn with_router(app: &str) -> Document {
        use std::fs;
        let dir = std::env::temp_dir().join(format!(
            "rux_router_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("components")).unwrap();
        fs::write(dir.join("components/home.rux"), "<template><text>the home page</text></template>")
            .unwrap();
        fs::write(
            dir.join("components/settings.rux"),
            "<template><text>settings live here</text></template>",
        )
        .unwrap();
        fs::write(
            dir.join("components/user.rux"),
            "<template><view>\
               <text>user {{ id }} seen {{ seen }}</text>\
               <view @tap=\"seen = seen + 1\"><text>look</text></view>\
             </view></template>\n\
             <script>\nlet seen = signal(0);\n</script>",
        )
        .unwrap();
        fs::write(
            dir.join("components/missing.rux"),
            "<template><text>no such page</text></template>",
        )
        .unwrap();
        fs::write(dir.join("app.rux"), app).unwrap();
        let doc = Document::load(dir.join("app.rux")).expect("load");
        let _ = fs::remove_dir_all(&dir);
        doc
    }

    /// The standard three-route app, used by most of the router tests.
    fn router_app() -> Document {
        with_router(
            "<template><screen>\
               <text to=\"/\">home</text>\
               <text to=\"/settings\">settings</text>\
               <router>\
                 <route path=\"/\" view=\"home\" />\
                 <route path=\"/settings\" view=\"settings\" />\
                 <route path=\"/user/:id\" view=\"user\" />\
                 <route fallback view=\"missing\" />\
               </router>\
             </screen></template>\n\
             <script>\nuse components::home;\nuse components::settings;\n\
             use components::user;\nuse components::missing;\n</script>",
        )
    }

    /// The whole point: one path renders one view, and only that one.
    #[test]
    fn the_router_renders_the_matching_route() {
        let mut doc = router_app();
        assert!(find_text(&doc.root, "the home page"), "{:?}", text_of(&doc.root));
        assert!(!find_text(&doc.root, "settings live here"), "and not the others");

        assert!(doc.navigate("/settings"), "navigating changed something");
        assert!(find_text(&doc.root, "settings live here"), "{:?}", text_of(&doc.root));
        assert!(!find_text(&doc.root, "the home page"), "the old view is gone");
    }

    /// A `:segment` is captured and handed to the view as a prop, which is what
    /// makes a list-detail app expressible at all.
    #[test]
    fn a_path_parameter_reaches_the_view() {
        let mut doc = router_app();
        doc.navigate("/user/7");
        assert!(find_text(&doc.root, "user 7 seen 0"), "{:?}", text_of(&doc.root));
        doc.navigate("/user/12");
        assert!(find_text(&doc.root, "user 12 seen 0"), "{:?}", text_of(&doc.root));
    }

    /// A path nothing matches renders the fallback, not a blank screen.
    #[test]
    fn an_unmatched_path_falls_back() {
        let mut doc = router_app();
        doc.navigate("/nowhere");
        assert!(find_text(&doc.root, "no such page"), "{:?}", text_of(&doc.root));
    }

    /// A trailing slash is not a different path. Anyone typing one by hand
    /// produces both spellings and means the same place.
    #[test]
    fn a_trailing_slash_is_the_same_path() {
        let mut doc = router_app();
        doc.navigate("/settings/");
        assert!(find_text(&doc.root, "settings live here"), "{:?}", text_of(&doc.root));
    }

    /// `to="/path"` navigates when tapped: the spec's promise since v0.1.
    #[test]
    fn a_link_navigates_when_tapped() {
        let mut doc = router_app();
        let link = doc.root.children[1].clone();
        assert_eq!(link.access.role, rux_layout::AccessRole::Link, "announced as a link");
        assert!(doc.apply_handler(&link.on_tap.clone().expect("a link taps")));
        assert_eq!(doc.route(), "/settings");
        assert!(find_text(&doc.root, "settings live here"), "{:?}", text_of(&doc.root));
    }

    /// Back and forward walk the same list, and forward is only available after
    /// going back.
    #[test]
    fn history_walks_both_ways() {
        let mut doc = router_app();
        doc.navigate("/settings");
        doc.navigate("/user/3");
        assert!(!doc.forward(), "nothing ahead of the newest entry");

        assert!(doc.back(), "back to settings");
        assert_eq!(doc.route(), "/settings");
        assert!(doc.back(), "back to home");
        assert_eq!(doc.route(), "/");
        assert!(!doc.back(), "and no further");

        assert!(doc.forward(), "forward again");
        assert_eq!(doc.route(), "/settings");
        assert!(find_text(&doc.root, "settings live here"), "{:?}", text_of(&doc.root));
    }

    /// Going somewhere new after going back drops what was ahead, which is the
    /// behaviour a back button has everywhere else.
    #[test]
    fn a_new_path_after_going_back_drops_the_forward_entries() {
        let mut doc = router_app();
        doc.navigate("/settings");
        doc.back();
        doc.navigate("/user/1");
        assert!(!doc.forward(), "settings is no longer ahead: {:?}", doc.history);
        assert!(doc.back(), "but home is still behind");
        assert_eq!(doc.route(), "/");
    }

    /// Navigating to where we already are is not a visit. Otherwise tapping the
    /// current link fills the history with repeats and Back does nothing.
    #[test]
    fn navigating_to_the_current_path_is_not_a_visit() {
        let mut doc = router_app();
        doc.navigate("/settings");
        assert!(!doc.navigate("/settings"), "no change, so nothing to repaint");
        assert!(doc.back());
        assert_eq!(doc.route(), "/", "one Back is enough to leave");
    }

    /// A view keeps its state while you are on it, and starts fresh when you
    /// come back to it. Instance state is keyed by template position, so
    /// *keeping* it across a visit is what would happen by accident.
    #[test]
    fn a_route_view_is_fresh_on_a_second_visit() {
        let mut doc = router_app();
        doc.navigate("/user/7");

        let view = doc.root.children[2].clone();
        let button = view.children.iter().find(|c| c.on_tap.is_some()).expect("the look button");
        let (src, instance) = (button.on_tap.clone().unwrap(), button.instance.clone());
        doc.apply_handler_in(&src, instance.as_deref());
        assert!(find_text(&doc.root, "user 7 seen 1"), "state moves while here: {:?}", text_of(&doc.root));

        doc.navigate("/settings");
        doc.navigate("/user/7");
        assert!(
            find_text(&doc.root, "user 7 seen 0"),
            "and starts over on return: {:?}",
            text_of(&doc.root)
        );
    }

    /// `:current` is how a nav bar shows where you are. It reads the route, so
    /// the link restyles on navigation without the tree being rebuilt.
    #[test]
    fn the_current_link_matches_the_current_pseudo() {
        let mut doc = with_router(
            "<template><screen>\
               <text to=\"/\" class=\"nav\">home</text>\
               <text to=\"/settings\" class=\"nav\">settings</text>\
               <router><route path=\"/\" view=\"home\" />\
                 <route path=\"/settings\" view=\"settings\" /></router>\
             </screen></template>\n\
             <style>.nav { color: #888888; } .nav:current { color: #ff0000; }</style>\n\
             <script>\nuse components::home;\nuse components::settings;\n</script>",
        );
        // As a tuple, since `Rgba` is deliberately not `PartialEq`.
        let lit = |n: &LayoutNode| -> Option<(f32, f32, f32)> {
            n.text.as_ref().map(|t| (t.color.r, t.color.g, t.color.b))
        };
        assert_ne!(lit(&doc.root.children[0]), lit(&doc.root.children[1]), "one of them is current");
        let home_on_home = lit(&doc.root.children[0]);

        doc.navigate("/settings");
        assert_eq!(
            lit(&doc.root.children[1]),
            home_on_home,
            "the current colour moved to the settings link"
        );
        assert_ne!(lit(&doc.root.children[0]), home_on_home, "and off the home link");
    }

    /// A route naming a view that was never imported is a mistake worth saying
    /// out loud: the screen would otherwise just be empty.
    #[test]
    fn a_route_naming_an_unimported_view_warns() {
        let _ = take_warnings();
        let doc = with_router(
            "<template><screen><router>\
               <route path=\"/\" view=\"nowhere\" />\
             </router></screen></template>\n\
             <script>\nuse components::home;\n</script>",
        );
        // A load drains the sink into its own diagnostics, which is where the
        // overlay reads from.
        let warnings = &doc.diagnostics.warnings;
        assert!(
            warnings.iter().any(|w| w.message.contains("nowhere")),
            "the warning names the view: {warnings:?}"
        );
    }

    /// A component's state is not a document signal, so the app cannot reach in
    /// and read it by name. That isolation is the reason a component can be
    /// used in a second app at all.
    #[test]
    fn component_state_is_not_a_document_signal() {
        let mut doc = with_component(
            "<template><view><text>{{ count }}</text></view></template>\n\
             <script>\nlet count = signal(7);\n</script>",
            "<template><screen><card /><text>{{ count }}</text></screen></template>\n\
             <script>\nuse components::card;\n</script>",
        );
        assert!(find_text(&doc.root, "7"), "the component sees its own: {:?}", text_of(&doc.root));
        // The document's own `{{ count }}` has nothing to read, and says so
        // rather than borrowing the component's.
        assert_eq!(doc.value_in("count", None), "", "{:?}", text_of(&doc.root));
    }

    /// The isolation is one-directional, and the docs claimed otherwise. A
    /// component's *script* runs in a fresh scope, so its `let`s are private.
    /// Its template and handlers do not: they run against the document's scope
    /// with the instance's names pushed on top, so an un-shadowed document
    /// signal is both readable and writable from inside. The router depends on
    /// it (`{{ route }}` inside a route view).
    #[test]
    fn a_component_reads_and_writes_an_unshadowed_document_signal() {
        let mut doc = with_component(
            "<template><view>\
               <text>saw {{ theme }}</text>\
               <view @tap=\"theme = &quot;dark&quot;\"><text>go</text></view>\
             </view></template>\n\
             <script>\nlet count = signal(0);\n</script>",
            "<template><screen><card /></screen></template>\n\
             <script>\nuse components::card;\nlet theme = signal(\"light\");\n</script>",
        );
        assert!(
            find_text(&doc.root, "saw light"),
            "the document's signal is visible inside: {:?}",
            text_of(&doc.root)
        );
        let card = doc.root.children[0].clone();
        assert!(tap(&mut doc, &card), "the handler wrote a document signal");
        assert_eq!(doc.value_in("theme", None), "dark");
        assert!(find_text(&doc.root, "saw dark"), "{:?}", text_of(&doc.root));
    }

    /// The whole point of a slot: a component can wrap markup it never saw.
    /// Before this, the children written between the tags were silently thrown
    /// away, so a component could only ever be a fixed shape.
    #[test]
    fn a_slot_renders_the_callers_children() {
        let doc = with_component(
            "<template><view class=\"card\"><text>title</text><slot /></view></template>",
            "<template><screen>\
               <card><text>from the caller</text></card>\
             </screen></template>\n\
             <script>\nuse components::card;\n</script>",
        );
        assert!(find_text(&doc.root, "title"), "the component's own markup: {:?}", text_of(&doc.root));
        assert!(
            find_text(&doc.root, "from the caller"),
            "and the children it was handed: {:?}",
            text_of(&doc.root)
        );
    }

    /// Slot content is the caller's, so it is evaluated in the caller's scope,
    /// which includes the caller's own instance state the component cannot see.
    #[test]
    fn slot_content_reads_the_callers_scope() {
        let doc = with_component(
            "<template><view><slot /></view></template>",
            "<template><screen>\
               <card><text>{{ greeting }}</text></card>\
             </screen></template>\n\
             <script>\nuse components::card;\nlet greeting = signal(\"hello there\");\n</script>",
        );
        assert!(
            find_text(&doc.root, "hello there"),
            "the caller's signal resolved inside the slot: {:?}",
            text_of(&doc.root)
        );
    }

    /// An unfilled slot falls back to its own children, as in HTML, so a
    /// component can offer a default without the caller writing one.
    #[test]
    fn an_empty_slot_falls_back_to_its_own_children() {
        let doc = with_component(
            "<template><view><slot><text>nothing here yet</text></slot></view></template>",
            "<template><screen><card /></screen></template>\n\
             <script>\nuse components::card;\n</script>",
        );
        assert!(
            find_text(&doc.root, "nothing here yet"),
            "the fallback showed: {:?}",
            text_of(&doc.root)
        );
    }

    /// The slot leaves no box of its own: the caller's children sit exactly
    /// where the `<slot />` was, so a component adds no wrapper nobody wrote.
    #[test]
    fn a_slot_adds_no_node_of_its_own() {
        let doc = with_component(
            "<template><view><slot /></view></template>",
            "<template><screen>\
               <card><text>a</text><text>b</text></card>\
             </screen></template>\n\
             <script>\nuse components::card;\n</script>",
        );
        // screen > view(card root) > the two texts, with nothing in between.
        let card = &doc.root.children[0];
        assert_eq!(card.children.len(), 2, "two children, no wrapper: {:?}", text_of(card));
        assert!(card.children.iter().all(|c| c.text.is_some()));
    }

    /// A number reads the same whether it went through `{{ }}` or through
    /// string concatenation in a handler. Every Rux number is an f64, so rhai
    /// rendered a whole one as "32.0" beside the same value shown as "32" a
    /// line above it.
    #[test]
    fn numbers_render_the_same_in_text_and_in_script() {
        let doc = Document::from_source(
            "<template><screen>\
               <text>{{ total }}</text>\
               <text>{{ \"total is \" + total }}</text>\
               <text>{{ half }}</text>\
               <text>{{ \"half is \" + half }}</text>\
             </screen></template>
             <script>\n\
               let total = signal(32);\n\
               let half = signal(2.5);\n\
             </script>",
        )
        .expect("load");
        let shown = text_of(&doc.root);
        assert!(shown.contains(&"32".to_string()), "{shown:?}");
        assert!(shown.contains(&"total is 32".to_string()), "{shown:?}");
        // A fraction still shows its fraction; only the ".0" tail goes.
        assert!(shown.contains(&"2.5".to_string()), "{shown:?}");
        assert!(shown.contains(&"half is 2.5".to_string()), "{shown:?}");
    }

    /// A computed is derived state: it is written once, read anywhere, and
    /// keeps itself current. Before this the only "computed" was a `{{ }}`
    /// expression, so the same derivation was retyped at every use.
    #[test]
    fn a_computed_tracks_what_it_reads() {
        let mut doc = Document::from_source(
            "<template><screen><text>{{ total }} for {{ count }}</text></screen></template>
             <script>\
               let count = signal(2);\n\
               let price = signal(10);\n\
               computed total = count * price;\n\
             </script>",
        )
        .expect("load");
        assert!(find_text(&doc.root, "20 for 2"), "computed on load: {:?}", doc.root);

        assert!(doc.apply_handler("count = 3;"), "the handler changed a signal");
        assert!(find_text(&doc.root, "30 for 3"), "and the computed followed");
    }

    /// A computed may read one declared above it, and the refresh is a single
    /// pass in declaration order, so a chain settles in one go.
    #[test]
    fn a_computed_may_read_an_earlier_computed() {
        let mut doc = Document::from_source(
            "<template><screen><text>{{ shout }}</text></screen></template>
             <script>\n\
               let name = signal(\"ada\");\n\
               computed greeting = \"hi \" + name;\n\
               computed shout = greeting + \"!\";\n\
             </script>",
        )
        .expect("load");
        assert!(find_text(&doc.root, "hi ada!"));

        assert!(doc.apply_handler("name = \"grace\";"));
        assert!(find_text(&doc.root, "hi grace!"), "the whole chain refreshed");
    }

    /// An effect runs on load, so it can *establish* something rather than only
    /// react to a later change, and again whenever what it read changes.
    #[test]
    fn an_effect_runs_on_load_and_on_change() {
        let mut doc = Document::from_source(
            "<template><screen><text>{{ mirror }}</text></screen></template>
             <script>\n\
               let count = signal(1);\n\
               let mirror = signal(0);\n\
               effect {\n\
                 mirror = count * 100;\n\
               }\n\
             </script>",
        )
        .expect("load");
        assert!(find_text(&doc.root, "100"), "ran once on load: {:?}", text_of(&doc.root));

        assert!(doc.apply_handler("count = 2;"));
        assert!(
            find_text(&doc.root, "200"),
            "ran again when count changed: {:?}",
            text_of(&doc.root)
        );
    }

    /// An effect only re-runs for what it actually read. A signal it never
    /// touches must not wake it, or "runs when its dependencies change" is just
    /// "runs on everything".
    #[test]
    fn an_effect_ignores_signals_it_never_read() {
        let mut doc = Document::from_source(
            "<template><screen><text>{{ mirror }}</text></screen></template>
             <script>\n\
               let watched = signal(1);\n\
               let other = signal(1);\n\
               let mirror = signal(0);\n\
               effect {\n\
                 mirror = watched * 100;\n\
               }\n\
             </script>",
        )
        .expect("load");
        // The subscription itself, which is the thing under test: an effect
        // that subscribed to everything would still pass a behavioural check.
        assert_eq!(doc.effects.len(), 1);
        assert!(doc.effects[0].deps.contains("watched"), "it read `watched`");
        assert!(!doc.effects[0].deps.contains("other"), "it never read `other`");
        assert!(
            !doc.effects[0].deps.contains("mirror"),
            "writing a signal is not reading it, or every effect would feed itself"
        );

        assert!(doc.apply_handler("other = 2;"));
        assert!(find_text(&doc.root, "100"), "an unread signal leaves it alone");

        assert!(doc.apply_handler("watched = 3;"));
        assert!(find_text(&doc.root, "300"), "the one it read wakes it");
    }

    /// An effect that writes the signal it reads settles instead of looping,
    /// because its own writes do not wake it.
    #[test]
    fn an_effect_is_not_woken_by_its_own_writes() {
        let _ = take_warnings();
        let mut doc = Document::from_source(
            "<template><screen><text>{{ n }}</text></screen></template>
             <script>\n\
               let n = signal(0);\n\
               effect {\n\
                 n = n + 1;\n\
               }\n\
             </script>",
        )
        .expect("load");
        assert!(find_text(&doc.root, "1"), "ran once: {:?}", text_of(&doc.root));

        assert!(doc.apply_handler("n = 100;"));
        assert!(
            find_text(&doc.root, "100"),
            "an outside write is not chased by the effect that owns n: {:?}",
            text_of(&doc.root)
        );
        assert!(doc.diagnostics.warnings.is_empty(), "{:?}", doc.diagnostics.warnings);
    }

    /// Two effects feeding each other still loop, and that is stopped and
    /// reported: a window that hangs says nothing at all.
    #[test]
    fn effects_that_feed_each_other_are_stopped_and_reported() {
        let _ = take_warnings();
        let doc = Document::from_source(
            "<template><screen><text>{{ a }}</text></screen></template>
             <script>\n\
               let a = signal(0);\n\
               let b = signal(0);\n\
               effect {\n\
                 b = a + 1;\n\
               }\n\
               effect {\n\
                 a = b + 1;\n\
               }\n\
             </script>",
        )
        .expect("load");
        // Reaching here at all is half the assertion: the loop is bounded.
        assert!(
            doc.diagnostics.warnings.iter().any(|w| w.message.contains("re-triggering")),
            "the cycle is named rather than hung on: {:?}",
            doc.diagnostics.warnings
        );
    }

    /// A `<select>` in each row is a separate select. The layout has to say so,
    /// or the shell opens the first row's dropdown wherever you tapped, draws it
    /// over that row, and writes the chosen option into it.
    #[test]
    fn each_rows_select_is_its_own() {
        let doc = Document::from_source(
            "<template><screen>\
               <input r-for=\"row in rows\" r-key=\"row.id\" type=\"select\" \
                      r-model=\"pick\" :options=\"row.options\" />\
             </screen></template>
             <script>\
               let pick = signal(\"a\");\
               let rows = signal([\
                 #{ id: \"one\", options: [\"a\", \"b\"] },\
                 #{ id: \"two\", options: [\"c\", \"d\"] }\
               ]);\
             </script>",
        )
        .expect("load");

        let mut measure = |_: &rux_layout::TextContent, _: Option<f32>| (10.0, 10.0);
        let out = rux_layout::layout(&doc.root, 800.0, 600.0, &mut measure);
        let rows: Vec<Option<String>> = out.selects.iter().map(|s| s.row.clone()).collect();
        assert_eq!(
            rows,
            vec![Some("one".to_string()), Some("two".to_string())],
            "two selects, each stamped with the row it is in"
        );
        // Same model text on both, which is exactly why the row is needed.
        assert_eq!(out.selects[0].model, out.selects[1].model);
        assert_eq!(out.selects[0].options, vec!["a", "b"]);
        assert_eq!(out.selects[1].options, vec!["c", "d"]);
    }

    /// Two rows claiming one identity is worse than none, so it is said out
    /// loud. Same for a key on an element that is not a row at all.
    #[test]
    fn keys_that_cannot_work_are_warned_about() {
        let _ = take_warnings();
        let doc = Document::from_source(
            "<template><screen>\
               <text r-for=\"row in rows\" r-key=\"row.id\">{{ row.id }}</text>\
             </screen></template>
             <script>let rows = signal([#{ id: \"a\" }, #{ id: \"a\" }]);</script>",
        )
        .expect("load");
        assert!(
            doc.diagnostics.warnings.iter().any(|w| w.message.contains("duplicate key")),
            "duplicate keys are reported: {:?}",
            doc.diagnostics.warnings
        );

        let _ = take_warnings();
        let doc = Document::from_source(
            "<template><screen><text r-key=\"x\">hi</text></screen></template>",
        )
        .expect("load");
        assert!(
            doc.diagnostics.warnings.iter().any(|w| w.message.contains("without `r-for`")),
            "a key with no list is reported: {:?}",
            doc.diagnostics.warnings
        );
    }

    /// A collapsed selection is no selection: a plain caret must not paint a
    /// zero-width highlight.
    #[test]
    fn a_collapsed_selection_is_none() {
        let mut doc = two_inputs();
        doc.set_focus(Some(Focus::at("name", 2)));
        assert_eq!(caret_of(&doc.root, "name"), Some(2));
        assert_eq!(selection_of(&doc.root, "name"), None);
    }

    /// Both caret and selection are re-applied after a rebuild, the whole-tree
    /// rebuild throws the tree away, so anything ephemeral must be put back.
    #[test]
    fn selection_survives_a_rebuild() {
        let mut doc = two_inputs();
        doc.set_focus(Some(Focus { model: "name".into(), row: None, caret: 3, anchor: 1, preedit: None }));
        doc.rebuild();
        assert_eq!(selection_of(&doc.root, "name"), Some((1, 3)));
        assert_eq!(caret_of(&doc.root, "name"), Some(3));
        assert_eq!(selection_of(&doc.root, "city"), None);
    }

    /// Text being composed through an input method is marked on the focused
    /// input only, and is cleared the same way a selection is when focus moves.
    /// Without the clearing, leaving a field mid-composition left the underline
    /// behind on text that had since been committed.
    #[test]
    fn a_composition_marks_only_the_focused_input() {
        let mut doc = two_inputs();

        doc.set_focus(Some(Focus {
            model: "name".into(),
            row: None,
            caret: 3,
            anchor: 3,
            preedit: Some((1, 3)),
        }));
        assert_eq!(preedit_of(&doc.root, "name"), Some((1, 3)));
        assert_eq!(preedit_of(&doc.root, "city"), None);

        doc.set_focus(Some(Focus::at("city", 1)));
        assert_eq!(preedit_of(&doc.root, "name"), None, "old input kept its composition");
        assert_eq!(preedit_of(&doc.root, "city"), None);
    }

    /// A composition outlives the rebuild that showing it causes: the shell
    /// writes the composed text into the bound signal, and that edit can rebuild
    /// the tree, so a range applied before it must be put back after.
    #[test]
    fn a_composition_survives_a_rebuild() {
        let mut doc = two_inputs();
        doc.set_focus(Some(Focus {
            model: "name".into(),
            row: None,
            caret: 2,
            anchor: 2,
            preedit: Some((0, 2)),
        }));
        doc.rebuild();
        assert_eq!(preedit_of(&doc.root, "name"), Some((0, 2)));
    }

    fn patch_doc() -> Document {
        // `n` is displayed only in a `{{ }}` text binding (patchable); `name` is
        // read by an input's r-model value (structural → forces a rebuild).
        Document::from_source(
            "<template><screen><text class=\"c\">{{ n }}</text><input r-model=\"name\" /></screen></template>
             <script>let n = signal(0); let name = signal(\"hi\");</script>",
        )
        .expect("load")
    }

    /// A display-only change patches the text node in place, no rebuild, so the
    /// caret in an unrelated input survives without any restore pass running.
    #[test]
    fn patch_updates_text_and_preserves_caret() {
        let mut doc = patch_doc();
        doc.set_focus(Some(Focus::at("name", 1)));

        let changed = doc.engine_mut().run_handler_tracked("n = n + 1");
        assert!(doc.patch(&changed), "a display-only change patches in place");
        assert_eq!(doc.root.children[0].text.as_ref().unwrap().text, "1");
        // The caret survived: patch never touched focus, and no rebuild happened.
        assert_eq!(caret_of(&doc.root, "name"), Some(1));
    }

    /// Changing an input-bound signal patches the input's value in place (it is a
    /// patchable value binding, not structural), leaving the sibling display alone.
    #[test]
    fn patch_updates_input_value_in_place() {
        let mut doc = patch_doc();
        let changed = doc.engine_mut().run_handler_tracked("name = \"yo\"");
        assert!(doc.patch(&changed), "an input value change patches in place");
        // The input (child 1) shows the new value; the `{{ n }}` display is untouched.
        assert_eq!(doc.root.children[1].children[0].text.as_ref().unwrap().text, "yo");
        assert_eq!(doc.root.children[0].text.as_ref().unwrap().text, "0");
    }

    fn input_text(doc: &Document) -> &str {
        // screen → input → text child.
        &doc.root.children[0].children[0].text.as_ref().unwrap().text
    }

    /// A keystroke patches the input's shown value in place, `patch` returns true
    /// (no rebuild needed) and the text updates, and the caret survives.
    #[test]
    fn typing_patches_the_input_value_in_place() {
        let mut doc = Document::from_source(
            "<template><screen><input r-model=\"name\" placeholder=\"type…\" /></screen></template>
             <script>let name = signal(\"ab\");</script>",
        )
        .expect("load");
        doc.set_focus(Some(Focus::at("name", 2)));
        assert_eq!(input_text(&doc), "ab");

        doc.engine_mut().set_string("name", "abc");
        let changed: HashSet<String> = std::iter::once("name".to_string()).collect();
        assert!(doc.patch(&changed), "value-only input edit patches in place");
        assert_eq!(input_text(&doc), "abc");

        // Emptying the field falls back to the placeholder (patched, not rebuilt).
        doc.engine_mut().set_string("name", "");
        assert!(doc.patch(&changed));
        assert_eq!(input_text(&doc), "type…");
    }

    /// `:options` rewrites a select's option list in place, no rebuild.
    #[test]
    fn options_patch_in_place() {
        let mut doc = Document::from_source(
            "<template><screen><input type=\"select\" r-model=\"fruit\" :options=\"fruits\" /></screen></template>
             <script>let fruit = signal(\"a\"); let fruits = signal([\"a\", \"b\"]);</script>",
        )
        .expect("load");
        assert_eq!(doc.root.children[0].options.as_ref().unwrap().len(), 2);

        let changed = doc.engine_mut().run_handler_tracked("fruits = [\"a\", \"b\", \"c\"]");
        assert!(doc.patch(&changed), "an :options change patches in place");
        assert_eq!(doc.root.children[0].options.as_ref().unwrap().len(), 3, "list grew in place");
    }

    /// A component prop change reconciles the instance subtree in place: the
    /// re-expanded component shows the new prop value, no wholesale rebuild.
    #[test]
    fn component_prop_reconciles_in_place() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!("rux_prop_{}", std::process::id()));
        let comp_dir = dir.join("components");
        fs::create_dir_all(&comp_dir).unwrap();
        fs::write(
            comp_dir.join("stat.rux"),
            r#"<template><view><text>{{ value }}</text></view></template>"#,
        )
        .unwrap();
        fs::write(
            dir.join("app.rux"),
            "<template><screen><stat :value=\"n\" /></screen></template>\n\
             <script>\nuse components::stat;\nlet n = signal(1);\n</script>",
        )
        .unwrap();

        let mut doc = Document::load(dir.join("app.rux")).expect("load app");
        assert!(find_text(&doc.root, "1"), "prop starts at 1");
        let changed = doc.engine_mut().run_handler_tracked("n = 2");
        assert!(doc.patch(&changed), "a component prop change reconciles in place");
        assert!(find_text(&doc.root, "2"), "component re-expanded with the new prop");

        let _ = fs::remove_dir_all(&dir);
    }

    /// Toggling a checkbox reconciles just that node (its checked style + mark) in
    /// place, and a caret on an input elsewhere survives with no whole-tree
    /// restore.
    #[test]
    fn toggle_reconciles_and_preserves_an_outside_caret() {
        let mut doc = Document::from_source(
            "<template><screen>\
               <input r-model=\"name\" />\
               <input type=\"checkbox\" class=\"box\" r-model=\"on\" />\
             </screen></template>
             <style>.box { background: #000000; } .box.checked { background: #00ff00; }</style>
             <script>let name = signal(\"ab\"); let on = signal(false);</script>",
        )
        .expect("load");
        doc.set_focus(Some(Focus::at("name", 1)));
        let green = |n: &LayoutNode| matches!(&n.style.background, Some(rux_layout::Background::Color(c)) if c.g == 1.0);
        assert!(!green(&doc.root.children[1]), "unchecked → not green");

        let changed = doc.engine_mut().run_handler_tracked("on = true");
        assert!(doc.patch(&changed), "a toggle reconciles in place");
        assert!(green(&doc.root.children[1]), "checked → .box.checked (green) applied");
        assert!(doc.root.children[1].children.len() == 1, "checkmark added");
        // Only the toggle node was spliced; the sibling input node is untouched, so
        // its caret persists by identity.
        assert_eq!(caret_of(&doc.root, "name"), Some(1));
    }

    // ── Pointer state (`:hover` / `:active`) ────────────────────────────────

    // ── Diagnostics / dev overlay ───────────────────────────────────────────

    /// A document that builds but whose CSS partly does nothing reports it,
    /// instead of the silence that made unknown CSS the worst failure mode here.
    #[test]
    fn warnings_are_collected_for_the_overlay() {
        let doc = Document::from_source(
            "<template><screen><view class=\"card\" /></screen></template>
             <style>.card { filter: blur(2px); background: var(--nope); }</style>",
        )
        .expect("load");
        let warnings = &doc.diagnostics().warnings;
        assert!(
            warnings.iter().any(|w| w.message.contains("filter")),
            "unhonored property reported: {warnings:?}"
        );
        assert!(
            warnings.iter().any(|w| w.message.contains("--nope")),
            "undefined var reported: {warnings:?}"
        );
        assert!(doc.diagnostics().error.is_none(), "the document still built");
    }

    /// A clean document reports nothing, so the overlay stays out of the way.
    #[test]
    fn a_clean_document_has_no_diagnostics() {
        let doc = Document::from_source(
            "<template><screen><view class=\"card\" /></screen></template>
             <style>.card { background: #313244; }</style>",
        )
        .expect("load");
        assert!(doc.diagnostics().is_empty(), "{:?}", doc.diagnostics());
    }

    /// A failed reload keeps the tree that is on screen and marks it stale,
    /// a typo mid-edit must not blank the window.
    #[test]
    fn a_failed_reload_keeps_the_last_good_tree() {
        let mut doc = Document::from_source(
            "<template><screen><text>hello</text></screen></template>",
        )
        .expect("load");
        let before = doc.root.children.len();

        doc.set_load_error("parse error at line 6, column 13: mismatched closing tag");
        assert_eq!(doc.root.children.len(), before, "the tree is untouched");
        assert!(doc.diagnostics().error.is_some());
        assert!(doc.diagnostics().stale, "what's on screen predates the error");
    }

    /// Loading a good document over a broken one clears the error.
    #[test]
    fn a_successful_reload_clears_the_error() {
        let mut doc = Document::from_source("<template><screen><text>old</text></screen></template>")
            .expect("load");
        doc.set_load_error("something was wrong");

        let fresh = Document::from_source("<template><screen><text>new</text></screen></template>")
            .expect("load");
        doc.replace_with(fresh);
        assert!(doc.diagnostics().error.is_none(), "error cleared");
        assert!(!doc.diagnostics().stale);
        assert_eq!(doc.root.children[0].text.as_ref().unwrap().text, "new");
    }

    /// The window owns the viewport, not the file, a reload must not reset it,
    /// or a hot-reload in a narrow window would come back with desktop styling.
    #[test]
    fn a_reload_keeps_the_window_viewport() {
        let mut doc = media_doc();
        doc.set_viewport(Viewport { width: 480.0, height: 800.0 });
        assert!(is_red(&doc.root.children[0]));

        let fresh = Document::from_source(
            "<template><screen><view class=\"card\" /></screen></template>
             <style>
               .card { background: #00ff00; }
               @media (max-width: 600px) { .card { background: #ff0000; } }
             </style>",
        )
        .expect("load");
        doc.replace_with(fresh);
        assert!(
            is_red(&doc.root.children[0]),
            "still narrow after the reload, so the @media rule still applies"
        );
    }

    // ── @media / viewport ───────────────────────────────────────────────────

    fn media_doc() -> Document {
        Document::from_source(
            "<template><screen><view class=\"card\" /></screen></template>
             <style>
               .card { background: #00ff00; }
               @media (max-width: 600px) { .card { background: #ff0000; } }
             </style>",
        )
        .expect("load")
    }

    fn is_red(n: &LayoutNode) -> bool {
        matches!(&n.style.background, Some(rux_layout::Background::Color(c)) if c.r == 1.0 && c.g == 0.0)
    }

    /// Crossing a breakpoint re-cascades; crossing back restores.
    #[test]
    fn resize_across_a_breakpoint_restyles() {
        let mut doc = media_doc();
        assert!(!is_red(&doc.root.children[0]), "the default viewport is wide");

        assert!(doc.set_viewport(Viewport { width: 480.0, height: 800.0 }), "breakpoint crossed");
        assert!(is_red(&doc.root.children[0]), "narrow → the @media rule applies");

        assert!(doc.set_viewport(Viewport { width: 1000.0, height: 800.0 }), "crossed back");
        assert!(!is_red(&doc.root.children[0]), "wide again → the base rule");
    }

    /// The case that has to stay free: a resize crossing no breakpoint reports no
    /// change, so dragging a window edge doesn't re-cascade on every pixel.
    #[test]
    fn resize_within_a_breakpoint_is_not_a_change() {
        let mut doc = media_doc();
        doc.set_viewport(Viewport { width: 400.0, height: 800.0 });
        assert!(
            !doc.set_viewport(Viewport { width: 500.0, height: 800.0 }),
            "still under 600px, nothing to redo"
        );
        assert!(is_red(&doc.root.children[0]), "and the styling is still correct");
    }

    /// A document with no `@media` at all never re-cascades on resize.
    #[test]
    fn resize_does_nothing_without_media_queries() {
        let mut doc = Document::from_source(
            "<template><screen><view class=\"card\" /></screen></template>
             <style>.card { background: #00ff00; }</style>",
        )
        .expect("load");
        assert!(!doc.set_viewport(Viewport { width: 320.0, height: 480.0 }));
        assert!(!doc.set_viewport(Viewport { width: 1600.0, height: 900.0 }));
    }

    /// Two sibling cards, only the second of which holds an input, plus a
    /// `:hover` rule that repaints a card green.
    fn hover_doc() -> Document {
        Document::from_source(
            "<template><screen>\
               <view class=\"card\"><text>one</text></view>\
               <view class=\"card\"><input r-model=\"name\" /></view>\
             </screen></template>
             <style>.card { background: #000000; } .card:hover { background: #00ff00; }</style>
             <script>let name = signal(\"ab\");</script>",
        )
        .expect("load")
    }

    fn hovering(path: &[usize]) -> InteractionState {
        InteractionState { hovered: Some(path.to_vec()), ..InteractionState::default() }
    }

    fn is_green(n: &LayoutNode) -> bool {
        matches!(&n.style.background, Some(rux_layout::Background::Color(c)) if c.g == 1.0)
    }

    /// The hovered element restyles, its unhovered sibling does not, and leaving
    /// puts it back, the negative case is the point.
    #[test]
    fn hover_restyles_only_the_hovered_element() {
        let mut doc = hover_doc();
        assert!(!is_green(&doc.root.children[0]), "nothing hovered → no green");

        assert!(doc.set_interaction(hovering(&[0])), "entering a card restyles");
        assert!(is_green(&doc.root.children[0]), "hovered card is green");
        assert!(!is_green(&doc.root.children[1]), "its sibling is NOT");

        assert!(doc.set_interaction(InteractionState::default()), "leaving restyles");
        assert!(!is_green(&doc.root.children[0]), "hover ends → back to black");
    }

    /// The pointer moving *within* the same element is not a state change, so it
    /// must not restyle anything, this is the every-mouse-move path.
    #[test]
    fn same_hover_target_is_not_a_change() {
        let mut doc = hover_doc();
        assert!(doc.set_interaction(hovering(&[0])));
        assert!(
            !doc.set_interaction(hovering(&[0])),
            "re-reporting the same target does no work"
        );
    }

    /// Hover moves between siblings while a caret sits in an input elsewhere: the
    /// caret survives, because only the diverging subtree is spliced.
    #[test]
    fn hover_change_preserves_a_caret_elsewhere() {
        let mut doc = hover_doc();
        doc.set_focus(Some(Focus::at("name", 1)));
        assert_eq!(caret_of(&doc.root, "name"), Some(1));

        assert!(doc.set_interaction(hovering(&[0])));
        assert_eq!(caret_of(&doc.root, "name"), Some(1), "caret survives a hover change");
        assert!(is_green(&doc.root.children[0]));
    }

    /// Clearing the pointer state (the pointer left the window) un-styles what was
    /// hovered or pressed. Found by driving it: leaving the window fires
    /// `CursorLeft`, not a `CursorMoved`, so a hovered button stayed lit after the
    /// pointer was gone. This is the "assert it is *cleared*" half of the rule.
    #[test]
    fn clearing_pointer_state_unstyles_the_hovered_element() {
        let mut doc = hover_doc();
        doc.set_interaction(InteractionState {
            hovered: Some(vec![0]),
            active: Some(vec![0]),
            ..InteractionState::default()
        });
        assert!(is_green(&doc.root.children[0]));

        assert!(doc.set_interaction(InteractionState::default()), "clearing restyles");
        assert!(!is_green(&doc.root.children[0]), "nothing is hovered any more");
    }

    /// `:hover` holds for the whole chain under the pointer, as in CSS: hovering a
    /// child leaves its ancestor hovered too.
    #[test]
    fn hover_applies_to_the_ancestor_chain() {
        let mut doc = Document::from_source(
            "<template><screen>\
               <view class=\"card\"><view class=\"inner\"><text>x</text></view></view>\
             </screen></template>
             <style>\
               .card { background: #000000; } .card:hover { background: #00ff00; }\
               .inner:hover { background: #0000ff; }\
             </style>",
        )
        .expect("load");
        // Pointer over the inner box (path [0, 0]).
        assert!(doc.set_interaction(hovering(&[0, 0])));
        assert!(is_green(&doc.root.children[0]), "the ancestor card is hovered too");
        let inner = &doc.root.children[0].children[0];
        assert!(
            matches!(&inner.style.background, Some(rux_layout::Background::Color(c)) if c.b == 1.0),
            "the inner box is hovered"
        );
    }

    /// A document with no pointer-state rules emits no state regions, so hover
    /// costs nothing at all.
    #[test]
    fn no_pointer_rules_means_no_state_regions() {
        let doc = Document::from_source(
            "<template><screen><view class=\"card\"><text>x</text></view></screen></template>
             <style>.card { background: #000000; }</style>",
        )
        .expect("load");
        fn any_marked(n: &LayoutNode) -> bool {
            n.state_path.is_some() || n.children.iter().any(any_marked)
        }
        assert!(!any_marked(&doc.root), "no :hover/:active rule → nothing to track");
    }

    /// The element a `:hover` rule could match carries a path, so the layout emits
    /// a region the shell can hit-test.
    #[test]
    fn hoverable_elements_are_marked_for_the_shell() {
        let doc = hover_doc();
        assert_eq!(doc.root.children[0].state_path.as_deref(), Some(&[0][..]));
        assert_eq!(doc.root.children[1].state_path.as_deref(), Some(&[1][..]));
        assert!(doc.root.state_path.is_none(), "the screen has no :hover rule");
    }

    /// `r-show` flips the node's `hidden` flag in place, no shape change, no
    /// rebuild, both ways.
    #[test]
    fn r_show_toggles_hidden_in_place() {
        let mut doc = Document::from_source(
            "<template><screen><text r-show=\"on\">hi</text></screen></template>
             <script>let on = signal(true);</script>",
        )
        .expect("load");
        assert!(!doc.root.children[0].hidden, "on=true → visible");

        let changed = doc.engine_mut().run_handler_tracked("on = false");
        assert!(doc.patch(&changed), "r-show change patches in place");
        assert!(doc.root.children[0].hidden, "on=false → hidden");

        let changed = doc.engine_mut().run_handler_tracked("on = true");
        assert!(doc.patch(&changed));
        assert!(!doc.root.children[0].hidden, "on=true → visible again");
    }

    /// An `r-if` toggling patches its owning subtree in place, and a caret on an
    /// input in a *different* subtree survives untouched, with no whole-tree
    /// `apply_focus`. This is the reconciliation payoff.
    #[test]
    fn r_if_reconciles_and_preserves_an_outside_caret() {
        let mut doc = Document::from_source(
            "<template><screen>\
               <view class=\"top\"><input r-model=\"name\" /></view>\
               <view class=\"list\"><text r-if=\"show\">secret</text></view>\
             </screen></template>
             <script>let name = signal(\"ab\"); let show = signal(false);</script>",
        )
        .expect("load");
        doc.set_focus(Some(Focus::at("name", 1)));
        assert_eq!(caret_of(&doc.root, "name"), Some(1));
        assert!(!find_text(&doc.root, "secret"), "hidden while show=false");

        // Reveal the r-if branch: reconciles the `.list` subtree only.
        let changed = doc.engine_mut().run_handler_tracked("show = true");
        assert!(doc.patch(&changed), "an r-if change reconciles in place");
        assert!(find_text(&doc.root, "secret"), "branch now shown");
        // The input is in `.top`, an untouched subtree, its caret persists with no
        // whole-tree restore.
        assert_eq!(caret_of(&doc.root, "name"), Some(1), "outside caret survived");

        // And hiding it again removes the branch.
        let changed = doc.engine_mut().run_handler_tracked("show = false");
        assert!(doc.patch(&changed));
        assert!(!find_text(&doc.root, "secret"));
        assert_eq!(caret_of(&doc.root, "name"), Some(1));
    }

    /// An `r-for` list change reconciles the row count in place.
    #[test]
    fn r_for_reconciles_row_count() {
        let mut doc = Document::from_source(
            "<template><screen><view class=\"list\"><text r-for=\"n in nums\">{{ n }}</text></view></screen></template>
             <script>let nums = signal([1, 2]);</script>",
        )
        .expect("load");
        assert_eq!(doc.root.children[0].children.len(), 2, "two rows initially");

        let changed = doc.engine_mut().run_handler_tracked("nums = [1, 2, 3, 4]");
        assert!(doc.patch(&changed), "an r-for change reconciles in place");
        assert_eq!(doc.root.children[0].children.len(), 4, "grew to four rows");
        assert!(find_text(&doc.root, "4"), "new row content present");
    }

    /// A label with `for="id"` inherits the `@tap` of the input with that `id`, so
    /// tapping the label toggles the input, even though the label doesn't wrap it.
    #[test]
    fn label_for_inherits_the_targets_tap() {
        let doc = Document::from_source(
            "<template><screen>\
               <input type=\"checkbox\" id=\"chk\" r-model=\"on\" />\
               <text for=\"chk\">Remember me</text>\
             </screen></template>
             <script>let on = signal(false);</script>",
        )
        .expect("load");
        // The label (child 1) picks up the checkbox's auto-generated toggle handler.
        assert_eq!(
            doc.root.children[1].on_tap.as_deref(),
            Some("on = !on"),
            "label with for= inherits the checkbox's @tap"
        );
        // An authored @tap on a label is not overridden.
        let doc2 = Document::from_source(
            "<template><screen>\
               <input type=\"checkbox\" id=\"chk\" r-model=\"on\" />\
               <text for=\"chk\" @tap=\"on = true\">Set</text>\
             </screen></template>
             <script>let on = signal(false);</script>",
        )
        .expect("load");
        assert_eq!(doc2.root.children[1].on_tap.as_deref(), Some("on = true"));
    }

    /// A label whose `for=` targets a *text* input (no `@tap`) gets a `focus_model`
    /// instead, so the shell focuses that input when the label is tapped.
    #[test]
    fn label_for_focuses_a_text_input() {
        let doc = Document::from_source(
            "<template><screen>\
               <input id=\"nm\" r-model=\"name\" />\
               <text for=\"nm\">Name</text>\
             </screen></template>
             <script>let name = signal(\"\");</script>",
        )
        .expect("load");
        let label = &doc.root.children[1];
        assert_eq!(label.on_tap, None, "a text-input label has no tap handler");
        assert_eq!(
            label.focus_model.as_deref(),
            Some("name"),
            "label focuses the text input's model"
        );
    }

    fn bg_rgb(n: &LayoutNode) -> Option<(f32, f32, f32)> {
        match &n.style.background {
            Some(rux_layout::Background::Color(c)) => Some((c.r, c.g, c.b)),
            _ => None,
        }
    }

    /// `:class` feeds a signal-driven class into the cascade, and a change to that
    /// signal reconciles the node's style in place.
    #[test]
    fn dynamic_class_reconciles() {
        let mut doc = Document::from_source(
            "<template><screen><view class=\"chip\" :class=\"tone\" /></screen></template>
             <style>.hot { background: #ff0000; } .cool { background: #0000ff; }</style>
             <script>let tone = signal(\"hot\");</script>",
        )
        .expect("load");
        assert_eq!(bg_rgb(&doc.root.children[0]), Some((1.0, 0.0, 0.0)), ":class=hot → .hot");

        let changed = doc.engine_mut().run_handler_tracked("tone = \"cool\"");
        assert!(doc.patch(&changed), ":class change reconciles in place");
        assert_eq!(bg_rgb(&doc.root.children[0]), Some((0.0, 0.0, 1.0)), "reconciled to .cool");
    }

    /// `:style` with a rhai backtick template literal (string interpolation) sets an
    /// inline style, overriding the cascade, and reconciles on change.
    #[test]
    fn dynamic_inline_style_interpolates_and_reconciles() {
        let mut doc = Document::from_source(
            "<template><screen><view :style=\"`background: ${col}`\" /></screen></template>
             <script>let col = signal(\"#00ff00\");</script>",
        )
        .expect("load");
        assert_eq!(bg_rgb(&doc.root.children[0]), Some((0.0, 1.0, 0.0)), ":style set green");

        let changed = doc.engine_mut().run_handler_tracked("col = \"#ff0000\"");
        assert!(doc.patch(&changed));
        assert_eq!(bg_rgb(&doc.root.children[0]), Some((1.0, 0.0, 0.0)), "reconciled to red");
    }

    /// The chip example: `:style` reads the `r-for` loop variable, so each item gets
    /// its own colour; the `r-for` drives it (no per-node binding needed).
    #[test]
    fn r_for_chip_styles() {
        let doc = Document::from_source(
            "<template><screen><view class=\"chips\">\
               <view class=\"chip\" r-for=\"c in colors\" :style=\"`background: ${c}`\"><text>{{ c }}</text></view>\
             </view></screen></template>
             <script>let colors = signal([\"#ff0000\", \"#00ff00\"]);</script>",
        )
        .expect("load");
        let chips = &doc.root.children[0];
        assert_eq!(bg_rgb(&chips.children[0]), Some((1.0, 0.0, 0.0)), "first chip red");
        assert_eq!(bg_rgb(&chips.children[1]), Some((0.0, 1.0, 0.0)), "second chip green");
    }

    /// `:class` object/conditional form (`#{ hot: cond }`), keys whose value is
    /// truthy become classes; a change flips them and reconciles.
    #[test]
    fn conditional_class_object_form() {
        let mut doc = Document::from_source(
            "<template><screen><view class=\"chip\" :class=\"#{ hot: warm, cool: !warm }\" /></screen></template>
             <style>.hot { background: #ff0000; } .cool { background: #0000ff; }</style>
             <script>let warm = signal(true);</script>",
        )
        .expect("load");
        assert_eq!(bg_rgb(&doc.root.children[0]), Some((1.0, 0.0, 0.0)), "warm → .hot");

        let changed = doc.engine_mut().run_handler_tracked("warm = false");
        assert!(doc.patch(&changed), "conditional class change reconciles");
        assert_eq!(bg_rgb(&doc.root.children[0]), Some((0.0, 0.0, 1.0)), "!warm → .cool");
    }

    /// The shipped `css-showcase.rux` (the `:class`/`:style` chip demo) loads and
    /// builds, a smoke test that the example stays valid.
    #[test]
    fn css_showcase_example_builds() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/css-showcase.rux");
        let doc = Document::load(path).expect("css-showcase.rux builds");
        assert!(find_text(&doc.root, "teal"), "a :style-coloured chip rendered");
    }

    /// `:style` object form (`#{ background: c }`), each entry a declaration.
    #[test]
    fn style_object_form() {
        let doc = Document::from_source(
            "<template><screen><view :style=\"#{ background: col }\" /></screen></template>
             <script>let col = signal(\"#00ff00\");</script>",
        )
        .expect("load");
        assert_eq!(bg_rgb(&doc.root.children[0]), Some((0.0, 1.0, 0.0)), ":style object → green");
    }

    /// A checked box gets a synthetic `checked` class, so its checked look is
    /// plain CSS. A radio matches on its `value`.
    #[test]
    fn checked_toggles_get_a_checked_class() {
        let doc = Document::from_source(
            "<template><screen>             <input type=\"checkbox\" class=\"box\" r-model=\"on\" />             <input type=\"radio\" class=\"box\" r-model=\"plan\" value=\"pro\" />             <input type=\"radio\" class=\"box\" r-model=\"plan\" value=\"free\" />             </screen></template>
             <style>.box { background: #000000; } .box.checked { background: #00ff00; }</style>
             <script>let on = signal(true); let plan = signal(\"pro\");</script>",
        )
        .expect("load");

        let green = |n: &LayoutNode| {
            matches!(&n.style.background, Some(rux_layout::Background::Color(c)) if c.g == 1.0)
        };
        let boxes = &doc.root.children;
        assert!(green(&boxes[0]), "checked checkbox should match .checked");
        assert!(green(&boxes[1]), "radio whose value == signal is checked");
        assert!(!green(&boxes[2]), "the other radio is not checked");

        // ...and the checked ones carry a mark, the unchecked one doesn't.
        assert_eq!(boxes[0].children.len(), 1);
        assert_eq!(boxes[1].children.len(), 1);
        assert_eq!(boxes[2].children.len(), 0);
    }
}

