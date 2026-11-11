# Docs and site audit, 2026-11-11

Input for the site redesign. Builds on `docs-audit-2026-11-06.md`, which checked
every file in `docs/` against the code; this pass covers what that one left out:
the 40 commits since it, the hand-written site pages (`site/content/` pages with
no `GENERATED FROM` header), the CLI's own help text, and the playground's move to
`playground.ruxlang.dev`. Everything marked **probed** was run with the installed
`rux 0.9.0-dev`; the rest was read from code.

## Fixed on `build/v0.9`, 2026-11-11

Owner's decisions first: docs are versioned, public docs describe only their own
release, and the playground keeps running the latest release. So these fixes are
v0.9 docs; `release/v0.8.0` carries the same stale Learn, home, how-it-works and
why text and needs its own pass.

- Home: the platform note, the feature chips, and the gap paragraph (now the
  known-gaps list of 05).
- Learn 3, 4, 5, its index, `examples/learn/04-tasks.rux`, `05-components.rux`,
  `components/row.rux`: no rhai; `{ }`, `t => ...`, `.length`; `r-for="(t, i)"`
  with a `toggle(i)` function; closures capture; typed props; the v0.7 edges list
  replaced by a pointer to 05's known gaps; "Persist it" names `settings` and
  `native/`. Checked at run time first: writing to the loop variable is still a
  no-op, so chapter 4's snapshot lesson stays. `r-show` keeps its space (code:
  `rux-style/src/lib.rs` "keeps the layout slot").
- `crates/rux-runtime/tests/learn.rs` asserts the new claims. The old
  "closure in a method cannot capture" test passed only because `|n|` no longer
  parsed.
- Tooling: `new` (tree with `stores/` and `about.rux`), `check` (handlers are
  type-checked and errors; `--native`, `--types`, `window.open`), `run`
  (`--device`, `--no-watch`, `--serial`, `[window]`), the index (`rux build`,
  `rux native`), `build` (a release compiles the script to Rust; `RUX_AOT=0`), and a
  new `tooling/native.md` (the CLI test `every_subcommand_has_a_tooling_page`
  requires it once `rux native` is in the usage).
- `how-it-works` rewritten from `04-architecture` (patching, not rebuilding; the
  overlay exists; 14 more crates; no rhai). `why`: no rhai, the element set, the
  "when not to use Rux" limits.
- 05: drag and drop is out of known gaps (system file drop is in), `:dragging` and
  `:drag-over` are in the pseudo-classes, the scaffold tree, no dated icon note.
  `docs/README.md` rows for 03, 07, 10, 11. `sync-docs.sh` description.
- `rux --help`: `stores/`, `--router`/`--no-router`, `rux native`, what a bare
  `rux check` reads. Completions (`vocab.rs`): `map(x => ...)` and the rest, no rhai
  in `trim`. The scaffold's `hello.rs` names `stores/greeter.rux`.
- The message-list recipe sends on Enter (`@key` on the field; test
  `the_message_list_recipe_sends_on_enter`). Not driven in a window.
- Found while verifying: **the site did not build at HEAD** (7 dead anchors from
  05's `(#forms)`-style links landing on a different split page). Fixed in
  `sync-docs.sh` (`cross_page_anchors`, one more apostrophe slug). The generated
  CSS properties page and `vocabulary.json` were behind the code (no `:dragging`,
  `:drag-over`); regenerated.

Gates passed: `zola build`, `check-links.py` (64 pages), `check-scripts.py`
(63 scripts), the three `--check` syncs, `cargo test -p ruxlang -p rux-style
-p rux-syntax`, and the runtime's `examples`, `learn`, `keys`, `list_rows` tests.

## 1. Stated in the docs, no longer true

### Site home page (`site/content/_index.md`, hand-written)

| Line | Says | Now |
|---|---|---|
| 11 | "desktop today, mobile next" | Android builds, installs and hot reloads (`rux run --device`); web builds too |
| 149 | biggest gap is inline text flow: two `<text>` cannot share a line | A `<text>` inside a `<text>` is part of the same sentence (vocab, 05 Elements) |
| 150 | `r-for` gives no index | `r-for="(item, i) in items"` (05 line 1605) |
| 151 | a closure passed to a method cannot capture | Removed with the move off rhai (05, 11-next) |
| 152 | no promises, no async anything | `async fn`, `await`, cancelling (07 "Async") |
| 152 | router is desktop-only until `rux build` bundles components for the web | `rux build --target web` loads the whole project, routed pages included (tooling/build) |
| 153 | no word-wise movement or triple-click | Both built (05 lines 2935 to 2939) |
| chips | "What works today" list | Missing everything since v0.7: Android, web build, `<icon>`, `<audio>`, `<video>`, file input, typed script, async, modules and stores, native Rust, drag and drop, child windows, title-bar overlay, forms and validation |

### Learn (`site/content/learn/`, hand-written): the whole tutorial predates the v0.8 language

- `03-state.md:7,31,110,131` and `04-lists-and-input.md:34,100,137,140,147`: the
  script "is [rhai]", "rhai's map literal `#{ }`", "rhai's rule", a link to rhai.rs.
  Rux runs its own language; `#{` is accepted and `rux fmt` rewrites it to `{`.
- Snippets use `|t| ...` closures, `.len()`, `.push(#{...})`, and the
  `for i in 0..items.len()` index loop (also in `learn/_index.md:62`). The language's
  forms are `t => ...`, `.length`, `{...}` and `r-for="(t, i) in items"`.
  `.len()` still runs (probed: `[1,2].len()` checks clean), only because rhai-era
  names were never removed (see 06 audit). If they are removed, Learn and
  `examples/learn/04-tasks.rux`, `05-components.rux` (13 uses) break.
- `03-state.md:69-91`: "A closure passed to a method cannot see the surrounding
  scope" and why. No longer true.
- `05-components.md:122-140` "Where the edges are, as of v0.7": all four bullets
  false (inline text, `r-for` index, closure capture, async).
- `_index.md` "every snippet is checked by the test suite": true, but the suite
  checks that they load, not that they use current syntax.

### Tooling (`site/content/tooling/`, hand-written)

- `new.md:65-82` "What it writes": no `stores/greeter.rux` (in every minimal
  project since `198e07a`), no `pages/about.rux` (with the router), `home.rux` "calls
  Rust" (it goes through the store now), and `missing.rux` is listed without saying
  it is router-only. `docs/05-as-built.md:16-75` has the right tree; the site page is
  a second, hand-kept copy and drifted.
- `check.md:31-40` "Handlers are compiled at load ... one that will not compile is a
  warning ... It is syntax only." Handlers are now name- and type-checked, and both
  are errors (probed: `count = count + "x"; nope()` gives "calls `nope`, which does
  not exist" and "this is `string`, where `int` is expected").
- `check.md`: no `--native`, no `--types`, no `rux check` over `window.open` calls.
- `run.md:44-48` Options table: no `--device`, `--no-watch`, `--serial` (they are in
  `rux --help`), and no word of `[window]` applying to `rux run`.
- `_index.md:24-31` commands table: no `rux build` (it has a page), no `rux native`.
- `doctor.md` and the index say it checks "the Android toolchain"; true, but the
  roadmap's device survey is still listed as open, so the page should say it does
  not look at a device.

### Other site pages

- `how-it-works.md:39,47-48,112,157`: "rux-script (the rhai engine)", "`host::`
  registry", "State lives in rhai signals", rhai among reused crates. The crate
  table lists 9 crates; the workspace has 23 (`rux-ir`, `rux-syntax`, `rux-codegen`,
  `rux-native`, `rux-native-macros`, `rux-bindgen`, `rux-icons`, `rux-audio`,
  `rux-video`, `rux-web`, `rux-reactive`, `rux-fmt`, `rux-highlight`, `rux-aot-tests`
  missing). `docs/04-architecture.md` is current; this page should be generated from
  it or cut down to a diagram that points there.
- `why.md:81,146`: "[rhai] for script".
- `recipes/modal.md:80`: "no way to say tappable but not focusable". Not checked
  against `tabindex`/focus code in this pass; verify before keeping.
- `recipes/message-list.md:159`: a dated history note inside a recipe.

### Generated pages and their sources

- `docs/05-as-built.md:3938` Known gaps: "**Drag and drop** of elements ... not
  built". Built in `d5c141e` (MS-010) and documented in `07-script.md` "Drag and drop".
- `docs/05-as-built.md:1255` Pseudo-classes: no `:dragging` or `:drag-over`
  (`HONORED_PSEUDO_CLASSES`, `crates/rux-style/src/lib.rs:5653`). Drag and drop has
  no entry in 05's Elements or pointer sections at all; it lives only in 07.
- `site/sync-docs.sh:48` and `docs/README.md:53`: Script page described as "every
  way it differs from rhai".
- `docs/README.md:52,54,55`: `03-guide` called a tutorial (it is false as written);
  `10-types` "being built ... a status table says which parts run"; `11-next` "the
  language being built ... until the build lands". The build has landed.
- `docs/05-as-built.md` `<icon>`: "(Until 2026-11-07 the icon read its parent's
  `color` ...)". History inside the reference; belongs in the blog or changelog.
- `02-spec.md` and `03-guide.md` are still published (`/reference/spec`), the guide
  linked from generated pages via `rewrite_links`. Carried over from the 06 audit.

### CLI text (`crates/rux-cli/src/main.rs:50`, feeds `rux --help`)

- `rux new` line lists "app.rux, pages/, components/, native/, assets/": no `stores/`.
- `--router` / `--no-router` are parsed (`new.rs:1303`) and missing from the usage.
- `rux new --help`, `rux build --help` are rejected; `rux check --help`,
  `rux fmt --help` print the whole usage; `rux run --help` tries to run and fails
  with "no app.rux". Probed. Only `rux --help` works.

## 2. Supported, and not documented (or documented only for contributors)

| Feature | Where it is | Where it should be |
|---|---|---|
| `rux native [dir]`: prints the native interface in Rux syntax | `rux-cli/src/native.rs:435`; one clause in 07 and tooling/new | `rux --help`, the tooling index, a section on the native page |
| `rux vocab --css-reference` | `main.rs:162`; used by `scripts/sync-vocabulary.sh` | tooling/vocab (it is the generator of `/reference/css-properties`) |
| A release build compiles the entry script to Rust (`rux-codegen`), `RUX_AOT=0` turns it off | `rux-cli/src/aot.rs`; only in `11-next.md:926` | tooling/build "Dev builds and release builds differ" (it currently says the only difference is embedding) |
| Diagnostic env vars: `RUX_TRACE`, `RUX_IR_PRINT`, `RUX_IR_COVERAGE`, `RUX_AOT_REPORT`, `RUX_FLAT` | `rux-runtime`, `rux-script` | a "Debugging" section in Contribute (author-facing ones in tooling/run) |
| Rhai-era methods: `len`, `retain`, `drain`, `dedup`, `parse_int`, snake_case string methods | interpreter | decide: document or remove (open since 11-06) |
| `[1,2].for_each(...)` passes `rux check`, fails at run time | checker vs interpreter | a code fix, not a doc (open since 11-06) |
| `rux check` reads every `.rux` in the project, each in its own context | `d4699da` | in check.md already; missing from `rux --help` ("Check documents") |
| A dev build with `native/` embeds the icon set (MS-014) | `3a52a30` | only in the field report; one line in 05 `<icon>` |
| The playground's `#<base64>` deep link | `site/templates/playground.html:152` | it becomes a cross-site contract once the playground moves; write it down |

The pointer, window, child-window, drag-and-drop, `scrollBy`, elliptical radius,
icon colour and title-bar-overlay work since 11-06 is documented in `07-script` and
`05-as-built`. What is missing is cross-linking: none of it reaches the hand-written
Learn, Tooling or home pages.

## 3. The structural problems behind the drift

1. **Two kinds of page, one of them unchecked.** Generated pages come from `docs/`
   and `sync-docs.sh --check` catches drift. The hand-written pages (home, Learn,
   Recipes, Tooling, why, how-it-works) have no check, and they are exactly where
   every false statement above sits. Tooling repeats 05's "Starting a project" to
   "Telling an editor" sections by hand, and the site strips those sections from
   the generated reference, so the two copies diverge silently.
2. **Three script references.** `07-script` (1,748 lines), `10-types` (612) and
   `11-next` (1,005) overlap on types, modules, async, native and operators. Only
   07 is published; 10 and 11 are linked to GitHub.
3. **05-as-built is 3,967 lines and mixes jobs**: CLI, crate list, element
   reference, CSS reference, reactivity, inputs, routing, accessibility, gaps. The
   site already slices it into 15 pages, but by heading, so the slices inherit the
   mixing (Reactivity lives under "What works", drag and drop lives in Script).
4. **The playground runs v0.7.1; every doc describes the tip.** `site.yml` builds the
   playground `--from-tag`, and the last tag is v0.7.1 (v0.8 is drafted, not
   released). So a "Try it" on a Learn chapter runs, but a Try it on anything in
   Reference that uses v0.8 syntax fails. On top of that, the wasm32 build fails at
   HEAD (HANDOFF: `window_settings` in `overlay_active`, `slides` taking
   `std::time::Instant`), so the tip cannot be put in a playground at all yet.
5. **History inside reference pages.** Dated "until 2026-..." notes, "this used to
   be worse", "as of v0.7". They belong in the blog; reference pages should state
   what is true.

## 4. Proposed structure

Organised by what the reader is trying to do. Every page is generated from `docs/`
or checked against it; nothing hand-kept repeats a fact owned elsewhere.

```
ruxlang.dev
  Home              one-file example, a feature grid linked to reference pages,
                    "what is not built" pulled from 05 Known gaps
  Get started       install, first window, rux new, project layout
  Learn             the tutorial, rewritten in v0.8 syntax (5 chapters stay)
  Guides            task pages: layout, state and stores, lists, forms and
                    validation, components, routing, windows (desktop window,
                    child windows, title bar), drag and drop, media (image,
                    icon, audio, video), files and clipboard, native Rust,
                    Tailwind, accessibility
                    (Recipes fold in here: message list, modal, tab bar)
  Reference         elements; attributes and events (from vocab, generated);
                    CSS: properties (generated), selectors and pseudo-classes,
                    differences from CSS; script language (one page set from
                    07 + 10 + 11: syntax, types, modules and async, built-ins,
                    element API); rux.toml (every key, one page);
                    CLI (every command and flag, generated from main.rs USAGE);
                    errors and the overlay
  Platforms         desktop (Windows, macOS, Linux status), Android, web, iOS
                    (not built), with a support matrix per feature
  Contribute        architecture, user tests, author notes, debugging env vars
  Blog              releases; all "used to" history moves here
  Archive           rationale, v0.1 spec, old guide: linked from Contribute,
                    out of the main nav
playground.ruxlang.dev
```

Concrete steps, in order:

1. Fix the false statements in section 1 (small, mechanical, no design decision).
2. Owner decisions: one script reference or three; remove or document rhai-era
   names (Learn depends on the answer); whether docs are versioned (tip and latest
   release) or show "since v0.x" badges.
3. Move tooling content into `docs/` (05's "Starting a project" through "Telling
   an editor" are already the source) and generate the Tooling pages, like the
   reference ones. Generate the CLI reference from `USAGE` and the attributes page
   from `rux vocab`, the way css-properties already is.
4. Split 05 by job (elements, CSS, interaction, routing, platform) as the site
   slices it now, and give drag and drop, windows and native their own pages.
5. A check for hand-written pages: run every ```rux fence through `rux check`
   (`site/check-scripts.py` exists; extend it) and grep for `rhai`, `#{`, `.len()`.

Ideas for making it more useful, for the owner to pick from:

- A per-feature platform matrix (desktop, Android, web, macOS, Linux; driven or
  not), sourced from `08-user-tests.md`, which already records it.
- "Try it" on every reference snippet with a `<template>`, once the playground
  runs the documented version.
- Search across all docs, not only the docs sections; show the version on every page.
- Copyable `rux new` and a short screen recording of hot reload on the home page.
- A light theme (`config.toml` says dark-only is deliberate; a choice for the owner).

## 5. Moving the playground to `playground.ruxlang.dev`

What has to change, from the code:

- **Hosting.** GitHub Pages serves one custom domain per repository, and
  `site/static/CNAME` is `ruxlang.dev`. The playground needs its own site: a second
  repo with Pages, or Cloudflare Pages (DNS is already at Cloudflare).
- **Build.** `site.yml`'s wasm steps (`build-playground.sh --from-tag`,
  `site/static/wasm/`) move to the playground's deploy. The main site then no longer
  needs `crates/**`, `Cargo.toml` and `Cargo.lock` as deploy triggers, so a docs
  push stops rebuilding a 7 MB wasm module.
- **Links.** `site/templates/base.html:38` (nav) and `:67` (`PLAYGROUND`, used by
  every Try it button), `site/content/learn/_index.md` (`@/playground.md`), and the
  blog's `/playground/` links (`v0-4-0.md:64` and others). Put the URL in
  `config.toml [extra]` once. Keep `/playground/` as a redirect page so old links
  and shared links still work, passing the `#` fragment along.
- **The deep link becomes an interface.** Try it hands source over as
  `#<base64>` (`playground.html:152`). It must keep working across both sites and
  across playground versions; document it and do not change its encoding.
- **Security gets better.** `playground.html:98` notes shared code runs on the
  `ruxlang.dev` origin (watchlist #31). A subdomain is its own origin, which is most
  of what the planned sandboxed iframe was for.
- **Shared assets.** The playground uses `base.html`, `style.css` and the TextMate
  grammar in `site/syntaxes/`; decide whether the new site copies them or fetches
  them from `ruxlang.dev`.
- **Version.** Decide what it runs. Today it is the latest tag (v0.7.1). With its
  own deploy it could offer both: the release by default and the tip on a second
  path. The tip needs the wasm32 build fixed first.
