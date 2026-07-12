//! Rux runtime — milestones M2–M5.
//!
//! The document model: loads a `.rux` file, seeds its signals from the script,
//! and builds the renderable layout tree with `{{ }}` bindings resolved against
//! the current signal values. Mutating a signal and calling `rebuild` refreshes
//! the tree — the coarse form of reactivity (whole-tree rebuild) that M5 uses;
//! fine-grained per-binding updates are a later refinement.

use std::path::Path;

use rux_layout::Node as LayoutNode;
use rux_parser::Sfc;
use rux_reactive::Signals;

/// A loaded `.rux` document: its parsed source, live signals, and current tree.
pub struct Document {
    sfc: Sfc,
    signals: Signals,
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
        let signals = Signals::from_script(&sfc.script);
        let root = rux_style::build_styled_tree(&sfc, &signals)?;
        Ok(Self { sfc, signals, root })
    }

    /// Mutable access to the signal table (the shell mutates it, then rebuilds).
    pub fn signals_mut(&mut self) -> &mut Signals {
        &mut self.signals
    }

    /// Rebuild the layout tree from the current signal values.
    pub fn rebuild(&mut self) {
        if let Ok(root) = rux_style::build_styled_tree(&self.sfc, &self.signals) {
            self.root = root;
        }
    }
}
