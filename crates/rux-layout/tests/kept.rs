//! A kept layout is the whole one: track (g).4 of `docs/11-next.md`.
//!
//! [`LayoutState`] keeps the Taffy tree between frames and touches only what
//! changed, so Taffy can answer for the rest from its cache. Wrong here is
//! silent and looks like anything: a row drawn where it was a frame ago, text
//! wrapped at last frame's width, a dropdown laid out under a box it left.
//! Every case lays out a sequence of trees through one kept state and checks
//! each frame against a layout built whole, paints, regions and metrics.
//! (Debug builds also check inside `layout_scrolled` itself.)

use std::cell::Cell;

use rux_layout::*;

const CHAR_W: f32 = 8.0;
const LINE_H: f32 = 12.0;

/// Greedy word wrap at whatever width it is given, so a stale width or cap
/// shows up as a wrong height.
fn wrap(text: &str, max: Option<f32>) -> (f32, f32) {
    let width = |s: &str| s.chars().count() as f32 * CHAR_W;
    let Some(max) = max else {
        return (width(text), LINE_H);
    };
    let (mut lines, mut line, mut widest) = (1.0, 0.0f32, 0.0f32);
    for word in text.split(' ') {
        let w = width(word);
        let next = if line == 0.0 { w } else { line + CHAR_W + w };
        if next > max && line > 0.0 {
            lines += 1.0;
            widest = widest.max(line);
            line = w;
        } else {
            line = next;
        }
    }
    (widest.max(line), lines * LINE_H)
}

fn text(s: &str) -> TextContent {
    TextContent {
        text: s.to_string(),
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
        selection_style: Default::default(),
        rich: None,
        white_space: Default::default(),
        direction: None,
    }
}

fn boxed(style: Style, children: Vec<Node>) -> Node {
    let mut n = Node::new(style);
    n.children = children;
    n
}

fn label(s: &str) -> Node {
    Node::text(Style::default(), text(s))
}

fn column() -> Style {
    Style { display: Display::Flex, axis: Axis::Column, ..Default::default() }
}

fn flex_row() -> Style {
    Style { display: Display::Flex, axis: Axis::Row, ..Default::default() }
}

/// Lays each `(tree, width, height)` out in turn through one kept state,
/// checking every frame against a whole layout. Returns how many times each
/// frame's kept layout called `measure`.
fn frames(trees: &[(Node, f32, f32)]) -> Vec<usize> {
    let mut kept = LayoutState::new();
    let calls = Cell::new(0usize);
    let mut counts = Vec::new();
    for (i, (tree, w, h)) in trees.iter().enumerate() {
        let mut whole_calls = 0;
        let whole = layout(tree, *w, *h, &mut |tc, max| {
            whole_calls += 1;
            wrap(&tc.text, max)
        });
        calls.set(0);
        let out = kept.layout_scrolled(tree, *w, *h, &[], &mut |tc, max| {
            calls.set(calls.get() + 1);
            wrap(&tc.text, max)
        });
        // Debug builds lay the frame out whole inside, with the same measure.
        let own = calls.get() - if cfg!(debug_assertions) { whole_calls } else { 0 };
        counts.push(own);
        assert_eq!(
            format!("{out:#?}"),
            format!("{whole:#?}"),
            "frame {i}: the kept layout differs from the whole one"
        );
    }
    counts
}

fn keyed_rows(order: &[usize]) -> Node {
    let rows = order
        .iter()
        .map(|&n| {
            let mut row = boxed(
                Style { padding: Sides::uniform(4.0), ..flex_row() },
                vec![label(&format!("row {n}")), label(&format!("some words for row {n}"))],
            );
            row.key = Some(n.to_string());
            row
        })
        .collect();
    boxed(column(), vec![label("title"), boxed(column(), rows)])
}

/// The case the track is for: a keyed list rotated a row a frame. Every row
/// is the same box with the same text, so nothing is measured again.
#[test]
fn a_rotated_list_measures_nothing_again() {
    let mut order: Vec<usize> = (0..30).collect();
    let mut trees = Vec::new();
    for _ in 0..5 {
        trees.push((keyed_rows(&order), 400.0, 300.0));
        order.rotate_left(1);
    }
    let counts = frames(&trees);
    assert!(counts[0] > 0, "the first frame measures every text");
    assert_eq!(&counts[1..], &[0, 0, 0, 0], "a rotation reuses every measured text");
}

/// One row's text changing is measured again; the others are not.
#[test]
fn a_changed_text_alone_is_measured_again() {
    let order: Vec<usize> = (0..10).collect();
    let before = keyed_rows(&order);
    let mut after = keyed_rows(&order);
    after.children[1].children[3].children[1] = label("some different and much longer words for this row");
    let counts = frames(&[(before, 400.0, 300.0), (after, 400.0, 300.0)]);
    assert!(counts[1] > 0 && counts[1] < counts[0], "{counts:?}");
}

/// A colour is not a size: changing one measures nothing.
#[test]
fn a_colour_change_measures_nothing() {
    let order: Vec<usize> = (0..10).collect();
    let before = keyed_rows(&order);
    let mut after = keyed_rows(&order);
    let t = after.children[1].children[2].children[0].text.as_mut().unwrap();
    t.color = Rgba::new(1.0, 0.0, 0.0, 1.0);
    let counts = frames(&[(before, 400.0, 300.0), (after, 400.0, 300.0)]);
    assert_eq!(counts[1], 0);
}

/// The measure hook wraps a text at the narrowest any ancestor allows, which
/// Taffy cannot see. Here the grandparent narrows and the text's own box,
/// style and content stay as they were.
#[test]
fn a_narrower_ancestor_rewraps_the_text_under_it() {
    let tree = |w: f32| {
        boxed(
            column(),
            vec![boxed(
                Style { max_width: Some(Len::Px(w)), ..column() },
                vec![boxed(flex_row(), vec![label("a long run of words that wraps at the cap")])],
            )],
        )
    };
    frames(&[(tree(300.0), 400.0, 300.0), (tree(120.0), 400.0, 300.0), (tree(300.0), 400.0, 300.0)]);
}

/// Out-of-flow boxes are laid out under their containing block, not their
/// parent, so a kept one can change Taffy parents between frames.
#[test]
fn an_absolute_box_moves_between_containing_blocks() {
    let pinned = || Node::new(Style {
        position: Position::Absolute,
        inset: [Some(Len::Px(5.0)), None, None, Some(Len::Px(5.0))],
        width: Some(Len::Px(20.0)),
        height: Some(Len::Px(20.0)),
        ..Default::default()
    });
    let tree = |relative: bool, fixed: bool| {
        let mut p = pinned();
        if fixed {
            p.style.position = Position::Fixed;
        }
        let holder = Style {
            position: if relative { Position::Relative } else { Position::Static },
            height: Some(Len::Px(100.0)),
            margin: Sides::uniform(30.0),
            ..column()
        };
        boxed(column(), vec![label("top"), boxed(holder, vec![label("inside"), p])])
    };
    frames(&[
        (tree(true, false), 400.0, 300.0),
        (tree(false, false), 400.0, 300.0),
        (tree(true, false), 400.0, 300.0),
        (tree(true, true), 400.0, 300.0),
        (tree(true, false), 400.0, 300.0),
    ]);
}

/// A subtree removed while holding a box hoisted out of it: the box's Taffy
/// parent is an ancestor that stays, and has to be laid out again without it.
#[test]
fn a_removed_subtree_takes_its_hoisted_box_with_it() {
    let tree = |with: bool| {
        let pinned = Node::new(Style {
            position: Position::Absolute,
            inset: [Some(Len::Px(0.0)), None, None, Some(Len::Px(0.0))],
            width: Some(Len::Px(50.0)),
            height: Some(Len::Px(50.0)),
            ..Default::default()
        });
        let mut kids = vec![label("stays")];
        if with {
            kids.push(boxed(column(), vec![label("goes"), pinned]));
        }
        kids.push(label("after"));
        boxed(Style { position: Position::Relative, ..column() }, vec![boxed(column(), kids)])
    };
    frames(&[(tree(true), 400.0, 300.0), (tree(false), 400.0, 300.0), (tree(true), 400.0, 300.0)]);
}

/// An absolute box with no insets is laid out in extra passes that pin it, so
/// what is kept in the tree is not what this frame's style says.
#[test]
fn a_static_position_box_is_found_again_each_frame() {
    let tree = |above: &str| {
        let floating = boxed(
            Style { position: Position::Absolute, ..column() },
            vec![label("floating")],
        );
        boxed(
            Style { position: Position::Relative, ..column() },
            vec![label(above), floating, label("below")],
        )
    };
    frames(&[
        (tree("short"), 200.0, 300.0),
        (tree("a much longer line above that wraps onto more lines than before"), 200.0, 300.0),
        (tree("short"), 200.0, 300.0),
    ]);
}

/// A node can change what it is: a box becomes text, text becomes a box with
/// children, and an unkeyed child inserted at the front shifts every match.
#[test]
fn a_node_that_changes_kind_is_laid_out_as_what_it_is_now() {
    let a = boxed(column(), vec![boxed(column(), vec![label("one"), label("two")]), label("three")]);
    let b = boxed(column(), vec![label("now text"), boxed(flex_row(), vec![label("x"), label("y")])]);
    let c = boxed(column(), vec![label("new first"), label("now text"), boxed(flex_row(), vec![label("x")])]);
    frames(&[(a.clone(), 300.0, 200.0), (b, 300.0, 200.0), (c, 300.0, 200.0), (a, 300.0, 200.0)]);
}

/// The window resizing, `display: none` toggling and two rows with one key.
#[test]
fn resizes_hidden_boxes_and_duplicate_keys() {
    let tree = |hide: bool| {
        let mut a = boxed(flex_row(), vec![label("same key, first")]);
        a.key = Some("k".into());
        let mut b = boxed(flex_row(), vec![label("same key, second")]);
        b.key = Some("k".into());
        let mut gone = boxed(column(), vec![label("sometimes hidden")]);
        if hide {
            gone.style.display = Display::None;
        }
        boxed(column(), vec![a, gone, b])
    };
    frames(&[
        (tree(false), 400.0, 300.0),
        (tree(true), 400.0, 300.0),
        (tree(true), 150.0, 300.0),
        (tree(false), 150.0, 500.0),
        (tree(false), 400.0, 300.0),
    ]);
}

/// A small seeded generator, so a failure names a seed and reproduces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, in_n: u64) -> bool {
        self.below(in_n) == 0
    }
}

const WORDS: [&str; 8] = ["a", "row", "of", "words", "that", "wraps", "sometimes", "longer"];

fn random_text(r: &mut Rng) -> String {
    (0..1 + r.below(6)).map(|_| WORDS[r.below(8) as usize]).collect::<Vec<_>>().join(" ")
}

fn random_style(r: &mut Rng) -> Style {
    let mut s = match r.below(4) {
        0 => flex_row(),
        1 => Style { display: Display::Block, ..Default::default() },
        _ => column(),
    };
    if r.chance(4) {
        s.width = Some(Len::Px(40.0 + r.below(200) as f32));
    }
    if r.chance(5) {
        s.max_width = Some(Len::Pct(0.5 + r.below(5) as f32 / 10.0));
    }
    if r.chance(4) {
        s.padding = Sides::uniform(r.below(10) as f32);
    }
    if r.chance(6) {
        s.position = Position::Relative;
    }
    if r.chance(10) {
        s.position = Position::Absolute;
        if r.chance(2) {
            s.inset = [Some(Len::Px(r.below(30) as f32)), None, None, Some(Len::Px(r.below(30) as f32))];
        }
    }
    if r.chance(25) {
        s.position = Position::Fixed;
        s.inset = [None, Some(Len::Px(0.0)), Some(Len::Px(0.0)), None];
    }
    if r.chance(15) {
        s.display = Display::None;
    }
    s
}

fn random_node(r: &mut Rng, depth: u32) -> Node {
    if depth == 0 || r.chance(3) {
        let mut t = Node::text(random_style(r), text(&random_text(r)));
        if r.chance(2) {
            t.style.display = Display::Block;
        }
        return t;
    }
    let n = r.below(5) as usize;
    let keyed = r.chance(2);
    let mut kids: Vec<Node> = (0..n).map(|_| random_node(r, depth - 1)).collect();
    if keyed {
        for (i, k) in kids.iter_mut().enumerate() {
            k.key = Some(format!("k{}", r.below(3) * 100 + i as u64));
        }
    }
    boxed(random_style(r), kids)
}

/// One random change somewhere in the tree.
fn mutate(r: &mut Rng, node: &mut Node, depth: u32) {
    if !node.children.is_empty() && r.below(3) != 0 {
        let i = r.below(node.children.len() as u64) as usize;
        return mutate(r, &mut node.children[i], depth + 1);
    }
    match r.below(8) {
        0 => node.style = random_style(r),
        1 => {
            if let Some(t) = node.text.as_mut() {
                t.text = random_text(r);
            } else {
                node.style.width = Some(Len::Px(30.0 + r.below(300) as f32));
            }
        }
        2 if !node.children.is_empty() => node.children.rotate_left(1),
        3 if !node.children.is_empty() => {
            let i = r.below(node.children.len() as u64) as usize;
            node.children.remove(i);
        }
        4 if node.text.is_none() => {
            let i = r.below(node.children.len() as u64 + 1) as usize;
            node.children.insert(i, random_node(r, 2));
        }
        5 => *node = random_node(r, 3u32.saturating_sub(depth).max(1)),
        6 if node.children.len() > 1 => node.children.reverse(),
        _ => node.key = r.chance(2).then(|| format!("k{}", r.below(5))),
    }
}

/// Random trees changed one random thing a frame, with now and then a resize.
/// The cases above are the ones thought of; this is for the rest.
#[test]
fn random_changes_lay_out_as_a_whole_layout_would() {
    // 200 by default; `KEPT_SEEDS=5000` for a longer hunt.
    let seeds: u64 = std::env::var("KEPT_SEEDS").ok().and_then(|n| n.parse().ok()).unwrap_or(200);
    for seed in 1..=seeds {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut tree = boxed(column(), (0..4).map(|_| random_node(&mut r, 3)).collect());
        let mut trees = Vec::new();
        let (mut w, mut h) = (400.0, 300.0);
        for _ in 0..25 {
            trees.push((tree.clone(), w, h));
            for _ in 0..1 + r.below(3) {
                mutate(&mut r, &mut tree, 0);
            }
            if r.chance(8) {
                w = 120.0 + r.below(400) as f32;
                h = 120.0 + r.below(400) as f32;
            }
        }
        let run = std::panic::catch_unwind(|| frames(&trees));
        assert!(run.is_ok(), "seed {seed} failed");
    }
}

fn show(n: &Node, d: usize, out: &mut String) {
    let s = &n.style;
    out.push_str(&format!("{}{:?} {:?} pos={:?} w={:?} maxw={:?} pad={} inset={:?} key={:?} text={:?}
", "  ".repeat(d), s.display, s.axis, s.position, s.width, s.max_width, s.padding.left, s.inset, n.key, n.text.as_ref().map(|t| t.text.clone())));
    for c in &n.children { show(c, d + 1, out); }
}

#[test]
#[ignore]
fn seed_dump() {
    let seed: u64 = std::env::var("SEED").unwrap().parse().unwrap();
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut tree = boxed(column(), (0..4).map(|_| random_node(&mut r, 3)).collect());
    let mut trees = Vec::new();
    let (mut w, mut h) = (400.0, 300.0);
    for _ in 0..25 {
        trees.push((tree.clone(), w, h));
        for _ in 0..1 + r.below(3) { mutate(&mut r, &mut tree, 0); }
        if r.chance(8) { w = 120.0 + r.below(400) as f32; h = 120.0 + r.below(400) as f32; }
    }
    for n in 1..=trees.len() {
        if std::panic::catch_unwind(|| frames(&trees[..n])).is_err() {
            for (i, (t, w, h)) in trees[n.saturating_sub(2)..n].iter().enumerate() {
                let mut o = String::new(); show(t, 0, &mut o);
                eprintln!("=== frame {} ({w}x{h})
{o}", n - 2 + i);
            }
            return;
        }
    }
}
