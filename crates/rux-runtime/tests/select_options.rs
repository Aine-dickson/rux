//! A select's options with a value and a label (Urbandrop's UD-013).
//!
//! `:options="[{ value: 60, label: `1 hour` }, ...]"`: the model holds the
//! value, the field shows the label, a disabled option cannot be picked.

use rux_layout::Node;
use rux_runtime::Document;

fn document(options: &str, model: &str) -> Document {
    Document::from_source(&format!(
        "<template><screen>\
           <input type=\"select\" r-model=\"pick\" :options=\"{options}\" />\
         </screen></template>\n\
         <script>\nlet pick = signal({model});\nlet choices = signal([\"a\", \"b\"]);\n</script>"
    ))
    .expect("loads")
}

fn select(doc: &Document) -> &Node {
    &doc.root.children[0]
}

fn shown(doc: &Document) -> String {
    select(doc).children[0].text.as_ref().expect("the shown text").text.clone()
}

#[test]
fn options_with_labels_hold_values_and_show_labels() {
    let doc = document(
        "[{ value: 60, label: `1 hour` }, { value: 120, label: `2 hours` }, { value: 240, label: `4 hours`, disabled: true }]",
        "60",
    );
    assert!(rux_runtime::take_warnings().is_empty());
    let node = select(&doc);
    assert_eq!(node.options.as_deref(), Some(&["60".to_string(), "120".to_string(), "240".to_string()][..]));
    let meta = node.option_meta.as_ref().expect("maps carry labels");
    assert_eq!(meta.labels, ["1 hour", "2 hours", "4 hours"]);
    assert_eq!(meta.disabled, [false, false, true]);
    assert!(meta.numeric, "every value is a number");
    assert_eq!(shown(&doc), "1 hour", "the field shows the label of what the model holds");
}

#[test]
fn the_shown_label_follows_the_model() {
    let mut doc = document("[{ value: 60, label: `1 hour` }, { value: 120, label: `2 hours` }]", "60");
    doc.apply_value_in("pick", None, None, &rux_reactive::Value::Number(120.0));
    assert_eq!(shown(&doc), "2 hours");
}

#[test]
fn plain_options_are_as_they_were() {
    let doc = document("choices", "\"b\"");
    let node = select(&doc);
    assert_eq!(node.options.as_deref(), Some(&["a".to_string(), "b".to_string()][..]));
    assert!(node.option_meta.is_none(), "text options need no labels");
    assert_eq!(shown(&doc), "b");
}

#[test]
fn text_values_with_labels_are_not_numeric() {
    let doc = document("[{ value: `am`, label: `Morning` }, { value: `pm`, label: `Evening` }]", "\"pm\"");
    let meta = select(&doc).option_meta.clone().expect("labels");
    assert!(!meta.numeric);
    assert_eq!(shown(&doc), "Evening");
}

#[test]
fn a_map_without_a_label_shows_its_value() {
    let doc = document("[{ value: `x` }, { value: `y`, disabled: false }]", "\"x\"");
    let meta = select(&doc).option_meta.clone().expect("a map is meta");
    assert_eq!(meta.labels, ["x", "y"]);
    assert_eq!(shown(&doc), "x");
}
