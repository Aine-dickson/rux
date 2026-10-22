//! Cascade layers (`@layer`), `@supports`, and the order the cascade sorts
//! matched rules in.
//!
//! **Layers decide before specificity.** A rule in a later layer beats any
//! rule in an earlier one however specific that is, and a rule in no layer
//! beats every layered rule. That is what lets a utility class (`.mt-4`, one
//! class) win over a component's `.card .title` (two) when the utilities
//! layer is declared after the components one, which is how every Tailwind v4
//! stylesheet is built: everything it emits sits in `@layer theme, base,
//! components, utilities`.
//!
//! `!important` turns layer order around, as in CSS: an important declaration
//! in an earlier layer beats one in a later layer, and an important layered
//! declaration beats an important unlayered one. Every important declaration
//! beats every normal one.

use std::cmp::Ordering;
use std::collections::HashMap;

use lightningcss::rules::supports::SupportsCondition;

/// Every layer one sheet (with what it includes) has named, in the order they
/// were first named, which is the order they stack in.
///
/// A layer's key is its index among its parent's sublayers at every level, so
/// `@layer a { @layer b { } }` is `[i(a), i(b)]`. Rules written directly in
/// `a` sit above everything in `a`'s sublayers, which [`cmp`] gets by reading a
/// key's end as one more, highest, index.
#[derive(Default)]
pub(crate) struct Layers {
    known: HashMap<Vec<String>, Vec<u32>>,
    children: HashMap<Vec<String>, u32>,
    anonymous: u32,
}

impl Layers {
    /// The key of the layer at `path`, naming it (and each layer above it)
    /// now if it is new.
    pub(crate) fn key(&mut self, path: &[String]) -> Vec<u32> {
        let mut key = Vec::new();
        for depth in 1..=path.len() {
            let here = path[..depth].to_vec();
            if let Some(k) = self.known.get(&here) {
                key.clone_from(k);
                continue;
            }
            let n = self.children.entry(path[..depth - 1].to_vec()).or_insert(0);
            key.push(*n);
            *n += 1;
            self.known.insert(here, key.clone());
        }
        key
    }

    /// A name for an `@layer { }` with none. Each one is its own layer, which
    /// nothing else can add to: CSS's anonymous layer.
    pub(crate) fn anonymous(&mut self) -> String {
        self.anonymous += 1;
        // A character no layer name written in a sheet can contain.
        format!("\u{0}{}", self.anonymous)
    }
}

/// Which of two layer keys stacks above the other for normal declarations.
/// The empty key is a rule in no layer.
pub(crate) fn cmp(a: &[u32], b: &[u32]) -> Ordering {
    let end = std::iter::once(&u32::MAX);
    a.iter().chain(end.clone()).cmp(b.iter().chain(end))
}

/// The order the cascade applies matched rules in: the last one wins.
pub(crate) fn cascade(a: &super::Rule, b: &super::Rule) -> Ordering {
    a.important
        .cmp(&b.important)
        .then_with(|| {
            let layers = cmp(&a.layer, &b.layer);
            if a.important { layers.reverse() } else { layers }
        })
        .then(a.specificity.cmp(&b.specificity))
        .then(a.order.cmp(&b.order))
}

/// Whether an `@supports` condition holds here.
///
/// A declaration holds when Rux honors the property, which is what a browser
/// answers too: it asks whether the property is one it implements, not
/// whether a rule elsewhere uses it. A `selector()` holds when every
/// pseudo-class in it is one Rux matches. Anything else (a `font-tech()`, a
/// condition lightningcss could not read) does not hold, as a browser that
/// does not know a test answers no.
pub(crate) fn supports(cond: &SupportsCondition) -> bool {
    match cond {
        SupportsCondition::Not(c) => !supports(c),
        SupportsCondition::And(all) => all.iter().all(supports),
        SupportsCondition::Or(any) => any.iter().any(supports),
        SupportsCondition::Declaration { property_id, .. } => {
            let name = property_id.name().to_ascii_lowercase();
            name.starts_with("--") || super::is_honored(&name)
        }
        SupportsCondition::Selector(sel) => super::selector_is_supported(sel),
        SupportsCondition::Unknown(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn layers_stack_in_the_order_they_are_first_named() {
        let mut layers = Layers::default();
        let theme = layers.key(&path(&["theme"]));
        let utilities = layers.key(&path(&["utilities"]));
        // Named again, it keeps its first place.
        assert_eq!(layers.key(&path(&["theme"])), theme);
        assert_eq!(cmp(&theme, &utilities), Ordering::Less);
        // No layer at all is above both.
        assert_eq!(cmp(&[], &utilities), Ordering::Greater);
    }

    #[test]
    fn a_layers_own_rules_sit_above_its_sublayers() {
        let mut layers = Layers::default();
        let a = layers.key(&path(&["a"]));
        let ab = layers.key(&path(&["a", "b"]));
        let c = layers.key(&path(&["c"]));
        assert_eq!(cmp(&a, &ab), Ordering::Greater);
        assert_eq!(cmp(&ab, &c), Ordering::Less);
    }
}
