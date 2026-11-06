# Handoff

Read this first. Keep it short; history goes to an archive, not here.
Last updated 2026-11-06.

## Objective

Ship v0.9 from `build/v0.9`. The line is nearly all built (see the v0.9 section
of [the roadmap](./06-roadmap.md)); what is left is listed there.

## State

- Branch `build/v0.9`, **local only, not pushed** (the owner pushes; a push runs CI).
- 2026-11-06: `build/v0.9-icons` was found never to have been merged, and is now
  in the tip (merge commit `20939f2`). `<icon>`, `rux-icons` and the build's
  icon tree-shaking are in. Two fixes were needed to merge it: `<icon>` had no
  entry in the parser's attribute tables, and its block inside
  `build_node_inner` overflowed the debug main-thread stack, so it is now
  `build_icon` in `rux-style`. All 138 test suites pass on the merged tree
  (`cargo test --workspace -j 2`; at full parallelism the machine runs out of
  memory compiling test files).
- Deleted locally on 2026-11-06 as superseded: `wip/g4-kept-tree` (landed as
  `812f71e`) and `site/launch` (the site sync has moved far past it). The
  remote `origin/site/launch` was left alone. `spike/c0-frames` is kept on
  purpose: a measuring harness, results recorded in `docs/13`.
- `build/v0.9-icons` and its worktree `../rux-icons` are fully merged and can go.
- Docs corrected to the code on 2026-11-06 (every doc was checked against the
  crates and `rux check` probes, about 350 commits of changes since v0.7.1
  considered). Rewritten: `05-as-built`, `07-script`, `10-types`, `11-next`
  (2,900 to 1,000 lines), `04-architecture`, and `06-roadmap` (3,300 to 211
  lines: open work and the decisions that still bind only). Removed text lives in
  `docs/archive/` (one file per doc; `roadmap-history.md` holds the whole old
  roadmap). Not reviewed in this pass: `02-spec`, `03-guide` (both known to be
  history, still published to the site), `08-user-tests` (a log), `09-author-notes`
  (19 open rows, statuses not re-checked), field reports.
- The full audit record (method, what was left unverified per doc, questions
  for the owner) is `docs/archive/docs-audit-2026-11-06.md`. Read it before
  auditing the docs again.
- The owner will decide the docs and site redesign. Open questions for it:
  `07-script`, `10-types` and `11-next` overlap heavily and could become one
  document; much of the CSS section of `05-as-built` is not CSS (r-transition,
  computed/effect, keyed lists, Tailwind, `env()`); `03-guide` is false as
  written; `02-spec` is design history.

## Decisions not to revisit

- `native/` is on by default for `rux new`, beside `rux.toml`; locks are per
  target (`locks/<target>.lock`); the Rux app stays at the project root.
- Superseded branches above stay deleted; do not resurrect them.

## Known issues and open work

- Still open in v0.9: physics (springs), live regions, pinch and rotate, the
  `rux doctor` device survey (those two need a phone), `hyphens: auto`, video on
  macOS and Linux.
- The 0.9 publish must include `rux-native`, `rux-native-macros`, `rux-bindgen`
  (and `rux-icons`, already in `scripts/release.sh`'s first layer); crates.io has
  `ruxlang` only up to 0.7.1, so a scaffolded `native/` crate cannot build for
  anyone outside a checkout until then.
- `main` has diverged from `build/v0.9`: it holds the v0.7.0 and v0.7.1 release
  commits, which `build/v0.9` does not descend from. A release-process question.
- `cargo clippy -D warnings` fails in `rux-syntax` (four needless borrows); not
  touched yet.
- Mava Studio reports to attend to are in `docs/field-reports/mava-studio-2.md`
  (it had uncommitted edits on 2026-11-06). Start a new session for them.
- Android and web builds of a scaffold with `native/` have not been driven.
- Code defects found by the docs pass (none fixed yet):
  - `throw "x"` is caught as the string, not as an `Error`; `do`, `break value`
    and the bitwise operators are silently inert in a `<script>` body; `trunc`
    of NaN does not throw; an `any` is not checked where it enters a typed name.
  - `[1,2].for_each(...)` passes `rux check` but the interpreter has no
    `for_each`. Rhai-era names (`len`, `retain`, `drain`, `dedup`, `parse_int`,
    snake_case string methods) still run and are undocumented: document or remove.
  - Stale text in code: `vocab.rs` says a component declaring `mounted` is
    warned (it runs per instance); the hover text for `border`, `outline` and
    `position` is out of date (that text feeds the generated css-properties page);
    the hover text for `trim` mentions rhai; `rux-ir/src/types.rs` says `Set` and
    non-string `Map` keys "come with Rux's own interpreter"; `lexer.rs` and many
    `crates/*/src/lib.rs` module docs still cite the fork or "milestones M2 to M9".
  - `rux check` warns that `font-size: calc(100% + 2px)` is ignored while a unit
    test says it resolves; `transition: opacity 1s linear(0,1)` is reported
    badly (calls `opacity` a function); `rux new --help` is rejected.
  - Two doc statements rest on one grep only: slider arrow keys, a drawn desktop
    date picker and dropping a file on the input are listed as not built.

## Next action

Attend to the Mava Studio reports in a new session. After that, decide with the
owner when to push `build/v0.9` and cut v0.9.

## Verified on 2026-11-06

Trial merge built and tested in a scratch worktree; `rux new` + `rux check` on
the `tasks` template passes after the stack fix; the roadmap was checked against
`git log` since the v0.8 freeze. An `<icon>` has not been driven in a window
since the merge.
