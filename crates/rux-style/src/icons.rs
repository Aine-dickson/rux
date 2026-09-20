//! Where `<icon name="heart" />` gets its geometry from.
//!
//! **The data is not here, and that is the whole point.** An app's generated
//! crate depends on `rux-runtime`, which depends on this one, and a cargo
//! dependency is resolved long before Rux knows which icons a document
//! mentions. Put six thousand icons behind that edge and every app compiles all
//! of them with no way to shake any out. So this holds a hook and a shape, and
//! whoever is in a position to know what is needed installs the rest:
//!
//! - `rux run` and `rux check` install the whole set, from `rux-icons`.
//! - `rux build` will install only the names a project mentions, generated into
//!   the wrapper crate beside the documents it already embeds.
//!
//! The default provider knows nothing, so a runtime used on its own says an
//! icon is missing rather than drawing an empty box.

use std::cell::RefCell;
use std::rc::Rc;

/// One path of an icon, and the paint it insists on for itself.
///
/// **Paint is per path, not per variant**, and an icon drawn without honouring
/// that is visibly wrong. An outline icon is stroked, but the dot on the head
/// in `accessible` is solid and the slice in `percentage-25` is filled with no
/// outline. Around ninety icons in Tabler do this.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconPath {
    /// SVG path data, in the set's own grid.
    pub d: String,
    /// Draw this path solid, whatever the variant does.
    pub fill_current: bool,
    /// Draw this path with no outline, whatever the variant does.
    pub no_stroke: bool,
    /// Draw this path at half opacity.
    pub half_opacity: bool,
}

/// An icon set, as much of one as Rux needs to draw from it.
pub trait IconSet {
    /// The paths of `name`, or `None` if this set cannot draw it.
    fn find(&self, name: &str, filled: bool) -> Option<Vec<IconPath>>;
    /// Whether `name` is in the set at all, in any variant.
    fn exists(&self, name: &str) -> bool;
    /// Whether `name` has filled artwork.
    ///
    /// Separate from [`IconSet::find`] so a missing icon and an icon with no
    /// filled variant can be told apart: they are different mistakes and
    /// deserve different messages.
    fn has_filled(&self, name: &str) -> bool;
    /// The grid the set is drawn on, 24 for Tabler and everything like it.
    fn grid(&self) -> f32;

    /// Whether this set can answer for anything at all.
    ///
    /// **The difference between "that icon does not exist" and "nobody has said
    /// what the icons are".** A bare runtime has no set, and reporting every
    /// `<icon>` in a document as a bad name would be a complaint about the host
    /// rather than about the document. Defaulted to true, because a set that
    /// bothered to exist can answer.
    fn installed(&self) -> bool {
        true
    }
}

/// An icon set carried in memory, which is what a built app has.
///
/// **This is the other end of tree-shaking.** `rux build` works out which icons
/// a project names, looks them up while it still has the whole set, and writes
/// the answers into the generated crate as calls to [`MemoryIcons::insert`]. An
/// app therefore carries the icons it draws and not six thousand others, and
/// nothing in its dependency graph ever saw the full table.
#[derive(Default)]
pub struct MemoryIcons {
    icons: std::collections::HashMap<String, (Vec<IconPath>, Vec<IconPath>)>,
    grid: f32,
}

impl MemoryIcons {
    /// An empty set drawn on `grid` units.
    pub fn new(grid: f32) -> Self {
        Self { icons: std::collections::HashMap::new(), grid }
    }

    /// Add one icon's artwork. `filled` may be empty, which is the common case.
    pub fn insert(&mut self, name: &str, outline: Vec<IconPath>, filled: Vec<IconPath>) {
        self.icons.insert(name.to_string(), (outline, filled));
    }

    /// Add one icon from the packed form a generated crate carries.
    ///
    /// **A generated app does not write out thousands of `Vec` literals**, and
    /// this exists because the first version did. An app whose icon name is
    /// bound embeds the whole set, and five thousand inline vector
    /// constructions in one function overflowed the stack of Android's
    /// `android_main` thread: the app died with `SIGSEGV` before drawing
    /// anything. Static tables and a loop have a stack frame of nothing.
    ///
    /// Each variant is its paths joined by `\n`, every path prefixed with one
    /// hex digit of paint flags. Empty means the variant does not exist.
    pub fn insert_packed(&mut self, name: &str, outline: &str, filled: &str) {
        self.icons.insert(name.to_string(), (unpack(outline), unpack(filled)));
    }
}

impl IconSet for MemoryIcons {
    fn find(&self, name: &str, filled: bool) -> Option<Vec<IconPath>> {
        let (o, f) = self.icons.get(name)?;
        let want = if filled { f } else { o };
        (!want.is_empty()).then(|| want.clone())
    }
    fn exists(&self, name: &str) -> bool {
        self.icons.contains_key(name)
    }
    fn has_filled(&self, name: &str) -> bool {
        self.icons.get(name).is_some_and(|(_, f)| !f.is_empty())
    }
    fn grid(&self) -> f32 {
        self.grid
    }
}

/// One variant's packed paths back into drawings.
///
/// The format is the one the icon data is generated in: paths joined by `\n`,
/// each prefixed with a hex digit of paint flags. Path data never contains a
/// newline and always begins with a command letter, so neither marker can be
/// confused with content.
fn unpack(packed: &str) -> Vec<IconPath> {
    if packed.is_empty() {
        return Vec::new();
    }
    packed
        .split('\n')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let (flags, d) = entry.split_at(1);
            let flags = u8::from_str_radix(flags, 16).unwrap_or(0);
            IconPath {
                d: d.to_string(),
                fill_current: flags & 1 != 0,
                no_stroke: flags & 2 != 0,
                half_opacity: flags & 4 != 0,
            }
        })
        .collect()
}

/// The set that knows nothing, which is what a bare runtime has.
struct NoIcons;

impl IconSet for NoIcons {
    fn find(&self, _name: &str, _filled: bool) -> Option<Vec<IconPath>> {
        None
    }
    fn exists(&self, _name: &str) -> bool {
        false
    }
    fn has_filled(&self, _name: &str) -> bool {
        false
    }
    fn grid(&self) -> f32 {
        24.0
    }
    fn installed(&self) -> bool {
        false
    }
}

thread_local! {
    static ICONS: RefCell<Rc<dyn IconSet>> = RefCell::new(Rc::new(NoIcons));
}

/// Install the set this thread draws icons from, returning the previous one.
///
/// Handed back for the same reason [`rux_parser`] callers hand back a source: a
/// checker or a test borrowing the thread for one pass can put the old one
/// where it found it rather than assume the default was in place.
pub fn set_icons(set: Rc<dyn IconSet>) -> Rc<dyn IconSet> {
    ICONS.with(|s| std::mem::replace(&mut *s.borrow_mut(), set))
}

/// Go back to knowing no icons.
pub fn reset_icons() {
    set_icons(Rc::new(NoIcons));
}

/// Run `f` against the installed set.
pub(crate) fn with<R>(f: impl FnOnce(&dyn IconSet) -> R) -> R {
    // Cloned out before the call, so a provider that somehow reached back in
    // could not find the cell already borrowed.
    let set = ICONS.with(|s| Rc::clone(&*s.borrow()));
    f(&*set)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct One;
    impl IconSet for One {
        fn find(&self, name: &str, filled: bool) -> Option<Vec<IconPath>> {
            (name == "heart" && !filled).then(|| {
                vec![IconPath {
                    d: "M 0 0 L 24 24".into(),
                    fill_current: false,
                    no_stroke: false,
                    half_opacity: false,
                }]
            })
        }
        fn exists(&self, name: &str) -> bool {
            name == "heart"
        }
        fn has_filled(&self, _name: &str) -> bool {
            false
        }
        fn grid(&self) -> f32 {
            24.0
        }
    }

    #[test]
    fn nothing_is_installed_by_default_and_that_is_not_a_panic() {
        reset_icons();
        assert!(with(|s| s.find("heart", false)).is_none());
        assert!(!with(|s| s.exists("heart")));
    }

    #[test]
    fn installing_hands_back_what_was_there() {
        reset_icons();
        let previous = set_icons(Rc::new(One));
        assert!(with(|s| s.exists("heart")));
        set_icons(previous);
        assert!(!with(|s| s.exists("heart")), "the old set was not restored");
    }
}
