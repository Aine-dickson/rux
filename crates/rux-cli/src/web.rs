//! Turning a built wasm module into something a browser can open.
//!
//! `cargo` produces a `.wasm` that a browser cannot load on its own: it has no
//! imports wired to the DOM and no way to be started. `wasm-bindgen` generates
//! the JavaScript that does both, and this runs it and writes a page around the
//! result.
//!
//! **The version check is the whole of the ceremony here.** `wasm-bindgen` the
//! crate and `wasm-bindgen` the command-line tool must be the same version, and
//! when they are not the failure is a wall of generated-code errors that says
//! nothing about versions. So the two are compared before that can happen, with
//! the one command that fixes it.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::manifest::Manifest;

/// Run `wasm-bindgen` over the built module and write a page around it.
pub fn bundle(
    manifest: &Manifest,
    work: &Path,
    built: &Path,
    out_dir: &Path,
) -> Result<PathBuf, String> {
    if !built.is_file() {
        return Err(format!("the generated crate built nothing at {}", built.display()));
    }
    check_version(work)?;

    std::fs::create_dir_all(out_dir).map_err(|e| format!("creating {}: {e}", out_dir.display()))?;
    let status = Command::new("wasm-bindgen")
        .arg("--target")
        .arg("web")
        // No TypeScript definitions: nothing here is imported by a typed build,
        // and the files are noise in a directory someone is meant to deploy.
        .arg("--no-typescript")
        .arg("--out-dir")
        .arg(out_dir)
        .arg("--out-name")
        .arg(manifest.artifact_stem())
        .arg(built)
        .status()
        .map_err(|e| missing_tool(&e.to_string()))?;
    if !status.success() {
        return Err("wasm-bindgen could not process the built module".into());
    }

    let page = out_dir.join("index.html");
    std::fs::write(&page, index_html(manifest))
        .map_err(|e| format!("writing {}: {e}", page.display()))?;

    // Said out loud, because it is the number that decides whether this is
    // deployable. A visitor pays it before anything appears on screen, and a
    // debug build is large enough that someone who did not check would find
    // out from their own users.
    let wasm = out_dir.join(format!("{}_bg.wasm", manifest.artifact_stem()));
    if let Ok(meta) = std::fs::metadata(&wasm) {
        println!("rux: the module is {}", megabytes(meta.len()));
    }
    Ok(page)
}

/// A byte count a person can read.
fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

/// What to say when `wasm-bindgen` is not there at all.
fn missing_tool(why: &str) -> String {
    format!(
        "could not run wasm-bindgen: {why}\n\n\
         A web build needs it to turn the compiled module into something a browser can \
         load:\n  cargo install wasm-bindgen-cli"
    )
}

/// The version of the `wasm-bindgen` tool on `PATH`.
///
/// **This is what the generated crate pins its dependency to**, rather than the
/// two being resolved independently and compared afterwards. The tool writes
/// the glue and the crate compiles it, so a difference produces errors in code
/// nobody wrote about symbols nobody named. Letting cargo pick the newest
/// `0.2.x` while the tool stays wherever it was installed makes that difference
/// the *default* outcome, which is exactly what happened the first time this
/// ran: 0.2.126 installed, 0.2.128 resolved, three minutes of compiling before
/// anyone found out.
///
/// Pinning to the tool inverts it. Whatever is installed is what gets compiled
/// against, so the two agree by construction.
pub fn installed_version() -> Result<String, String> {
    let output = Command::new("wasm-bindgen")
        .arg("--version")
        .output()
        .map_err(|e| missing_tool(&e.to_string()))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let version = text.split_whitespace().nth(1).unwrap_or("").trim().to_string();
    if version.is_empty() {
        return Err(missing_tool("it reported no version"));
    }
    Ok(version)
}

/// Confirm the build resolved the version it was pinned to.
///
/// Belt and braces after [`installed_version`] pinned it: a `[patch]` section or
/// a hand-run `cargo update` in the generated directory could still move it,
/// and the failure that follows is unreadable. Cheap, and it reads what cargo
/// actually resolved rather than what it was asked for.
fn check_version(work: &Path) -> Result<(), String> {
    let installed = installed_version()?;
    let Some(resolved) = locked_version(&work.join("Cargo.lock")) else { return Ok(()) };
    if installed == resolved {
        return Ok(());
    }
    Err(format!(
        "wasm-bindgen {installed} is installed, and this build resolved {resolved}.\n\n\
         The tool and the crate generate and compile the same glue, so they have to \
         match:\n  cargo install wasm-bindgen-cli --version {resolved} --force"
    ))
}

/// The version of `wasm-bindgen` a lockfile pins, if it names one.
fn locked_version(lock: &Path) -> Option<String> {
    let text = std::fs::read_to_string(lock).ok()?;
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.trim() == "name = \"wasm-bindgen\"" {
            let version = lines.next()?.trim();
            return version
                .strip_prefix("version = \"")
                .and_then(|v| v.strip_suffix('"'))
                .map(str::to_string);
        }
    }
    None
}

/// The page that loads the module.
///
/// Deliberately the smallest thing that works, because it is a starting point
/// someone will edit rather than a framework they have to learn. The canvas
/// fills the window, the module is imported as an ES module, and the only
/// contract with the generated code is the element's id.
///
/// `height: 100%` on `html` and `body` is not decoration: a canvas sized in
/// percentages against a body with no height is a canvas of height zero, which
/// renders as a blank page with no error anywhere.
fn index_html(manifest: &Manifest) -> String {
    let stem = manifest.artifact_stem();
    format!(
        r#"<!DOCTYPE html>
<!-- Generated by `rux build --target web`. Yours to edit: it is a starting
     point, and nothing regenerates it once it is here. -->
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <!-- `viewport-fit=cover`: the app draws to the display's edges and keeps
         clear of the notch with `env(safe-area-inset-*)`, as it does on a
         phone. `interactive-widget=resizes-content`: the keyboard makes the
         page shorter, so the app lays out above it, as it does on a phone. -->
    <meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover, interactive-widget=resizes-content" />
    <title>{title}</title>
    <style>
      html, body {{ height: 100%; margin: 0; background: #1e1e2e; }}
      /* The app draws here. A canvas has no intrinsic size, so it is given one. */
      #rux {{ display: block; width: 100%; height: 100%; }}
    </style>
  </head>
  <body>
    <canvas id="rux"></canvas>
    <script type="module">
      import init from "./{stem}.js";
      init();
    </script>
  </body>
</html>
"#,
        title = escape(&manifest.name),
    )
}

/// HTML-escape a value going into text or an attribute.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(name: &str) -> Manifest {
        Manifest {
            root: PathBuf::from("."),
            name: name.into(),
            id: "dev.example.tasks".into(),
            version: "0.1.0".into(),
            entry: PathBuf::from("app.rux"),
            signing: None,
            icon: None,
            scheme: None,
            link_hosts: Vec::new(),
        }
    }

    #[test]
    fn the_page_imports_the_module_it_will_sit_beside() {
        // `--out-name` and the import have to agree, and they are written in two
        // places. `Task List` is `task-list` in both or the page loads nothing.
        let html = index_html(&manifest("Task List"));
        assert!(html.contains(r#"import init from "./task-list.js""#), "{html}");
    }

    #[test]
    fn the_page_names_the_canvas_the_generated_code_looks_for() {
        let html = index_html(&manifest("Task List"));
        assert!(html.contains(r#"<canvas id="rux">"#), "{html}");
    }

    #[test]
    fn the_page_draws_to_the_edges_and_gives_way_to_the_keyboard() {
        // Without `viewport-fit=cover` every safe-area inset reads 0 in a
        // browser, and without `resizes-content` Chrome on a phone lays the
        // keyboard over the page instead of shortening it, so a low field is
        // typed into blind.
        let html = index_html(&manifest("Task List"));
        assert!(html.contains("viewport-fit=cover"), "{html}");
        assert!(html.contains("interactive-widget=resizes-content"), "{html}");
    }

    #[test]
    fn a_name_with_markup_in_it_does_not_reach_the_page() {
        let html = index_html(&manifest("Tasks & <b>more</b>"));
        assert!(html.contains("Tasks &amp; &lt;b&gt;more&lt;/b&gt;"), "{html}");
        assert!(!html.contains("<b>more</b>"), "{html}");
    }

    #[test]
    fn a_lockfile_gives_up_the_wasm_bindgen_version() {
        let dir = std::env::temp_dir().join("rux-web-lock-test");
        std::fs::create_dir_all(&dir).unwrap();
        let lock = dir.join("Cargo.lock");
        std::fs::write(
            &lock,
            "[[package]]\nname = \"other\"\nversion = \"9.9.9\"\n\n\
             [[package]]\nname = \"wasm-bindgen\"\nversion = \"0.2.126\"\n",
        )
        .unwrap();
        // The first version in the file belongs to another package, so a reader
        // that grabbed the first version-shaped line would get 9.9.9.
        assert_eq!(locked_version(&lock).as_deref(), Some("0.2.126"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
