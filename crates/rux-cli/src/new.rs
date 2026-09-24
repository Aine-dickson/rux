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
//! **A directory containing `app.rux` or `index.rux`.** That is still all
//! `rux run` and `rux check` need, and the lookup they use has not changed.
//!
//! **The scaffold also writes a `rux.toml`, as of v0.8.** It used to not, on
//! the grounds that a manifest would commit decisions `rux build` owned and had
//! not made. `rux build` has since made all of them, and the reasoning expired
//! without the code noticing: a scaffolded project could be run but not built,
//! and the missing file had to be written by hand before the first
//! `rux build --target android` would do anything. Scaffolding one is the whole
//! fix, and the keys it writes are the three a build cannot infer.
//!
//! See `entry` in `main.rs` for the lookup this pairs with, and
//! `crate::manifest` for what the file means.

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
///
/// It owns three things: the window's frame, the app's state, and the routes.
/// Everything a page changes, it changes by calling a function declared here.
const APP: &str = r#"<!-- The entry point. `rux run` looks for this file here and in every parent
     directory, so it works from anywhere inside the project.

     This file owns three things: the window's frame, the app's state, and the
     routes. Each page lives in `pages/`, and reads the state from here. -->
<template>
  <screen class="app">
    <view class="header">
      <text class="brand">Tasks</text>
      <text class="tally">{{ done_count() }} of {{ tasks.len() }} done</text>
    </view>

    <view class="stage">
      <router>
        <route path="/" view="home" name="home" />
        <route path="/new" view="new_task" name="new" />
        <route path="/task/:id" view="detail" name="task" />
        <route fallback view="missing" />
      </router>
    </view>

    <view class="tabs">
      <view class="tab" to="/" :class='#{ here: route == "/" }'>
        <text class="tab-label">List</text>
      </view>
      <view class="tab" to="/new" :class='#{ here: route == "/new" }'>
        <text class="tab-label">Add</text>
      </view>
    </view>
  </screen>
</template>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: #1e1e2e;
    font-family: sans-serif;
    /* The edges a phone will not let an app draw in: the status bar at the
       top, the gesture bar or the home indicator at the bottom. Both are zero
       on a desktop, so this changes nothing there and is the difference
       between a readable header and one under the clock on a device. Try it
       with `rux run --preview phone`. */
    padding-top: env(safe-area-inset-top);
    padding-bottom: env(safe-area-inset-bottom);
  }

  .header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    padding: 20px 20px 12px 20px;
  }

  .brand { color: #cdd6f4; font-size: 26px; font-weight: 700; }
  .tally { color: #6c7086; font-size: 13px; }

  /* The router's page sits here. `flex: 1` gives it the space between the
     header and the tabs, and `overflow-y: auto` is what makes a long list
     scroll: nothing scrolls in Rux unless it is told to. */
  .stage {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 0 20px;
  }

  .tabs {
    display: flex;
    gap: 8px;
    padding: 12px 20px 20px 20px;
    border-top: 1px #313244 solid;
  }

  .tab {
    flex: 1;
    display: flex;
    justify-content: center;
    padding: 12px;
    border-radius: 10px;
    background: #313244;
    transition: background 140ms ease-out;
  }

  .tab:hover { background: #45475a; }
  /* `:class` adds `here` on the tab whose route is the one showing. */
  .tab.here { background: #89b4fa; }
  .tab-label { color: #cdd6f4; font-size: 15px; font-weight: 700; }
  .tab.here .tab-label { color: #11111b; }
</style>

<script>
  use pages::home;
  use pages::new_task;
  use pages::detail;
  use pages::missing;

  /* The app's state. A page is a component and has its own state, but it can
     read these by name, and it changes them by calling the functions below:
     functions are shared with every file, and they run in this scope. */
  /* An id is text, not a number. A route carries `:id` out of the path as
     text, and Rux has no string-to-number conversion, so text on both sides is
     the only comparison that can work. */
  let tasks = signal([
    #{ id: "1", label: "Read the Rux guide", note: "ruxlang.dev/learn", done: true },
    #{ id: "2", label: "Add a task of your own", note: "The tab below, or the button on the list", done: false }
  ]);
  let next_id = signal(3);

  fn done_count() {
    let n = 0;
    for t in tasks {
      if t.done { n += 1; }
    }
    n
  }

  fn add_task(label, note) {
    tasks.push(#{ id: "" + next_id, label: label, note: note, done: false });
    next_id += 1;
  }

  fn task_by_id(id) {
    for t in tasks {
      if t.id == id { return t; }
    }
    ()
  }

  fn toggle_task(id) {
    for i in 0..tasks.len() {
      if tasks[i].id == id {
        tasks[i].done = !tasks[i].done;
      }
    }
  }

  fn remove_task(id) {
    let kept = [];
    for t in tasks {
      if t.id != id { kept.push(t); }
    }
    tasks = kept;
  }
</script>
"#;

/// The list. A page is an ordinary component that a `<route>` names, which is
/// why its root is a `<view>` and not a second `<screen>`.
const HOME: &str = r#"<!-- The list. A page is an ordinary component that a `<route>` names, so its
     root is a `<view>` and not a second `<screen>`. -->
<template>
  <view class="page">
    <view class="rows" r-if="tasks.len() > 0">
      <task-row
      r-for="t in tasks"
      r-key="t.id"
      :label="t.label"
      :note="t.note"
      :done="t.done"
      :id_of="t.id" />
    </view>

    <view class="blank" r-else>
      <text class="blank-title">Nothing here yet</text>
      <text class="blank-line">Add your first task and it shows up in this list.</text>
      <view class="blank-cta" to="/new">
        <text class="blank-cta-label">Add a task</text>
      </view>
    </view>
  </view>
</template>

<style>
  .page {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-bottom: 16px;
  }

  .rows {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .blank {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 48px 24px;
  }

  .blank-title { color: #cdd6f4; font-size: 18px; font-weight: 700; }
  .blank-line { color: #6c7086; font-size: 14px; text-align: center; }

  .blank-cta {
    margin-top: 8px;
    padding: 10px 18px;
    border-radius: 999px;
    background: #89b4fa;
  }

  .blank-cta:hover { background: #a6c8ff; }
  .blank-cta-label { color: #11111b; font-size: 14px; font-weight: 700; }
</style>

<script>
  use components::task_row;
</script>
"#;

/// The form, and the file that shows what an editable field needs: every
/// `<input>` carries an `r-model`, because that binding is what makes it
/// editable at all.
const NEW_TASK: &str = r#"<!-- The form. Every input needs an `r-model`: it is the binding that makes the
     field editable, and a field without one takes no typing. -->
<template>
  <view class="page">
    <text class="heading">New task</text>

    <view class="field">
      <text class="field-label">What needs doing</text>
      <input class="input" r-model="label" placeholder="Water the plants" />
    </view>

    <view class="field">
      <text class="field-label">Notes</text>
      <input class="input" r-model="note" placeholder="Optional" />
    </view>

    <text class="hint" r-if='label.trim() == ""'>A task needs a name before it can be added.</text>

    <view class="save" :class='#{ ready: label.trim() != "" }' @tap="save()">
      <text class="save-label">Add task</text>
    </view>
  </view>
</template>

<style>
  .page {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 8px 0 24px 0;
  }

  .heading { color: #cdd6f4; font-size: 20px; font-weight: 700; }
  .field { display: flex; flex-direction: column; gap: 6px; }
  .field-label { color: #6c7086; font-size: 12px; }

  .input {
    padding: 12px;
    border: 1px #45475a solid;
    border-radius: 10px;
    color: #cdd6f4;
    background: #313244;
    font-size: 15px;
  }

  .input:focus { border: 1px #89b4fa solid; }
  .hint { color: #6c7086; font-size: 12px; }

  /* Dim until there is something to save, and the handler checks the same
     thing: the look and the rule agree, and neither is load-bearing alone. */
  .save {
    display: flex;
    justify-content: center;
    padding: 14px;
    border-radius: 12px;
    background: #45475a;
    transition: background 140ms ease-out;
  }

  .save.ready { background: #89b4fa; }
  .save.ready:hover { background: #a6c8ff; }
  .save-label { color: #9399b2; font-size: 15px; font-weight: 700; }
  .save.ready .save-label { color: #11111b; }
</style>

<script>
  /* This page's own state. Two instances of a component would each get their
     own copy of these, which is why they live here and the task list does not. */
  let label = signal("");
  let note = signal("");

  fn save() {
    if label.trim() == "" {
      return;
    }
    /* `add_task` is declared in app.rux. Functions are shared across every file
       in the project, so a page changes the app's state by calling one. */
    add_task(label.trim(), note.trim());
    label = "";
    note = "";
    navigate("/");
  }
</script>
"#;

/// One task, opened from the list. The file that shows `params`, and that a
/// route parameter arrives as text.
const DETAIL: &str = r#"<!-- One task, opened from the list.

     `params` carries the `:id` out of the path, as text. A task's own id is
     text for that reason: it is what the path can carry back. -->
<template>
  <view class="page">
    <view class="back" @tap="back()">
      <text class="back-label">‹ All tasks</text>
    </view>

    <view class="card" r-if="task() != ()">
      <text class="label" :class='#{ done: task().done }'>{{ task().label }}</text>
      <text class="note" r-if='task().note != ""'>{{ task().note }}</text>

      <view class="actions">
        <view class="action done-action" @tap="toggle_task(id())">
          <text class="action-label" r-if="task().done">Mark as not done</text>
          <text class="action-label" r-else>Mark as done</text>
        </view>
        <view class="action delete" @tap="drop()">
          <text class="action-label delete-label">Delete</text>
        </view>
      </view>
    </view>

    <text class="gone" r-else>That task is no longer here.</text>
  </view>
</template>

<style>
  .page {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 8px 0 24px 0;
  }

  .back { align-self: flex-start; padding: 6px 2px; }
  .back-label { color: #89b4fa; font-size: 14px; }

  .card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 18px;
    border-radius: 14px;
    background: #313244;
  }

  .label { color: #cdd6f4; font-size: 20px; font-weight: 700; }
  .label.done { color: #6c7086; text-decoration: line-through; }
  .note { color: #9399b2; font-size: 14px; }

  .actions {
    width: 100%;
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }

  .action {
    flex: 1;
    display: flex;
    justify-content: center;
    padding: 12px;
    border-radius: 10px;
    background: #45475a;
    transition: background 140ms ease-out;
  }

  .action:hover { background: #585b70; }

  .action-label {
    color: #cdd6f4;
    font-size: 14px;
    font-weight: 700;
    white-space: nowrap;
  }

  .delete { background: #45303a; }
  .delete:hover { background: #5c3a46; }
  .delete-label { color: #f38ba8; }
  .gone { color: #6c7086; font-size: 14px; }
</style>

<script>
  fn id() {
    params?.id
  }

  fn task() {
    task_by_id(id())
  }

  fn drop() {
    remove_task(id());
    navigate("/");
  }
</script>
"#;

/// The fallback route. A project without one sends a mistyped path to a blank
/// screen, which reads as a crash.
const MISSING: &str = r#"<!-- The fallback route: shown for any path no other route matches. -->
<template>
  <view class="page">
    <text class="title">Nothing at that address</text>
    <view class="home" to="/">
      <text class="home-label">Back to the list</text>
    </view>
  </view>
</template>

<style>
  .page {
    width: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding: 48px 24px;
  }

  .title { color: #cdd6f4; font-size: 18px; font-weight: 700; }
  .home { padding: 10px 18px; border-radius: 999px; background: #313244; }
  .home:hover { background: #45475a; }
  .home-label { color: #cdd6f4; font-size: 14px; }
</style>
"#;

/// The one component, because a project without one does not show where they
/// live or how a prop arrives. Props are read-only inside an instance, which is
/// worth meeting early rather than discovering.
const TASK_ROW: &str = r#"<!-- One row of the list.

     A component declares its props at the bottom (`label`, `note`, `done`,
     `id_of`) and reads nothing else of its caller's, so this file can be
     dropped into another app as it is. Its CSS styles this subtree only. -->
<template>
  <view class="row" :class='#{ done: done }' :to='path_for("task", #{ id: id_of })'>
    <view class="box">
      <path class="tick" d="M 4 9 L 7 12 L 14 4" />
    </view>
    <view class="text">
      <text class="label">{{ label }}</text>
      <text class="note" r-if='note != ""'>{{ note }}</text>
    </view>
    <text class="chev">›</text>
  </view>
</template>

<style>
  .row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px;
    border-radius: 12px;
    background: #313244;
    transition: background 140ms ease-out, transform 140ms ease-out;
  }

  .row:hover { background: #3a3c51; transform: translateX(2px); }

  .box {
    width: 20px;
    height: 20px;
    flex-shrink: 0;
    border: 2px #585b70 solid;
    border-radius: 6px;
    background: #1e1e2e;
  }

  /* The tick is drawn in the box's own colour and hidden until the row is
     done, so nothing has to swap one icon for another. */
  .tick {
    width: 20px;
    height: 20px;
    stroke: #1e1e2e;
    stroke-width: 2.5px;
    fill: none;
    opacity: 0;
    transition: opacity 140ms ease-out;
  }

  .text {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .label { color: #cdd6f4; font-size: 15px; }
  .note { color: #6c7086; font-size: 12px; }
  .chev { color: #585b70; font-size: 18px; }
  /* `:class` above adds `done`, so how a finished task looks lives here, in
     the file that owns how a task looks. */
  .row.done .box { background: #a6e3a1; border: 2px #a6e3a1 solid; }
  .row.done .tick { opacity: 1; }
  .row.done .label { color: #6c7086; text-decoration: line-through; }
</style>

<script>
  // What the caller hands in, and all this file reads of the caller's. A tag
  // writes them kebab or snake alike: `:id-of` and `:id_of` both reach `id_of`.
  prop label;
  prop note = "";
  prop done = false;
  prop id_of;
</script>
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
app.rux              the frame, the state, and the routes
pages/               one file per route
  home.rux           the list
  new-task.rux       the form
  detail.rux         one task, by id
  missing.rux        the fallback, for a path nothing matches
components/          one file per component
  task-row.rux       one row of the list
assets/              images; `src` resolves relative to the .rux file
```

`components/` and `pages/` are conventions the runtime already follows:
`use pages::new_task;` names `pages/new-task.rux`, and an
`<image src="assets/logo.png">` resolves from the document's own directory, not
from wherever you happened to run `rux`.

## How it fits together

**`app.rux` owns the state.** A page is a component, and a component gets its
own copy of anything it declares, so state that outlives one screen lives in
`app.rux`. Pages read it by name and change it by calling the functions declared
there: functions are shared with every file in the project.

**A name is written twice, deliberately.** `use pages::new_task;` names a file,
so it is written the way files are; `<new-task>` is a custom element, so it is
written the way elements are. Either spelling resolves.

**An id is text.** A route carries `:id` out of the path as text, so the tasks
carry text ids and the two can be compared without converting anything.

## While you work

```bash
rux check          what is wrong, without opening a window
rux fmt            re-indent, and format the CSS
```

`rux check` on its own walks the project and skips components, because a
component read alone is missing the props its caller passes. Name one to check
it in full: `rux check pages/home.rux`.
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

/// The manifest a scaffolded project starts with.
///
/// **Three keys and no more.** `entry` is left out because `app.rux` is what
/// the lookup already prefers, so writing it would turn a convention into
/// configuration on day one. `icon` and `[signing]` are left out because they
/// name things that do not exist yet in a new project, and a manifest full of
/// commented-out keys is a manifest nobody reads.
const MANIFEST_TEMPLATE: &str = r#"[app]
name = "{name}"

# Reverse-DNS, and the one field worth getting right before you ship. An
# operating system files the app under this, so changing it later produces a
# different app rather than an update. `dev.example` is a placeholder.
id = "{id}"

version = "0.1.0"
"#;

const FILES: &[File] = &[
    File { path: "app.rux", body: APP },
    File { path: "pages/home.rux", body: HOME },
    File { path: "pages/new-task.rux", body: NEW_TASK },
    File { path: "pages/detail.rux", body: DETAIL },
    File { path: "pages/missing.rux", body: MISSING },
    File { path: "components/task-row.rux", body: TASK_ROW },
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

    let manifest = MANIFEST_TEMPLATE
        .replace("{name}", name)
        .replace("{id}", &default_id(name));
    let path = target.join(crate::manifest::MANIFEST);
    fs::write(&path, manifest).map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(())
}

/// A reverse-DNS id for a project called `name`, for the author to replace.
///
/// **The last segment has to be a valid Java identifier**, because that is what
/// Android package segments are, and a project name is allowed characters that
/// are not: `rux new my-app` would otherwise scaffold `dev.example.my-app`,
/// which `rux build` accepts and `aapt2` rejects, a whole Android build later.
/// So anything that is not a letter, a digit or an underscore becomes one, and
/// a leading digit gains a prefix rather than being dropped, since `3d-viewer`
/// and `d-viewer` are different names.
fn default_id(name: &str) -> String {
    let mut segment: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if segment.starts_with(|c: char| c.is_ascii_digit()) {
        segment.insert(0, 'a');
    }
    format!("dev.example.{}", segment.to_ascii_lowercase())
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

    /// Every `use` in the scaffold has to name a file the scaffold writes.
    ///
    /// This is the mistake the two spellings invite: `use pages::new_task;`
    /// names `pages/new-task.rux`, and a file named anything else renders
    /// nothing. Checked here as well as by the integration test, because this
    /// one names which import is wrong.
    #[test]
    fn every_scaffolded_import_names_a_file_the_scaffold_writes() {
        let written: Vec<&str> = FILES.iter().map(|f| f.path).collect();
        for body in [APP, HOME, NEW_TASK, DETAIL, MISSING, TASK_ROW] {
            for line in body.lines().map(str::trim) {
                let Some(path) = line.strip_prefix("use ").and_then(|l| l.strip_suffix(';')) else {
                    continue;
                };
                // `use pages::new_task;` -> `pages/new-task.rux`, the runtime's
                // own mapping.
                let file = format!("{}.rux", path.replace("::", "/").replace('_', "-"));
                assert!(
                    written.contains(&file.as_str()),
                    "`{line}` names `{file}`, which the scaffold does not write"
                );
            }
        }
    }

    /// A component is reached by writing its tag; a page is reached by naming
    /// it in a `view=`. Only the first of those is a tag, which is worth
    /// pinning because the two are easy to conflate when both live in files.
    #[test]
    fn the_component_is_written_as_a_tag() {
        assert!(HOME.contains("<task-row"), "the row is never written");
        assert!(HOME.contains("use components::task_row;"), "and never imported");
        assert!(FILES.iter().any(|f| f.path == "components/task-row.rux"));
        assert!(
            !APP.contains("<home"),
            "a page is named in a `view=`, not written as a tag"
        );
    }

    /// The router's own promise: every `view=` names an imported page, and
    /// there is a fallback. A route naming a view that is not imported is an
    /// error at load since v0.7.1, so this keeps the scaffold ahead of its own
    /// diagnostic.
    #[test]
    fn every_route_names_an_imported_view_and_there_is_a_fallback() {
        assert!(APP.contains("<route fallback"), "a mistyped path must land somewhere");
        for line in APP.lines().map(str::trim).filter(|l| l.starts_with("<route ")) {
            let Some(rest) = line.split("view=\"").nth(1) else { continue };
            let view = rest.split('"').next().unwrap_or_default();
            assert!(
                APP.contains(&format!("use pages::{view};")),
                "`view=\"{view}\"` is not imported"
            );
        }
    }
}
