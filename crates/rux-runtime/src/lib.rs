//! Rux runtime — milestones M2–M9.
//!
//! The document model: loads a `.rux` file, resolves its `use` component imports
//! (loading each imported `.rux`), builds the script [`Engine`] (merging the main
//! and component scripts, registering host functions), and builds the renderable
//! tree with bindings, directives, and component expansions resolved. Running an
//! `@tap` handler mutates engine state; `rebuild` refreshes the tree.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rux_layout::Node as LayoutNode;
use rux_parser::Sfc;
use rux_script::{Builder, Engine};

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
        let mut root = rux_style::build_styled_tree(&sfc, &components, &mut engine)?;
        resolve_images(&mut root, base);
        Ok(Self {
            sfc,
            components,
            engine,
            base: base.to_path_buf(),
            focus: None,
            root,
        })
    }

    /// Process `.rux` source with no import resolution (used for fallbacks/tests).
    pub fn from_source(src: &str) -> Result<Self, String> {
        let sfc = rux_parser::parse_sfc(src).map_err(|e| e.to_string())?;
        let (main_script, _imports) = extract_imports(&sfc.script);
        let mut engine = build_engine(&main_script)?;
        let mut root = rux_style::build_styled_tree(&sfc, &HashMap::new(), &mut engine)?;
        let base = PathBuf::from(".");
        resolve_images(&mut root, &base);
        Ok(Self {
            sfc,
            components: HashMap::new(),
            engine,
            base,
            focus: None,
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
        if let Ok(mut root) = rux_style::build_styled_tree(&self.sfc, &self.components, &mut self.engine) {
            resolve_images(&mut root, &self.base);
            apply_focus(&mut root, self.focus.as_ref());
            self.root = root;
        }
    }
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
