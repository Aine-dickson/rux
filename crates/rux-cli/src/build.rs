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
//! documents too, and hot reloads by having them sent again: the app dials
//! `rux run --device` through `adb reverse` and patches what it embedded.
//! See `wrapper_main` and `crate::device::serve`.
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

/// The route a dev build opens on, which `rux run --route` sets for the build
/// it starts.
pub const ROUTE_VAR: &str = "RUX_RUN_ROUTE";
/// The device profile a dev build previews, from `rux run --preview`.
pub const PREVIEW_VAR: &str = "RUX_RUN_PREVIEW";

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
    /// Whether `[build] before` runs. Only `rux run` for a native app says no:
    /// it has run them once already, and its watchers are writing the same
    /// files while it rebuilds.
    pub before: bool,
    /// `--locked`: build with exactly the versions in `locks/<target>.lock`,
    /// and fail rather than change them. What CI passes, so a build is the
    /// one that was tested. See `kept_lock`.
    pub locked: bool,
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
pub(crate) fn collect(root: &Path) -> Result<Vec<PathBuf>, String> {
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
        // The app's Rust is compiled in, never shipped as files: a release
        // carrying its own source is a leak, not an asset.
        if dir == root && name == crate::native::DIR && path.join("Cargo.toml").is_file() {
            continue;
        }
        // Nor are the kept locks (`kept_lock`): they describe how it is built.
        if dir == root && name == LOCKS_DIR {
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
pub(crate) fn key(path: &Path) -> String {
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

fn wrapper_manifest(
    manifest: &Manifest,
    options: &Options,
    wasm_bindgen: Option<&str>,
    native: Option<&crate::native::NativeCrate>,
) -> String {
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
    // **A debug build runs Rux optimised**, and only the app's own Rust as
    // written, which is what a debugger is for. Unoptimised, Rux's layout of
    // a 1,000-row list took 12.7 ms a frame against 4.2 in a release build,
    // and the app paid for it in every window it opened from `rux run`. The
    // cost is the first build, once.
    let own_debug = native.map_or(String::new(), |n| {
        format!("\n[profile.dev.package.{}]\nopt-level = 0\n", literal(&n.package))
    });
    let profile = format!("{profile}\n[profile.dev.package.\"*\"]\nopt-level = 2\n{own_debug}");
    // The app's own Rust, and, when Rux itself comes from a checkout, the
    // checkout's rux-native for it too: two copies of the registry would be
    // two registries, and the app's exports would land in the one nobody
    // reads.
    //
    // A checkout's own `[patch]` never reaches a crate outside its workspace,
    // so the vendored taffy (its `calc()` resolver, see `vendor/README.md`)
    // is patched in here too, or the wrapper resolves taffy from crates.io
    // and rux-layout does not compile against it.
    let patch = match &options.rux_source {
        Some(dir) => {
            let dir = dir.display().to_string().replace('\\', "/");
            // The vendored vello with it, which sizes its buffers to the scene.
            let mut patch = format!(
                "\n[patch.crates-io]\ntaffy = {{ path = {} }}\nvello = {{ path = {} }}\nvello_encoding = {{ path = {} }}\n",
                literal(&format!("{dir}/vendor/taffy")),
                literal(&format!("{dir}/vendor/vello")),
                literal(&format!("{dir}/vendor/vello_encoding")),
            );
            if native.is_some() {
                for (name, path) in crate::native::checkout_crates(Path::new(&dir)) {
                    patch.push_str(&format!("{name} = {{ path = {} }}\n", literal(&path)));
                }
            }
            patch
        }
        None => String::new(),
    };
    let deps = match native {
        None => deps,
        Some(n) => format!("{deps}{}", crate::native::dependency(n)),
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
         {deps}{profile}{patch}\n\
         # Its own workspace, so a project inside another one (a monorepo, or\n\
         # an example in Rux's own repository) still builds.\n\
         [workspace]\n",
        // Not the app's own name: an app's native/ crate is likely to have
        // that one, and cargo cannot hold two packages of one name. What is
        // built is named by [[bin]] or [lib] above, not by this.
        literal(&format!("rux-app-{}", manifest.artifact_stem())),
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
    if options.locked {
        cargo.arg("--locked");
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
        let mut message = match target {
            Some((_, name)) => format!("the generated crate did not build for {name}"),
            None => "the generated crate did not build".to_string(),
        };
        if options.locked {
            message.push_str(
                ". If cargo said it cannot update the lock file, a dependency or the app's \
                 version changed: build once without --locked and commit the new locks/ file",
            );
        }
        return Err(message);
    }
    Ok(())
}

/// The directory of kept locks, at the project root.
const LOCKS_DIR: &str = "locks";

/// Where a target's lock is kept in the project: `locks/<target>.lock`.
///
/// **The generated crate has a `Cargo.lock` of its own, and it lives in
/// `.rux-build`, which nobody commits.** Without a copy that is kept, a fresh
/// clone or a CI job resolved every dependency to whatever was newest that day:
/// Rux's own, and anything added to `native/` with `cargo add`. So the lock is
/// kept in the project, one file per target, and put back before each build.
///
/// One file per target rather than one shared, because the wrappers do not
/// have the same dependencies (the web one carries `wasm-bindgen`). A shared
/// lock would be pruned by whichever target built last, and the next target
/// would resolve what was pruned afresh: the drift this exists to stop.
///
/// Not a root `Cargo.lock`, which is the name cargo gives a workspace at the
/// root of the project, such as the one a `native/` crate may sit in.
fn kept_lock(root: &Path, target: Target) -> PathBuf {
    root.join(LOCKS_DIR).join(format!("{}.lock", target.dir()))
}

/// Copy the kept lock over the generated crate's. `false` when there is none.
fn install_lock(kept: &Path, lock: &Path) -> Result<bool, String> {
    if !kept.is_file() {
        return Ok(false);
    }
    std::fs::copy(kept, lock)
        .map_err(|e| format!("copying {} to {}: {e}", kept.display(), lock.display()))?;
    Ok(true)
}

/// Copy the generated crate's lock back into the project. `true` when that
/// created or changed the kept one, so an unchanged build says nothing.
fn keep_lock(lock: &Path, kept: &Path) -> Result<bool, String> {
    let Ok(new) = std::fs::read(lock) else { return Ok(false) };
    if std::fs::read(kept).is_ok_and(|old| old == new) {
        return Ok(false);
    }
    if let Some(dir) = kept.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    }
    std::fs::write(kept, new).map_err(|e| format!("writing {}: {e}", kept.display()))?;
    Ok(true)
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
    embed_files(manifest, files, out);
    out.push_str("    rux_runtime::set_source(std::rc::Rc::new(files));\n");
}

/// The `MemorySource` alone, left in a local called `files` for the caller.
fn embed_files(manifest: &Manifest, files: &[PathBuf], out: &mut String) {
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
    embed_icons(manifest, files, out);
}


/// The port a dev build and `rux run --device` meet on, one per app.
///
/// Derived from the app id rather than chosen per run, so the generated crate
/// does not change (and rebuild) every time, and two apps on one machine
/// rarely want the same port. FNV-1a, spelled out, because `DefaultHasher`
/// may change between Rust releases and both ends must agree.
pub fn dev_port(manifest: &Manifest) -> u16 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in manifest.id.bytes() {
        hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
    }
    40_000 + (hash % 20_000) as u16
}

/// The icons this project names, and only those.
///
/// **This is tree-shaking, and it is why the icon data lives in the tool rather
/// than in the runtime.** The generated crate depends on `rux-runtime`, and a
/// cargo dependency is resolved long before anyone knows which icons a document
/// mentions, so a full table behind that edge would be compiled into every app
/// with no way to remove it. Here, the side that has already read the documents
/// looks each name up while it still has all six thousand, and writes out the
/// handful that were used.
fn embed_icons(manifest: &Manifest, files: &[PathBuf], out: &mut String) {
    let mut wanted: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut dynamic = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(manifest.root.join(file)) else { continue };
        for used in scan_icons(&text) {
            if used.bound {
                dynamic.push(format!("{}:{}", file.display(), used.line));
            } else if let Some(name) = used.name {
                wanted.insert(name);
            }
        }
    }

    // **A bound name cannot be resolved without running the app**, so the
    // honest answer is all of them rather than a guess that leaves an icon
    // blank on a screen nobody tested. Said out loud, because the cost lands in
    // the artifact and an author should learn why from the build rather than
    // from a file listing.
    if !dynamic.is_empty() {
        println!(
            "rux: an icon name is bound ({}), so all {} icons are embedded. \
             A literal `name=` embeds only what it uses.",
            dynamic[0],
            rux_icons::count()
        );
        wanted = rux_icons::names().map(str::to_string).collect();
    }

    if wanted.is_empty() {
        return;
    }
    emit_icons(&wanted, out);
}

/// Every icon, for a dev build.
///
/// A dev build reads the project from disk and hot reloads it, so a document can
/// name an icon the moment after the build was generated. Embedding only what the
/// files name now would leave that icon blank until a rebuild (MS-014). The cost
/// is said, because it lands in the binary and the compile.
fn embed_all_icons(out: &mut String) {
    println!(
        "rux: a dev build embeds all {} icons, since a hot reload can name any of them. \
         `--release` embeds only the ones used.",
        rux_icons::count()
    );
    let all: std::collections::BTreeSet<String> = rux_icons::names().map(str::to_string).collect();
    emit_icons(&all, out);
}

fn emit_icons(wanted: &std::collections::BTreeSet<String>, out: &mut String) {
    // **A blob and an index, not thousands of literals in a function body.**
    // The first version emitted one `MemoryIcons::insert` with two inline
    // `vec![]` per icon. That is fine for the nine a normal app names and fatal
    // for the whole set: an app with a bound name embeds all of them, and five
    // thousand inline vector constructions in one function overflowed the stack
    // of Android's `android_main` thread. The app died with `SIGSEGV` before it
    // drew anything, and only building one found it. Statics take no stack, and
    // this is the same shape `rux-icons` itself is generated in.
    let mut blob = String::new();
    let mut index = String::new();
    let mut embedded = 0usize;
    for name in wanted {
        // A name that is not in the set is skipped rather than failing here.
        // `verify_icons` has already refused the build if one was misspelled,
        // so reaching this with an unknown name means the set changed under us.
        if !rux_icons::exists(name) {
            continue;
        }
        let name_at = blob.len();
        blob.push_str(name);
        let outline_at = blob.len();
        pack_into(&mut blob, name, rux_icons::Variant::Outline);
        let filled_at = blob.len();
        pack_into(&mut blob, name, rux_icons::Variant::Filled);
        index.push_str(&format!(
            "[{},{},{},{},{},{}],",
            name_at,
            outline_at - name_at,
            outline_at,
            filled_at - outline_at,
            filled_at,
            blob.len() - filled_at
        ));
        embedded += 1;
    }

    out.push_str(&format!("    static ICON_BLOB: &str = {};\n", literal(&blob)));
    out.push_str(&format!("    static ICON_INDEX: [[usize; 6]; {embedded}] = [{index}];\n"));
    out.push_str(&format!(
        "    let mut icons = rux_runtime::MemoryIcons::new({:?});\n",
        rux_icons::GRID
    ));
    out.push_str(
        "    for r in ICON_INDEX.iter() {\n\
         \x20       icons.insert_packed(\n\
         \x20           &ICON_BLOB[r[0]..r[0] + r[1]],\n\
         \x20           &ICON_BLOB[r[2]..r[2] + r[3]],\n\
         \x20           &ICON_BLOB[r[4]..r[4] + r[5]],\n\
         \x20       );\n\
         \x20   }\n",
    );
    out.push_str("    rux_runtime::set_icons(std::rc::Rc::new(icons));\n");
    println!("rux: {embedded} icons embedded");
}

/// One variant's paths, in the packed form the generated crate unpacks.
///
/// Paths joined by `\n`, each prefixed by a hex digit of paint flags. Nothing
/// at all when the variant does not exist, which is what `insert_packed` reads
/// as absent rather than empty.
fn pack_into(blob: &mut String, name: &str, variant: rux_icons::Variant) {
    let Some(paths) = rux_icons::find(name, variant) else { return };
    for (i, p) in paths.iter().enumerate() {
        if i > 0 {
            blob.push('\n');
        }
        let flags = u32::from(p.fill_current)
            | (u32::from(p.no_stroke) << 1)
            | (u32::from(p.half_opacity) << 2);
        blob.push(char::from_digit(flags, 16).expect("flags fit a hex digit"));
        blob.push_str(p.d);
    }
}

/// Replace everything that is not markup with spaces, keeping every newline.
///
/// Comments and `<script>` bodies both contain text that looks like a tag and
/// is not one. Spaces rather than deletion, and newlines kept, because the line
/// numbers this feeds are the ones an author reads in the error.
fn blank_non_markup(text: &str) -> String {
    let bytes: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    let blank = |out: &mut String, slice: &[char]| {
        for c in slice {
            out.push(if *c == '\n' || *c == '\r' { *c } else { ' ' });
        }
    };
    while i < bytes.len() {
        let rest: String = bytes[i..].iter().take(9).collect();
        if rest.starts_with("<!--") {
            let end = find_from(&bytes, i, "-->").map(|e| e + 3).unwrap_or(bytes.len());
            blank(&mut out, &bytes[i..end]);
            i = end;
        } else if rest.to_ascii_lowercase().starts_with("<script") {
            let end = find_from(&bytes, i, "</script>").map(|e| e + 9).unwrap_or(bytes.len());
            blank(&mut out, &bytes[i..end]);
            i = end;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// Where `needle` next starts at or after `from`, in chars.
fn find_from(chars: &[char], from: usize, needle: &str) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    (from..chars.len().saturating_sub(needle.len() - 1))
        .find(|&i| {
            chars[i..i + needle.len()]
                .iter()
                .zip(&needle)
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
        })
}

/// One `<icon>` as written in a document.
#[derive(Debug, PartialEq, Eq)]
pub struct IconUse {
    /// The literal `name`, or `None` for a bound or missing one.
    pub name: Option<String>,
    /// Whether the name is bound, which makes it unknowable until the app runs.
    pub bound: bool,
    /// Whether `variant="filled"` was asked for.
    pub filled: bool,
    /// 1-based, so a message can be read against the file.
    pub line: usize,
}

/// Every `<icon>` a document writes, with where it was written.
///
/// Text rather than a parse, deliberately: this runs over the same files the
/// build is about to embed, and a second full parse to answer one question
/// would double the work for no more certainty than the attribute already
/// gives.
fn scan_icons(text: &str) -> Vec<IconUse> {
    // **Comments and scripts are not markup**, and a scan that forgets it reads
    // the word `<icon>` in a sentence about icons as an icon with no name.
    // Found by building a probe whose opening comment described what it drew:
    // the build refused, naming line 1. Blanked rather than removed, so every
    // line number afterwards is still the line the author sees.
    let text = &blank_non_markup(text);
    let mut out = Vec::new();
    let mut rest = text.as_str();
    let mut consumed = 0usize;
    while let Some(at) = rest.find("<icon") {
        let after = &rest[at + "<icon".len()..];
        // A tag name ends at whitespace or at the end of the tag. Without this,
        // `<iconography>` would be read as an icon.
        if !after.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            consumed += at + "<icon".len();
            rest = after;
            continue;
        }
        let end = after.find('>').unwrap_or(after.len());
        let attrs = &after[..end];
        // Counted from the start of the file rather than tracked as we go, so
        // the number is right whatever the line endings are.
        let line = text[..consumed + at].matches('\n').count() + 1;
        let bound = attr_value(attrs, ":name").is_some();
        out.push(IconUse {
            name: attr_value(attrs, "name").filter(|_| !bound),
            bound,
            filled: attr_value(attrs, "variant").as_deref() == Some("filled"),
            line,
        });
        consumed += at + "<icon".len() + end;
        rest = &after[end..];
    }
    out
}

/// Refuse to build a project whose icons cannot be drawn.
///
/// **`rux build` does not otherwise check a document**, so without this a
/// misspelled name is skipped at embedding time and the app ships with a hole
/// where an icon should be. `rux check` says the same things, and an author who
/// runs it first sees them first, but a build must not be the step that stays
/// quiet about something it is in the act of getting wrong.
///
/// Checked before anything compiles, for the reason the keystore is: this is
/// knowable in the time it takes to read the files, and finding out afterwards
/// means finding out sixteen minutes later.
fn verify_icons(manifest: &Manifest, files: &[PathBuf]) -> Result<(), String> {
    let mut problems = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(manifest.root.join(file)) else { continue };
        for used in scan_icons(&text) {
            let at = format!("{}:{}", file.display(), used.line);
            match (&used.name, used.bound) {
                // Unknowable until the app runs, so there is nothing to check.
                (_, true) => {}
                (None, false) => {
                    problems.push(format!("{at}: <icon> needs a `name`"));
                }
                (Some(name), false) if !rux_icons::exists(name) => {
                    problems.push(format!("{at}: there is no icon called `{name}`"));
                }
                (Some(name), false) if used.filled && !rux_icons::has_filled(name) => {
                    problems.push(format!(
                        "{at}: `{name}` has no filled artwork, and `variant=\"filled\"` \
                         does not fall back to the outline"
                    ));
                }
                _ => {}
            }
        }
    }
    if problems.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{}\n\nRoughly four icons in five are outline only, and a name has to be one \
         Tabler draws. `rux check` reports these with the rest of a document's problems.",
        problems.join("\n")
    ))
}

/// One attribute's value out of an element's attribute text.
fn attr_value(attrs: &str, name: &str) -> Option<String> {
    let mut rest = attrs;
    loop {
        let at = rest.find(name)?;
        let before_ok = at == 0 || rest[..at].ends_with(char::is_whitespace);
        let after = rest[at + name.len()..].trim_start();
        if before_ok && after.starts_with('=') {
            let q = after[1..].trim_start().strip_prefix('"')?;
            let close = q.find('"')?;
            return Some(q[..close].to_string());
        }
        rest = &rest[at + name.len()..];
    }
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
        // no disk on the other side of this one. A dev build hot reloads
        // anyway: it starts from what it embedded and `rux run --device` sends
        // it every file that changes after that.
        out.push_str(
            "// An Android app is entered here, in a shared object the platform loads.\n\
             // Documents are embedded whether or not this is a release build: there is\n\
             // no filesystem on the other side to read them from.\n",
        );
        out.push_str("#[no_mangle]\n");
        out.push_str("fn android_main(app: rux_shell::android_activity::AndroidApp) {\n");
        if options.release {
            embed(manifest, files, &mut out);
            out.push_str(&format!(
                "    rux_shell::run_android(app, std::path::PathBuf::from({}));\n",
                literal(&entry)
            ));
        } else {
            // A dev build hot reloads: it keeps a line open to `rux run
            // --device` and patches these files with whatever it is sent.
            embed_files(manifest, files, &mut out);
            out.push_str(&format!(
                "    rux_shell::run_android_dev(app, std::path::PathBuf::from({}), files, {});\n",
                literal(&entry),
                dev_port(manifest)
            ));
        }
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
            "    rux_shell::run_windowed(std::path::PathBuf::from({}), None, None, {});\n",
            literal(&entry),
            window_settings(manifest)
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
        embed_all_icons(&mut out);
        out.push_str(&format!(
            "    let root =std::path::PathBuf::from({});\n",
            literal(&root.display().to_string())
        ));
        // What `rux run --route` and `--preview` asked for, handed over by
        // `native::run`.
        out.push_str(&format!("    let route = std::env::var({ROUTE_VAR:?}).ok();\n"));
        out.push_str(&format!(
            "    let preview = std::env::var({PREVIEW_VAR:?}).ok().and_then(|n| rux_shell::DeviceProfile::by_name(&n));\n"
        ));
        out.push_str(&format!(
            "    rux_shell::run_windowed(root.join({}), route, preview, {});\n",
            literal(&entry),
            window_settings(manifest)
        ));
        out.push_str("}\n");
    }
    out
}

/// The manifest's window, written out as the expression the wrapper hands
/// the shell: the app's name as its title, and `[window]`.
fn window_settings(manifest: &Manifest) -> String {
    let w = manifest.window_settings();
    format!(
        "rux_shell::WindowSettings {{ title: Some({}.into()), width: {:?}, height: {:?}, \
         min_width: {:?}, min_height: {:?}, maximized: {}, frameless: {}, \
         title_overlay: {} }}",
        literal(&manifest.name),
        w.width,
        w.height,
        w.min_width,
        w.min_height,
        w.maximized,
        w.frameless,
        w.title_overlay,
    )
}

/// Where cargo puts what it builds for the wrapper at `work`: its own
/// `target/`, unless `CARGO_TARGET_DIR` says otherwise, which is how a build
/// shares one already compiled.
fn target_dir(work: &Path) -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) if !dir.is_empty() => {
            let dir = PathBuf::from(dir);
            if dir.is_absolute() { dir } else { work.join(dir) }
        }
        _ => work.join("target"),
    }
}

/// `main` with `code` first in its entry function, whichever target's that
/// is: the app's native exports are registered before anything loads.
fn with_registration(main: &str, code: &str) -> String {
    for header in ["fn android_main(app: rux_shell::android_activity::AndroidApp) {\n", "pub fn main() {\n", "fn main() {\n"] {
        if let Some(at) = main.find(header) {
            let at = at + header.len();
            return format!("{}{code}{}", &main[..at], &main[at..]);
        }
    }
    main.to_string()
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
    // sixteen minutes later. An icon nobody can draw is the same kind of thing.
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

    // After every check that costs nothing and before the walk, so what these
    // write (Tailwind's stylesheet, say) is collected like anything else.
    if options.before {
        crate::commands::run_before(&manifest.build_before, &manifest.root)?;
    }

    let mut files = collect(&manifest.root)?;
    if files.is_empty() {
        return Err(format!("{} holds no files to build", manifest.root.display()));
    }
    // Every `<style>` that uses Tailwind's directives, compiled here and
    // carried in, so the app never needs the CLI where it runs. The results
    // sit in `.rux-build`, which the walk above skips, and are found by the
    // app under the same path. See `rux_runtime::tailwind`.
    files.extend(rux_runtime::tailwind::precompile(&manifest.root, &files)?);

    // Before the wrapper is written and long before anything compiles. An icon
    // that cannot be drawn would otherwise be skipped silently at embedding
    // time and ship as a hole in the app.
    verify_icons(&manifest, &files)?;

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
    // The app's Rust, read for what to register before anything is written:
    // a problem in it is reported where it is, not as a compile error in
    // generated code.
    let native = crate::native::find(&manifest.root)?;
    let registration = match &native {
        Some(n) => Some(crate::native::registration(n, &rux_bindgen::scan(&n.lib))?),
        None => None,
    };
    std::fs::write(
        work.join("Cargo.toml"),
        wrapper_manifest(&manifest, &options, wasm_bindgen.as_deref(), native.as_ref()),
    )
        .map_err(|e| format!("writing the generated manifest: {e}"))?;
    // A bin is entered at `main.rs` and a cdylib at `lib.rs`, and the generated
    // `Cargo.toml` names whichever this is.
    let entry_file = match options.target {
        Target::Desktop => "src/main.rs",
        Target::Android | Target::Web => "src/lib.rs",
    };
    // Built against a checkout, the wrapper starts from the checkout's lock:
    // the versions Rux was tested with, and what an already-built target
    // holds. Cargo trims it to what the wrapper uses.
    //
    // **A lock the project keeps comes first**: see `kept_lock`. It is what
    // makes this build the one that was tested, and the checkout's lock only
    // stands in for it until the first build has written one.
    let kept = kept_lock(&manifest.root, options.target);
    if !install_lock(&kept, &work.join("Cargo.lock"))? {
        if options.locked {
            return Err(format!(
                "--locked needs {}, and there is none. Build once without --locked, then commit \
                 the file it writes",
                kept.display()
            ));
        }
        if let Some(dir) = &options.rux_source {
            let lock = work.join("Cargo.lock");
            if !lock.exists() {
                let _ = std::fs::copy(dir.join("Cargo.lock"), &lock);
            }
        }
    }
    let mut main = wrapper_main(&manifest, &files, &options);
    if let Some(code) = &registration {
        main = with_registration(&main, code);
    }
    // Which app this is, for where `settings.set` keeps its values. First, so
    // nothing the app runs at start can read another app's.
    main = with_registration(&main, &format!("    rux_shell::set_app_id({});\n", literal(&manifest.id)));
    // A release build's script, compiled (step 9 of docs/archive/language-build-log.md).
    let _ = std::fs::remove_file(work.join("src").join("rux_aot.rs"));
    if options.release {
        if let Some((code, line)) = crate::aot::generate(&manifest) {
            std::fs::write(work.join("src").join("rux_aot.rs"), code)
                .map_err(|e| format!("writing the compiled script: {e}"))?;
            main = with_registration(&main, "    rux_aot::install();\n");
            main = format!("mod rux_aot;\n{main}");
            println!("rux: {line}");
        }
    }
    std::fs::write(work.join(entry_file), main)
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
        // And which C compiler, for a dependency with C in it: `ring`, under
        // the TLS a network image is fetched over, compiles some. `cc` reads
        // `CC_<target>` and `AR_<target>`.
        text.push_str("[env]\n");
        for abi in &abis {
            let clang = toolchain.clang(*abi);
            let ar = clang.with_file_name(if cfg!(windows) { "llvm-ar.exe" } else { "llvm-ar" });
            let var = abi.rust_target.replace('-', "_");
            text.push_str(&format!("CC_{var} = {}\n", literal(&clang.display().to_string())));
            text.push_str(&format!("AR_{var} = {}\n", literal(&ar.display().to_string())));
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
            target_dir(&work)
                .join(abi.rust_target)
                .join(mode)
                .join(format!("lib{}.so", lib_stem(&manifest))),
        ));
    }
    if abis.is_empty() {
        let host_or_wasm = (options.target == Target::Web).then_some(WASM);
        compile(&work, &options, host_or_wasm)?;
    }
    // After the compile, so a lock is only ever kept for a set of versions
    // that built. Under `--locked` cargo has refused to change it already.
    if !options.locked && keep_lock(&work.join("Cargo.lock"), &kept)? {
        println!("rux: wrote {}; commit it so every machine builds the same versions", kept.display());
    }

    let dist =manifest.root.join("dist");
    std::fs::create_dir_all(&dist).map_err(|e| format!("creating {}: {e}", dist.display()))?;

    if let Some(toolchain) = &toolchain {
        let out = dist.join(format!("{}.apk", manifest.artifact_stem()));
        crate::apk::pack(&manifest, toolchain, &compiles, &work, &out, !options.release)?;
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
        let built = target_dir(&work)
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
    let built = target_dir(&work).join(mode).join(&exe);
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

    /// What `scan_icons` finds in one document, as the older tests read it.
    fn scan(text: &str) -> (Vec<String>, bool) {
        let uses = scan_icons(text);
        let mut names: Vec<String> =
            uses.iter().filter_map(|u| u.name.clone()).collect();
        names.sort();
        names.dedup();
        (names, uses.iter().any(|u| u.bound))
    }

    #[test]
    fn only_the_icons_a_document_names_are_collected() {
        let (names, dynamic) = scan(
            r#"<icon name="heart" /><icon name="star" size="2em"/><icon name="heart"/>"#,
        );
        assert_eq!(names, vec!["heart", "star"], "duplicates should collapse");
        assert!(!dynamic);
    }

    #[test]
    fn a_tag_that_merely_starts_with_icon_is_not_one() {
        // `<iconography>` is somebody's component. Without the boundary check
        // this would scan its attributes as an icon's and embed nothing useful
        // while looking like it worked.
        let (names, _) = scan(r#"<iconography name="heart" />"#);
        assert!(names.is_empty(), "{names:?}");
    }

    #[test]
    fn a_bound_name_is_reported_because_it_cannot_be_shaken() {
        // The whole point of the scan is knowing what to embed. A bound name
        // cannot be known without running the app, so the build has to say so
        // rather than quietly ship an app with a blank space in it.
        let (names, dynamic) = scan(r#"<icon :name="chosen" />"#);
        assert!(names.is_empty());
        assert!(dynamic, "a bound name was not noticed");
    }

    #[test]
    fn the_embedded_icons_are_the_ones_named_and_no_others() {
        let mut blob = String::new();
        pack_into(&mut blob, "heart", rux_icons::Variant::Outline);
        assert!(!blob.is_empty(), "heart packed to nothing");
        // The flag digit, then a command letter. If the digit were lost the
        // data would still look plausible and every override would be gone.
        assert!(blob.starts_with(|c: char| c.is_ascii_hexdigit()), "{blob}");
        assert!(blob[1..].starts_with(|c: char| c.is_ascii_alphabetic()), "{blob}");

        // A name nobody asked for must not be in there. `star` is a real icon,
        // which is what makes it a fair check that only `heart` was taken.
        let mut other = String::new();
        pack_into(&mut other, "star", rux_icons::Variant::Outline);
        assert!(!blob.contains(&other));
    }

    #[test]
    fn a_comment_that_talks_about_icons_is_not_an_icon() {
        // Found by building a probe whose opening comment described what it
        // drew. The build refused, naming line 1, and the document was fine.
        // Now that a bad icon fails a build, a false positive is not a nuisance
        // but a valid project that cannot be built.
        let uses = scan_icons("<!-- the <icon> element, driven -->\n<icon name=\"heart\" />");
        assert_eq!(uses.len(), 1, "the comment was read as markup: {uses:?}");
        assert_eq!(uses[0].name.as_deref(), Some("heart"));
        assert_eq!(uses[0].line, 2, "blanking a comment moved the line numbers");
    }

    #[test]
    fn a_script_that_mentions_an_icon_tag_is_not_one() {
        // A string in a script is not markup either, and the same false
        // positive would be harder to see coming.
        let uses = scan_icons("<script>\n  let s = \"<icon name=\\\"x\\\" />\";\n</script>");
        assert!(uses.is_empty(), "a script body was read as markup: {uses:?}");
    }

    /// The three lines both line-ending tests scan, joined per test.
    const DOC: [&str; 3] = ["<template>", "  <screen>", "    <icon name=\"heart\" />"];

    #[test]
    fn an_icon_carries_the_line_it_was_written_on() {
        // Without a position these messages are a list of names and a hunt.
        let uses = scan_icons(&DOC.join("\n"));
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].line, 3, "{:?}", uses[0]);
    }

    #[test]
    fn line_numbers_survive_windows_line_endings() {
        // This tree mixes CRLF and LF file by file, so a count that only works
        // for one of them is a number that is wrong on half the repository.
        let uses = scan_icons(&DOC.join("\r\n"));
        assert_eq!(uses[0].line, 3, "{:?}", uses[0]);
    }

    #[test]
    fn a_variant_is_read_and_a_bound_name_is_not_a_name() {
        let uses = scan_icons(r#"<icon name="a" variant="filled" /><icon :name="x" />"#);
        assert!(uses[0].filled);
        assert_eq!(uses[0].name.as_deref(), Some("a"));
        assert!(uses[1].bound);
        assert!(uses[1].name.is_none(), "a bound name is not a literal one");
    }

    fn manifest() -> Manifest {
        Manifest {
            root: PathBuf::from("."),
            name: "Task List".into(),
            id: "dev.example.tasks".into(),
            version: "0.1.0".into(),
            entry: PathBuf::from("app.rux"),
            signing: None,
            icon: None,
            scheme: None,
            link_hosts: Vec::new(),
            window: Default::default(),
            permissions: Default::default(),
            dev_watch: Vec::new(),
            build_before: Vec::new(),
        }
    }

    #[test]
    fn a_release_wrapper_embeds_every_file_and_installs_the_memory_source() {
        let files = vec![PathBuf::from("app.rux"), PathBuf::from("components/task.rux")];
        let options = Options { release: true, target: Target::Desktop, rux_source: None, abis: None, before: true, locked: false };
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
        let options = Options { release: false, target: Target::Desktop, rux_source: None, abis: None, before: true, locked: false };
        let main = wrapper_main(&manifest(), &[PathBuf::from("app.rux")], &options);
        assert!(!main.contains("include_bytes!"), "a dev build must not embed: {main}");
        assert!(!main.contains("MemorySource"), "a dev build reads the disk: {main}");
        assert!(main.contains("rux_shell::run"), "{main}");
        // What `rux run --route` and `--preview` hand the build it starts.
        assert!(main.contains(&format!("std::env::var({ROUTE_VAR:?})")), "{main}");
        assert!(main.contains(&format!("std::env::var({PREVIEW_VAR:?})")), "{main}");
        assert!(main.contains("route, preview,"), "{main}");
    }

    #[test]
    fn a_dev_wrapper_installs_the_whole_icon_set() {
        // MS-014: a dev build never called `set_icons`, so every `<icon>` was
        // blank. It embeds all of them, since a hot reload can name any.
        let options = Options { release: false, target: Target::Desktop, rux_source: None, abis: None, before: true, locked: false };
        let main = wrapper_main(&manifest(), &[PathBuf::from("app.rux")], &options);
        assert!(main.contains("rux_runtime::set_icons"), "{main}");
        assert!(main.contains(&format!("[[usize; 6]; {}]", rux_icons::count())), "{}", &main[..400.min(main.len())]);
    }

    #[test]
    fn a_desktop_wrapper_opens_the_window_the_manifest_asks_for() {
        let mut m = manifest();
        m.window.width = Some(1280.0);
        m.window.maximized = true;
        for release in [false, true] {
            let options = Options { release, target: Target::Desktop, rux_source: None, abis: None, before: true, locked: false };
            let main = wrapper_main(&m, &[PathBuf::from("app.rux")], &options);
            assert!(main.contains("rux_shell::run_windowed("), "{main}");
            assert!(main.contains(&format!("title: Some({}.into())", literal(&m.name))), "{main}");
            assert!(main.contains("width: Some(1280.0), height: None"), "{main}");
            assert!(main.contains("maximized: true"), "{main}");
        }
    }

    #[test]
    fn windows_paths_survive_being_written_into_rust_source() {
        assert_eq!(literal(r"C:\a\b.rux"), "\"C:\\\\a\\\\b.rux\"");
        assert_eq!(literal("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rux-locks-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Each target keeps its own lock, since the wrappers have different
    /// dependencies, and a lock round-trips through the generated crate.
    #[test]
    fn a_lock_is_kept_per_target_and_put_back_before_a_build() {
        let root = scratch("roundtrip");
        assert_eq!(kept_lock(&root, Target::Desktop), root.join("locks").join("desktop.lock"));
        assert_ne!(kept_lock(&root, Target::Desktop), kept_lock(&root, Target::Web));

        let kept = kept_lock(&root, Target::Desktop);
        let generated = root.join("Cargo.lock");
        assert!(!install_lock(&kept, &generated).unwrap(), "no kept lock yet, so nothing to install");

        std::fs::write(&generated, "version = 3\n").unwrap();
        assert!(keep_lock(&generated, &kept).unwrap(), "the first lock is written");
        assert!(!keep_lock(&generated, &kept).unwrap(), "an unchanged lock says nothing");

        // A fresh clone: the generated crate has no lock, the project does.
        std::fs::remove_file(&generated).unwrap();
        assert!(install_lock(&kept, &generated).unwrap());
        assert_eq!(std::fs::read_to_string(&generated).unwrap(), "version = 3\n");

        std::fs::write(&generated, "version = 4\n").unwrap();
        assert!(keep_lock(&generated, &kept).unwrap(), "a changed lock is rewritten");
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), "version = 4\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The kept locks describe how the app is built, so a release does not
    /// carry them.
    #[test]
    fn the_kept_locks_are_not_part_of_the_app() {
        let root = scratch("walk");
        std::fs::write(root.join("app.rux"), "<template><view/></template>").unwrap();
        std::fs::create_dir_all(root.join("locks")).unwrap();
        std::fs::write(root.join("locks").join("desktop.lock"), "version = 3\n").unwrap();
        let files = collect(&root).unwrap();
        assert_eq!(files, vec![PathBuf::from("app.rux")], "{files:?}");
        let _ = std::fs::remove_dir_all(&root);
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
            before: true,
            locked: false,
        };
        let toml = wrapper_manifest(&manifest(), &options, None, None);
        assert!(toml.contains("C:/rux/crates/rux-shell"), "{toml}");
        assert!(toml.contains("task-list"), "{toml}");
        // The checkout's vendored taffy, which its own `[patch]` cannot
        // reach from a crate outside its workspace.
        assert!(toml.contains("[patch.crates-io]\ntaffy = { path = \"C:/rux/vendor/taffy\" }"), "{toml}");
        assert!(toml.contains("vello = { path = \"C:/rux/vendor/vello\" }"), "{toml}");
    }

    /// A debug build compiles Rux optimised and the app's own native crate
    /// as written, for a debugger.
    #[test]
    fn a_debug_build_optimises_rux_and_not_the_apps_own_rust() {
        let options = Options { release: false, target: Target::Desktop, rux_source: None, abis: None, before: true, locked: false };
        let native = crate::native::NativeCrate {
            dir: PathBuf::from("C:/app/native"),
            lib: PathBuf::from("C:/app/native/src/lib.rs"),
            package: "tracer".to_string(),
            ident: "tracer".to_string(),
        };
        let text = wrapper_manifest(&manifest(), &options, None, Some(&native));
        let parsed: toml::Table = text.parse().unwrap_or_else(|e| panic!("{e}\n{text}"));
        let packages = &parsed["profile"]["dev"]["package"];
        assert_eq!(packages["*"]["opt-level"].as_integer(), Some(2), "{text}");
        assert_eq!(packages["tracer"]["opt-level"].as_integer(), Some(0), "{text}");
    }
}
