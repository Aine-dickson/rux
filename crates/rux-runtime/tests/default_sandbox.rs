//! The terms every document of a process is opened on when the opener says
//! nothing: `rux run --sandbox` and the playground set it once. In a file of
//! its own because it is process-wide, and a test beside others would change
//! what they load.

use rux_runtime::{set_default_sandbox, Document, Sandbox};

const COPIES: &str =
    "<template><screen><text>hi</text></screen></template>\n<script>\nfn copy() { clipboard.writeText(\"x\"); }\n</script>";

#[test]
fn a_default_sandbox_covers_every_load_until_it_is_taken_off() {
    assert!(Document::from_source(COPIES).is_ok(), "trusted until told otherwise");

    set_default_sandbox(Some(Sandbox::new()));
    let err = Document::from_source(COPIES).err().expect("refused");
    assert!(err.contains("`clipboard`"), "{err}");

    set_default_sandbox(Some(Sandbox::new().grant("clipboard")));
    assert!(Document::from_source(COPIES).is_ok());

    set_default_sandbox(None);
    assert!(Document::from_source(COPIES).is_ok());
}
