//! `rux`: the Rux command-line entry point.
//!
//! ```text
//! rux                      print the usage
//! rux new my-app           create a project
//! rux run                  run the workspace's app.rux or index.rux
//! rux run app.rux          run a named file
//! rux app.rux              the same, said shorter
//! rux app.rux --route /x   open it on a route, the way a deep link arrives
//! rux check [path...]      report what is wrong with a file or a tree
//! rux fmt [path...]        re-indent, and format the CSS inside
//! rux vocab                print the runtime's vocabulary as JSON, for editors
//! ```
//!
//! `rux app.rux` keeps working because it is what every doc, blog post and
//! README written so far tells people to type. A bare first argument that is not
//! a known subcommand is a path, which is unambiguous while no subcommand ends
//! in `.rux`.
//!
//! **`rux run` is the way to run a workspace, and bare `rux` prints the usage.**
//! A tool that launches a GUI when invoked with no arguments is a surprise; one
//! that explains itself is what `cargo`, `git` and `npm` all do. The old bare
//! form defaulted to `examples/battery.rux`, a path that exists only in a
//! checkout of this repo, so it panicked for everyone else.

mod check;
mod files;
mod fmt;
mod new;
mod vocab;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
rux: the Rux runtime and tools

Usage:
  rux new <name>             Create a project: app.rux, components/, assets/
  rux run                    Run the workspace: its app.rux or index.rux,
                             looked for here and in every parent directory
  rux run <app.rux>          Run a named document
  rux <app.rux>              The same, for a file you can name
  rux check [path...]        Check documents and report problems
                             (defaults to the current directory)
  rux fmt [path...]          Re-indent files in place, and format their CSS
                             (defaults to the current directory)
  rux vocab                  Print what the runtime understands (elements,
                             directives, honored CSS) as JSON, for an editor

Run options:
  --route <path>             Open on this route instead of `/`, the way a
                             deep link arrives

Check options:
  --format json              Emit diagnostics as JSON, for an editor
  --deny-warnings            Exit non-zero on warnings as well as errors

Format options:
  --indent <n|tab>           One indent level: spaces, or a tab (default 2)
  --check                    Change nothing; exit non-zero if a file would
  --stdout                   Write the result out instead of back to the file
  -                          Read the document from stdin, write it to stdout

Other:
  -h, --help                 Show this
  -V, --version              Show the version

Exit codes: 0 clean, 1 problems found, 2 the request itself was wrong.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("-V" | "--version" | "version") => {
            println!("rux {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("check") => ExitCode::from(check(&args[1..]) as u8),
        Some("fmt") => ExitCode::from(format(&args[1..]) as u8),
        Some("new") => ExitCode::from(new::create(&args[1..]) as u8),
        Some("vocab") => ExitCode::from(vocab::emit() as u8),
        Some("run") => match args.get(1).filter(|a| !a.starts_with('-')) {
            Some(path) => run(PathBuf::from(path), &args[2..]),
            // `run` with no file means the same as bare `rux`: the workspace's
            // own entry point. It used to be a usage error, which was correct
            // when there was no such thing as a workspace.
            None => match entry(&std::env::current_dir().unwrap_or_default()) {
                Some(path) => run(path, &args[1..]),
                None => {
                    eprintln!("{}", no_entry_message());
                    ExitCode::from(2)
                }
            },
        },
        // A bare path, the form everything written so far tells people to use.
        Some(path) if !path.starts_with('-') => run(PathBuf::from(path), &args[1..]),
        Some(flag) => {
            eprintln!("rux: unknown option `{flag}`\n\n{USAGE}");
            ExitCode::from(2)
        }
        // Bare `rux` says what `rux` is. Running a workspace is `rux run`,
        // which is worth typing: it is the command that appears in every
        // README and every terminal recording, and a bare tool name that
        // launches a GUI is a surprise, while a bare tool name that explains
        // itself is what `cargo`, `git` and `npm` all do.
        //
        // This used to default to `examples/battery.rux`, which exists only in
        // a checkout of this repo, so the first thing a new user typed after
        // `cargo install ruxlang` was a panic out of the file watcher.
        None => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
    }
}

/// The entry files a workspace can have, in the order they are looked for.
///
/// `app.rux` is what `rux new` writes and what every doc calls the file you
/// run. `index.rux` is accepted because the web habit is strong and someone
/// will reach for it; it is not *generated*, because offering two names for one
/// thing is how a convention stops being one.
const ENTRIES: &[&str] = &["app.rux", "index.rux"];

/// Find the workspace entry point, starting at `from` and walking up.
///
/// Walking up is the point: `rux` should work from `components/` the way `git`
/// works from a subdirectory, because being told to `cd` back to the root is a
/// papercut in a tool people run dozens of times an hour.
///
/// A directory holding both names is not an error. `app.rux` wins, silently,
/// because the alternative is refusing to start over a question the author does
/// not care about at the moment they asked to run something.
fn entry(from: &Path) -> Option<PathBuf> {
    let mut dir = Some(from);
    while let Some(current) = dir {
        for name in ENTRIES {
            let candidate = current.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        dir = current.parent();
    }
    None
}

/// Said when there is nothing to run.
///
/// The whole usage follows, because bare `rux` is what someone types the first
/// minute after `cargo install ruxlang`, and until this existed what they got
/// was a panic out of the file watcher: the default path pointed at
/// `examples/battery.rux`, which exists only in a checkout of this repo. A
/// stack trace is not an answer to "what does this program do", and that is the
/// question actually being asked. So: what went wrong, what to do about it,
/// then the commands.
fn no_entry_message() -> String {
    format!(
        "rux: no {} found here or in any parent directory\n\n\
         Start a project:          rux new my-app\n\
         Or run a file directly:   rux path/to/app.rux\n\n\
         {USAGE}",
        ENTRIES.join(" or ")
    )
}

fn run(path: PathBuf, args: &[String]) -> ExitCode {
    // Checked here rather than left to the runtime, which starts a file watcher
    // on the path and panics when there is nothing to watch. A missing file is
    // an ordinary mistake, not a bug in Rux, and it should not read like one.
    if !path.exists() {
        eprintln!("rux: `{}` does not exist", path.display());
        if path.extension().is_none() {
            eprintln!("\nDid you mean a subcommand? `rux --help` lists them.");
        }
        return ExitCode::from(2);
    }
    if path.is_dir() {
        eprintln!(
            "rux: `{}` is a directory\n\nRun a file, or `cd` into a project and run `rux`.",
            path.display()
        );
        return ExitCode::from(2);
    }

    let mut route = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--route" => match rest.next() {
                Some(value) => route = Some(value.clone()),
                None => {
                    eprintln!("rux: `--route` needs a path, like `--route /settings`");
                    return ExitCode::from(2);
                }
            },
            flag => {
                eprintln!("rux: unknown option `{flag}`\n\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    rux_shell::run_at(path, route);
    ExitCode::SUCCESS
}

fn check(args: &[String]) -> i32 {
    let mut options = check::Options { paths: Vec::new(), json: false, deny_warnings: false };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--deny-warnings" => options.deny_warnings = true,
            "--format" => match rest.next().map(String::as_str) {
                Some("json") => options.json = true,
                Some("text") => options.json = false,
                Some(other) => {
                    eprintln!("rux: unknown format `{other}` (expected `text` or `json`)");
                    return 2;
                }
                None => {
                    eprintln!("rux: `--format` needs a value (`text` or `json`)");
                    return 2;
                }
            },
            "-h" | "--help" => {
                print!("{USAGE}");
                return 0;
            }
            flag if flag.starts_with('-') => {
                eprintln!("rux: unknown option `{flag}`\n\n{USAGE}");
                return 2;
            }
            path => options.paths.push(PathBuf::from(path)),
        }
    }
    check::run(options)
}

fn format(args: &[String]) -> i32 {
    let mut options = fmt::Options {
        paths: Vec::new(),
        indent: fmt::Indent::default(),
        check: false,
        to_stdout: false,
        stdin: false,
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--check" => options.check = true,
            "--stdout" => options.to_stdout = true,
            // The conventional spelling for "the document is on stdin".
            "-" => options.stdin = true,
            "--indent" => match rest.next() {
                Some(spec) => match fmt::Indent::parse(spec) {
                    Ok(indent) => options.indent = indent,
                    Err(e) => {
                        eprintln!("rux: {e}");
                        return 2;
                    }
                },
                None => {
                    eprintln!("rux: `--indent` needs a value (1 to 16, or `tab`)");
                    return 2;
                }
            },
            "-h" | "--help" => {
                print!("{USAGE}");
                return 0;
            }
            flag if flag.starts_with('-') => {
                eprintln!("rux: unknown option `{flag}`\n\n{USAGE}");
                return 2;
            }
            path => options.paths.push(PathBuf::from(path)),
        }
    }
    if options.stdin && !options.paths.is_empty() {
        eprintln!("rux: `-` reads one document from stdin, so it takes no paths");
        return 2;
    }
    fmt::run(options)
}
