//! The controls `type=` names beyond a text field: `number`, `switch`,
//! `slider` and `date`. What the build and the document do with each; the
//! keystrokes, the drag and the platform's picker are the shell's, and are
//! driven by hand (see `docs/08-user-tests.md`).

use rux_layout::{InputKind, Node};
use rux_reactive::Value;
use rux_runtime::{Document, Focus};

fn doc(template: &str, script: &str) -> Document {
    Document::from_source(&format!(
        "<template><screen>{template}</screen></template>\n<script>\n{script}</script>"
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

/// What an input shows: the text of its first child.
fn shown(doc: &Document, model: &str) -> String {
    input(doc, model).children[0].text.as_ref().expect("a text child").text.clone()
}

fn problems(doc: &Document) -> String {
    format!("{:?}", doc.diagnostics()).replace('\\', "")
}

#[test]
fn a_number_field_is_its_own_kind() {
    let d = doc(r#"<input type="number" r-model="qty" />"#, "let qty = signal(3);");
    assert!(!problems(&d).contains("level: Error"), "{}", problems(&d));
    assert_eq!(input(&d, "qty").kind, InputKind::Number);
    assert_eq!(shown(&d, "qty"), "3");
}

/// The signal is written as a number, not as the text of one, so script
/// arithmetic on it is arithmetic and not concatenation.
#[test]
fn a_number_field_writes_a_number() {
    let mut d = doc(
        r#"<input type="number" r-model="qty" /><text>{{ qty + 1 }}</text>"#,
        "let qty = signal(3);",
    );
    d.apply_value_in("qty", None, None, &Value::Number(41.0));
    assert_eq!(d.typed_value_in("qty", None, None), Some(Value::Number(41.0)));
    assert!(find(&d.root, &|n| n.text.as_ref().is_some_and(|t| t.text == "42")).is_some());
}

/// `1.` is on its way to a number. The field shows what was typed while the
/// signal keeps the number it had, and leaving the field shows the number.
#[test]
fn a_number_draft_shows_while_focused_and_ends_with_focus() {
    let mut d = doc(
        r#"<input type="number" r-model="qty" /><input r-model="name" />"#,
        "let qty = signal(1); let name = signal(\"\");",
    );
    d.set_focus(Some(Focus::at("qty", 2)));
    d.set_draft(Some("1.".to_string()));
    assert_eq!(shown(&d, "qty"), "1.");
    assert_eq!(d.typed_value_in("qty", None, None), Some(Value::Number(1.0)));
    // The caret indexes the draft, so it may sit past the number's text.
    assert_eq!(input(&d, "qty").children[0].text.as_ref().unwrap().caret, Some(2));

    d.set_focus(Some(Focus::at("name", 0)));
    assert_eq!(shown(&d, "qty"), "1", "the field shows its number again");
}

/// A signal change while a draft is up still paints the draft, because the
/// shell writes the draft and the number together.
#[test]
fn a_patch_does_not_paint_over_a_draft() {
    let mut d = doc(r#"<input type="number" r-model="qty" />"#, "let qty = signal(1);");
    d.set_focus(Some(Focus::at("qty", 4)));
    d.apply_value_in("qty", None, None, &Value::Number(1.5));
    d.set_draft(Some("1.50".to_string()));
    assert_eq!(shown(&d, "qty"), "1.50");
    d.apply_value_in("qty", None, None, &Value::Number(1.25));
    assert_eq!(shown(&d, "qty"), "1.50", "the draft stands until the shell drops it");
    d.set_draft(None);
    assert_eq!(shown(&d, "qty"), "1.25");
}

/// An emptied field shows its placeholder, even though the signal still
/// holds the last number.
#[test]
fn an_empty_draft_shows_the_placeholder() {
    let mut d = doc(
        r#"<input type="number" r-model="qty" placeholder="How many" />"#,
        "let qty = signal(7);",
    );
    d.set_focus(Some(Focus::at("qty", 0)));
    d.set_draft(Some(String::new()));
    assert_eq!(shown(&d, "qty"), "How many");
    assert_eq!(d.typed_value_in("qty", None, None), Some(Value::Number(7.0)));
}

/// A number field inside a component writes its instance's own state, as a
/// number.
#[test]
fn a_number_field_in_a_component_writes_a_number() {
    let dir = std::env::temp_dir().join(format!("rux_number_instance_{}", std::process::id()));
    std::fs::create_dir_all(dir.join("components")).unwrap();
    std::fs::write(
        dir.join("components/counter.rux"),
        "<template><view><input type=\"number\" r-model=\"n\" /><text>{{ n * 2 }}</text></view></template>\
         <script>let n = signal(1);</script>",
    )
    .unwrap();
    std::fs::write(
        dir.join("app.rux"),
        "<template><screen><counter /></screen></template><script>use components::counter;</script>",
    )
    .unwrap();
    let mut d = Document::load(dir.join("app.rux")).expect("loads");
    let _ = std::fs::remove_dir_all(&dir);
    let instance = input(&d, "n").instance.clone().expect("in an instance");
    d.apply_value_in("n", None, Some(&instance), &Value::Number(21.0));
    assert_eq!(d.typed_value_in("n", None, Some(&instance)), Some(Value::Number(21.0)));
    assert!(
        find(&d.root, &|n| n.text.as_ref().is_some_and(|t| t.text == "42")).is_some(),
        "{}",
        problems(&d)
    );
}

fn laid_out(doc: &Document) -> rux_layout::Layout {
    rux_layout::layout(&doc.root, 400.0, 800.0, &mut |_, _| (10.0, 10.0))
}

/// A switch is a checkbox that looks and announces itself differently: a tap
/// flips the bound bool, `@change` sees the new value, and the thumb moves to
/// the end the value points to.
#[test]
fn a_switch_toggles_like_a_checkbox() {
    let mut d = doc(
        r#"<input type="switch" r-model="wifi" @change="seen = event.value" />"#,
        "let wifi = signal(false); let seen = signal(\"never\");",
    );
    assert!(!problems(&d).contains("level: Error"), "{}", problems(&d));
    let node = input_by_role(&d);
    assert_eq!(node.access.role, rux_layout::AccessRole::Switch);
    assert_eq!(node.access.checked, Some(false));
    assert_eq!(node.children.len(), 1, "a thumb, on or off");
    let tap = node.on_tap.clone().expect("a tap toggles it");

    let off = thumb_x(&d);
    assert!(d.apply_handler(&tap));
    assert_eq!(d.typed_value_in("wifi", None, None), Some(Value::Bool(true)));
    assert_eq!(d.typed_value_in("seen", None, None), Some(Value::Bool(true)));
    assert_eq!(input_by_role(&d).access.checked, Some(true));
    assert!(thumb_x(&d) > off + 10.0, "the thumb moved to the far end");
}

/// An unstyled switch has a size, so it is not a box with nothing in it.
#[test]
fn an_unstyled_switch_has_a_size() {
    let d = doc(r#"<input type="switch" r-model="on" />"#, "let on = signal(true);");
    let layout = laid_out(&d);
    let hit = layout.hits.iter().find(|h| h.width > 0.0).expect("something to tap");
    assert_eq!((hit.width, hit.height), (44.0, 24.0));
}

/// A disabled switch shows its state and cannot be flipped.
#[test]
fn a_disabled_switch_does_not_flip() {
    let d = doc(r#"<input type="switch" r-model="on" disabled />"#, "let on = signal(true);");
    assert!(input_by_role(&d).on_tap.is_none());
}

fn input_by_role(doc: &Document) -> &Node {
    find(&doc.root, &|n| n.access.role == rux_layout::AccessRole::Switch).expect("a switch")
}

/// Where the switch's thumb sits: the painted box that is round and small.
fn thumb_x(doc: &Document) -> f32 {
    let layout = laid_out(doc);
    let hit = layout.hits.iter().find(|h| h.width > 0.0).expect("the switch");
    // The thumb is the second box painted inside the track's bounds.
    layout
        .paints
        .iter()
        .filter_map(|p| match p {
            rux_layout::Paint::Rect(r) if r.width < hit.width => Some(r.x),
            _ => None,
        })
        .next()
        .expect("a thumb is painted")
}

fn slider(doc: &Document) -> &Node {
    find(&doc.root, &|n| n.access.role == rux_layout::AccessRole::Slider).expect("a slider")
}

/// A pointer event the way the shell builds one: `x` across an element of
/// `width`.
fn pointer(x: f64, width: f64, phase: Option<&str>) -> Value {
    let mut fields = vec![
        ("x".to_string(), Value::Number(x)),
        ("y".to_string(), Value::Number(16.0)),
        ("width".to_string(), Value::Number(width)),
        ("height".to_string(), Value::Number(32.0)),
    ];
    if let Some(phase) = phase {
        fields.push(("phase".to_string(), Value::Text(phase.to_string())));
    }
    Value::Map(fields)
}

fn number(doc: &mut Document, name: &str) -> f64 {
    match doc.typed_value_in(name, None, None) {
        Some(Value::Number(n)) => n,
        other => panic!("{name} is not a number: {other:?}"),
    }
}

/// A tap sets the value from where it lands, snapped to the step: the thumb
/// rests between x = 10 and x = width - 10, so on a 220-wide slider the
/// middle of the range is x = 110.
#[test]
fn a_tap_on_a_slider_sets_the_value_where_it_lands() {
    let mut d = doc(
        r#"<input type="slider" r-model="vol" min="0" max="10" step="1" />"#,
        "let vol = signal(0);",
    );
    assert!(!problems(&d).contains("level: Error"), "{}", problems(&d));
    let tap = slider(&d).on_tap.clone().expect("a tap handler");
    d.apply_handler_with_event(&tap, None, &pointer(110.0, 220.0, None));
    assert_eq!(number(&mut d, "vol"), 5.0);
    // Past either end is the end, not beyond it.
    d.apply_handler_with_event(&tap, None, &pointer(-40.0, 220.0, None));
    assert_eq!(number(&mut d, "vol"), 0.0);
    d.apply_handler_with_event(&tap, None, &pointer(400.0, 220.0, None));
    assert_eq!(number(&mut d, "vol"), 10.0);
    // Snapped: 0.62 of the way is 6.2, which the step makes 6.
    d.apply_handler_with_event(&tap, None, &pointer(10.0 + 0.62 * 200.0, 220.0, None));
    assert_eq!(number(&mut d, "vol"), 6.0);
    assert_eq!(slider(&d).access.value.as_deref(), Some("6"));
}

/// A tap with no pointer (Space on a focused slider, `tap()` from script)
/// has nowhere to put the thumb, and leaves the value alone rather than
/// failing.
#[test]
fn a_tap_without_a_pointer_leaves_a_slider_alone() {
    let mut d = doc(r#"<input type="slider" r-model="vol" />"#, "let vol = signal(40);");
    let tap = slider(&d).on_tap.clone().unwrap();
    d.apply_handler_with_event(&tap, None, &Value::Map(vec![]));
    assert_eq!(number(&mut d, "vol"), 40.0);
    assert!(!problems(&d).contains("failed"), "{}", problems(&d));
}

/// A fractional step lands on the step, not on the float next to it.
#[test]
fn a_fractional_step_reads_as_written() {
    let mut d = doc(
        r#"<input type="slider" r-model="t" min="0" max="1" step="0.1" />"#,
        "let t = signal(0);",
    );
    let tap = slider(&d).on_tap.clone().unwrap();
    d.apply_handler_with_event(&tap, None, &pointer(10.0 + 0.3 * 200.0, 220.0, None));
    assert_eq!(number(&mut d, "t"), 0.3);
    d.apply_handler_with_event(&tap, None, &pointer(10.0 + 0.7 * 200.0, 220.0, None));
    assert_eq!(number(&mut d, "t"), 0.7);
}

/// A drag moves the value as it goes, fires `@input` on every change and
/// `@change` once, when the hand lets go.
#[test]
fn a_drag_fires_input_as_it_moves_and_change_at_the_end() {
    let mut d = doc(
        r#"<input type="slider" r-model="v" @input="inputs += 1" @change="changed = event.value" />"#,
        "let v = signal(0); let inputs = signal(0); let changed = signal(-1);",
    );
    let (_, drag) = slider(&d)
        .gestures
        .iter()
        .find(|(g, _)| *g == rux_layout::Gesture::Drag)
        .cloned()
        .expect("a drag handler");
    for (x, phase) in [(60.0, "start"), (110.0, "move"), (110.0, "move"), (160.0, "move")] {
        d.apply_handler_with_event(&drag, None, &pointer(x, 220.0, Some(phase)));
    }
    assert_eq!(number(&mut d, "v"), 75.0);
    assert_eq!(number(&mut d, "inputs"), 3.0, "one per change, none for the still move");
    assert_eq!(number(&mut d, "changed"), -1.0, "not yet");
    d.apply_handler_with_event(&drag, None, &pointer(160.0, 220.0, Some("end")));
    assert_eq!(number(&mut d, "changed"), 75.0);
}

/// The fill and the rest share the track in the value's proportion, so the
/// thumb moves with the value and never overhangs an end.
#[test]
fn the_thumb_sits_where_the_value_is() {
    let thumb = |value: f64| {
        let d = doc(
            r#"<input type="slider" r-model="v" style="width: 220px" />"#,
            &format!("let v = signal({value});"),
        );
        let layout = laid_out(&d);
        layout
            .paints
            .iter()
            .find_map(|p| match p {
                rux_layout::Paint::Rect(r) if r.width == 20.0 && r.height == 20.0 => Some(r.x),
                _ => None,
            })
            .expect("a thumb")
    };
    assert_eq!(thumb(0.0), 0.0);
    assert_eq!(thumb(50.0), 100.0);
    assert_eq!(thumb(100.0), 200.0);
}

/// A disabled slider shows its value and takes no finger.
#[test]
fn a_disabled_slider_takes_no_finger() {
    let d = doc(r#"<input type="slider" r-model="v" disabled />"#, "let v = signal(3);");
    assert!(slider(&d).on_tap.is_none() && slider(&d).gestures.is_empty());
}

/// A date field shows `YYYY-MM-DD` and is announced as a date.
#[test]
fn a_date_field_is_its_own_kind() {
    let d = doc(
        r#"<input type="date" r-model="due" min="2026-01-01" max="2026-12-31" />"#,
        "let due = signal(\"2026-09-23\");",
    );
    assert!(!problems(&d).contains("level: Error"), "{}", problems(&d));
    let field = input(&d, "due");
    assert_eq!(field.kind, InputKind::Date);
    assert_eq!(field.access.role, rux_layout::AccessRole::DateInput);
    assert_eq!(field.field.min, Some((2026, 1, 1)));
    assert_eq!(field.field.max, Some((2026, 12, 31)));
    assert_eq!(shown(&d, "due"), "2026-09-23");
}

/// A `min` that is not a date is an error, not a picker that offers every
/// day there is.
#[test]
fn a_date_range_that_is_not_dates_is_an_error() {
    let d = doc(r#"<input type="date" r-model="due" min="yesterday" />"#, "let due = signal(\"\");");
    assert!(problems(&d).contains("is not a date written `YYYY-MM-DD`"), "{}", problems(&d));
}

/// A slider's bad range is an error too, and a date's range is not read as
/// a slider's.
#[test]
fn a_slider_range_that_is_not_numbers_is_an_error() {
    let d = doc(r#"<input type="slider" r-model="v" max="loud" />"#, "let v = signal(0);");
    assert!(problems(&d).contains("`max=\"loud\"` is not a number"), "{}", problems(&d));
    let d = doc(r#"<input type="slider" r-model="v" min="5" max="5" />"#, "let v = signal(5);");
    assert!(problems(&d).contains("is not above `min`"), "{}", problems(&d));
    let d = doc(r#"<input type="slider" r-model="v" step="0" />"#, "let v = signal(5);");
    assert!(problems(&d).contains("`step=\"0\"` is not a number above zero"), "{}", problems(&d));
}
