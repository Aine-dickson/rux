//! `rux build`: a project becomes something you can hand to someone.
//!
//! The output is a **generated wrapper crate**. `rux build` writes a small crate
//! that embeds the project's documents and calls `rux-shell`, then builds it
//! with cargo. The result is one executable with nothing beside it.
//!
//! The alternative considered and rejected was appending a payload to a
//! prebuilt runtime binary, which needs no toolchain and builds far faster. It
//! lost on two counts: code signing dislikes an appended payload, and Android
//! cannot reuse any of it. An Android app is a `cdylib` with an `android_main`,
//! which is the same generated-wrapper shape with a different target and a
//! different entry. Doing it this way means the generator is proven on the one
//! target we can already run before it is asked to produce an APK.
//!
//! **Dev and release differ, and the difference is the embedding.** A release
//! build embeds every document, so the artifact is self-contained and its
//! contents cannot drift from what was tested. A dev build points the same
//! wrapper at the project directory, so a document edited on disk is picked up
//! by hot reload exactly as under `rux run`. This is the rule for every target,
//! not a desktop convenience: on Android the dev build serves documents as APK
//! assets and reloads them over `adb`.
//!
//! Images took a second step to get there, and it was found by driving a build
//! rather than by reasoning about one. An image is read twice: the runtime
//! reads the header for the intrinsic size, and `rux-paint` read the pixels
//! with `image::open` on a path, which inside an executable is a path to
//! nothing. The first release build laid out correctly and drew empty boxes.
//! `rux-paint` and `rux-runtime` share no dependency edge, so the fix is a
//! reader hook in `rux-layout`, which both depend on; see
//! `rux_layout::set_image_reader`.
//!
//! Embedding is possible at all because the runtime stopped reading the disk
//! directly: a document's files come from a `Source`, and an embedded build
//! installs a `MemorySource` built from `include_bytes!`.

use std::path::{Path, PathBuf};

use crate::manifest::Manifest;

pub struct Options {
    pub release: bool,
    pub target: Target,
    /// Where the Rux crates come from in the generated manifest.
    ///
    /// A released `rux` names published versions. Building Rux itself has no
    /// published version to name, so `--rux-source <dir>` points the wrapper at
    /// a checkout instead. It is how this command is driven in this repo, and
    /// how anyone tracking the tip builds.
    pub rux_source: Option<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Desktop,
}

impl Target {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "desktop" => Ok(Target::Desktop),
            // Named rather than ignored: both are real targets of this
            // milestone, and "unknown target" would read as if they were never
            // coming.
            "android" | "ios" => Err(format!(
                "`{name}` is not built yet. `desktop` is what `rux build` can produce today"
            )),
            other => Err(format!("unknown target `{other}`; the one that exists is `desktop`")),
        }
    }
}

/// Every file the app is made of, relative to the project root.
///
/// Collected by walking the project rather than by following imports. Following
/// imports would be exact, but it would also mean an asset named only in CSS,
/// or a route loaded by a string, silently missing from the build: the failure
/// would be a missing file at runtime in someone else's hands. A directory walk
/// is a few kilobytes too generous and never wrong in that direction.
fn collect(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("reading {}: {e}", dir.display()))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        // Build output, version control and editor state are not the app. `.`
        // covers `.git`, `.rux-build` and every editor directory at once.
        if name.starts_with('.') || name == "target" || name == "dist" {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, out)?;
        } else if path.is_file() {
            // The manifest describes the build; it is not part of the app.
            if path.file_name().map(|n| n == crate::manifest::MANIFEST).unwrap_or(false) {
                continue;
            }
            let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            out.push(relative);
        }
    }
    Ok(())
}

/// One spelling for a path used as a key, matching `MemorySource`'s own.
fn key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// A Rust string literal for a path, with backslashes and quotes escaped.
///
/// Windows paths are full of backslashes and a raw string would break on a path
/// containing `"#`, so this escapes rather than using `r"..."`.
fn literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn wrapper_manifest(manifest: &Manifest, options: &Options) -> String {
    let deps = match &options.rux_source {
        Some(dir) => {
            let dir = dir.display().to_string().replace('\\', "/");
            format!(
                "rux-shell = {{ path = {} }}\nrux-runtime = {{ path = {} }}\n",
                literal(&format!("{dir}/crates/rux-shell")),
                literal(&format!("{dir}/crates/rux-runtime")),
            )
        }
        None => {
            let version = env!("CARGO_PKG_VERSION");
            format!("rux-shell = \"{version}\"\nrux-runtime = \"{version}\"\n")
        }
    };
    format!(
        "# Generated by `rux build`. Edits are overwritten on the next build.\n\
         [package]\n\
         name = {}\n\
         version = {}\n\
         edition = \"2021\"\n\
         publish = false\n\
         \n\
         [[bin]]\n\
         name = {}\n\
         path = \"src/main.rs\"\n\
         \n\
         [dependencies]\n\
         {deps}",
        literal(&manifest.artifact_stem()),
        literal(&manifest.version),
        literal(&manifest.artifact_stem()),
    )
}

fn wrapper_main(manifest: &Manifest, files: &[PathBuf], options: &Options) -> String {
    let entry = key(&manifest.entry);
    let mut out = String::new();
    out.push_str("// Generated by `rux build`. Edits are overwritten on the next build.\n");
    out.push_str("//\n");

    if options.release {
        out.push_str(
            "// A release build embeds every document, so the artifact carries its own\n\
             // contents and cannot drift from what was tested. `include_bytes!` resolves\n\
             // at compile time against the absolute path each file had when this was\n\
             // generated, which is why the generated crate is disposable.\n",
        );
        out.push_str("fn main() {\n");
        out.push_str("    let mut files = rux_runtime::MemorySource::new();\n");
        for file in files {
            let absolute = manifest.root.join(file);
            let absolute = std::fs::canonicalize(&absolute).unwrap_or(absolute);
            out.push_str(&format!(
                "    files.insert({}, include_bytes!({}).to_vec());\n",
                literal(&key(file)),
                literal(&absolute.display().to_string()),
            ));
        }
        out.push_str("    rux_runtime::set_source(std::rc::Rc::new(files));\n");
        out.push_str(&format!(
            "    rux_shell::run(std::path::PathBuf::from({}));\n",
            literal(&entry)
        ));
        out.push_str("}\n");
    } else {
        out.push_str(
            "// A dev build reads the project from disk, so hot reload works exactly as\n\
             // it does under `rux run`. The project path is baked in because the binary\n\
             // may be run from anywhere.\n",
        );
        let root = std::fs::canonicalize(&manifest.root).unwrap_or_else(|_| manifest.root.clone());
        out.push_str("fn main() {\n");
        out.push_str(&format!(
            "    let root = std::path::PathBuf::from({});\n",
            literal(&root.display().to_string())
        ));
        out.push_str(&format!("    rux_shell::run(root.join({}));\n", literal(&entry)));
        out.push_str("}\n");
    }
    out
}

pub fn run(options: Options) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("{e}"))?;
    let manifest = Manifest::find(&cwd)?;
    let Target::Desktop = options.target;

    let files = collect(&manifest.root)?;
    if files.is_empty() {
        return Err(format!("{} holds no files to build", manifest.root.display()));
    }

    // Inside the project, and hidden, so the directory walk above skips it and
    // a `.gitignore` does not have to learn a new name.
    let work = manifest.root.join(".rux-build").join("desktop");
    std::fs::create_dir_all(work.join("src"))
        .map_err(|e| format!("creating {}: {e}", work.display()))?;
    std::fs::write(work.join("Cargo.toml"), wrapper_manifest(&manifest, &options))
        .map_err(|e| format!("writing the generated manifest: {e}"))?;
    std::fs::write(work.join("src/main.rs"), wrapper_main(&manifest, &files, &options))
        .map_err(|e| format!("writing the generated entry point: {e}"))?;

    let mode = if options.release { "release" } else { "debug" };
    // The id is shown rather than merely validated. It is the one field that
    // cannot be corrected later without an operating system treating the result
    // as a different app, so it is worth putting in front of whoever is
    // watching the build rather than only in the file they wrote it in.
    println!(
        "rux: building {} {} ({}) [{mode}, {} file{}]",
        manifest.name,
        manifest.version,
        manifest.id,
        files.len(),
        if files.len() == 1 { "" } else { "s" }
    );

    let mut cargo = std::process::Command::new("cargo");
    cargo.arg("build").current_dir(&work);
    if options.release {
        cargo.arg("--release");
    }
    let status = cargo.status().map_err(|e| {
        format!(
            "could not run cargo: {e}\n\
             `rux build` compiles a small generated crate, so it needs a Rust toolchain. \
             Install one from https://rustup.rs"
        )
    })?;
    if !status.success() {
        return Err("the generated crate did not build".into());
    }

    let exe = format!("{}{}", manifest.artifact_stem(), std::env::consts::EXE_SUFFIX);
    let built = work.join("target").join(mode).join(&exe);
    let dist = manifest.root.join("dist");
    std::fs::create_dir_all(&dist).map_err(|e| format!("creating {}: {e}", dist.display()))?;
    let out = dist.join(&exe);
    std::fs::copy(&built, &out)
        .map_err(|e| format!("copying {} to {}: {e}", built.display(), out.display()))?;

    println!("rux: wrote {}", out.display());
    if !options.release {
        println!("rux: a dev build reads the project from disk; `--release` embeds it");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        Manifest {
            root: PathBuf::from("."),
            name: "Task List".into(),
            id: "dev.example.tasks".into(),
            version: "0.1.0".into(),
            entry: PathBuf::from("app.rux"),
        }
    }

    #[test]
    fn a_release_wrapper_embeds_every_file_and_installs_the_memory_source() {
        let files = vec![PathBuf::from("app.rux"), PathBuf::from("components/task.rux")];
        let options = Options { release: true, target: Target::Desktop, rux_source: None };
        let main = wrapper_main(&manifest(), &files, &options);
        assert!(main.contains("MemorySource"), "{main}");
        assert!(main.contains("include_bytes!"), "{main}");
        // The key is the document-relative path, not the absolute one, because
        // that is what an import inside the document resolves to.
        assert!(main.contains("\"components/task.rux\""), "{main}");
        assert!(main.contains("set_source"), "{main}");
    }

    #[test]
    fn a_dev_wrapper_embeds_nothing_and_keeps_hot_reload() {
        let options = Options { release: false, target: Target::Desktop, rux_source: None };
        let main = wrapper_main(&manifest(), &[PathBuf::from("app.rux")], &options);
        assert!(!main.contains("include_bytes!"), "a dev build must not embed: {main}");
        assert!(!main.contains("MemorySource"), "a dev build reads the disk: {main}");
        assert!(main.contains("rux_shell::run"), "{main}");
    }

    #[test]
    fn windows_paths_survive_being_written_into_rust_source() {
        assert_eq!(literal(r"C:\a\b.rux"), "\"C:\\\\a\\\\b.rux\"");
        assert_eq!(literal("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn a_target_that_is_coming_says_so_differently_from_one_that_is_not() {
        assert!(Target::parse("android").unwrap_err().contains("not built yet"));
        assert!(Target::parse("banana").unwrap_err().contains("unknown target"));
        assert!(Target::parse("desktop").is_ok());
    }

    #[test]
    fn the_generated_manifest_points_at_a_checkout_when_one_is_given() {
        let options = Options {
            release: true,
            target: Target::Desktop,
            rux_source: Some(PathBuf::from("C:/rux")),
        };
        let toml = wrapper_manifest(&manifest(), &options);
        assert!(toml.contains("C:/rux/crates/rux-shell"), "{toml}");
        assert!(toml.contains("task-list"), "{toml}");
    }
}
