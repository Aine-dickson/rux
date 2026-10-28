//! `[dev] watch` and `[build] before`: other tools run by `rux` itself.
//!
//! The case that asked for it is Tailwind. Its CLI turns the class names in
//! `.rux` files into a stylesheet, and without this it was a second terminal
//! that had to be started, kept running and remembered, beside `rux run`. Vite
//! makes that one command by running Tailwind as a plugin; Rux has no plugin
//! host and no JavaScript to run one in, so it runs the tool instead.
//!
//! **Not Tailwind-specific, and that was the owner's call.** The manifest
//! names commands, and Rux knows nothing about what they do:
//!
//! ```toml
//! [dev]
//! watch = ["tailwindcss -i app.in.css -o app.css --watch"]
//!
//! [build]
//! before = ["tailwindcss -i app.in.css -o app.css --minify"]
//! ```
//!
//! - `before` runs to completion, in order, before `rux build` collects the
//!   project, and before `rux run` opens the window. The second is not
//!   decoration: a `<style src>` naming a file that is not there yet is a load
//!   failure, so on a fresh checkout the window would open on an error.
//! - `watch` starts with `rux run` (and `rux run --device`) and is stopped
//!   when it stops. Whatever it writes, the window's own watcher reloads.
//!
//! **The first word is a program, found before anything runs.** On `PATH`, or
//! a path relative to `rux.toml`. Resolved here rather than left to the shell,
//! because a shell's "not recognized" scrolls past behind a window that opened
//! anyway, and because `cmd` does not understand `./tool`. The rest of the
//! line goes to the platform's shell as written, so quoting is the shell's.
//!
//! The program is never fetched. Rux makes no network requests on an author's
//! behalf, and a tool it downloaded would be a version nobody chose.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Run each `[build] before` command to completion, in order, in `root`.
pub fn run_before(commands: &[String], root: &Path) -> Result<(), String> {
    for line in commands {
        let mut command = shell(line, root, "[build] `before`")?;
        eprintln!("rux: running `{line}`");
        let status = command
            .status()
            .map_err(|e| format!("[build] `before` could not start `{line}`: {e}"))?;
        if !status.success() {
            return Err(format!(
                "[build] `before` command `{line}` failed ({}), so nothing was built",
                exit(status.code())
            ));
        }
    }
    Ok(())
}

/// The `[dev] watch` commands, running. Dropping this stops them.
pub struct Watching {
    children: Vec<Arc<Mutex<Child>>>,
    stopping: Arc<AtomicBool>,
}

/// Start each `[dev] watch` command in `root`.
///
/// Every program is resolved before any starts, so a typo in the second does
/// not leave the first running behind an error.
pub fn start_watch(commands: &[String], root: &Path) -> Result<Watching, String> {
    let mut prepared = Vec::with_capacity(commands.len());
    for line in commands {
        prepared.push((line, shell(line, root, "[dev] `watch`")?));
    }
    let stopping = Arc::new(AtomicBool::new(false));
    let mut watching = Watching { children: Vec::new(), stopping: stopping.clone() };
    for (line, mut command) in prepared {
        eprintln!("rux: watching with `{line}`");
        let child = command
            .spawn()
            .map_err(|e| format!("[dev] `watch` could not start `{line}`: {e}"))?;
        let child = Arc::new(Mutex::new(child));
        watching.children.push(child.clone());
        // Said when it happens, because a watcher that died is a stylesheet
        // that silently stopped following the source, and the window has no
        // way to know.
        let line = line.clone();
        let stopping = stopping.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(300));
            if stopping.load(Ordering::Relaxed) {
                return;
            }
            let Ok(mut child) = child.lock() else { return };
            if let Ok(Some(status)) = child.try_wait() {
                if !stopping.load(Ordering::Relaxed) {
                    eprintln!(
                        "rux: [dev] `watch` command `{line}` stopped ({}); what it writes is no \
                         longer kept up to date",
                        exit(status.code())
                    );
                }
                return;
            }
        });
    }
    Ok(watching)
}

impl Drop for Watching {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        for child in &self.children {
            let Ok(mut child) = child.lock() else { continue };
            if matches!(child.try_wait(), Ok(Some(_))) {
                continue;
            }
            kill_tree(&mut child);
            let _ = child.wait();
        }
    }
}

/// The child is `cmd`, and the tool is *its* child, which `Child::kill` would
/// leave running with the console to itself. `taskkill /T` takes the tree.
#[cfg(windows)]
fn kill_tree(child: &mut Child) {
    let killed = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &child.id().to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !killed {
        let _ = child.kill();
    }
}

/// `sh -c "exec …"` makes the tool the child itself, so killing it is enough.
#[cfg(not(windows))]
fn kill_tree(child: &mut Child) {
    let _ = child.kill();
}

fn exit(code: Option<i32>) -> String {
    match code {
        Some(code) => format!("exit code {code}"),
        None => "ended by a signal".into(),
    }
}

/// `line` as a command for the platform's shell, working in `root`, with its
/// first word replaced by the program it names.
fn shell(line: &str, root: &Path, table: &str) -> Result<Command, String> {
    let (word, rest) = first_word(line)
        .ok_or_else(|| format!("{table} has an empty command"))?;
    let program = resolve(word, root).ok_or_else(|| not_found(word, table))?;
    let program = program.to_string_lossy();

    #[cfg(windows)]
    let command = {
        use std::os::windows::process::CommandExt;
        // `/S` with the whole line in one pair of quotes is the form `cmd`
        // promises to run verbatim; `raw_arg` keeps Rust from quoting it again.
        let mut command = Command::new("cmd");
        command.raw_arg(format!("/D /S /C \"\"{program}\"{rest}\""));
        command
    };
    #[cfg(not(windows))]
    let command = {
        let mut command = Command::new("sh");
        command.arg("-c").arg(format!("exec '{}'{rest}", program.replace('\'', "'\\''")));
        command
    };

    // stdin is the terminal's, as it would be in a terminal of their own.
    // Tailwind's `--watch` stops when its stdin closes (`--watch=always` is
    // the way out of that), so a null one ended it the moment it started.
    let mut command = command;
    command.current_dir(root);
    Ok(command)
}

/// The first word, unquoted, and the rest of the line after it, untouched.
fn first_word(line: &str) -> Option<(&str, &str)> {
    let line = line.trim_start();
    if line.is_empty() {
        return None;
    }
    for quote in ['"', '\''] {
        if let Some(inner) = line.strip_prefix(quote) {
            let end = inner.find(quote)?;
            return Some((&inner[..end], &inner[end + 1..]));
        }
    }
    let end = line.find(char::is_whitespace).unwrap_or(line.len());
    Some((&line[..end], &line[end..]))
}

/// A path when the word has a separator in it, relative to `root`; otherwise
/// a name looked up on `PATH`, with Windows' executable extensions tried.
fn resolve(word: &str, root: &Path) -> Option<PathBuf> {
    let candidates = |base: PathBuf| -> Vec<PathBuf> {
        let mut out = vec![base.clone()];
        if cfg!(windows) && base.extension().is_none() {
            let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
            for ext in exts.split(';').filter(|e| !e.is_empty()) {
                let mut name = base.clone().into_os_string();
                name.push(ext.to_ascii_lowercase());
                out.push(PathBuf::from(name));
            }
        }
        out
    };
    let found = |base: PathBuf| candidates(base).into_iter().find(|p| p.is_file());

    if word.contains('/') || word.contains('\\') {
        let path = Path::new(word);
        let path = if path.is_absolute() { path.to_path_buf() } else { root.join(path) };
        return found(path);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| found(dir.join(word)))
}

fn not_found(word: &str, table: &str) -> String {
    let mut message = if word.contains('/') || word.contains('\\') {
        format!(
            "{table} runs `{word}`, which is not there. A path is relative to {}",
            crate::manifest::MANIFEST
        )
    } else {
        format!(
            "{table} runs `{word}`, which is not on PATH.\n\nInstall it, or name it by its path \
             relative to {}, like `./{word}`",
            crate::manifest::MANIFEST
        )
    };
    // The one tool this was built for, so the one hint worth carrying.
    if word.rsplit(['/', '\\']).next().is_some_and(|name| name.starts_with("tailwindcss")) {
        message.push_str(
            ".\n\nTailwind's standalone CLI needs no Node: \
             https://github.com/tailwindlabs/tailwindcss/releases (keep it outside the project, \
             or a release build carries it)",
        );
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_word_comes_off_with_the_rest_untouched() {
        assert_eq!(first_word("tailwindcss -i a.css"), Some(("tailwindcss", " -i a.css")));
        assert_eq!(first_word("  tool"), Some(("tool", "")));
        assert_eq!(first_word("\"my tool\" --x \"y z\""), Some(("my tool", " --x \"y z\"")));
        assert_eq!(first_word("'a b' c"), Some(("a b", " c")));
        assert_eq!(first_word("   "), None);
        assert_eq!(first_word("\"unclosed"), None);
    }

    #[test]
    fn a_program_named_by_path_is_relative_to_the_manifest() {
        let dir = std::env::temp_dir().join(format!("rux_commands_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let name = if cfg!(windows) { "tool.exe" } else { "tool" };
        std::fs::write(dir.join("bin").join(name), b"").unwrap();
        // Written without the extension on Windows, as on every other system.
        assert_eq!(resolve("./bin/tool", &dir), Some(dir.join("./bin").join(name)));
        assert_eq!(resolve("./bin/nope", &dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_program_that_is_not_there_is_named_before_anything_runs() {
        let error = run_before(&["rux-no-such-tool-anywhere --x".into()], Path::new("."))
            .expect_err("nothing to run");
        assert!(error.contains("`rux-no-such-tool-anywhere`") && error.contains("PATH"), "{error}");
        let error = start_watch(&["tailwindcss-not-here".into()], Path::new("."))
            .err()
            .expect("nothing to watch with");
        assert!(error.contains("tailwindlabs"), "names where Tailwind comes from: {error}");
    }

    #[test]
    fn a_failing_command_stops_the_build_and_a_passing_one_does_not() {
        let (ok, fail) = if cfg!(windows) {
            ("cmd /C exit 0", "cmd /C exit 3")
        } else {
            ("sh -c 'exit 0'", "sh -c 'exit 3'")
        };
        let root = std::env::temp_dir();
        assert_eq!(run_before(&[ok.into()], &root), Ok(()));
        let error = run_before(&[ok.into(), fail.into()], &root).expect_err("exit 3");
        assert!(error.contains("exit code 3"), "{error}");
    }

    #[test]
    fn a_watcher_is_stopped_when_rux_stops() {
        let line = if cfg!(windows) { "ping -n 30 127.0.0.1" } else { "sleep 30" };
        let watching = start_watch(&[line.into()], &std::env::temp_dir()).unwrap();
        let child = watching.children[0].clone();
        let started = std::time::Instant::now();
        drop(watching);
        assert!(child.lock().unwrap().try_wait().unwrap().is_some(), "still running");
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
