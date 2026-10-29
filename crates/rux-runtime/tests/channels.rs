//! `rux::Channel`: every value Rust sends reaches script, in order (Tracer's
//! AW-011), where a `rux::Signal` keeps only the newest.

use rux_runtime::native as rn;
use rux_runtime::Document;

static EVENTS: rn::Channel<i64> = rn::Channel::new();

fn project(files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rux_channels_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, body) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    dir
}

fn texts(n: &rux_layout::Node) -> String {
    n.text.iter().map(|t| t.text.clone()).chain(n.children.iter().map(texts)).collect()
}

#[test]
fn every_value_pushed_reaches_an_effect_in_order() {
    rn::Module::new("feed").export(rn::Export::channel("events", "int[]", || EVENTS.__take())).install();
    // In a routed page, as Tracer's are, with its own state.
    let page = "<template><view><text>got {{ log.length }} last {{ last }} ordered {{ ordered }}</text></view></template>\n\
<script>\nuse native::feed;\nlet log: int[] = signal([]);\nlet last = signal(-1);\nlet ordered = signal(true);\n\
effect { for e in feed.events { if e != last + 1 { ordered = false; } last = e; log.push(e); } }\n</script>";
    let app = "<template><screen><router><route path=\"/\" view=\"page\" /></router></screen></template>\n<script>\nuse pages::page;\n</script>";
    let dir = project(&[("app.rux", app), ("pages/page.rux", page)]);
    let mut doc = Document::load(dir.join("app.rux")).expect("loads");
    assert!(texts(&doc.root).contains("got 0"), "{}", texts(&doc.root));

    // A burst with no gap: all of it, in one update.
    for i in 0..1000 {
        EVENTS.push(i);
    }
    assert!(doc.refresh_native_signals());
    assert!(texts(&doc.root).contains("got 1000 last 999 ordered true"), "{}", texts(&doc.root));
    assert!(!doc.refresh_native_signals(), "nothing pushed since");

    // One update per look, however the values came.
    EVENTS.push(1000);
    assert!(doc.refresh_native_signals());
    EVENTS.extend([1001, 1002]);
    assert!(doc.refresh_native_signals());
    assert!(texts(&doc.root).contains("got 1003 last 1002 ordered true"), "{}", texts(&doc.root));

    // Two lists alike are still two arrivals.
    EVENTS.push(7);
    assert!(doc.refresh_native_signals());
    EVENTS.push(7);
    assert!(doc.refresh_native_signals());
    assert!(texts(&doc.root).contains("got 1005 last 7"), "{}", texts(&doc.root));
}
