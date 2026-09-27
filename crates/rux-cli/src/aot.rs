//! Compiling a release build's script: step 9 of `docs/11-next.md`.
//!
//! The entry document is loaded exactly as the app will load it, so its unit
//! and the hash of its text are the ones the app will compute; `rux-codegen`
//! writes Rust for that unit into the wrapper crate, and the wrapper installs
//! it before the first document loads. What the generator hands back to the
//! interpreter, and every document the build did not load, run interpreted,
//! so compiling can only make a build faster, never different.
//!
//! `RUX_AOT=0` turns it off, for comparing a build with and without.

use std::path::Path;

use crate::manifest::Manifest;

/// The generated module's text, and a line for the build log. `None` when
/// compiling is off or the document does not load (the build then runs
/// interpreted, and says why).
pub fn generate(manifest: &Manifest) -> Option<(String, String)> {
    if std::env::var("RUX_AOT").is_ok_and(|v| v == "0") {
        return None;
    }
    let entry = manifest.root.join(&manifest.entry);
    // Loaded the way `rux check` loads it: its Rust read for signatures, and
    // nothing printed, since the build reports what matters itself.
    let _ = crate::native::declare_for(&[entry.clone()]);
    rux_runtime::set_stderr_echo(false);
    let doc = rux_runtime::Document::load(&entry);
    rux_runtime::set_stderr_echo(true);
    let doc = match doc {
        Ok(doc) => doc,
        Err(e) => {
            let shown = entry.strip_prefix(&manifest.root).unwrap_or(Path::new(&manifest.entry)).display().to_string();
            println!("rux: {shown} did not load, so its script runs interpreted: {e}");
            return None;
        }
    };
    let (hash, unit) = doc.compiled_unit();
    let out = rux_codegen::generate(unit, hash, "rux_runtime::aot");
    let line = format!(
        "compiled {} function{} and {} closure bod{} to Rust ({} statement{} compiled, {} left to the interpreter)",
        out.functions,
        if out.functions == 1 { "" } else { "s" },
        out.closures,
        if out.closures == 1 { "y" } else { "ies" },
        out.compiled,
        if out.compiled == 1 { "" } else { "s" },
        out.handed_back
    );
    Some((out.code, line))
}
