//! `rux new [name]`: scaffold a project.
//!
//! **Two templates and one convention.** `minimal` is the default: a frame, one
//! page, one component, with nothing to delete before the first line of the
//! author's own. `tasks` is the larger worked example, the shape a bigger
//! project splits into. The Rust half, `native/` beside `rux.toml`, is on for
//! both unless `--no-native` says otherwise, because it is the one convention
//! every serious project meets, and it is learned by finding it in the same
//! place each time. The first real app on Rux put its entire window in one
//! 4,000-line `app.rux` and never used `pages/` at all, so the scaffold's job
//! is also to make the split the path of least resistance.
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

     This file owns two things: the window's frame and the routes. Each page
     lives in `pages/`, and the app's state lives in `stores/todo.rux`, where
     every file that imports it reads the same tasks. -->
<template>
  <screen class="app">
    <view class="header">
      <text class="brand">Tasks</text>
      <text class="tally">{{ todo.done_count() }} of {{ todo.tasks.len() }} done</text>
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
      <view class="tab" to="/" :class='{ here: route == "/" }'>
        <text class="tab-label">List</text>
      </view>
      <view class="tab" to="/new" :class='{ here: route == "/new" }'>
        <text class="tab-label">Add</text>
      </view>
    </view>
  </screen>
</template>

<style>
  * { box-sizing: border-box; }
  :root { color-scheme: dark; }

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
  use stores::todo;
</script>
"#;

/// The list. A page is an ordinary component that a `<route>` names, which is
/// why its root is a `<view>` and not a second `<screen>`.
const HOME: &str = r#"<!-- The list. A page is an ordinary component that a `<route>` names, so its
     root is a `<view>` and not a second `<screen>`. -->
<template>
  <view class="page">
    <view class="rows" r-if="todo.tasks.len() > 0">
      <task-row
      r-for="t in todo.tasks"
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
  * { box-sizing: border-box; }

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
  use stores::todo;
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

    <view class="save" :class='{ ready: label.trim() != "" }' @tap="save()">
      <text class="save-label">Add task</text>
    </view>
  </view>
</template>

<style>
  * { box-sizing: border-box; }

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
  use stores::todo;

  /* This page's own state. Two instances of a component would each get their
     own copy of these, which is why they live here and the task list does not. */
  let label = signal("");
  let note = signal("");

  fn save() {
    if label.trim() == "" {
      return;
    }
    /* The list is the store's: a page changes it by calling one of the
       functions `stores/todo.rux` exports, never by writing it. */
    todo.add(label.trim(), note.trim());
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

    <view class="card" r-if="task != none">
      <text class="label" :class='{ done: task.done }'>{{ task.label }}</text>
      <text class="note" r-if='task.note != ""'>{{ task.note }}</text>

      <view class="actions">
        <view class="action done-action" @tap="todo.toggle(id())">
          <text class="action-label" r-if="task.done">Mark as not done</text>
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
  * { box-sizing: border-box; }

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
  use stores::todo;

  fn id() {
    params?.id
  }

  // Read once, so the `r-if` above knows it is there below.
  computed task = todo.find(id());

  fn drop() {
    todo.remove(id());
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
      <text class="home-label">Back home</text>
    </view>
  </view>
</template>

<style>
  * { box-sizing: border-box; }

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
  <view class="row" :class='{ done: done }' :to='path_for("task", { id: id_of })'>
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
  * { box-sizing: border-box; }

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
  prop label: string;
  prop note = "";
  prop done = false;
  prop id_of: string;
</script>
"#;

/// The app's state: a store. A module (a file with no `<template>`) is
/// loaded once, so every file that imports it reads the same tasks.
const TODO: &str = r#"<!-- The app's state. A file with no <template> is a module: it is loaded
     once, so every file that imports it with `use stores::todo;` reads the
     same tasks. What it exports is all another file sees, and only its own
     functions change its state: the list's rules live in one place. -->
<script>
  export type Task = { id: string, label: string, note: string, done: bool };

  /* An id is text, not a number. A route carries `:id` out of the path as
     text, so text on both sides is the comparison that needs no conversion. */
  export let tasks: Task[] = signal([
    { id: "1", label: "Read the Rux guide", note: "ruxlang.dev/learn", done: true },
    { id: "2", label: "Add a task of your own", note: "The tab below, or the button on the list", done: false }
  ]);
  let next_id = signal(3);

  export fn done_count(): int {
    let n = 0;
    for t in tasks {
      if t.done { n += 1; }
    }
    n
  }

  export fn add(label: string, note: string) {
    tasks.push({ id: "" + next_id, label: label, note: note, done: false });
    next_id += 1;
  }

  export fn find(id: string): Task? {
    for t in tasks {
      if t.id == id { return t; }
    }
    none
  }

  export fn toggle(id: string) {
    for i in 0..tasks.len() {
      if tasks[i].id == id {
        tasks[i].done = !tasks[i].done;
      }
    }
  }

  export fn remove(id: string) {
    let kept: Task[] = [];
    for t in tasks {
      if t.id != id { kept.push(t); }
    }
    tasks = kept;
  }
</script>
"#;

/// The `minimal` template's entry point: a frame, one route and a fallback.
///
/// Everything a project needs to show where things go, and nothing to delete
/// first. The `tasks` template is the same shape grown into a whole app.
const MIN_APP: &str = r#"<!-- The entry point. `rux run` looks for this file here and in every parent
     directory, so it works from anywhere inside the project.

     This file owns two things: the window's frame and the routes. Each page
     lives in `pages/`. Add a page by writing a file there, importing it in the
     script below, and naming it in a <route>. -->
<template>
  <screen class="app">
    <view class="header">
      <text class="brand">%%NAME%%</text>
    </view>

    <view class="stage">
      <router>
        <route path="/" view="home" name="home" />
        <route fallback view="missing" />
      </router>
    </view>
  </screen>
</template>

<style>
  * { box-sizing: border-box; }
  :root { color-scheme: dark; }

  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: #1e1e2e;
    font-family: sans-serif;
    /* The edges a phone will not let an app draw in: the status bar at the
       top, the gesture bar or the home indicator at the bottom. Both are zero
       on a desktop, so this changes nothing there. Try it with
       `rux run --preview phone`. */
    padding-top: env(safe-area-inset-top);
    padding-bottom: env(safe-area-inset-bottom);
  }

  .header { padding: 20px 20px 12px 20px; }
  .brand { color: #cdd6f4; font-size: 26px; font-weight: 700; }

  /* The router's page sits here. `overflow-y: auto` is what makes a long page
     scroll: nothing scrolls in Rux unless it is told to. */
  .stage {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 0 20px 20px 20px;
  }
</style>

<script>
  use pages::home;
  use pages::missing;
</script>
"#;

/// The `minimal` template's page: a field, and a greeting that Rust writes.
const MIN_HOME: &str = r#"<!-- The home page. A page is an ordinary component that a `<route>` names, so
     its root is a `<view>` and not a second `<screen>`.

     %%ORIGIN%% -->
<template>
  <view class="page">
    <text class="heading">Hello from Rux</text>
    <text class="lead">Type a name. %%LEAD%%</text>

    <view class="field">
      <text class="field-label">Your name</text>
      <input class="input" r-model="who" placeholder="World" />
    </view>

    <message-card :message="greeting" note="%%NOTE%%" />
  </view>
</template>

<style>
  * { box-sizing: border-box; }

  .page {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 8px 0 24px 0;
  }

  .heading { color: #cdd6f4; font-size: 20px; font-weight: 700; }
  .lead { color: #6c7086; font-size: 14px; }
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
</style>

"#;

/// The script when the project has a `native/` crate: Rust writes the greeting.
const MIN_HOME_NATIVE_SCRIPT: &str = r#"<script>
  use components::message_card;
  use stores::greeter;

  /* Every `<input>` needs an `r-model`: it is the binding that makes the field
     editable at all. */
  let who = signal("");

  /* A `computed` runs again whenever a signal it reads changes. The store
     calls into Rust; to this page that is an ordinary function. */
  computed greeting = greeter.greet(who);
</script>
"#;

/// The same page for `--no-native`: the greeting is written here instead, so
/// the project still runs with no Rust in it.
const MIN_HOME_PLAIN_SCRIPT: &str = r#"<script>
  use components::message_card;
  use stores::greeter;

  /* Every `<input>` needs an `r-model`: it is the binding that makes the field
     editable at all. */
  let who = signal("");

  /* A `computed` runs again whenever a signal it reads changes. */
  computed greeting = greeter.greet(who);
</script>
"#;

/// The home page for the choices made.
fn min_home(native: bool) -> String {
    let (origin, lead, note, script) = if native {
        (
            "The greeting is computed by Rust, in `native/src/hello.rs`, through `stores/greeter.rux`.",
            "The greeting is written by Rust.",
            "From native/src/hello.rs",
            MIN_HOME_NATIVE_SCRIPT,
        )
    } else {
        (
            "The greeting is computed in `stores/greeter.rux`.",
            "The greeting is written in Rux.",
            "Computed in the script",
            MIN_HOME_PLAIN_SCRIPT,
        )
    };
    let top = MIN_HOME.replace("%%ORIGIN%%", origin).replace("%%LEAD%%", lead).replace("%%NOTE%%", note);
    format!("{top}
{script}")
}

/// The `minimal` template's store: where the app's rules live.
const MIN_STORE_PLAIN: &str = r#"<!-- The app's state and rules. A file with no <template> is a module: it is
     loaded once, so every file that imports it with `use stores::greeter;`
     shares it. Only what it exports is visible, and as the app grows, state
     that more than one page reads (a list, a signed-in user) belongs here and
     not in a page. -->
<script>
  export fn greet(who: string): string {
    if who.trim() == "" { "Hello, World!" } else { "Hello, " + who.trim() + "!" }
  }
</script>
"#;

/// The same store for a project with a `native/` crate: Rust writes the greeting.
const MIN_STORE_NATIVE: &str = r#"<!-- The app's state and rules. A file with no <template> is a module: it is
     loaded once, so every file that imports it with `use stores::greeter;`
     shares it. Only what it exports is visible, and as the app grows, state
     that more than one page reads (a list, a signed-in user) belongs here and
     not in a page. Here the rule is Rust's, in `native/src/hello.rs`. -->
<script>
  use native::hello;

  export fn greet(who: string): string {
    hello.greet(who)
  }
</script>
"#;

/// The `minimal` template's component: the file that shows how a prop arrives.
const MIN_CARD: &str = r#"<!-- A card with a message.

     A component declares its props at the bottom (`message`, `note`) and reads
     nothing else of its caller's, so this file can be dropped into another app
     as it is. Its CSS styles this subtree only. -->
<template>
  <view class="card">
    <text class="message">{{ message }}</text>
    <text class="note" r-if='note != ""'>{{ note }}</text>
  </view>
</template>

<style>
  * { box-sizing: border-box; }

  .card {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 18px;
    border-radius: 14px;
    background: #313244;
  }

  .message { color: #cdd6f4; font-size: 20px; font-weight: 700; }
  .note { color: #6c7086; font-size: 12px; }
</style>

<script>
  prop message: string;
  prop note = "";
</script>
"#;

/// The Rust half, when there is one. The crate's name is fixed: the generated
/// crate `rux build` makes is named after the app, so two of one name would
/// collide, and nothing refers to this one by name except its own Cargo.toml.
const NATIVE_CARGO: &str = r#"[package]
name = "app-native"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
# What every app's native/ crate depends on, under the name `rux`. It has to be
# the version of Rux that builds the app: a second copy would be a second
# registry, and the exports would land in the one nobody reads.
rux = { package = "rux-native", version = "%%RUX%%" }

# Crates from crates.io go here, added with `cargo add` from this directory.
# They are compiled into the app for every target, and not every crate builds
# for Android or the web: one that links OpenSSL or starts threads is the usual
# trouble. `rux build` keeps the versions it resolved in locks/.
"#;

const NATIVE_LIB: &str = "//! The Rust half of this app: what its pages reach as `native::<module>`.\n\
    //!\n\
    //! A `pub mod hello;` here is `use native::hello;` in a `.rux` file.\n\
    \n\
    pub mod hello;\n";

const NATIVE_HELLO: &str = r#"//! The greeting on the home page: `use native::hello;` in `pages/home.rux`.

/// A greeting for `name`, or for the world when there is no name.
///
/// `#[rux::export]` is what makes a function reachable from a `.rux` file.
/// `rux check` reads its signature from this source, without building anything,
/// so a wrong call is reported where it is written.
#[rux::export]
pub fn greet(name: String) -> String {
    let name = name.trim();
    if name.is_empty() {
        "Hello, World!".to_string()
    } else {
        format!("Hello, {name}!")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_greeted_and_no_name_greets_the_world() {
        assert_eq!(greet("Ada".into()), "Hello, Ada!");
        assert_eq!(greet("  ".into()), "Hello, World!");
    }
}
"#;

/// Points rust-analyzer at the crate, which is not at the project root where it
/// looks.
const VSCODE_SETTINGS: &str = "{\n  \"rust-analyzer.linkedProjects\": [\"native/Cargo.toml\"]\n}\n";

const GITIGNORE: &str = "# Build output: `rux build` writes dist/, and the generated crate it\n\
                         # compiles lives in .rux-build/. locks/ is NOT ignored: commit it.\n\
                         dist/\n\
                         .rux-build/\n\
                         # The native/ crate's own build output, if cargo is run there.\n\
                         target/\n\
                         \n\
                         # Editors and OS cruft.\n\
                         .DS_Store\n\
                         Thumbs.db\n\
                         *.swp\n";

/// The manifest a scaffolded project starts with.
///
/// **Three keys and no more.** `entry` is left out because `app.rux` is what
/// the lookup already prefers, so writing it would turn a convention into
/// configuration on day one. `icon` and `[signing]` are left out because they
/// name things that do not exist yet in a new project, and a manifest full of
/// commented-out keys is a manifest nobody reads.
const MANIFEST_TEMPLATE: &str = r#"[app]
name = "%%NAME%%"

# Reverse-DNS, and the one field worth getting right before you ship. An
# operating system files the app under this, so changing it later produces a
# different app rather than an update. `dev.example` is a placeholder.
id = "%%ID%%"

version = "0.1.0"
"#;

/// What every template shares.
const COMMON_FILES: &[File] = &[
    File { path: ".gitignore", body: GITIGNORE },
    File { path: "assets/.gitkeep", body: "" },
];

const MINIMAL_FILES: &[File] = &[
    File { path: "app.rux", body: MIN_APP },
    File { path: "pages/missing.rux", body: MISSING },
    File { path: "components/message-card.rux", body: MIN_CARD },
];

const TASKS_FILES: &[File] = &[
    File { path: "app.rux", body: APP },
    File { path: "stores/todo.rux", body: TODO },
    File { path: "pages/home.rux", body: HOME },
    File { path: "pages/new-task.rux", body: NEW_TASK },
    File { path: "pages/detail.rux", body: DETAIL },
    File { path: "pages/missing.rux", body: MISSING },
    File { path: "components/task-row.rux", body: TASK_ROW },
];

const NATIVE_FILES: &[File] = &[
    File { path: "native/Cargo.toml", body: NATIVE_CARGO },
    File { path: "native/src/lib.rs", body: NATIVE_LIB },
    File { path: "native/src/hello.rs", body: NATIVE_HELLO },
    File { path: ".vscode/settings.json", body: VSCODE_SETTINGS },
];

/// Which project `rux new` writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Template {
    /// A frame, one page that calls Rust, one component. The default: nothing
    /// to delete before the first line of your own.
    Minimal,
    /// A small tasks app: routes, a store, forms, a detail page. The worked
    /// example of how a larger project is split.
    Tasks,
}

impl Template {
    fn parse(name: &str) -> Result<Self, String> {
        match name {
            "minimal" => Ok(Template::Minimal),
            "tasks" => Ok(Template::Tasks),
            other => Err(format!("unknown template `{other}`; the ones that exist are `minimal` and `tasks`")),
        }
    }

    fn files(self) -> &'static [File] {
        match self {
            Template::Minimal => MINIMAL_FILES,
            Template::Tasks => TASKS_FILES,
        }
    }

    /// The tree the README draws, without the native half.
    fn tree(self) -> &'static str {
        match self {
            Template::Minimal => {
                "app.rux              the frame and the routes\n\
                 pages/               one file per route\n\
                 \x20 home.rux           the home page\n\
                 \x20 missing.rux        the fallback, for a path nothing matches\n\
                 stores/              the app's state and rules\n\
                 \x20 greeter.rux        writes the greeting\n\
                 components/          one file per component\n\
                 \x20 message-card.rux  a card that takes props\n"
            }
            Template::Tasks => {
                "app.rux              the frame and the routes\n\
                 stores/              the app's state\n\
                 \x20 todo.rux           the tasks, and the functions that change them\n\
                 pages/               one file per route\n\
                 \x20 home.rux           the list\n\
                 \x20 new-task.rux       the form\n\
                 \x20 detail.rux         one task, by id\n\
                 \x20 missing.rux        the fallback, for a path nothing matches\n\
                 components/          one file per component\n\
                 \x20 task-row.rux       one row of the list\n"
            }
        }
    }

    /// The component the README tells the reader to check by name.
    fn component(self) -> &'static str {
        match self {
            Template::Minimal => "components/message-card.rux",
            Template::Tasks => "components/task-row.rux",
        }
    }
}

/// The project's README, written for the choices that were made.
fn readme(name: &str, template: Template, native: bool) -> String {
    let mut out = format!(
        "# {name}\n\
         \n\
         A [Rux](https://ruxlang.dev) app.\n\
         \n\
         ## Run it\n\
         \n\
         ```bash\n\
         rux run\n\
         ```\n\
         \n\
         `rux run` looks for `app.rux` here and in every parent directory, so it works\n\
         from anywhere inside the project. Edits to a `.rux` file reload in the open\n\
         window; nothing needs rebuilding.\n"
    );
    if native {
        out.push_str(
            "\nThe first run compiles the app's Rust together with Rux, which takes several\n\
             minutes. Later runs reuse that, and rebuild only when something under `native/`\n\
             changes.\n",
        );
    }
    out.push_str("\n## What is here\n\n```\n");
    out.push_str(template.tree());
    if native {
        out.push_str(
            "native/              the app's Rust, always here, beside rux.toml\n\
             \x20 Cargo.toml         its dependencies\n\
             \x20 src/lib.rs         one `pub mod` per `native::<module>`\n\
             \x20 src/hello.rs       the greeting on the home page\n",
        );
    }
    out.push_str(
        "assets/              images; `src` resolves relative to the .rux file\n\
         rux.toml             name, id and version\n\
         locks/               dependency versions, written by `rux build`: commit it\n\
         ```\n\
         \n\
         `components/` and `pages/` are conventions the runtime already follows:\n\
         `use pages::new_task;` names `pages/new-task.rux`, and an\n\
         `<image src=\"assets/logo.png\">` resolves from the document's own directory, not\n\
         from wherever you happened to run `rux`.\n",
    );
    if native {
        out.push_str(
            "\n## Rust\n\
             \n\
             **The Rust half of an app lives in `native/`, beside `rux.toml`.** It is an\n\
             ordinary library crate. A function marked `#[rux::export]` in\n\
             `native/src/hello.rs` is `hello.greet(...)` after `use native::hello;` in a\n\
             `.rux` file, and `rux check` reads its signature from the source without building.\n\
             `rux native` prints what Rux sees.\n\
             \n\
             To use a crate from crates.io, add it from inside `native/`:\n\
             \n\
             ```bash\n\
             cd native\n\
             cargo add serde_json\n\
             cargo test        # the crate's own tests\n\
             ```\n\
             \n\
             Everything in `native/Cargo.toml` is compiled into the app for every target,\n\
             and not every crate builds for Android or the web. A crate that links OpenSSL\n\
             or starts threads is the usual trouble, so check a crate's target support\n\
             before depending on it.\n",
        );
    }
    out.push_str(
        "\n## Building\n\
         \n\
         ```bash\n\
         rux build                    # dist/<name>.exe\n\
         rux build --release          # embeds the documents\n\
         rux build --target android   # dist/<name>.apk\n\
         rux build --target web       # dist/web/\n\
         ```\n\
         \n\
         What comes out is in `dist/`. The crate `rux build` generates and compiles is in\n\
         `.rux-build/`, and its compiled output is in `.rux-build/<target>/target/`; both\n\
         are disposable. Set `CARGO_TARGET_DIR` to share one compiled copy of Rux across\n\
         projects instead of compiling it in each.\n\
         \n\
         The first build writes `locks/<target>.lock`, the dependency versions it\n\
         resolved. Commit them, so every machine builds the same versions, and pass\n\
         `--locked` in CI to fail instead of resolving anything new.\n",
    );
    if template == Template::Tasks {
        out.push_str(
            "\n## How it fits together\n\
             \n\
             **`stores/todo.rux` owns the state.** A page is a component, and a component\n\
             gets its own copy of anything it declares, so state that outlives one screen\n\
             lives in a store: a file with no `<template>`, loaded once, so every file that\n\
             imports it with `use stores::todo;` reads the same tasks. A page reads\n\
             `todo.tasks` and changes the list by calling what the store exports,\n\
             `todo.add(…)` or `todo.toggle(id)`; nothing outside the store writes it.\n\
             \n\
             **A name is written twice, deliberately.** `use pages::new_task;` names a file,\n\
             so it is written the way files are; `<new-task>` is a custom element, so it is\n\
             written the way elements are. Either spelling resolves.\n\
             \n\
             **An id is text.** A route carries `:id` out of the path as text, so the tasks\n\
             carry text ids and the two can be compared without converting anything.\n",
        );
    }
    out.push_str(&
        "\n## While you work\n\
         \n\
         ```bash\n\
         rux check          what is wrong, without opening a window\n\
         rux fmt            re-indent, and format the CSS\n\
         ```\n\
         \n\
         `rux check` on its own walks the whole project, from any folder in it. A\n\
         component is read on its own, owing the props it declares to whoever uses it,\n\
         and a page is read through the app, with the app's state in scope. To check\n\
         just one file: `rux check {}`.\n"
            .replace("{}", template.component()),
    );
    out
}

/// Create the project. Returns a process exit code.
pub fn create(args: &[String]) -> i32 {
    let mut name = None;
    let mut template = Template::Minimal;
    // **On unless asked off.** The Rust half of an app is not an extra: any
    // project past a demo reaches for it, and a convention that has to be
    // opted into is one people meet by accident, in the wrong place.
    let mut native = true;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--template" => {
                let Some(value) = args.get(i + 1) else {
                    eprintln!("rux: --template needs a name: `minimal` or `tasks`");
                    return 2;
                };
                match Template::parse(value) {
                    Ok(t) => template = t,
                    Err(why) => {
                        eprintln!("rux: {why}");
                        return 2;
                    }
                }
                i += 1;
            }
            "--no-native" => native = false,
            _ if arg.starts_with('-') => {
                eprintln!("rux: unknown option `{arg}` for `new`");
                return 2;
            }
            _ => {
                if name.is_some() {
                    eprintln!("rux: `new` takes one name, not several");
                    return 2;
                }
                name = Some(arg.clone());
            }
        }
        i += 1;
    }

    // No name given: ask for one, as `rux new my-app` would have been told it.
    // Only at a terminal. A script or CI job with nobody to answer would hang
    // on the question, so it gets the error instead.
    let name = match name {
        Some(name) => name,
        None => match ask_name() {
            Some(name) => name,
            None => {
                eprintln!("rux: `new` needs a name, like `rux new my-app`");
                return 2;
            }
        },
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

    if let Err(e) = write_all(&target, &display, template, native) {
        eprintln!("rux: {e}");
        return 1;
    }

    println!("Created {}", target.display());
    println!();
    println!("  cd {name}");
    println!("  rux run");
    println!();
    println!("`rux run` finds app.rux from anywhere inside the project. Edits reload live.");
    if native {
        println!("The first run compiles the app's Rust with Rux, which takes several minutes.");
    }
    0
}

/// Ask for the project name, until it is one `check_name` accepts.
///
/// `None` when nobody can answer: stdin is not a terminal, or it closed. An
/// empty answer asks again rather than giving up, since pressing Enter early is
/// a slip and not a refusal; end of input is the way out.
fn ask_name() -> Option<String> {
    use std::io::IsTerminal;

    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return None;
    }
    prompt_name(&mut stdin.lock(), &mut std::io::stdout(), &mut std::io::stderr())
}

/// The question and its retries, over any reader, so a test can answer it.
fn prompt_name(
    input: &mut impl std::io::BufRead,
    out: &mut impl std::io::Write,
    err: &mut impl std::io::Write,
) -> Option<String> {
    loop {
        let _ = write!(out, "Project name: ");
        let _ = out.flush();
        let mut line = String::new();
        if input.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let name = line.trim();
        match check_name(name) {
            Ok(()) => return Some(name.to_string()),
            Err(why) => {
                let _ = writeln!(err, "rux: {why}");
            }
        }
    }
}

/// The files a project of these choices is made of, as path and contents.
fn files_for(template: Template, native: bool) -> Vec<(&'static str, String)> {
    let mut files: Vec<&File> = COMMON_FILES.iter().chain(template.files()).collect();
    if native {
        files.extend(NATIVE_FILES);
    }
    let mut out: Vec<(&'static str, String)> =
        files.iter().map(|f| (f.path, f.body.to_string())).collect();
    if template == Template::Minimal {
        out.push(("pages/home.rux", min_home(native)));
        out.push(("stores/greeter.rux", if native { MIN_STORE_NATIVE } else { MIN_STORE_PLAIN }.to_string()));
    }
    out
}

fn write_all(target: &Path, name: &str, template: Template, native: bool) -> Result<(), String> {
    for (file_path, body) in files_for(template, native) {
        let path = target.join(file_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        // The two placeholders a template may carry: the project's name, and the
        // version of Rux that wrote it, which the native crate has to match.
        let body = body
            .replace("%%NAME%%", name)
            .replace("%%RUX%%", env!("CARGO_PKG_VERSION"));
        fs::write(&path, body).map_err(|e| format!("writing {}: {e}", path.display()))?;
    }
    let readme = readme(name, template, native);
    let path = target.join("README.md");
    fs::write(&path, readme).map_err(|e| format!("writing {}: {e}", path.display()))?;

    let manifest = MANIFEST_TEMPLATE
        .replace("%%NAME%%", name)
        .replace("%%ID%%", &default_id(name));
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

    /// The prompt asks again after a slip and gives up only at end of input.
    #[test]
    fn the_name_prompt_asks_again_until_the_name_is_usable() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut input = std::io::Cursor::new("\nmy app\n  my-app  \n");
        assert_eq!(prompt_name(&mut input, &mut out, &mut err).as_deref(), Some("my-app"));
        assert_eq!(String::from_utf8(out).unwrap().matches("Project name: ").count(), 3);
        let err = String::from_utf8(err).unwrap();
        assert!(err.contains("cannot be empty") && err.contains("use letters"), "{err}");

        let mut closed = std::io::Cursor::new("my app\n");
        assert_eq!(prompt_name(&mut closed, &mut Vec::new(), &mut Vec::new()), None);
    }

    /// Every `use` in the scaffold has to name a file the scaffold writes.
    ///
    /// This is the mistake the two spellings invite: `use pages::new_task;`
    /// names `pages/new-task.rux`, and a file named anything else renders
    /// nothing. Checked here as well as by the integration test, because this
    /// one names which import is wrong.
    #[test]
    fn every_scaffolded_import_names_a_file_the_scaffold_writes() {
        for (template, bodies) in [
            (Template::Tasks, vec![APP, HOME, NEW_TASK, DETAIL, MISSING, TASK_ROW, TODO]),
            (
                Template::Minimal,
                vec![MIN_APP, MIN_HOME_NATIVE_SCRIPT, MIN_HOME_PLAIN_SCRIPT, MISSING, MIN_CARD],
            ),
        ] {
            let written: Vec<&str> = files_for(template, true).iter().map(|f| f.0).collect();
            for body in bodies {
                for line in body.lines().map(str::trim) {
                    let Some(path) = line.strip_prefix("use ").and_then(|l| l.strip_suffix(';')) else {
                        continue;
                    };
                    // The app's Rust is a crate, not a `.rux` file.
                    if path.starts_with("native::") {
                        continue;
                    }
                    // `use pages::new_task;` -> `pages/new-task.rux`, the
                    // runtime's own mapping.
                    let file = format!("{}.rux", path.replace("::", "/").replace('_', "-"));
                    assert!(
                        written.contains(&file.as_str()),
                        "`{line}` names `{file}`, which the {template:?} scaffold does not write"
                    );
                }
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
        assert!(TASKS_FILES.iter().any(|f| f.path == "components/task-row.rux"));
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
        for app in [APP, MIN_APP] {
            assert!(app.contains("<route fallback"), "a mistyped path must land somewhere");
            for line in app.lines().map(str::trim).filter(|l| l.starts_with("<route ")) {
                let Some(rest) = line.split("view=\"").nth(1) else { continue };
                let view = rest.split('"').next().unwrap_or_default();
                assert!(
                    app.contains(&format!("use pages::{view};")),
                    "`view=\"{view}\"` is not imported"
                );
            }
        }
    }
}
