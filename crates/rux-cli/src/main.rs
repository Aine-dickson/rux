//! `rux`: the Rux command-line entry point.
//!
//! ```text
//! rux                      run examples/battery.rux
//! rux app.rux              run a file
//! rux run app.rux          the same, said explicitly
//! rux app.rux --route /x   open it on a route, the way a deep link arrives
//! rux check [path...]      report what is wrong with a file or a tree
//! rux fmt [path...]        re-indent, and format the CSS inside
//! ```
//!
//! `rux app.rux` keeps working because it is what every doc, blog post and
//! README written so far tells people to type. A bare first argument that is not
//! a known subcommand is a path, which is unambiguous while no subcommand ends
//! in `.rux`.

mod check;
mod files;
mod fmt;

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
rux: the Rux runtime and tools

Usage:
  rux [app.rux]              Run a document (defaults to examples/battery.rux)
  rux run <app.rux>          Run a document
  rux check [path...]        Check documents and report problems
                             (defaults to the current directory)
  rux fmt [path...]          Re-indent files in place, and format their CSS
                             (defaults to the current directory)

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

/// What bare `rux` runs. Only present inside a checkout of this repo, which is
/// why a missing one is explained rather than left to panic.
const DEFAULT_APP: &str = "examples/battery.rux";

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
        Some("run") => match args.get(1).filter(|a| !a.starts_with('-')) {
            Some(path) => run(PathBuf::from(path), &args[2..]),
            None => {
                eprintln!("rux: `run` needs a file\n\n{USAGE}");
                ExitCode::from(2)
            }
        },
        // A bare path, the form everything written so far tells people to use.
        Some(path) if !path.starts_with('-') => run(PathBuf::from(path), &args[1..]),
        Some(flag) => {
            eprintln!("rux: unknown option `{flag}`\n\n{USAGE}");
            ExitCode::from(2)
        }
        None => run(PathBuf::from(DEFAULT_APP), &[]),
    }
}

fn run(path: PathBuf, args: &[String]) -> ExitCode {
    // Checked here rather than left to the runtime, which starts a file watcher
    // on the path and panics when there is nothing to watch.
    //
    // Bare `rux` defaults to `examples/battery.rux`, which exists only inside a
    // checkout of this repo. So for everyone who installed from crates.io, the
    // first command they typed produced a stack trace out of the watcher. A
    // missing file is an ordinary mistake and must not read like a bug in Rux.
    if !path.exists() {
        eprintln!("rux: `{}` does not exist\n", path.display());
        if path == PathBuf::from(DEFAULT_APP) {
            eprintln!(
                "That is the default, and it only exists inside a checkout of the Rux\n\
                 repository. Name a file to run:\n\n    rux app.rux\n"
            );
        }
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    if path.is_dir() {
        eprintln!("rux: `{}` is a directory, not a document", path.display());
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
