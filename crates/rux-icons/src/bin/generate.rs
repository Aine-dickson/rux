//! Turn a checkout of Tabler's icons into the one data file `rux-icons` serves.
//!
//! **Nobody but a Rux maintainer runs this, and then only when Tabler is
//! bumped.** Its output is committed, so an author never sees Tabler, never
//! downloads 6,000 SVG files, and never runs a build step to get an icon.
//!
//! ```text
//! cargo run -p rux-icons --bin generate -- <path to tabler-icons checkout>
//! ```
//!
//! # Why the output looks the way it does
//!
//! **One string and one integer table, not six thousand literals.** Everything
//! goes into a single `&str`, and the index is a flat array of `[u32; 6]`.
//!
//! The reason is **structural, not compile time**, and the difference was
//! measured rather than assumed: the literal form compiles in 1.6 seconds and
//! this one in 0.6, which is two and a half times and nobody would notice
//! either. What matters is that a subset of a blob is a **slice**, where a
//! subset of a literal table is a filter that has to be rebuilt. `rux build`
//! will embed only the icons an app mentions, and this is the shape that makes
//! that nearly free.
//!
//! **Paint is per path, not per icon**, and that is the thing a first design
//! gets wrong. An outline icon is stroked and a filled icon is filled, but 77
//! outline icons carry a path with `fill="currentColor"` (the dot on the head
//! in `accessible`), 13 carry `stroke="none"` (the slice in `percentage-25`),
//! and one carries `opacity=".5"`. Drop those and the dot becomes a ring and
//! the slice becomes an outline. So each path is prefixed with one hex digit
//! carrying its overrides, which costs one byte and keeps the index fixed-size.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// `fill="currentColor"` on this path: draw it solid, whatever the variant does.
const FILL_CURRENT: u8 = 1;
/// `stroke="none"` on this path: no outline, whatever the variant does.
const NO_STROKE: u8 = 2;
/// `opacity=".5"`. One icon in the whole set, and it would be invisible to lose.
const HALF_OPACITY: u8 = 4;

/// The separator between paths of one variant inside the blob.
///
/// A newline, because SVG path data cannot contain one: Tabler writes each `d`
/// on a single line, and a command is a letter followed by numbers.
const SEP: char = '\n';

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        eprintln!("usage: generate <path to tabler-icons checkout>");
        eprintln!();
        eprintln!("Clone one first:");
        eprintln!("  git clone --depth 1 --branch v3.47.0 https://github.com/tabler/tabler-icons");
        std::process::exit(2);
    };
    let root = PathBuf::from(root);

    let version = match read_version(&root) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("generate: {e}");
            std::process::exit(1);
        }
    };

    let outline = match collect(&root.join("icons").join("outline")) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("generate: {e}");
            std::process::exit(1);
        }
    };
    let filled = match collect(&root.join("icons").join("filled")) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("generate: {e}");
            std::process::exit(1);
        }
    };

    // **Filled is a strict subset, and this is where that is enforced rather
    // than believed.** A filled icon with no outline counterpart would be
    // unreachable through an element whose default variant is outline, and it
    // would mean the set's shape had changed under a design that rests on it.
    let orphans: Vec<&String> = filled.keys().filter(|n| !outline.contains_key(*n)).collect();
    if !orphans.is_empty() {
        eprintln!(
            "generate: {} filled icons have no outline counterpart, which the \
             design says cannot happen: {}",
            orphans.len(),
            orphans.iter().take(5).map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        );
        std::process::exit(1);
    }

    let (blob, index) = build(&outline, &filled);
    let out = render(&version, &blob, &index);

    let dest = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("generated.rs");
    if let Err(e) = std::fs::write(&dest, out) {
        eprintln!("generate: writing {}: {e}", dest.display());
        std::process::exit(1);
    }

    println!("rux-icons: Tabler {version}");
    println!("  {} outline, {} filled", outline.len(), filled.len());
    println!("  {} icons, {} bytes of data", index.len(), blob.len());
    println!("  wrote {}", dest.display());
}

/// Tabler's version, read from its `package.json` rather than passed in.
///
/// Recorded in the generated file so that "which Tabler is this?" is answered
/// by the artifact instead of by a memory of running the command.
fn read_version(root: &Path) -> Result<String, String> {
    let path = root.join("package.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("reading {}: {e}", path.display()))?;
    let key = "\"version\"";
    let at = text.find(key).ok_or_else(|| format!("no version in {}", path.display()))?;
    let rest = &text[at + key.len()..];
    let open = rest.find('"').ok_or("malformed version")?;
    let close = rest[open + 1..].find('"').ok_or("malformed version")?;
    Ok(rest[open + 1..open + 1 + close].to_string())
}

/// Every icon in one directory, as name to its list of (flags, path data).
fn collect(dir: &Path) -> Result<BTreeMap<String, Vec<(u8, String)>>, String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
    let mut out = BTreeMap::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("reading {}: {e}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("svg") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("odd file name: {}", path.display()))?
            .to_string();
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        let paths = parse_paths(&text, &path)?;
        if paths.is_empty() {
            return Err(format!("{} has no <path>", path.display()));
        }
        out.insert(name, paths);
    }
    if out.is_empty() {
        return Err(format!("{} held no icons; is this a tabler-icons checkout?", dir.display()));
    }
    Ok(out)
}

/// Pull every `<path>` out of one SVG, with the paint overrides it carries.
///
/// **Hand-rolled rather than an XML crate**, because these files are machine
/// generated, uniform, and checked: the whole set is `<path>` elements and
/// nothing else, no groups, no masks, no `<circle>`. Anything this does not
/// recognise is an error rather than a shrug, so the day Tabler changes shape
/// the generator says so instead of quietly emitting less.
fn parse_paths(svg: &str, file: &Path) -> Result<Vec<(u8, String)>, String> {
    // The elements that would mean this file is not the flat list of paths the
    // whole design assumes. Checked per file so a future Tabler cannot slip a
    // group past us.
    for tag in ["<circle", "<rect", "<line", "<polyline", "<polygon", "<ellipse", "<g ", "<g>",
                "<use", "<defs", "<mask", "<clipPath"] {
        if svg.contains(tag) {
            return Err(format!(
                "{} contains `{tag}`, which Rux cannot draw as path data.\n\n\
                 The icon set was chosen because it is path-only. If Tabler has \
                 changed, that decision needs revisiting, not this parser.",
                file.display()
            ));
        }
    }

    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(at) = rest.find("<path") {
        let after = &rest[at + "<path".len()..];
        let close = after
            .find('>')
            .ok_or_else(|| format!("{}: unterminated <path", file.display()))?;
        let attrs = &after[..close];

        let d = attr(attrs, "d").ok_or_else(|| format!("{}: a <path> with no d", file.display()))?;

        let mut flags = 0u8;
        match attr(attrs, "fill").as_deref() {
            None => {}
            Some("currentColor") => flags |= FILL_CURRENT,
            Some("none") => {}
            Some(other) => {
                return Err(format!("{}: unhandled fill={other:?}", file.display()));
            }
        }
        match attr(attrs, "stroke").as_deref() {
            None => {}
            Some("none") => flags |= NO_STROKE,
            Some(other) => {
                return Err(format!("{}: unhandled stroke={other:?}", file.display()));
            }
        }
        match attr(attrs, "opacity").as_deref() {
            None => {}
            Some(".5") | Some("0.5") => flags |= HALF_OPACITY,
            Some(other) => {
                return Err(format!("{}: unhandled opacity={other:?}", file.display()));
            }
        }

        // A newline in path data would break the separator the blob relies on.
        // Tabler writes one line per `d`; this is the check that says so.
        if d.contains('\n') || d.contains('\r') {
            return Err(format!("{}: path data spans lines", file.display()));
        }

        out.push((flags, d));
        rest = &after[close..];
    }
    Ok(out)
}

/// One attribute out of an element's attribute text, if it is there.
fn attr(attrs: &str, name: &str) -> Option<String> {
    let mut rest = attrs;
    loop {
        let at = rest.find(name)?;
        let before_ok = at == 0
            || rest[..at].ends_with(|c: char| c.is_whitespace())
            || rest[..at].ends_with('"');
        let after = &rest[at + name.len()..];
        let after_trimmed = after.trim_start();
        if before_ok && after_trimmed.starts_with('=') {
            let q = after_trimmed[1..].trim_start();
            let q = q.strip_prefix('"')?;
            let end = q.find('"')?;
            return Some(q[..end].to_string());
        }
        rest = &rest[at + name.len()..];
    }
}

/// One entry of the generated index.
struct Entry {
    name: String,
    name_at: u32,
    name_len: u32,
    outline_at: u32,
    outline_len: u32,
    filled_at: u32,
    filled_len: u32,
}

/// Pack everything into one blob and the index that points into it.
fn build(
    outline: &BTreeMap<String, Vec<(u8, String)>>,
    filled: &BTreeMap<String, Vec<(u8, String)>>,
) -> (String, Vec<Entry>) {
    let mut blob = String::new();
    let mut index = Vec::with_capacity(outline.len());

    // `BTreeMap` iterates in sorted order, which is what the binary search at
    // the other end needs. Recorded here rather than sorted again, so the two
    // cannot disagree.
    for (name, paths) in outline {
        let name_at = blob.len() as u32;
        blob.push_str(name);
        let name_len = name.len() as u32;

        let outline_at = blob.len() as u32;
        write_paths(&mut blob, paths);
        let outline_len = blob.len() as u32 - outline_at;

        let (filled_at, filled_len) = match filled.get(name) {
            None => (0, 0),
            Some(paths) => {
                let at = blob.len() as u32;
                write_paths(&mut blob, paths);
                (at, blob.len() as u32 - at)
            }
        };

        index.push(Entry {
            name: name.clone(),
            name_at,
            name_len,
            outline_at,
            outline_len,
            filled_at,
            filled_len,
        });
    }
    (blob, index)
}

/// One variant's paths, each as a hex flag digit then its data, newline joined.
fn write_paths(blob: &mut String, paths: &[(u8, String)]) {
    for (i, (flags, d)) in paths.iter().enumerate() {
        if i > 0 {
            blob.push(SEP);
        }
        // A hex digit, so the marker is one byte and can never be mistaken for
        // path data: every SVG command is a letter.
        blob.push(char::from_digit(u32::from(*flags), 16).expect("flags fit in a hex digit"));
        blob.push_str(d);
    }
}

/// The generated Rust source.
fn render(version: &str, blob: &str, index: &[Entry]) -> String {
    let mut out = String::with_capacity(blob.len() + index.len() * 40 + 4096);
    out.push_str(
        "//! Tabler icon data. GENERATED, do not edit.\n\
         //!\n\
         //! Written by `cargo run -p rux-icons --bin generate -- <tabler checkout>`.\n\
         //! See that file for the format and why it is this one.\n\
         //!\n\
         //! Tabler Icons is MIT licensed, Copyright (c) 2020-2026 Paweł Kuna.\n\
         //! The notice travels with the data: see LICENSE-TABLER beside this crate.\n\n",
    );
    let _ = writeln!(out, "/// The Tabler release this data was generated from.");
    let _ = writeln!(out, "pub const TABLER_VERSION: &str = {version:?};\n");

    let _ = writeln!(out, "/// Every name and every path, concatenated. See `generate.rs`.");
    out.push_str("pub(crate) const BLOB: &str = \"");
    for ch in blob.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push_str("\";\n\n");

    let _ = writeln!(
        out,
        "/// Per icon: name start and length, then each variant's start and length\n\
         /// in [`BLOB`]. A length of 0 means the icon has no filled artwork.\n\
         /// Sorted by name, which is what the lookup binary-searches."
    );
    let _ = writeln!(out, "pub(crate) const INDEX: [[u32; 6]; {}] = [", index.len());
    for e in index {
        let _ = writeln!(
            out,
            "    [{}, {}, {}, {}, {}, {}], // {}",
            e.name_at, e.name_len, e.outline_at, e.outline_len, e.filled_at, e.filled_len, e.name
        );
    }
    out.push_str("];\n");
    out
}
