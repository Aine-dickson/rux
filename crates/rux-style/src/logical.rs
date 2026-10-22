//! Logical properties: a side named by the direction, not by the box.
//!
//! `margin-inline-start` is the left margin of a left-to-right box and the
//! right margin of a right-to-left one, which is what lets one stylesheet serve
//! both: the gap between an icon and its label stays on the label's side when
//! the row is mirrored. Rux writes horizontally only, so the block sides are
//! always top and bottom.
//!
//! Each logical declaration becomes the physical one(s) it stands for, at the
//! moment the cascade meets it. Order matters exactly as it does between two
//! physical declarations: `margin-left: 8px` after `margin-inline-start: 4px`
//! wins for a left-to-right box and leaves the start margin alone for a
//! right-to-left one, whose start is its right.

use std::collections::HashMap;

use rux_layout::Direction;

use crate::Rule;

/// The physical declarations a logical one stands for, `None` for anything
/// that is not logical.
pub(crate) fn expand(name: &str, value: &str, dir: Direction) -> Option<Vec<(String, String)>> {
    let (start, end) = match dir {
        Direction::Ltr => ("left", "right"),
        Direction::Rtl => ("right", "left"),
    };
    // `inset-*` sides are the bare `left`/`top`; the others are `margin-left`.
    let (prefix, rest) = ["margin", "padding", "inset"].iter().find_map(|p| {
        let rest = name.strip_prefix(p)?.strip_prefix('-')?;
        Some((*p, rest))
    })?;
    let side = |s: &str| if prefix == "inset" { s.to_string() } else { format!("{prefix}-{s}") };
    let one = |s: &str| Some(vec![(side(s), value.to_string())]);
    match rest {
        "inline-start" => one(start),
        "inline-end" => one(end),
        "block-start" => one("top"),
        "block-end" => one("bottom"),
        // One value for both sides, or the start's then the end's. Split
        // outside brackets: `calc(var(--spacing) * 2)` is one value, and
        // split on every space it was three, the sides `calc(var(--spacing)`
        // and `*` (found under Tailwind's `py-2`).
        "inline" | "block" => {
            let parts = crate::split_top_level(value);
            let mut parts = parts.into_iter();
            let a = parts.next()?.to_string();
            let b = parts.next().map_or_else(|| a.clone(), str::to_string);
            let (s, e) = if rest == "inline" { (start, end) } else { ("top", "bottom") };
            Some(vec![(side(s), a), (side(e), b)])
        }
        _ => None,
    }
}

pub(crate) fn is_logical(name: &str) -> bool {
    expand(name, "0", Direction::Ltr).is_some()
}

/// The cascade again, in order: each rule's declarations, then the inline
/// ones, with every logical name put on its side as it is met.
pub(crate) fn cascade_in_order(rules: Vec<&Rule>, inline: &[(String, String)], dir: Direction) -> HashMap<String, String> {
    let mut props: HashMap<String, String> = HashMap::new();
    let decls = rules.into_iter().flat_map(|r| r.decls.iter()).chain(inline.iter());
    for (k, v) in decls {
        match expand(k, v, dir) {
            Some(physical) => {
                for (pk, pv) in physical {
                    props.insert(pk, pv);
                }
            }
            None => crate::declare(&mut props, k, v),
        }
    }
    props
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(v: Option<Vec<(String, String)>>) -> Vec<(String, String)> {
        v.expect("logical")
    }

    #[test]
    fn inline_sides_follow_the_direction() {
        assert_eq!(pairs(expand("margin-inline-start", "4px", Direction::Ltr)), [("margin-left".into(), "4px".into())]);
        assert_eq!(pairs(expand("margin-inline-start", "4px", Direction::Rtl)), [("margin-right".into(), "4px".into())]);
        assert_eq!(pairs(expand("padding-inline-end", "2px", Direction::Rtl)), [("padding-left".into(), "2px".into())]);
        assert_eq!(pairs(expand("inset-inline-start", "0", Direction::Rtl)), [("right".into(), "0".into())]);
    }

    #[test]
    fn block_sides_are_top_and_bottom() {
        assert_eq!(pairs(expand("margin-block-start", "1px", Direction::Rtl)), [("margin-top".into(), "1px".into())]);
        assert_eq!(pairs(expand("inset-block-end", "3px", Direction::Ltr)), [("bottom".into(), "3px".into())]);
    }

    #[test]
    fn shorthands_take_one_value_or_two() {
        assert_eq!(
            pairs(expand("margin-inline", "4px 8px", Direction::Rtl)),
            [("margin-right".into(), "4px".into()), ("margin-left".into(), "8px".into())]
        );
        assert_eq!(
            pairs(expand("padding-block", "6px", Direction::Ltr)),
            [("padding-top".into(), "6px".into()), ("padding-bottom".into(), "6px".into())]
        );
        // A value with spaces inside its brackets is one value.
        let calc = "calc(var(--spacing) * 2)";
        assert_eq!(
            pairs(expand("padding-block", calc, Direction::Ltr)),
            [("padding-top".into(), calc.into()), ("padding-bottom".into(), calc.into())]
        );
        assert_eq!(
            pairs(expand("margin-inline", "calc(1px + 2px) auto", Direction::Ltr)),
            [("margin-left".into(), "calc(1px + 2px)".into()), ("margin-right".into(), "auto".into())]
        );
    }

    #[test]
    fn physical_names_are_not_logical() {
        for name in ["margin", "margin-left", "padding-top", "inset", "left", "margin-inline-middle"] {
            assert!(!is_logical(name), "{name}");
        }
    }
}
