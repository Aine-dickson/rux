//! `rux fmt`: re-indent `.rux` files, and format the CSS inside them.
//!
//! This exists as much to *delete* code as to add it. The re-indenter was
//! written twice, once in `rux-fmt` and once in the VS Code extension's
//! JavaScript, and the two drifted within a week: the JS copy inherited HTML's
//! void-tag list, which has `img` but not Rux's `<image>`, so an `<image src>`
//! written without a self-closing slash over-indented everything after it. One
//! implementation behind a command both the editor and CI can call is the fix.
//!
//! What it does is deliberately narrow, and the same split `rux-fmt` documents:
//! `<template>` and `<script>` are only re-indented, while `<style>` is properly
//! formatted. Rearranging someone's rhai is not a formatter's business.

use std::io::Read;
use std::path::PathBuf;

/// One indent level.
#[derive(Clone)]
pub struct Indent(String);

impl Indent {
    /// `"2"` for two spaces, `"tab"` for a tab. Matches what an editor knows
    /// about itself: VS Code hands a formatter `tabSize` and `insertSpaces`.
    pub fn parse(spec: &str) -> Result<Self, String> {
        if spec.eq_ignore_ascii_case("tab") {
            return Ok(Self("\t".into()));
        }
        match spec.parse::<usize>() {
            Ok(n) if (1..=16).contains(&n) => Ok(Self(" ".repeat(n))),
            _ => Err(format!("bad indent `{spec}` (expected 1 to 16, or `tab`)")),
        }
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for Indent {
    /// Two spaces, which is what the examples, the guide and the playground's
    /// Format button all already use.
    fn default() -> Self {
        Self("  ".into())
    }
}

pub struct Options {
    pub paths: Vec<PathBuf>,
    pub indent: Indent,
    /// Report which files are not formatted and change nothing. Exits non-zero
    /// if any would change, which is the form CI wants.
    pub check: bool,
    /// Write the result to stdout instead of back to the file.
    pub to_stdout: bool,
    /// Read the document from stdin and write it to stdout. What an editor
    /// shells out to, since the buffer it wants formatted is usually unsaved.
    pub stdin: bool,
}

/// Format as asked. Returns the process exit code.
pub fn run(options: Options) -> i32 {
    if options.stdin {
        return format_stdin(&options);
    }

    let files = match crate::files::collect(&options.paths, crate::files::Components::Include) {
        Ok(files) => files,
        Err(err) => {
            eprintln!("rux: {err}");
            return 2;
        }
    };
    if files.is_empty() {
        eprintln!("rux: no .rux files found");
        return 2;
    }
    if options.to_stdout && files.len() > 1 {
        eprintln!("rux: --stdout takes a single file, got {}", files.len());
        return 2;
    }

    let mut changed = Vec::new();
    let mut failed = false;
    for file in &files {
        let source = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("rux: reading {}: {e}", file.display());
                failed = true;
                continue;
            }
        };
        let formatted = rux_fmt::reindent(&source, options.indent.as_str());

        if options.to_stdout {
            print!("{formatted}");
            continue;
        }
        if formatted == source {
            continue;
        }
        changed.push(file.clone());
        if options.check {
            continue;
        }
        if let Err(e) = std::fs::write(file, &formatted) {
            eprintln!("rux: writing {}: {e}", file.display());
            failed = true;
        }
    }

    if failed {
        return 2;
    }
    if options.to_stdout {
        return 0;
    }

    report(&changed, files.len(), options.check);
    if options.check && !changed.is_empty() {
        1
    } else {
        0
    }
}

fn report(changed: &[PathBuf], total: usize, check: bool) {
    let files = if total == 1 { "file" } else { "files" };
    if changed.is_empty() {
        eprintln!("rux: {total} {files} already formatted");
        return;
    }
    for path in changed {
        // In check mode this is the actionable output, so it goes to stdout
        // where it can be piped; the counts stay on stderr either way.
        if check {
            println!("{}", path.display());
        }
    }
    let n = changed.len();
    if check {
        eprintln!("rux: {n} of {total} {files} would be reformatted");
    } else {
        eprintln!("rux: reformatted {n} of {total} {files}");
    }
}

/// The editor path: a buffer in, the formatted buffer out, nothing touched on
/// disk. Failing here must leave stdout empty, or an editor will replace the
/// document with a half-written one.
fn format_stdin(options: &Options) -> i32 {
    let mut source = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut source) {
        eprintln!("rux: reading stdin: {e}");
        return 2;
    }
    let formatted = rux_fmt::reindent(&source, options.indent.as_str());
    if options.check && formatted != source {
        return 1;
    }
    print!("{formatted}");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indent_accepts_spaces_and_tabs() {
        assert_eq!(Indent::parse("4").unwrap().as_str(), "    ");
        assert_eq!(Indent::parse("tab").unwrap().as_str(), "\t");
        assert_eq!(Indent::parse("TAB").unwrap().as_str(), "\t");
        assert_eq!(Indent::default().as_str(), "  ");
    }

    /// A bad indent is a usage error rather than a silent fallback: quietly
    /// formatting to the wrong width would be worse than refusing.
    #[test]
    fn indent_rejects_nonsense() {
        assert!(Indent::parse("0").is_err());
        assert!(Indent::parse("-2").is_err());
        assert!(Indent::parse("999").is_err());
        assert!(Indent::parse("two").is_err());
    }
}
