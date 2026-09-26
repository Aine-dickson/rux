//! Script modules and stores, from files: step 7 of `docs/11-next.md`,
//! "Modules". A `.rux` file with no `<template>` is a module; it is loaded
//! once per document, so its top-level signals are one store every importer
//! shares.

use std::fs;
use std::path::PathBuf;

use rux_runtime::Document;

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A throwaway project directory. `files` is `(relative path, contents)`.
/// See `tests/imports.rs` for why the counter is there.
fn project(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rux_modules_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    ));
    let _ = fs::remove_dir_all(&dir);
    for (rel, body) in files {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    dir
}

fn texts(node: &rux_layout::Node) -> Vec<String> {
    let mut out: Vec<String> = node.text.iter().map(|t| t.text.clone()).collect();
    for child in &node.children {
        out.extend(texts(child));
    }
    out
}

fn tappable<'a>(node: &'a rux_layout::Node, label: &str) -> Option<&'a rux_layout::Node> {
    if node.on_tap.is_some() && texts(node).iter().any(|t| t.trim() == label) {
        return Some(node);
    }
    node.children.iter().find_map(|c| tappable(c, label))
}

fn tap(doc: &mut Document, label: &str) {
    let node = tappable(&doc.root, label).unwrap_or_else(|| panic!("nothing to tap says {label}")).clone();
    assert!(doc.apply_handler_in(&node.on_tap.clone().unwrap(), node.instance.as_deref()), "{label} did nothing");
}

fn problems(doc: &Document) -> Vec<String> {
    doc.diagnostics().warnings.iter().map(|w| w.message.clone()).collect()
}

fn load_err(dir: &PathBuf, file: &str) -> rux_runtime::LoadError {
    match Document::load_checked(dir.join(file)) {
        Ok(_) => panic!("{file} loaded"),
        Err(e) => e,
    }
}

const CART: &str = "<script>
export type Item = { name: string };
export let items: Item[] = signal([]);
export fn add(name: string) { items.push({ name: name }); }
export fn count(): int { items.length }
</script>";

/// A component that shows the store and adds to it.
const BADGE: &str = "<template><view><text>badge {{ cart.count() }}</text>\
    <button @tap=\"cart.add(`from badge`)\"><text>badge add</text></button></view></template>
<script>
use stores::cart;
</script>";

const APP: &str = "<template><screen>
  <text>app {{ cart.items.length }}</text>
  <button @tap=\"put(`tea`)\"><text>app add</text></button>
  <badge />
  <badge />
</screen></template>
<script>
use stores::cart;
use stores::cart::{add as put};
use components::badge;
</script>";

#[test]
fn one_store_is_shared_by_the_document_and_every_component() {
    let dir = project(&[("app.rux", APP), ("stores/cart.rux", CART), ("components/badge.rux", BADGE)]);
    let mut doc = Document::load(dir.join("app.rux")).expect("loads");
    assert!(problems(&doc).is_empty(), "{:?}", problems(&doc));
    let shown = |doc: &Document| texts(&doc.root);
    assert!(shown(&doc).contains(&"app 0".to_string()), "{:?}", shown(&doc));

    tap(&mut doc, "app add");
    let now = shown(&doc);
    assert!(now.contains(&"app 1".to_string()), "{now:?}");
    assert_eq!(now.iter().filter(|t| *t == "badge 1").count(), 2, "both badges follow it: {now:?}");

    // A component's handler writes the same store through its function.
    tap(&mut doc, "badge add");
    let now = shown(&doc);
    assert!(now.contains(&"app 2".to_string()), "{now:?}");
    assert_eq!(now.iter().filter(|t| *t == "badge 2").count(), 2, "{now:?}");
}

#[test]
fn both_spellings_and_a_bare_module_load_the_same() {
    let app = "<template><screen><text>{{ money.label(3) }} {{ half(3) }}</text></screen></template>
<script>
import money from \"./utils/money\";
import { half } from \"./utils/money\";
</script>";
    // No tags at all: the whole file is script.
    let money = "// money helpers\nexport fn label(n: int): string { `$${n}` }\nexport fn half(n: int): float { n / 2 }\n";
    let dir = project(&[("app.rux", app), ("utils/money.rux", money)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    assert!(problems(&doc).is_empty(), "{:?}", problems(&doc));
    assert!(texts(&doc.root).contains(&"$3 1.5".to_string()), "{:?}", texts(&doc.root));
}

#[test]
fn a_modules_top_level_runs_once_before_the_document() {
    let log = "export let lines = signal([\"log\"]);\nexport fn say(s: string) { lines.push(s); }";
    let a = "use stores::log;\nlog.say(\"a\");\nexport fn nothing() { }";
    let app = "<template><screen><text>{{ log.lines.join(\",\") }}</text></screen></template>
<script>
use stores::log;
use stores::a;
use stores::a as again;
log.say(\"app\");
</script>";
    let dir = project(&[("app.rux", app), ("stores/log.rux", log), ("stores/a.rux", a)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    assert!(texts(&doc.root).contains(&"log,a,app".to_string()), "{:?}", texts(&doc.root));
}

#[test]
fn what_a_module_does_not_allow_is_refused() {
    let page = |line: &str| format!("<template><screen><text>x</text></screen></template>\n<script>\n{line}\n</script>");
    let cases: &[(&str, &[(&str, &str)], &str)] = &[
        ("use stores::cart::{secret};", &[("stores/cart.rux", CART)], "does not export `secret`"),
        ("use stores::cart::{Item};", &[("stores/cart.rux", CART)], "`Item` is a type: import it with `type`"),
        ("use components::badge::{x};", &[("components/badge.rux", BADGE), ("stores/cart.rux", CART)], "exports nothing to pick"),
        ("use stores::loop_a;", &[("stores/loop_a.rux", "use stores::loop_b;\nexport fn a() { }"), ("stores/loop_b.rux", "use stores::loop_a;\nexport fn b() { }")], "in a circle"),
        ("use stores::bad;", &[("stores/bad.rux", "export let n = signal(1);\ncomputed twice = n * 2;")], "a `computed` belongs to a component"),
        ("use stores::ui;", &[("stores/ui.rux", "use components::badge;\nexport fn f() { }"), ("components/badge.rux", BADGE), ("stores/cart.rux", CART)], "a module has no <template>"),
    ];
    for (line, files, said) in cases {
        let app = page(line);
        let mut all = vec![("app.rux", app.as_str())];
        all.extend_from_slice(files);
        let dir = project(&all);
        let err = load_err(&dir, "app.rux");
        assert!(err.message.contains(said), "{line}: {}", err.message);
    }
}

#[test]
fn only_the_module_changes_its_state_and_the_checker_says_so() {
    let app = "<template><screen><text>x</text></screen></template>
<script>
use stores::cart;
fn clear() { cart.items = []; }
fn wrong() { cart.add(3); }
</script>";
    let dir = project(&[("app.rux", app), ("stores/cart.rux", CART)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let p = problems(&doc);
    assert!(p.iter().any(|m| m.contains("only stores/cart changes it")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("string")), "the argument is checked against the export: {p:?}");
}

#[test]
fn a_problem_in_a_module_is_reported_on_the_modules_own_line() {
    let bad = "<script>\nexport fn f(): int {\n  \"text\"\n}\n</script>";
    let app = "<template><screen><text>x</text></screen></template>\n<script>\nuse stores::bad;\n</script>";
    let dir = project(&[("app.rux", app), ("stores/bad.rux", bad)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let w = doc
        .diagnostics()
        .warnings
        .iter()
        .find(|w| w.file.as_ref().is_some_and(|f| f.ends_with("bad.rux")))
        .cloned()
        .unwrap_or_else(|| panic!("{:?}", problems(&doc)));
    assert!(w.line == Some(2) || w.line == Some(3), "{w:?}");
}

#[test]
fn a_type_a_module_does_not_export_is_imported_with_a_warning() {
    let types = "<script>\ntype Task = { id: int };\nexport type Page = { n: int };\n</script>";
    let app = "<template><screen><text>x</text></screen></template>\n<script>\nuse type types::{Task, Page};\nlet t: Task[] = signal([]);\nlet p: Page? = signal(none);\n</script>";
    let dir = project(&[("app.rux", app), ("types.rux", types)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let p = problems(&doc);
    assert_eq!(p.len(), 1, "{p:?}");
    assert!(p[0].contains("does not export `Task`") && p[0].contains("export type Task"), "{p:?}");
}
