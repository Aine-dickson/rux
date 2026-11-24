//! A page's `<image src>` is read from its app's folder, where it runs, also
//! when `rux check` reads the page on its own (Urbandrop's small notes).

use rux_runtime::Document;

/// A valid 1×1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49,
    0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00,
    0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

fn image_size(node: &rux_layout::Node) -> Option<(f32, f32)> {
    node.image.as_ref().map(|i| i.intrinsic).or_else(|| node.children.iter().find_map(image_size))
}

#[test]
fn a_page_read_on_its_own_reads_its_image_from_the_apps_folder() {
    let dir = std::env::temp_dir().join(format!("rux_image_base_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("pages")).unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("assets/logo.png"), PNG).unwrap();
    std::fs::write(
        dir.join("app.rux"),
        "<template><screen><router><route path=\"/\" view=\"home\" /></router></screen></template>\n<script>\nuse pages::home;\n</script>",
    )
    .unwrap();
    std::fs::write(dir.join("pages/home.rux"), "<template><view><image src=\"assets/logo.png\" /></view></template>").unwrap();

    // As `rux check` reads a file something uses.
    rux_runtime::set_may_have_caller(true);
    let page = Document::load(dir.join("pages/home.rux"));
    rux_runtime::set_may_have_caller(false);
    let page = page.expect("loads");
    assert_eq!(image_size(&page.root), Some((1.0, 1.0)));

    // And through its app, as it runs.
    let app = Document::load(dir.join("app.rux")).expect("loads");
    assert_eq!(image_size(&app.root), Some((1.0, 1.0)));
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::RgbaImage::new(w, h).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

/// A picture's size is read once and kept, since a patch resolves every image
/// of a fresh tree each frame (64 map tiles were read from disk every frame).
/// It stays right as the tree is patched, and a load reads the file again.
#[test]
fn an_images_size_is_kept_across_patches_and_read_again_on_load() {
    let dir = std::env::temp_dir().join(format!("rux_image_sizes_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tile.png"), png(3, 2)).unwrap();
    std::fs::write(
        dir.join("app.rux"),
        "<template><screen><view :style=\"`width: ${w}px`\"><image src=\"tile.png\" /></view><text>{{ w }}</text></screen></template>\n\
         <script>\nlet w = signal(10);\n</script>",
    )
    .unwrap();
    let mut doc = Document::load(dir.join("app.rux")).expect("loads");
    assert_eq!(image_size(&doc.root), Some((3.0, 2.0)));
    assert!(doc.apply_handler("w = 20"));
    assert!(doc.apply_handler("w = 30"));
    assert_eq!(image_size(&doc.root), Some((3.0, 2.0)), "kept through patches");
    // The file changes, as an author saving a new picture does: the next load
    // is the hot reload, and it reads the file again.
    std::fs::write(dir.join("tile.png"), png(5, 4)).unwrap();
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    assert_eq!(image_size(&doc.root), Some((5.0, 4.0)));
}
