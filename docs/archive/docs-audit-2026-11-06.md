# Docs audit, 2026-11-06

What the docs pass did, what it could not verify, and what is left. Read this
before auditing the docs again, so the same ground is not covered twice.

## Method

1. Ledger: `git log --no-merges --format='%h %ad %s' --date=short v0.7.1..HEAD -- crates editors`
   gives about 350 code commits. 207 touched a reference doc; the 146 that did
   not were the candidates for omissions (most were internal fixes).
2. One agent per file or section range, all working to one spec (verify against
   code, `rux check` probes and tests; remove history; add what is missing; keep
   the headings `site/sync-docs.sh` extracts; no em dashes; no first person).
3. Gates: the vocab coverage tests (`cargo test -p ruxlang -p rux-style -p rux-syntax -j 2`),
   `bash site/sync-docs.sh --check`, `bash scripts/sync-vocabulary.sh --check`,
   and a scan for em dashes. All passed on 2026-11-06 (commit `3cbc10e`).

Removed text is in this folder, one file per doc: `as-built-A.md` (intro through
"Where Rux differs from CSS"), `as-built-B.md` (Honored CSS through anchor
positioning), `as-built-C.md` (reactivity through scrolling), `as-built-D.md`
(components to the end), `script.md`, `types.md`, `language-build-log.md`
(from `11-next.md`), `architecture.md`, and `roadmap-history.md` (the whole old
roadmap, including items that are still open; the live roadmap wins).

## Not reviewed in this pass

`02-spec.md`, `03-guide.md` (false as written, still published), `08-user-tests.md`
(a log), `09-author-notes.md` (19 open rows, statuses not re-checked), and
`field-reports/`. `01-rationale.md` got only its rhai statements fixed.

## Claims left unverified, by doc

These stand as written because they could not be checked cheaply. Verify before
relying on them.

- **05 intro to Differs from CSS:** 1,054 filled icons (5,148 outline confirmed);
  `<video>`/`<audio>` timings; `loading="lazy"` at 1250 px; whether `<icon>` sits
  inside a text line like `<image>`; the `use components::message_card;` to
  `message-card.rux` mapping (inferred from the scaffold); that `rux build --release`
  compiles the entry script to Rust (left out).
- **05 Honored CSS to anchor:** `font-size` percentages (a unit test says
  `calc(50% + 2px)` resolves, `rux check` warns it is ignored); whether clip-path
  and mask make stacking contexts (assumed yes); `type=select` in a keyed list;
  Tailwind CLI caching details; the "not yet for a sentence with runs" notes.
- **05 reactivity to scrolling:** text input and composition internals
  (`set_ime_allowed` and the rest); the number locale table; Android file-input,
  camera and text-menu details; "not built" for slider arrow keys, a drawn desktop
  date picker and dropping a file onto the input (grep only).
- **05 components to end:** Android Back and deep-link behaviour (from code and
  commit messages, not driven); the removal of the method-style `fn` scope gotcha
  rests on `11-next.md`; assigning an undeclared document name in a component
  handler passes `rux check`, so only reads are documented as an error.
- **07-script:** settings file paths; clipboard and web behaviour; Android audio
  focus; "one kind of number at the tree boundary"; the `rux fmt` rewrites of
  `number` and `#{` (`#{` confirmed 2026-11-06).
- **10-types:** that assigning an `any` to a typed name is unchecked at run time;
  that "anything class-shaped" is an error; that a `use type` import of a
  non-exported type warns.
- **11-next:** the negative `**` runtime failure; the `unmounted` hook order
  claims; the `#[rux(name)]` override (only seen parsed).
- **06-roadmap:** "controlled state is opt-in" is listed as a binding decision but
  not checked against the code; `setTimeout` is listed as declined only because
  `07-script.md` never mentions it; whether a `rux test` command exists (the v1.0
  "test story for app authors" item stands).
- **04-architecture:** the `ruxlang` entry-point wording is from the manifests; the
  accesskit tree being built from layout's accessibility nodes is from crate docs.

## Questions for the owner (docs and site redesign)

- `07-script.md`, `10-types.md` and `11-next.md` overlap on types, modules, async,
  native, operators and built-ins. `11-next.md` says the other two "retire into
  history" when the build finishes; the build has, and they are the maintained
  references. One document, or three with clear jobs?
- Much of 05's CSS section is not CSS: `r-transition`, computed and effect, keyed
  lists, shared styles, Tailwind, `env()`. It wants a better home.
- `03-guide.md` is false as written and still published; `02-spec.md` is design
  history and still published as `/reference/spec`.
- Rhai-era names still run and are undocumented (`len`, `retain`, `drain`, `dedup`,
  `parse_int`, snake_case string methods): document or remove.
- `docs/field-reports/mava-studio-2.md` line 92 still says icons are "decided and
  unscheduled"; the file has uncommitted edits from the Mava session, so it was left.
- `crates/*/src/lib.rs` module docs and `lexer.rs` comments still cite the fork or
  "milestones M2 to M9".

## Code defects found

Listed in `docs/HANDOFF.md` under Known issues. None fixed.
