//! The type checker, as a document meets it: findings on the file's own lines,
//! types imported from another file, and a component checked on its own with
//! its props typed. The rules themselves are tested in `rux-script`
//! (`check.rs`); these are about the wiring. See `docs/10-types.md`.

use std::fs;
use std::path::PathBuf;

use rux_runtime::Document;

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn project(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rux_type_check_{}_{}",
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

/// Every finding as (line, is it an error, message).
fn found(doc: &Document) -> Vec<(Option<usize>, bool, String)> {
    doc.diagnostics().warnings.iter().map(|w| (w.line, w.is_error(), w.message.clone())).collect()
}

fn load_err(path: PathBuf) -> String {
    match Document::load(path) {
        Ok(_) => panic!("loaded, and should not have"),
        Err(e) => e,
    }
}

const TYPES: &str = "<script>\n\
    type Task = { id: int, title: string, done: bool };\n\
    type Filter = \"all\" | \"open\" | \"done\";\n\
    type Board = { tasks: Task[], filter: Filter };\n\
    </script>\n";

fn page(script: &str) -> String {
    format!("<template><screen><text>x</text></screen></template>\n<script>\n{script}\n</script>\n")
}

/// A type error is placed on the line of the file it is on, counting the
/// template above the script.
#[test]
fn a_type_error_is_on_its_file_line() {
    let dir = project(&[("app.rux", &page("let a = 1;\nlet n: int = \"x\";"))]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let f = found(&doc);
    // Line 1 is the template, 2 is `<script>`, 3 is `let a`, 4 is `let n`.
    assert!(f.iter().any(|(line, err, m)| *line == Some(4) && *err && m.contains("where `int`")), "{f:?}");
}

/// `use types::Task` makes `Task` known, and checks against it.
#[test]
fn an_imported_type_is_checked_against() {
    let script = "use types::Task;\nlet t: Task = { id: 1, titel: \"a\", done: false };";
    let dir = project(&[("app.rux", &page(script)), ("types.rux", TYPES)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let f = found(&doc);
    assert!(f.iter().any(|(_, err, m)| *err && m.contains("did you mean `title`")), "{f:?}");
}

/// A generic type crosses files with its parameters: checked in the file
/// that imports it, and at a component's tag when a prop takes one.
#[test]
fn an_imported_generic_type_keeps_its_parameters() {
    let types = "<script>\n  type Page<T> = { items: T[], next: string? };\n</script>\n";
    let script = "use types::Page;\nlet p: Page<int> = { items: [\"a\"], next: none };";
    let dir = project(&[("app.rux", &page(script)), ("types.rux", types)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let f = found(&doc);
    assert!(f.iter().any(|(_, err, m)| *err && m.contains("where `int` is expected")), "{f:?}");

    let list = "<template><view><text>{{ page.items.length }}</text></view></template>\n\
        <script>\n  use types::Page;\n  prop page: Page<int>;\n</script>\n";
    let app = "<template><screen>\n<list :page=\"raw\" />\n</screen></template>\n\
        <script>\nuse components::list;\nlet raw: any = { items: [\"a\"], next: none };\n</script>\n";
    let dir = project(&[("app.rux", app), ("components/list.rux", list), ("types.rux", types)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let f = found(&doc);
    assert!(f.iter().any(|(_, err, m)| *err && m.contains("which is not the `Page<int>` `prop page` takes")), "{f:?}");
}

/// An imported type may be built from the types beside it, and those resolve;
/// naming one without importing it is an error that says how to.
#[test]
fn a_type_beside_an_imported_one_resolves_and_is_not_nameable() {
    let script = "use types::Board;\n\
                  let b: Board = { tasks: [{ id: 1, title: \"a\", done: false }], filter: \"al\" };\n\
                  let f: Filter = \"all\";";
    let dir = project(&[("app.rux", &page(script)), ("types.rux", TYPES)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let f = found(&doc);
    assert!(f.iter().any(|(_, err, m)| *err && m.contains("did you mean \"all\"")), "{f:?}");
    assert!(
        f.iter().any(|(_, err, m)| *err && m.contains("`Filter` is not imported here; bring it in with `use types::Filter;`")),
        "{f:?}"
    );
}

/// A type import naming a type its file does not declare says what the file
/// does declare.
#[test]
fn a_type_the_file_does_not_declare() {
    let dir = project(&[("app.rux", &page("use types::Tasks;")), ("types.rux", TYPES)]);
    let err = load_err(dir.join("app.rux"));
    assert!(err.contains("`types.rux` has no type `Tasks`"), "{err}");
    assert!(err.contains("`Task`, `Filter`, `Board`"), "{err}");
}

/// A component checked on its own: a typed prop is what it says, an untyped
/// one with no default is warned, and a default that does not fit its type is
/// an error.
#[test]
fn a_component_on_its_own_knows_its_props() {
    let comp = "<template><view><text>{{ label }}</text></view></template>\n\
        <script>\n\
          prop label: string;\n\
          prop size: int = \"big\";\n\
          prop tone;\n\
          fn shout(): int { label }\n\
        </script>\n";
    let dir = project(&[("components/badge.rux", comp)]);
    let doc = Document::load(dir.join("components/badge.rux")).expect("loads");
    let f = found(&doc);
    assert!(f.iter().any(|(_, err, m)| *err && m.contains("where `int` is expected")), "the default: {f:?}");
    assert!(f.iter().any(|(_, err, m)| !*err && m.contains("`prop tone` has no type")), "{f:?}");
    assert!(
        f.iter().any(|(_, err, m)| *err && m.contains("`shout` is declared to return `int`, and this is `string`")),
        "the prop's type reaches the body: {f:?}"
    );
}

/// A component's functions ride in the document's script, and a finding in
/// one is the component's to report, not the document's.
#[test]
fn a_components_function_is_not_reported_against_the_document() {
    let comp = "<template><view><text>c</text></view></template>\n\
        <script>\n  fn broken(): int { \"x\" }\n</script>\n";
    let app = "<template><screen><card /></screen></template>\n<script>\nuse components::card;\n</script>\n";
    let dir = project(&[("app.rux", app), ("components/card.rux", comp)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let f = found(&doc);
    assert!(!f.iter().any(|(_, _, m)| m.contains("broken")), "{f:?}");
}

/// A program with nothing wrong in it says nothing, and the typed version of
/// an ordinary page runs as it did.
#[test]
fn a_clean_typed_page_is_quiet() {
    let script = "use types::Task;\nuse types::Filter;\n\
                  let tasks: Task[] = signal([]);\n\
                  let filter: Filter = signal(\"all\");\n\
                  fn visible(): Task[] { tasks.filter(t => filter == \"all\" || t.done == (filter == \"done\")) }\n\
                  fn add(title: string) { tasks = tasks + [{ id: tasks.length, title: title, done: false }]; }";
    let dir = project(&[("app.rux", &page(script)), ("types.rux", TYPES)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    assert!(found(&doc).is_empty(), "{:?}", found(&doc));
}

/// A template is checked against the script: an `r-for` variable is an
/// element of its list, a bound attribute takes what it takes, `r-model`
/// matches its field, and each finding is on the template's own line.
#[test]
fn a_template_is_checked_against_its_script() {
    let app = "<template><screen>\n\
        <text r-for=\"t in tasks\">{{ t.titel }}</text>\n\
        <button :disabled=\"count\">go</button>\n\
        <input type=\"number\" r-model=\"name\" />\n\
        <text :class=\"{ on: tasks }\">x</text>\n\
        </screen></template>\n\
        <script>\nuse types::Task;\nlet tasks: Task[] = signal([]);\n\
        let count = signal(0);\nlet name = signal(\"\");\n</script>\n";
    let dir = project(&[("app.rux", app), ("types.rux", TYPES)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let f = found(&doc);
    let at = |line: usize, part: &str| f.iter().any(|(l, err, m)| *l == Some(line) && *err && m.contains(part));
    assert!(at(2, "`{{ }}`: `Task` has no field `titel`; did you mean `title`?"), "{f:?}");
    assert!(at(3, "`:disabled` on <button>: this is `int`, where `bool` is expected"), "{f:?}");
    assert!(at(4, "`r-model`: the field writes `float` into `name`, which holds `string`"), "{f:?}");
    assert!(at(5, "`:class` on <text>"), "{f:?}");
}

/// `r-if` narrows what is under it, and `r-else` learns the opposite.
#[test]
fn an_r_if_narrows_the_elements_under_it() {
    let app = "<template><screen>\n\
        <text r-if=\"t?.note != null\">{{ t.note }}</text>\n\
        <text r-else>{{ t.note }}</text>\n\
        </screen></template>\n\
        <script>\ntype T = { note?: string };\nlet t: T = signal({});\n</script>\n";
    let dir = project(&[("app.rux", app)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let f: Vec<_> = found(&doc).into_iter().filter(|(_, _, m)| m.contains("may be absent")).collect();
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!(f[0].0, Some(3), "only the r-else reads it unchecked");
}

/// A prop is checked at the tag: a plain attribute passes its text, a bound
/// one its expression's type.
#[test]
fn a_prop_is_checked_at_the_tag() {
    let comp = "<template><view><text>{{ label }}</text></view></template>\n\
        <script>\n  prop label: string;\n  prop kind: \"primary\" | \"quiet\" = \"quiet\";\n</script>\n";
    let app = "<template><screen>\n\
        <btn kind=\"big\" :label=\"n\" />\n\
        <btn kind=\"primary\" label=\"ok\" />\n\
        </screen></template>\n\
        <script>\nuse components::btn;\nlet n = signal(0);\n</script>\n";
    let dir = project(&[("app.rux", app), ("components/btn.rux", comp)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let f = found(&doc);
    assert!(f.iter().any(|(l, e, m)| *l == Some(2) && *e && m.contains("`kind` on <btn>: \"big\" is not")), "{f:?}");
    assert!(f.iter().any(|(l, e, m)| *l == Some(2) && *e && m.contains("`:label` on <btn>: this is `int`")), "{f:?}");
    assert!(!f.iter().any(|(l, _, _)| *l == Some(3)), "the second tag is right: {f:?}");
}

/// An event is typed by what hands it over: a swipe's direction is one of
/// four, and a form's values are its fields while its errors may lack any.
#[test]
fn an_event_has_the_type_of_what_sent_it() {
    let app = "<template><screen>\n\
        <view @swipe=\"if event.direction == &quot;lefty&quot; { n = 1; }\"><text>x</text></view>\n\
        <view role=\"form\" @submit=\"said = event.values.email + event.values.emial\" @invalid=\"said = event.errors.email\">\n\
        <input r-model=\"email\" name=\"email\" required />\n\
        </view>\n\
        </screen></template>\n\
        <script>\nlet n = signal(0);\nlet email = signal(\"\");\nlet said = signal(\"\");\n</script>\n";
    let dir = project(&[("app.rux", app)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let f = found(&doc);
    assert!(f.iter().any(|(l, e, m)| *l == Some(2) && *e && m.contains("did you mean \"left\"")), "{f:?}");
    assert!(f.iter().any(|(l, e, m)| *l == Some(3) && *e && m.contains("no field `emial`; did you mean `email`")), "{f:?}");
    assert!(!f.iter().any(|(_, _, m)| m.contains("`values`") && m.contains("`email`") && !m.contains("emial")), "values.email reads plainly: {f:?}");
    assert!(f.iter().any(|(l, e, m)| *l == Some(3) && *e && m.contains("`@invalid` on <view>") && m.contains("event.errors?.email")), "{f:?}");
}
