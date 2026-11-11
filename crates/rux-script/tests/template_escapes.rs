//! A backtick string decodes its escapes as a plain string does (AW-040):
//! `` `a\nb` `` is three characters, as in JavaScript, not four.

use rux_script::{Builder, Engine};

fn engine() -> Engine {
    let mut b = Builder::new();
    b.interpreted();
    b.build("let x = \"a\";").expect("builds")
}

fn shown(e: &mut Engine, what: &str) -> String {
    e.eval_display(what, &[])
}

#[test]
fn a_template_string_decodes_its_escapes() {
    let mut e = engine();
    assert_eq!(shown(&mut e, r#""a\nb".length"#), "3");
    assert_eq!(shown(&mut e, r#"`a\nb`.length"#), "3");
    assert_eq!(shown(&mut e, r#"`${x}\nb`.length"#), "3");
    assert_eq!(shown(&mut e, r#"`a\tb` == "a\tb""#), "true");
    assert_eq!(shown(&mut e, r#"`a\rb` == "a\rb""#), "true");
    assert_eq!(shown(&mut e, r#"`a\\b`.length"#), "3");
    assert_eq!(shown(&mut e, r#"`A`"#), "A");
    assert_eq!(shown(&mut e, r#"`a\`b`"#), "a`b");
    assert_eq!(shown(&mut e, r#"`\${x}`"#), "${x}");
    assert_eq!(shown(&mut e, r#"`\$5`"#), "$5");
    // A backslash ending a line joins the next on; a newline as written stays.
    assert_eq!(shown(&mut e, "`a\\\nb`"), "ab");
    assert_eq!(shown(&mut e, "`a\nb`.length"), "3");
}
