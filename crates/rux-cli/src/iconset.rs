//! Handing the bundled icon set to the engine that draws it.
//!
//! **This is the seam that keeps tree-shaking possible.** `rux-style` holds a
//! hook and a shape; the data lives in `rux-icons`, which only this crate
//! depends on. An app's generated crate depends on `rux-runtime`, so an icon
//! table behind that edge would be compiled by every app with no way to shake
//! any of it out. Here, the tool that already knows which document is being
//! loaded is the one that supplies the icons.
//!
//! `rux run` and `rux check` install the whole set, because a document being
//! edited can name any icon at any moment. A built app will install only what
//! it mentions, generated into the wrapper beside the documents it embeds.

use std::rc::Rc;

use rux_style::{IconPath, IconSet};

/// Every icon Rux ships, which is Tabler's set.
pub struct Bundled;

impl IconSet for Bundled {
    fn find(&self, name: &str, filled: bool) -> Option<Vec<IconPath>> {
        let variant = if filled { rux_icons::Variant::Filled } else { rux_icons::Variant::Outline };
        let paths = rux_icons::find(name, variant)?;
        Some(
            paths
                .iter()
                .map(|p| IconPath {
                    d: p.d.to_string(),
                    fill_current: p.fill_current,
                    no_stroke: p.no_stroke,
                    half_opacity: p.half_opacity,
                })
                .collect(),
        )
    }

    fn exists(&self, name: &str) -> bool {
        rux_icons::exists(name)
    }

    fn has_filled(&self, name: &str) -> bool {
        rux_icons::has_filled(name)
    }

    fn grid(&self) -> f32 {
        rux_icons::GRID
    }
}

/// Make the bundled set the one this thread draws from.
///
/// Called by every command that turns a document into something on a screen or
/// into a verdict, which is `run`, `check` and `build`. A command that forgets
/// draws every icon as an empty box of the right size, which is why this is one
/// function rather than a line copied into each of them.
pub fn install() {
    rux_style::set_icons(Rc::new(Bundled));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_set_answers_for_a_real_icon() {
        let set = Bundled;
        let paths = set.find("heart", false).expect("heart is in Tabler");
        assert!(!paths.is_empty());
        assert!(paths[0].d.starts_with(|c: char| c.is_ascii_alphabetic()), "{:?}", paths[0].d);
    }

    #[test]
    fn a_missing_icon_is_none_and_a_missing_variant_is_distinguishable() {
        let set = Bundled;
        assert!(set.find("definitely-not-an-icon", false).is_none());
        assert!(!set.exists("definitely-not-an-icon"));

        // An icon that exists in outline only: asking for filled is a different
        // mistake from asking for a name that is not there, and the checker
        // needs to tell them apart to say the right thing.
        let outline_only = rux_icons::names()
            .find(|n| !rux_icons::has_filled(n))
            .expect("some icon is outline only");
        assert!(set.exists(outline_only));
        assert!(!set.has_filled(outline_only));
        assert!(set.find(outline_only, true).is_none());
        assert!(set.find(outline_only, false).is_some());
    }

    #[test]
    fn per_path_paint_reaches_the_engine() {
        // The dot on the head in `accessible` is solid inside a stroked icon.
        // If this stops crossing the seam, that dot renders as a ring and
        // nothing else in the pipeline notices.
        let set = Bundled;
        let paths = set.find("accessible", false).expect("accessible");
        assert!(paths.iter().any(|p| p.fill_current), "{paths:?}");
    }
}
