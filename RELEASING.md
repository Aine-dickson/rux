# Releasing Rux

Every release ships with a blog post. **No post, no tag**. The post is not
paperwork after the fact, it's part of the release, and writing it is often where
you notice the release isn't actually ready.

This file is the checklist. It's short on purpose.

The checklist below is what a release *is*. **[Packing a
release](#packing-a-release)** is how one is held safely between the day the
work closes and the day it goes out, which is the normal case now that the line
keeps moving after a milestone closes. Read that section too before shipping.

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

`./scripts/release.sh freeze X.Y.Z` does this step and checks it; the boxes below
are what it does, kept here because knowing what the script is for matters more
than the script.

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

`./scripts/release.sh ship X.Y.Z` does the first two boxes and stops before the
push; the rest is by hand on purpose.

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

## Packing a release

Releases go out on Fridays; the work does not stop on Thursday. Once a milestone
closes, the next one starts immediately, and without a discipline for that a
release either drags whatever landed afterwards into the tag, or has to be
reconstructed on the day from memory.

So a release is **packed when the work closes** and **shipped on its date**, and
the two are separate operations on separate commits.

### The capsule

The capsule is a branch, `release/vX.Y.Z`, cut when the milestone closes and
carrying the version drop from step 4. **Nothing lands on it afterwards** except
the single dated commit that `ship` makes. The next milestone branches *from the
capsule*, so the released tree is an ancestor of everything that follows and
nothing that follows can reach back into it.

One rule makes this work, and it is the easiest to forget:

> **Nothing merges to `main` until the release that owns it has shipped.**

`site.yml` deploys on any push to `main` touching `site/**`, so a merge is a
publish. A later milestone merged early would put its docs live under the
current release's name, and would drag its commits into the release merge.

### The commands

```bash
./scripts/release.sh freeze 0.6.0      # pack it: capsule branch + version drop
./scripts/release.sh open-next 0.7.0   # start the next line, off the capsule
./scripts/release.sh check 0.6.0       # prove it still ships, any day
./scripts/release.sh ship 0.6.0        # release day: date, undraft, merge, tag
./scripts/release.sh ship 0.6.0 --push # ...and push, the one-way half
./scripts/release.sh publish 0.6.0            # crates.io, dry run
./scripts/release.sh publish 0.6.0 --execute  # crates.io, for real
```

`check` is the one that earns the discipline. It runs every gate inside a
throwaway git worktree of the capsule, so its answer cannot be influenced by the
working tree, the branch you happen to be on, or anything built since. Run it
the morning of the release; if it is green, the release is the same release it
was the day it was packed.

The gates are: the version and every `[workspace.dependencies]` entry agree and
carry no `-dev`; the post exists, names the right version, and has no
placeholders left; `docs/` and the generated site pages match; `cargo build` is
warning-clean; `cargo test --workspace` is green; and `rux-web` compiles for
`wasm32`. That last one is there because the web build was once broken for three
weeks without anything noticing: nothing else in the suite compiles for wasm.

### What is deliberately not automated

`ship` stops before `git push` unless you pass `--push`, and it never cuts the
GitHub Release. Everything it does before that point is local and undoable;
everything after is not.

`publish` runs from the **tag**, in its own worktree, never from your working
tree. crates.io is a one-way door: a version cannot be replaced, only yanked,
so publishing from a tree that had moved on would put the next milestone's code
under this version number permanently. It skips crates already on the index, so
a run stopped by rate limiting is resumed by running it again.

### The date

`ship` stamps the post with the day it runs and flips `draft = false` in the
same commit. That is why the post is packed as a draft: Zola builds future-dated
pages, so a date alone will not hold a post back, and `draft = true` will. The
post therefore reads as written on the day it went out, however long before it
was actually written.

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
