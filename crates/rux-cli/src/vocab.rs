//! `rux vocab`: print what the runtime understands, as JSON, for an editor.
//!
//! The VS Code extension offers completions for elements, attributes,
//! directives and CSS properties. Every one of those lists already exists
//! inside a crate here, and the extension having its own copy is exactly how
//! the `<image>` indentation bug happened: the JS formatter inherited HTML's
//! void-tag set, which has `img` and not Rux's `image`, and over-indented
//! everything after an `<image src="…">` for two releases.
//!
//! So the lists that *can* be read from the runtime are read from it:
//! [`rux_style::honored_properties`] is the same slice the unhonored-property
//! warning consults, and [`rux_fmt::void_tags`] is the same one the formatter
//! indents by. Offering a CSS property the runtime would then warn does nothing
//! is worse than offering no completions at all.
//!
//! The element and attribute tables below are declared here rather than read
//! from a crate because no single crate owns them today: tags are strings all
//! the way through the parser, and the runtime learns what `<image>` means by
//! matching on the string where it needs to. Extracting a real element registry
//! is worth doing and is not this change; until then `vocabulary_matches_docs`
//! in the tests pins these names against `docs/05-as-built.md`, so the two
//! cannot drift silently.

/// One completable name, with the one-line description the editor shows beside
/// it. `detail` is the grey text in the completion list, `doc` the popup.
struct Entry {
    name: &'static str,
    detail: &'static str,
    doc: &'static str,
}

/// The elements the runtime renders. `<slot>`, `<router>` and `<route>` render
/// no box of their own but are still written in a template, so they belong in
/// the list an author completes from.
const ELEMENTS: &[Entry] = &[
    Entry {
        name: "screen",
        detail: "the root element of a document",
        doc: "The root of a `.rux` document. A component whose template root is \
              `<screen>` is a page; anything else is a fragment.",
    },
    Entry {
        name: "view",
        detail: "a box; the generic container",
        doc: "A generic box, the element most markup is made of. Defaults to \
              `display: block`, so give it `display: flex` to lay its children out.",
    },
    Entry {
        name: "text",
        detail: "a run of text",
        doc: "A run of text. Two `<text>` siblings **stack**: there is no inline \
              flow, so they do not share a line. `role=\"heading\"` announces it \
              as a heading.",
    },
    Entry {
        name: "image",
        detail: "an image from a file",
        doc: "`src` resolves relative to the `.rux` file, not the working \
              directory. PNG, JPEG, GIF and WebP. With no CSS size it lays out at \
              its intrinsic pixel size.",
    },
    Entry {
        name: "button",
        detail: "a tappable box, announced as a button",
        doc: "A tappable box. `<view @tap>` is the same thing to the layout; \
              `<button>` says so to the accessibility tree.",
    },
    Entry {
        name: "input",
        detail: "text field, textarea, select, checkbox or radio",
        doc: "A text field by default. `type=\"textarea\"` is multiline, \
              `type=\"select\"` a combo box, `type=\"checkbox\"` and \
              `type=\"radio\"` carry live checked state. Bind it with `r-model`.",
    },
    Entry {
        name: "slot",
        detail: "a component's hole for the caller's children",
        doc: "Renders whatever the caller wrote between the component's tags. \
              Children written inside `<slot>…</slot>` are the default, used when \
              the caller passed nothing.",
    },
    Entry {
        name: "router",
        detail: "renders the one matching route",
        doc: "Renders the first `<route>` whose path matches. Leaves no box of \
              its own: the matched view expands in its place.",
    },
    Entry {
        name: "route",
        detail: "maps a path to a component",
        doc: "`<route path=\"/crew/:id\" view=\"crew-detail\" />`. Routes are \
              tried in the order written and the first match wins, so `fallback` \
              can sit anywhere among them.",
    },
];

/// Attributes that mean something on any element.
const GLOBAL_ATTRIBUTES: &[Entry] = &[
    Entry { name: "@press", detail: "a finger or button went down", doc: "Runs when a press lands on this element. `event.x` / `event.y` are relative to the element; `event.touches` lists every finger down." },
    Entry { name: "@release", detail: "the press came up", doc: "Runs on release, whether or not the press stayed still enough to be a tap." },
    Entry { name: "@longpress", detail: "the press rested", doc: "Runs once, after the press has been held still for half a second." },
    Entry { name: "@swipe", detail: "a flick, with a direction", doc: "Runs once at the end of a press that travelled far enough, fast enough. `event.direction` is `left`, `right`, `up` or `down`, and `event.dx` / `event.dy` are how far it came." },
    Entry { name: "@drag", detail: "the pointer is moving", doc: "Runs at the start of a drag, on every move, and at the end. `event.phase` says which, `event.dx` / `event.dy` are the distance from where it began. A `@drag` claims the finger, so the page under it does not scroll." },
    Entry { name: "class", detail: "CSS classes", doc: "Space-separated class names, matched by `.name` selectors. `:class` is the bound form." },
    Entry { name: "id", detail: "unique id", doc: "Matched by `#name` selectors, and by `query(\"#name\")` from script." },
    Entry { name: "style", detail: "inline declarations", doc: "Inline CSS for this element. `:style` is the bound form." },
    Entry { name: "role", detail: "accessibility role", doc: "Honored for selectors (`[role=\"heading\"]`) and for the accessibility tree. Matches case-insensitively." },
    Entry { name: "to", detail: "make this element a link", doc: "Tapping navigates to this path, the element announces as a link, and it matches `:current` when it names the path you are on. `:to` is the bound form." },
];

/// The structural directives. These are attributes, but they are the ones worth
/// ranking first in a completion list: they are what makes a template a
/// template rather than a picture.
const DIRECTIVES: &[Entry] = &[
    Entry { name: "r-for", detail: "repeat this element per item", doc: "`r-for=\"item in items\"`. Pair it with `r-key` so reconciliation can match rows across a change." },
    Entry { name: "r-key", detail: "identity for a repeated row", doc: "`r-key=\"item.id\"`. Without it a keyed change is a rebuild, and input state inside a row moves to the wrong row." },
    Entry { name: "r-if", detail: "render only when true", doc: "`r-if=\"count > 0\"`. The element and its subtree are absent, not hidden, when false." },
    Entry { name: "r-elif", detail: "an else-if branch", doc: "Follows an `r-if` sibling. The first branch whose condition holds renders." },
    Entry { name: "r-else", detail: "the fallback branch", doc: "Follows an `r-if` or `r-elif` sibling, and renders when none of them held." },
    Entry { name: "r-show", detail: "hide without removing", doc: "`r-show=\"expanded\"`. The element stays in the tree and keeps its state; it is only not painted. Prefer `r-if` when the subtree is expensive." },
    Entry { name: "r-model", detail: "two-way bind an input", doc: "`r-model=\"name\"` on an `<input>`: typing writes the signal, and writing the signal updates the field." },
];

/// Attributes that only mean something on one element.
const ELEMENT_ATTRIBUTES: &[(&str, &[Entry])] = &[
    (
        "image",
        &[
            Entry { name: "src", detail: "path, relative to this .rux file", doc: "Resolved relative to the document, not the working directory. `:src` binds an expression." },
            Entry { name: "alt", detail: "accessible description", doc: "What the image is, for the accessibility tree." },
        ],
    ),
    (
        "input",
        &[
            Entry { name: "type", detail: "text | textarea | select | checkbox | radio", doc: "Omitted means a single-line text field." },
            Entry { name: "placeholder", detail: "text shown while empty", doc: "Shown until the field has a value. Not a label." },
            Entry { name: "value", detail: "the field's value", doc: "The literal starting value. For state that changes, use `r-model`." },
            Entry { name: "checked", detail: "checkbox / radio state", doc: "Live state for `type=\"checkbox\"` and `type=\"radio\"`, and matched by the `:checked` pseudo-class." },
            Entry { name: "name", detail: "radio group", doc: "Radios sharing a `name` are one group, so choosing one clears the others." },
            Entry { name: "options", detail: "the choices for a select", doc: "For `type=\"select\"`. `:options` binds an array." },
        ],
    ),
    (
        "route",
        &[
            Entry { name: "path", detail: "the path to match", doc: "`/crew/:id` captures `id` and hands it to the view as a prop. A child route's path is relative to its parent, and `path=\"\"` is the index route that fills the parent's outlet at the parent's own path." },
            Entry { name: "view", detail: "the component to render", doc: "Names an imported component, the same name its tag would use." },
            Entry { name: "fallback", detail: "match anything unmatched", doc: "Valueless. Renders when no other route matched, wherever it sits among them." },
        ],
    ),
    (
        "router",
        &[Entry { name: "restore-scroll", detail: "restore scroll position on Back", doc: "Puts a page back where it was when you return to it." }],
    ),
    (
        "text",
        &[Entry { name: "for", detail: "label another element", doc: "Names the `id` of the element this text labels, so the accessibility tree pairs them." }],
    ),
];

/// The names script can call that are not ordinary rhai. Kept in step with
/// `docs/07-script.md`, which is the reference for this tier.
const SCRIPT_GLOBALS: &[Entry] = &[
    Entry { name: "signal", detail: "signal(value)", doc: "Declare a piece of reactive state." },
    Entry { name: "computed", detail: "computed(|| expr)", doc: "A value derived from other signals, recomputed when they change." },
    Entry { name: "effect", detail: "effect(|| …)", doc: "Run when the signals it reads change." },
    Entry { name: "mounted", detail: "mounted { … }", doc: "Runs once the document is on screen. Document level today; a component declaring one is warned." },
    Entry { name: "unmounted", detail: "unmounted { … }", doc: "Runs when the document stops being the one on screen." },
    Entry { name: "query", detail: "query(\"selector\")", doc: "Find an element by CSS selector, in a handler, against the frame already laid out." },
    Entry { name: "navigate", detail: "navigate(path)", doc: "Go to a path, leaving the current one in the history." },
    Entry { name: "replace", detail: "replace(path)", doc: "Go to a path *without* a history entry. The only correct way to redirect." },
    Entry { name: "back", detail: "back()", doc: "Walk back through the history." },
    Entry { name: "forward", detail: "forward()", doc: "Walk forward through the history." },
    Entry { name: "path_for", detail: "path_for(\"route\", #{ … })", doc: "Build a path from a named route and its parameters." },
    Entry { name: "emit", detail: "emit(\"name\", payload)", doc: "A component telling its caller something happened." },
    Entry { name: "print", detail: "print(x)", doc: "Printf-debugging. Reaches the dev overlay, not just stderr." },
    Entry { name: "debug", detail: "debug(x)", doc: "Like `print`, with the value's structure shown." },
];

/// Print the whole vocabulary as JSON on stdout.
pub fn emit() -> i32 {
    let mut out = String::from("{\n");
    out.push_str(&format!("  \"version\": {},\n", quote(env!("CARGO_PKG_VERSION"))));
    out.push_str("  \"elements\": ");
    entries(&mut out, ELEMENTS, 1);
    out.push_str(",\n  \"globalAttributes\": ");
    entries(&mut out, GLOBAL_ATTRIBUTES, 1);
    out.push_str(",\n  \"directives\": ");
    entries(&mut out, DIRECTIVES, 1);
    out.push_str(",\n  \"elementAttributes\": {\n");
    for (i, (tag, attrs)) in ELEMENT_ATTRIBUTES.iter().enumerate() {
        out.push_str(&format!("    {}: ", quote(tag)));
        entries(&mut out, attrs, 2);
        out.push_str(if i + 1 == ELEMENT_ATTRIBUTES.len() { "\n" } else { ",\n" });
    }
    out.push_str("  },\n  \"scriptGlobals\": ");
    entries(&mut out, SCRIPT_GLOBALS, 1);

    out.push_str(",\n  \"voidTags\": ");
    strings(&mut out, rux_fmt::void_tags());
    out.push_str(",\n  \"cssProperties\": ");
    strings(&mut out, rux_style::honored_properties());
    out.push_str("\n}\n");

    print!("{out}");
    0
}

/// A JSON array of entry objects, indented `depth` levels in.
fn entries(out: &mut String, list: &[Entry], depth: usize) {
    let pad = "  ".repeat(depth + 1);
    out.push_str("[\n");
    for (i, e) in list.iter().enumerate() {
        out.push_str(&format!(
            "{pad}{{ \"name\": {}, \"detail\": {}, \"doc\": {} }}",
            quote(e.name),
            quote(e.detail),
            quote(&squash(e.doc)),
        ));
        out.push_str(if i + 1 == list.len() { "\n" } else { ",\n" });
    }
    out.push_str(&format!("{}]", "  ".repeat(depth)));
}

/// A JSON array of bare strings, wrapped so a long list stays readable in the
/// checked-in copy the editor ships.
fn strings(out: &mut String, list: &[&str]) {
    out.push_str("[\n");
    for (i, chunk) in list.chunks(6).enumerate() {
        if i > 0 {
            out.push_str(",\n");
        }
        let line: Vec<String> = chunk.iter().map(|s| quote(s)).collect();
        out.push_str(&format!("    {}", line.join(", ")));
    }
    out.push_str("\n  ]");
}

/// Rust source strings are written across lines for readability; JSON should
/// carry one space where the source had a newline and its indentation.
fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The six escapes JSON requires, plus the control-character form. None of the
/// strings above need more, and a dependency on serde for this would be one
/// more crate in the published `ruxlang`.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The JSON has to parse, and the editor reads it with `JSON.parse`. There
    /// is no JSON parser in this crate's dependencies, so this checks the two
    /// things that hand-rolled emitters actually get wrong: balance, and
    /// unescaped quotes inside the doc strings.
    #[test]
    fn emitted_json_is_balanced_and_escaped() {
        let mut out = String::new();
        entries(&mut out, ELEMENTS, 1);
        let braces = out.matches('{').count();
        assert_eq!(braces, ELEMENTS.len(), "one object per element");
        assert_eq!(braces, out.matches('}').count(), "every object closes");

        // Every `"` in the body is either a delimiter or escaped. Counting them
        // catches a doc string that smuggled a bare quote in.
        for e in ELEMENTS.iter().chain(DIRECTIVES).chain(SCRIPT_GLOBALS) {
            let q = quote(e.doc);
            assert!(!q[1..q.len() - 1].contains("\\\\\""), "no double-escaping");
            let bare = q[1..q.len() - 1].match_indices('"').filter(|(i, _)| {
                *i == 0 || q[1..q.len() - 1].as_bytes()[i - 1] != b'\\'
            });
            assert_eq!(bare.count(), 0, "unescaped quote in `{}`'s doc", e.name);
        }
    }

    /// The lists that *are* owned by a crate must be read from it, not copied.
    /// If these ever stop matching, the extension is offering something the
    /// runtime does not honor.
    #[test]
    fn borrowed_lists_come_from_their_owners() {
        assert!(rux_style::honored_properties().contains(&"display"));
        assert!(rux_style::honored_properties().contains(&"transition"));
        assert!(!rux_style::honored_properties().contains(&"float"));
        assert!(rux_fmt::void_tags().contains(&"image"), "the `<image>` bug");
        assert!(!rux_fmt::void_tags().contains(&"view"));
    }

    /// The element and attribute tables are declared here because no crate owns
    /// them yet, so this pins them against the document that describes them.
    /// A tag added to the runtime and not to `docs/05-as-built.md` fails here,
    /// which is the cheapest available substitute for a real registry.
    #[test]
    fn vocabulary_matches_docs() {
        let docs = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/05-as-built.md"),
        );
        let Ok(docs) = docs else { return }; // not a checkout; nothing to pin against
        let Some(section) = docs.split("### Elements").nth(1) else {
            panic!("`### Elements` is gone from docs/05-as-built.md");
        };
        let section: String = section.chars().take(600).collect();
        for e in ELEMENTS {
            assert!(
                section.contains(&format!("`<{}>", e.name))
                    || section.contains(&format!("<{}>", e.name)),
                "`<{}>` is offered as a completion but is not in the Elements section of \
                 docs/05-as-built.md. Add it there, or stop offering it.",
                e.name
            );
        }
    }
}
