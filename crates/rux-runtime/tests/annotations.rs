//! Type annotations, parsed and erased: the first step of the type system in
//! `docs/10-types.md`. Every form an author can write is accepted, a document
//! runs the same with its annotations as without them, and the places that are
//! read outside the fork (`prop`, `computed`, `use`) take them too.

use std::fs;
use std::path::PathBuf;

use rux_runtime::Document;

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A throwaway project, numbered for the reason `props.rs` gives.
fn project(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rux_annotations_{}_{}",
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

/// The error a load that must fail gives.
fn load_err(path: PathBuf) -> String {
    match Document::load(path) {
        Ok(_) => panic!("loaded, and should not have"),
        Err(e) => e,
    }
}

fn problems(doc: &Document) -> Vec<String> {
    doc.diagnostics().warnings.iter().map(|w| w.message.clone()).collect()
}

fn texts(node: &rux_layout::Node, out: &mut Vec<String>) {
    if let Some(t) = &node.text {
        out.push(t.text.clone());
    }
    for child in &node.children {
        texts(child, out);
    }
}

fn shown(doc: &Document) -> Vec<String> {
    let mut out = Vec::new();
    texts(&doc.root, &mut out);
    out
}

const TYPES: &str = "<script>\n\
    type Task = { id: int, title: string, done: bool, note?: string };\n\
    type Filter =\n  | \"all\"\n  | \"open\"\n  | \"done\";\n\
    </script>\n";

/// A whole document written with annotations everywhere they can go runs, and
/// shows what the same document without them would.
#[test]
fn an_annotated_document_runs_as_if_unannotated() {
    let app = "<template><screen>\n\
        <text>{{ total }}/{{ visible().length }}</text>\n\
        <text r-for=\"t in visible()\">{{ label(t, 1) }}</text>\n\
        </screen></template>\n\
        <script>\n\
        use types::Task;\n\
        use types::Filter;\n\
        type Pair = { a: int, b: int };\n\
        let tasks: Task[] = signal([{ id: 1, title: \"a\", done: false }, { id: 2, title: \"b\", done: true }]);\n\
        let filter: Filter = signal(\"open\");\n\
        computed total: int = tasks.length;\n\
        fn visible(): Task[] {\n\
          switch filter {\n\
            \"all\" => tasks,\n\
            \"open\" => tasks.filter((t: Task) => !t.done),\n\
            \"done\" => tasks.filter(t => t.done),\n\
          }\n\
        }\n\
        fn label(t: Task, n: int): string { `${n}:${t.title}` }\n\
        </script>\n";
    let dir = project(&[("app.rux", app), ("types.rux", TYPES)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    assert!(problems(&doc).is_empty(), "{:?}", problems(&doc));
    let shown = shown(&doc);
    assert!(shown.contains(&"2/1".to_string()), "{shown:?}");
    assert!(shown.contains(&"1:a".to_string()), "{shown:?}");
}

/// `prop name: T` and `prop name: T = default` are declarations like any other:
/// the value arrives, the default fills one left off, and a type with commas,
/// braces or `=>` in it does not split the line in the wrong place.
#[test]
fn a_typed_prop_is_declared() {
    let stat = "<template><view><text>{{ label }}={{ value }}:{{ shape.b }}</text></view></template>\n\
        <script>\n\
          prop label: string;\n\
          prop value: float = 0;\n\
          prop shape: { a: int, b: int } = { a: 1, b: 2 };\n\
          prop pick: (string, int) => bool = null;\n\
          prop kind: \"primary\" | \"quiet\" = \"quiet\";\n\
        </script>";
    let app = "<template><screen><stat :label='\"x\"' :value=\"3\" /></screen></template>\n\
        <script>\nuse components::stat;\n</script>";
    let dir = project(&[("app.rux", app), ("components/stat.rux", stat)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let errors: Vec<_> = doc.diagnostics().warnings.iter().filter(|w| w.is_error()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(shown(&doc).contains(&"x=3:2".to_string()), "{:?}", shown(&doc));
}

/// A prop's type that is not a type is an error on its line, in the words
/// the type parser uses, and names the prop.
#[test]
fn a_prop_whose_type_is_not_a_type_is_an_error() {
    let stat = "<template><view><text>x</text></view></template>\n\
        <script>\n  prop a: Array<int>;\n  prop b: ;\n  prop c: { x: int\n</script>";
    let app = "<template><screen><stat /></screen></template>\n\
        <script>\nuse components::stat;\n</script>";
    let dir = project(&[("app.rux", app), ("components/stat.rux", stat)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let errors: Vec<String> =
        doc.diagnostics().warnings.iter().filter(|w| w.is_error()).map(|w| w.message.clone()).collect();
    assert!(errors.iter().any(|e| e.contains("prop a: Array<int>") && e.contains("`T[]`")), "{errors:?}");
    assert!(errors.iter().any(|e| e.contains("`prop b:` has no type")), "{errors:?}");
    assert!(errors.iter().any(|e| e.contains("prop c")), "{errors:?}");
}

/// A computed in a component, typed, runs per instance as an untyped one does.
#[test]
fn a_typed_computed_in_a_component() {
    let stat = "<template><view><text>{{ twice }}</text></view></template>\n\
        <script>\n  prop value: float;\n  computed twice: float = value * 2;\n</script>";
    let app = "<template><screen><stat :value=\"4\" /></screen></template>\n\
        <script>\nuse components::stat;\n</script>";
    let dir = project(&[("app.rux", app), ("components/stat.rux", stat)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    assert!(shown(&doc).contains(&"8".to_string()), "{:?} {:?}", shown(&doc), problems(&doc));
}

/// `use types::Task` names `types.rux`; a type import is never taken for a
/// component, and one whose file is not there says where it looked.
#[test]
fn a_type_import_names_a_file() {
    let app = |line: &str| {
        format!("<template><screen><text>x</text></screen></template>\n<script>\n{line}\n</script>")
    };
    let dir = project(&[("app.rux", &app("use types::Task;")), ("types.rux", TYPES)]);
    assert!(Document::load(dir.join("app.rux")).is_ok());

    let dir = project(&[("app.rux", &app("use shapes::Task;"))]);
    let err = load_err(dir.join("app.rux"));
    assert!(err.contains("no file for `use shapes::Task;`"), "{err}");

    let dir = project(&[("app.rux", &app("use Task;"))]);
    let err = load_err(dir.join("app.rux"));
    assert!(err.contains("names no file") && err.contains("use types::Task;"), "{err}");

    // A component file's own types: `use components::row::Row`.
    let row = "<template><view><text>r</text></view></template>\n\
        <script>\n  type Row = { id: int };\n</script>";
    let dir = project(&[("app.rux", &app("use components::row::Row;")), ("components/row.rux", row)]);
    assert!(Document::load(dir.join("app.rux")).is_ok());
}

/// A file with only a `<script>` is a types file: it loads when it declares
/// types and nothing else, and says so when it declares something that would
/// never run.
#[test]
fn a_types_file_holds_types_and_nothing_else() {
    let dir = project(&[("types.rux", TYPES)]);
    assert!(Document::load(dir.join("types.rux")).is_ok());

    let dir = project(&[("types.rux", "<script>\n  type A = int;\n  fn f() { 1 }\n</script>")]);
    let err = load_err(dir.join("types.rux"));
    assert!(err.contains("no <template>") && err.contains("a function"), "{err}");

    let dir = project(&[("types.rux", "<script>\n  type A = int;\n  let n = 1;\n</script>")]);
    let err = load_err(dir.join("types.rux"));
    assert!(err.contains("a statement"), "{err}");
}

/// A malformed annotation in a script is a syntax error with a line, the same
/// as any other.
#[test]
fn a_malformed_annotation_in_a_script_is_placed() {
    let app = "<template><screen><text>x</text></screen></template>\n\
        <script>\nlet a = 1;\nlet n: = 2;\n</script>";
    let dir = project(&[("app.rux", app)]);
    let Err(err) = Document::load_checked(dir.join("app.rux")) else { panic!("loaded") };
    // Rux's parser says it now (docs/11-next.md, step 2), in its own words.
    assert!(err.to_string().contains("expecting a type after `:`"), "{err}");
    assert_eq!(err.line, Some(4), "{err}");
}
