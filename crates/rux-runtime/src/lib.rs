//! Rux runtime — milestones M2–M9.
//!
//! The document model: loads a `.rux` file, resolves its `use` component imports
//! (loading each imported `.rux`), builds the script [`Engine`] (merging the main
//! and component scripts, registering host functions), and builds the renderable
//! tree with bindings, directives, and component expansions resolved. Running an
//! `@tap` handler mutates engine state; `rebuild` refreshes the tree.

use std::collections::HashMap;
use std::path::Path;

use rux_layout::Node as LayoutNode;
use rux_parser::Sfc;
use rux_script::{Builder, Engine};

/// A loaded `.rux` document: parsed source, imported components (by tag), the
/// script engine, and the current tree.
pub struct Document {
    sfc: Sfc,
    components: HashMap<String, Sfc>,
    engine: Engine,
    pub root: LayoutNode,
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
        let root = rux_style::build_styled_tree(&sfc, &components, &mut engine)?;
        Ok(Self {
            sfc,
            components,
            engine,
            root,
        })
    }

    /// Process `.rux` source with no import resolution (used for fallbacks/tests).
    pub fn from_source(src: &str) -> Result<Self, String> {
        let sfc = rux_parser::parse_sfc(src).map_err(|e| e.to_string())?;
        let (main_script, _imports) = extract_imports(&sfc.script);
        let mut engine = build_engine(&main_script)?;
        let root = rux_style::build_styled_tree(&sfc, &HashMap::new(), &mut engine)?;
        Ok(Self {
            sfc,
            components: HashMap::new(),
            engine,
            root,
        })
    }

    /// The script engine, for running `@tap` handlers.
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    /// Rebuild the layout tree from the engine's current state.
    pub fn rebuild(&mut self) {
        if let Ok(root) = rux_style::build_styled_tree(&self.sfc, &self.components, &mut self.engine) {
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
            if let Some(path) = rest.strip_suffix(';') {
                let segments: Vec<&str> = path.trim().split("::").collect();
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
    fn loads_dashboard_and_expands_component() {
        // Exercises the real path: import resolution, component file loading,
        // engine merge, and component expansion with props.
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/dashboard.rux");
        let doc = Document::load(path).expect("load dashboard");

        // The <stat> component expanded and interpolated its label/value props.
        assert!(find_text(&doc.root, "Battery"), "component label prop rendered");
        assert!(find_text(&doc.root, "82"), "component value prop rendered");
    }
}
