//! What a document opened from somewhere that is not the app's own gets.
//!
//! Compiled app code and the files shipped in the app have full trust. A
//! document that arrives from elsewhere (the playground, a fetched or
//! over-the-air one) is run under a [`Sandbox`]: limits on what it may spend,
//! and no native module unless the opener grants it by name. A file can never
//! mark itself trusted; only whoever opens it hands over a `Sandbox`, and with
//! none the document is trusted, as it has always been.

use std::collections::BTreeSet;

/// What a sandboxed document may spend. Every limit is checked before the work
/// it limits, so a document is stopped, not slowed.
#[derive(Clone, Debug)]
pub struct Limits {
    /// The most steps one run may take. See [`crate::MAX_OPERATIONS`].
    pub steps_per_run: u64,
    /// The longest one piece of text may be, in bytes.
    pub max_text_bytes: usize,
    /// The most entries in one array, map or record.
    pub max_items: usize,
    /// What one run may grow memory by, in bytes.
    pub run_alloc_bytes: usize,
    /// What the document's state may come to after a run, in bytes.
    pub state_bytes: usize,
    /// The most elements one render may build.
    pub max_nodes: usize,
    /// The longest document text, in bytes, one file may be.
    pub max_source_bytes: usize,
    /// The deepest brackets, blocks, templates and prefix operators may nest.
    pub max_nesting: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            steps_per_run: crate::MAX_OPERATIONS,
            max_text_bytes: 16 << 20,
            max_items: 1_000_000,
            run_alloc_bytes: 64 << 20,
            state_bytes: 64 << 20,
            max_nodes: 50_000,
            max_source_bytes: 1 << 20,
            max_nesting: 64,
        }
    }
}

/// The terms a document is run on when it is not the app's own: [`Limits`], and
/// the native modules it was granted, by the name script uses (`clipboard`,
/// `settings`, `files`, `native::shop`, `images`). Nothing is granted by
/// [`Sandbox::new`].
#[derive(Clone, Debug, Default)]
pub struct Sandbox {
    pub grants: BTreeSet<String>,
    pub limits: Limits,
}

impl Sandbox {
    /// The default limits and no grants.
    pub fn new() -> Self {
        Self::default()
    }

    /// Allow the document `module`.
    pub fn grant(mut self, module: &str) -> Self {
        self.grants.insert(module.to_string());
        self
    }

    /// Whether `module` was granted.
    pub fn grants(&self, module: &str) -> bool {
        self.grants.contains(module)
    }

    /// The first reason `text`, one file of a document, may not be loaded under
    /// these limits: the offset it goes wrong at, and what to tell the author.
    /// Checked before the text is lexed, so nothing hostile reaches a parser.
    pub fn refuse(&self, text: &str) -> Option<(usize, String)> {
        let l = &self.limits;
        if text.len() > l.max_source_bytes {
            return Some((
                0,
                format!(
                    "this file is {} bytes; a sandboxed document may be at most {}",
                    text.len(),
                    l.max_source_bytes
                ),
            ));
        }
        rux_syntax::nests_deeper_than(text, l.max_nesting).map(|at| {
            (at, format!("this is nested more than {} deep, the most a sandboxed document may be", l.max_nesting))
        })
    }
}

/// The name a grant uses for native module `module` (as linked): `clipboard`
/// for `native/rux/clipboard`, `files` for the dialogs and sounds Rux gives
/// every script by bare name (`native/rux`), and the path an app's own module
/// is imported by (`native::shop`) for the rest.
pub fn grant_name(module: &str) -> String {
    if module == crate::native::BUILTIN_MODULE {
        return "files".to_string();
    }
    match module.strip_prefix("native/rux/") {
        Some(name) => name.to_string(),
        None => module.replace('/', "::"),
    }
}

/// What a document is told when it uses what it was not granted: what to
/// call it, and who can change it.
pub fn not_granted(name: &str) -> String {
    format!(
        "`{name}` is not available in this document: whoever opens it has to grant it \
         (`.grant(\"{name}\")`)"
    )
}

impl Sandbox {
    /// Whether the native module `module` (as linked) was granted.
    pub fn allows(&self, module: &str) -> bool {
        self.grants.contains(&grant_name(module))
    }
}
