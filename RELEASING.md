# Releasing Rux

Every release ships with a blog post. **No post, no tag** — the post is not
paperwork after the fact, it's part of the release, and writing it is often where
you notice the release isn't actually ready.

This file is the checklist. It's short on purpose.

## The rule

> A release is a version tag, a GitHub Release, **and** a blog post at
> `site/content/blog/`. All three, or none.

The blog is not a changelog. A changelog says *what changed*; a Rux release post
says **what landed, what it cost, and what's still broken** — in that order, in
prose. The two existing posts (`v0-1-0.md`, `v0-2-0.md`) are the reference for
tone and length. If you're tempted to write "various fixes and improvements,"
stop: that sentence means you haven't found the story yet.

Two things every post must contain, because they're the two things this project
keeps learning:

1. **The bug you only found by looking.** Every release so far had one — a defect
   the test suite was green through, obvious within seconds in the window. Name
   it. It's the most useful paragraph for the next person (usually you).
2. **What the release is *not*.** The honest limits, as plainly as the wins. This
   is the project's whole credibility; don't spend it.

## Checklist

### 1. Land the work

- [ ] Every feature has been **driven in the window**, not just tested. This is
      the standing rule — see [`docs/06-roadmap.md`](docs/06-roadmap.md). Tests
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

### 4. Tag and publish

- [ ] Merge to `main`. The site deploys automatically
      ([`.github/workflows/site.yml`](.github/workflows/site.yml)).
- [ ] `git tag vX.Y.Z && git push --tags`.
- [ ] Cut the GitHub Release. Link the blog post; attach a release binary if the
      platform matrix changed (`cargo build --release -p rux-cli`).
- [ ] Confirm the post is live at `/rux/blog/vX-Y-Z/` and the home page shows it.

## Building the site

Zola is a single Rust binary — no JS toolchain, which is the point. Get it from
<https://www.getzola.org/documentation/getting-started/installation/>, then:

```bash
cd site
zola serve      # live preview at http://127.0.0.1:1111
zola build      # writes site/public (what CI publishes)
```

The version CI uses is pinned in `.github/workflows/site.yml`; build locally with
the same one to avoid surprises.
