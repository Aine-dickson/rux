//! Rux runtime — milestones M2–M9.
//!
//! The document model: loads a `.rux` file, resolves its `use` component imports
//! (loading each imported `.rux`), builds the script [`Engine`] (merging the main
//! and component scripts, registering host functions), and builds the renderable
//! tree with bindings, directives, and component expansions resolved. Running an
//! `@tap` handler mutates engine state; `rebuild` refreshes the tree.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rux_layout::Node as LayoutNode;
use rux_parser::Sfc;
use rux_script::{Builder, Engine};
use rux_style::BindingRegistry;

/// A loaded `.rux` document: parsed source, imported components (by tag), the
/// script engine, and the current tree.
pub struct Document {
    sfc: Sfc,
    components: HashMap<String, Sfc>,
    engine: Engine,
    /// Directory the document was loaded from — `<image src>` resolves against it.
    base: PathBuf,
    /// The focused input, with its caret and selection, if any. Re-applied on
    /// every rebuild so both survive a state change.
    focus: Option<Focus>,
    /// Where each patchable text binding lives and which signals force a rebuild —
    /// refreshed on every full build. Lets [`Document::patch`] update value
    /// bindings in place instead of throwing the tree away.
    registry: BindingRegistry,
    pub root: LayoutNode,
}

/// Which input has keyboard focus, and where its caret and selection are.
///
/// The selection is the range between `anchor` (where it started) and `caret`
/// (where it has been dragged/extended to); `anchor == caret` means no selection,
/// just a caret. Either may be the smaller — dragging leftwards puts the caret
/// before the anchor — so consumers normalize with [`Focus::range`].
#[derive(Clone, Debug, PartialEq)]
pub struct Focus {
    pub model: String,
    pub caret: usize,
    pub anchor: usize,
}

impl Focus {
    /// A plain caret with nothing selected.
    pub fn at(model: impl Into<String>, caret: usize) -> Self {
        let model = model.into();
        Self { model, caret, anchor: caret }
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
/// it paints them — and clear every other input's.
///
/// Clearing matters: this runs against the *existing* tree when focus moves, not
/// only against a freshly built one. Setting without clearing left the caret
/// showing in the input you just left, until some unrelated rebuild wiped it.
/// The selection is one more thing that can be left behind the same way.
fn apply_focus(node: &mut LayoutNode, focus: Option<&Focus>) {
    if node.model.is_some() {
        if let Some(text) = node.children.first_mut().and_then(|c| c.text.as_mut()) {
            let mine = focus.filter(|f| node.model.as_deref() == Some(f.model.as_str()));
            // An empty input shows its placeholder; the caret still sits at 0.
            text.caret = mine.map(|f| f.caret.min(text.text.len()));
            text.selection = mine.filter(|f| !f.is_collapsed()).map(|f| {
                let (start, end) = f.range();
                (start.min(text.text.len()), end.min(text.text.len()))
            });
        }
    }
    for child in &mut node.children {
        apply_focus(child, focus);
    }
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

impl Document {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let src = std::fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
        let sfc = rux_parser::parse_sfc(&src).map_err(|e| e.to_string())?;

        // Resolve `use module::component;` imports relative to this file.
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let (main_script, imports) = extract_imports(&sfc.script);

        let mut components = HashMap::new();
        let mut combined_script = main_script;
        for import in imports {
            let comp_path = base.join(&import.file);
            let comp_src = std::fs::read_to_string(&comp_path)
                .map_err(|e| format!("reading component {}: {e}", comp_path.display()))?;
            let comp_sfc = rux_parser::parse_sfc(&comp_src).map_err(|e| e.to_string())?;
            let (comp_script, _nested) = extract_imports(&comp_sfc.script);
            // Merge the component's (pure) functions into the shared engine.
            combined_script.push('\n');
            combined_script.push_str(&comp_script);
            components.insert(import.tag, comp_sfc);
        }

        let mut engine = build_engine(&combined_script)?;
        let (mut root, registry) = rux_style::build_styled_tree_tracked(&sfc, &components, &mut engine)?;
        resolve_images(&mut root, base);
        Ok(Self {
            sfc,
            components,
            engine,
            base: base.to_path_buf(),
            focus: None,
            registry,
            root,
        })
    }

    /// Process `.rux` source with no import resolution (used for fallbacks/tests).
    pub fn from_source(src: &str) -> Result<Self, String> {
        let sfc = rux_parser::parse_sfc(src).map_err(|e| e.to_string())?;
        let (main_script, _imports) = extract_imports(&sfc.script);
        let mut engine = build_engine(&main_script)?;
        let (mut root, registry) = rux_style::build_styled_tree_tracked(&sfc, &HashMap::new(), &mut engine)?;
        let base = PathBuf::from(".");
        resolve_images(&mut root, &base);
        Ok(Self {
            sfc,
            components: HashMap::new(),
            engine,
            base,
            focus: None,
            registry,
            root,
        })
    }

    /// The script engine, for running `@tap` handlers.
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    /// Focus an input (by `r-model`), with its caret and selection. `None` clears.
    pub fn set_focus(&mut self, focus: Option<Focus>) {
        self.focus = focus;
        apply_focus(&mut self.root, self.focus.as_ref());
    }

    /// Rebuild the layout tree from the engine's current state.
    pub fn rebuild(&mut self) {
        if let Ok((mut root, registry)) =
            rux_style::build_styled_tree_tracked(&self.sfc, &self.components, &mut self.engine)
        {
            resolve_images(&mut root, &self.base);
            apply_focus(&mut root, self.focus.as_ref());
            self.registry = registry;
            self.root = root;
        }
    }

    /// Apply a set of changed signals *in place* where possible: re-evaluate the
    /// text bindings that read them and write the new strings into their nodes,
    /// without rebuilding the tree (so ephemeral state — caret, scroll — survives
    /// untouched). Returns `false` when the change can't be patched — it touched a
    /// signal that drives structure, an attribute, an input value, or a component
    /// prop — in which case the caller must [`rebuild`](Self::rebuild). Nothing is
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
    /// deps changed and write them into their nodes — no shape change.
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
        // `r-show` only flips paint on/off — rewrite the `hidden` bool in place.
        for binding in &self.registry.show {
            if binding.deps.is_disjoint(changed) {
                continue;
            }
            let visible = self.engine.eval_bool(&binding.cond, &binding.locals);
            if let Some(node) = node_at_mut(&mut self.root, &binding.path) {
                node.hidden = !visible;
            }
        }
        // `:src` — rewrite the image source and re-resolve it (path + intrinsic size).
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
        // `:options` — rewrite a select's option list in place.
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

        let Ok((mut fresh_root, fresh_reg)) =
            rux_style::build_styled_tree_tracked(&self.sfc, &self.components, &mut self.engine)
        else {
            return;
        };
        resolve_images(&mut fresh_root, &self.base);
        // Structural parents: replace the affected parent's children wholesale.
        for p in &roots {
            let Some(fresh) = node_at(&fresh_root, p) else { continue };
            let fresh_children = fresh.children.clone();
            if let Some(live) = node_at_mut(&mut self.root, p) {
                live.children = fresh_children;
                // Put the caret back only within this rebuilt subtree.
                apply_focus(live, self.focus.as_ref());
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
                if let Some(live) = node_at_mut(&mut self.root, p) {
                    *live = fresh_node;
                    apply_focus(live, self.focus.as_ref());
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
        self.engine.set_string(model, value);
        let changed: HashSet<String> = std::iter::once(model.to_string()).collect();
        self.apply_change(&changed);
    }

    /// Run an `@tap` handler and reflect its effect the cheapest correct way:
    /// patch the changed bindings in place, falling back to a full rebuild only
    /// when the change is structural. Returns whether anything changed, so the
    /// shell knows whether to repaint.
    pub fn apply_handler(&mut self, src: &str) -> bool {
        let changed = self.engine.run_handler_tracked(src);
        if changed.is_empty() {
            return false;
        }
        self.apply_change(&changed);
        true
    }

    /// Reflect a set of changed signals: patch in place, or rebuild when the change
    /// is structural. `RUX_TRACE=1` prints which path was taken, so the reactivity
    /// behavior is observable while driving (the pixels are identical either way).
    fn apply_change(&mut self, changed: &HashSet<String>) {
        let patched = self.patch(changed);
        if !patched {
            self.rebuild();
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

/// Follow a child-index path from the root to a node, mutably.
fn node_at_mut<'a>(root: &'a mut LayoutNode, path: &[usize]) -> Option<&'a mut LayoutNode> {
    let mut node = root;
    for &i in path {
        node = node.children.get_mut(i)?;
    }
    Some(node)
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
            // spaces or extra `;` is malformed — leave it for rhai to reject.
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
    builder.build(script)
}

#[cfg(test)]
mod tests {
    use super::*;

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

        doc.set_focus(Some(Focus { model: "name".into(), caret: 3, anchor: 1 }));
        assert_eq!(selection_of(&doc.root, "name"), Some((1, 3)));
        assert_eq!(selection_of(&doc.root, "city"), None);

        // Dragging leftwards puts the caret *before* the anchor; same range.
        doc.set_focus(Some(Focus { model: "name".into(), caret: 1, anchor: 3 }));
        assert_eq!(selection_of(&doc.root, "name"), Some((1, 3)));
    }

    /// The negative case, which is where the caret bug lived: moving focus must
    /// *clear* the old input's selection, not just set the new one's. A rebuild
    /// isn't required to notice.
    #[test]
    fn focus_moves_the_selection_out_of_the_old_input() {
        let mut doc = two_inputs();

        doc.set_focus(Some(Focus { model: "name".into(), caret: 3, anchor: 0 }));
        assert_eq!(selection_of(&doc.root, "name"), Some((0, 3)));

        doc.set_focus(Some(Focus { model: "city".into(), caret: 2, anchor: 0 }));
        assert_eq!(selection_of(&doc.root, "name"), None, "old input kept its selection");
        assert_eq!(selection_of(&doc.root, "city"), Some((0, 2)));

        doc.set_focus(None);
        assert_eq!(selection_of(&doc.root, "name"), None);
        assert_eq!(selection_of(&doc.root, "city"), None);
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

    /// Both caret and selection are re-applied after a rebuild — the whole-tree
    /// rebuild throws the tree away, so anything ephemeral must be put back.
    #[test]
    fn selection_survives_a_rebuild() {
        let mut doc = two_inputs();
        doc.set_focus(Some(Focus { model: "name".into(), caret: 3, anchor: 1 }));
        doc.rebuild();
        assert_eq!(selection_of(&doc.root, "name"), Some((1, 3)));
        assert_eq!(caret_of(&doc.root, "name"), Some(3));
        assert_eq!(selection_of(&doc.root, "city"), None);
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

    /// A display-only change patches the text node in place — no rebuild — so the
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

    /// A keystroke patches the input's shown value in place — `patch` returns true
    /// (no rebuild needed) and the text updates — and the caret survives.
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

    /// `:options` rewrites a select's option list in place — no rebuild.
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
    /// place — and a caret on an input elsewhere survives with no whole-tree
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

    /// `r-show` flips the node's `hidden` flag in place — no shape change, no
    /// rebuild — both ways.
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

    /// An `r-if` toggling patches its owning subtree in place — and a caret on an
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
        // The input is in `.top`, an untouched subtree — its caret persists with no
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
    /// tapping the label toggles the input — even though the label doesn't wrap it.
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

    /// `:class` object/conditional form (`#{ hot: cond }`) — keys whose value is
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
    /// builds — a smoke test that the example stays valid.
    #[test]
    fn css_showcase_example_builds() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/css-showcase.rux");
        let doc = Document::load(path).expect("css-showcase.rux builds");
        assert!(find_text(&doc.root, "teal"), "a :style-coloured chip rendered");
    }

    /// `:style` object form (`#{ background: c }`) — each entry a declaration.
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
