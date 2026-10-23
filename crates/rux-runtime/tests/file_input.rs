//! `<input type="file">`: what the build makes of it, what the layout hands
//! the shell, and what the checks say. The picker itself is the platform's,
//! driven by hand (see `docs/08-user-tests.md`).

use rux_layout::{AccessRole, Background, FocusKind, Node, Rgba, Shape};
use rux_reactive::Value;
use rux_runtime::Document;

fn doc(template: &str, script: &str, style: &str) -> Document {
    Document::from_source(&format!(
        "<template><screen>{template}</screen></template>\n<script>\n{script}</script>\n<style>{style}</style>"
    ))
    .expect("loads")
}

fn find<'a>(node: &'a Node, pred: &dyn Fn(&Node) -> bool) -> Option<&'a Node> {
    if pred(node) {
        return Some(node);
    }
    node.children.iter().find_map(|c| find(c, pred))
}

fn input<'a>(doc: &'a Document, model: &str) -> &'a Node {
    find(&doc.root, &|n| n.model.as_deref() == Some(model)).expect("the input is built")
}

/// The button's words and the name beside it.
fn shown(doc: &Document, model: &str) -> (String, String) {
    let words = |n: &Node| n.text.as_ref().expect("a text child").text.clone();
    let kids = &input(doc, model).children;
    (words(&kids[0]), words(&kids[1]))
}

fn problems(doc: &Document) -> String {
    format!("{:?}", doc.diagnostics()).replace('\\', "")
}

fn a_file(name: &str) -> Value {
    Value::Map(vec![
        ("name".to_string(), Value::Text(name.to_string())),
        ("size".to_string(), Value::Number(12.0)),
        ("type".to_string(), Value::Text("image/png".to_string())),
        ("url".to_string(), Value::Text(format!("file:///tmp/{name}#0"))),
        ("path".to_string(), Value::Text(format!("/tmp/{name}"))),
    ])
}

#[test]
fn a_file_input_is_a_button_and_a_name() {
    let mut d = doc(
        r#"<input type="file" r-model="photo" accept="image/*, .pdf" capture="environment" @change="n += 1" @cancel="c += 1" />"#,
        "let photo: File? = signal(none);\nlet n = signal(0);\nlet c = signal(0);",
        "",
    );
    assert!(!problems(&d).contains("level: Error"), "{}", problems(&d));
    assert_eq!(shown(&d, "photo"), ("Choose file".to_string(), "No file chosen".to_string()));
    let node = input(&d, "photo");
    let file = node.field().file.clone().expect("a file input");
    assert_eq!(file.accept, "image/*, .pdf");
    assert!(!file.multiple);
    assert_eq!(file.capture.as_deref(), Some("environment"));
    assert!(file.on_cancel.is_some() && node.field().on_change.is_some());
    assert_eq!(node.field().shape, Shape::File);
    assert_eq!(node.access.role, AccessRole::Button);

    // What the shell writes after a pick, shown as the file's name.
    d.apply_value_in("photo", None, None, &a_file("cat.png"));
    assert_eq!(shown(&d, "photo").1, "cat.png");
    assert_eq!(input(&d, "photo").access.value.as_deref(), Some("cat.png"));
}

#[test]
fn several_files_are_counted() {
    let mut d = doc(r#"<input type="file" r-model="docs" multiple />"#, "let docs: File[] = signal([]);", "");
    assert!(!problems(&d).contains("level: Error"), "{}", problems(&d));
    assert_eq!(shown(&d, "docs"), ("Choose files".to_string(), "No file chosen".to_string()));
    d.apply_value_in("docs", None, None, &Value::List(vec![a_file("a.png")]));
    assert_eq!(shown(&d, "docs").1, "a.png");
    d.apply_value_in("docs", None, None, &Value::List(vec![a_file("a.png"), a_file("b.png"), a_file("c.png")]));
    assert_eq!(shown(&d, "docs").1, "3 files");
}

/// The layout hands the shell a region to open a picker from, and Tab a
/// stop that opens it too, never a caret.
#[test]
fn the_layout_makes_a_file_region_not_a_text_field() {
    let d = doc(r#"<input type="file" r-model="photo" />"#, "let photo: File? = signal(none);", "");
    let layout = rux_layout::layout(&d.root, 400.0, 800.0, &mut |_, _| (10.0, 10.0));
    assert_eq!(layout.files.len(), 1);
    assert_eq!(layout.files[0].model, "photo");
    assert!(layout.focuses.is_empty(), "no caret: {:?}", layout.focuses.len());
    assert!(layout.focusables.iter().any(|f| matches!(&f.kind, FocusKind::File { model, .. } if model == "photo")));

    // Disabled: nothing to tap, nothing for Tab.
    let d = doc(r#"<input type="file" r-model="photo" disabled />"#, "let photo: File? = signal(none);", "");
    let layout = rux_layout::layout(&d.root, 400.0, 800.0, &mut |_, _| (10.0, 10.0));
    assert!(layout.files.is_empty());
}

/// `::file-selector-button` styles the button and not the input, and only
/// the input it is written for.
#[test]
fn file_selector_button_styles_the_button() {
    let d = doc(
        r#"<input type="file" class="f" r-model="a" /><input type="file" r-model="b" />"#,
        "let a: File? = signal(none);\nlet b: File? = signal(none);",
        ".f::file-selector-button { background: #ff0000; } .f { background: #00ff00; }",
    );
    assert!(!problems(&d).contains("pseudo-element"), "{}", problems(&d));
    let bg = |n: &Node| match &n.style.background {
        Some(Background::Color(Rgba { r, g, b, .. })) => Some([*r, *g, *b]),
        _ => None,
    };
    let styled = input(&d, "a");
    assert_eq!(bg(&styled.children[0]), Some([1.0, 0.0, 0.0]), "the rule's button");
    assert_eq!(bg(styled), Some([0.0, 1.0, 0.0]), "the input keeps its own");
    let plain = bg(&input(&d, "b").children[0]).expect("the default look has a background");
    assert_ne!(plain, [1.0, 0.0, 0.0]);
}

/// `required` is not met by nothing, nor by an empty list.
#[test]
fn a_required_file_needs_a_file() {
    let style = ".f:invalid { background: #ff0000; } .f:valid { background: #00ff00; }";
    let red = |d: &Document, m: &str| {
        matches!(&input(d, m).style.background, Some(Background::Color(Rgba { r, g, .. })) if *r == 1.0 && *g == 0.0)
    };
    let mut d = doc(
        r#"<input type="file" class="f" r-model="one" required /><input type="file" class="f" r-model="many" multiple required />"#,
        "let one: File? = signal(none);\nlet many: File[] = signal([]);",
        style,
    );
    assert!(red(&d, "one") && red(&d, "many"));
    d.apply_value_in("one", None, None, &a_file("a.png"));
    d.apply_value_in("many", None, None, &Value::List(vec![a_file("a.png")]));
    assert!(!red(&d, "one") && !red(&d, "many"));
}

/// The checker holds the model to `File?` (or `File[]`), knows the event's
/// value, and `@cancel` belongs to a file input alone.
#[test]
fn a_file_input_is_checked() {
    let d = doc(r#"<input type="file" r-model="name" />"#, "let name = signal(\"\");", "");
    assert!(problems(&d).contains("level: Error"), "a string is not a file: {}", problems(&d));

    let d = doc(
        r#"<input type="file" r-model="f" @change="n = event.value.size" />"#,
        "let f: File? = signal(none);\nlet n = signal(0);",
        "",
    );
    assert!(!problems(&d).contains("level: Error"), "{}", problems(&d));

    let d = doc(r#"<input r-model="t" @cancel="t = ''" />"#, "let t = signal(\"\");", "");
    assert!(problems(&d).contains("belongs on an `<input type=\"file\">`"), "{}", problems(&d));
}

/// A picked file's `blob:` URL (the web's) or `content://` one (Android's)
/// is drawn from the bytes the shell remembered for it, for its size and
/// for the painter alike, in a document loaded from source as the web's is.
#[test]
fn a_remembered_url_is_an_image() {
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(5, 3, image::Rgba([0, 255, 0, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let url = "blob:http://127.0.0.1/6f1c";
    rux_runtime::remember_url_bytes(url, png);
    let d = doc(&format!(r#"<image src="{url}" />"#), "", "");
    let img = d.root.children[0].image.as_ref().expect("an image");
    assert_eq!(img.intrinsic, (5.0, 3.0));
    assert_eq!(img.src, url, "kept as written");
    assert!(rux_layout::read_image_bytes(url).is_some(), "the painter reads it too");
}
