//! Tabler's icons, as path data Rux can draw.
//!
//! **An author never sees this crate and never downloads an icon.** The data is
//! generated once from a Tabler checkout and committed, by
//! `cargo run -p rux-icons --bin generate`. What arrives here is one string and
//! one integer table; see the generator for why that shape and not a map of
//! string literals.
//!
//! # What an icon is
//!
//! A **list of paths on a 24 by 24 grid**, in two variants. Outline is stroked,
//! filled is solid, and **filled is a strict subset**: roughly four icons in
//! five exist in outline only, which is why the default variant is outline and
//! why asking for a filled icon that has none is an error rather than a
//! fallback. The generator refuses to emit a filled icon with no outline
//! counterpart, so that subset relationship is enforced rather than assumed.
//!
//! # Paint is per path
//!
//! The variant decides how an icon is painted, and then **individual paths
//! override it**. `accessible` is stroked except for the dot on the head, which
//! is solid; `percentage-25` is stroked except for the pie slice, which is
//! filled with no outline. Around ninety icons do this. [`IconPath`] carries
//! those overrides, and anything drawing an icon has to honour them or those
//! icons come out visibly wrong.
//!
//! Everything else about paint is the author's: `fill`, `stroke` and
//! `stroke-width` are CSS in Rux and cascade like any other property, so an
//! icon takes its colour from the text around it with no plumbing here.

mod generated;

pub use generated::TABLER_VERSION;

/// The grid every icon is drawn on.
///
/// Tabler, Lucide and Phosphor all use 24, but nothing here assumes that of
/// anyone else: this is Tabler's, and it is what an element scales from to
/// reach the size an author asked for.
pub const GRID: f32 = 24.0;

/// Which artwork to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Variant {
    /// Stroked. The default, because four icons in five have nothing else.
    #[default]
    Outline,
    /// Solid. Present for roughly one icon in five.
    Filled,
}

/// One path of an icon, and the paint it insists on for itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconPath {
    /// SVG path data, in the 24-unit grid.
    pub d: &'static str,
    /// `fill: currentColor` on this path, whatever the variant does.
    pub fill_current: bool,
    /// `stroke: none` on this path, whatever the variant does.
    pub no_stroke: bool,
    /// `opacity: 0.5`. One icon in the set, and losing it would be invisible.
    pub half_opacity: bool,
}

/// Look `name` up, or `None` if the set has no such icon.
///
/// Binary search over the sorted index, so the cost is a handful of string
/// comparisons rather than a scan of six thousand names.
pub fn find(name: &str, variant: Variant) -> Option<Paths> {
    let row = lookup(name)?;
    let (at, len) = match variant {
        Variant::Outline => (row[2], row[3]),
        Variant::Filled => (row[4], row[5]),
    };
    if len == 0 {
        return None;
    }
    Some(Paths { slice: &generated::BLOB[at as usize..(at + len) as usize] })
}

/// Whether the set has `name` at all, in any variant.
pub fn exists(name: &str) -> bool {
    lookup(name).is_some()
}

/// Whether `name` has filled artwork.
///
/// Separate from [`find`] so that a build can tell "no such icon" from "that
/// icon has no filled variant", which are different mistakes and deserve
/// different messages.
pub fn has_filled(name: &str) -> bool {
    lookup(name).is_some_and(|row| row[5] != 0)
}

/// Every icon name, in sorted order.
pub fn names() -> impl Iterator<Item = &'static str> {
    generated::INDEX.iter().map(|row| name_of(row))
}

/// How many icons the set holds.
pub fn count() -> usize {
    generated::INDEX.len()
}

/// The paths of one icon in one variant.
#[derive(Clone, Copy, Debug)]
pub struct Paths {
    slice: &'static str,
}

impl Paths {
    /// Walk the paths, in the order they are drawn.
    pub fn iter(&self) -> impl Iterator<Item = IconPath> {
        self.slice.split('\n').map(|entry| {
            // One hex digit of flags, then the data. The generator guarantees
            // it, and a path command is always a letter, so the two can never
            // be confused.
            let (flags, d) = entry.split_at(1);
            let flags = u8::from_str_radix(flags, 16).unwrap_or(0);
            IconPath {
                d,
                fill_current: flags & 1 != 0,
                no_stroke: flags & 2 != 0,
                half_opacity: flags & 4 != 0,
            }
        })
    }
}

impl IntoIterator for Paths {
    type Item = IconPath;
    type IntoIter = std::vec::IntoIter<IconPath>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter().collect::<Vec<_>>().into_iter()
    }
}

fn name_of(row: &[u32; 6]) -> &'static str {
    &generated::BLOB[row[0] as usize..(row[0] + row[1]) as usize]
}

fn lookup(name: &str) -> Option<&'static [u32; 6]> {
    let at = generated::INDEX.binary_search_by(|row| name_of(row).cmp(name)).ok()?;
    generated::INDEX.get(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_set_is_the_size_the_design_rests_on() {
        // The design rests on abundance and on filled being a strict subset. If
        // either changes shape, several decisions want revisiting rather than
        // this number quietly moving.
        assert!(count() > 5_000, "only {} icons", count());
        let filled = names().filter(|n| has_filled(n)).count();
        assert!(filled > 1_000, "only {filled} filled");
        assert!(filled * 3 < count(), "filled is no longer a small subset");
    }

    #[test]
    fn names_are_sorted_which_is_what_the_search_assumes() {
        let all: Vec<&str> = names().collect();
        let mut sorted = all.clone();
        sorted.sort_unstable();
        assert_eq!(all, sorted, "the index is not in sorted order");
    }

    #[test]
    fn every_icon_resolves_and_has_drawable_paths() {
        for name in names() {
            let paths = find(name, Variant::Outline)
                .unwrap_or_else(|| panic!("{name} has no outline"));
            let mut any = false;
            for p in paths.iter() {
                assert!(!p.d.is_empty(), "{name} has an empty path");
                // Every SVG path starts with a command, which is a letter. A
                // digit here would mean the flag byte had been eaten.
                assert!(
                    p.d.starts_with(|c: char| c.is_ascii_alphabetic()),
                    "{name}: path data starts with {:?}",
                    p.d.chars().next()
                );
                any = true;
            }
            assert!(any, "{name} has no paths");
        }
    }

    #[test]
    fn a_filled_variant_is_absent_rather_than_empty() {
        let outline_only = names().find(|n| !has_filled(n)).expect("some icon is outline only");
        assert!(find(outline_only, Variant::Filled).is_none());
        assert!(find(outline_only, Variant::Outline).is_some());
    }

    #[test]
    fn filled_is_a_strict_subset_with_no_orphans() {
        // The generator refuses to emit an orphan, so this is the belt to that
        // braces: every filled icon is reachable by its outline name.
        for name in names().filter(|n| has_filled(n)) {
            assert!(find(name, Variant::Outline).is_some(), "{name} is filled-only");
        }
    }

    #[test]
    fn per_path_paint_survives_generation() {
        // `accessible` is the canonical case: stroked, except the dot on the
        // head, which is solid. If this is lost the dot renders as a ring, and
        // nothing else in the pipeline would notice.
        let paths: Vec<IconPath> =
            find("accessible", Variant::Outline).expect("accessible").iter().collect();
        assert!(paths.len() >= 3, "accessible has {} paths", paths.len());
        assert!(
            paths.iter().any(|p| p.fill_current),
            "the solid dot lost its fill: {paths:?}"
        );

        // `percentage-25` carries the other override: a filled slice with no
        // outline of its own.
        let slice: Vec<IconPath> =
            find("percentage-25", Variant::Outline).expect("percentage-25").iter().collect();
        assert!(
            slice.iter().any(|p| p.no_stroke && p.fill_current),
            "the pie slice lost its paint: {slice:?}"
        );
    }

    #[test]
    fn a_name_that_is_not_there_is_none_rather_than_a_panic() {
        assert!(!exists("definitely-not-an-icon"));
        assert!(find("definitely-not-an-icon", Variant::Outline).is_none());
        assert!(!has_filled("definitely-not-an-icon"));
    }

    #[test]
    fn the_tabler_version_is_recorded() {
        // "which Tabler is this?" should be answerable from the artifact.
        assert!(TABLER_VERSION.starts_with(char::is_numeric), "{TABLER_VERSION}");
    }
}
