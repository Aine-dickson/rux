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
    /// The focused input's `r-model` and caret byte index, if any. Re-applied on
    /// every rebuild so the caret survives a state change.
    focus: Option<(String, usize)>,
    pub root: LayoutNode,
}

/// Mark the focused input's text child with the caret position, so it paints one.
fn apply_focus(node: &mut LayoutNode, focus: Option<&(String, usize)>) {
    if let Some((model, caret)) = focus {
        if node.model.as_deref() == Some(model.as_str()) {
            if let Some(text) = node.children.first_mut().and_then(|c| c.text.as_mut()) {
                // An empty input shows its placeholder; the caret still sits at 0.
                text.caret = Some((*caret).min(text.text.len()));
            }
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

    /// Focus an input (by `r-model`) and put its caret at `caret`. `None` clears.
    pub fn set_focus(&mut self, focus: Option<(String, usize)>) {
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
}
