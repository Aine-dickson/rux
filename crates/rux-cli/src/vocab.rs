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
        name: "path",
        detail: "vector geometry from SVG path data",
        doc: "`d` is SVG path data, in the element's own coordinates. Paint is               CSS, not attributes: `fill`, `stroke`, `stroke-width`,               `stroke-linecap`, `stroke-linejoin`, `fill-rule`. With no CSS size               it lays out at the size of its own geometry. `:d` binds an               expression, and two paths with the same command sequence animate               between one another under `transition: d`.",
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
    Entry { name: "@tap", detail: "run script when this element is tapped", doc: "A click and a finger are the same event. `@tap=\"count += 1\"` takes an expression, and the element becomes focusable and announces as tappable." },
    Entry { name: "@press", detail: "a finger or button went down", doc: "Runs when a press lands on this element. `event.x` / `event.y` are relative to the element, `event.pageX` / `event.pageY` to the window; `event.touches` lists every finger down." },
    Entry { name: "@release", detail: "the press came up", doc: "Runs on release, whether or not the press stayed still enough to be a tap." },
    Entry { name: "@longpress", detail: "the press rested", doc: "Runs once, after the press has been held still for half a second." },
    Entry { name: "@swipe", detail: "a flick, with a direction", doc: "Runs once at the end of a press that travelled far enough, fast enough. `event.direction` is `left`, `right`, `up` or `down`, and `event.totalX` / `event.totalY` are how far it came. A drag that ends as a flick fires this too." },
    Entry { name: "@drag", detail: "the pointer is moving", doc: "Runs at the start of a drag, on every move, and at the end. `event.phase` says which; `event.totalX` / `event.totalY` measure from where the press landed, `event.moveX` / `event.moveY` from the previous event. A `@drag` claims the finger, so the page under it does not scroll." },
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
    Entry { name: "r-transition", detail: "animate the way in and out", doc: "On an `r-if` branch or a keyed `r-for` row: the element is held on screen while it leaves. Style the two sides with `:enter-from` and `:leave-to`; the element's own `transition` sets how long. `:r-transition=\"expr\"` hands progress to you instead, 0 to 1." },
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
        "path",
        &[
            Entry { name: "d", detail: "SVG path data", doc: "The full SVG grammar: `M L H V C S Q T A Z`, absolute or relative. Coordinates are in the element's own box. `:d` binds an expression." },
            Entry { name: "alt", detail: "accessible description", doc: "What the drawing shows. Without it the path is treated as decoration and left out of the accessibility tree." },
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
            Entry { name: "guard", detail: "decide whether this route may be entered", doc: "`guard=\"expr\"`, run whenever this route is part of the match. `false` cancels the navigation, a string redirects to that path, and anything else allows it, `()` included, so a function that falls off the end has consented. Outer guards run first." },
        ],
    ),
    (
        "router",
        &[
            Entry { name: "restore-scroll", detail: "restore scroll position on Back", doc: "Puts a page back where it was when you return to it." },
            Entry { name: "guard", detail: "decide whether any navigation may proceed", doc: "`guard=\"expr\"`, run on every navigation this router handles, before the history moves. So a refusal leaves no entry behind, and Back and Forward pass through it too, not just `navigate`. `false` cancels, a string redirects, anything else allows." },
        ],
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
    Entry { name: "computed", detail: "computed name = expr;", doc: "Derived state, readable anywhere a signal is. A **declaration, not a call**: there is no `computed(|| …)`. Refreshing is one pass in declaration order, so a computed may read one declared above it and not below. Inside a component it runs per instance." },
    Entry { name: "effect", detail: "effect { … }", doc: "Runs when what it read changes, and once on load. A **block, not a call**: there is no `effect(|| …)`. It subscribes to what it actually read on its last run, and is never woken by its own writes." },
    Entry { name: "mounted", detail: "mounted { … }", doc: "Runs once the document is on screen. Document level today; a component declaring one is warned." },
    Entry { name: "unmounted", detail: "unmounted { … }", doc: "Runs when the document stops being the one on screen." },
    Entry { name: "query", detail: "query(\"selector\")", doc: "Find elements by CSS selector, in a handler, against the frame already laid out. Returns an **array**, so check `.length` or index it: a selector that matches nothing is an empty array, not an error. Only inside a handler, because a binding that read the tree would rebuild the tree it read." },
    Entry { name: "navigate", detail: "navigate(path)", doc: "Go to a path, leaving the current one in the history." },
    Entry { name: "replace", detail: "replace(path)", doc: "Go to a path *without* a history entry. The only correct way to redirect." },
    Entry { name: "back", detail: "back()", doc: "Walk back through the history." },
    Entry { name: "forward", detail: "forward()", doc: "Walk forward through the history." },
    Entry { name: "path_for", detail: "path_for(\"route\", #{ … })", doc: "Build a path from a named route and its parameters." },
    Entry { name: "emit", detail: "emit(\"name\", payload)", doc: "A component telling its caller something happened." },
    Entry { name: "blur", detail: "blur()", doc: "Drop focus. Free-standing rather than a method on an element, because there is only one focused element: blurring \"this one\" would either do nothing or take focus from something else." },
    Entry { name: "setInterval", detail: "setInterval(ms) { … }", doc: "Run a block on a period, handing back a handle. A **block, not a callback**: `setInterval(1000) { seconds++ }`. Stop it with `clearInterval(handle)`." },
    Entry { name: "clearInterval", detail: "clearInterval(handle)", doc: "Stop a timer started by `setInterval`. The handle is what `setInterval` returned." },
    Entry { name: "print", detail: "print(x)", doc: "Printf-debugging. Reaches the dev overlay, not just stderr." },
    Entry { name: "debug", detail: "debug(x)", doc: "Like `print`, with the value's structure shown." },
];

/// The pseudo-classes a selector may name, with the one line each needs.
///
/// The *names* are owned by [`rux_style::honored_pseudo_classes`] and pinned
/// against it by `pseudo_classes_match_the_runtime` below; only the prose is
/// declared here, for the same reason the element table is.
const PSEUDO_CLASSES: &[Entry] = &[
    Entry { name: "hover", detail: "the pointer is over this box", doc: "Needs a layout region, so the shell can tell when the pointer enters and leaves it. There is no hover on a touch screen, so it can decorate and must not be the only way to reach something." },
    Entry { name: "focus", detail: "this element has the keyboard", doc: "Reached by tapping a field or by walking the focus order. Worth styling: a focus ring is the only thing on screen that says where typing will go." },
    Entry { name: "active", detail: "a press is down on this box", doc: "Held between press and release, so it is the state a button uses to look pushed." },
    Entry { name: "checked", detail: "a checkbox or radio is on", doc: "Resolved during the build from the input's live state, rather than supplied by the shell as `:hover` and `:active` are." },
    Entry { name: "current", detail: "this link names the path you are on", doc: "Matches an element whose `to` is the route you are currently on. It is the nav highlight, without a signal to track it." },
    Entry { name: "enter-from", detail: "the frame an element enters from", doc: "Under `r-transition`: the style the element holds for exactly one frame as it arrives, and animates away from. The *hidden* end of the animation goes here." },
    Entry { name: "leave-to", detail: "the state an element leaves to", doc: "Under `r-transition`: the style the element animates towards while it is held on screen on its way out. `position: absolute` here takes it out of flow, so the rest of a list closes up under it rather than waiting." },
];

/// Keyword values, per property, for the properties whose values are a closed
/// set of words rather than lengths, colours or lists.
///
/// Hand-kept from `interpret` in `crates/rux-style`, which is where each of
/// these match arms lives. Every property named here is checked against
/// [`rux_style::honored_properties`] by `css_values_name_honored_properties`,
/// which pins the *keys*; the values themselves are prose against code, and are
/// worth re-reading whenever a match arm over there grows an option.
///
/// A property absent from this table is not one without values, it is one whose
/// values are not a closed set: `width` takes a length, `color` a colour,
/// `grid-template-columns` a track list.
const CSS_VALUES: &[(&str, &[&str])] = &[
    ("display", &["block", "flex", "grid", "inline", "none"]),
    // All five, and every one of them honored since v0.7. Before that, four of
    // these were silently `relative`.
    ("position", &["static", "relative", "sticky", "absolute", "fixed"]),
    ("overflow", &["visible", "hidden", "clip", "auto", "scroll"]),
    ("overflow-x", &["visible", "hidden", "clip", "auto", "scroll"]),
    ("overflow-y", &["visible", "hidden", "clip", "auto", "scroll"]),
    ("flex-direction", &["row", "column"]),
    ("flex-wrap", &["nowrap", "wrap", "wrap-reverse"]),
    ("flex", &["none", "auto", "initial"]),
    ("flex-basis", &["auto", "content"]),
    ("justify-content", &["flex-start", "start", "center", "flex-end", "end", "space-between", "space-around"]),
    ("align-content", &["flex-start", "start", "center", "flex-end", "end", "space-between", "space-around"]),
    ("align-items", &["flex-start", "start", "center", "flex-end", "end", "stretch"]),
    ("align-self", &["flex-start", "start", "center", "flex-end", "end", "stretch"]),
    ("justify-self", &["flex-start", "start", "center", "flex-end", "end", "stretch"]),
    ("justify-items", &["flex-start", "start", "center", "flex-end", "end", "stretch"]),
    ("grid-auto-flow", &["row", "column", "row dense", "column dense"]),
    ("text-align", &["left", "start", "center", "right", "end", "justify"]),
    ("font-weight", &["normal", "bold", "lighter", "bolder", "100", "200", "300", "400", "500", "600", "700", "800", "900"]),
    ("font-style", &["normal", "italic", "oblique"]),
    ("white-space", &["normal", "nowrap", "pre"]),
    ("text-decoration", &["none", "underline", "line-through"]),
    ("text-decoration-line", &["none", "underline", "line-through"]),
    ("overflow-wrap", &["normal", "break-word", "anywhere"]),
    ("word-wrap", &["normal", "break-word", "anywhere"]),
    ("word-break", &["normal", "break-all"]),
    ("cursor", &["default", "pointer"]),
    ("fill-rule", &["nonzero", "evenodd"]),
    ("stroke-linecap", &["butt", "round", "square"]),
    ("stroke-linejoin", &["miter", "round", "bevel"]),
];

/// The easing keywords a `transition` may name. `cubic-bezier(a, b, c, d)` is
/// accepted as well and is not a keyword, so it is not offered as one.
const EASINGS: &[&str] = &["linear", "ease", "ease-in", "ease-out", "ease-in-out"];

/// What `query()` hands back. Properties are reads of the frame already laid
/// out; the actions record an intent and are applied after the handler returns.
///
/// Kept here with the rest of the vocabulary because an editor offering
/// `el.innerHTML` would be offering the web, and this is not the web.
const ELEMENT_PROPERTIES: &[Entry] = &[
    Entry { name: "tag", detail: "the element's tag name", doc: "`\"view\"`, `\"text\"`, and so on." },
    Entry { name: "id", detail: "its `id`, or absent", doc: "**Absent rather than empty** when it has none, so `el.id ?? \"none\"` reads the way it does everywhere else." },
    Entry { name: "classes", detail: "its classes, as an array", doc: "Every class on the element, in the order written." },
    Entry { name: "x", detail: "left edge, in pixels", doc: "From the frame **currently on screen**, so it is one frame stale. That is the guarantee, not a defect: a handler runs before the next layout, exactly as `getBoundingClientRect` does. Absent when there is no frame yet, so \"not laid out\" stays distinguishable from \"laid out and genuinely zero\"." },
    Entry { name: "y", detail: "top edge, in pixels", doc: "See `x`: one frame stale, absent before the first layout." },
    Entry { name: "width", detail: "width, in pixels", doc: "See `x`: one frame stale, absent before the first layout." },
    Entry { name: "height", detail: "height, in pixels", doc: "See `x`: one frame stale, absent before the first layout." },
];

/// The actions on an element handle. Each returns nothing.
const ELEMENT_METHODS: &[Entry] = &[
    Entry { name: "focus", detail: "focus()", doc: "Give this element keyboard focus. Recorded and applied **after the handler finishes**, so a handler that focuses something and then changes state does not race its own tree." },
    Entry { name: "scrollIntoView", detail: "scrollIntoView()", doc: "Scroll whichever box scrolls until this element is visible. Applied after the handler, like the rest." },
    Entry { name: "tap", detail: "tap()", doc: "Tap this element as though a finger had. This is how a `mounted` hook drives the document, and how most of the lifecycle was verified." },
];

/// Methods on strings and arrays: JavaScript's names, deliberately.
///
/// Worth publishing because rhai's own string methods are **in-place mutators**
/// with some of the same names, and Rux shadows them with returning versions.
/// An editor completing from rhai's list would be completing a different
/// language that happens to share the spelling.
const VALUE_METHODS: &[Entry] = &[
    Entry { name: "map", detail: "map(|x| …)", doc: "A new array, each item transformed. Returns; the receiver is untouched." },
    Entry { name: "filter", detail: "filter(|x| …)", doc: "A new array of the items that passed." },
    Entry { name: "reduce", detail: "reduce(|acc, x| …, start)", doc: "Fold the array to one value." },
    Entry { name: "forEach", detail: "forEach(|item, index| …)", doc: "Called with `(item, index)`, and falls back to `(item)`." },
    Entry { name: "find", detail: "find(|x| …)", doc: "The first item that passed, or absent." },
    Entry { name: "includes", detail: "includes(x)", doc: "Whether the array or string contains it." },
    Entry { name: "indexOf", detail: "indexOf(x)", doc: "Where it first occurs, or -1." },
    Entry { name: "slice", detail: "slice(start, end)", doc: "A section, without changing the original." },
    Entry { name: "split", detail: "split(sep)", doc: "A string to an array of pieces." },
    Entry { name: "join", detail: "join(sep)", doc: "An array to one string." },
    Entry { name: "charAt", detail: "charAt(i)", doc: "The character at an index." },
    Entry { name: "repeat", detail: "repeat(n)", doc: "The string over again, n times." },
    Entry { name: "startsWith", detail: "startsWith(s)", doc: "Whether it begins with that." },
    Entry { name: "endsWith", detail: "endsWith(s)", doc: "Whether it ends with that." },
    Entry { name: "trim", detail: "trim()", doc: "**Returns** the trimmed string and leaves the receiver alone. rhai's own `trim` empties the string it was given and returns nothing, which rendered `{{ name.trim() }}` blank; Rux shadows it." },
    Entry { name: "toLowerCase", detail: "toLowerCase()", doc: "Returns a new string." },
    Entry { name: "toUpperCase", detail: "toUpperCase()", doc: "Returns a new string." },
    Entry { name: "length", detail: "length", doc: "A **property, not `len()`**, and only on arrays and strings." },
];

/// What each honored CSS property actually does, and where it applies.
///
/// The completion list used to show "honored by the runtime" beside every one
/// of these, which answers a question nobody asked: it says the editor approves
/// of the word, and nothing about what setting it will do.
///
/// The names are checked against [`rux_style::honored_properties`] by
/// `every_honored_property_is_described` below, so a property added to the
/// runtime cannot reach the editor undocumented.
///
/// Where Rux differs from CSS, the difference **is** the sentence: the `block`
/// default, `align-items` starting at `flex-start`, `overflow` being what makes
/// a scroller, a `transform` becoming a containing block. Those are the places
/// a general CSS reference is actively wrong here.
const CSS_PROPERTY_DOCS: &[Entry] = &[
    Entry { name: "display", detail: "how this box lays its children out", doc: "`block` (the default), `flex`, `grid`, `inline` or `none`. **Rux defaults to `block`**, so a box does not lay its children out in a row until you say `display: flex`." },
    Entry { name: "width", detail: "how wide the box is", doc: "A length or a percentage of the parent's width. `100%` is the usual way to make a child fill a flex parent, because Rux's cross-axis default is `flex-start` rather than CSS's `stretch`." },
    Entry { name: "height", detail: "how tall the box is", doc: "A length or a percentage. A definite height is half of what makes a scroller; the other half is `overflow`." },
    Entry { name: "gap", detail: "space between children", doc: "On a flex or grid parent. Space *between* items only, never outside them, which is what makes it different from margin." },
    Entry { name: "min-width", detail: "a width it will not go under", doc: "Stops a flex item shrinking past this. `min-width: 0` is the escape hatch when an item refuses to shrink at all." },
    Entry { name: "max-width", detail: "a width it will not go over", doc: "Caps the box however much room there is. Common on text so a line does not run the full window." },
    Entry { name: "min-height", detail: "a height it will not go under", doc: "Useful on a scroller that must keep its size when empty." },
    Entry { name: "max-height", detail: "a height it will not go over", doc: "Caps growth; pair with `overflow` to scroll the excess rather than clip it." },
    Entry { name: "padding", detail: "space inside the border, all four sides", doc: "One to four lengths, clockwise from the top. Inside the background, so padding is painted in the box's own colour." },
    Entry { name: "padding-top", detail: "space inside the top edge", doc: "Overrides the `padding` shorthand for this side." },
    Entry { name: "padding-right", detail: "space inside the right edge", doc: "Overrides the `padding` shorthand for this side." },
    Entry { name: "padding-bottom", detail: "space inside the bottom edge", doc: "Overrides the `padding` shorthand for this side." },
    Entry { name: "padding-left", detail: "space inside the left edge", doc: "Overrides the `padding` shorthand for this side." },
    Entry { name: "margin", detail: "space outside the box, all four sides", doc: "One to four lengths, clockwise from the top. Outside the background, so it shows the parent through." },
    Entry { name: "margin-top", detail: "space outside the top edge", doc: "Overrides the `margin` shorthand for this side." },
    Entry { name: "margin-right", detail: "space outside the right edge", doc: "Overrides the `margin` shorthand for this side." },
    Entry { name: "margin-bottom", detail: "space outside the bottom edge", doc: "Overrides the `margin` shorthand for this side." },
    Entry { name: "margin-left", detail: "space outside the left edge", doc: "Overrides the `margin` shorthand for this side." },
    Entry { name: "border", detail: "width, style and colour of the outline", doc: "`2px solid #45475a`. The style word is accepted and only the width and colour are drawn." },
    Entry { name: "border-width", detail: "how thick the outline is", doc: "Takes room in the layout, so changing it moves the content inside." },
    Entry { name: "border-color", detail: "what colour the outline is", doc: "Animatable, so it is the usual way to show focus without the box moving." },
    Entry { name: "border-radius", detail: "how round the corners are", doc: "One to four lengths. Clips the background and the border; it does **not** clip children unless the box also has `overflow: hidden`." },
    Entry { name: "border-top-left-radius", detail: "roundness of one corner", doc: "Overrides `border-radius` for this corner." },
    Entry { name: "border-top-right-radius", detail: "roundness of one corner", doc: "Overrides `border-radius` for this corner." },
    Entry { name: "border-bottom-right-radius", detail: "roundness of one corner", doc: "Overrides `border-radius` for this corner." },
    Entry { name: "border-bottom-left-radius", detail: "roundness of one corner", doc: "Overrides `border-radius` for this corner." },
    Entry { name: "border-top", detail: "the top edge's width and colour", doc: "The one-side form of the `border` shorthand." },
    Entry { name: "border-right", detail: "the right edge's width and colour", doc: "The one-side form of the `border` shorthand." },
    Entry { name: "border-bottom", detail: "the bottom edge's width and colour", doc: "The one-side form of the `border` shorthand. A cheap way to draw a divider under a row." },
    Entry { name: "border-left", detail: "the left edge's width and colour", doc: "The one-side form of the `border` shorthand. A cheap way to draw a quote bar." },
    Entry { name: "border-top-width", detail: "thickness of the top edge", doc: "Set on its own, leaving the colour to the shorthand." },
    Entry { name: "border-right-width", detail: "thickness of the right edge", doc: "Set on its own, leaving the colour to the shorthand." },
    Entry { name: "border-bottom-width", detail: "thickness of the bottom edge", doc: "Set on its own, leaving the colour to the shorthand." },
    Entry { name: "border-left-width", detail: "thickness of the left edge", doc: "Set on its own, leaving the colour to the shorthand." },
    Entry { name: "overflow", detail: "what happens to content too big for the box", doc: "`auto` or `scroll` makes this box **the scroller**; `hidden` or `clip` cuts the overflow off. **Nothing in Rux scrolls until a box is told to**, and the box told is the one with a definite size." },
    Entry { name: "overflow-x", detail: "the same, for the horizontal axis only", doc: "Use when a row should scroll sideways while the page does not." },
    Entry { name: "overflow-y", detail: "the same, for the vertical axis only", doc: "The usual half: a list that scrolls while the screen around it stays put." },
    Entry { name: "opacity", detail: "how see-through the whole box is", doc: "`0` to `1`, and it applies to the **subtree**, not just this box. Animatable, and the cheapest thing to animate." },
    Entry { name: "cursor", detail: "the pointer shape over this box", doc: "Only `pointer` differs from the default arrow. Applied by the shell on hover." },
    Entry { name: "box-shadow", detail: "a shadow cast by the box", doc: "`x y blur colour`. Painted outside the border and takes no space in the layout." },
    Entry { name: "transform", detail: "move, rotate or scale the painted box", doc: "`translate`, `rotate`, `scale`, composed left to right. Paint-only, so it never moves anything else. **A transformed box becomes the containing block** for `absolute` *and* `fixed` descendants, which is why a `fixed` child stops being fixed inside one." },
    Entry { name: "transition", detail: "animate a property when it changes", doc: "`<property> <duration> <easing>`, comma-separated for more than one. Only some properties can animate; naming one that cannot is a warning rather than silence." },
    Entry { name: "flex", detail: "grow, shrink and basis in one", doc: "`flex: 1` means `1 1 0%`, not `1 1 auto`. The usual way to say \\\"take the leftover room\\\"." },
    Entry { name: "flex-grow", detail: "share of the spare room this item takes", doc: "`0` (the default) means it never grows." },
    Entry { name: "flex-shrink", detail: "how readily this item gives room up", doc: "`0` stops it shrinking below its content." },
    Entry { name: "flex-basis", detail: "the size to start from before growing", doc: "`auto` uses the item's own size; a length overrides it." },
    Entry { name: "flex-wrap", detail: "whether items may move onto another line", doc: "`wrap` or `wrap-reverse` allow it; anything else keeps one line." },
    Entry { name: "flex-direction", detail: "which way the children are laid out", doc: "`row` (the default) or `column`. This is the axis every alignment property below is described against." },
    Entry { name: "justify-content", detail: "alignment along the main axis", doc: "The axis `flex-direction` set. On a `row`, this is horizontal." },
    Entry { name: "align-items", detail: "alignment across the main axis", doc: "On a `row`, this is vertical. **Rux defaults to `flex-start`, not CSS's `stretch`**, so a child hugs its content unless told to fill." },
    Entry { name: "align-self", detail: "this one item's cross-axis alignment", doc: "Overrides the parent's `align-items` for this item only. `align-self: flex-end` is how one row in a list moves to the far side." },
    Entry { name: "justify-self", detail: "this one item's main-axis alignment", doc: "Grid mostly; a single item's override." },
    Entry { name: "justify-items", detail: "the default main-axis alignment for children", doc: "Grid mostly; set on the parent." },
    Entry { name: "align-content", detail: "how whole lines are spaced", doc: "Only does anything with more than one line, so it needs `flex-wrap: wrap`." },
    Entry { name: "row-gap", detail: "space between rows", doc: "Overrides `gap` on the vertical axis." },
    Entry { name: "column-gap", detail: "space between columns", doc: "Overrides `gap` on the horizontal axis." },
    Entry { name: "grid-template-columns", detail: "the column tracks", doc: "`repeat(3, 1fr)`, `200px 1fr`, and so on. Setting this is most of what makes a grid." },
    Entry { name: "grid-template-rows", detail: "the row tracks", doc: "Explicit row sizes; rows beyond these come from `grid-auto-rows`." },
    Entry { name: "grid-column", detail: "which columns this item spans", doc: "`grid-column: 1 / 3`, or `span 2`." },
    Entry { name: "grid-row", detail: "which rows this item spans", doc: "`grid-row: 1 / 3`, or `span 2`." },
    Entry { name: "grid-column-start", detail: "the column this item starts at", doc: "The longhand half of `grid-column`." },
    Entry { name: "grid-column-end", detail: "the column this item ends at", doc: "The longhand half of `grid-column`." },
    Entry { name: "grid-row-start", detail: "the row this item starts at", doc: "The longhand half of `grid-row`." },
    Entry { name: "grid-row-end", detail: "the row this item ends at", doc: "The longhand half of `grid-row`." },
    Entry { name: "grid-auto-flow", detail: "how items without a place are filled in", doc: "`row` or `column`, plus `dense` to backfill gaps." },
    Entry { name: "grid-auto-rows", detail: "the size of rows nobody declared", doc: "Applies to rows created beyond `grid-template-rows`." },
    Entry { name: "grid-auto-columns", detail: "the size of columns nobody declared", doc: "Applies to columns created beyond `grid-template-columns`." },
    Entry { name: "position", detail: "how this box is placed", doc: "`static` (the default), `relative`, `sticky`, `absolute` or `fixed`. **All five are honored as of v0.7**; before that four of them parsed and silently behaved as `relative`." },
    Entry { name: "top", detail: "distance from the top edge", doc: "An offset for `relative`/`absolute`/`fixed`, and a **threshold** for `sticky`: where it stops and holds." },
    Entry { name: "right", detail: "distance from the right edge", doc: "An offset, or a sticky threshold. `absolute` measures from the nearest positioned ancestor, not the parent." },
    Entry { name: "bottom", detail: "distance from the bottom edge", doc: "An offset, or a sticky threshold." },
    Entry { name: "left", detail: "distance from the left edge", doc: "An offset, or a sticky threshold." },
    Entry { name: "aspect-ratio", detail: "width-to-height ratio to keep", doc: "`16 / 9`. Gives a box a height derived from its width, so a media box keeps its shape as it resizes." },
    Entry { name: "background", detail: "the colour or gradient behind the content", doc: "A colour, `linear-gradient(…)` or `radial-gradient(…)`. `url()` is not supported; use `<image>` for a picture." },
    Entry { name: "background-color", detail: "a flat colour behind the content", doc: "Animatable, so it is what a hover or pressed state usually changes." },
    Entry { name: "background-image", detail: "a gradient behind the content", doc: "`linear-gradient` and `radial-gradient` only." },
    Entry { name: "color", detail: "the colour of the text", doc: "Inherited, so setting it on a container covers the text inside. Animatable." },
    Entry { name: "font-size", detail: "how large the text is", doc: "Also the reference for `em` on this box, so changing it moves anything sized in `em`." },
    Entry { name: "font-weight", detail: "how heavy the text is", doc: "`normal`, `bold`, `lighter`, `bolder`, or a number from 100 to 900." },
    Entry { name: "font-family", detail: "which typeface to use", doc: "Falls back through the list to whatever the system has." },
    Entry { name: "font-style", detail: "upright or italic", doc: "`italic` and `oblique` both slant; anything else is upright." },
    Entry { name: "text-align", detail: "how lines sit within the box", doc: "`start`, `center`, `end`/`right`, or `justify`. Aligns the **lines inside** the box, which is not the same as moving the box." },
    Entry { name: "letter-spacing", detail: "extra space between characters", doc: "A length, positive or negative." },
    Entry { name: "word-spacing", detail: "extra space between words", doc: "A length, positive or negative." },
    Entry { name: "line-height", detail: "the height of one line of text", doc: "A number (a multiple of the font size) or a length. The usual way to give body text room to breathe." },
    Entry { name: "white-space", detail: "whether runs of space collapse and lines wrap", doc: "`nowrap` and `pre` keep the text on one line; anything else wraps normally." },
    Entry { name: "text-decoration", detail: "underline or strike-through", doc: "`underline` and `line-through` are drawn." },
    Entry { name: "text-decoration-line", detail: "which decoration to draw", doc: "The longhand of `text-decoration`." },
    Entry { name: "overflow-wrap", detail: "whether a long word may break mid-word", doc: "`break-word` or `anywhere` let an unbreakable word break rather than overflow." },
    Entry { name: "word-wrap", detail: "the older name for `overflow-wrap`", doc: "Accepted as a synonym." },
    Entry { name: "word-break", detail: "how aggressively words break", doc: "`break-all` breaks anywhere at all, not only to avoid an overflow." },
    Entry { name: "fill", detail: "the colour inside the path", doc: "On `<path>`. Paint is CSS here rather than an SVG attribute, which is what gives a path `:hover`, `:class` and `transition`. Animatable." },
    Entry { name: "fill-rule", detail: "which parts count as inside", doc: "`nonzero` (the default) or `evenodd`. The difference shows on a shape that crosses itself, like a star." },
    Entry { name: "stroke", detail: "the colour of the outline", doc: "On `<path>`. Animatable." },
    Entry { name: "stroke-width", detail: "how thick the outline is", doc: "On `<path>`. Animatable, so a line can thicken on hover." },
    Entry { name: "stroke-linecap", detail: "the shape of a line's ends", doc: "`butt` (the default), `round` or `square`." },
    Entry { name: "stroke-linejoin", detail: "the shape where two segments meet", doc: "`miter` (the default), `round` or `bevel`." },
];

/// One worked line per property, for the popup beside the completion list.
///
/// The description says what a property does and where it applies; neither
/// shows a line anyone could type. A declaration in context answers "how do I
/// use this" faster than a sentence about it, and several of these carry the
/// pairing that actually matters: `overflow` next to a `height`, `top` next to
/// a `position: sticky`, `align-items` next to the `display: flex` without
/// which it does nothing at all.
///
/// Pinned against the description table by `every_property_shows_how_it_is_used`.
const CSS_PROPERTY_USAGE: &[(&str, &str)] = &[
    ("align-content", ".wrapped { flex-wrap: wrap; align-content: flex-start; }"),
    ("align-items", ".row { display: flex; align-items: center; }"),
    ("align-self", ".mine { align-self: flex-end; }"),
    ("aspect-ratio", ".cover { width: 100%; aspect-ratio: 16 / 9; }"),
    ("background", ".app { background: #1e1e2e; }"),
    ("background-color", ".send:hover { background-color: #b4befe; }"),
    ("background-image", ".hero { background-image: linear-gradient(#89b4fa, #1e1e2e); }"),
    ("border", ".field { border: 2px solid #45475a; }"),
    ("border-bottom", ".row { border-bottom: 1px solid #313244; }"),
    ("border-bottom-left-radius", ".bubble { border-bottom-left-radius: 2px; }"),
    ("border-bottom-right-radius", ".bubble { border-bottom-right-radius: 2px; }"),
    ("border-bottom-width", ".row { border-bottom-width: 1px; }"),
    ("border-color", ".field:focus { border-color: #89b4fa; }"),
    ("border-left", ".quote { border-left: 3px solid #89b4fa; }"),
    ("border-left-width", ".row { border-left-width: 3px; }"),
    ("border-radius", ".card { border-radius: 12px; }"),
    ("border-right", ".sidebar { border-right: 1px solid #313244; }"),
    ("border-right-width", ".row { border-right-width: 1px; }"),
    ("border-top", ".footer { border-top: 1px solid #313244; }"),
    ("border-top-left-radius", ".bubble { border-top-left-radius: 2px; }"),
    ("border-top-right-radius", ".bubble { border-top-right-radius: 2px; }"),
    ("border-top-width", ".row { border-top-width: 1px; }"),
    ("border-width", ".field { border-width: 2px; }"),
    ("bottom", ".bar { position: fixed; bottom: 0; }"),
    ("box-shadow", ".modal { box-shadow: 0 8px 24px #00000066; }"),
    ("color", ".label { color: #cdd6f4; }"),
    ("column-gap", ".grid { column-gap: 8px; }"),
    ("cursor", ".send { cursor: pointer; }"),
    ("display", ".row { display: flex; }"),
    ("fill", ".mark { fill: #f9e2af; }"),
    ("fill-rule", ".star { fill-rule: evenodd; }"),
    ("flex", ".field { flex: 1; }"),
    ("flex-basis", ".col { flex-basis: 200px; }"),
    ("flex-direction", ".app { display: flex; flex-direction: column; }"),
    ("flex-grow", ".field { flex-grow: 1; }"),
    ("flex-shrink", ".icon { flex-shrink: 0; }"),
    ("flex-wrap", ".tags { display: flex; flex-wrap: wrap; }"),
    ("font-family", ".app { font-family: Inter, sans-serif; }"),
    ("font-size", ".title { font-size: 22px; }"),
    ("font-style", ".empty { font-style: italic; }"),
    ("font-weight", ".title { font-weight: 700; }"),
    ("gap", ".row { display: flex; gap: 8px; }"),
    ("grid-auto-columns", ".grid { grid-auto-columns: 1fr; }"),
    ("grid-auto-flow", ".grid { grid-auto-flow: column; }"),
    ("grid-auto-rows", ".grid { grid-auto-rows: 80px; }"),
    ("grid-column", ".wide { grid-column: 1 / 3; }"),
    ("grid-column-end", ".cell { grid-column-end: 4; }"),
    ("grid-column-start", ".cell { grid-column-start: 2; }"),
    ("grid-row", ".tall { grid-row: span 2; }"),
    ("grid-row-end", ".cell { grid-row-end: 3; }"),
    ("grid-row-start", ".cell { grid-row-start: 1; }"),
    ("grid-template-columns", ".grid { display: grid; grid-template-columns: repeat(3, 1fr); }"),
    ("grid-template-rows", ".grid { grid-template-rows: auto 1fr auto; }"),
    ("height", ".thread { height: 300px; }"),
    ("justify-content", ".row { display: flex; justify-content: space-between; }"),
    ("justify-items", ".grid { justify-items: start; }"),
    ("justify-self", ".cell { justify-self: center; }"),
    ("left", ".badge { position: absolute; left: 8px; }"),
    ("letter-spacing", ".caps { letter-spacing: 0.5px; }"),
    ("line-height", ".body { line-height: 20px; }"),
    ("margin", ".card { margin: 0 auto; }"),
    ("margin-bottom", ".card { margin-bottom: 8px; }"),
    ("margin-left", ".card { margin-left: 8px; }"),
    ("margin-right", ".card { margin-right: 8px; }"),
    ("margin-top", ".card { margin-top: 8px; }"),
    ("max-height", ".menu { max-height: 240px; overflow-y: auto; }"),
    ("max-width", ".msg { max-width: 320px; }"),
    ("min-height", ".panel { min-height: 120px; }"),
    ("min-width", ".cell { min-width: 0; }"),
    ("opacity", ".msg:enter-from { opacity: 0; }"),
    ("overflow", ".thread { height: 300px; overflow: auto; }"),
    ("overflow-wrap", ".body { overflow-wrap: break-word; }"),
    ("overflow-x", ".tabs { overflow-x: auto; }"),
    ("overflow-y", ".thread { height: 300px; overflow-y: auto; }"),
    ("padding", ".card { padding: 12px 16px; }"),
    ("padding-bottom", ".card { padding-bottom: 12px; }"),
    ("padding-left", ".card { padding-left: 16px; }"),
    ("padding-right", ".card { padding-right: 16px; }"),
    ("padding-top", ".card { padding-top: 12px; }"),
    ("position", ".heading { position: sticky; top: 0; }"),
    ("right", ".badge { position: absolute; right: 8px; }"),
    ("row-gap", ".grid { row-gap: 12px; }"),
    ("stroke", ".mark { stroke: #1e1e2e; }"),
    ("stroke-linecap", ".mark { stroke-linecap: round; }"),
    ("stroke-linejoin", ".mark { stroke-linejoin: round; }"),
    ("stroke-width", ".mark { stroke-width: 2; }"),
    ("text-align", ".title { text-align: center; }"),
    ("text-decoration", ".link { text-decoration: underline; }"),
    ("text-decoration-line", ".done { text-decoration-line: line-through; }"),
    ("top", ".heading { position: sticky; top: 0; }"),
    ("transform", ".msg:enter-from { transform: translateY(10px); }"),
    ("transition", ".msg { transition: opacity 180ms ease-out; }"),
    ("white-space", ".label { white-space: nowrap; }"),
    ("width", ".field { width: 100%; }"),
    ("word-break", ".code { word-break: break-all; }"),
    ("word-spacing", ".lead { word-spacing: 1px; }"),
    ("word-wrap", ".body { word-wrap: break-word; }"),
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
    out.push_str(",\n  \"elementProperties\": ");
    entries(&mut out, ELEMENT_PROPERTIES, 1);
    out.push_str(",\n  \"elementMethods\": ");
    entries(&mut out, ELEMENT_METHODS, 1);
    out.push_str(",\n  \"valueMethods\": ");
    entries(&mut out, VALUE_METHODS, 1);

    out.push_str(",\n  \"pseudoClasses\": ");
    entries(&mut out, PSEUDO_CLASSES, 1);

    out.push_str(",\n  \"voidTags\": ");
    strings(&mut out, rux_fmt::void_tags());
    out.push_str(",\n  \"cssProperties\": ");
    strings(&mut out, rux_style::honored_properties());
    out.push_str(",\n  \"cssPropertyDocs\": ");
    property_docs(&mut out);
    out.push_str(",\n  \"cssValues\": {\n");
    for (i, (property, values)) in CSS_VALUES.iter().enumerate() {
        out.push_str(&format!("    {}: ", quote(property)));
        inline_strings(&mut out, values);
        out.push_str(if i + 1 == CSS_VALUES.len() { "\n" } else { ",\n" });
    }
    out.push_str("  },\n  \"animatableProperties\": ");
    strings(&mut out, &rux_style::animatable_properties());
    out.push_str(",\n  \"easings\": ");
    strings(&mut out, EASINGS);
    out.push_str("\n}\n");

    print!("{out}");
    0
}

/// The property table, with each entry's worked example alongside it.
///
/// Written here rather than through [`entries`] because these carry a fourth
/// field and nothing else does; widening `Entry` for one list would put an
/// always-empty `usage` on every element and directive.
fn property_docs(out: &mut String) {
    out.push_str("[\n");
    for (i, e) in CSS_PROPERTY_DOCS.iter().enumerate() {
        let usage = CSS_PROPERTY_USAGE
            .iter()
            .find(|(name, _)| *name == e.name)
            .map(|(_, example)| *example)
            .unwrap_or("");
        out.push_str(&format!(
            "    {{ \"name\": {}, \"detail\": {}, \"doc\": {}, \"usage\": {} }}",
            quote(e.name),
            quote(e.detail),
            quote(&squash(e.doc)),
            quote(usage),
        ));
        out.push_str(if i + 1 == CSS_PROPERTY_DOCS.len() { "\n" } else { ",\n" });
    }
    out.push_str("  ]");
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

/// A JSON array of bare strings on one line, for the short per-property
/// value lists where wrapping would cost more than it gives.
fn inline_strings(out: &mut String, list: &[&str]) {
    let quoted: Vec<String> = list.iter().map(|s| quote(s)).collect();
    out.push_str(&format!("[{}]", quoted.join(", ")));
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

    /// `PSEUDO_CLASSES` carries the prose; `rux_style` owns the names. If the
    /// runtime learns a pseudo-class and this table does not, the editor stops
    /// offering something that works, and if this table invents one, the editor
    /// offers a rule that silently never matches. Both are failures.
    #[test]
    fn pseudo_classes_match_the_runtime() {
        let ours: Vec<&str> = PSEUDO_CLASSES.iter().map(|e| e.name).collect();
        assert_eq!(
            ours,
            rux_style::honored_pseudo_classes().to_vec(),
            "the pseudo-classes offered and the pseudo-classes matched have drifted"
        );
    }

    /// The *keys* of the value table have to be properties the runtime honors.
    /// Offering values for a property it ignores would be two wrong answers in
    /// one completion.
    #[test]
    fn css_values_name_honored_properties() {
        for (property, values) in CSS_VALUES {
            assert!(
                rux_style::honored_properties().contains(property),
                "`{property}` has values in the vocabulary and is not honored"
            );
            assert!(!values.is_empty(), "`{property}` has an empty value list");
        }
    }

    /// `transition` takes a property name, a duration and an easing. The first
    /// list is the runtime's, and this holds the second one honest against the
    /// same parser.
    #[test]
    fn transition_vocabulary_is_the_runtimes() {
        let animatable = rux_style::animatable_properties();
        assert!(animatable.contains(&"all"), "`all` is legal to write");
        assert!(animatable.contains(&"d"), "`<path>` geometry morphs since v0.7");
        assert!(!animatable.contains(&"display"));
        assert_eq!(EASINGS[0], "linear");
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

    /// The completion list shows `detail` beside a name, and for a script global
    /// that string is the whole answer to "how is this written". Two of them
    /// were wrong for as long as the vocabulary existed: `computed(|| expr)`
    /// and `effect(|| …)`, both of which are a rhai call and neither of which
    /// is what Rux has. `computed name = expr;` is a declaration and
    /// `effect { … }` is a block, and a file written the way the editor
    /// suggested failed to load with "Function not found: computed (Fn)".
    ///
    /// The name-level pinning below could not catch it, because the names were
    /// right. So the forms are pinned too, against the document that defines
    /// them.
    #[test]
    fn script_global_forms_match_the_script_doc() {
        let docs = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/07-script.md"),
        );
        let Ok(docs) = docs else { return }; // not a checkout; nothing to pin against

        // The declaration and block forms, exactly as `07-script.md` writes
        // them. If that document changes its mind, this fails and the
        // completion list is updated with it rather than after it.
        for (name, form) in [("computed", "computed name = expr;"), ("effect", "effect { … }")] {
            let entry = SCRIPT_GLOBALS
                .iter()
                .find(|e| e.name == name)
                .unwrap_or_else(|| panic!("`{name}` is gone from the vocabulary"));
            assert_eq!(
                entry.detail, form,
                "`{name}` is offered as `{}`, and docs/07-script.md writes it `{form}`",
                entry.detail
            );
            assert!(
                docs.contains(form),
                "docs/07-script.md no longer writes `{form}`; the vocabulary follows it, so \
                 update both"
            );
        }

        // Neither is a call, and the completion list must not imply one.
        for name in ["computed", "effect", "mounted", "unmounted"] {
            let entry = SCRIPT_GLOBALS.iter().find(|e| e.name == name).expect("present");
            assert!(
                !entry.detail.contains("(|"),
                "`{name}` is a declaration or a block, and `{}` reads as a call",
                entry.detail
            );
        }
    }

    /// Every property the runtime honors has a description, and nothing is
    /// described that the runtime does not honor.
    ///
    /// Without this the two drift the moment a property is added: the editor
    /// would offer the new name with no help at all, or keep describing one
    /// that has been dropped. The honored list is the owner and this test makes
    /// it the owner in practice.
    #[test]
    fn every_honored_property_is_described() {
        let described: Vec<&str> = CSS_PROPERTY_DOCS.iter().map(|e| e.name).collect();

        let missing: Vec<&&str> = rux_style::honored_properties()
            .iter()
            .filter(|p| !described.contains(p))
            .collect();
        assert!(
            missing.is_empty(),
            "these properties are honored and have no description: {missing:?}.              Add them to CSS_PROPERTY_DOCS."
        );

        let extra: Vec<&&str> = described
            .iter()
            .filter(|p| !rux_style::honored_properties().contains(p))
            .collect();
        assert!(
            extra.is_empty(),
            "these are described and not honored: {extra:?}. The editor would be              offering help for a property the runtime warns does nothing."
        );
    }

    /// Every described property carries a worked example, and every example
    /// names a property that exists.
    ///
    /// The example is the half that answers "how do I use this", and a property
    /// that quietly lost one would show a description with an empty code block
    /// under it, which reads as a bug in the editor.
    #[test]
    fn every_property_shows_how_it_is_used() {
        for e in CSS_PROPERTY_DOCS {
            let usage = CSS_PROPERTY_USAGE.iter().find(|(name, _)| *name == e.name);
            let (_, example) = usage
                .unwrap_or_else(|| panic!("`{}` has a description and no example", e.name));
            assert!(
                example.contains(e.name),
                "`{}`'s example does not use it: {example}",
                e.name
            );
            assert!(example.contains('{') && example.contains('}'), "`{}` is not a rule", e.name);
        }

        let described: Vec<&str> = CSS_PROPERTY_DOCS.iter().map(|e| e.name).collect();
        for (name, _) in CSS_PROPERTY_USAGE {
            assert!(described.contains(name), "`{name}` has an example and no description");
        }
    }

    /// A description that merely says the word is allowed is the thing this
    /// table replaced, so it must not creep back in.
    #[test]
    fn a_description_says_what_the_property_does() {
        for e in CSS_PROPERTY_DOCS {
            assert!(
                !e.detail.contains("honored"),
                "`{}` is described as \"{}\", which says the editor approves of the                  word and nothing about what it does",
                e.name,
                e.detail
            );
            assert!(e.detail.len() > 8, "`{}` has no real description", e.name);
        }
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
