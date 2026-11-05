//! A text field's selection seen from script, and runs styled over its text
//! (Tracer's AW-046): `el.selection`, `el.setSelectionRange`, `el.insertText`,
//! `focus()` putting a selection back, and `:marks`. What the shell does with
//! the requests (the edit, its undo, `@select`) is driven by hand; see
//! `docs/08-user-tests.md`.

use rux_layout::Node;
use rux_runtime::{Document, Focus};

/// "héllo wörld": two letters of two bytes each, so characters and bytes
/// differ from the second letter on.
const SRC: &str = r#"<template>
  <screen>
    <input id="q" type="textarea" r-model="text" :marks="marks" />
    <input id="other" r-model="other" />
    <view id="plain"><text>not a field</text></view>
  </screen>
</template>
<style>
  .kw { color: #0000ff; background: #ffff00; }
  .bad { text-decoration: underline wavy #ff0000; }
</style>
<script>
  let text = signal("héllo wörld");
  let other = signal("");
  let out = signal("");
  let marks = signal([{ start: 6, end: 11, class: "kw" }]);
</script>"#;

fn doc() -> Document {
    Document::from_source(SRC).expect("loads")
}

fn focus(caret: usize, anchor: usize) -> Focus {
    Focus { model: "text".into(), row: None, instance: None, caret, anchor, preedit: None }
}

fn out(d: &mut Document) -> String {
    d.value_in("out", None, None)
}

fn field_text(d: &Document) -> &rux_layout::TextContent {
    fn find<'a>(n: &'a Node) -> Option<&'a Node> {
        if n.model.as_deref() == Some("text") {
            return Some(n);
        }
        n.children.iter().find_map(find)
    }
    find(&d.root).and_then(|n| n.children.first()).and_then(|c| c.text.as_ref()).expect("the field's text")
}

#[test]
fn selection_is_read_in_characters_while_focused_and_after() {
    let mut d = doc();
    d.apply_handler(r##"out = `${query("#q")[0].selection}`"##);
    assert_eq!(out(&mut d), "", "a field never focused has none");
    // "hé" is three bytes, two characters; "héllo w" is eight bytes, seven.
    d.set_focus(Some(focus(3, 8)));
    d.apply_handler(r##"let s = query("#q")[0].selection; out = `${s.start} ${s.end} ${s.caret}`"##);
    assert_eq!(out(&mut d), "2 7 2");
    // Left for another field, it keeps where it was.
    d.set_focus(Some(Focus { model: "other".into(), ..focus(0, 0) }));
    d.apply_handler(r##"let s = query("#q")[0].selection; out = `${s.start} ${s.end} ${s.caret}`"##);
    assert_eq!(out(&mut d), "2 7 2");
    d.apply_handler(r##"out = `${query("#plain")[0].selection}`"##);
    assert_eq!(out(&mut d), "", "not a field");
}

#[test]
fn set_selection_range_asks_for_focus_with_that_selection_in_bytes() {
    let mut d = doc();
    d.apply_handler(r##"query("#q")[0].setSelectionRange(1, 7)"##);
    let asked = d.take_focus_request().expect("a focus").expect("not a blur");
    assert_eq!((asked.model.as_str(), asked.anchor, asked.caret), ("text", 1, 8));
    // `focus()` afterwards puts that selection back.
    d.set_focus(Some(asked));
    d.set_focus(None);
    d.apply_handler(r##"query("#q")[0].focus()"##);
    let back = d.take_focus_request().unwrap().unwrap();
    assert_eq!((back.anchor, back.caret), (1, 8));
}

#[test]
fn focus_on_a_field_never_focused_puts_the_caret_at_the_end_in_bytes() {
    let mut d = doc();
    d.apply_handler(r##"query("#q")[0].focus()"##);
    let f = d.take_focus_request().unwrap().unwrap();
    assert_eq!((f.anchor, f.caret), ("héllo wörld".len(), "héllo wörld".len()));
}

#[test]
fn insert_text_is_handed_to_the_shell_with_the_selection_it_replaces() {
    let mut d = doc();
    d.set_focus(Some(focus(3, 3)));
    d.set_focus(None);
    d.apply_handler(r##"query("#q")[0].insertText("XY")"##);
    let inserts = d.take_inserts();
    assert_eq!(inserts.len(), 1);
    let (f, text) = &inserts[0];
    assert_eq!((f.model.as_str(), f.caret, text.as_str()), ("text", 3, "XY"));
    // Anything but a text field is said and left alone.
    d.apply_handler(r##"query("#plain")[0].insertText("XY")"##);
    assert!(d.take_inserts().is_empty());
}

#[test]
fn marks_are_runs_over_the_text_in_bytes_styled_by_their_class() {
    let mut d = doc();
    let rich = field_text(&d).rich.clone().expect("runs");
    assert_eq!(rich.runs.len(), 1);
    // Characters 6..11, "wörld", are bytes 7..13.
    assert_eq!(rich.runs[0].range, 7..13);
    let style = &rich.runs[0].style;
    assert_eq!((style.color.b, style.color.r), (1.0, 0.0), "the class's colour");
    assert_eq!(style.background.map(|b| (b.r, b.g, b.b)), Some((1.0, 1.0, 0.0)));
    // A new list restyles the field; overlapping marks, the later one wins.
    d.apply_handler(r##"marks = [{ start: 0, end: 5, class: "kw" }, { start: 3, end: 8, class: "bad" }]"##);
    let rich = field_text(&d).rich.clone().expect("runs");
    let ranges: Vec<_> = rich.runs.iter().map(|r| r.range.clone()).collect();
    assert_eq!(ranges, vec![0..4, 4..10]);
    assert!(rich.runs[1].style.underline, "the decoration");
    // None, and none are drawn.
    d.apply_handler("marks = []");
    assert!(field_text(&d).rich.is_none());
}

#[test]
fn marks_that_no_longer_fit_the_text_are_dropped_not_drawn() {
    let mut d = doc();
    d.apply_handler(r##"text = "hé""##);
    assert!(field_text(&d).rich.is_none(), "a mark past the end of the text");
}
