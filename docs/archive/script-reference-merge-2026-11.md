# Script reference merge (2026-11-12)

`docs/07-script.md`, `docs/10-types.md` and `docs/11-next.md` became one
document, `docs/07-script.md`. The two retired files are kept here whole as
`10-types.md` and `11-next.md`. The 11-11 audit (section 3, item 2) asked for it,
and the owner chose one reference. This file says where every part went, so the
merge can be checked.

The text was assembled from the three files as they stood at `2b70bfb` by a
script (slices by line number), then read through. Where two sources said the
same thing, the fuller or more recently verified one was kept.

## Where each part went

| From | Section | Now |
|---|---|---|
| 07 intro, 11 intro and Not built | the introduction | merged; Principles from 11 kept, with the old engine's spellings pointed at Retired names |
| 07 The three places script runs | same | kept, plus 11's sentence on the grammar |
| 11 Lexical structure | Lexical structure | kept; 07's single-quote and map-block examples added; 07 Reserved words folded into Keywords |
| 10 The shape of it, Where an annotation goes, The types, Types every file can name | Types | kept as Type annotations, Where an annotation goes, The types, Types every file can name |
| 11 The shape of it, The types, Declaring a type, Numbers, The empty value, Checked not erased, Result | Types | kept; the types table, Numbers, The empty value, Checked not erased, Declaring a type and Result are 11's |
| 07 Type annotations | Types | dropped: its example and rules are in 10's and 11's sections; its `is` and props paragraph is 10's Boundaries |
| 10 Inference, Optional fields, Unions and narrowing, `switch`, Boundaries, Sharing types, errors and warnings | Types | kept (10's, one level deeper) |
| 11 Inference, Optional fields and narrowing, `switch`, `is` | Types | dropped: 10's sections say the same in full; 11's `switch` arm rules added to 10's |
| 11 Statements and expressions | Statements and expressions | kept |
| 07 Values, Truthiness, Operators | Statements and expressions, Lexical structure | dropped: 11's sections cover them; 07's examples moved |
| 07 Functions; 10 Functions; 11 Functions | Functions | merged |
| 10 A typed function cannot read its caller's locals | Functions | kept |
| 07 State, Lifecycle, Intervals; 11 State | State, Lifecycle, Intervals | 07's kept; 11's top-level `let` rule added |
| 10 Templates, Props; 11 Templates | Templates, Props | 10's kept; 11's section was a subset |
| 07 Async, Cancelling; 11 Async | Async | 07's kept; 11's "async fn may call another" bullet added |
| 07 Modules; 11 Modules | Modules | 07's kept; 11's import rules, `export let`, "scoped" Not built and Values move by copy added |
| 07 Native code; 11 Native code | Native code | 07's kept; 11's What crosses, `rux::Any`, `#[rux::init]`, the interface listing and the generated crate name added |
| 07 Built-in methods, Retired names; 11 Collections | Built-in methods | 07's kept; 11's map rules and Not built list added; 11's "Everything else global" table dropped (each row is documented where its call lives) |
| 07 closing the app, windows, keys, pointer, drag and drop, events, files, sounds, settings, clipboard, the tree | same | unchanged |
| 07 Errors, Deliberately absent | same | kept |
| 07 Reserved words | Keywords | folded into Lexical structure |
| 11 Security | Security | kept |
| 07 and 11 If you know JavaScript | same | merged into one list |
| 10 Editor | Editor and checking | kept; "How it is built" dropped (the pipeline is in `04-architecture.md`) |
| 11 Architecture, The pipeline | `04-architecture.md`, "The script pipeline" | moved |
| 11 Build order | dropped | the diary is `language-build-log.md` |
| 11 Decisions that still define the language | dissolved | each rule is stated where it applies; the two that were nowhere (`native/` never embedded and `CARGO_TARGET_DIR`; compiled code never replaces the interpreter) went to Native code and to `04-architecture.md` |

## Not carried over

- 10 "How it is built" paragraph (the crate names are in `04-architecture.md`).
- 11 "Everything else global" table, and the 11 Collections tables of array
  methods (07's list is the one checked against the interpreter).
- 11 Kept/Changed/New columns against rhai, and the "If you know the fork" list:
  already in `language-build-log.md`.
