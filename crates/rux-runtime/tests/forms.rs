//! `<view role="form">`: which fields belong to it, what a submit button asks
//! for, what a submission checks and sends, and `:invalid` and its relatives.
//! What Enter and Next do with it is the shell's, driven by hand; see
//! `docs/08-user-tests.md`.

use rux_layout::{Background, Node, Rgba};
use rux_reactive::Value;
use rux_runtime::Document;
use rux_style::InteractionState;

fn doc(template: &str, script: &str) -> Document {
    Document::from_source(&format!(
        "<template><screen>{template}</screen></template>\n<script>\n{script}</script>\n\
         <style>\
         .f:invalid {{ background: #ff0000; }} .f:valid {{ background: #00ff00; }}\
         .u:user-invalid {{ background: #ff0000; }}\
         .r:required {{ background: #0000ff; }} .r:optional {{ background: #ffff00; }}\
         </style>"
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
    find(&doc.root, &|n| n.field.bind.as_deref() == Some(model)).expect("the input is built")
}

fn background(doc: &Document, model: &str) -> Option<[f32; 4]> {
    match &input(doc, model).style.background {
        Some(Background::Color(Rgba { r, g, b, a })) => Some([*r, *g, *b, *a]),
        _ => None,
    }
}

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

fn problems(doc: &Document) -> String {
    format!("{:?}", doc.diagnostics()).replace('\\', "")
}

/// The path of the form node, which is how a field and a button name it.
fn form_path(doc: &Document) -> Vec<usize> {
    fn walk(node: &Node, path: &mut Vec<usize>) -> bool {
        if node.form.is_some() {
            return true;
        }
        for (i, child) in node.children.iter().enumerate() {
            path.push(i);
            if walk(child, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut path = Vec::new();
    assert!(walk(&doc.root, &mut path), "there is a form");
    path
}

fn get<'a>(values: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    values.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

const SIGNUP: &str = r#"
<view role="form" @submit="sent = event.values.email" @invalid="why = event.errors.email">
  <input r-model="email" name="email" inputmode="email" required />
  <input r-model="code" minlength="4" pattern="\d+" />
  <input type="checkbox" r-model="terms" name="terms" required />
  <input r-model="off" disabled required />
  <button type="submit">Join</button>
</view>
<input r-model="elsewhere" required />
"#;

const SIGNUP_SCRIPT: &str = "let email = signal(\"\"); let code = signal(\"\"); \
     let terms = signal(false); let off = signal(\"\"); let elsewhere = signal(\"\"); \
     let sent = signal(\"\"); let why = signal(\"\");";

#[test]
fn fields_belong_to_the_form_around_them() {
    let d = doc(SIGNUP, SIGNUP_SCRIPT);
    assert!(!problems(&d).contains("level: Error"), "{}", problems(&d));
    let form = form_path(&d);
    for model in ["email", "code", "terms", "off"] {
        assert_eq!(input(&d, model).field.form.as_ref(), Some(&form), "{model}");
    }
    assert_eq!(input(&d, "elsewhere").field.form, None, "outside the form");
    let email = &input(&d, "email").field;
    assert_eq!(email.name.as_deref(), Some("email"));
    assert!(email.checks.required);
    assert_eq!(input(&d, "code").field.checks.minlength, Some(4));
    assert_eq!(input(&d, "terms").field.shape, rux_layout::Shape::Flag);
}

/// The button's tap ends by asking for its form, which is what makes a
/// finger, Enter on the button and a script's `tap()` all submit alike.
#[test]
fn a_submit_button_asks_for_its_form() {
    let mut d = doc(SIGNUP, SIGNUP_SCRIPT);
    let form = form_path(&d);
    let tap = find(&d.root, &|n| n.on_tap.as_deref().is_some_and(|t| t.contains("__rux_submit")))
        .and_then(|n| n.on_tap.clone())
        .expect("the button submits");
    d.apply_handler_in(&tap, None);
    assert_eq!(d.take_submits(), vec![form]);
    assert!(d.take_submits().is_empty(), "taking them clears them");
}

#[test]
fn a_failing_form_reports_each_field_in_order() {
    let mut d = doc(SIGNUP, SIGNUP_SCRIPT);
    let form = form_path(&d);
    d.apply_handler_in("email = \"grace\"; code = \"12\"", None);
    let report = d.check_form(&form).expect("a form");
    let failed: Vec<(&str, &str)> =
        report.failures.iter().map(|f| (f.name.as_str(), f.message.as_str())).collect();
    assert_eq!(
        failed,
        vec![
            ("email", "Enter an email address."),
            ("code", "Use at least 4 characters (you are using 2)."),
            ("terms", "Fill in this field."),
        ],
        "the disabled field is neither checked nor sent"
    );
    // Text fields can take the caret; a checkbox cannot.
    assert_eq!(report.failures[0].focus, Some(("email".into(), None, None)));
    assert_eq!(report.failures[2].focus, None);
    assert!(get(&report.values, "off").is_none(), "disabled is not sent");
}

#[test]
fn a_passing_form_sends_every_value_by_name_or_model() {
    let mut d = doc(SIGNUP, SIGNUP_SCRIPT);
    let form = form_path(&d);
    d.apply_handler_in("email = \"grace@navy.mil\"; code = \"1906\"; terms = true", None);
    let report = d.check_form(&form).expect("a form");
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(get(&report.values, "email"), Some(&Value::Text("grace@navy.mil".into())));
    assert_eq!(get(&report.values, "code"), Some(&Value::Text("1906".into())), "by r-model");
    assert_eq!(get(&report.values, "terms"), Some(&Value::Bool(true)));
    assert_eq!(report.form.on_submit.as_deref(), Some("sent = event.values.email"));
}

/// A list of rows, each with a field bound to the same `r-model` text: every
/// value is sent, under the one name.
#[test]
fn a_name_in_every_row_sends_every_row() {
    let mut d = doc(
        r#"<view role="form">
             <view r-for="item in items" r-key="item.id">
               <input r-model="item.name" name="names" required />
             </view>
           </view>"#,
        "let items = signal([#{ id: 1, name: \"a\" }, #{ id: 2, name: \"\" }]);",
    );
    let form = form_path(&d);
    let report = d.check_form(&form).expect("a form");
    assert_eq!(
        get(&report.values, "names"),
        Some(&Value::List(vec![Value::Text("a".into()), Value::Text("".into())]))
    );
    assert_eq!(report.failures.len(), 1, "only the empty row fails");
    assert_eq!(report.failures[0].focus.as_ref().and_then(|f| f.1.clone()), Some("2".into()));
}

/// `:invalid` and `:valid` hold from the start, as in CSS, and follow the
/// value as it changes.
#[test]
fn invalid_follows_the_value() {
    let mut d = doc(
        r#"<input class="f" r-model="name" required /><input class="f" r-model="free" />
           <input class="f" r-model="locked" required readonly />"#,
        "let name = signal(\"\"); let free = signal(\"\"); let locked = signal(\"\");",
    );
    assert_eq!(background(&d, "name"), Some(RED));
    assert_eq!(background(&d, "free"), Some(GREEN), "nothing to fail");
    assert_eq!(background(&d, "locked"), None, "readonly is not checked at all");
    d.apply_handler_in("name = \"Ada\"", None);
    assert_eq!(background(&d, "name"), Some(GREEN));
}

/// `:user-invalid` waits for the person: a field they have left, or a form
/// whose submission they tried.
#[test]
fn user_invalid_waits_for_the_person() {
    let mut d = doc(
        r#"<view role="form"><input class="u" r-model="a" required /><input class="u" r-model="b" required /></view>"#,
        "let a = signal(\"\"); let b = signal(\"\");",
    );
    assert_eq!(background(&d, "a"), None, "nobody has been near it");
    let mut state = InteractionState {
        touched: vec![("a".into(), None, None)],
        ..InteractionState::default()
    };
    d.set_interaction(state.clone());
    assert_eq!(background(&d, "a"), Some(RED), "left, and still empty");
    assert_eq!(background(&d, "b"), None, "not yet touched");
    state.attempted.push(form_path(&d));
    d.set_interaction(state);
    assert_eq!(background(&d, "b"), Some(RED), "a tried submission reaches every field");
}

#[test]
fn required_and_optional() {
    let d = doc(
        r#"<input class="r" r-model="a" required /><input class="r" r-model="b" />"#,
        "let a = signal(\"\"); let b = signal(\"\");",
    );
    assert_eq!(background(&d, "a"), Some([0.0, 0.0, 1.0, 1.0]));
    assert_eq!(background(&d, "b"), Some([1.0, 1.0, 0.0, 1.0]));
}

#[test]
fn what_cannot_work_is_reported() {
    for (template, says) in [
        (r#"<button type="submit">Go</button>"#, "nothing for it to submit"),
        (r#"<button type="reset">Go</button>"#, "not a kind of button"),
        (r#"<input r-model="a" pattern="(" />"#, "not a regular expression"),
        (r#"<input r-model="a" minlength="two" />"#, "not a whole number"),
        (r#"<view @submit="a = 1"></view>"#, "belongs on the `<view role=\"form\">`"),
    ] {
        let d = doc(template, "let a = signal(\"\");");
        assert!(problems(&d).contains(says), "{template}: {}", problems(&d));
    }
}
