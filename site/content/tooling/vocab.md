+++
title = "rux vocab"
description = "Print the elements, attributes, directives and honored CSS the runtime understands, as JSON, for editors."
weight = 5
+++

```bash
rux vocab
```

JSON on stdout: elements, their attributes, the directives, the script globals,
the tags that never nest, and every CSS property the runtime honors.

It exists for editors. The [VS Code extension](@/tooling/editor.md) offers its
completions from this.

## Why it is a command and not a document

The lists that a crate already owns are **read from that crate** rather than
copied into the output:

- The CSS properties are the same slice the unhonored-property warning
  consults.
- The tags that never nest are the same list the formatter indents by.

So the guarantee an editor can make is that **if it offers something, it
works**. A completion list built from a second, hand-kept copy would eventually
suggest a property the runtime then warns does nothing, which is worse than
offering no completions at all.

It also means a property honored in a later release reaches completions without
anyone remembering to update a second list.

## The drift this prevents already happened

The extension used to carry its own copy of the tags that never nest. It
inherited HTML's set, which has `img` and not Rux's `<image>`, and
over-indented everything after an `<image src="…">` for two releases before
anyone noticed.

The extension ships a generated copy of this output so completions work before
`cargo install ruxlang` has finished, and prefers whatever the installed binary
says when there is one. A release gate regenerates the committed copy and fails
if it differs.

## Shape

```json
{
  "version": "0.7.0-dev",
  "elements":         [{ "name": "view", "detail": "…", "doc": "…" }],
  "globalAttributes": [{ "name": "class", "detail": "…", "doc": "…" }],
  "directives":       [{ "name": "r-for", "detail": "…", "doc": "…" }],
  "elementAttributes": { "image": [{ "name": "src", "detail": "…", "doc": "…" }] },
  "scriptGlobals":    [{ "name": "signal", "detail": "…", "doc": "…" }],
  "voidTags":         ["image", "input", "…"],
  "cssProperties":    ["display", "width", "…"]
}
```

The element and attribute tables are declared by the command itself, because no
single crate owns them today: tags are strings all the way through the parser.
A test pins them against the reference, so a tag offered as a completion has to
be one the documentation describes.
