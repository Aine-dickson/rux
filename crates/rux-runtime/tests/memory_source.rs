//! Loading a whole document without a filesystem.
//!
//! This is the test the source provider exists for. An Android release build has
//! no paths to read and a browser has no filesystem at all, so the question is
//! not whether one file can be read from memory but whether a document's *whole*
//! graph can: the document, a component it `use`s, and a stylesheet it includes,
//! each of which was a separate `std::fs` call before.
//!
//! If this passes, nothing in the load path is reaching past the provider.

use std::rc::Rc;

use rux_runtime::{set_source, Document, MemorySource};

/// Put the filesystem back even if the test fails, so a panic here cannot make
/// an unrelated test on the same thread read from a map it never heard of.
struct Restore(Option<Rc<dyn rux_runtime::Source>>);

impl Drop for Restore {
    fn drop(&mut self) {
        if let Some(previous) = self.0.take() {
            set_source(previous);
        }
    }
}

fn install(source: MemorySource) -> Restore {
    Restore(Some(set_source(Rc::new(source))))
}

/// `Document` is not `Debug`, so `expect_err` is not available on the result.
fn load_err(msg: &str) -> String {
    match Document::load("app.rux") {
        Ok(_) => panic!("{msg}"),
        Err(e) => e,
    }
}

fn collect_image_srcs(node: &rux_layout::Node, out: &mut Vec<String>) {
    if let Some(image) = &node.image {
        out.push(image.src.clone());
    }
    for child in &node.children {
        collect_image_srcs(child, out);
    }
}

fn collect_text(node: &rux_layout::Node, out: &mut Vec<String>) {
    if let Some(text) = &node.text {
        out.push(text.text.clone());
    }
    for child in &node.children {
        collect_text(child, out);
    }
}

#[test]
fn a_document_its_component_and_its_stylesheet_load_from_memory() {
    let _restore = install(
        MemorySource::new()
            .with(
                "app.rux",
                r#"<template>
  <screen>
    <greeting />
  </screen>
</template>
<style src="theme.css"></style>
<script>
use components::greeting;
let who = signal("memory");
</script>"#,
            )
            .with(
                "components/greeting.rux",
                r#"<template>
  <text class="hello">hello</text>
</template>"#,
            )
            .with("theme.css", ".hello { color: #ff0000; }"),
    );

    let doc = Document::load("app.rux").expect("the document should load with no filesystem");

    // The component expanded: its text is in the tree, which it could only be if
    // `components/greeting.rux` was read through the provider.
    let mut texts = Vec::new();
    collect_text(&doc.root, &mut texts);
    assert!(
        texts.iter().any(|t| t == "hello"),
        "the component did not expand: {texts:?}"
    );
}

/// The painter reads image bytes through a hook the runtime installs, because
/// `image::open` on a path is exactly what an embedded build cannot do. This
/// asserts at that seam: after a load, the bytes for an `<image src>` are
/// reachable without touching a disk.
///
/// A real PNG is not needed. The hook's job is to hand over whatever the
/// provider holds; decoding is `image`'s business and is tested by drawing.
#[test]
fn the_painter_can_reach_an_images_bytes_without_a_filesystem() {
    let _restore = install(
        MemorySource::new()
            .with(
                "app.rux",
                r#"<template>
  <screen>
    <image src="assets/logo.png" />
  </screen>
</template>"#,
            )
            .with("assets/logo.png", b"not-a-real-png".to_vec()),
    );

    let doc = Document::load("app.rux").expect("loads");

    // The tree holds the resolved src, which is what the painter is handed.
    let mut srcs = Vec::new();
    collect_image_srcs(&doc.root, &mut srcs);
    let src = srcs.first().expect("the document has an image");

    let bytes = rux_layout::read_image_bytes(src)
        .expect("the painter should reach the bytes through the installed reader");
    assert_eq!(bytes, b"not-a-real-png");
}

#[test]
fn a_missing_component_still_fails_the_way_it_always_did() {
    // The provider must not turn a real mistake into a silent success. An import
    // naming a file that is in no map is the same failure as one naming a file
    // that is on no disk.
    let _restore = install(MemorySource::new().with(
        "app.rux",
        r#"<template>
  <screen>
    <missing />
  </screen>
</template>
<script>
use components::missing;
</script>"#,
    ));

    let err = load_err("a missing component should not load");
    assert!(
        err.contains("missing"),
        "the failure should name the component that is not there: {err}"
    );
}

#[test]
fn a_missing_stylesheet_is_still_an_error() {
    let _restore = install(MemorySource::new().with(
        "app.rux",
        r#"<template><screen /></template>
<style src="gone.css"></style>"#,
    ));

    let err = load_err("a missing stylesheet should not load");
    assert!(
        err.contains("gone.css"),
        "the failure should name the stylesheet: {err}"
    );
}
