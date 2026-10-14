//! A `<text>` with elements inside it: joined into one sentence, measured
//! and drawn as one text, with each part placed where the text engine put it
//! and answering for itself.

use rux_layout::*;

fn on_screen(node: Node) -> Node {
    let mut screen = Node::new(Style { display: Display::Flex, axis: Axis::Column, ..Default::default() });
    screen.children.push(node);
    screen
}

fn words(text: &str, ws: WhiteSpace) -> TextContent {
    TextContent {
        text: text.into(),
        font_size: 16.0,
        weight: 400,
        color: Rgba::new(0.0, 0.0, 0.0, 1.0),
        align: TextAlign::Start,
        wrap: TextWrap::Normal,
        font_family: None,
        letter_spacing: None,
        word_spacing: None,
        line_height: None,
        italic: false,
        underline: false,
        strikethrough: false,
        nowrap: false,
        caret: None,
        selection: None,
        preedit: None,
        selection_style: Default::default(),
        rich: None,
        white_space: ws,
        direction: None,
        isolate: Default::default(),
    }
}

fn run(text: &str) -> Node {
    Node::text(Style::default(), words(text, WhiteSpace::Normal))
}

/// `Read the <b @tap>terms</b> <icon> now`, as the build hands it over.
fn sentence() -> Node {
    let mut s = Node::text(Style { align_self: Some(Align::Start), ..Default::default() }, words("", WhiteSpace::Normal));
    s.children.push(run("\n  Read  the "));
    let mut terms = run("terms");
    terms.text.as_mut().unwrap().weight = 700;
    terms.on_tap = Some("open()".into());
    terms.access = Access { role: AccessRole::Button, label: Some("terms".into()), ..Access::default() };
    s.children.push(terms);
    s.children.push(run(" "));
    let icon = Node::path(
        Style {
            width: Some(Len::Px(12.0)),
            height: Some(Len::Px(10.0)),
            vertical_align: VerticalAlign::Middle,
            fill: Some(Rgba::new(1.0, 0.0, 0.0, 1.0)),
            ..Default::default()
        },
        PathContent::parse("M0 0 H12 V10 H0 Z"),
    );
    s.children.push(icon);
    s.children.push(run(" now\n"));
    s
}

/// A text engine that answers the layout's questions with fixed numbers:
/// every text is 100 by 20, the box sits at (40, 5), and each run covers one
/// 10px-wide rectangle per character on one line.
struct Fake {
    asked: Vec<(String, f32)>,
}

impl TextMeasure for Fake {
    fn measure(&mut self, _: &TextContent, _: Option<f32>) -> (f32, f32) {
        (100.0, 20.0)
    }

    fn inline(&mut self, tc: &TextContent, width: f32) -> std::sync::Arc<InlineGeometry> {
        self.asked.push((tc.text.clone(), width));
        let rich = tc.rich.as_ref().expect("asked only about a sentence");
        std::sync::Arc::new(InlineGeometry {
            items: rich.items.iter().map(|_| (40.0, 5.0)).collect(),
            runs: rich
                .runs
                .iter()
                .map(|r| vec![(r.range.start as f32 * 10.0, 0.0, r.range.len() as f32 * 10.0, 20.0)])
                .collect(),
        })
    }
}

fn lay_out(root: Node) -> (Layout, Fake) {
    let mut fake = Fake { asked: Vec::new() };
    let out = LayoutState::new().lay_out_text(&on_screen(root), 800.0, 600.0, &[], &mut fake);
    (out, fake)
}

/// The pieces become one text with spaces collapsed across them, the runs
/// at their places in it and the box between two words.
#[test]
fn the_pieces_are_joined_into_one_text() {
    let (out, _) = lay_out(sentence());
    let texts: Vec<&TextContent> =
        out.paints.iter().filter_map(|p| if let Paint::Text(t) = p { Some(&t.content) } else { None }).collect();
    assert_eq!(texts.len(), 1, "one text is drawn, not one per run");
    let tc = texts[0];
    assert_eq!(tc.text, "Read the terms  now");
    let rich = tc.rich.as_ref().unwrap();
    let ranges: Vec<_> = rich.runs.iter().map(|r| r.range.clone()).collect();
    assert_eq!(ranges, [0..9, 9..14, 14..15, 15..19]);
    assert_eq!(rich.runs[1].style.weight, 700);
    assert_eq!(rich.items.len(), 1);
    assert_eq!(rich.items[0].index, 15, "after the space, before ` now`");
    assert_eq!((rich.items[0].width, rich.items[0].height), (12.0, 10.0));
    assert_eq!(rich.items[0].align, VerticalAlign::Middle);
}

/// The box is drawn where the engine put it, from the text's content corner,
/// and the engine was asked at the width the text was drawn at.
#[test]
fn a_box_is_drawn_where_the_text_engine_put_it() {
    let (out, fake) = lay_out(sentence());
    let Some(text) = out.paints.iter().find_map(|p| if let Paint::Text(t) = p { Some(t) } else { None }) else {
        panic!("a text")
    };
    let path = out.paints.iter().find_map(|p| if let Paint::Path(t) = p { Some(t) } else { None }).expect("the icon");
    assert_eq!((path.x, path.y), (text.x + 40.0, text.y + 5.0));
    // Debug builds lay out a second time from scratch to check the first, so
    // it may be asked more than once; always at the width it is drawn at.
    assert!(!fake.asked.is_empty());
    assert!(fake.asked.iter().all(|a| a == &("Read the terms  now".to_string(), text.width)), "{:?}", fake.asked);
}

/// A tappable run is found by a tap on its words, after the sentence's own
/// region, and is a Tab stop and a button to a screen reader. The words
/// around it are none of those.
#[test]
fn a_tappable_run_answers_at_its_words() {
    let (out, _) = lay_out(sentence());
    let text = out.paints.iter().find_map(|p| if let Paint::Text(t) = p { Some(t) } else { None }).unwrap();
    let hit = out.hits.iter().find(|h| h.on_tap.as_deref() == Some("open()")).expect("a region for the run");
    assert_eq!((hit.x, hit.y, hit.width, hit.height), (text.x + 90.0, text.y, 50.0, 20.0));
    assert_eq!(out.hits.len(), 1, "only the run answers taps");
    assert!(out.focusables.iter().any(|f| matches!(&f.kind, FocusKind::Activate { on_tap, .. } if on_tap == "open()")));
    let buttons: Vec<_> = out.access.iter().filter(|a| a.access.role == AccessRole::Button).collect();
    assert_eq!(buttons.len(), 1);
    assert_eq!(buttons[0].x, text.x + 90.0);
}

/// `r-show="false"` takes a run's words out of the sentence.
#[test]
fn a_hidden_run_is_not_in_the_sentence() {
    let mut s = sentence();
    s.children[1].hidden = true;
    let (out, _) = lay_out(s);
    let tc = out.paints.iter().find_map(|p| if let Paint::Text(t) = p { Some(&t.content) } else { None }).unwrap();
    assert_eq!(tc.text, "Read the  now");
    assert!(out.hits.is_empty());
}

/// Measured as one text: the size the engine gave the joined sentence.
#[test]
fn the_sentence_is_measured_as_one_text() {
    let mut fake = Fake { asked: Vec::new() };
    let out = LayoutState::new().lay_out_text(&on_screen(sentence()), 800.0, 600.0, &[], &mut fake);
    let text = out.paints.iter().find_map(|p| if let Paint::Text(t) = p { Some(t) } else { None }).unwrap();
    assert_eq!((text.width, text.height), (100.0, 20.0));
}

/// A plain closure knows only sizes: the box sits at the text's corner and
/// nothing is tappable, rather than anything landing somewhere wrong.
#[test]
fn a_measure_that_cannot_place_leaves_parts_at_the_corner() {
    let mut measure = |_: &TextContent, _: Option<f32>| (100.0, 20.0);
    let out = layout(&on_screen(sentence()), 800.0, 600.0, &mut measure);
    let text = out.paints.iter().find_map(|p| if let Paint::Text(t) = p { Some(t) } else { None }).unwrap();
    let path = out.paints.iter().find_map(|p| if let Paint::Path(t) = p { Some(t) } else { None }).unwrap();
    assert_eq!((path.x, path.y), (text.x, text.y));
    assert!(out.hits.is_empty());
}

/// A run with a `dir` of its own is wrapped in the isolate controls inside
/// its own range, after the spaces were collapsed, so it is ordered on its own
/// and the spaces around it are what they would have been.
#[test]
fn an_isolated_run_is_wrapped_in_its_controls() {
    let mut s = Node::text(Style::default(), words("", WhiteSpace::Normal));
    s.children.push(run("say "));
    let mut name = run("שלום");
    name.text.as_mut().unwrap().isolate = Isolate::Rtl;
    s.children.push(name);
    s.children.push(run(" now"));
    let (out, _) = lay_out(s);
    let tc = out.paints.iter().find_map(|p| if let Paint::Text(t) = p { Some(&t.content) } else { None }).unwrap();
    assert_eq!(tc.text, "say \u{2067}שלום\u{2069} now");
    let rich = tc.rich.as_ref().unwrap();
    assert_eq!(&tc.text[rich.runs[1].range.clone()], "שלום", "the run's words, the controls beside them");
}
