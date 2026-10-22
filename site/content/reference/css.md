+++
title = "Honored CSS"
description = "The authoritative list of properties the runtime interprets, plus selectors, pseudo-classes and transitions."
weight = 4
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->

```
display (block|flex|grid|inline|none)
flex-direction, justify-content, align-items, gap, row-gap, column-gap
align-self, justify-self, justify-items, align-content
flex-grow, flex-shrink, flex-basis, flex-wrap, flex (shorthand)
grid-template-columns, grid-template-rows   (repeat(), [names], minmax(), fit-content())
grid-template-areas, grid-template, grid-area
grid-column, grid-column-start, grid-column-end
grid-row, grid-row-start, grid-row-end   (1 / 3, span 2, -1, a line or area name)
grid-auto-flow, grid-auto-rows, grid-auto-columns
transform (translate/scale/rotate; visual only; hit regions aren't transformed)
position (static|relative|sticky|absolute|fixed), top, right, bottom, left
inset-inline, inset-inline-start, inset-inline-end,
inset-block, inset-block-start, inset-block-end   (logical: see `direction`)
z-index (auto|<integer>), isolation (auto|isolate)
anchor-name, position-anchor, anchor() in top/right/bottom/left, anchor-size() in width/height
position-try, position-try-fallbacks (flip-block, flip-inline, flip-start, @position-try names),
position-try-order
aspect-ratio
width, height, min-width, max-width, min-height, max-height
padding, padding-top, padding-right, padding-bottom, padding-left
margin, margin-top, margin-right, margin-bottom, margin-left
                       (each shorthand takes 1–4 values)
border, border-width, border-color, border-style (and each side's -width and
                       -style; none, hidden, solid, dashed, dotted, double,
                       groove, ridge, inset, outset; with no style a border is
                       none and has no width, as in CSS; one colour for all four)
border-top, border-right, border-bottom, border-left
border-top-width, border-right-width, border-bottom-width, border-left-width
border-top-style, border-right-style, border-bottom-style, border-left-style
background, background-color, background-image, opacity
  (colour, linear-/radial-gradient, or url(…) image; a comma list of layers, the
   first on top, the colour under them all; clipped to the corners)
background-size, background-position, background-position-x,
background-position-y, background-repeat,
background-clip, background-origin, background-attachment
                      (as CSS: cover, contain, lengths, % and auto; one to four
                       position values, `right 10px bottom 20px` included;
                       repeat, no-repeat, repeat-x/-y, space, round; border-,
                       padding- and content-box; scroll, fixed, local. A url()
                       image is its natural size, tiled from the padding box's
                       top left, as in CSS: before v0.9 it was drawn cover-sized,
                       which is now `background-size: cover`. Not yet:
                       `background-clip: text`, and gradient stop positions in
                       px, which are placed as if they had none)
box-shadow (a comma list, the first on top; `inset` drawn inside the border, over
                       the background; no colour is currentColor; one corner
                       radius, the largest, for an outer shadow's blur)
outline, outline-width, outline-style, outline-color, outline-offset
  (none | solid | auto; drawn outside the border box, over the content, following
   border-radius; the colour defaults to the element's color; see below)
transition (property duration easing delay, comma-separated; see below)
border-radius (1–4 diagonal shorthand; px/rem/em, and % against the box)
border-top-left-radius, border-top-right-radius
border-bottom-right-radius, border-bottom-left-radius
color, font-size, font-weight, font-family, font-style (italic), text-align
letter-spacing, word-spacing, line-height
white-space (normal|nowrap|pre|pre-wrap|pre-line)
vertical-align (baseline|middle|text-top|text-bottom; an <image> or <path> in a <text>)
direction (ltr|rtl; inherited, and set by the `dir` attribute too)
color-scheme (normal | light | dark | light dark; inherited; which side of
                       `light-dark()` a colour takes)
visibility (visible | hidden | collapse; inherited; a hidden box keeps its place,
                       draws nothing and takes no tap or focus, and a child can
                       be `visible` inside it; `collapse` is `hidden`)
margin-inline, margin-inline-start, margin-inline-end,
margin-block, margin-block-start, margin-block-end,
padding-inline, padding-inline-start, padding-inline-end,
padding-block, padding-block-start, padding-block-end   (logical)
text-shadow (a comma list of `x y blur? colour?`, the first on top; inherited;
                       no colour is the text's own; a blur is a real Gaussian,
                       drawn apart around the text, as filter: blur() is)
text-decoration, text-decoration-line (underline / overline / line-through; the
                       longhand wins where both are set)  (color: any CSS colour, see below)
text-decoration-color, text-decoration-style, text-decoration-thickness,
text-underline-offset (solid, double, dotted, dashed, wavy; no colour is the
                       text's; the shorthand takes all four in any order; not
                       inherited, but a run of a sentence takes its sentence's)
text-transform (uppercase, lowercase, capitalize; inherited; a letter whose
                       other case is not one letter, `ß` to `SS`, stays as
                       written, so carets and selections keep their places)
text-indent (a length or a %, with `hanging` and `each-line`; inherited)
text-overflow (clip, ellipsis: a line too long for a box that clips, with
                       `white-space: nowrap`, ends in `…`; a field being edited
                       shows all of it; not yet for a sentence with runs)
font-variant, font-variant-caps, font-variant-numeric, font-variant-ligatures,
font-variant-position, font-variant-east-asian, font-feature-settings,
font-variation-settings, font-stretch, font-width
                      (the font's own OpenType features and width axis, all
                       inherited; a font without the feature draws the text as
                       it is: small capitals are not made up, as a browser does)
caret-color (auto or a colour; inherited)
mix-blend-mode (every CSS mode: multiply, screen, overlay, darken, lighten,
                       color-dodge, color-burn, hard-light, soft-light, difference,
                       exclusion, hue, saturation, color, luminosity; the box and
                       its subtree blend as one layer, a stacking context, as in CSS)
clip-path (inset() with `round`, xywh(), rect(), circle() and ellipse() with
                       `at` and closest-side/farthest-side, polygon() and path()
                       with evenodd, and a box: margin-box, border-box (the
                       default), padding-box, content-box; the box, its shadow,
                       outline and subtree are all cut. Not yet: taps are not
                       cut, so a tap in a clipped-away corner still lands, and
                       url() to an SVG clipPath, which Rux does not have)
mask, mask-image, mask-mode (a gradient or url() over the border box; alpha,
                       or luminance; the box and its subtree show only where the
                       mask does, and nothing outside the box. Not yet:
                       mask-size, -position and -repeat: an image covers the
                       box, as background-image does)
filter, backdrop-filter (blur(), brightness(), contrast(), drop-shadow(),
                       grayscale(), hue-rotate(), invert(), opacity(), saturate(),
                       sepia(), in any order; a stacking context and a containing
                       block, as in CSS. The box and its subtree, or what is behind
                       the box, are drawn apart and run through the GPU; a
                       drop-shadow with no colour is currentColor. Not yet: url()
                       to an SVG filter)
overflow, overflow-x, overflow-y     (hidden|clip = clip; auto|scroll = scroll;
                                      each axis on its own; `overflow: x y`)
overscroll-behavior, -x, -y   (auto | contain | none; contain and none alike)
scroll-behavior (auto | smooth: a scroll a script's `scrollIntoView()` or Tab
                       moving focus asks for glides there over 300ms; a wheel,
                       a finger and a fling never do, and one of them stops a
                       glide on the same scroller, as in CSS)
pointer-events (auto | none; inherited: a pointer goes through the box to what
                       is under it, `:hover` and `:active` do not take it, and a
                       `for=` label does not focus from it; the keyboard still
                       reaches it, and a text field under `none` is not
                       focused by a press)
object-fit, object-position (on `<image>`: fill, contain, cover, none,
                       scale-down; the picture is drawn in the content box,
                       inside the image's padding, and cut to it)
scrollbar-width (auto | thin | none), scrollbar-color (thumb track | auto)
content-visibility (visible | auto | hidden), contain-intrinsic-size (and -width,
                       -height, -inline-size, -block-size; `auto <length>`)
overflow-wrap (break-word), word-wrap (the legacy alias for it), word-break (break-all)
cursor (pointer, on @tap boxes only)
touch-action (auto | none | pan-x | pan-y; who wins between a @drag and a
                       scroller, settled at the drag threshold; touch only)
fill, fill-rule, stroke, stroke-width, stroke-linecap, stroke-linejoin
  (<path> only; see above)
```
**Percentages need a box, and most of these are resolved before there is one.**
`width`, `height`, `min`/`max-*` and the insets become a length that layout
resolves, so `width: 50%` works. So do `padding`, `margin` and `gap`/
`row-gap`/`column-gap` (since 2026-10-13): their percentage is carried to the
layout, a side resolving against the containing block's **width** (the top and
bottom too, as in CSS) and a gap against the box's own content size on its
axis. The border widths, `font-size`, `letter-spacing` and `word-spacing` are
still resolved to plain pixels during the cascade, where there is no box yet,
so a percentage on one of those is **ignored and says so**. Until v0.7.1 it was
ignored in silence, because the interpreter read them with a px-only parser
while the length check validated with a percentage-capable one: the value was
dropped and then pronounced fine. An animated `padding`, `margin` or `gap`
walks the pixel value, so a side written as a percentage does not animate.

**A border needs a style, as in CSS.** `border-style` is `none` until one is
written, and a side whose style is `none` (or `hidden`) has no width, so
`border: 2px red` draws nothing and takes no room. Rux drew it solid until
v0.9, so a border given a width or a colour and no style anywhere is said,
quoting the declaration. A style with no width is `medium`, 3px, and
`thin`, `medium` and `thick` are 1, 3 and 5px.

Every style is drawn. `dashed` is dashes three widths long with gaps as
long, and `dotted` round dots a width across, two widths apart; both follow
the corners when all four sides match. `double` is two lines a third of the
width each. `groove`, `ridge`, `inset` and `outset` are shaded from the
colour, a third darker on the side in shade; CSS leaves the shades to the
browser, and browsers differ. Every style follows `border-radius`; on a rounded
corner the shade changes on the diagonal, as it does in browsers.

**A border may differ per side.** `border-bottom: 2px solid #89b4fa` draws under
the box and nowhere else, which is the underlined-field shape most forms want.
Until v0.7.1 it drew *nothing*: the cascade computed all four sides, and paint
carried one width taken from the top, so `border-bottom` was dropped and
`border-top` was drawn on all four sides. Both silent, and `border-bottom` is
offered by the editor's completion list, which is supposed to mean it works.

Uniform borders are drawn as one stroke, which is what follows the corner
radius. Uneven ones are drawn as four filled edges, so where two different
non-zero widths meet the corner is square rather than mitred diagonally. That
difference is only visible on a box that sets two adjacent sides to different
widths.

**`border-radius` is the exception, and takes a percentage.** It is the one of
these that layout never reads: only paint does, and paint knows the box. So
`border-radius: 50%` is resolved against the laid-out box and a square comes out
a circle. It works on the corner longhands too, and a longhand in px after a
shorthand in `%` replaces that corner in both units.

A percentage resolves against the **shorter side**, not per axis. Rux draws
circular corners (one radius per corner), so `50%` on a 160x60 box is a pill of
radius 30 rather than the ellipse CSS would draw. On a square the two agree
exactly, and the pill is what someone writing `50%` on a button is after.

`border-radius: 9999px` still works and still means "as round as it goes", since
a radius larger than the box is clamped to it. That is what the runtime itself
uses to draw a radio button.

**Selectors:** tag, `.class`, `#id`, `[role="…"]`, compounds, and all four
combinators: descendant (`.a .b`), child (`.a > .b`), next-sibling (`.a + .b`),
subsequent-sibling (`.a ~ .b`).

**Pseudo-classes:** `:hover`, `:focus`, `:focus-visible`, `:active`, `:checked`, `:current` (a
link whose `to` names the path you are on), `:disabled` / `:enabled` (an
`<input>` or `<button>` with or without `disabled`; a plain box is neither, as in
CSS), `:valid` / `:invalid`, `:user-valid` / `:user-invalid` and `:required` /
`:optional` (an input's checks, under [Forms](#forms)), and `:enter-from` / `:leave-to`
(the two sides of an enter/leave swap, below). They stack
(`.btn:hover:active`), count as class-level specificity, and work anywhere in a
chain, `.card:hover .title` recolours the title while the pointer is over the
card. `:hover`/`:active` hold for the whole chain under the pointer, as in CSS;
`:active` is press-to-release and drops if you drag off the element; `:focus`
matches the focused element itself (a field, a select or a `@tap` box), not its
ancestors; `:focus-visible` matches it when the focus should show, which is
browsers' rule: a text field always, anything else only when the keyboard put
focus there. Driven in `examples/pseudo.rux`.

**Where an element sits, and what it is not** (2026-10-22): `:first-child`,
`:nth-child(an+b)` (`odd`, `even`, `3`, `2n+1`, `-n+3`) and `:not(a, b)`.
The place is counted among the elements drawn there, from 1, so a row an
`r-if` leaves out does not count and a list restyles as rows come and go.
`:not()` takes compound selectors (`.done`, `button:disabled`) and counts as
specific as the most specific of them, as in CSS. Not yet: `:last-child`,
`:only-child` and `:nth-last-child()`, which need the number of siblings before
they are built; `:nth-child(… of S)`; and a combinator inside `:not()`
(`:not(.a .b)`). Each of those never matches, with a warning. A rule that reads
a place stops a list from reusing its built rows, as `+` and `~` already do.

**The focus ring is an `outline`.** The default stylesheet is
`:focus-visible { outline: auto }`, as every browser's is, so the ring is
restyled and removed with ordinary CSS:

```css
.send:focus-visible { outline: 2px solid #f9e2af; outline-offset: 2px; }
.tab:focus-visible  { outline: none; background: #313244; }
.card               { outline-color: #f38ba8; }   /* recolours the ring only */
```

An author's `outline-style` (or the `outline` shorthand) on the focused element
replaces the ring; `outline-color`, `outline-width` or `outline-offset` alone
adjust it. `auto` is 2px in the ring's own blue, a little round even on a
square box. `outline` works on any element, focused or not, and takes no room
in the layout. It is painted over the element's content, outside the element's
own `overflow` clip and inside every ancestor's, so an outline on a row in a
scrolled list is clipped by the list, as in a browser. Its style is a border's:
`dashed`, `dotted`, `double` and the 3D styles are drawn as they are for
`border`.

Any *other* pseudo-class (`:last-child`, `::before`) **never
matches**, and says so once on stderr. Before this existed the `:` was silently
dropped, so `.box:hover` parsed as `.box` and applied *unconditionally*, failing
closed is the safer half of that trade.

**Transitions:** `transition` walks a property to its new value instead of
jumping to it, whatever moved it: a signal, a `:class`, or a pseudo-class.
```css
.card { transition: background-color 200ms ease-out, transform 200ms ease-out; }
.card:hover { background: #45475a; transform: translateY(-4px); }

.panel { height: 0; opacity: 0; transition: all 250ms ease-in-out 50ms; }
.panel.open { height: 60px; opacity: 1; }
```
Each entry is a property, a duration, an easing and a delay, in any order after
the property. As in CSS the **first** time is the duration and the second is the
delay, `all` stands for every animatable property, and a bare `transition: 200ms`
means `all 200ms`. Easings: `linear`, `ease` (the default), `ease-in`,
`ease-out`, `ease-in-out`, `cubic-bezier(x1, y1, x2, y2)`, `steps(n)` with
`jump-start` (or `start`), `jump-end` (or `end`, the default), `jump-both` or
`jump-none`, and `step-start` and `step-end`. `linear()` with stops is not
drawn yet: the transition eases with `ease` and says so.

**Animatable:** `opacity`, `background-color`, `color`, `border-color`,
`border-width`, `border-radius`, `width`, `height`, `padding`, `margin`, `gap`,
`font-size`, `transform`, the insets (`top`/`right`/`bottom`/`left`, which
animate together), and on a `<path>`: `fill`, `stroke`, `stroke-width` and `d`,
the geometry itself. Naming anything else warns and lists what is animatable,
rather than leaving you with an element that silently never moves. A longhand of
an animatable shorthand (`padding-left`) is pointed at the shorthand: the four
sides animate as a unit.

Three limits, all of them deliberate:
- **A value that has no midpoint jumps.** `10px` → `50%` needs a layout to
  resolve, and a colour becoming a gradient has no halfway. Same-unit lengths
  interpolate; anything else lands at once.
- **Enter and leave are opt-in.** Unmarked, a node still arrives at its
  authored style and a node the build stops reaching is simply gone. Add
  `r-transition` to animate the way in and out; see below.
- **`transition` does not inherit**, exactly as in CSS. A parent's `color`
  change does not animate a child's text; put the transition on the element
  whose style is moving.

An app with no transitions running is still fully event-driven: it sleeps
waiting for real events and renders nothing. Frames are scheduled only while
something is actually in flight, and stop the frame it lands. Driven in
`examples/transition.rux`.

**Enter and leave:** `r-transition` on an element with `r-if` (or on a keyed
`r-for` row) says that its arrival and departure are animated. What the two
sides look like is CSS, on `:enter-from` and `:leave-to`; how long the swap
lasts is the element's own `transition`.
```rux
<view class="panel" r-if="open" r-transition>…</view>
```
```css
.panel { opacity: 1; transform: translateY(0px);
         transition: opacity 300ms ease-out, transform 300ms ease-out; }
.panel:enter-from { opacity: 0; transform: translateY(-16px); }
.panel:leave-to   { opacity: 0; transform: translateY(-16px); }
```
The two sides are ordinary rules and the walk between them is the same
machinery a `:hover` uses. `:enter-from` is worn for one frame and dropped by
the next, which is what turns an arrival into an ordinary style change;
`:leave-to` is held from the moment the swap opens until it commits.

While a swap is pending **both branches are really there**: laid out, styled,
and still updating. That is why the condition changing back mid-swap reverses
the swap instead of stacking a second one, and why a departing component's
`unmounted` fires when the swap **commits** rather than when it starts. A
cancelled swap never fired one.
