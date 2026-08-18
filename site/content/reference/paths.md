+++
title = "Paths"
description = "SVG path data as an element: the d attribute, paint as CSS, and shapes that morph."
weight = 16
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->

A leaf that draws Bézier geometry, so a Rux app is not limited to boxes and
images. The renderer was always past that ceiling; until v0.7 the language was
not.
```rux
<path class="wave" d="M 0 60 C 40 0, 80 120, 120 60" />
```
```css
.wave { stroke: #89b4fa; stroke-width: 4px; stroke-linecap: round; fill: none; }
```
**`d` is the full SVG path grammar**: `M L H V C S Q T A Z`, absolute or
relative, in the element's own coordinates. Path data pasted from a design tool
works, arcs included. `:d` binds an expression, which is what a chart drawn from
a signal uses.

**Geometry is an attribute and paint is CSS**, and the split is the design
rather than an accident. Paint belongs in the cascade because that is where
every other appearance in Rux is written, so `fill` gets `:hover`, `:class`,
`:enter-from` and `transition` for nothing. Geometry belongs in an attribute
because a data-driven path is computed per row, and the cascade is the wrong
place for a value that changes with the data.

| Property | |
|---|---|
| `fill` | the colour inside. Defaults to **opaque black**, as SVG does, so a path with geometry and no paint still draws. `fill: none` turns it off |
| `fill-rule` | `nonzero` (default) or `evenodd`, for a path that overlaps itself |
| `stroke` | the outline colour. Defaults to **none** |
| `stroke-width` | px. A width of `0` draws no outline, the way a zero-width border is no border |
| `stroke-linecap` | `butt` (default), `round`, `square` |
| `stroke-linejoin` | `miter` (default), `round`, `bevel` |

The fill is painted first and the stroke over it, which is SVG's order and the
one that looks right: a stroke is a border on the shape and belongs over its own
fill.

**With no CSS size a path lays out at the size of its own geometry**, so pasting
path data and seeing it needs no box to be written. Naming a `width` does not
rescale the drawing, it changes the box the drawing sits in; scaling is
`transform`, which every other element already uses for the same thing. There is
no `viewBox`: it is a second coordinate system to learn, and it can be added
later without breaking anything, which is not true in the other direction.

**Shapes morph.** `transition: d` animates the geometry itself, and the rule is
the one every other animatable value follows: **two paths with the same sequence
of commands interpolate, and anything else jumps**. That is why the parser
normalises everything to moves, cubics and closes, straight lines included: a
square written with four sides and a circle written with four arcs have the same
sequence, so one becomes the other. Nothing is resampled and no correspondence
is guessed, because guessing produces a fold as often as a morph, and writing
two shapes with matching commands is the discipline every morphing tool already
imposes. `fill`, `stroke` and `stroke-width` animate as the ordinary colours and
lengths they are.

`alt=` describes the drawing to the accessibility tree. **Without it a path is
treated as decoration** and left out, which is right far more often than
announcing an unnamed graphic.

Malformed path data keeps whatever parsed before the problem, which is what SVG
itself specifies and what shows an author where the typo is. An empty box shows
nothing at all.

Driven in `examples/chart.rux` and `examples/morph.rux`.
