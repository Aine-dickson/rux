//! `rux new <name>`: scaffold a project.
//!
//! This exists because the on-disk shape of a Rux project existed nowhere. Every
//! shipped example is a single file, so someone who had read all of `/learn`
//! still did not know where components go, or that `assets/` resolves relative
//! to the `.rux` file rather than the working directory. The scaffold is a
//! documentation deliverable as much as a convenience: it is the answer to
//! "what does a real one look like", in the only form that cannot go stale,
//! because `rux check` and `rux fmt --check` run over it in the tests.
//!
//! # What a workspace is
//!
//! **A directory containing `app.rux` or `index.rux`.** That is the whole
//! definition, and there is deliberately no manifest file yet.
//!
//! A `rux.toml` would have to carry a window title, an icon, a target and a
//! version, and every one of those is a decision `rux build` owns and has not
//! made. Inventing the manifest here would commit the build format by accident,
//! from the side of the tool least able to see the consequences. An entry file
//! is enough for `rux run` to find its way, costs nothing to keep if a manifest
//! arrives later, and if one does, `rux new` is where it gets written.
//!
//! See `entry` in `main.rs` for the lookup this pairs with.

use std::fs;
use std::path::{Path, PathBuf};

/// One scaffolded file: where it goes, and what goes in it.
struct File {
    path: &'static str,
    body: &'static str,
}

/// The entry point. Named `app.rux` because that is what every doc written so
/// far calls the file someone runs; `index.rux` is accepted by the lookup for
/// people arriving from the web, but not generated, because offering two names
/// for one thing is how a convention stops being one.
const APP: &str = r#"<template>
  <screen class="app">
    <text class="title">{{ title }}</text>

    <input class="field" r-model="draft" placeholder="add a task" />

    <button class="add" @tap="add()">
      <text class="add-label">Add</text>
    </button>

    <view class="list">
      <task r-for="t in tasks" r-key="t.id" :label="t.label" :done="t.done" />
    </view>

    <text class="empty" r-if="tasks.len() == 0">Nothing yet.</text>
  </screen>
</template>

<style>
  .app {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 24px;
    background: #1e1e2e;
  }

  .title { color: #cdd6f4; font-size: 24px; font-weight: 700; }

  .field {
    padding: 10px;
    border: 1px solid #45475a;
    border-radius: 8px;
    color: #cdd6f4;
    background: #313244;
  }

  .add {
    display: flex;
    justify-content: center;
    padding: 10px;
    border-radius: 8px;
    background: #89b4fa;
  }

  .add:hover { background: #a6c8ff; }
  .add-label { color: #11111b; font-weight: 700; }
  .list { display: flex; flex-direction: column; gap: 6px; }
  .empty { color: #6c7086; font-size: 14px; }
</style>

<script>
  use components::task;

  let title = signal("Tasks");
  let draft = signal("");
  let tasks = signal([]);
  let next_id = signal(1);

  fn add() {
    if draft.trim() == "" {
      return;
    }
    tasks.push(#{ id: next_id, label: draft.trim(), done: false });
    next_id += 1;
    draft = "";
  }
</script>
"#;

/// A component, because a project with none does not show where they live or
/// how a prop arrives. Props are read-only inside an instance, which is worth
/// meeting early rather than discovering.
const TASK: &str = r#"<!-- A component sees only its props (`label`, `done`). The caller's signals
     are not in scope here, and this file's CSS styles only this subtree. -->
<template>
  <view class="task" :class='#{ done: done }'>
    <view class="box" />
    <text class="label">{{ label }}</text>
  </view>
</template>

<style>
  .task {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px;
    border-radius: 6px;
    background: #313244;
  }

  .box {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    border: 2px #585b70 solid;
    border-radius: 4px;
    background: #1e1e2e;
  }

  .label { color: #cdd6f4; font-size: 15px; }
  /* `:class` above adds `done`, so the finished look lives here, in the file
     that owns how a task looks. */
  .task.done .box { background: #a6e3a1; border: 2px #a6e3a1 solid; }
  .task.done .label { color: #6c7086; text-decoration: line-through; }
</style>
"#;

const README: &str = r#"# {name}

A [Rux](https://ruxlang.dev) app.

## Run it

```bash
rux run
```

`rux run` looks for `app.rux` here and in every parent directory, so it works
from anywhere inside the project. Edits reload in the open window; nothing needs
rebuilding.

## What is here

```
app.rux            the entry point: <template>, <style>, <script>
components/        one file per component
  task.rux         imported by `use components::task;`
assets/            images; `src` resolves relative to the .rux file
```

`components/` and `assets/` are conventions the runtime already follows:
`use components::task;` names `components/task.rux`, and an
`<image src="assets/logo.png">` resolves from the document's own directory, not
from wherever you happened to run `rux`.

## While you work

```bash
rux check          what is wrong, without opening a window
rux fmt            re-indent, and format the CSS
```
"#;

const GITIGNORE: &str = "# Rux keeps no build output yet; `rux build` will land in a later release.\n\
                         # Editors and OS cruft, though, start on day one.\n\
                         .DS_Store\n\
                         Thumbs.db\n\
                         *.swp\n";

/// `assets/` has to exist to be a convention, and an empty directory does not
/// survive `git add`. A note is more honest than a placeholder image.
const ASSETS_NOTE: &str = "Images go here.\n\
    \n\
    An `<image src=\"assets/logo.png\">` resolves this path relative to the\n\
    `.rux` file that names it, not relative to the directory you ran `rux` in,\n\
    so it keeps working wherever the app is launched from.\n\
    \n\
    PNG, JPEG, GIF and WebP.\n";

const FILES: &[File] = &[
    File { path: "app.rux", body: APP },
    File { path: "components/task.rux", body: TASK },
    File { path: "assets/README.md", body: ASSETS_NOTE },
    File { path: ".gitignore", body: GITIGNORE },
];

/// Create the project. Returns a process exit code.
pub fn create(args: &[String]) -> i32 {
    let mut name = None;
    for arg in args {
        if arg.starts_with('-') {
            eprintln!("rux: unknown option `{arg}` for `new`");
            return 2;
        }
        if name.is_some() {
            eprintln!("rux: `new` takes one name, not several");
            return 2;
        }
        name = Some(arg.clone());
    }

    let Some(name) = name else {
        eprintln!("rux: `new` needs a name, like `rux new my-app`");
        return 2;
    };

    let target = PathBuf::from(&name);
    let display = target
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.clone());

    if let Err(why) = check_name(&display) {
        eprintln!("rux: {why}");
        return 2;
    }

    // Refusing a non-empty directory rather than merging into it: a scaffold
    // that silently overwrote an `app.rux` someone had been working in would be
    // unforgivable, and there is no way to ask here.
    if target.exists() {
        let occupied = fs::read_dir(&target).map(|mut d| d.next().is_some()).unwrap_or(true);
        if occupied {
            eprintln!("rux: `{}` already exists and is not empty", target.display());
            return 2;
        }
    }

    if let Err(e) = write_all(&target, &display) {
        eprintln!("rux: {e}");
        return 1;
    }

    println!("Created {}", target.display());
    println!();
    println!("  cd {name}");
    println!("  rux run");
    println!();
    println!("`rux run` finds app.rux from anywhere inside the project. Edits reload live.");
    0
}

fn write_all(target: &Path, name: &str) -> Result<(), String> {
    for file in FILES {
        let path = target.join(file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        fs::write(&path, file.body).map_err(|e| format!("writing {}: {e}", path.display()))?;
    }
    let readme = README.replace("{name}", name);
    let path = target.join("README.md");
    fs::write(&path, readme).map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(())
}

/// What makes a usable project name.
///
/// This is deliberately stricter than the filesystem: the name is also what a
/// person types after `cd`, and later, when `rux build` exists, a plausible
/// default for the binary and the window title. Letting a space or a quote in
/// here would be a papercut in every one of those places.
fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("a project name cannot be empty".into());
    }
    if name == "." || name == ".." {
        return Err(format!("`{name}` is not a project name"));
    }
    if name.starts_with('-') {
        return Err(format!("`{name}` starts with a dash, which reads as an option"));
    }
    if let Some(c) = name.chars().find(|c| {
        !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
    }) {
        return Err(format!(
            "`{name}` contains `{c}`; use letters, digits, `-`, `_` and `.`"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_would_bite_later_are_refused() {
        assert!(check_name("my-app").is_ok());
        assert!(check_name("my_app.v2").is_ok());
        assert!(check_name("").is_err());
        assert!(check_name(".").is_err());
        assert!(check_name("--force").is_err(), "reads as an option");
        assert!(check_name("my app").is_err(), "a space is a papercut in every shell");
        assert!(check_name("app/inner").is_err(), "a separator is not part of the name");
    }

    /// The scaffold's own import has to name the file the scaffold writes. This
    /// is the mistake the underscore-to-hyphen mapping invites: `use
    /// components::task;` names `components/task.rux`, and a component file
    /// named anything else renders nothing, silently.
    #[test]
    fn the_scaffolds_import_names_the_file_it_writes() {
        assert!(APP.contains("use components::task;"));
        assert!(FILES.iter().any(|f| f.path == "components/task.rux"));
        assert!(APP.contains("<task "), "the tag must match the import");
    }
}
