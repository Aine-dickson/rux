//! Script modules on disk: step 7 of `docs/11-next.md`, "Modules".
//!
//! A `.rux` file with no `<template>` is a module: a `<script>` alone, or
//! bare script with no tags. It holds functions, types and state for other
//! files to import. It is loaded once per document however many files import
//! it, which is what makes its top-level signals a store: every importer
//! sees the same state.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rux_syntax::ast::StmtKind;

use crate::LoadError;

/// A module, read and held to what a module may say.
pub(crate) struct ModuleFile {
    /// `stores/cart`: its path from the project root, without `.rux`. What
    /// its names are linked under, and what a message calls it.
    pub name: String,
    pub path: PathBuf,
    /// Its script, imports taken out and their lines kept.
    pub script: String,
    /// The file line its script starts on.
    pub script_line: usize,
    /// What it exports, by name, and whether each is a type.
    pub exports: Vec<(String, bool)>,
    /// What it imports from other modules.
    pub aliases: Vec<rux_script::link::Alias>,
    /// The modules it imports, with the file line of each import.
    pub deps: Vec<(String, usize)>,
    /// The types it imports with `use type`, as name and text.
    pub types: Vec<(String, String)>,
}

impl ModuleFile {
    /// Whether it exports `name`, and if so whether that is a type.
    pub fn export(&self, name: &str) -> Option<bool> {
        self.exports.iter().find(|(n, _)| n == name).map(|(_, t)| *t)
    }

    /// `it exports `a`, `b``, for a message; types left out unless `types`.
    pub fn listed(&self, types: bool) -> String {
        let names: Vec<String> =
            self.exports.iter().filter(|(_, t)| *t == types).map(|(n, _)| format!("`{n}`")).collect();
        if names.is_empty() {
            format!("it exports no {}", if types { "types" } else { "functions or values" })
        } else {
            format!("it exports {}", names.join(", "))
        }
    }
}

/// The name a module at `path` is known by: its path from `root`, joined with
/// `/`, without `.rux`. A file outside the root is known by its whole path.
pub(crate) fn name_of(path: &Path, root: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    parts.join("/").trim_end_matches(".rux").to_string()
}

/// Read a module's script, already taken from its file, and check that it
/// says only what a module may. Its own imports are the caller's to walk.
pub(crate) fn load(
    name: String,
    path: PathBuf,
    script: String,
    script_line: usize,
) -> Result<ModuleFile, LoadError> {
    let opts = rux_syntax::Options { declarations: true };
    let parsed = rux_syntax::parse(&script, opts).map_err(|e| {
        let (line, _) = e.line_col(&script);
        LoadError::at_line(e.message.clone(), script_line + line - 1, &path)
    })?;
    let lines = rux_syntax::LineIndex::new(&script);
    for stmt in &parsed.stmts {
        let what = match &stmt.kind {
            StmtKind::Computed { .. } => "a `computed`",
            StmtKind::Lifecycle { kind: rux_syntax::ast::Lifecycle::Effect, .. } => "an `effect`",
            StmtKind::Lifecycle { .. } => "a lifecycle block",
            StmtKind::Prop(_) => "a `prop`",
            _ => continue,
        };
        let line = lines.line_col(&script, stmt.span.start as usize).0;
        return Err(LoadError::at_line(
            format!(
                "{what} belongs to a component or a document; a module (a file with no <template>) \
                 holds functions, types and state. An exported function can work out what a \
                 `computed` would"
            ),
            script_line + line - 1,
            &path,
        ));
    }
    let exports = parsed
        .exports
        .iter()
        .map(|x| {
            let is_type = matches!(parsed.stmts.get(x.stmt).map(|s| &s.kind), Some(StmtKind::Type { .. }));
            (x.name.name.clone(), is_type)
        })
        .collect();
    Ok(ModuleFile { name, path, script, script_line, exports, aliases: Vec::new(), deps: Vec::new(), types: Vec::new() })
}

/// The modules, keyed as the loader keys them, each after every module it
/// imports: the order their top levels run in. Modules that import each
/// other in a circle have no such order, and are refused.
pub(crate) fn order(modules: &HashMap<String, ModuleFile>) -> Result<Vec<String>, LoadError> {
    let by_name: HashMap<&str, &str> = modules.iter().map(|(k, m)| (m.name.as_str(), k.as_str())).collect();
    let mut keys: Vec<&String> = modules.keys().collect();
    keys.sort_by(|a, b| modules[*a].name.cmp(&modules[*b].name));
    let mut done: Vec<String> = Vec::new();
    let mut on_path: Vec<&str> = Vec::new();

    fn visit<'a>(
        key: &'a str,
        modules: &'a HashMap<String, ModuleFile>,
        by_name: &HashMap<&'a str, &'a str>,
        done: &mut Vec<String>,
        on_path: &mut Vec<&'a str>,
    ) -> Result<(), LoadError> {
        if done.iter().any(|d| d == key) {
            return Ok(());
        }
        let m = &modules[key];
        on_path.push(key);
        for (dep, line) in &m.deps {
            let Some(&dep_key) = by_name.get(dep.as_str()) else { continue };
            if let Some(at) = on_path.iter().position(|k| *k == dep_key) {
                let mut circle: Vec<&str> = on_path[at..].iter().map(|k| modules[*k].name.as_str()).collect();
                circle.push(dep);
                return Err(LoadError::at_line(
                    format!(
                        "modules import each other in a circle ({}), so none of them can run first. \
                         Move what they share into a module of its own",
                        circle.join(" imports ")
                    ),
                    *line,
                    &m.path,
                ));
            }
            visit(dep_key, modules, by_name, done, on_path)?;
        }
        on_path.pop();
        done.push(key.to_string());
        Ok(())
    }

    for key in keys {
        visit(key, modules, &by_name, &mut done, &mut on_path)?;
    }
    Ok(done)
}
