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
//! by hot reload exactly as under `rux run`.
//!
//! **Android is the exception, and it embeds either way.** There is no
//! filesystem on the other side of an APK, so a dev build there carries its
//! documents too and **does not hot reload**. Serving them as assets through
//! Android's `AssetManager` and pushing edits over `adb` is the shape that
//! would fix it, and it is not built. See `wrapper_main`, which is where the
//! decision actually lives.
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
    /// Which Android ABIs to build, when the target is Android.
    ///
    /// `None` lets the build decide: one ABI for a dev build, because four
    /// compiles of the same code is four times the wait for three artifacts
    /// nobody is about to run, and every ABI for a release, because an APK
    /// carrying one installs on a fraction of the devices it claims to support.
    ///
    /// `rux run --device` sets it to whatever the attached device reports, so
    /// plugging in a real phone builds for the phone rather than for the
    /// emulator this all started on.
    pub abis: Option<Vec<crate::android::Abi>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Desktop,
    Android,
    Web,
}

impl Target {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "desktop" => Ok(Target::Desktop),
            "android" => Ok(Target::Android),
            "web" => Ok(Target::Web),
            // Named rather than ignored: iOS is a real target of this milestone,
            // and "unknown target" would read as if it were never coming.
            "ios" => Err("`ios` is not built yet. `desktop`, `android` and `web` are what \
                 `rux build` can produce today"
                .to_string()),
            other => Err(format!(
                "unknown target `{other}`; the ones that exist are `desktop`, `android` and `web`"
            )),
        }
    }

    /// The directory under `.rux-build` this target generates into.
    fn dir(self) -> &'static str {
        match self {
            Target::Desktop => "desktop",
            Target::Android => "android",
            Target::Web => "web",
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

fn wrapper_manifest(manifest: &Manifest, options: &Options, wasm_bindgen: Option<&str>) -> String {
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
    // An Android app is not a binary. `android-activity` reaches it through
    // `android_main` in a shared object the platform loads, so the same wrapper
    // becomes a cdylib with a `lib.rs` rather than a bin with a `main.rs`.
    // `lib.name` is what ends up in `libNAME.so`, and it has to match the
    // `android.app.lib_name` in the generated manifest or the activity starts
    // and finds nothing to run.
    //
    // The web is the same shape again: a browser loads a wasm module, not a
    // binary, and `wasm-bindgen` needs a cdylib to generate its glue from.
    let output = match options.target {
        Target::Desktop => format!(
            "[[bin]]\nname = {}\npath = \"src/main.rs\"\n",
            literal(&manifest.artifact_stem())
        ),
        Target::Android | Target::Web => format!(
            "[lib]\nname = {}\ncrate-type = [\"cdylib\"]\npath = \"src/lib.rs\"\n",
            literal(&lib_stem(manifest))
        ),
    };
    // Only the web wrapper talks to the browser directly, so only it carries
    // the bindings. `wasm-bindgen`'s version has to match the CLI that
    // processes the output, which is checked after the build rather than
    // guessed here; see `web::bundle`.
    // **Pinned to the tool that will process the output**, not left to resolve
    // to the newest `0.2.x`. The two generate and compile the same glue, so a
    // difference produces errors in code nobody wrote; see
    // `web::installed_version` for the run that proved it.
    let deps = match (options.target, wasm_bindgen) {
        (Target::Web, Some(version)) => format!(
            "{deps}wasm-bindgen = \"={version}\"\n\
             web-sys = {{ version = \"0.3\", features = [\"Document\", \"Element\", \
             \"HtmlCanvasElement\", \"Window\"] }}\n"
        ),
        _ => deps,
    };
    // A release for the browser is tuned for size, not speed, because the cost
    // a visitor pays is the download before anything appears. `opt-level = "z"`
    // with LTO and one codegen unit is the difference between a bundle someone
    // will deploy and one they will not.
    let profile = match options.target {
        Target::Web => {
            "\n[profile.release]\n\
             opt-level = \"z\"\n\
             lto = true\n\
             codegen-units = 1\n\
             # Nothing unwinds on wasm anyway, and the panic hook still reports\n\
             # to the console before aborting.\n\
             panic = \"abort\"\n\
             strip = true\n"
        }
        _ => "",
    };
    format!(
        "# Generated by `rux build`. Edits are overwritten on the next build.\n\
         [package]\n\
         name = {}\n\
         version = {}\n\
         edition = \"2021\"\n\
         publish = false\n\
         \n\
         {output}\n\
         [dependencies]\n\
         {deps}{profile}",
        literal(&manifest.artifact_stem()),
        literal(&manifest.version),
    )
}

/// The triple a web build compiles to, and what to call it in a message.
const WASM: (&str, &str) = ("wasm32-unknown-unknown", "the web");

/// Run cargo once, for one cross target or for the host.
///
/// The target is a triple and a name to call it by, because they differ and the
/// readable one is the one worth printing: `arm64-v8a` says more to someone
/// waiting on a build than `aarch64-linux-android` does.
fn compile(work: &Path, options: &Options, target: Option<(&str, &str)>) -> Result<(), String> {
    let mut cargo = std::process::Command::new("cargo");
    cargo.arg("build").current_dir(work);
    if options.release {
        cargo.arg("--release");
    }
    if let Some((triple, _)) = target {
        cargo.args(["--target", triple]);
    }
    let status = cargo.status().map_err(|e| {
        format!(
            "could not run cargo: {e}\n\
             `rux build` compiles a small generated crate, so it needs a Rust toolchain. \
             Install one from https://rustup.rs"
        )
    })?;
    if !status.success() {
        // Named, because after four compiles "it did not build" leaves someone
        // scrolling back to work out which architecture broke.
        return Err(match target {
            Some((_, name)) => format!("the generated crate did not build for {name}"),
            None => "the generated crate did not build".to_string(),
        });
    }
    Ok(())
}

/// The crate name behind `libNAME.so`, which cargo spells with underscores.
///
/// `artifact_stem` is a filename and uses dashes; a Rust library target cannot,
/// so cargo silently rewrites them. Doing it here rather than letting cargo do
/// it means the generated `AndroidManifest.xml` and the built `.so` agree about
/// the name, instead of agreeing only for apps with no space in their title.
fn lib_stem(manifest: &Manifest) -> String {
    manifest.artifact_stem().replace('-', "_")
}

/// The embedding half, shared by every target that carries its documents.
///
/// `include_bytes!` resolves at compile time against the absolute path each
/// file had when this was generated, which is why the generated crate is
/// disposable.
fn embed(manifest: &Manifest, files: &[PathBuf], out: &mut String) {
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
}

fn wrapper_main(manifest: &Manifest, files: &[PathBuf], options: &Options) -> String {
    let entry = key(&manifest.entry);
    let mut out = String::new();
    out.push_str("// Generated by `rux build`. Edits are overwritten on the next build.\n");
    out.push_str("//\n");

    if options.target == Target::Android {
        // An Android app is entered at `android_main` in a shared object, not
        // at a `fn main`, and `#[no_mangle]` is what keeps the symbol findable
        // by the name `NativeActivity` will look for.
        //
        // **Android always embeds, `--release` or not.** Everywhere else a dev
        // build reads the project from disk so hot reload works, and there is
        // no disk on the other side of this one: an APK's documents would have
        // to be assets read through Android's `AssetManager`. Embedding is what
        // makes the first APK possible without that, and the cost is named
        // rather than hidden: a dev build for Android does not hot reload yet.
        out.push_str(
            "// An Android app is entered here, in a shared object the platform loads.\n\
             // Documents are embedded whether or not this is a release build: there is\n\
             // no filesystem on the other side to read them from.\n",
        );
        out.push_str("#[no_mangle]\n");
        out.push_str("fn android_main(app: rux_shell::android_activity::AndroidApp) {\n");
        embed(manifest, files, &mut out);
        out.push_str(&format!(
            "    rux_shell::run_android(app, std::path::PathBuf::from({}));\n",
            literal(&entry)
        ));
        out.push_str("}\n");
        return out;
    }

    if options.target == Target::Web {
        // **The web always embeds too, and for the same reason as Android:**
        // there is no filesystem behind a URL. The documents become a
        // `MemorySource`, and the entry is loaded through it by path, so
        // components, stylesheets and routed pages all resolve exactly as they
        // do on a desktop. The playground's `start_web` cannot do this: it
        // takes one document as text, and `Document::from_source` drops its
        // imports on the floor.
        out.push_str(
            "// A web app is entered by the browser once the module is loaded.\n\
             // Documents are embedded: there is no filesystem behind a URL.\n",
        );
        out.push_str("use wasm_bindgen::prelude::*;\n\n");
        out.push_str("#[wasm_bindgen(start)]\n");
        out.push_str("pub fn main() {\n");
        embed(manifest, files, &mut out);
        out.push_str(
            "    let window = web_sys::window().expect(\"a browser window\");\n\
             \x20   let document = window.document().expect(\"a document\");\n\
             \x20   // The id the generated page gives its canvas. Someone hosting this\n\
             \x20   // module in a page of their own needs one element and this is its name.\n\
             \x20   let canvas = document\n\
             \x20       .get_element_by_id(\"rux\")\n\
             \x20       .expect(\"a <canvas id=\\\"rux\\\"> to draw into\");\n\
             \x20   let canvas: web_sys::HtmlCanvasElement =\n\
             \x20       wasm_bindgen::JsCast::dyn_into(canvas).expect(\"#rux is a <canvas>\");\n",
        );
        out.push_str(&format!(
            "    rux_shell::start_web_app(\n\
             \x20       canvas,\n\
             \x20       {}.to_string(),\n\
             \x20       rux_shell::DEFAULT_FONT.to_vec(),\n\
             \x20       // A built app owns its address bar, so its routes go in the URL.\n\
             \x20       Some(\"/\".to_string()),\n\
             \x20   );\n",
            literal(&entry)
        ));
        out.push_str("}\n");
        return out;
    }

    if options.release {
        out.push_str(
            "// A release build embeds every document, so the artifact carries its own\n\
             // contents and cannot drift from what was tested. `include_bytes!` resolves\n\
             // at compile time against the absolute path each file had when this was\n\
             // generated, which is why the generated crate is disposable.\n",
        );
        out.push_str("fn main() {\n");
        embed(manifest, files, &mut out);
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

    // Resolved before a line is generated, so a missing NDK is reported in the
    // second it takes to look rather than after a three minute compile.
    let toolchain = match options.target {
        Target::Desktop | Target::Web => None,
        Target::Android => Some(crate::android::Toolchain::resolve()?),
    };

    // One ABI for a dev build, every ABI for a release. Four compiles of the
    // same code is four times the wait, and three of those artifacts are for
    // machines nobody is about to run this on. A release cannot make that
    // trade: an APK carrying one ABI installs on a fraction of the devices it
    // claims to support, and Android does not explain why to the person holding
    // the phone.
    let abis: Vec<crate::android::Abi> = match (options.target, &options.abis) {
        (Target::Desktop | Target::Web, _) => Vec::new(),
        (Target::Android, Some(abis)) => abis.clone(),
        (Target::Android, None) if options.release => crate::android::ABIS.to_vec(),
        (Target::Android, None) => vec![crate::android::DEV_ABI],
    };
    // Checked here, before anything compiles. A keystore that is not where the
    // manifest says, or a password that was never exported, is knowable in the
    // moment it takes to look, and finding out afterwards means finding out
    // sixteen minutes later.
    if options.target == Target::Android {
        manifest.signing_key()?;
        if options.release && manifest.signing.is_none() {
            // Said plainly rather than left to be discovered by a store. A
            // debug-signed APK installs perfectly well, which is exactly why
            // this is easy to ship by accident.
            println!(
                "rux: no [signing] block, so this release is signed with the shared debug key.\n\
                 rux: it will install, and no store will accept it. See \
                 https://ruxlang.dev/tooling/build/"
            );
        }
    }

    // Asked once, up front, for all of them. Finding out on the third compile
    // that a target was never installed means waiting through two to learn
    // something that was knowable before the first.
    let missing = crate::android::missing_rust_targets(&abis);
    if !missing.is_empty() {
        let names: Vec<&str> = missing.iter().map(|abi| abi.name).collect();
        let mut message = format!(
            "the Rust {} for {} {} not installed.",
            if missing.len() == 1 { "target" } else { "targets" },
            names.join(", "),
            if missing.len() == 1 { "is" } else { "are" },
        );
        for abi in &missing {
            message.push_str("\n  rustup target add ");
            message.push_str(abi.rust_target);
        }
        return Err(message);
    }

    let files = collect(&manifest.root)?;
    if files.is_empty() {
        return Err(format!("{} holds no files to build", manifest.root.display()));
    }

    // Inside the project, and hidden, so the directory walk above skips it and
    // a `.gitignore` does not have to learn a new name.
    let work = manifest.root.join(".rux-build").join(options.target.dir());
    std::fs::create_dir_all(work.join("src"))
        .map_err(|e| format!("creating {}: {e}", work.display()))?;
    // Asked before anything is generated, so a missing tool is reported now
    // rather than after the compile it would invalidate.
    let wasm_bindgen = match options.target {
        Target::Web => Some(crate::web::installed_version()?),
        _ => None,
    };
    std::fs::write(
        work.join("Cargo.toml"),
        wrapper_manifest(&manifest, &options, wasm_bindgen.as_deref()),
    )
        .map_err(|e| format!("writing the generated manifest: {e}"))?;
    // A bin is entered at `main.rs` and a cdylib at `lib.rs`, and the generated
    // `Cargo.toml` names whichever this is.
    let entry_file = match options.target {
        Target::Desktop => "src/main.rs",
        Target::Android | Target::Web => "src/lib.rs",
    };
    std::fs::write(work.join(entry_file), wrapper_main(&manifest, &files, &options))
        .map_err(|e| format!("writing the generated entry point: {e}"))?;

    // cargo is told which linker to use through the generated crate's own
    // config rather than through the environment, so that the choice travels
    // with the build and a shell that never set `CARGO_TARGET_*` still works.
    if let Some(toolchain) = &toolchain {
        let config = work.join(".cargo");
        std::fs::create_dir_all(&config)
            .map_err(|e| format!("creating {}: {e}", config.display()))?;
        // A section per ABI, so one file serves every compile of this build
        // rather than being rewritten between them.
        let mut text =
            String::from("# Generated by `rux build`. Edits are overwritten on the next build.\n");
        for abi in &abis {
            text.push_str(&format!(
                "[target.{}]\nlinker = {}\n",
                abi.rust_target,
                literal(&toolchain.clang(*abi).display().to_string()),
            ));
        }
        std::fs::write(config.join("config.toml"), text)
            .map_err(|e| format!("writing the generated cargo config: {e}"))?;
    }

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

    // One cargo invocation per ABI. Cargo can be given several `--target`
    // flags at once, and deliberately is not: a failure then reports against
    // the whole set rather than naming the architecture that broke, and the
    // architectures are exactly what differ.
    let mut compiles: Vec<(crate::android::Abi, PathBuf)> = Vec::new();
    for abi in &abis {
        if abis.len() > 1 {
            println!("rux: compiling for {}", abi.name);
        }
        compile(&work, &options, Some((abi.rust_target, abi.name)))?;
        compiles.push((
            *abi,
            work.join("target")
                .join(abi.rust_target)
                .join(mode)
                .join(format!("lib{}.so", lib_stem(&manifest))),
        ));
    }
    if abis.is_empty() {
        let host_or_wasm = (options.target == Target::Web).then_some(WASM);
        compile(&work, &options, host_or_wasm)?;
    }

    let dist = manifest.root.join("dist");
    std::fs::create_dir_all(&dist).map_err(|e| format!("creating {}: {e}", dist.display()))?;

    if let Some(toolchain) = &toolchain {
        let out = dist.join(format!("{}.apk", manifest.artifact_stem()));
        crate::apk::pack(&manifest, toolchain, &compiles, &work, &out)?;
        println!(
            "rux: wrote {} [{}]",
            out.display(),
            compiles.iter().map(|(abi, _)| abi.name).collect::<Vec<_>>().join(", ")
        );
        return Ok(out);
    }

    if options.target == Target::Web {
        // A web app is a directory, not a file: the module, the glue that loads
        // it, and a page that puts the two together. So it gets one of its own
        // rather than three loose files beside a desktop executable.
        let built = work
            .join("target")
            .join(WASM.0)
            .join(mode)
            .join(format!("{}.wasm", lib_stem(&manifest)));
        let out_dir = dist.join("web");
        let page = crate::web::bundle(&manifest, &work, &built, &out_dir)?;
        println!("rux: wrote {}", out_dir.display());
        if !options.release {
            // A debug wasm is many times the size of a release one, and the
            // difference is a download a visitor waits through. Worth saying
            // before someone deploys one rather than after.
            println!("rux: this is a debug build; `--release` is far smaller and is what to deploy");
        }
        println!(
            "rux: open {} through a web server, not as a file:// path; a module will not load \
             from one",
            page.file_name().unwrap_or_default().to_string_lossy()
        );
        return Ok(page);
    }

    let exe = format!("{}{}", manifest.artifact_stem(), std::env::consts::EXE_SUFFIX);
    let built = work.join("target").join(mode).join(&exe);
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
            signing: None,
            icon: None,
        }
    }

    #[test]
    fn a_release_wrapper_embeds_every_file_and_installs_the_memory_source() {
        let files = vec![PathBuf::from("app.rux"), PathBuf::from("components/task.rux")];
        let options = Options { release: true, target: Target::Desktop, rux_source: None, abis: None };
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
        let options = Options { release: false, target: Target::Desktop, rux_source: None, abis: None };
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
        // `ios` is named rather than refused as nonsense, because it is a real
        // target of this milestone and "unknown target" would read as if it
        // were never coming. `android` used to be in this list and is not any
        // more, which is the point of the slice this test changed in.
        assert!(Target::parse("ios").unwrap_err().contains("not built yet"));
        assert!(Target::parse("banana").unwrap_err().contains("unknown target"));
        assert_eq!(Target::parse("desktop"), Ok(Target::Desktop));
        assert_eq!(Target::parse("android"), Ok(Target::Android));
    }

    #[test]
    fn the_generated_manifest_points_at_a_checkout_when_one_is_given() {
        let options = Options {
            release: true,
            target: Target::Desktop,
            rux_source: Some(PathBuf::from("C:/rux")),
            abis: None,
        };
        let toml = wrapper_manifest(&manifest(), &options, None);
        assert!(toml.contains("C:/rux/crates/rux-shell"), "{toml}");
        assert!(toml.contains("task-list"), "{toml}");
    }
}
