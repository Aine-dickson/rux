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
        detail: "the whole display",
        doc: "The display itself, not a box in it. Wherever it is written, \
              including inside a clipped box or a component a `<route>` names, \
              it covers the device: it defaults to `position: fixed` with every \
              inset at zero.",
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
        doc: "A run of text. A `<text>` inside a `<text>` is part of the same \
              sentence, styled on its own (`Read the <text class=\"b\">terms</text>`), \
              and `<image>` or `<path>` inside one sits in the line as a box, placed by \
              `vertical-align`. Two `<text>` siblings still **stack**: they are two \
              paragraphs. `role=\"heading\"` announces it as a heading.",
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
        doc: "`d` is SVG path data, in the element's own coordinates. Paint is \
              CSS, not attributes: `fill`, `stroke`, `stroke-width`, \
              `stroke-linecap`, `stroke-linejoin`, `fill-rule`. With no CSS size \
              it lays out at the size of its own geometry. `:d` binds an \
              expression, and two paths with the same command sequence animate \
              between one another under `transition: d`.",
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
        name: "audio",
        detail: "a sound, with or without its controls",
        doc: "`<audio src=\"song.mp3\" controls />` plays a sound from the project, or a               picked `File`'s `url`. With `controls` it draws play/pause, the time, a seek               bar and mute; without them it is nothing on screen, as in a browser.               `autoplay` plays only when `muted`, or once the person has pressed something               in the app, on every platform. `query(\"audio\")[0].play()` and `.pause()`               reach it from script.",
    },
    Entry {
        name: "video",
        detail: "a video, with or without its controls",
        doc: "`<video src=\"clip.mp4\" controls />` plays a video from the project, or a \
              picked `File`'s `url`, with its sound, sized by its picture until CSS says \
              otherwise and fitted by `object-fit` (`contain` unless set). `poster` shows \
              until it is played. While it plays its controls hide after a few idle seconds, \
              and their last button fills the screen. Decoded by the platform: Media \
              Foundation on Windows, the browser on the web; elsewhere, for now, it shows its \
              poster and fires `@error`.",
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
    Entry { name: "@press", detail: "a finger or button went down", doc: "Runs when a press lands on this element. `event.x` / `event.y` are relative to the element, `event.pageX` / `event.pageY` to the window, and `event.width` / `event.height` are the element's own size; `event.touches` lists every finger down." },
    Entry { name: "@release", detail: "the press came up", doc: "Runs on release, whether or not the press stayed still enough to be a tap." },
    Entry { name: "@longpress", detail: "the press rested", doc: "Runs once, after the press has been held still for half a second." },
    Entry { name: "@swipe", detail: "a flick, with a direction", doc: "Runs once at the end of a press that travelled far enough, fast enough. `event.direction` is `left`, `right`, `up` or `down`, and `event.totalX` / `event.totalY` are how far it came. A drag that ends as a flick fires this too." },
    Entry { name: "@drag", detail: "the pointer is moving", doc: "Runs at the start of a drag, on every move, and at the end. `event.phase` says which; `event.totalX` / `event.totalY` measure from where the press landed, `event.moveX` / `event.moveY` from the previous event. A `@drag` claims the finger, so the page under it does not scroll." },
    Entry { name: "class", detail: "CSS classes", doc: "Space-separated class names, matched by `.name` selectors. `:class` is the bound form." },
    Entry { name: "id", detail: "unique id", doc: "Matched by `#name` selectors, and by `query(\"#name\")` from script." },
    Entry { name: "style", detail: "inline declarations", doc: "Inline CSS for this element. `:style` is the bound form." },
    Entry { name: "label", detail: "accessible name", doc: "What a screen reader calls this element, in place of the text inside it. On an `<image>` or `<path>`, `alt` says the same." },
    Entry { name: "role", detail: "accessibility role", doc: "Honored for selectors (`[role=\"heading\"]`) and for the accessibility tree. Matches case-insensitively." },
    Entry { name: "dir", detail: "which way its text and rows run", doc: "`ltr`, `rtl` or `auto`, inherited. Sets `direction`: flex rows, grid columns and `text-align: start` begin from the right under `rtl`, and logical properties such as `margin-inline-start` follow it. `auto` leaves a text to its first letter with a direction. A `direction` rule overrides it." },
    Entry { name: "to", detail: "make this element a link", doc: "Tapping navigates to this path, the element announces as a link, and it matches `:current` when it names the path you are on. `:to` is the bound form." },
    Entry { name: "@submit", detail: "the form was sent, and every field passed", doc: "On a `role=\"form\"` only. Sent by a `<button type=\"submit\">` inside it or by the action key in its last field. `event.values` maps each field's `name` (or its `r-model`) to its value." },
    Entry { name: "@invalid", detail: "the form was sent, and a field failed", doc: "On a `role=\"form\"` only. Runs instead of `@submit`: the first failing field takes the caret and every field matches `:user-invalid` from then on. `event.errors` maps each failing field's name to a sentence saying why; `event.values` is as for `@submit`." },
    Entry { name: "@key", detail: "a key was pressed here", doc: "Runs while focus is on this element or inside it. `event.key` is the key by the browser's names (`\"s\"`, `\"Enter\"`, `\"Escape\"`), with `event.ctrl`, `shift`, `alt`, `meta`, `primary` (Ctrl, or Cmd on macOS) and `repeat`. **`false` takes the key**: no handler further out runs, and the shell does nothing with it. A focused text field handles the keys it uses (text, editing, its Ctrl chords) before the `@key`s around it." },
    Entry { name: "@close", detail: "the person asked to close the app", doc: "On the app's `<screen>` only. Runs when the window's close button is pressed, or Android's Back has no page left. **`false` keeps the app open**, as a route guard's `false` refuses; anything else lets it close, and one that fails lets it close too. Open your own \"save changes?\" and call `close()` from it. Must answer at once, so an `async fn` cannot refuse. Not asked on the web yet." },
];

/// The structural directives. These are attributes, but they are the ones worth
/// ranking first in a completion list: they are what makes a template a
/// template rather than a picture.
const DIRECTIVES: &[Entry] = &[
    Entry { name: "r-for", detail: "repeat this element per item", doc: "`r-for=\"item in items\"`, or `r-for=\"(item, i) in items\"` for the index too. An `r-if` beside it filters the rows. Pair it with `r-key` so reconciliation can match rows across a change." },
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
            Entry { name: "src", detail: "path, relative to this .rux file, or a URL", doc: "Resolved relative to the document, not the working directory. An `https://` address is fetched without holding the window up. `:src` binds an expression." },
            Entry { name: "alt", detail: "accessible description", doc: "What the image is, for the accessibility tree." },
            Entry { name: "srcset", detail: "pictures for different screens", doc: "`a.png 1x, a@2x.png 2x`, or widths, `s.png 400w, l.png 1600w` with `sizes`. The one nearest above the screen's density is shown, `src` when none is given." },
            Entry { name: "sizes", detail: "how wide the image is drawn, for `srcset`'s widths", doc: "`(max-width: 600px) 100vw, 300px`: the first whose condition holds, else the last. The whole viewport when absent." },
            Entry { name: "loading", detail: "`lazy`: fetch only when near the window", doc: "`loading=\"lazy\"` fetches a network picture once its box comes within 1250 px of the window, as Chromium does. Give it a size, or the page moves when it arrives." },
            Entry { name: "@load", detail: "the picture is there", doc: "Once per `src`. `event.width` and `event.height` are its own size in pixels." },
            Entry { name: "@error", detail: "the picture could not be had", doc: "Once per `src`: missing, refused by the network, or not a picture. `event.message` says which." },
        ],
    ),
    (
        "audio",
        &[
            Entry { name: "src", detail: "the sound: a project path or a File's url", doc: "Read as `loadSound` reads it: a path from the project (built into a release), or a picked `File`'s `url`. WAV, MP3, AAC, FLAC and Ogg Vorbis everywhere; a browser plays what it plays. `:src` binds an expression, and a new value is a new sound." },
            Entry { name: "controls", detail: "draw play, time, seek and mute", doc: "Play/pause, the time, a seek bar to tap or drag, and mute, in the element's `color` and `accent-color`. 300 by 40 unless CSS sizes it." },
            Entry { name: "autoplay", detail: "play once loaded, as a browser allows", doc: "Plays by itself only when `muted`, or once the person has pressed something in the app; otherwise it waits for a tap and says why. The browser's rule, on every platform." },
            Entry { name: "loop", detail: "start again at the end", doc: "Plays from the start again at its end, with no `@ended`." },
            Entry { name: "muted", detail: "start silent", doc: "The volume at nothing. The mute button turns it back on." },
            Entry { name: "preload", detail: "`none`: read nothing until played", doc: "`none` reads nothing until the first play; `auto` and `metadata` (the default) read it at once." },
            Entry { name: "@play", detail: "it started playing", doc: "From the controls, `play()`, `autoplay` or a script. `event.currentTime` and `event.duration` (`none` when not known) say where it is." },
            Entry { name: "@pause", detail: "it stopped playing", doc: "Paused, or at its end (then `@ended` follows). `event.currentTime` and `event.duration`." },
            Entry { name: "@ended", detail: "it played to its end", doc: "After the `@pause` at its end. Not with `loop`." },
            Entry { name: "@timeupdate", detail: "where it is moved", doc: "About four times a second while it plays, and on a seek. `event.currentTime` and `event.duration`." },
            Entry { name: "@error", detail: "it could not be loaded", doc: "`event.kind` (`io`, `unsupported`, `decode`) and `event.message`. A file Rux cannot decode on the desktop (Opus, WebM) is `unsupported`." },
            Entry { name: "@loadedmetadata", detail: "it is loaded and its length known", doc: "Once per `src`. `event.duration` (`none` when not known)." },
        ],
    ),
    (
        "video",
        &[
            Entry { name: "src", detail: "the video: a project path or a File's url", doc: "A path from the project (built into a release), or a picked `File`'s `url`. On Windows, what Media Foundation plays: H.264 always, VP9, HEVC and AV1 with their store extensions. `:src` binds an expression, and a new value is a new video." },
            Entry { name: "poster", detail: "a picture shown until it plays", doc: "An image path, shown in the video's box until the first play or seek. Without it the first frame shows." },
            Entry { name: "controls", detail: "draw play, time, seek and mute over it", doc: "The `<audio>` controls in a bar along the bottom of the picture, in white on a dark band." },
            Entry { name: "autoplay", detail: "play once loaded, as a browser allows", doc: "As `<audio autoplay>`: by itself only when `muted`, or once the person has pressed something in the app." },
            Entry { name: "loop", detail: "start again at the end", doc: "Plays from the start again at its end, with no `@ended`." },
            Entry { name: "muted", detail: "start silent", doc: "The volume at nothing. The mute button turns it back on." },
            Entry { name: "preload", detail: "`none`: read nothing until played", doc: "`none` reads nothing until the first play, and shows only the poster; `auto` and `metadata` (the default) read it at once." },
            Entry { name: "playsinline", detail: "accepted: Rux always plays in the page", doc: "A phone browser's attribute to keep a video in the page instead of full screen, which is the only way Rux plays one." },
            Entry { name: "@play", detail: "it started playing", doc: "As `<audio>`'s. `event.currentTime` and `event.duration`." },
            Entry { name: "@pause", detail: "it stopped playing", doc: "Paused, or at its end (then `@ended` follows)." },
            Entry { name: "@ended", detail: "it played to its end", doc: "After the `@pause` at its end. Not with `loop`." },
            Entry { name: "@timeupdate", detail: "where it is moved", doc: "About four times a second while it plays, and on a seek." },
            Entry { name: "@error", detail: "it could not be loaded", doc: "`event.kind` (`io`, `unsupported`, `decode`) and `event.message`. A format the machine has no decoder for, or any video off Windows and the web for now, is `unsupported`." },
            Entry { name: "@loadedmetadata", detail: "it is loaded and its size known", doc: "Once per `src`. `event.videoWidth`, `event.videoHeight` and `event.duration`." },
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
            Entry { name: "type", detail: "text | textarea | password | search | number | date | select | checkbox | switch | slider | radio | file", doc: "Omitted means a single-line text field." },
            Entry { name: "placeholder", detail: "text shown while empty", doc: "Shown until the field has a value. Not a label." },
            Entry { name: "value", detail: "the field's value", doc: "The literal starting value. For state that changes, use `r-model`." },
            Entry { name: "checked", detail: "checkbox / radio state", doc: "Live state for `type=\"checkbox\"` and `type=\"radio\"`, and matched by the `:checked` pseudo-class." },
            Entry { name: "name", detail: "radio group, and the key a form sends it under", doc: "Radios sharing a `name` are one group, so choosing one clears the others. In a `role=\"form\"`, `event.values` carries the field under this name; without one, under its `r-model`." },
            Entry { name: "required", detail: "must have a value to submit", doc: "Valueless, or bound with `:required`. Empty text, or a checkbox or switch that is off, fails it. Matched by `:required`, and by `:invalid` while it fails." },
            Entry { name: "minlength", detail: "the fewest characters it takes", doc: "Checked when its form is submitted, counted like `maxlength`. An empty field fails only `required`." },
            Entry { name: "pattern", detail: "a regular expression the value matches", doc: "Matches the whole value, as HTML's does: `pattern=\"\\d{4}\"` takes `1906` and refuses `19064`. An empty field fails only `required`." },
            Entry { name: "options", detail: "the choices for a select", doc: "For `type=\"select\"`. `:options` binds an array." },
            Entry { name: "disabled", detail: "cannot be focused, tapped or changed", doc: "On whenever it is written, so `disabled=\"false\"` is still disabled: bind it with `:disabled=\"expr\"` instead. Matched by `:disabled`. Also on `<button>`." },
            Entry { name: "readonly", detail: "can be selected and copied, not changed", doc: "The caret and the selection work and no keyboard opens. `:readonly` binds it." },
            Entry { name: "min", detail: "the lowest value or earliest day", doc: "A number for `type=\"slider\"` (0 when left off), a `YYYY-MM-DD` day for `type=\"date\"`. On `type=\"number\"`, a check its form's submission makes." },
            Entry { name: "max", detail: "the highest value or latest day", doc: "A number above `min` for `type=\"slider\"` (100 when left off), a `YYYY-MM-DD` day for `type=\"date\"`. On `type=\"number\"`, a check its form's submission makes." },
            Entry { name: "step", detail: "what a slider's value moves by", doc: "For `type=\"slider\"`. 1 when left off; `any` for no steps. The value is rounded to as many decimal places as the step is written with." },
            Entry { name: "maxlength", detail: "the most characters it takes", doc: "Counted as HTML counts them, in UTF-16 units. Typing past it is refused and a paste is cut short." },
            Entry { name: "inputmode", detail: "text | numeric | decimal | tel | email | url | search | none", doc: "Which keyboard a phone raises. The field is still a text field and its value still text: a PIN or a phone number is `inputmode=\"numeric\"`, not a number." },
            Entry { name: "enterkeyhint", detail: "enter | done | go | next | previous | search | send", doc: "What the keyboard's action key says. `next` and `previous` move focus like Tab, `done` closes the field, and in a form every other one submits it; every one commits the field first, which fires `@change`. Left off in a form, it is `next` on each typing field and `go` on the last." },
            Entry { name: "autocomplete", detail: "what the field holds, for autofill", doc: "HTML's field names: `username`, `current-password`, `new-password`, `email`, `tel`, `name`, `postal-code`, `cc-number`, `one-time-code` and the rest. A phone's password manager fills the fields of the focused field's form from them, and offers to save after `@submit`. `off` keeps the field away from autofill. Without it, a password, an email keyboard and a phone keyboard are recognised anyway." },
            Entry { name: "autofocus", detail: "take focus when it appears", doc: "Valueless. Focuses the field when it first appears, a route or an `r-if` bringing it in included, unless something already has focus." },
            Entry { name: "accept", detail: "which files a file input offers", doc: "For `type=\"file\"`, as HTML's: extensions (`.png`), media types (`application/pdf`) and families (`image/*`, `video/*`, `audio/*`), comma-separated. A hint to the picker, not a check: `File.type` says what was picked." },
            Entry { name: "multiple", detail: "more than one file", doc: "Valueless, for `type=\"file\"`. The bound signal holds a `File[]`, empty until something is chosen." },
            Entry { name: "capture", detail: "a phone's camera or recorder", doc: "For `type=\"file\"`: `environment` (the back camera) or `user` (the front), with `accept` saying photo, video or sound. A desktop opens the picker, as a desktop browser does." },
            Entry { name: "@input", detail: "the text changed", doc: "After every change, however it was made. `event.value` is the new text." },
            Entry { name: "@change", detail: "a changed value was committed", doc: "When the field is left with a different value than it had, or Enter is pressed in a one-line field. A select fires it on choosing, a checkbox or radio on toggling. `event.value` is the value." },
            Entry { name: "@focus", detail: "the field gained focus", doc: "`event.value` is its text." },
            Entry { name: "@cancel", detail: "a file picker closed with nothing chosen", doc: "On `type=\"file\"` only. The bound value stays what it was." },
            Entry { name: "@blur", detail: "the field lost focus", doc: "After `@change`, when both fire. `event.value` is its text." },
        ],
    ),
    (
        "button",
        &[
            Entry { name: "disabled", detail: "cannot be tapped", doc: "No `@tap`, no link, no gesture, and nothing for Tab to land on. On whenever it is written; `:disabled=\"expr\"` binds it. Matched by `:disabled`." },
            Entry { name: "type", detail: "submit | button", doc: "`submit` sends the `role=\"form\"` around it once its own `@tap` has run: `@submit` if every field passes, `@invalid` if not. `button`, or leaving it off, is an ordinary button." },
        ],
    ),
    (
        "route",
        &[
            Entry { name: "path", detail: "the path to match", doc: "`/crew/:id` captures `id` and hands it to the view as a prop. A child route's path is relative to its parent, and `path=\"\"` is the index route that fills the parent's outlet at the parent's own path." },
            Entry { name: "view", detail: "the component to render", doc: "Names an imported component. Either spelling: `view=\"crew-detail\"` as the tag is written, or `view=\"crew_detail\"` as the `use` that imports it is written. They are the same component." },
            Entry { name: "name", detail: "a name to build this route's path from", doc: "`path_for(\"crew-detail\", { id: \"ada\" })` builds the path from the name and the parameters, so a link survives the path being changed." },
            Entry { name: "fallback", detail: "match anything unmatched", doc: "Valueless. Renders when no other route matched, wherever it sits among them." },
            Entry { name: "guard", detail: "decide whether this route may be entered", doc: "`guard=\"expr\"`, run whenever this route is part of the match. `false` cancels the navigation, a string redirects to that path, and anything else allows it, `()` included, so a function that falls off the end has consented. Outer guards run first. In scope: `to`, `from`, the path's parameters, and `linked`, true when a deep link or a browser URL started the navigation rather than the app itself." },
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

/// Attributes whose value is a **closed set**, so the editor can offer the set
/// rather than leave the author to guess it from a `detail` line reading
/// `text | textarea | select | checkbox | radio`.
///
/// Keyed by tag, then by attribute, so the answer depends on the element the
/// cursor is in and not on the attribute name alone.
///
/// An attribute absent from this table is not one without values, it is one
/// whose values are not a closed set: `placeholder` takes any text, `class` any
/// list of names, `to` any path the router knows.
///
/// The same rule as [`CSS_VALUES`]: these are prose written against code, and
/// `attribute_values_match_their_detail_line` pins each set against the
/// `detail` the same attribute already advertises, so the two cannot drift.
const ATTRIBUTE_VALUES: &[(&str, &[(&str, &[Entry])])] = &[(
    "input",
    &[(
        "type",
        &[
            Entry { name: "text", detail: "a single-line field", doc: "The default, so `type` may be left off entirely." },
            Entry { name: "textarea", detail: "a multi-line field", doc: "Enter inserts a newline instead of being ignored, and the box scrolls its own content." },
            Entry { name: "password", detail: "a masked field", doc: "Shows bullets, refuses Copy and Cut, and asks a phone's keyboard not to learn what is typed. The bound signal holds the real text." },
            Entry { name: "search", detail: "a field whose action key searches", doc: "A one-line field in every other respect." },
            Entry { name: "number", detail: "a number field", doc: "The bound signal always holds a number. While what is typed is not one yet (`-`, `1.`, nothing) the signal keeps its last number and the field shows the typing; leaving the field shows the number again. A phone raises a signed decimal keyboard." },
            Entry { name: "date", detail: "a day", doc: "The bound signal holds `YYYY-MM-DD`, as HTML's does, or is empty. A phone opens the platform's date picker; a desktop types the date, and only a real day inside `min` and `max` reaches the signal." },
            Entry { name: "select", detail: "a dropdown", doc: "Shows the bound value and opens a list on tap. Takes its choices from `:options`." },
            Entry { name: "checkbox", detail: "a tap-toggle", doc: "No caret and no focus of its own: a tap flips the bound value, and `:checked` matches it. Space or Enter activates it when tabbed to." },
            Entry { name: "switch", detail: "an on/off switch", doc: "A checkbox that draws as a track and a thumb and is announced as a switch. `:checked` matches it on; `accent-color` colours the track when on." },
            Entry { name: "slider", detail: "a number along a track", doc: "Bound to a number between `min` and `max`, in steps of `step`. A tap puts the thumb where it lands and a sideways drag moves it; a vertical swipe still scrolls the page. `@input` fires as it moves, `@change` when the finger lifts. `accent-color` colours it." },
            Entry { name: "radio", detail: "one of a group", doc: "Radios sharing a `name` are one group, so choosing one clears the others. Carries the chosen value in `value`." },
            Entry { name: "file", detail: "a file the person chooses", doc: "A \"Choose file\" button and the chosen name; a tap opens the platform's picker. The bound signal holds a `File?` (`File[]` with `multiple`): `name`, `size`, `type`, `url` and `await file.text()`. `::file-selector-button` styles the button. `@change` after a pick, `@cancel` after none." },
        ],
    ), (
        "inputmode",
        &[
            Entry { name: "text", detail: "the ordinary keyboard", doc: "The default." },
            Entry { name: "numeric", detail: "digits", doc: "PINs, codes, card and phone numbers: digit strings, not quantities." },
            Entry { name: "decimal", detail: "digits and a decimal point", doc: "For amounts typed as text." },
            Entry { name: "tel", detail: "a phone keypad", doc: "Digits with `+`, `*` and `#`." },
            Entry { name: "email", detail: "a keyboard with @", doc: "Also turns off autocapitalisation on most keyboards." },
            Entry { name: "url", detail: "a keyboard with / and .", doc: "For addresses." },
            Entry { name: "search", detail: "a Search action key", doc: "The same field, with Search where Enter was." },
            Entry { name: "none", detail: "no on-screen keyboard", doc: "For an app that draws its own keys. The field still takes a hardware keyboard." },
        ],
    ), (
        "enterkeyhint",
        &[
            Entry { name: "enter", detail: "a plain return key", doc: "No action label." },
            Entry { name: "done", detail: "Done: closes the field", doc: "Commits, then drops focus and the keyboard." },
            Entry { name: "go", detail: "Go", doc: "Commits; what going means is the app's `@change`." },
            Entry { name: "next", detail: "Next: the following field", doc: "Commits, then moves focus as Tab does." },
            Entry { name: "previous", detail: "Previous: the field before", doc: "Commits, then moves focus as Shift+Tab does." },
            Entry { name: "search", detail: "Search", doc: "Commits; the search is the app's `@change`." },
            Entry { name: "send", detail: "Send", doc: "Commits; sending is the app's `@change`." },
        ],
    )],
)];

/// The names script can call that are not ordinary rhai. Kept in step with
/// `docs/07-script.md`, which is the reference for this tier.
const SCRIPT_GLOBALS: &[Entry] = &[
    Entry { name: "signal", detail: "signal(value)", doc: "Declare a piece of reactive state." },
    Entry { name: "computed", detail: "computed name = expr;", doc: "Derived state, readable anywhere a signal is. A **declaration, not a call**: there is no `computed(|| …)`. Refreshing is one pass in declaration order, so a computed may read one declared above it and not below. Inside a component it runs per instance." },
    Entry { name: "prop", detail: "prop name;", doc: "In a component: something its caller passes, as `:name=\"expr\"` or `name=\"text\"` on the tag. `prop size = 16;` gives a default, so the caller may leave it off; without one the caller must pass it. A **declaration, not state**: the component reads it and cannot write it. A tag writes a two-word prop kebab or snake alike, so `:id-of` reaches `prop id_of;`. Any attribute on a component tag that is not a declared prop, a listener or a directive is an error." },
    Entry { name: "effect", detail: "effect { … }", doc: "Runs when what it read changes, and once on load. A **block, not a call**: there is no `effect(|| …)`. It subscribes to what it actually read on its last run, and is never woken by its own writes." },
    Entry { name: "mounted", detail: "mounted { … }", doc: "Runs once the document is on screen. Document level today; a component declaring one is warned." },
    Entry { name: "unmounted", detail: "unmounted { … }", doc: "Runs when the document stops being the one on screen." },
    Entry { name: "query", detail: "query(\"selector\")", doc: "Find elements by CSS selector, in a handler, against the frame already laid out. Returns an **array**, so check `.length` or index it: a selector that matches nothing is an empty array, not an error. Only inside a handler, because a binding that read the tree would rebuild the tree it read." },
    Entry { name: "navigate", detail: "navigate(path)", doc: "Go to a path, leaving the current one in the history." },
    Entry { name: "replace", detail: "replace(path)", doc: "Go to a path *without* a history entry. The only correct way to redirect." },
    Entry { name: "back", detail: "back()", doc: "Walk back through the history." },
    Entry { name: "forward", detail: "forward()", doc: "Walk forward through the history." },
    Entry { name: "pickFile", detail: "await pickFile({ title, filters, folder })", doc: "Ask for one file with the platform's own dialog: a `File` (`name`, `size`, `type`, `url`, `path`, `await file.text()`), or `none` if cancelled. Awaited from an `async fn`. `filters` is `[{ name, extensions }]`. On the web, call it in a tap handler before any other `await`, or it throws `gesture`; there and on Android a `File` has no `path`." },
    Entry { name: "pickFiles", detail: "await pickFiles({ title, filters, folder })", doc: "Ask for any number of files: a `File[]`, `[]` if cancelled. Awaited from an `async fn`." },
    Entry { name: "pickSaveFile", detail: "await pickSaveFile({ title, filters, folder, name })", doc: "Ask where to save, with the platform's own dialog: the path, or `none` if cancelled. `name` is the file name it suggests. Awaited from an `async fn`." },
    Entry { name: "loadSound", detail: "await loadSound(src)", doc: "Load a sound: a path beside the document, or a picked `File`'s `url`. A `Sound`: `play()`, `pause()`, `stop()`, `seek(seconds)`, and `volume`, `loop`, `currentTime`, `duration`, `playing`, which follow it as it plays. WAV, MP3, AAC, FLAC, Ogg Vorbis; not Opus. In a browser, the browser's own audio, after a tap." },
    Entry { name: "settings", detail: "settings.get(key) / .set(key, value) / .remove(key)", doc: "An app's own small values, kept between runs: `get` gives `any?` (`none` if never set), `set` writes at once, `remove` forgets. Kept per `[app] id`, in the platform's app-data folder (localStorage on the web)." },
    Entry { name: "close", detail: "close()", doc: "Close the app for good, without asking `@close` again: the app has decided. Every `unmounted` runs first. Does nothing on the web, where a page cannot close its own tab." },
    Entry { name: "path_for", detail: "path_for(\"route\", { … })", doc: "Build a path from a named route and its parameters." },
    Entry { name: "emit", detail: "emit(\"name\", payload)", doc: "A component telling its caller something happened." },
    Entry { name: "blur", detail: "blur()", doc: "Drop focus. Free-standing rather than a method on an element, because there is only one focused element: blurring \"this one\" would either do nothing or take focus from something else." },
    Entry { name: "setInterval", detail: "setInterval(ms) { … }", doc: "Run a block on a period, handing back a handle. A **block, not a callback**: `setInterval(1000) { seconds++ }`. Stop it with `clearInterval(handle)`." },
    Entry { name: "clearInterval", detail: "clearInterval(handle)", doc: "Stop a timer started by `setInterval`. The handle is what `setInterval` returned." },
    Entry { name: "print", detail: "print(x)", doc: "Printf-debugging. Reaches the dev overlay, not just stderr." },
    Entry { name: "debug", detail: "debug(x)", doc: "Like `print`, with the value's structure shown." },
    Entry { name: "Number", detail: "Number(text)", doc: "The whole text as a number, as JavaScript reads it: `Number(\"2\")` is 2, `Number(\"\")` is 0, `Number(\"12px\")` is `NaN`. How a route parameter, which is always text, meets a numeric id." },
    Entry { name: "parseInt", detail: "parseInt(text, radix): int?", doc: "The whole number that leads the text: `parseInt(\"42px\")` is 42, and `parseInt(\"px\")` is `none`, so read it with `??`: `parseInt(s) ?? 0`. The radix is optional." },
    Entry { name: "parseFloat", detail: "parseFloat(text): float?", doc: "The number that leads the text: `parseFloat(\"12.5px\")` is 12.5, and `parseFloat(\"px\")` is `none`, where JavaScript says `NaN`." },
    Entry { name: "Ok", detail: "Ok(value): Result<T, E>", doc: "A `Result` that holds a value: `{ ok: true, value }`. Read it with `if r.ok { r.value }`, or `r.unwrap()`." },
    Entry { name: "Err", detail: "Err(error): Result<T, E>", doc: "A `Result` that holds an error kept as an answer: `{ ok: false, error }`. `r.unwrap()` on it throws the error." },
    Entry { name: "intDiv", detail: "intDiv(a, b): int", doc: "Division that keeps an `int`, cutting toward zero: `intDiv(7, 2)` is 3, where `7 / 2` is 3.5. Dividing by zero is an error." },
    Entry { name: "String", detail: "String(x)", doc: "Any value as the text a `{{ }}` binding would show." },
    Entry { name: "isNaN", detail: "isNaN(n)", doc: "Whether a `float` is `NaN`, the answer `Number` gives for text that is not a number, and `0.0 / 0` gives." },
];

/// The pseudo-classes a selector may name, with the one line each needs.
///
/// The *names* are owned by [`rux_style::honored_pseudo_classes`] and pinned
/// against it by `pseudo_classes_match_the_runtime` below; only the prose is
/// declared here, for the same reason the element table is.
const PSEUDO_CLASSES: &[Entry] = &[
    Entry { name: "hover", detail: "the pointer is over this box", doc: "Needs a layout region, so the shell can tell when the pointer enters and leaves it. There is no hover on a touch screen, so it can decorate and must not be the only way to reach something." },
    Entry { name: "focus", detail: "this element has the keyboard", doc: "A field, a select or a `@tap` box, reached by tapping it or by walking the focus order with Tab. The element itself only, not its ancestors. For the ring, prefer `:focus-visible`, which leaves a tapped button alone." },
    Entry { name: "focus-visible", detail: "focused, and it should show", doc: "A field whenever it has focus; anything else only when the keyboard put focus there, so a button tapped with a finger shows nothing and the same button reached with Tab does. The default ring is this rule's `outline: auto`, so `outline: none` here removes it and any other `outline` replaces it." },
    Entry { name: "active", detail: "a press is down on this box", doc: "Held between press and release, so it is the state a button uses to look pushed." },
    Entry { name: "checked", detail: "a checkbox or radio is on", doc: "Resolved during the build from the input's live state, rather than supplied by the shell as `:hover` and `:active` are." },
    Entry { name: "current", detail: "this link names the path you are on", doc: "Matches an element whose `to` is the route you are currently on. It is the nav highlight, without a signal to track it." },
    Entry { name: "enter-from", detail: "the frame an element enters from", doc: "Under `r-transition`: the style the element holds for exactly one frame as it arrives, and animates away from. The *hidden* end of the animation goes here." },
    Entry { name: "leave-to", detail: "the state an element leaves to", doc: "Under `r-transition`: the style the element animates towards while it is held on screen on its way out. `position: absolute` here takes it out of flow, so the rest of a list closes up under it rather than waiting." },
    Entry { name: "disabled", detail: "an input or button that is switched off", doc: "Matches a `<input>` or `<button>` with `disabled` (or a truthy `:disabled`). Rux draws no default look for it, so this is where the dimming goes." },
    Entry { name: "enabled", detail: "an input or button that is not disabled", doc: "Form controls only: a plain box is neither enabled nor disabled, as in CSS." },
    Entry { name: "valid", detail: "an input whose value passes its checks", doc: "Matched from the start, as in CSS. A disabled or readonly input is neither valid nor invalid." },
    Entry { name: "invalid", detail: "an input whose value fails a check", doc: "`required`, `minlength`, `pattern`, a number's `min`/`max`, or the shape `inputmode=\"email\"` or `\"url\"` implies. Matched from the start, so an empty required field is invalid before anyone has touched it: `:user-invalid` waits." },
    Entry { name: "user-valid", detail: "valid, once the person has been through it", doc: "Like `:valid`, but only after the field was changed and left, or its form's submission was tried." },
    Entry { name: "user-invalid", detail: "invalid, once the person has been through it", doc: "Like `:invalid`, but only after the field was changed and left, or its form's submission was tried. The one to put error styling on." },
    Entry { name: "required", detail: "an input with `required`", doc: "Inputs only." },
    Entry { name: "optional", detail: "an input without `required`", doc: "Inputs only." },
    Entry { name: "first-child", detail: "the first of its siblings", doc: "Counted among the elements drawn there, so a row an `r-if` leaves out is not counted." },
    Entry { name: "last-child", detail: "the last of its siblings", doc: "The siblings are counted before they are built: each `r-for` row, and the branch each `r-if` chain takes. A row animating out is not counted." },
    Entry { name: "only-child", detail: "the one element in its parent", doc: "Counted as `:last-child` is." },
    Entry { name: "nth-child", detail: "`:nth-child(an+b)`: by its place among its siblings", doc: "Counted from 1: `odd`, `even`, `3`, `2n+1`, `-n+3`. The `of S` form is not read yet." },
    Entry { name: "nth-last-child", detail: "`:nth-last-child(an+b)`: counted from the end", doc: "As `:nth-child()`, from the last sibling back: `:nth-last-child(-n+2)` is the last two." },
    Entry { name: "not", detail: "`:not(a, b)`: matches none of these", doc: "Takes compound selectors (`.done`, `button:disabled`), separated by commas, and counts as specific as the most specific of them. A combinator inside (`:not(.a .b)`) is not read yet, and the rule never matches." },
    Entry { name: "is", detail: "`:is(a, b)`: matches any of these", doc: "Takes full selectors, combinators included (`.x:is(.group:hover *)`), and counts as specific as the most specific of them. A forgiving list, as in CSS: an argument that does not parse is dropped, not the whole rule." },
    Entry { name: "where", detail: "`:where(a, b)`: as `:is()`, with no weight", doc: "Matches what `:is()` matches and adds nothing to specificity, so any later rule overrides it. The usual way a reset or a library keeps out of the author's way." },
    Entry { name: "dir", detail: "the element's text direction", doc: "`:dir(rtl)` or `:dir(ltr)`: what the `dir` attribute says, on the element or its nearest ancestor that has one, and `ltr` where none does. Tailwind's `rtl:` and `ltr:` are written with it. `dir=\"auto\"` counts as saying nothing." },
    Entry { name: "root", detail: "the top element of the app", doc: "The screen's own element, so `:root { --accent: #89b4fa; }` sets a variable for everything under it, as on a web page." },
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
    ("display", &["block", "flex", "grid", "inline", "none", "inline-flex", "inline-grid", "inline-block", "flow-root"]),
    // All five, and every one of them honored since v0.7. Before that, four of
    // these were silently `relative`.
    ("position", &["static", "relative", "sticky", "absolute", "fixed"]),
    ("isolation", &["auto", "isolate"]),
    ("overflow", &["visible", "hidden", "clip", "auto", "scroll"]),
    ("overflow-x", &["visible", "hidden", "clip", "auto", "scroll"]),
    ("overflow-y", &["visible", "hidden", "clip", "auto", "scroll"]),
    ("overscroll-behavior", &["auto", "contain", "none"]),
    ("overscroll-behavior-x", &["auto", "contain", "none"]),
    ("overscroll-behavior-y", &["auto", "contain", "none"]),
    ("scrollbar-width", &["auto", "thin", "none"]),
    ("scrollbar-color", &["auto"]),
    ("flex-direction", &["row", "column"]),
    ("flex-wrap", &["nowrap", "wrap", "wrap-reverse"]),
    ("flex", &["none", "auto", "initial"]),
    ("flex-basis", &["auto", "content"]),
    ("justify-content", &["flex-start", "start", "center", "flex-end", "end", "space-between", "space-around"]),
    ("align-content", &["flex-start", "start", "center", "flex-end", "end", "space-between", "space-around"]),
    ("align-items", &["flex-start", "start", "center", "flex-end", "end", "stretch"]),
    ("align-self", &["flex-start", "start", "center", "flex-end", "end", "stretch"]),
    ("justify-self", &["flex-start", "start", "center", "flex-end", "end", "stretch"]),
    ("box-sizing", &["content-box", "border-box"]),
    ("appearance", &["none", "auto"]),
    ("column-fill", &["balance", "auto"]),
    ("user-select", &["auto", "none", "text", "all"]),
    ("text-wrap", &["wrap", "nowrap", "balance", "pretty", "stable"]),
    ("text-wrap-mode", &["wrap", "nowrap"]),
    ("text-wrap-style", &["auto", "balance", "pretty", "stable"]),
    ("transform-origin", &["center", "top", "right", "bottom", "left"]),
    ("animation-direction", &["normal", "reverse", "alternate", "alternate-reverse"]),
    ("animation-fill-mode", &["none", "forwards", "backwards", "both"]),
    ("animation-play-state", &["running", "paused"]),
    ("animation-iteration-count", &["infinite"]),
    ("justify-items", &["flex-start", "start", "center", "flex-end", "end", "stretch"]),
    ("grid-auto-flow", &["row", "column", "row dense", "column dense"]),
    ("text-align", &["left", "start", "center", "right", "end", "justify"]),
    ("font-weight", &["normal", "bold", "lighter", "bolder", "100", "200", "300", "400", "500", "600", "700", "800", "900"]),
    ("font-style", &["normal", "italic", "oblique"]),
    ("resize", &["none", "both", "horizontal", "vertical"]),
    ("white-space", &["normal", "nowrap", "pre", "pre-wrap", "pre-line"]),
    ("vertical-align", &["baseline", "middle", "text-top", "text-bottom"]),
    ("direction", &["ltr", "rtl"]),
    ("color-scheme", &["normal", "light", "dark", "light dark"]),
    ("visibility", &["visible", "hidden", "collapse"]),
    ("mix-blend-mode", &["normal", "multiply", "screen", "overlay", "darken", "lighten", "color-dodge", "color-burn", "hard-light", "soft-light", "difference", "exclusion", "hue", "saturation", "color", "luminosity"]),
    ("mask-mode", &["alpha", "luminance", "match-source"]),
    ("background-size", &["auto", "cover", "contain"]),
    ("background-position", &["center", "top", "right", "bottom", "left"]),
    ("background-repeat", &["repeat", "no-repeat", "repeat-x", "repeat-y", "space", "round"]),
    ("background-clip", &["border-box", "padding-box", "content-box"]),
    ("background-origin", &["padding-box", "border-box", "content-box"]),
    ("background-attachment", &["scroll", "fixed", "local"]),
    ("border-style", &["solid", "dashed", "dotted", "double", "groove", "ridge", "inset", "outset", "hidden", "none"]),
    ("text-decoration", &["none", "underline", "overline", "line-through", "solid", "double", "dotted", "dashed", "wavy"]),
    ("text-decoration-line", &["none", "underline", "overline", "line-through"]),
    ("text-decoration-style", &["solid", "double", "dotted", "dashed", "wavy"]),
    ("text-decoration-thickness", &["auto", "from-font"]),
    ("text-underline-offset", &["auto"]),
    ("text-transform", &["none", "uppercase", "lowercase", "capitalize"]),
    ("text-indent", &["hanging", "each-line"]),
    ("text-overflow", &["clip", "ellipsis"]),
    ("font-variant", &["normal", "none", "small-caps", "all-small-caps", "tabular-nums", "oldstyle-nums", "slashed-zero"]),
    ("font-variant-caps", &["normal", "small-caps", "all-small-caps", "petite-caps", "all-petite-caps", "unicase", "titling-caps"]),
    ("font-variant-numeric", &["normal", "lining-nums", "oldstyle-nums", "proportional-nums", "tabular-nums", "diagonal-fractions", "stacked-fractions", "ordinal", "slashed-zero"]),
    ("font-variant-ligatures", &["normal", "none", "common-ligatures", "no-common-ligatures", "discretionary-ligatures", "no-discretionary-ligatures", "historical-ligatures", "no-historical-ligatures", "contextual", "no-contextual"]),
    ("font-variant-position", &["normal", "sub", "super"]),
    ("font-variant-east-asian", &["normal", "jis78", "jis83", "jis90", "jis04", "simplified", "traditional", "full-width", "proportional-width", "ruby"]),
    ("font-stretch", &["normal", "ultra-condensed", "extra-condensed", "condensed", "semi-condensed", "semi-expanded", "expanded", "extra-expanded", "ultra-expanded"]),
    ("font-width", &["normal", "condensed", "expanded"]),
    ("caret-color", &["auto"]),
    ("object-fit", &["fill", "contain", "cover", "none", "scale-down"]),
    ("object-position", &["center", "top", "right", "bottom", "left"]),
    ("pointer-events", &["auto", "none"]),
    ("scroll-behavior", &["auto", "smooth"]),
    ("scroll-snap-type", &["none", "x mandatory", "y mandatory", "both mandatory", "x proximity", "y proximity"]),
    ("scroll-snap-align", &["none", "start", "center", "end"]),
    ("scroll-snap-stop", &["normal", "always"]),
    ("overflow-wrap", &["normal", "break-word", "anywhere"]),
    ("word-wrap", &["normal", "break-word", "anywhere"]),
    ("word-break", &["normal", "break-all"]),
    ("cursor", &[
        "auto", "default", "none", "context-menu", "help", "pointer", "progress", "wait", "cell", "crosshair",
        "text", "vertical-text", "alias", "copy", "move", "no-drop", "not-allowed", "grab", "grabbing",
        "all-scroll", "col-resize", "row-resize", "n-resize", "e-resize", "s-resize", "w-resize", "ne-resize",
        "nw-resize", "se-resize", "sw-resize", "ew-resize", "ns-resize", "nesw-resize", "nwse-resize",
        "zoom-in", "zoom-out",
    ]),
    ("outline-style", &["none", "auto", "solid"]),
    ("touch-action", &["auto", "none", "pan-x", "pan-y", "manipulation"]),
    ("fill-rule", &["nonzero", "evenodd"]),
    ("stroke-linecap", &["butt", "round", "square"]),
    ("stroke-linejoin", &["miter", "round", "bevel"]),
];

/// The easing keywords a `transition` may name. `cubic-bezier(a, b, c, d)` and
/// `steps(n, <position>)` are accepted as well and are not keywords, so they
/// are not offered as ones.
const EASINGS: &[&str] = &["linear", "ease", "ease-in", "ease-out", "ease-in-out", "step-start", "step-end"];

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
    Entry { name: "findIndex", detail: "findIndex(x => …)", doc: "Where the first item that passed is, or -1." },
    Entry { name: "some", detail: "some(x => …)", doc: "Whether any item passes." },
    Entry { name: "every", detail: "every(x => …)", doc: "Whether every item passes." },
    Entry { name: "sort", detail: "sort((a, b) => a - b)", doc: "Sorts by the comparison and **returns** the sorted array, as JavaScript's does. The receiver is sorted too." },
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
/// default, `overflow` being what makes
/// a scroller, a `transform` becoming a containing block. Those are the places
/// a general CSS reference is actively wrong here.
const CSS_PROPERTY_DOCS: &[Entry] = &[
    Entry { name: "display", detail: "how this box lays its children out", doc: "`block` (the default), `flex`, `grid`, `inline` or `none`. **Rux defaults to `block`**, so a box does not lay its children out in a row until you say `display: flex`. Rux has no inline level for a box, so `inline-flex` is `flex`, `inline-grid` is `grid` and `inline-block` is `block`; a value with no layout here (`contents`, `table`) warns and is `block`." },
    Entry { name: "width", detail: "how wide the box is", doc: "A length or a percentage of the parent's width. A child of a flex column already fills its width, since the cross-axis default is `stretch`; `width` is for when it should not." },
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
    Entry { name: "border-color", detail: "what colour the outline is", doc: "Animatable, so it is the usual way to show focus without the box moving. One to four colours, top, right, bottom, left, as `margin` takes lengths." },
    Entry { name: "border-top-color", detail: "the top edge's colour", doc: "Overrides `border-color` for this side. A border whose sides differ in colour is drawn side by side, each with its own; only the box's `border-color` animates." },
    Entry { name: "border-right-color", detail: "the right edge's colour", doc: "Overrides `border-color` for this side." },
    Entry { name: "border-bottom-color", detail: "the bottom edge's colour", doc: "Overrides `border-color` for this side." },
    Entry { name: "border-left-color", detail: "the left edge's colour", doc: "Overrides `border-color` for this side." },
    Entry { name: "border-radius", detail: "how round the corners are", doc: "One to four lengths, grouped by diagonal. Takes a percentage, resolved against the box's shorter side, so `50%` rounds a square into a circle and an oblong into a pill. Clips the background and the border; it does **not** clip children unless the box also has `overflow: hidden`." },
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
    Entry { name: "overscroll-behavior", detail: "whether scrolling past the end passes outward", doc: "`auto` (the default) lets what this scroller cannot take go on to the scroller around it, so a list at its end lets the page scroll. `contain` or `none` keeps it here. One value for both axes, or `x y`." },
    Entry { name: "overscroll-behavior-x", detail: "the same, sideways only", doc: "`auto`, `contain` or `none`, for the horizontal axis." },
    Entry { name: "overscroll-behavior-y", detail: "the same, up and down only", doc: "`auto`, `contain` or `none`, for the vertical axis: a panel whose list should never scroll the page behind it." },
    Entry { name: "scrollbar-width", detail: "how thick the scrollbars are drawn", doc: "`auto`, `thin`, or `none`, which draws no bar while the box still scrolls. Bars show while the box scrolls or the pointer is over it, then fade." },
    Entry { name: "scrollbar-color", detail: "the scrollbars' thumb and track colours", doc: "Two colours, thumb then track, or `auto` for the default grey." },
    Entry { name: "clip-path", detail: "cut the box to a shape", doc: "`circle()`, `ellipse()`, `inset(… round …)`, `polygon()`, `path()`, `xywh()` or `rect()`, over `border-box` (the default), `padding-box`, `content-box` or `margin-box`. The box and everything in it are cut; taps are not yet." },
    Entry { name: "mask-image", detail: "show the box only where an image or gradient is", doc: "A gradient or `url()`, laid over the border box: the box and everything in it show where the mask is opaque, and fade where it fades. `mask-mode: luminance` uses how light it is instead. An image covers the box, as `background-image` does." },
    Entry { name: "mask", detail: "mask-image and mask-mode at once", doc: "`mask: linear-gradient(black, transparent)`, and `luminance` to mask by lightness." },
    Entry { name: "mask-mode", detail: "mask by opacity or by lightness", doc: "`alpha` (and `match-source`, the default) or `luminance`." },
    Entry { name: "border-style", detail: "how the border's line is drawn", doc: "`solid`, `dashed`, `dotted`, `double`, `groove`, `ridge`, `inset`, `outset`, `hidden` or `none`, one to four values like `margin`. **A border with no style draws nothing**, as in CSS: write `border: 2px solid red`, not `border: 2px red`." },
    Entry { name: "border-top-style", detail: "how the top edge is drawn", doc: "One of the `border-style` values, for that side alone." },
    Entry { name: "border-right-style", detail: "how the right edge is drawn", doc: "One of the `border-style` values, for that side alone." },
    Entry { name: "border-bottom-style", detail: "how the bottom edge is drawn", doc: "One of the `border-style` values, for that side alone." },
    Entry { name: "border-left-style", detail: "how the left edge is drawn", doc: "One of the `border-style` values, for that side alone." },
    Entry { name: "text-shadow", detail: "a shadow or glow under the text", doc: "`x y blur colour`, and a comma list of them, the first on top. Inherited. With no colour it is the text's own: `text-shadow: 0 0 8px` glows in the text colour." },
    Entry { name: "filter", detail: "blur or recolour the box and everything in it", doc: "`blur()`, `brightness()`, `contrast()`, `drop-shadow()`, `grayscale()`, `hue-rotate()`, `invert()`, `opacity()`, `saturate()` and `sepia()`, applied in the order written." },
    Entry { name: "backdrop-filter", detail: "blur or recolour what is behind the box", doc: "The same functions as `filter`, applied to what is drawn behind the box and shown inside it: `backdrop-filter: blur(12px)` is frosted glass. Give the box a translucent background to see it." },
    Entry { name: "mix-blend-mode", detail: "how the box blends with what is under it", doc: "`multiply`, `screen`, `overlay`, `darken`, `lighten`, `color-dodge`, `color-burn`, `hard-light`, `soft-light`, `difference`, `exclusion`, `hue`, `saturation`, `color` or `luminosity`. The box and everything in it blend as one layer." },
    Entry { name: "opacity", detail: "how see-through the whole box is", doc: "`0` to `1`, and it applies to the **subtree**, not just this box. Animatable, and the cheapest thing to animate." },
    Entry { name: "cursor", detail: "the pointer shape over this box", doc: "Every CSS keyword, on any box, inherited. `url()` images are skipped for the keyword after them. Applied by the shell on hover." },
    Entry { name: "touch-action", detail: "which axes a finger may still scroll here", doc: "Decides who wins when a `@drag` and a scroller want the same finger. The default `auto` lets the scroller keep any axis it can actually scroll, so a drag inside a scrolling list does not trap the page; `none` gives the element the finger outright; `pan-x` and `pan-y` keep one axis for the scroller. Settled once, when the finger passes the drag threshold. Touch only: a mouse scrolls by wheel, not by dragging." },
    Entry { name: "box-shadow", detail: "a shadow cast by the box", doc: "`x y blur spread colour`, and a comma list of them, the first on top. Painted outside the border, or with `inset` inside it over the background, and takes no space in the layout. With no colour it is the text `color`." },
    Entry { name: "outline", detail: "a line around the box, outside its border", doc: "`width style colour`, in any order. Takes no space, is drawn over the element's content, and follows `border-radius`. The focus ring is the default `:focus-visible { outline: auto }`, so this is also how it is restyled or removed. Solid lines only: `dashed` and `dotted` are drawn solid, with a warning." },
    Entry { name: "outline-color", detail: "the outline's colour", doc: "The element's own `color` when not written, as CSS's `currentColor`. On the focus ring alone, it recolours the ring and keeps its shape." },
    Entry { name: "outline-offset", detail: "the gap between the border and the outline", doc: "`0` by default. Negative draws the outline inside the box." },
    Entry { name: "outline-style", detail: "whether and how the outline is drawn", doc: "`none` (the initial value), `solid`, or `auto`, the platform's focus ring." },
    Entry { name: "outline-width", detail: "the outline's thickness", doc: "A length, or `thin`, `medium` (the initial, 3px) or `thick`. `0` removes the outline." },
    Entry { name: "transform", detail: "move, rotate or scale the painted box", doc: "`translate`, `rotate`, `scale`, composed left to right. Paint-only, so it never moves anything else. **A transformed box becomes the containing block** for `absolute` *and* `fixed` descendants, which is why a `fixed` child stops being fixed inside one." },
    Entry { name: "transition", detail: "animate a property when it changes", doc: "`<property> <duration> <easing>`, comma-separated for more than one. Only some properties can animate; naming one that cannot is a warning rather than silence." },
    Entry { name: "transition-property", detail: "which properties a transition animates", doc: "A comma list. There is one transition per name, and the duration, easing and delay lists are repeated to match, as in CSS. A property that changes at once anyway (`display`, a custom property) is taken without a warning." },
    Entry { name: "transition-duration", detail: "how long each transition takes", doc: "A comma list of times, repeated to the length of `transition-property`." },
    Entry { name: "transition-timing-function", detail: "the easing of each transition", doc: "A comma list of easings, the same ones `transition` takes, repeated to the length of `transition-property`." },
    Entry { name: "transition-delay", detail: "how long each transition waits", doc: "A comma list of times, repeated to the length of `transition-property`." },
    Entry { name: "transition-behavior", detail: "whether discrete properties transition", doc: "Read and set aside: a discrete property in Rux always changes at once." },
    Entry { name: "translate", detail: "move the painted box", doc: "`10px`, `10px 20px`. Applied before `rotate`, `scale` and `transform`, as in CSS. Paint-only, as `transform` is." },
    Entry { name: "rotate", detail: "turn the painted box", doc: "`45deg`, or `z 45deg`. A turn about another axis is 3D and draws as none." },
    Entry { name: "scale", detail: "grow or shrink the painted box", doc: "`1.1`, or `1.1 0.9` for each axis. Applied after `translate` and `rotate`, before `transform`." },
    Entry { name: "flex", detail: "grow, shrink and basis in one", doc: "`flex: 1` means `1 1 0%`, not `1 1 auto`. The usual way to say \\\"take the leftover room\\\"." },
    Entry { name: "flex-grow", detail: "share of the spare room this item takes", doc: "`0` (the default) means it never grows." },
    Entry { name: "flex-shrink", detail: "how readily this item gives room up", doc: "`0` stops it shrinking below its content." },
    Entry { name: "flex-basis", detail: "the size to start from before growing", doc: "`auto` uses the item's own size; a length overrides it." },
    Entry { name: "flex-wrap", detail: "whether items may move onto another line", doc: "`wrap` or `wrap-reverse` allow it; anything else keeps one line." },
    Entry { name: "flex-direction", detail: "which way the children are laid out", doc: "`row` (the default) or `column`. This is the axis every alignment property below is described against." },
    Entry { name: "justify-content", detail: "alignment along the main axis", doc: "The axis `flex-direction` set. On a `row`, this is horizontal." },
    Entry { name: "align-items", detail: "alignment across the main axis", doc: "On a `row`, this is vertical. Defaults to `stretch`, as in CSS, so a child fills the cross axis. `align-self: flex-start` makes one child hug." },
    Entry { name: "align-self", detail: "this one item's cross-axis alignment", doc: "Overrides the parent's `align-items` for this item only. `align-self: flex-end` is how one row in a list moves to the far side." },
    Entry { name: "justify-self", detail: "this one item's main-axis alignment", doc: "Grid mostly; a single item's override." },
    Entry { name: "justify-items", detail: "the default main-axis alignment for children", doc: "Grid mostly; set on the parent." },
    Entry { name: "place-items", detail: "`align-items` and `justify-items` at once", doc: "One value for both, or the align value then the justify one. `place-items: center` centres every grid item in its cell." },
    Entry { name: "place-content", detail: "`align-content` and `justify-content` at once", doc: "One value for both, or the align value then the justify one." },
    Entry { name: "place-self", detail: "`align-self` and `justify-self` at once", doc: "One value for both, or the align value then the justify one." },
    Entry { name: "align-content", detail: "how whole lines are spaced", doc: "Only does anything with more than one line, so it needs `flex-wrap: wrap`." },
    Entry { name: "row-gap", detail: "space between rows", doc: "Overrides `gap` on the vertical axis." },
    Entry { name: "column-gap", detail: "space between columns", doc: "Overrides `gap` on the horizontal axis." },
    Entry { name: "grid-template-columns", detail: "the column tracks", doc: "`repeat(3, 1fr)`, `200px 1fr`, and so on. Setting this is most of what makes a grid." },
    Entry { name: "grid-template-rows", detail: "the row tracks", doc: "Explicit row sizes; rows beyond these come from `grid-auto-rows`." },
    Entry { name: "grid-template-areas", detail: "name the areas of a grid", doc: "One string per row, one name per cell, `.` for an empty one: `\"head head\" \"side main\"`. Each name must make a rectangle. Place an item with `grid-area: main`." },
    Entry { name: "grid-template", detail: "rows, columns and areas at once", doc: "`<rows> / <columns>`, or with areas, each row's string then its size: `\"head head\" 60px \"side main\" 1fr / 200px 1fr`." },
    Entry { name: "grid-area", detail: "which area this item fills", doc: "An area name from `grid-template-areas`, or `row-start / column-start / row-end / column-end`." },
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
    Entry { name: "inset", detail: "`top`, `right`, `bottom` and `left` at once", doc: "One to four values, as `margin` takes them. `inset: 0` on an `absolute` box fills its containing block." },
    Entry { name: "aspect-ratio", detail: "width-to-height ratio to keep", doc: "`16 / 9`. Gives a box a height derived from its width, so a media box keeps its shape as it resizes." },
    Entry { name: "box-sizing", detail: "what `width` and `height` measure", doc: "`content-box` (the default, as in CSS): the padding and border are added to the width. `border-box`: they are inside it, which is what `<input>` and `<button>` use unless told otherwise." },
    Entry { name: "z-index", detail: "which box is drawn over which", doc: "`auto` or a whole number. Counts on a positioned box and on a flex or grid item; higher is drawn over lower, inside the same stacking context. A `fixed` box is **not** on top by itself: give it a `z-index`. Animates in whole steps." },
    Entry { name: "isolation", detail: "start a stacking context", doc: "`isolate` makes this box a stacking context and does nothing else, so a `z-index` inside it, negative ones included, is sorted inside it and never escapes." },
    Entry { name: "anchor-name", detail: "name this box as an anchor", doc: "`--menu-button`. Another box can then be placed against it with `position-anchor` and `anchor()`. Names are document-wide, so a component can anchor to a box anywhere; when two boxes share a name the later one is the anchor." },
    Entry { name: "position-anchor", detail: "the anchor this box is placed against", doc: "`--menu-button`, on an `absolute` or `fixed` box. Then `top: anchor(bottom); left: anchor(left)` puts it under that box, and it follows the box when a panel scrolls. `fixed` also escapes every `overflow` clip, which is how a dropdown gets out of a scrolling panel. When the anchor scrolls out of view the box is not drawn." },
    Entry { name: "position-try-fallbacks", detail: "other places to try when an anchored box does not fit", doc: "A comma-separated list: `flip-block` (above instead of below), `flip-inline` (left instead of right), `flip-start`, or a `@position-try --name { … }` rule's name. The first that fits wins; with none, the box's own place. A fallback that fitted is kept while it still fits." },
    Entry { name: "position-try-order", detail: "which fallback with the most room wins", doc: "`normal` tries them in order. `most-width` / `most-height` (or `most-inline-size` / `most-block-size`) try the places with the most room on that axis first." },
    Entry { name: "position-try", detail: "fallback places, shorthand", doc: "An optional `position-try-order`, then `position-try-fallbacks`: `position-try: most-height flip-block`." },
    Entry { name: "content-visibility", detail: "skip a box's contents while it is off screen", doc: "`auto` lays out and draws the contents only while the box is near the screen, which is how a list of thousands of rows stays fast: put it on each row with `contain-intrinsic-size`. `hidden` always skips them. Either way the box clips its contents and is a stacking context." },
    Entry { name: "contain-intrinsic-size", detail: "the size a skipped box takes", doc: "With `content-visibility`, the size standing in for skipped contents. `auto 40px` is 40px until the box has been drawn, then the size it was last drawn at. One value for both axes, or width then height." },
    Entry { name: "contain-intrinsic-width", detail: "the width a skipped box takes", doc: "One axis of `contain-intrinsic-size`." },
    Entry { name: "contain-intrinsic-height", detail: "the height a skipped box takes", doc: "One axis of `contain-intrinsic-size`." },
    Entry { name: "contain-intrinsic-inline-size", detail: "the width a skipped box takes", doc: "`contain-intrinsic-width`, by its logical name." },
    Entry { name: "contain-intrinsic-block-size", detail: "the height a skipped box takes", doc: "`contain-intrinsic-height`, by its logical name." },
    Entry { name: "background", detail: "the colour and images behind the content", doc: "A colour, and layers of `linear-gradient(…)`, `radial-gradient(…)` or `url(…)`, each with its position, `/ size`, repeat, attachment and boxes, as in CSS. Sets every `background-*` longhand." },
    Entry { name: "background-color", detail: "a flat colour behind the content", doc: "Animatable, so it is what a hover or pressed state usually changes." },
    Entry { name: "background-image", detail: "images behind the content", doc: "A comma list of `linear-gradient`, `radial-gradient` and `url(…)`, the first on top. An image is its natural size, tiled, unless `background-size` says otherwise." },
    Entry { name: "background-size", detail: "how large each background image is", doc: "`cover`, `contain`, or a width and height as lengths, percentages of the box or `auto`." },
    Entry { name: "background-position", detail: "where each background image sits", doc: "One to four values: `center`, `right 10px bottom 20px`, `25% 75%`." },
    Entry { name: "background-position-x", detail: "where each background image sits across", doc: "`left`, `center`, `right`, an offset, or `right 10px`." },
    Entry { name: "background-position-y", detail: "where each background image sits down", doc: "`top`, `center`, `bottom`, an offset, or `bottom 10px`." },
    Entry { name: "background-repeat", detail: "whether a background image tiles", doc: "`repeat`, `no-repeat`, `repeat-x`, `repeat-y`, `space` or `round`." },
    Entry { name: "background-clip", detail: "which box a background is cut to", doc: "`border-box` (the default), `padding-box` or `content-box`." },
    Entry { name: "background-origin", detail: "which box a background is placed in", doc: "`padding-box` (the default), `border-box` or `content-box`." },
    Entry { name: "background-attachment", detail: "what a background moves with", doc: "`scroll` (the box), `fixed` (the window) or `local` (the box's scrolled content)." },
    Entry { name: "columns", detail: "a multi-column box", doc: "`column-count` and `column-width` in one: a number, a length, or both. The children are set one after another in columns that even out by height." },
    Entry { name: "column-count", detail: "how many columns", doc: "The box's children are set down this many columns, fewer if `column-width` says they would be too narrow." },
    Entry { name: "column-width", detail: "how wide a column may be", doc: "As many columns as fit at least this wide; with `column-count`, no more than it." },
    Entry { name: "column-fill", detail: "balance or fill the columns", doc: "`balance` (the default) evens the columns out; `auto` fills each to the box's own height before the next." },
    Entry { name: "column-rule", detail: "a line between columns", doc: "Width, style and colour, as `border` takes them, drawn down the middle of each gap in use. Drawn solid." },
    Entry { name: "column-rule-width", detail: "the line between columns' width", doc: "Overrides the `column-rule` shorthand." },
    Entry { name: "column-rule-style", detail: "the line between columns' style", doc: "Overrides the `column-rule` shorthand. Any style but `none` and `hidden` is drawn solid." },
    Entry { name: "column-rule-color", detail: "the line between columns' colour", doc: "Overrides the `column-rule` shorthand; none of its own is the text's colour." },
    Entry { name: "content", detail: "what `::before` and `::after` hold", doc: "Strings, joined, and `attr(name)`; `\"\"` makes an empty box, `none` makes nothing. Only in a rule ending in `::before` or `::after`." },
    Entry { name: "accent-color", detail: "the colour a control is drawn in", doc: "A switch's track when on, and a slider's fill and thumb. Not inherited." },
    Entry { name: "appearance", detail: "whether a control is drawn as one", doc: "`none`: a checkbox, radio or switch loses the mark, dot or thumb Rux draws in it, and its default shape and colours, leaving a box to style with `:checked`. Any other value draws the control. A slider and a select are drawn either way." },
    Entry { name: "color", detail: "the colour of the text", doc: "Inherited, so setting it on a container covers the text inside. Animatable." },
    Entry { name: "font-size", detail: "how large the text is", doc: "Also the reference for `em` on this box, so changing it moves anything sized in `em`." },
    Entry { name: "font-weight", detail: "how heavy the text is", doc: "`normal`, `bold`, `lighter`, `bolder`, or a number from 100 to 900." },
    Entry { name: "font-family", detail: "which typeface to use", doc: "Falls back through the list to whatever the system has." },
    Entry { name: "font", detail: "style, weight, size, line height and family at once", doc: "`italic 600 1rem/1.5 Inter, sans-serif`. The size and the family are needed; what is left out goes back to `normal`, as in CSS. A system font (`caption`, `menu`) is not read and warns." },
    Entry { name: "font-style", detail: "upright or italic", doc: "`italic` and `oblique` both slant; anything else is upright." },
    Entry { name: "text-align", detail: "how lines sit within the box", doc: "`start`, `center`, `end`/`right`, or `justify`. Aligns the **lines inside** the box, which is not the same as moving the box." },
    Entry { name: "letter-spacing", detail: "extra space between characters", doc: "A length, positive or negative." },
    Entry { name: "word-spacing", detail: "extra space between words", doc: "A length, positive or negative." },
    Entry { name: "line-height", detail: "the height of one line of text", doc: "A number (a multiple of the font size) or a length. The usual way to give body text room to breathe." },
    Entry { name: "direction", detail: "which way text and rows run", doc: "`ltr` (the default) or `rtl`, inherited; the `dir` attribute sets it too, and a rule wins over the attribute. Under `rtl` a flex row starts from the right, grid columns count from the right, `text-align: start` is the right edge, and the logical properties (`margin-inline-start` and the rest) take the right side as their start." },
    Entry { name: "visibility", detail: "hide a box but keep its place", doc: "`hidden` draws nothing and takes no tap or focus, but the box keeps its space, unlike `display: none`. Inherited: a child can be `visible` inside a hidden box. `collapse` is `hidden`." },
    Entry { name: "color-scheme", detail: "which side of light-dark() colours take", doc: "`light dark` follows the system's light or dark setting, `dark` is always dark, and `normal` (the default) is light whatever the system says, as in CSS. Inherited, so setting it on the screen sets it for the page." },
    Entry { name: "margin-inline", detail: "the margins at the start and end of the line", doc: "One value for both, or the start's then the end's. Start is left under `ltr` and right under `rtl`." },
    Entry { name: "margin-inline-start", detail: "the margin where a line starts", doc: "Left under `ltr`, right under `rtl`." },
    Entry { name: "margin-inline-end", detail: "the margin where a line ends", doc: "Right under `ltr`, left under `rtl`." },
    Entry { name: "margin-block", detail: "the top and bottom margins", doc: "One value for both, or the top's then the bottom's." },
    Entry { name: "margin-block-start", detail: "the top margin", doc: "Rux writes horizontally, so the block start is always the top." },
    Entry { name: "margin-block-end", detail: "the bottom margin", doc: "Always the bottom." },
    Entry { name: "padding-inline", detail: "the padding at the start and end of the line", doc: "One value for both, or the start's then the end's." },
    Entry { name: "padding-inline-start", detail: "the padding where a line starts", doc: "Left under `ltr`, right under `rtl`." },
    Entry { name: "padding-inline-end", detail: "the padding where a line ends", doc: "Right under `ltr`, left under `rtl`." },
    Entry { name: "padding-block", detail: "the top and bottom padding", doc: "One value for both, or the top's then the bottom's." },
    Entry { name: "padding-block-start", detail: "the top padding", doc: "Always the top." },
    Entry { name: "padding-block-end", detail: "the bottom padding", doc: "Always the bottom." },
    Entry { name: "inset-inline", detail: "`left` and `right` by the direction", doc: "One value for both, or the start's then the end's." },
    Entry { name: "inset-inline-start", detail: "the offset from the start side", doc: "`left` under `ltr`, `right` under `rtl`." },
    Entry { name: "inset-inline-end", detail: "the offset from the end side", doc: "`right` under `ltr`, `left` under `rtl`." },
    Entry { name: "inset-block", detail: "`top` and `bottom`", doc: "One value for both, or the top's then the bottom's." },
    Entry { name: "inset-block-start", detail: "the same as `top`", doc: "Always the top." },
    Entry { name: "inset-block-end", detail: "the same as `bottom`", doc: "Always the bottom." },
    Entry { name: "vertical-align", detail: "where a box in a line of text sits", doc: "For an `<image>` or `<path>` inside a `<text>`. `baseline` (the default) stands it on the baseline, so an icon as tall as the text sits high; `middle` centres it on the text, which is usually what an icon wants; `text-top` and `text-bottom` line it up with the top or bottom of the text." },
    Entry { name: "tab-size", detail: "how wide a kept tab is", doc: "A number of spaces, 8 by default, or a length: the width of a tab that `pre` or `pre-wrap` keeps. Inherited." },
    Entry { name: "resize", detail: "a grip that drags the box bigger", doc: "`both`, `horizontal`, `vertical` or `none`, on a scroller: its corner drags its size with a mouse. A textarea is `both` unless it says. Not on Android." },
    Entry { name: "white-space", detail: "whether runs of space collapse and lines wrap", doc: "`normal` and `nowrap` turn every run of spaces, tabs and newlines into one space, as CSS does; `pre` and `pre-wrap` keep them all; `pre-line` keeps newlines only, the one for text from data whose line breaks matter. `nowrap` and `pre` keep the text on one line." },
    Entry { name: "text-decoration", detail: "lines over, under or through the text", doc: "Its lines (`underline`, `overline`, `line-through`), style, colour and thickness, in any order." },
    Entry { name: "text-decoration-line", detail: "which decoration to draw", doc: "`underline`, `overline` and `line-through`, any of them together." },
    Entry { name: "text-decoration-color", detail: "the colour of the decoration", doc: "The text's own colour when not set." },
    Entry { name: "text-decoration-style", detail: "how the decoration is drawn", doc: "`solid`, `double`, `dotted`, `dashed` or `wavy`." },
    Entry { name: "text-decoration-thickness", detail: "how thick the decoration is", doc: "A length, or `auto` for the font's own." },
    Entry { name: "text-underline-offset", detail: "how far below the baseline the underline is", doc: "A length, or `auto` for the font's own." },
    Entry { name: "text-transform", detail: "the text in capitals or small letters", doc: "`uppercase`, `lowercase` or `capitalize`. Inherited." },
    Entry { name: "text-indent", detail: "how far in the first line starts", doc: "A length or a percentage of the width, with `hanging` to indent every line but the first, and `each-line`. Inherited." },
    Entry { name: "text-overflow", detail: "what a line too long for its box ends in", doc: "`ellipsis` ends it in `…`. Needs `overflow: hidden` and `white-space: nowrap`, as in CSS." },
    Entry { name: "animation", detail: "run a @keyframes on this box", doc: "`<name> <duration> <easing> <delay> <count> <direction> <fill> <play-state>`, in any order after the name, comma-separated for more than one. It starts when the box first appears and keeps running while the box keeps the name; a new name starts it again. The keyframes animate what `transition` can; `prefers-reduced-motion` stills it, as it stills a transition." },
    Entry { name: "animation-name", detail: "which @keyframes to run", doc: "A comma list, one animation for each name; the other `animation-*` lists are repeated to match. `none` is none. A name with no `@keyframes` warns." },
    Entry { name: "animation-duration", detail: "how long one run takes", doc: "A comma list of times. `0s` is over at once: the box shows its own style, or its last keyframe when filled forwards." },
    Entry { name: "animation-timing-function", detail: "the easing between keyframes", doc: "The same easings `transition` takes. It runs from each keyframe to the next, not over the whole run, and a keyframe can set its own." },
    Entry { name: "animation-delay", detail: "how long before it starts", doc: "A comma list of times. Until then the box shows its own style, or the first keyframe when filled backwards." },
    Entry { name: "animation-iteration-count", detail: "how many times it runs", doc: "A number, fractions included, or `infinite`." },
    Entry { name: "animation-direction", detail: "which way each run goes", doc: "`normal`, `reverse`, `alternate` (every other run backwards) or `alternate-reverse`." },
    Entry { name: "animation-fill-mode", detail: "whether the ends hold outside the run", doc: "`forwards` keeps the last keyframe after the end, `backwards` shows the first during the delay, `both` does both. `none`, the default, gives the box its own style outside the run." },
    Entry { name: "animation-play-state", detail: "pause or resume it", doc: "`paused` stops the clock where it is, and `running` picks it up from there." },
    Entry { name: "transform-origin", detail: "the point a transform turns and grows about", doc: "`center` (the default), keywords in either order (`top left`), or lengths and percentages of the box, x then y. A third value, the z offset, is read past." },
    Entry { name: "will-change", detail: "a hint that a property is about to change", doc: "Rux needs no warning to be fast about it, so it is a hint and nothing more, except as CSS has it: naming a property that makes a stacking context (`transform`, `opacity`, `filter` and the like) makes the box one now." },
    Entry { name: "text-wrap", detail: "whether and how lines wrap", doc: "`nowrap` keeps the text on one line, as `white-space: nowrap` does, and `wrap` lets it wrap again. Inherited. `balance` evens the lines out, as many as wrapping makes, the box keeping its width. `pretty` and `stable` wrap as `wrap` does, which CSS allows." },
    Entry { name: "text-wrap-mode", detail: "whether lines wrap", doc: "`wrap` or `nowrap`: the wrapping half of `white-space`. Inherited." },
    Entry { name: "text-wrap-style", detail: "how wrapped lines are chosen", doc: "`balance` evens the lines out, as many as wrapping makes; `auto`, `pretty` and `stable` wrap as usual. Inherited." },
    Entry { name: "user-select", detail: "whether the person can select this text", doc: "`none` and `auto` are honoured as they are: text outside a field cannot be selected yet, and a field's text always can, as in a browser. `text` and `all` warn, since there is nothing yet for them to turn on." },
    Entry { name: "line-clamp", detail: "the most lines a text may wrap to", doc: "`line-clamp: 2` keeps two lines, the second ending in `…`, and the text is as tall as the two. Tailwind's `line-clamp-2` writes it as `-webkit-line-clamp: 2` beside `display: -webkit-box`, which is read too; the prefixed one alone does nothing, as in a browser. On a `<text>`; text with runs is not cut yet." },
    Entry { name: "font-variant", detail: "small capitals, numerals and ligatures", doc: "Any of the `font-variant-*` keywords; the font's own OpenType features." },
    Entry { name: "font-variant-caps", detail: "small capitals", doc: "`small-caps`, `all-small-caps`, `petite-caps` and the rest, when the font has them." },
    Entry { name: "font-variant-numeric", detail: "how numerals are drawn", doc: "`tabular-nums` for columns that line up, `oldstyle-nums`, fractions, `ordinal`, `slashed-zero`." },
    Entry { name: "font-variant-ligatures", detail: "which ligatures the font joins", doc: "`none`, `no-common-ligatures`, `discretionary-ligatures` and the rest." },
    Entry { name: "font-variant-position", detail: "the font's own subscripts and superscripts", doc: "`sub` or `super`, when the font has them." },
    Entry { name: "font-variant-east-asian", detail: "East Asian glyph forms", doc: "`jis78` to `jis04`, `simplified`, `traditional`, `full-width`, `proportional-width`, `ruby`." },
    Entry { name: "font-feature-settings", detail: "OpenType features by tag", doc: "`\"tnum\"`, `\"liga\" 0`: drawn after `font-variant`, so it wins where they meet." },
    Entry { name: "font-variation-settings", detail: "a variable font's axes by tag", doc: "`\"wght\" 650, \"wdth\" 80`." },
    Entry { name: "font-stretch", detail: "a narrower or wider face", doc: "`condensed` to `expanded`, or a percentage, from the font's width axis or its narrow and wide faces." },
    Entry { name: "font-width", detail: "the newer name of `font-stretch`", doc: "The same values." },
    Entry { name: "caret-color", detail: "the colour of a field's caret", doc: "`auto` is the text's colour. Inherited." },
    Entry { name: "object-fit", detail: "how an image fills its box", doc: "`fill` (stretched, the default), `contain`, `cover`, `none` or `scale-down`." },
    Entry { name: "object-position", detail: "where an image sits in its box", doc: "One to four values, as `background-position`: `left`, `right 8px bottom 8px`." },
    Entry { name: "pointer-events", detail: "whether a pointer can reach the box", doc: "`none` lets a tap through to what is under it; the keyboard still reaches it. Inherited." },
    Entry { name: "scroll-behavior", detail: "whether a scroll asked for glides", doc: "`smooth` on a scroller: `scrollIntoView()` and Tab glide there. A wheel or a finger never does." },
    Entry { name: "scroll-snap-type", detail: "where a scroller comes to rest", doc: "On a scroller: `x`, `y` or `both`, then `mandatory` (always rests on a snap point) or `proximity` (only when one is near, the default). A wheel, a finger, a fling and `scrollIntoView()` all come to rest on one." },
    Entry { name: "scroll-snap-align", detail: "where a box meets its scroller's view", doc: "On a box in a snapping scroller: `start`, `center` or `end`, one value for both axes or block then inline. The scroller can rest with that edge of the box at the same edge of its view." },
    Entry { name: "scroll-snap-stop", detail: "whether a fling may pass this box", doc: "`always`: a fling stops here rather than passing by, as a carousel's pages do. `normal` lets it pass." },
    Entry { name: "scroll-padding", detail: "insets a snapping scroller's view", doc: "One to four lengths or percentages of the view, clockwise from the top: where the boxes snap to, under a sticky header for one." },
    Entry { name: "scroll-padding-top", detail: "insets the top of a snapping view", doc: "Overrides the `scroll-padding` shorthand for this side." },
    Entry { name: "scroll-padding-right", detail: "insets the right of a snapping view", doc: "Overrides the `scroll-padding` shorthand for this side." },
    Entry { name: "scroll-padding-bottom", detail: "insets the bottom of a snapping view", doc: "Overrides the `scroll-padding` shorthand for this side." },
    Entry { name: "scroll-padding-left", detail: "insets the left of a snapping view", doc: "Overrides the `scroll-padding` shorthand for this side." },
    Entry { name: "scroll-margin", detail: "grows a box for snapping", doc: "One to four lengths, clockwise from the top, added around the box where it snaps, and nowhere else." },
    Entry { name: "scroll-margin-top", detail: "grows a box's top for snapping", doc: "Overrides the `scroll-margin` shorthand for this side." },
    Entry { name: "scroll-margin-right", detail: "grows a box's right for snapping", doc: "Overrides the `scroll-margin` shorthand for this side." },
    Entry { name: "scroll-margin-bottom", detail: "grows a box's bottom for snapping", doc: "Overrides the `scroll-margin` shorthand for this side." },
    Entry { name: "scroll-margin-left", detail: "grows a box's left for snapping", doc: "Overrides the `scroll-margin` shorthand for this side." },
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
    ("anchor-name", ".more { anchor-name: --row-menu; }"),
    ("aspect-ratio", ".cover { width: 100%; aspect-ratio: 16 / 9; }"),
    ("box-sizing", "* { box-sizing: border-box; }"),
    ("background", ".app { background: #1e1e2e; }"),
    ("background-color", ".send:hover { background-color: #b4befe; }"),
    ("background-image", ".hero { background-image: linear-gradient(#89b4fa, #1e1e2e); }"),
    ("background-size", ".hero { background-size: cover; }"),
    ("background-position", ".hero { background-position: right 16px bottom 16px; }"),
    ("background-position-x", ".hero { background-position-x: right 16px; }"),
    ("background-position-y", ".hero { background-position-y: bottom; }"),
    ("background-repeat", ".pattern { background-repeat: repeat-x; }"),
    ("background-clip", ".card { background-clip: padding-box; }"),
    ("background-origin", ".card { background-origin: content-box; }"),
    ("background-attachment", ".page { background-attachment: fixed; }"),
    ("border", ".field { border: 2px solid #45475a; }"),
    ("border-bottom", ".row { border-bottom: 1px solid #313244; }"),
    ("border-bottom-left-radius", ".bubble { border-bottom-left-radius: 2px; }"),
    ("border-bottom-right-radius", ".bubble { border-bottom-right-radius: 2px; }"),
    ("border-bottom-width", ".row { border-bottom-width: 1px; }"),
    ("border-color", ".field:focus { border-color: #89b4fa; }"),
    ("border-top-color", ".tab { border-top-color: #89b4fa; }"),
    ("border-right-color", ".cell { border-right-color: #313244; }"),
    ("border-bottom-color", ".row { border-bottom-color: #313244; }"),
    ("border-left-color", ".quote { border-left-color: #f9e2af; }"),
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
    ("outline", ".send:focus-visible { outline: 2px solid #f9e2af; }"),
    ("outline-color", ".send { outline-color: #f38ba8; }"),
    ("outline-offset", ".card:focus-visible { outline-offset: 3px; }"),
    ("outline-style", ".tab:focus-visible { outline-style: none; }"),
    ("outline-width", ".send { outline-width: 3px; }"),
    ("columns", ".article { columns: 2 240px; }"),
    ("column-count", ".tags { column-count: 3; }"),
    ("column-width", ".tags { column-width: 160px; }"),
    ("column-fill", ".tags { column-fill: auto; height: 300px; }"),
    ("column-rule", ".article { column-rule: 1px solid #45475a; }"),
    ("column-rule-width", ".article { column-rule-width: 2px; }"),
    ("column-rule-style", ".article { column-rule-style: solid; }"),
    ("column-rule-color", ".article { column-rule-color: #45475a; }"),
    ("content", ".required::after { content: \" *\"; color: #f38ba8; }"),
    ("accent-color", ".volume { accent-color: #f38ba8; }"),
    ("appearance", ".tick { appearance: none; } .tick:checked { background: #89b4fa; }"),
    ("color", ".label { color: #cdd6f4; }"),
    ("column-gap", ".grid { column-gap: 8px; }"),
    ("cursor", ".send { cursor: pointer; }"),
    ("touch-action", ".slider { touch-action: none; }"),
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
    ("font", ".title { font: 600 1.25rem/1.4 Inter, sans-serif; }"),
    ("font-size", ".title { font-size: 22px; }"),
    ("font-style", ".empty { font-style: italic; }"),
    ("font-weight", ".title { font-weight: 700; }"),
    ("gap", ".row { display: flex; gap: 8px; }"),
    ("grid-auto-columns", ".grid { grid-auto-columns: 1fr; }"),
    ("grid-auto-flow", ".grid { grid-auto-flow: column; }"),
    ("grid-auto-rows", ".grid { grid-auto-rows: 80px; }"),
    ("grid-column", ".wide { grid-column: 1 / 3; }"),
    ("grid-template-areas", ".page { display: grid; grid-template-areas: \"head head\" \"side main\"; }"),
    ("grid-template", ".page { display: grid; grid-template: \"head head\" 60px \"side main\" 1fr / 200px 1fr; }"),
    ("grid-area", ".content { grid-area: main; }"),
    ("grid-column-end", ".cell { grid-column-end: 4; }"),
    ("grid-column-start", ".cell { grid-column-start: 2; }"),
    ("grid-row", ".tall { grid-row: span 2; }"),
    ("grid-row-end", ".cell { grid-row-end: 3; }"),
    ("grid-row-start", ".cell { grid-row-start: 1; }"),
    ("grid-template-columns", ".grid { display: grid; grid-template-columns: repeat(3, 1fr); }"),
    ("grid-template-rows", ".grid { grid-template-rows: auto 1fr auto; }"),
    ("height", ".thread { height: 300px; }"),
    ("isolation", ".card { position: relative; isolation: isolate; }"),
    ("justify-content", ".row { display: flex; justify-content: space-between; }"),
    ("justify-items", ".grid { justify-items: start; }"),
    ("place-items", ".grid { place-items: center; }"),
    ("place-content", ".grid { place-content: center space-between; }"),
    ("place-self", ".badge { place-self: end; }"),
    ("justify-self", ".cell { justify-self: center; }"),
    ("left", ".badge { position: absolute; left: 8px; }"),
    ("inset", ".scrim { position: absolute; inset: 0; }"),
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
    ("mix-blend-mode", ".stamp { mix-blend-mode: multiply; }"),
    ("clip-path", ".avatar { clip-path: circle(50%); }"),
    ("filter", ".disabled { filter: grayscale(1) opacity(0.6); }"),
    ("text-shadow", ".title { text-shadow: 0 1px 2px #00000066; }"),
    ("border-style", ".drop-zone { border-width: 2px; border-style: dashed; }"),
    ("border-top-style", ".row { border-top-style: dotted; }"),
    ("border-right-style", ".row { border-right-style: dotted; }"),
    ("border-bottom-style", ".row { border-bottom-style: dotted; }"),
    ("border-left-style", ".row { border-left-style: dotted; }"),
    ("backdrop-filter", ".sheet { backdrop-filter: blur(12px); background: #ffffff99; }"),
    ("mask-image", ".fade { mask-image: linear-gradient(black, transparent); }"),
    ("mask", ".fade { mask: linear-gradient(to right, black 80%, transparent); }"),
    ("mask-mode", ".logo { mask-mode: luminance; }"),
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
    ("position-anchor", ".menu { position: fixed; position-anchor: --row-menu; top: anchor(bottom); left: anchor(left); }"),
    ("position-try", ".tip { position-try: most-height flip-block; }"),
    ("content-visibility", ".row { content-visibility: auto; contain-intrinsic-size: auto 40px; }"),
    ("contain-intrinsic-size", ".row { contain-intrinsic-size: auto 40px; }"),
    ("contain-intrinsic-width", ".card { contain-intrinsic-width: 200px; }"),
    ("contain-intrinsic-height", ".row { contain-intrinsic-height: auto 40px; }"),
    ("contain-intrinsic-inline-size", ".card { contain-intrinsic-inline-size: 200px; }"),
    ("contain-intrinsic-block-size", ".row { contain-intrinsic-block-size: auto 40px; }"),
    ("position-try-fallbacks", ".menu { position-try-fallbacks: flip-block, --left; }"),
    ("position-try-order", ".menu { position-try-order: most-height; }"),
    ("overscroll-behavior", ".sheet .list { overscroll-behavior: contain; }"),
    ("overscroll-behavior-x", ".carousel { overscroll-behavior-x: contain; }"),
    ("overscroll-behavior-y", ".panel { overscroll-behavior-y: contain; }"),
    ("right", ".badge { position: absolute; right: 8px; }"),
    ("row-gap", ".grid { row-gap: 12px; }"),
    ("scrollbar-color", ".thread { scrollbar-color: #89b4fa transparent; }"),
    ("scrollbar-width", ".tabs { scrollbar-width: none; }"),
    ("stroke", ".mark { stroke: #1e1e2e; }"),
    ("stroke-linecap", ".mark { stroke-linecap: round; }"),
    ("stroke-linejoin", ".mark { stroke-linejoin: round; }"),
    ("stroke-width", ".mark { stroke-width: 2; }"),
    ("text-align", ".title { text-align: center; }"),
    ("text-decoration", ".link { text-decoration: underline; }"),
    ("text-decoration-line", ".done { text-decoration-line: line-through; }"),
    ("text-decoration-color", ".link { text-decoration-color: #89b4fa; }"),
    ("text-decoration-style", ".typo { text-decoration-style: wavy; }"),
    ("text-decoration-thickness", ".link { text-decoration-thickness: 2px; }"),
    ("text-underline-offset", ".link { text-underline-offset: 4px; }"),
    ("text-transform", ".label { text-transform: uppercase; }"),
    ("text-indent", ".para { text-indent: 2em; }"),
    ("text-overflow", ".name { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }"),
    ("line-clamp", ".summary { line-clamp: 2; }"),
    ("user-select", ".label { user-select: none; }"),
    ("text-wrap", ".tag { text-wrap: nowrap; }"),
    ("will-change", ".drawer { will-change: transform; }"),
    ("text-wrap-mode", ".tag { text-wrap-mode: nowrap; }"),
    ("text-wrap-style", ".lede { text-wrap-style: pretty; }"),
    ("transform-origin", ".menu { transform-origin: top right; }"),
    ("animation", ".spinner { animation: spin 1s linear infinite; }"),
    ("animation-name", ".toast { animation-name: slide-in; }"),
    ("animation-duration", ".toast { animation-duration: 300ms; }"),
    ("animation-timing-function", ".toast { animation-timing-function: ease-out; }"),
    ("animation-delay", ".toast { animation-delay: 1s; }"),
    ("animation-iteration-count", ".pulse { animation-iteration-count: infinite; }"),
    ("animation-direction", ".breathe { animation-direction: alternate; }"),
    ("animation-fill-mode", ".toast { animation-fill-mode: forwards; }"),
    ("animation-play-state", ".spinner.idle { animation-play-state: paused; }"),
    ("font-variant", ".heading { font-variant: small-caps; }"),
    ("font-variant-caps", ".heading { font-variant-caps: all-small-caps; }"),
    ("font-variant-numeric", ".price { font-variant-numeric: tabular-nums; }"),
    ("font-variant-ligatures", ".code { font-variant-ligatures: none; }"),
    ("font-variant-position", ".note { font-variant-position: super; }"),
    ("font-variant-east-asian", ".jp { font-variant-east-asian: jis04; }"),
    ("font-feature-settings", ".price { font-feature-settings: \"tnum\"; }"),
    ("font-variation-settings", ".title { font-variation-settings: \"wght\" 650; }"),
    ("font-stretch", ".dense { font-stretch: condensed; }"),
    ("font-width", ".dense { font-width: 75%; }"),
    ("caret-color", ".field { caret-color: #f38ba8; }"),
    ("object-fit", ".avatar { object-fit: cover; }"),
    ("object-position", ".avatar { object-position: top; }"),
    ("pointer-events", ".glass { pointer-events: none; }"),
    ("scroll-behavior", ".list { scroll-behavior: smooth; }"),
    ("scroll-snap-type", ".pages { scroll-snap-type: x mandatory; }"),
    ("scroll-snap-align", ".page { scroll-snap-align: center; }"),
    ("scroll-snap-stop", ".page { scroll-snap-stop: always; }"),
    ("scroll-padding", ".list { scroll-padding: 48px 0 0; }"),
    ("scroll-padding-top", ".list { scroll-padding-top: 48px; }"),
    ("scroll-padding-right", ".list { scroll-padding-right: 16px; }"),
    ("scroll-padding-bottom", ".list { scroll-padding-bottom: 16px; }"),
    ("scroll-padding-left", ".list { scroll-padding-left: 16px; }"),
    ("scroll-margin", ".row { scroll-margin: 8px; }"),
    ("scroll-margin-top", ".row { scroll-margin-top: 8px; }"),
    ("scroll-margin-right", ".row { scroll-margin-right: 8px; }"),
    ("scroll-margin-bottom", ".row { scroll-margin-bottom: 8px; }"),
    ("scroll-margin-left", ".row { scroll-margin-left: 8px; }"),
    ("top", ".heading { position: sticky; top: 0; }"),
    ("transform", ".msg:enter-from { transform: translateY(10px); }"),
    ("transition", ".msg { transition: opacity 180ms ease-out; }"),
    ("transition-property", ".btn { transition-property: color, background-color; }"),
    ("transition-duration", ".btn { transition-duration: 150ms; }"),
    ("transition-timing-function", ".btn { transition-timing-function: ease-out; }"),
    ("transition-delay", ".btn { transition-delay: 50ms; }"),
    ("transition-behavior", ".menu { transition-behavior: allow-discrete; }"),
    ("translate", ".card:hover { translate: 0 -4px; }"),
    ("rotate", ".chevron.open { rotate: 180deg; }"),
    ("scale", ".btn:active { scale: 0.97; }"),
    ("tab-size", ".code { white-space: pre; tab-size: 4; }"),
    ("resize", ".notes { resize: vertical; }"),
    ("white-space", ".label { white-space: nowrap; }"),
    ("vertical-align", ".icon { vertical-align: middle; }"),
    ("direction", ".page { direction: rtl; }"),
    ("color-scheme", "screen { color-scheme: light dark; }"),
    ("visibility", ".spinner { visibility: hidden; }"),
    ("margin-inline-start", ".label { margin-inline-start: 8px; }"),
    ("margin-inline", ".chip { margin-inline: 4px 12px; }"),
    ("margin-inline-end", ".icon { margin-inline-end: 6px; }"),
    ("margin-block", ".section { margin-block: 16px; }"),
    ("margin-block-start", ".section { margin-block-start: 24px; }"),
    ("margin-block-end", ".section { margin-block-end: 24px; }"),
    ("padding-inline", ".button { padding-inline: 16px; }"),
    ("padding-inline-start", ".item { padding-inline-start: 24px; }"),
    ("padding-inline-end", ".item { padding-inline-end: 8px; }"),
    ("padding-block", ".button { padding-block: 8px; }"),
    ("padding-block-start", ".card { padding-block-start: 12px; }"),
    ("padding-block-end", ".card { padding-block-end: 12px; }"),
    ("inset-inline", ".bar { position: absolute; inset-inline: 0; }"),
    ("inset-inline-start", ".badge { position: absolute; inset-inline-start: 4px; }"),
    ("inset-inline-end", ".close { position: absolute; inset-inline-end: 8px; }"),
    ("inset-block", ".overlay { position: absolute; inset-block: 0; }"),
    ("inset-block-start", ".bar { position: fixed; inset-block-start: 0; }"),
    ("inset-block-end", ".bar { position: fixed; inset-block-end: 0; }"),
    ("width", ".field { width: 100%; }"),
    ("word-break", ".code { word-break: break-all; }"),
    ("word-spacing", ".lead { word-spacing: 1px; }"),
    ("word-wrap", ".body { word-wrap: break-word; }"),
    ("z-index", ".menu { position: absolute; z-index: 10; }"),
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
    out.push_str("  },\n  \"attributeValues\": {\n");
    for (i, (tag, attrs)) in ATTRIBUTE_VALUES.iter().enumerate() {
        out.push_str(&format!("    {}: {{\n", quote(tag)));
        for (j, (attribute, values)) in attrs.iter().enumerate() {
            out.push_str(&format!("      {}: ", quote(attribute)));
            entries(&mut out, values, 3);
            out.push_str(if j + 1 == attrs.len() { "\n" } else { ",\n" });
        }
        out.push_str(if i + 1 == ATTRIBUTE_VALUES.len() { "    }\n" } else { "    },\n" });
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

/// The honored-CSS reference page, `site/content/reference/css-properties.md`,
/// written by `scripts/sync-vocabulary.sh` and checked by it in CI.
///
/// **Generated, so it cannot drift** (roadmap v0.9 item 6). The hand-kept list
/// under "Honored CSS" in `docs/05-as-built.md` was found in 2026-08 to have
/// carried three false claims for a release or more. Every name here comes from
/// the runtime: the honored properties and pseudo-classes from `rux_style`,
/// each described by the same entry the editor shows on hover, which a test
/// below already requires of every honored property; the keywords from the
/// value table the editor completes from; whether it animates from
/// `animatable_properties`; and what is known and not built from the list the
/// warning reads.
pub fn css_reference() -> i32 {
    print!("{}", css_reference_page());
    0
}

fn css_reference_page() -> String {
    // A `|` inside a table cell ends the cell.
    let cell = |s: &str| squash(s).replace('|', "\\|");
    let animatable = rux_style::animatable_properties();
    let mut out = String::new();
    out.push_str(concat!(
        "+++\n",
        "title = \"CSS properties\"\n",
        "description = \"Every CSS property and pseudo-class the runtime honors, generated from the runtime itself.\"\n",
        "weight = 4\n",
        "+++\n\n",
        "<!-- GENERATED BY `rux vocab --css-reference` (scripts/sync-vocabulary.sh). DO NOT EDIT HERE. -->\n\n",
    ));
    out.push_str(&format!(
        "Every property below is one the runtime interprets, and the list is written by the \
         runtime: a property is on this page exactly when the runtime honors it, and is \
         described in the words the editor shows on hover. Anything else is parsed, ignored \
         and reported once on stderr. [Honored CSS](/reference/css/) says how each one behaves \
         in detail, and [where Rux differs from CSS](/reference/css-differences/) lists the \
         places it does not answer as a browser would.\n\n\
         {} properties and {} pseudo-classes, as of `rux {}`.\n\n",
        CSS_PROPERTY_DOCS.len(),
        PSEUDO_CLASSES.len(),
        env!("CARGO_PKG_VERSION"),
    ));
    out.push_str("## Properties\n\n| Property | What it does | Keywords | Animates |\n|---|---|---|---|\n");
    for e in CSS_PROPERTY_DOCS {
        let keywords = CSS_VALUES
            .iter()
            .find(|(name, _)| *name == e.name)
            .map(|(_, values)| values.iter().map(|v| format!("`{v}`")).collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        let usage = CSS_PROPERTY_USAGE
            .iter()
            .find(|(name, _)| *name == e.name)
            .map(|(_, example)| format!(" For example `{example}`"))
            .unwrap_or_default();
        out.push_str(&format!(
            "| `{}` | **{}.** {}{} | {} | {} |\n",
            e.name,
            cell(e.detail),
            cell(e.doc),
            cell(&usage),
            keywords,
            if animatable.contains(&e.name) { "yes" } else { "" },
        ));
    }
    out.push_str("\n## Pseudo-classes\n\n| Pseudo-class | What it matches |\n|---|---|\n");
    for e in PSEUDO_CLASSES {
        out.push_str(&format!("| `:{}` | **{}.** {} |\n", e.name, cell(e.detail), cell(e.doc)));
    }
    out.push_str(
        "\n## Known, and not honored yet\n\n\
         Real CSS an author reaches for, which the runtime parses, ignores and reports as a \
         pending feature rather than a typo:\n\n",
    );
    let pending: Vec<String> = rux_style::unimplemented_properties().iter().map(|p| format!("`{p}`")).collect();
    out.push_str(&pending.join(", "));
    out.push_str(".\n");
    out
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
    /// The reference page has a row for every honored property and
    /// pseudo-class, and lists every pending one, because it is written from
    /// the same lists. A property the runtime honors and the page leaves out
    /// is the drift the page exists to make impossible.
    #[test]
    fn the_css_page_names_everything_the_runtime_honors() {
        let page = css_reference_page();
        for p in rux_style::honored_properties() {
            assert!(page.contains(&format!("| `{p}` |")), "`{p}` has no row");
        }
        for p in rux_style::honored_pseudo_classes() {
            assert!(page.contains(&format!("| `:{p}` |")), "`:{p}` has no row");
        }
        for p in rux_style::unimplemented_properties() {
            assert!(page.contains(&format!("`{p}`")), "`{p}` is not listed as pending");
        }
        let rows = page.lines().filter(|l| l.starts_with("| `")).count();
        assert_eq!(rows, rux_style::honored_properties().len() + rux_style::honored_pseudo_classes().len());
    }

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

    /// Every attribute with a value table has to be an attribute that element
    /// actually takes, and the set offered has to be the same set its `detail`
    /// line advertises.
    ///
    /// The `detail` is what a reader sees in the attribute completion
    /// (`text | textarea | select | checkbox | radio`) and the table is what the
    /// value completion offers. Two hand-kept lists of the same thing drift, and
    /// the way it would show is the editor offering a value its own tooltip does
    /// not mention.
    #[test]
    fn attribute_values_match_their_detail_line() {
        for (tag, attrs) in ATTRIBUTE_VALUES {
            let declared = ELEMENT_ATTRIBUTES
                .iter()
                .find(|(name, _)| name == tag)
                .unwrap_or_else(|| panic!("`{tag}` has value tables and is not an element"))
                .1;
            for (attribute, values) in *attrs {
                let entry = declared
                    .iter()
                    .find(|e| e.name == *attribute)
                    .unwrap_or_else(|| panic!("`<{tag} {attribute}>` is not an attribute"));
                assert!(!values.is_empty(), "`<{tag} {attribute}>` has an empty value list");
                let advertised: Vec<&str> = entry.detail.split('|').map(str::trim).collect();
                let offered: Vec<&str> = values.iter().map(|v| v.name).collect();
                assert_eq!(
                    advertised, offered,
                    "`<{tag} {attribute}>` advertises one set and offers another"
                );
            }
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

    /// The elements this prints and the ones the runtime accepts are one list.
    ///
    /// They became two answers to one question when an unknown tag became an
    /// error: the runtime decides what a tag may be, and this decides what the
    /// editor offers, and a name in one and not the other is either a
    /// completion that fails to load or an element nobody can find. The same
    /// drift that put `img` in the void list and left out Rux's `<image>`.
    ///
    /// `router-view` is the one deliberate difference: it is a tag Rux defines
    /// and is documented with the router rather than on its own, so it is in
    /// the parser's list and not in this one.
    #[test]
    fn every_element_offered_is_one_the_runtime_knows() {
        for element in ELEMENTS {
            assert!(
                rux_parser::is_element(element.name),
                "`<{}>` is offered here and the runtime would call it a component",
                element.name
            );
        }
        let offered: Vec<&str> = ELEMENTS.iter().map(|e| e.name).collect();
        let missing: Vec<&&str> =
            rux_parser::element_tags().iter().filter(|t| !offered.contains(t)).collect();
        assert_eq!(missing, vec![&"router-view"], "the only element not documented alone");
    }

    /// The attributes offered per element are the ones the runtime accepts, and
    /// no others. The runtime refuses any attribute it does not read, so an
    /// attribute offered here and missing there is a completion that fails
    /// `rux check`, and one missing here is a working attribute nobody can find.
    #[test]
    fn every_attribute_offered_is_one_the_runtime_accepts() {
        let listener = |n: &str| n.starts_with('@');
        for tag in rux_parser::element_tags() {
            let mut offered: Vec<&str> = GLOBAL_ATTRIBUTES
                .iter()
                .chain(DIRECTIVES.iter())
                .map(|e| e.name)
                .filter(|n| !listener(n))
                .collect();
            if let Some((_, own)) = ELEMENT_ATTRIBUTES.iter().find(|(t, _)| t == tag) {
                offered.extend(own.iter().map(|e| e.name).filter(|n| !listener(n)));
            }
            let mut accepted = rux_parser::attributes_of(tag);
            offered.sort_unstable();
            accepted.sort_unstable();
            // `r-model` is a directive offered everywhere and only accepted on an
            // `<input>`, which is the one element that has a value to bind.
            offered.retain(|n| *n != "r-model" || *tag == "input");
            offered.dedup();
            assert_eq!(offered, accepted, "<{tag}>: the vocabulary and the runtime disagree");
        }
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
            "these are described and not honored: {extra:?}. The editor would be \
             offering help for a property the runtime warns does nothing."
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
                "`{}` is described as \"{}\", which says the editor approves of the \
                 word and nothing about what it does",
                e.name,
                e.detail
            );
            assert!(e.detail.len() > 8, "`{}` has no real description", e.name);
        }
    }

    /// Every name the editor offers, written somewhere in the reference.
    ///
    /// **This is the gate the whole vocabulary arrangement was missing.** One
    /// existed before and checked ten names: the elements, against the first
    /// 600 characters of one section. Nine shipped features walked past it,
    /// because it never looked at directives, gestures, pseudo-classes, script
    /// globals, element attributes or the 97 CSS properties. `<path>` was a
    /// headline feature of v0.7 and no test anywhere noticed it was missing
    /// from the reference tables.
    ///
    /// The promise being kept is the one the vocabulary exists for: if the
    /// editor offers it, it works, and if it works, it is written down. A name
    /// nobody can find is not much better than a name that does not work, and
    /// the audit that found these took a person an afternoon. This takes 40ms.
    ///
    /// The reference is `docs/05-as-built.md` plus `docs/07-script.md`, which
    /// is the pair the site publishes as the reference. `docs/02-spec.md` is
    /// deliberately **not** consulted: it is design history, and letting it
    /// satisfy this gate is what would make it a second reference and start the
    /// drift again.
    #[test]
    fn every_offered_name_is_in_the_reference() {
        let Some(doc) = DocIndex::read() else { return }; // not a checkout
        let mut missing: Vec<String> = Vec::new();

        // Elements are written as markup, so `<view` is the form that counts.
        // A bare `view`, `path`, `slot` or `route` is an ordinary English word
        // and matching it is how the first audit of this convinced itself the
        // spec documented four elements it had never heard of.
        for e in ELEMENTS {
            if !doc.tags.contains(e.name) {
                missing.push(format!("<{}>            (element)", e.name));
            }
        }
        for e in GLOBAL_ATTRIBUTES {
            // The gestures carry their `@`; `class`, `id`, `style`, `role` and
            // `to` are plain names.
            let found = match e.name.strip_prefix('@') {
                Some(bare) => doc.at.contains(bare),
                None => doc.plain.contains(e.name),
            };
            if !found {
                missing.push(format!("{}            (attribute)", e.name));
            }
        }
        for e in DIRECTIVES {
            if !doc.plain.contains(e.name) {
                missing.push(format!("{}            (directive)", e.name));
            }
        }
        for (tag, attrs) in ELEMENT_ATTRIBUTES {
            for e in *attrs {
                // An input's events carry their `@`, as the gestures do.
                let found = match e.name.strip_prefix('@') {
                    Some(bare) => doc.at.contains(bare),
                    None => doc.plain.contains(e.name),
                };
                if !found {
                    missing.push(format!("{}            (attribute of <{tag}>)", e.name));
                }
            }
        }
        for e in SCRIPT_GLOBALS {
            if !doc.plain.contains(e.name) {
                missing.push(format!("{}            (script global)", e.name));
            }
        }
        for e in ELEMENT_PROPERTIES {
            if !doc.plain.contains(e.name) {
                missing.push(format!("{}            (element property)", e.name));
            }
        }
        for e in ELEMENT_METHODS {
            if !doc.plain.contains(e.name) {
                missing.push(format!("{}            (element method)", e.name));
            }
        }
        for e in VALUE_METHODS {
            if !doc.plain.contains(e.name) {
                missing.push(format!("{}            (value method)", e.name));
            }
        }
        for e in PSEUDO_CLASSES {
            if !doc.colon.contains(e.name) {
                missing.push(format!(":{}            (pseudo-class)", e.name));
            }
        }
        // Read from `rux_style`, not from a table here, so honoring a property
        // and never writing it down fails on the commit that honors it.
        for name in rux_style::honored_properties() {
            if !doc.plain.contains(*name) {
                missing.push(format!("{name}            (CSS property)"));
            }
        }

        assert!(
            missing.is_empty(),
            "{} name(s) are offered to authors and appear nowhere in docs/05-as-built.md \
             or docs/07-script.md:\n  {}\n\nWrite them into the reference, or stop \
             offering them. A name has to be written in code voice (inside backticks or \
             a fenced block) to count, because that is how the reference names things and \
             it is what makes an author's search for it succeed.",
            missing.len(),
            missing.join("\n  ")
        );
    }

    /// The names the reference writes, in code voice.
    ///
    /// Only code voice counts: text inside backticks, or inside a fenced block.
    /// Prose mentioning the word "padding" is not documentation of the
    /// `padding` property, and an author searching a page for `margin-top`
    /// is searching for the code form.
    ///
    /// Names are collected as whole tokens rather than matched as substrings,
    /// which is what keeps `border-top` from being satisfied by
    /// `border-top-width`. That exact false positive inflated the hand audit
    /// this replaces, in both directions: it hid missing longhands and invented
    /// missing shorthands.
    struct DocIndex {
        /// Bare names: `padding`, `signal`, `r-for`, `trim`.
        plain: std::collections::HashSet<String>,
        /// Names written as a tag: the `view` in `<view class="card">`.
        tags: std::collections::HashSet<String>,
        /// Names written as an event: the `tap` in `@tap="…"`.
        at: std::collections::HashSet<String>,
        /// Names written as a pseudo-class: the `hover` in `.btn:hover`.
        colon: std::collections::HashSet<String>,
    }

    impl DocIndex {
        fn read() -> Option<Self> {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let mut corpus = String::new();
            for name in ["docs/05-as-built.md", "docs/07-script.md"] {
                corpus.push_str(&std::fs::read_to_string(root.join(name)).ok()?);
                corpus.push('\n');
            }
            Some(Self::index(&Self::code_voice_only(&corpus)))
        }

        /// Keep the fenced blocks and the inline spans, drop the prose.
        ///
        /// Fences are handled first and line by line, because an inline span
        /// may not straddle a line while a fenced block is nothing but
        /// straddling lines. Inside a fence every character counts; outside
        /// one, only what sits between a pair of backticks does.
        fn code_voice_only(corpus: &str) -> String {
            let mut out = String::new();
            let mut fenced = false;
            for line in corpus.lines() {
                if line.trim_start().starts_with("```") {
                    fenced = !fenced;
                    continue;
                }
                if fenced {
                    out.push_str(line);
                    out.push('\n');
                    continue;
                }
                // An odd number of backticks means one is unclosed on this
                // line; the trailing fragment is prose and is dropped with it.
                let mut inside = false;
                for span in line.split('`') {
                    if inside {
                        out.push_str(span);
                        out.push('\n');
                    }
                    inside = !inside;
                }
            }
            out
        }

        fn index(code: &str) -> Self {
            let mut this = Self {
                plain: Default::default(),
                tags: Default::default(),
                at: Default::default(),
                colon: Default::default(),
            };
            let chars: Vec<char> = code.chars().collect();
            let is_name = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
            let mut i = 0;
            while i < chars.len() {
                if !is_name(chars[i]) {
                    i += 1;
                    continue;
                }
                let start = i;
                while i < chars.len() && is_name(chars[i]) {
                    i += 1;
                }
                let token: String = chars[start..i].iter().collect();
                // The character immediately before decides which set it joins.
                // A space between `:` and the name makes it a CSS value
                // (`display: flex`), not a pseudo-class, so only an adjacent
                // marker counts.
                match start.checked_sub(1).map(|p| chars[p]) {
                    Some('<') => {
                        this.tags.insert(token.clone());
                    }
                    Some('@') => {
                        this.at.insert(token.clone());
                    }
                    Some(':') => {
                        this.colon.insert(token.clone());
                    }
                    _ => {}
                }
                // Every token is also a plain name. A tag is still the word,
                // and `r-transition` is written both as an attribute and as
                // prose-in-backticks.
                this.plain.insert(token);
            }
            this
        }
    }

    /// The tokenizer has to be right, or the gate above is a rubber stamp that
    /// passes because everything looks documented. These are the two failures
    /// that inflated the hand audit it replaces.
    #[test]
    fn the_doc_index_reads_code_voice_and_whole_tokens() {
        let index = DocIndex::index(&DocIndex::code_voice_only(
            "The word padding in prose does not count.\n\
             `border-top-width` and `<view>` and `@tap=\"x\"` and `.btn:hover` do.\n\
             ```\n\
             margin-left, r-for\n\
             ```\n",
        ));
        // Prose is not documentation.
        assert!(!index.plain.contains("prose"));
        // A longhand does not document the shorthand hiding inside it.
        assert!(index.plain.contains("border-top-width"));
        assert!(!index.plain.contains("border-top"));
        // Each marker files the name where it belongs.
        assert!(index.tags.contains("view"));
        assert!(index.at.contains("tap"));
        assert!(index.colon.contains("hover"));
        // A fenced block counts in full, backticks or not.
        assert!(index.plain.contains("margin-left"));
        assert!(index.plain.contains("r-for"));
        // A tag is a plain name too, or `<view>` would not document `view`.
        assert!(index.plain.contains("view"));
    }
}
