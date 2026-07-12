//! Rux runtime — milestone M2.
//!
//! The document model: loads a `.rux` file, runs it through parse → cascade, and
//! exposes the resulting renderable layout tree. In M2 this is a one-shot load;
//! M3 adds a file watcher and reload, M5 the reactive graph
//! (see `docs/04-architecture.md`).

use std::path::Path;

use rux_layout::Node as LayoutNode;

/// A loaded `.rux` document, reduced to its renderable layout tree.
pub struct Document {
    pub root: LayoutNode,
}

impl Document {
    /// Load and process a `.rux` file from disk.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let src = std::fs::read_to_string(&path)
            .map_err(|e| format!("reading {}: {e}", path.as_ref().display()))?;
        Self::from_source(&src)
    }

    /// Process `.rux` source already in memory.
    pub fn from_source(src: &str) -> Result<Self, String> {
        let sfc = rux_parser::parse_sfc(src).map_err(|e| e.to_string())?;
        let root = rux_style::build_styled_tree(&sfc)?;
        Ok(Self { root })
    }
}
