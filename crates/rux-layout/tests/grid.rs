//! Grid templates: `repeat()`, named lines and `grid-template-areas`.

use rux_layout::*;

fn colour(r: f32) -> Option<Background> {
    Some(Background::Color(Rgba::new(r, 0.0, 0.0, 1.0)))
}

fn cell(r: f32) -> Node {
    Node::new(Style { background: colour(r), height: Some(Len::Px(20.0)), ..Default::default() })
}

fn grid(style: Style, cells: Vec<Node>) -> Node {
    let mut root = Node::new(Style::default());
    let mut g = Node::new(Style { display: Display::Grid, width: Some(Len::Px(300.0)), ..style });
    g.children = cells;
    root.children.push(g);
    root
}

fn rect(out: &Layout, r: f32) -> Option<(f32, f32, f32, f32)> {
    out.paints.iter().find_map(|p| match p {
        Paint::Rect(PaintRect { background: Some(Background::Color(c)), x, y, width, height, .. }) if c.r == r => {
            Some((*x, *y, *width, *height))
        }
        _ => None,
    })
}

fn lay(root: &Node) -> Layout {
    let mut measure = |_: &TextContent, _: Option<f32>| (10.0, 10.0);
    layout(root, 800.0, 600.0, &mut measure)
}

#[test]
fn repeat_auto_fill_fits_as_many_as_there_is_room_for() {
    // 300px of 100px columns: three, so the fourth cell starts row two.
    let style = Style {
        grid_columns: vec![GridTrack::Repeat(Repeat::AutoFill, vec![Track::Px(100.0)], Vec::new())],
        ..Default::default()
    };
    let out = lay(&grid(style, vec![cell(0.1), cell(0.2), cell(0.3), cell(0.4)]));
    assert_eq!(rect(&out, 0.3).map(|r| (r.0, r.1)), Some((200.0, 0.0)));
    assert_eq!(rect(&out, 0.4).map(|r| (r.0, r.1)), Some((0.0, 20.0)));
}

#[test]
fn repeat_a_count_of_tracks() {
    let style = Style {
        grid_columns: vec![
            GridTrack::One(Track::Px(50.0)),
            GridTrack::Repeat(Repeat::Count(2), vec![Track::Fr(1.0)], Vec::new()),
        ],
        ..Default::default()
    };
    let out = lay(&grid(style, vec![cell(0.1), cell(0.2), cell(0.3)]));
    assert_eq!(rect(&out, 0.2).map(|r| (r.0, r.2)), Some((50.0, 125.0)));
    assert_eq!(rect(&out, 0.3).map(|r| (r.0, r.2)), Some((175.0, 125.0)));
}

#[test]
fn areas_place_items_by_name() {
    // "head head" / "side main"
    let n = GridName::new;
    let style = Style {
        grid_columns: vec![GridTrack::One(Track::Px(100.0)), GridTrack::One(Track::Fr(1.0))],
        grid_names: Some(Box::new(GridNames {
            areas: Some(GridAreas {
                areas: vec![(n("head"), 1, 2, 1, 3), (n("side"), 2, 3, 1, 2), (n("main"), 2, 3, 2, 3)],
                rows: 2,
                columns: 2,
            }),
            ..Default::default()
        })),
        ..Default::default()
    };
    let mut main = cell(0.3);
    main.style.grid_row = (GridPlace::Named(n("main"), 0), GridPlace::Named(n("main"), 0));
    main.style.grid_column = (GridPlace::Named(n("main"), 0), GridPlace::Named(n("main"), 0));
    let mut head = cell(0.1);
    head.style.grid_row = (GridPlace::Named(n("head"), 0), GridPlace::Named(n("head"), 0));
    head.style.grid_column = (GridPlace::Named(n("head"), 0), GridPlace::Named(n("head"), 0));
    // Written first, placed last: the areas decide.
    let out = lay(&grid(style, vec![main, head]));
    assert_eq!(rect(&out, 0.1), Some((0.0, 0.0, 300.0, 20.0)));
    assert_eq!(rect(&out, 0.3), Some((100.0, 20.0, 200.0, 20.0)));
}

#[test]
fn named_lines_place_an_item() {
    // [start] 100px [mid] 1fr [end]
    let n = GridName::new;
    let style = Style {
        grid_columns: vec![GridTrack::One(Track::Px(100.0)), GridTrack::One(Track::Fr(1.0))],
        grid_names: Some(Box::new(GridNames {
            columns: vec![vec![n("start")], vec![n("mid")], vec![n("end")]],
            ..Default::default()
        })),
        ..Default::default()
    };
    let mut c = cell(0.2);
    c.style.grid_column = (GridPlace::Named(n("mid"), 0), GridPlace::Named(n("end"), 0));
    let out = lay(&grid(style, vec![c]));
    assert_eq!(rect(&out, 0.2).map(|r| (r.0, r.2)), Some((100.0, 200.0)));
}
