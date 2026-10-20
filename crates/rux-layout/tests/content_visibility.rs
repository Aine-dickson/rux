//! `content-visibility` and `contain-intrinsic-size`: a box far from the
//! screen keeps its place and skips its contents.

use rux_layout::*;

fn colour(r: f32) -> Option<Background> {
    Some(Background::Color(Rgba::new(r, 0.0, 0.0, 1.0)))
}

/// A 20px-high row with one 20px child, `content-visibility: auto`.
fn row(i: usize) -> Node {
    let mut r = Node::new(Style {
        content_visibility: ContentVisibility::Auto,
        contain_intrinsic: Some(Box::new([IntrinsicSize::default(), IntrinsicSize { auto: true, len: Some(10.0) }])),
        shrink: 0.0,
        background: colour(0.5),
        ..Default::default()
    });
    let mut cell = Node::new(Style { height: Some(Len::Px(20.0)), background: colour(0.25), ..Default::default() });
    cell.key = Some(format!("cell{i}"));
    r.children.push(cell);
    r
}

/// `n` rows in a 600px scroller.
fn list(n: usize) -> Node {
    let mut root = Node::new(Style::default());
    let mut scroller = Node::new(Style {
        overflow: Overflow::Scroll,
        height: Some(Len::Px(600.0)),
        display: Display::Flex,
        axis: Axis::Column,
        ..Default::default()
    });
    for i in 0..n {
        scroller.children.push(row(i));
    }
    root.children.push(scroller);
    root
}

fn cells(out: &Layout) -> usize {
    out.paints
        .iter()
        .filter(|p| matches!(p, Paint::Rect(PaintRect { background: Some(Background::Color(c)), .. }) if c.r == 0.25))
        .count()
}

fn row_heights(out: &Layout) -> Vec<f32> {
    out.paints
        .iter()
        .filter_map(|p| match p {
            Paint::Rect(PaintRect { background: Some(Background::Color(c)), height, .. }) if c.r == 0.5 => Some(*height),
            _ => None,
        })
        .collect()
}

#[test]
fn only_rows_near_the_screen_have_contents() {
    let mut state = LayoutState::new();
    let mut measure = |_: &TextContent, _: Option<f32>| (10.0, 10.0);
    // The first frame knows nothing, so it lays out every row as its 10px
    // stand-in, finds which are on screen, and lays out again: more rows
    // than end up needed, but none on screen without its contents.
    let first = state.layout_scrolled(&list(1000), 800.0, 600.0, &[], &mut measure);
    assert!(first.contained.iter().filter(|c| c.on_screen).all(|c| !c.skipped));
    assert_eq!(first.contained.len(), 1000);
    // From then on, the 30 on screen and the 15 within half a window below.
    let out = state.layout_scrolled(&list(1000), 800.0, 600.0, &[], &mut measure);
    assert!((30..=48).contains(&cells(&out)), "{} cells drawn", cells(&out));
    assert!(out.contained.iter().filter(|c| c.on_screen).all(|c| !c.skipped));
    let h = row_heights(&out);
    assert_eq!(h.len(), 1000, "every row keeps its place");
    assert_eq!((h[0], h[999]), (20.0, 10.0), "drawn at its size, skipped at its stand-in");
}

#[test]
fn a_row_scrolled_away_keeps_the_size_it_was_drawn_at() {
    let mut state = LayoutState::new();
    let mut measure = |_: &TextContent, _: Option<f32>| (10.0, 10.0);
    for _ in 0..2 {
        state.layout_scrolled(&list(200), 800.0, 600.0, &[], &mut measure);
    }
    // Scrolled far down: the first rows are skipped now, and with
    // `contain-intrinsic-size: auto` keep the 20px they were drawn at.
    let offsets = [Offset { x: 0.0, y: 2000.0 }];
    for _ in 0..3 {
        state.layout_scrolled(&list(200), 800.0, 600.0, &offsets, &mut measure);
    }
    let out = state.layout_scrolled(&list(200), 800.0, 600.0, &offsets, &mut measure);
    assert!(out.contained[0].skipped);
    assert_eq!(row_heights(&out)[0], 20.0);
    assert!(out.contained.iter().filter(|c| c.on_screen).all(|c| !c.skipped));
}

#[test]
fn hidden_is_always_skipped_and_contents_are_never_measured() {
    let mut root = list(3);
    let text = Node::text(
        Style::default(),
        TextContent {
            text: "never measured".into(),
            font_size: 12.0,
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
            shadows: None,
            selection_style: Default::default(),
            look: None,
            rich: None,
            white_space: Default::default(),
            direction: None,
            isolate: Default::default(),
        },
    );
    root.children[0].children[0].children.push(text);
    for r in &mut root.children[0].children {
        r.style.content_visibility = ContentVisibility::Hidden;
    }
    let mut asked = 0;
    let mut measure = |_: &TextContent, _: Option<f32>| {
        asked += 1;
        (10.0, 10.0)
    };
    let out = LayoutState::new().layout_scrolled(&root, 800.0, 600.0, &[], &mut measure);
    assert_eq!(cells(&out), 0);
    assert_eq!(asked, 0);
    assert_eq!(row_heights(&out), vec![10.0; 3]);
}
