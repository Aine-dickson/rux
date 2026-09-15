//! How a `<route>` names its view, and what it says when the name is wrong.
//!
//! A view is named the way its tag is, in kebab, while the `use` that brings it
//! in names the file, in snake. Two spellings for one component is a place an
//! author will land, so the two ways of landing there both have to say
//! something true.

use std::fs;
use std::path::PathBuf;

use rux_runtime::Document;

/// Write a little app into a scratch directory and load it.
///
/// The `use` lines are given explicitly, because whether a component is
/// imported is the thing under test.
fn app(components: &[(&str, &str)], template: &str, script: &str) -> Document {
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "rux_views_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(dir.join("components")).unwrap();
    for (name, body) in components {
        fs::write(dir.join("components").join(format!("{name}.rux")), body).unwrap();
    }
    fs::write(
        dir.join("app.rux"),
        format!(
            "<template><screen>{template}</screen></template>\n<script>\n{script}</script>"
        ),
    )
    .unwrap();
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let _ = fs::remove_dir_all(&dir);
    doc
}

const PAGE: &str = r#"<template><view class="page"><text>page</text></view></template>"#;

fn problems(doc: &Document) -> String {
    // Unescaped: the Debug form doubles every quote in the message, and these
    // messages are about the quotes.
    format!("{:?}", doc.diagnostics()).replace('\\', "")
}

/// The spelling that works, which is the control the other two are worth
/// nothing without.
#[test]
fn a_view_named_the_way_its_tag_is_renders() {
    let doc = app(
        &[("page_a", PAGE)],
        r#"<router><route path="/" view="page-a" /></router>"#,
        "use components::page_a;\n",
    );
    let said = problems(&doc);
    assert!(!said.contains("<route> names the view"), "nothing to say: {said}");
}

/// Writing the view in the import's spelling used to be told to add the import
/// it already had.
///
/// The old message was "which is not imported; add `use components::page_a;`",
/// advice the script followed verbatim two lines further down. An author who
/// does what the message says gets the identical message back.
#[test]
fn a_view_written_in_the_import_spelling_is_told_which_spelling_to_use() {
    let doc = app(
        &[("page_a", PAGE)],
        r#"<router><route path="/" view="page_a" /></router>"#,
        "use components::page_a;\n",
    );
    let said = problems(&doc);
    assert!(said.contains(r#"write `view="page-a"`"#), "names the spelling: {said}");
    assert!(
        said.contains("already in the script is what imports it"),
        "and does not ask for the import it can see: {said}"
    );
}

/// A view nobody imported is still told to import it, and told a name that is
/// a name.
///
/// The suggestion used to echo the view's own hyphens, so pasting
/// `use components::page-a;` turned a warning into a hard error looking for a
/// `page-a.rux` that no convention here would ever produce.
#[test]
fn an_unimported_view_is_suggested_an_import_that_parses() {
    let doc = app(
        &[],
        r#"<router><route path="/" view="page-a" /></router>"#,
        "let x = signal(0);\n",
    );
    let said = problems(&doc);
    assert!(said.contains("which is not imported"), "still the missing-import case: {said}");
    assert!(said.contains("use components::page_a;"), "in the file's spelling: {said}");
    assert!(!said.contains("use components::page-a;"), "never the tag's: {said}");
}
