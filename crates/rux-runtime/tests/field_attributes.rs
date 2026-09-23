//! An `<input>`'s attributes and events, as far as the build and the layout
//! carry them: what reaches the field, what `disabled` takes away, and what is
//! refused. Enforcing them (the keystrokes, the focus, the keyboard) is the
//! shell's, and is driven by hand; see `docs/08-user-tests.md`.

use rux_layout::{EnterKey, Keyboard, Node};
use rux_runtime::Document;

fn doc(template: &str, script: &str) -> Document {
    Document::from_source(&format!(
        "<template><screen>{template}</screen></template>\n<script>\n{script}</script>\n\
         <style>.f:disabled {{ color: #ff0000; }} .f:enabled {{ color: #00ff00; }}</style>"
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

fn laid_out(doc: &Document) -> rux_layout::Layout {
    rux_layout::layout(&doc.root, 400.0, 800.0, &mut |_, _| (10.0, 10.0))
}

fn problems(doc: &Document) -> String {
    format!("{:?}", doc.diagnostics()).replace('\\', "")
}

#[test]
fn every_attribute_reaches_the_field() {
    let d = doc(
        r#"<input r-model="code" maxlength="6" inputmode="numeric" enterkeyhint="go"
                  autofocus readonly @input="n = 1" @change="n = 2" @focus="n = 3" @blur="n = 4" />"#,
        "let code = signal(\"\"); let n = signal(0);",
    );
    // The four events are an input's own, so none of this is an error.
    assert!(!problems(&d).contains("level: Error"), "{}", problems(&d));
    let field = &input(&d, "code").field;
    assert_eq!(field.maxlength, Some(6));
    assert_eq!(field.keyboard, Keyboard::Numeric);
    assert_eq!(field.enter_key, EnterKey::Go);
    assert!(field.autofocus && field.readonly && !field.disabled);
    assert_eq!(field.on_input.as_deref(), Some("n = 1"));
    assert_eq!(field.on_change.as_deref(), Some("n = 2"));
    assert_eq!(field.on_focus.as_deref(), Some("n = 3"));
    assert_eq!(field.on_blur.as_deref(), Some("n = 4"));
    // And the layout hands them to the shell, on the region it focuses.
    let layout = laid_out(&d);
    let region = layout.focuses.iter().find(|f| f.model == "code").expect("focusable");
    assert_eq!(region.field, *field);
}

/// A field with none of them is an ordinary editable field telling nobody.
#[test]
fn a_plain_field_has_the_defaults() {
    let d = doc(r#"<input r-model="name" />"#, "let name = signal(\"\");");
    assert_eq!(input(&d, "name").field, rux_layout::Field::default());
}

/// A disabled field keeps showing its value, and has nothing to focus: no
/// region to tap, nothing for Tab, nothing for a label to reach.
#[test]
fn a_disabled_field_cannot_be_reached() {
    let d = doc(
        r#"<text for="n">Name</text><input id="n" class="f" r-model="name" disabled />"#,
        "let name = signal(\"Ada\");",
    );
    let layout = laid_out(&d);
    assert!(layout.focuses.is_empty(), "no focus region, the label's included");
    assert!(layout.focusables.is_empty(), "nothing for Tab to land on");
    let shown = find(&d.root, &|n| n.text.as_ref().is_some_and(|t| t.text == "Ada"));
    assert!(shown.is_some(), "the value still shows");
}

#[test]
fn a_disabled_button_answers_nothing() {
    let d = doc(
        r#"<button class="f" disabled @tap="n += 1" @press="n += 1"><text>Go</text></button>"#,
        "let n = signal(0);",
    );
    let layout = laid_out(&d);
    assert!(layout.hits.is_empty(), "no tap and no gesture to hit");
    assert!(layout.focusables.is_empty());
}

/// A disabled checkbox cannot be toggled, and a label pointing at it cannot
/// toggle it either.
#[test]
fn a_disabled_checkbox_does_not_toggle() {
    let d = doc(
        r#"<text for="c">Agree</text><input id="c" type="checkbox" r-model="ok" disabled />"#,
        "let ok = signal(false);",
    );
    assert!(laid_out(&d).hits.is_empty(), "neither the box nor its label taps");
}

/// `:disabled` and `:enabled` match form controls, and the bound form reads a
/// signal.
#[test]
fn disabled_and_enabled_match_and_bind() {
    let d = doc(
        r#"<input class="f" r-model="a" :disabled="locked" /><input class="f" r-model="b" />"#,
        // Values, because an empty field paints its placeholder colour instead.
        "let a = signal(\"x\"); let b = signal(\"y\"); let locked = signal(true);",
    );
    // Red for disabled, green for enabled, per the stylesheet `doc` writes.
    let (red, green) = ((1.0, 0.0), (0.0, 1.0));
    let colour = |d: &Document, m: &str| {
        let c = input(d, m).children[0].text.as_ref().unwrap().color;
        (c.r, c.g)
    };
    assert!(input(&d, "a").field.disabled);
    assert_eq!(colour(&d, "a"), red);
    assert_eq!(colour(&d, "b"), green);
    let mut d = d;
    d.apply_handler("locked = false");
    assert!(!input(&d, "a").field.disabled, "the bound form follows its signal");
    assert_eq!(colour(&d, "a"), green);
}

/// `disabled="false"` is disabled, as in HTML, and says so.
#[test]
fn disabled_false_is_still_disabled_and_warns() {
    let d = doc(r#"<input r-model="a" disabled="false" />"#, "let a = signal(\"\");");
    assert!(input(&d, "a").field.disabled);
    assert!(problems(&d).contains("still means disabled"), "{}", problems(&d));
}

#[test]
fn an_unknown_keyboard_or_action_or_length_is_refused() {
    for (attr, said) in [
        (r#"inputmode="digits""#, "numeric"),
        (r#"enterkeyhint="submit""#, "send"),
        (r#"maxlength="ten""#, "whole number"),
    ] {
        let d = doc(&format!(r#"<input r-model="a" {attr} />"#), "let a = signal(\"\");");
        let p = problems(&d);
        assert!(p.contains("level: Error") && p.contains(said), "{attr}: {p}");
    }
}

/// Off an input, a field event is still refused, and the refusal says where
/// it belongs.
#[test]
fn a_field_event_off_an_input_is_refused() {
    let d = doc(r#"<view @change="a = 1"><text>x</text></view>"#, "let a = signal(0);");
    let p = problems(&d);
    assert!(p.contains("not an event") && p.contains("belongs on an `<input>`"), "{p}");
}

/// A toggle's `@change` runs after the toggle, handed the value it now has.
#[test]
fn a_checkbox_change_sees_the_new_value() {
    let mut d = doc(
        r#"<input type="checkbox" r-model="ok" @change="seen = event.value" />"#,
        "let ok = signal(false); let seen = signal(\"never\");",
    );
    let tap = laid_out(&d).hits[0].on_tap.clone().expect("the box taps");
    d.apply_handler(&tap);
    assert_eq!(d.value_in("seen", None, None), "true");
}
