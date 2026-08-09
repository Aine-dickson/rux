use rux_layout::*;

fn boxed(style: Style, children: Vec<Node>) -> Node {
    let mut n = Node::new(style);
    n.children = children;
    n
}

const CHAR_W: f32 = 10.0;
const LINE_H: f32 = 15.0;

/// Text with nothing set on it. `TextContent` has no `Default`, deliberately:
/// the colour and size defaults are the cascade's policy and live in
/// `rux-style`, not here.
fn plain_text(text: &str) -> TextContent {
    TextContent {
        text: text.to_string(),
        font_size: 13.0,
        weight: 400,
        color: Rgba::new(1.0, 1.0, 1.0, 1.0),
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
    }
}

/// A text measurer that wraps like a real one: greedy, by word, at whatever
/// width it is given. A stub that always answers with one line cannot catch a
/// box sized for fewer lines than get drawn, which is the whole failure this
/// file exists to pin down.
fn wrapping_measure(tc: &TextContent, max: Option<f32>) -> (f32, f32) {
    let width_of = |s: &str| s.chars().count() as f32 * CHAR_W;
    let Some(max) = max else {
        return (width_of(&tc.text), LINE_H); // unconstrained: one long line
    };
    let mut lines = 1;
    let mut widest: f32 = 0.0;
    let mut line = String::new();
    for word in tc.text.split_whitespace() {
        let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if width_of(&candidate) > max && !line.is_empty() {
            widest = widest.max(width_of(&line));
            line = word.to_string();
            lines += 1;
        } else {
            line = candidate;
        }
    }
    widest = widest.max(width_of(&line));
    (widest, lines as f32 * LINE_H)
}

fn all_rects(root: &Node, w: f32, h: f32) -> Vec<(f32, f32, f32, f32)> {
    let mut measure = |_: &rux_layout::TextContent, _: Option<f32>| (0.0, 0.0);
    layout(root, w, h, &mut measure)
        .paints
        .iter()
        .filter_map(|p| match p {
            Paint::Rect(r) => Some((r.x, r.y, r.width, r.height)),
            _ => None,
        })
        .collect()
}

/// A card capped by `max-width`, holding text longer than one line, must be
/// tall enough for every line that gets drawn.
///
/// The bug this pins: text was only ever measured at max-content (one long
/// line) or min-content, and min-content answered with that same single-line
/// width. Taffy was therefore told the text could never be narrower than 454px,
/// never asked it to measure at a definite width, and sized the card's height
/// for one line. Paint then wrapped the text to the card's real inner width and
/// drew three lines, two of them outside the border. Same shape as every other
/// bug in this project: two places computing the same thing, disagreeing.
///
/// Min-content for text is the longest unbreakable word, which is what wrapping
/// at zero reports.
#[test]
fn a_capped_box_is_tall_enough_for_the_text_it_draws() {
    let text = "This card is styled entirely by the shared sheet. Nothing about it is written here.";
    let card = boxed(
        Style {
            display: Display::Flex,
            axis: Axis::Column,
            padding: Sides { top: 16.0, right: 16.0, bottom: 16.0, left: 16.0 },
            max_width: Some(Len::Px(260.0)),
            background: Some(Background::Color(Rgba::new(0.2, 0.2, 0.3, 1.0))),
            ..Default::default()
        },
        vec![Node::text(Style::default(), plain_text(text))],
    );
    let screen = boxed(
        Style { display: Display::Flex, axis: Axis::Row, ..Default::default() },
        vec![card],
    );

    let mut measure = wrapping_measure;
    let out = layout(&screen, 1000.0, 800.0, &mut measure);

    let card_rect = out
        .paints
        .iter()
        .find_map(|p| match p {
            Paint::Rect(r) if r.background.is_some() => Some(r.clone()),
            _ => None,
        })
        .expect("the card paints a background");
    let text_rect = out
        .paints
        .iter()
        .find_map(|p| match p {
            Paint::Text(t) => Some(t.clone()),
            _ => None,
        })
        .expect("the text paints");

    // What the text will actually occupy at the width it was finally given.
    let (_, drawn_height) = wrapping_measure(&text_rect.content, Some(text_rect.width));
    assert!(
        drawn_height > LINE_H,
        "the width should force a wrap, or this proves nothing (width {})",
        text_rect.width
    );
    assert!(
        text_rect.y + drawn_height <= card_rect.y + card_rect.height + 0.5,
        "text of {drawn_height}px starting at y={} spills past the card, which ends at {}",
        text_rect.y,
        card_rect.y + card_rect.height
    );
}

/// A `flex-wrap` grid whose width is a percentage (`width: 100%`) sits in a
/// flex column, followed by a sibling. The grid holds eight 64px boxes and, at
/// this width, must wrap onto two rows. The bug: taffy measures the grid's
/// height as if everything fits on one row (max-content), so the column places
/// the sibling as if the grid were one row tall, while the grid actually paints
/// the 8th box on a second row, under the sibling. The sibling must sit below
/// the whole grid, not on top of the wrapped row.
#[test]
fn wrapped_grid_reserves_height_for_every_row() {
    let thumb = || {
        boxed(
            Style {
                width: Some(Len::Px(64.0)),
                height: Some(Len::Px(64.0)),
                shrink: 0.0,
                background: Some(Background::Color(Rgba::new(0.5, 0.5, 0.5, 1.0))),
                ..Default::default()
            },
            vec![],
        )
    };
    let grid = boxed(
        Style {
            display: Display::Flex,
            axis: Axis::Row,
            wrap: true,
            gap: 8.0,
            width: Some(Len::Pct(1.0)),
            max_width: Some(Len::Px(520.0)),
            ..Default::default()
        },
        (0..8).map(|_| thumb()).collect(),
    );
    // A distinct-width sentinel so we can pick it out of the paint list.
    let sentinel = boxed(
        Style {
            width: Some(Len::Px(200.0)),
            height: Some(Len::Px(20.0)),
            background: Some(Background::Color(Rgba::new(1.0, 0.0, 0.0, 1.0))),
            ..Default::default()
        },
        vec![],
    );
    let screen = boxed(
        Style {
            display: Display::Flex,
            axis: Axis::Column,
            gap: 12.0,
            ..Default::default()
        },
        vec![grid, sentinel],
    );

    let rects = all_rects(&screen, 1260.0, 790.0);
    // Only the thumbs (64px) and the sentinel (200px) have a background, so
    // those are the only rects. The grid and screen paint nothing themselves.
    let thumbs: Vec<_> = rects.iter().filter(|r| r.2 == 64.0).collect();
    let sentinel = *rects.iter().find(|r| r.2 == 200.0).expect("sentinel");
    assert_eq!(thumbs.len(), 8, "all eight thumbs should paint");

    let thumbs_bottom = thumbs
        .iter()
        .map(|t| t.1 + t.3)
        .fold(0.0_f32, f32::max);

    // At 520px the row fits 7 thumbs, so the 8th wraps: two rows, ~136px tall.
    assert!(
        thumbs_bottom > 100.0,
        "expected the thumbs to wrap onto a second row (bottom {thumbs_bottom})"
    );
    // The sentinel must sit below the whole grid, not overlap the wrapped row.
    assert!(
        sentinel.1 >= thumbs_bottom - 0.5,
        "sentinel (y={}) overlaps the wrapped thumbnails (bottom {thumbs_bottom})",
        sentinel.1
    );
}
