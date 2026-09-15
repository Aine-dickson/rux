//! What a percentage says on a property that is resolved to plain pixels.
//!
//! These are reported rather than dropped, which was the patch. The message
//! itself is the thing under test here: it used to read as a design
//! impossibility ("is resolved before there is a box to take a percentage of"),
//! which is true of the cascade and tells an author to give up. The feature is
//! unbuilt, not impossible, and for a radius there is a workaround that gives
//! exactly what the percentage was reaching for.
//!
//! Reported twice by the same person, the second time with a screenshot of a
//! square button carrying `border-radius: 100%`.

use rux_runtime::Document;

/// Both shapes of the message, from one document, because `warn_once` dedupes
/// for the life of the process and two tests would fight over it.
#[test]
fn a_percentage_says_it_is_unbuilt_and_a_radius_says_what_to_write_instead() {
    let doc = Document::from_source(
        "<template><screen><view class=\"a\"><text>x</text></view></screen></template>
         <style>
           .a { border-radius: 50%; padding: 10%; }
         </style>",
    )
    .expect("loads");
    let said = format!("{:?}", doc.diagnostics());

    // Unbuilt, not impossible. "yet" is the word doing the work.
    assert!(
        said.contains("does not honor a percentage on `border-radius` yet"),
        "says it is not built yet: {said}"
    );
    assert!(
        said.contains("does not honor a percentage on `padding` yet"),
        "and the same for the others: {said}"
    );

    // The radius names the workaround, because a radius bigger than the box is
    // clamped to it and that is the circle the author wanted.
    assert!(said.contains("9999px"), "the radius names a way to get there: {said}");

    // `padding` has no such trick, so it must not be offered one.
    let padding_sentence = said
        .split("`padding: 10%`")
        .nth(1)
        .unwrap_or("")
        .split("`border-radius")
        .next()
        .unwrap_or("");
    assert!(
        !padding_sentence.contains("9999px"),
        "padding is not offered a radius trick: {padding_sentence}"
    );
}
