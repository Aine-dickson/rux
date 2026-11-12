//! Slot content is the caller's markup: its tags mean what the caller's `use`
//! says and its expressions read the caller's names, however deep the
//! `<slot />` placing it is (Urbandrop's UD-009).

use rux_runtime::Document;

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn project(files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rux_slot_scope_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, body) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    dir
}

fn texts(node: &rux_layout::Node, out: &mut Vec<String>) {
    if let Some(t) = &node.text {
        out.push(t.text.clone());
    }
    for child in &node.children {
        texts(child, out);
    }
}

fn shown(doc: &Document) -> String {
    let mut out = Vec::new();
    texts(&doc.root, &mut out);
    out.join("|")
}

fn errors(doc: &Document) -> Vec<String> {
    doc.diagnostics().warnings.iter().filter(|w| w.is_error()).map(|w| w.message.clone()).collect()
}

const CARD: &str = "<template><view><text>card</text><slot /></view></template>\n<script>\n  prop flat = false;\n</script>";
const FIELDS: &str = "<template><view><text>fields</text></view></template>\n<script>\n</script>";

fn load(kind: &str) -> Document {
    let page = format!(
        "<template><view><record-card><view r-if='editKind == \"owner\"'><owner-fields /></view>\
<text>kind [{{{{ editKind }}}}]</text></record-card></view></template>\n\
<script>\nuse components::record_card;\nuse components::owner_fields;\nlet editKind = signal(\"{kind}\");\n</script>"
    );
    let app = "<template><screen><router><route path=\"/\" view=\"page\" /></router></screen></template>\n\
<script>\nuse pages::page;\n</script>";
    let dir = project(&[
        ("app.rux", app),
        ("pages/page.rux", &page),
        ("components/record_card.rux", CARD),
        ("components/owner_fields.rux", FIELDS),
    ]);
    Document::load(dir.join("app.rux")).expect("loads")
}

#[test]
fn a_slots_r_if_reads_the_callers_state() {
    let doc = load("");
    assert!(errors(&doc).is_empty(), "{:?}", errors(&doc));
    assert_eq!(shown(&doc), "card|kind []");
}

#[test]
fn a_slots_tags_are_the_callers_imports() {
    let doc = load("owner");
    assert!(errors(&doc).is_empty(), "{:?}", errors(&doc));
    assert_eq!(shown(&doc), "card|fields|kind [owner]");
}

/// A tap in the content runs in the caller's instance, and the content's
/// `r-if` follows what it wrote.
#[test]
fn a_tap_in_a_slot_moves_the_callers_state_and_its_r_if_follows() {
    let page = "<template><view><record-card><view @tap='editKind = \"owner\"'><text>edit</text></view><view r-if='editKind == \"owner\"'><owner-fields /></view></record-card></view></template>
<script>
use components::record_card;
use components::owner_fields;
let editKind = signal(\"\");
</script>";
    let app = "<template><screen><router><route path=\"/\" view=\"page\" /></router></screen></template>
<script>
use pages::page;
</script>";
    let dir = project(&[
        ("app.rux", app),
        ("pages/page.rux", page),
        ("components/record_card.rux", CARD),
        ("components/owner_fields.rux", FIELDS),
    ]);
    let mut doc = Document::load(dir.join("app.rux")).expect("loads");
    assert_eq!(shown(&doc), "card|edit");
    fn tappable(n: &rux_layout::Node) -> Option<&rux_layout::Node> {
        if n.on_tap.is_some() {
            return Some(n);
        }
        n.children.iter().find_map(tappable)
    }
    let button = tappable(&doc.root).expect("the edit button").clone();
    assert!(doc.apply_handler_in(&button.on_tap.clone().unwrap(), button.instance.as_deref()));
    assert!(errors(&doc).is_empty(), "{:?}", errors(&doc));
    assert_eq!(shown(&doc), "card|edit|fields");
}
