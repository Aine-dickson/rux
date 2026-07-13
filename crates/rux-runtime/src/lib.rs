//! Rux runtime — milestones M2–M8.
//!
//! The document model: loads a `.rux` file, builds its script [`Engine`]
//! (registering host functions), and builds the renderable layout tree with
//! `{{ }}` bindings and directives resolved against the engine's live state.
//! Running an `@tap` handler mutates engine state; `rebuild` refreshes the tree
//! (coarse whole-tree rebuild — fine-grained updates are a later refinement).

use std::path::Path;

use rux_layout::Node as LayoutNode;
use rux_parser::Sfc;
use rux_script::{Builder, Engine};

/// A loaded `.rux` document: its parsed source, script engine, and current tree.
pub struct Document {
    sfc: Sfc,
    engine: Engine,
    pub root: LayoutNode,
}

impl Document {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let src = std::fs::read_to_string(&path)
            .map_err(|e| format!("reading {}: {e}", path.as_ref().display()))?;
        Self::from_source(&src)
    }

    pub fn from_source(src: &str) -> Result<Self, String> {
        let sfc = rux_parser::parse_sfc(src).map_err(|e| e.to_string())?;
        let mut engine = build_engine(&sfc.script)?;
        let root = rux_style::build_styled_tree(&sfc, &mut engine)?;
        Ok(Self { sfc, engine, root })
    }

    /// The script engine, for running `@tap` handlers.
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    /// Rebuild the layout tree from the engine's current state.
    pub fn rebuild(&mut self) {
        if let Ok(root) = rux_style::build_styled_tree(&self.sfc, &mut self.engine) {
            self.root = root;
        }
    }
}

/// Build the script engine and register the host functions available to `.rux`
/// scripts. This is the native-capability boundary; a real app would register
/// its own host functions here. For now a couple of demo capabilities.
fn build_engine(script: &str) -> Result<Engine, String> {
    let mut builder = Builder::new();
    builder.host_number("full", || 100.0);
    builder.build(script)
}
