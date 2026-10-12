//! Every example's layout, written out whole, so a change under the layout
//! (a Taffy upgrade, track (g).4a of `docs/11-next.md`) can be diffed against
//! the build before it, box by box. Ignored; run it with a directory:
//!
//! `LAYOUT_DUMP=some/dir cargo test -p rux-runtime --test layout_dump -- --ignored`
//!
//! Text is measured by a stub that wraps greedily by word, so the dump depends
//! on the layout alone and not on the fonts installed.

use std::path::PathBuf;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn measure(tc: &rux_layout::TextContent, max: Option<f32>) -> (f32, f32) {
    let char_w = tc.font_size * 0.5;
    let line_h = tc.line_height.unwrap_or(tc.font_size * 1.25);
    let width = |s: &str| s.chars().count() as f32 * char_w;
    let Some(max) = max.filter(|_| !tc.nowrap) else {
        return (width(&tc.text), line_h);
    };
    let (mut lines, mut line, mut widest) = (1.0, 0.0f32, 0.0f32);
    for word in tc.text.split(' ') {
        let w = width(word);
        let next = if line == 0.0 { w } else { line + char_w + w };
        if next > max && line > 0.0 {
            lines += 1.0;
            widest = widest.max(line);
            line = w;
        } else {
            line = next;
        }
    }
    (widest.max(line), lines * line_h)
}

#[test]
#[ignore]
fn dump_every_example_layout() {
    let Some(out) = std::env::var_os("LAYOUT_DUMP").map(PathBuf::from) else {
        panic!("set LAYOUT_DUMP to the directory to write into");
    };
    std::fs::create_dir_all(&out).expect("dump directory");
    let mut files: Vec<PathBuf> = std::fs::read_dir(examples_dir())
        .expect("examples")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "rux"))
        .collect();
    files.sort();
    let mut written = 0;
    for file in files {
        let name = file.file_stem().unwrap().to_string_lossy().to_string();
        let Ok(doc) = rux_runtime::Document::load(&file) else {
            eprintln!("skipped {name}: does not load here");
            continue;
        };
        for (w, h) in [(1000.0, 800.0), (360.0, 640.0)] {
            let layout = rux_layout::layout(&doc.root, w, h, &mut measure);
            std::fs::write(out.join(format!("{name}-{w}x{h}.txt")), format!("{layout:#?}"))
                .expect("write dump");
            written += 1;
        }
    }
    eprintln!("wrote {written} layouts to {}", out.display());
}
