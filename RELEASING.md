# Releasing Rux

Every release ships with a blog post. **No post, no tag**. The post is not
paperwork after the fact, it's part of the release, and writing it is often where
you notice the release isn't actually ready.

This file is the checklist. It's short on purpose.

## The rule

> A release is a version tag, a GitHub Release, **and** a blog post at
> `site/content/blog/`. All three, or none.

The blog is not a changelog. A changelog says *what changed*; a Rux release post
says **what landed, what it cost, and what's still broken**, in that order, in
prose. The two existing posts (`v0-1-0.md`, `v0-2-0.md`) are the reference for
tone and length. If you're tempted to write "various fixes and improvements,"
stop: that sentence means you haven't found the story yet.

Two things every post must contain, because they're the two things this project
keeps learning:

1. **The bug you only found by looking.** Every release so far had one: a defect
   the test suite was green through, obvious within seconds in the window. Name
   it. It's the most useful paragraph for the next person (usually you).
2. **What the release is *not*.** The honest limits, as plainly as the wins. This
   is the project's whole credibility; don't spend it.

## Checklist

### 1. Land the work

- [ ] Every feature has been **driven in the window**, not just tested. This is
      the standing rule; see [`docs/06-roadmap.md`](docs/06-roadmap.md). Tests
      protect against regression; they don't tell you it works.
- [ ] `cargo test` is green, and the count in the post matches reality.
- [ ] `cargo build` is warning-clean.
- [ ] [`docs/05-as-built.md`](docs/05-as-built.md) reflects what now works, and
      its "known gaps" list is honest.
- [ ] [`docs/06-roadmap.md`](docs/06-roadmap.md) marks the shipped items done and
      keeps the restore-pass list current (v0.3 deletes from it).

### 2. Write the post

- [ ] Copy [`site/content/blog/_template.md`](site/content/blog/_template.md) to
      `site/content/blog/vX-Y-Z.md` (dashes, not dots, in the filename).
- [ ] Fill the front matter: `title`, `description`, `date` (real date),
      `extra.version = "vX.Y.Z"`.
- [ ] Write it: **what landed → what it cost → what's still broken.** Include the
      look-only bug and the "what it's not" section.
- [ ] Refresh screenshots if the UI changed. Drive
      `examples/showcase.rux`, capture the window, and update
      `examples/assets/showcase.png` + `site/static/showcase.png` together.
- [ ] Build the site locally and **read the post in a browser**, same rule as the
      app: `cd site && zola serve`. Check the code blocks, tables, and links.

### 3. Update the front door

- [ ] Update the "What works today" chips on `site/content/_index.md` if the
      feature set changed.
- [ ] Update `README.md`'s "What works today" line to match.

### 4. Version

Between releases the workspace version carries a `-dev` suffix, `0.4.0-dev` is
the *line being built*, not a release. Releasing is what drops the suffix.

- [ ] Drop `-dev` from `version` in `[workspace.package]` (root `Cargo.toml`),
      so `X.Y.Z-dev` becomes `X.Y.Z`.
- [ ] Drop it from every `rux-*` entry in `[workspace.dependencies]`, in the same
      file. These must match the workspace version **exactly**: a plain `X.Y.Z`
      requirement does not match the prerelease `X.Y.Z-dev`, so changing one
      without the other breaks the build.
      Grep: `grep -n 'rux-.* = { version' Cargo.toml`.
      Members inherit with `rux-x.workspace = true` and need no edit; the only
      one without a version is `rux-highlight`, which is never published.
- [ ] `cargo build` passes after the change.
- [ ] After tagging, open the next line: set both back to `X.Y.(Z+1)-dev`.

### 5. Tag and publish

- [ ] Merge to `main`. The site deploys automatically
      ([`.github/workflows/site.yml`](.github/workflows/site.yml)).
- [ ] `git tag vX.Y.Z && git push --tags`.
- [ ] Cut the GitHub Release. Link the blog post; attach a release binary if the
      platform matrix changed (`cargo build --release -p ruxlang`).
- [ ] Confirm the post is live at `/blog/vX-Y-Z/` and the home page shows it.

### 6. crates.io (when publishing)

The CLI publishes as **`ruxlang`**, not `rux`, the bare name is held by an
unrelated abandoned crate. The binary is still `rux`, so `cargo install ruxlang`
gives you a `rux` command.

**Eleven crates publish.** `rux-web` and `rux-highlight` carry
`publish = false`: the first is a `cdylib` only the playground links, the second
is used by nothing else, and a crates.io name is held forever whether or not it
turns out to be wanted.

Versions on the intra-workspace deps live in **`[workspace.dependencies]` in the
root `Cargo.toml`**, and members inherit them with `rux-x.workspace = true`.
`cargo publish` rejects a path dependency with no version, so each one needs a
version that matches `[workspace.package] version` exactly. Keeping them in one
block is what stops that being nineteen lines across six files. Step 4 above
bumps them.

**Publishing is a staircase, not a batch.** `cargo publish --dry-run` resolves
against the *real* index, so a crate cannot be dry-run until everything it
depends on is genuinely published. Before the first publish only the five leaves
can be checked; every layer above is unverifiable until the layer below is up,
and `cargo package --no-verify` does not get around it, it resolves too.

Plan for that: publish a layer, wait for the index, then dry-run the next.

- [ ] `cargo publish --dry-run -p <crate>` for the leaves, which is all that can
      be checked up front.
- [ ] Publish in dependency order, one at a time, dry-running each layer once
      the one below it is on the index:
      `rux-parser`, `rux-reactive`, `rux-text`, `rux-layout`, `rux-fmt`
      → `rux-script`, `rux-paint` → `rux-style` → `rux-runtime`
      → `rux-shell` → `ruxlang`.
- [ ] Expect throttling: crates.io rate-limits new crate names, so eleven in one
      sitting will stall partway. That is survivable precisely because the order
      is a staircase, pick up where it stopped.
- [ ] Double-check the version. **crates.io is a one-way door**: yanking hides a
      version, it does not remove or free it, and you cannot re-publish over it.
      Whatever ships first becomes Rux's permanent floor on the registry; there
      will never be a `0.1.0` there.
- [ ] Check the rendered page for `ruxlang` afterwards. It is the one crate with
      a `readme`, and it is what anyone finding Rux by search reads first.

## Release timing

Development and releasing are deliberately decoupled. Keep building and tagging
locally on whatever schedule the work wants; publish on the dates you've
announced.

Nothing in CI fires on a tag. `git tag` is purely local until `git push --tags`,
and the GitHub Release is cut by hand, so a tag that hasn't been pushed is
invisible, and a pushed tag with no Release is just a tag.

**The one thing that publishes itself is a merge to `main`.** `site.yml` deploys
on any push to `main` touching `site/**`, so merging a branch that carries a
release post puts that post live *immediately*, before the tag, before the
announcement. To hold a release for its date, set `draft = true` in the post's
front matter and flip it on release day; CI builds without `--drafts`, so drafts
stay unpublished. Post-dating alone will **not** work, unlike Hugo, Zola builds
future-dated pages.

## Building the site

Zola is a single Rust binary, with no JS toolchain, which is the point. Get it from
<https://www.getzola.org/documentation/getting-started/installation/>, then:

```bash
cd site
zola serve      # live preview at http://127.0.0.1:1111
zola build      # writes site/public (what CI publishes)
```

The version CI uses is pinned in `.github/workflows/site.yml`; build locally with
the same one to avoid surprises.
