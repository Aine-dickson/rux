//! Whether a declaration is one a browser would keep.
//!
//! A browser throws an invalid declaration away before the cascade sees it,
//! so `grid-template-rows: minmax 1fr` in a later rule leaves an earlier valid
//! one in force. Rux used to keep it and let each property's reader guess,
//! which turned the unknown word into `auto` and overrode the rule a browser
//! would have used. This is asked for every declaration, from a sheet and from
//! `style` / `:style`, before it can win anything.
//!
//! Two sources answer. lightningcss has a typed parser for most properties,
//! and a value it cannot parse comes back unparsed: that is the browser's
//! verdict, less a few valid values it has no parser for yet
//! ([`lightningcss_misses`]). The properties it has no parser for at all are
//! checked here, against their CSS grammar ([`untyped_valid`]). The grammar is
//! the CSS one, not what Rux honors: a valid keyword Rux does nothing with is
//! still kept, as a browser would keep it.
//!
//! A value holding `var()` or `env()` cannot be judged until it is
//! substituted, so it is always kept, as in CSS. If it turns out invalid then,
//! the property's reader falls back to the initial value, which is what CSS
//! calls invalid at computed-value time.

use lightningcss::properties::{Property, PropertyId};
use lightningcss::stylesheet::ParserOptions;

/// The CSS-wide keywords, valid for every property.
const WIDE: &[&str] = &["inherit", "initial", "unset", "revert", "revert-layer"];

/// Functions lightningcss has no parser for, so any value using one comes back
/// unparsed whether or not it is valid.
const NEWER_FUNCTIONS: &[&str] =
    &["anchor(", "anchor-size(", "color-mix(", "light-dark(", "calc-size(", "attr(", "if("];

/// Functions authors write, named so the warning for one written without its
/// brackets (`minmax 1fr`) can say what is missing.
const FUNCTION_NAMES: &[&str] = &[
    "minmax", "repeat", "fit-content", "calc", "min", "max", "clamp", "var", "env", "rgb", "rgba",
    "hsl", "hsla", "oklch", "oklab", "lab", "lch", "color", "color-mix", "url", "linear-gradient",
    "radial-gradient", "conic-gradient", "translate", "translatex", "translatey", "rotate", "scale",
    "scalex", "scaley", "skew", "skewx", "skewy", "matrix", "cubic-bezier", "steps", "blur",
    "brightness", "contrast", "drop-shadow", "grayscale", "hue-rotate", "invert", "opacity",
    "saturate", "sepia", "anchor", "anchor-size", "attr",
];

/// What is wrong with `name: value`, as the sentence a warning gives, or
/// `None` when a browser would keep it. `unparsed` is lightningcss's verdict
/// when the caller has one already (a sheet's declarations come parsed);
/// `None` asks for it here.
pub(crate) fn problem(name: &str, value: &str, unparsed: Option<bool>) -> Option<String> {
    // `!important` is the cascade's, and a sheet has already taken it off.
    let value = value.trim();
    let value = match value.len().checked_sub("!important".len()) {
        Some(at) if value.is_char_boundary(at) && value[at..].eq_ignore_ascii_case("!important") => {
            value[..at].trim_end()
        }
        _ => value,
    };
    let v = value.to_ascii_lowercase();
    if name.starts_with("--") || v.contains("var(") || v.contains("env(") {
        return None;
    }
    if WIDE.contains(&v.as_str()) {
        return None;
    }
    let id = PropertyId::from(name);
    let valid = if matches!(id, PropertyId::Custom(_)) {
        // An unknown property is the unhonored-property warning's to report.
        untyped_valid(name, &v).unwrap_or(true)
    } else {
        let unparsed = unparsed.unwrap_or_else(|| {
            matches!(
                Property::parse_string(id, value, ParserOptions::default()),
                Ok(Property::Unparsed(_)) | Err(_)
            )
        });
        !unparsed || lightningcss_misses(name, &v)
    };
    if valid {
        return None;
    }
    let mut message = format!(
        "`{name}: {value}` is not a value `{name}` takes, so this declaration is skipped and \
         any earlier rule for `{name}` still applies"
    );
    if let Some(f) = v.split_whitespace().find(|w| FUNCTION_NAMES.contains(w)) {
        message.push_str(&format!(
            ". `{f}` is a function and takes its arguments in brackets, like `{f}(…)`"
        ));
    }
    Some(message)
}

/// Valid CSS that lightningcss leaves unparsed all the same. Found by running
/// every sheet in the test suites through it (2026-10-29); a new entry belongs
/// here only with a value a browser really accepts.
fn lightningcss_misses(name: &str, v: &str) -> bool {
    if NEWER_FUNCTIONS.iter().any(|f| v.contains(f)) {
        return true;
    }
    match name {
        "text-shadow" | "box-shadow" => v == "none",
        // The third value is the z offset.
        "transform-origin" => v.split_whitespace().count() == 3,
        _ => false,
    }
}

/// The CSS grammar of a property lightningcss has no parser for, or `None`
/// for one not checked here (`content`, `font-feature-settings` and the other
/// open-ended ones), which is kept.
fn untyped_valid(name: &str, v: &str) -> Option<bool> {
    let words: Vec<&str> = v.split_whitespace().collect();
    let one_of = |set: &[&str]| words.len() == 1 && set.contains(&words[0]);
    let all_in = |set: &[&str], most: usize| {
        !words.is_empty() && words.len() <= most && words.iter().all(|w| set.contains(w))
    };
    const OVERSCROLL: &[&str] = &["auto", "contain", "none"];
    const BORDER_STYLES: &[&str] =
        &["none", "hidden", "dotted", "dashed", "solid", "double", "groove", "ridge", "inset", "outset"];
    Some(match name {
        "touch-action" => touch_action(&words),
        "transition-behavior" => {
            v.split(',').all(|part| matches!(part.trim(), "normal" | "allow-discrete"))
        }
        "overscroll-behavior" => all_in(OVERSCROLL, 2),
        "overscroll-behavior-x" | "overscroll-behavior-y" => one_of(OVERSCROLL),
        "object-fit" => one_of(&["fill", "contain", "cover", "none", "scale-down"]),
        "pointer-events" => one_of(&[
            "auto", "none", "visiblepainted", "visiblefill", "visiblestroke", "visible", "painted",
            "fill", "stroke", "all", "bounding-box",
        ]),
        "scroll-behavior" => one_of(&["auto", "smooth"]),
        "column-fill" => one_of(&["auto", "balance", "balance-all"]),
        "column-rule-style" => one_of(BORDER_STYLES),
        "column-rule-width" => one_of(&["thin", "medium", "thick"]) || single_length(&words),
        "column-rule-color" => is_color(v),
        "column-count" => words == ["auto"] || positive_integer(&words),
        "column-width" => words == ["auto"] || single_length(&words),
        "scroll-snap-type" => match words.as_slice() {
            ["none"] => true,
            [axis] => SNAP_AXES.contains(axis),
            [axis, how] => SNAP_AXES.contains(axis) && matches!(*how, "mandatory" | "proximity"),
            _ => false,
        },
        "scroll-snap-align" => all_in(&["none", "start", "end", "center"], 2),
        "scroll-snap-stop" => one_of(&["normal", "always"]),
        "scrollbar-width" => one_of(&["auto", "thin", "none"]),
        "scrollbar-color" => {
            words == ["auto"] || (words.len() == 2 && words.iter().all(|w| is_color(w)))
        }
        "isolation" => one_of(&["auto", "isolate"]),
        "position-try-order" => one_of(&[
            "normal", "most-width", "most-height", "most-block-size", "most-inline-size",
        ]),
        "content-visibility" => one_of(&["visible", "auto", "hidden"]),
        "text-wrap-mode" => one_of(&["wrap", "nowrap"]),
        "text-wrap-style" => one_of(&["auto", "balance", "stable", "pretty"]),
        "text-wrap" => {
            let mode = words.iter().filter(|w| matches!(**w, "wrap" | "nowrap")).count();
            let style =
                words.iter().filter(|w| matches!(**w, "auto" | "balance" | "stable" | "pretty")).count();
            !words.is_empty() && mode <= 1 && style <= 1 && mode + style == words.len()
        }
        "line-clamp" => words == ["none"] || positive_integer(&words),
        "outline-offset" => single_length(&words),
        "text-underline-offset" => words == ["auto"] || single_length(&words),
        "font-width" => {
            one_of(&[
                "normal", "ultra-condensed", "extra-condensed", "condensed", "semi-condensed",
                "semi-expanded", "expanded", "extra-expanded", "ultra-expanded",
            ]) || (words.len() == 1 && percentage(words[0]))
        }
        "anchor-name" => {
            v == "none" || v.split(',').all(|n| dashed_ident(n.trim()))
        }
        "position-anchor" => v == "auto" || dashed_ident(v),
        _ => return None,
    })
}

const SNAP_AXES: &[&str] = &["x", "y", "block", "inline", "both"];

/// `auto | none | manipulation | [ pan-x | pan-left | pan-right ] ||
/// [ pan-y | pan-up | pan-down ] || pinch-zoom`.
fn touch_action(words: &[&str]) -> bool {
    if let [only] = words {
        if matches!(*only, "auto" | "none" | "manipulation") {
            return true;
        }
    }
    let (mut x, mut y, mut zoom) = (0, 0, 0);
    for w in words {
        match *w {
            "pan-x" | "pan-left" | "pan-right" => x += 1,
            "pan-y" | "pan-up" | "pan-down" => y += 1,
            "pinch-zoom" => zoom += 1,
            _ => return false,
        }
    }
    !words.is_empty() && x <= 1 && y <= 1 && zoom <= 1
}

fn positive_integer(words: &[&str]) -> bool {
    matches!(words, [n] if n.parse::<u32>().is_ok_and(|n| n > 0))
}

fn percentage(w: &str) -> bool {
    w.strip_suffix('%').is_some_and(|n| n.parse::<f32>().is_ok_and(|n| n >= 0.0))
}

/// One `<length>` (or `<length-percentage>`): a number with a unit, a bare
/// `0`, or a maths function.
fn single_length(words: &[&str]) -> bool {
    let [w] = words else {
        // A maths function may hold spaces (`calc(1px + 2px)`).
        let joined = words.join(" ");
        return math(&joined);
    };
    length(w)
}

fn length(w: &str) -> bool {
    const UNITS: &[&str] = &[
        "px", "em", "rem", "ex", "rex", "ch", "rch", "cap", "ic", "lh", "rlh", "vw", "vh", "vi", "vb",
        "vmin", "vmax", "svw", "svh", "lvw", "lvh", "dvw", "dvh", "cqw", "cqh", "cqi", "cqb", "cqmin",
        "cqmax", "cm", "mm", "q", "in", "pt", "pc", "%",
    ];
    if w == "0" || math(w) {
        return true;
    }
    let split = w.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))).unwrap_or(w.len());
    let (number, unit) = w.split_at(split);
    number.parse::<f32>().is_ok() && UNITS.contains(&unit)
}

fn math(w: &str) -> bool {
    w.ends_with(')') && rux_layout::calc::FUNCTIONS.iter().any(|f| w.starts_with(&format!("{f}(")))
}

fn dashed_ident(w: &str) -> bool {
    w.len() > 2
        && w.starts_with("--")
        && w.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || !c.is_ascii())
}

fn is_color(v: &str) -> bool {
    matches!(v, "currentcolor" | "transparent") || super::parse_color(v).is_some()
}

#[cfg(test)]
mod tests {
    use super::problem;

    fn kept(name: &str, value: &str) -> bool {
        problem(name, value, None).is_none()
    }

    #[test]
    fn a_function_written_without_brackets_is_dropped_and_named() {
        let why = problem("grid-template-rows", "minmax 1fr", None).expect("dropped");
        assert!(why.contains("`minmax` is a function"), "{why}");
        assert!(kept("grid-template-rows", "minmax(auto, 1fr)"));
        assert!(kept("grid-template-rows", "auto 1fr"));
    }

    #[test]
    fn typed_properties_follow_lightningcss() {
        assert!(!kept("transform", "translate(foo)"));
        assert!(!kept("transition-duration", "fast"));
        assert!(!kept("grid-column", "1 / 2 / 3"));
        assert!(!kept("color", "notacolor"));
        assert!(kept("transform", "translate(10px)"));
        assert!(kept("color", "red"));
    }

    #[test]
    fn what_lightningcss_cannot_parse_but_is_valid_is_kept() {
        assert!(kept("color", "inherit"));
        assert!(kept("text-shadow", "none"));
        assert!(kept("top", "anchor(bottom)"));
        assert!(kept("color", "color-mix(in srgb, currentcolor 50%, transparent)"));
        assert!(kept("transform-origin", "10px 20px 30px"));
        assert!(!kept("transform-origin", "sideways"));
    }

    #[test]
    fn a_value_with_var_waits_for_substitution() {
        assert!(kept("grid-template-rows", "var(--rows)"));
        assert!(kept("touch-action", "var(--t, nope)"));
    }

    #[test]
    fn untyped_properties_follow_their_grammar() {
        assert!(kept("touch-action", "pan-x pinch-zoom"));
        assert!(!kept("touch-action", "pan-x pan-left"));
        assert!(!kept("touch-action", "sideways"));
        assert!(kept("scrollbar-width", "thin"));
        assert!(!kept("scrollbar-width", "skinny"));
        assert!(kept("scroll-snap-type", "x mandatory"));
        assert!(!kept("scroll-snap-type", "mandatory x"));
        assert!(kept("column-count", "3"));
        assert!(!kept("column-count", "0"));
        assert!(kept("outline-offset", "2px"));
        assert!(kept("outline-offset", "calc(1px + 1px)"));
        assert!(!kept("outline-offset", "far"));
        assert!(kept("text-wrap", "nowrap balance"));
        assert!(!kept("text-wrap", "wrap nowrap"));
        assert!(kept("anchor-name", "--a, --b"));
        assert!(!kept("anchor-name", "a"));
        assert!(kept("scrollbar-color", "red blue"));
        // Open-ended ones are not judged.
        assert!(kept("content", "\"x\""));
        assert!(kept("TOUCH-ACTION".to_ascii_lowercase().as_str(), "NONE"));
    }
}
