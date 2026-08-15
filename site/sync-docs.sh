#!/usr/bin/env bash
#
# Generate site pages from the canonical docs in `docs/`.
#
# `docs/` is the source of truth. Edit there, never in the generated files.
# This script wraps each doc in Zola front matter and rewrites the intra-doc
# links so they resolve as site URLs instead of relative .md paths.
#
# The generated files ARE committed, so the site builds standalone and reviewers
# can see the published text in a diff. CI re-runs this and fails if the result
# differs from what's committed (see .github/workflows/site.yml), which is what
# stops docs/ and the site from drifting apart silently.
#
#   ./site/sync-docs.sh          regenerate
#   ./site/sync-docs.sh --check  exit 1 if regenerating would change anything
#
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
content="$repo/site/content"

# Not every doc is publishable, and that is deliberate:
#   03-guide      -> opens by saying it does not work as written; /learn is
#                    authored against 05-as-built instead, by hand
# See docs/README.md.
#
# `01-rationale.md` used to be on that list, on the grounds that the
# hand-written `why.md` covered the same ground. It does not, and leaving it
# unpublished had a cost that went unnoticed for a long time: 02-spec,
# 04-architecture and 06-roadmap all link into its headings, those links were
# rewritten to `/why/`, and `why.md` has none of those headings. Eleven dead
# anchors shipped, and Zola did not complain because it only validates `@/`
# links, not absolute ones.
#
# The two documents are for different readers. `why.md` is the argument for
# using Rux; the rationale is the design record: the four laws, the element
# audit, and the decisions with their tradeoffs. Publishing it makes those
# links true and stops the reasoning being reachable only from GitHub.
#
#   <source>|<output path>|<title>|<weight>|<description>
pages=(
  "05-as-built.md|reference/_index.md|Reference|1|What Rux actually does today: the authoritative honored-CSS set, elements, and directives."
  # Weights interleave with the `splits` table below, which is what orders the
  # reference sidebar: overview, elements, layout, CSS, reactivity, script, then
  # the rest of the runtime, with the v0.1 design history last.
  "07-script.md|reference/script.md|Script|6|The script language: state, functions, values, the element API, and every way it differs from rhai and from JavaScript."
  "02-spec.md|reference/spec.md|Design surface (v0.1)|16|The original v0.1 spec, kept as design history. Not a description of the built runtime."
  "01-rationale.md|reference/rationale.md|Design rationale|17|The four laws, the element audit, and the decisions behind them, with the tradeoffs each one accepted."
  "06-roadmap.md|roadmap/_index.md|Roadmap|4|Where Rux goes next: milestones, the release cadence, and what is deliberately not being built."
  "04-architecture.md|contribute/_index.md|Architecture|5|How a .rux file becomes pixels, and which crate owns which stage."
)

# Sections of a doc that become pages of their own.
#
# `docs/05-as-built.md` is one file on GitHub and was one 1173-line page on the
# site, which is a scroll rather than a reference: someone who wants to know how
# routing works should land on routing, not on the middle of a wall. Splitting
# `docs/` itself into thirteen files was the other option and it was rejected,
# because the value of that document is that it is *one* thing you can read end
# to end, and because a contributor should not have to guess which file a new
# paragraph belongs in.
#
# So the split happens here, on the way out. Each entry lifts one heading and
# everything under it, up to the next heading at the same or a shallower level,
# into its own page. The parent keeps everything else, and gets a generated
# contents list where the sections used to be.
#
#   <source>|<heading>|<output path>|<title>|<weight>|<description>
splits=(
  "05-as-built.md|### Elements|reference/elements.md|Elements|2|The six elements the runtime renders, plus slot, router and route."
  # Single-quoted: this heading contains backticks, which a double-quoted shell
  # string would run as a command.
  '05-as-built.md|### Layout: **use `display: flex`**|reference/layout.md|Layout|3|Everything defaults to block; use display: flex. Hug, fill, and why inline flow is gone.'
  "05-as-built.md|### Honored CSS|reference/css.md|Honored CSS|4|The authoritative list of properties the runtime interprets, plus selectors, pseudo-classes and transitions."
  "05-as-built.md|### Reactivity & script|reference/reactivity.md|Reactivity|5|Signals, computed values, effects, and what re-runs when one changes."
  "05-as-built.md|### Inputs|reference/inputs.md|Inputs|7|Text fields, textarea, select, checkbox and radio, and two-way binding with r-model."
  "05-as-built.md|### Text input and composition|reference/text-input.md|Text input|8|The caret, the soft keyboard, and IME composition for text that is not typed one key at a time."
  "05-as-built.md|### Touch|reference/touch.md|Touch|9|What a finger does today, and why @tap is the whole vocabulary."
  "05-as-built.md|### Selection & clipboard|reference/selection.md|Selection|10|Drag-select, double-click, and the clipboard keys."
  "05-as-built.md|### Scrolling|reference/scrolling.md|Scrolling|11|Scrollers, scrollbars, and the ways a scroll can be driven."
  "05-as-built.md|### Components|reference/components.md|Components|12|Importing a file as a tag, props, slots, events, and what a component cannot see."
  "05-as-built.md|### Routing|reference/routing.md|Routing|13|Routes, parameters, named routes, links, and the fact that the path is an ordinary signal."
  "05-as-built.md|### Accessibility|reference/accessibility.md|Accessibility|14|The real accessibility tree, the roles elements map to, and what a screen reader is told."
  "05-as-built.md|### Errors & the dev overlay|reference/errors.md|Errors|15|What happens when a document will not load, and what the overlay shows."
)

# Sections removed from the site copy with no page of their own, because
# something hand-written already covers that ground better.
#
# The CLI sections here are the site's `/tooling/` pages, which are task-shaped
# and go further than this reference does (installing the editor extension has
# no place in a document about what the runtime honors). They stay in
# `docs/05-as-built.md` for whoever reads it end to end on GitHub.
#
# What the site copy says instead, where those sections used to be. Without
# this the CLI would simply vanish from the reference with no trail to follow.
declare -A pointer_for=(
  ["05-as-built.md"]='## The `rux` command

Creating a project, running it, checking it and formatting it each have a page
under [Tooling](/tooling/), along with how to set up the VS Code extension.
'
)

#   <source>|<heading>
strip_only=(
  "05-as-built.md|## Starting a project"
  "05-as-built.md|## Running it"
  "05-as-built.md|## Formatting"
  "05-as-built.md|## Checking a file without opening a window"
  "05-as-built.md|## Telling an editor what the runtime understands"
)

# Extract one section: the heading line is dropped (the template renders the
# title from front matter) and everything under it is kept, up to the next
# heading at the same or a shallower level.
extract_section() {
  awk -v want_heading="$2" '
    function level(s,   n) { n = 0; while (substr(s, n + 1, 1) == "#") n++; return n }
    !started && $0 == want_heading { started = 1; want = level($0); next }
    started && /^#+[ \t]/ && level($0) <= want { exit }
    started { print }
  ' "$1"
}

# The parent, minus every section that was lifted out of it.
#
# The order of the rules matters: a heading that ends one skipped section may
# itself begin the next, so the "stop skipping" rule has to run before the
# "start skipping" rule, and neither may consume the line the other needs.
strip_sections() {
  local file="$1"
  shift
  awk -v heads="$(printf '%s\n' "$@")" '
    function level(s,   n) { n = 0; while (substr(s, n + 1, 1) == "#") n++; return n }
    BEGIN {
      n = split(heads, list, "\n")
      for (i = 1; i <= n; i++) if (list[i] != "") drop[list[i]] = 1
    }
    skipping && /^#+[ \t]/ && level($0) <= want { skipping = 0 }
    !skipping && ($0 in drop) { skipping = 1; want = level($0); next }
    !skipping { print }
  ' "$file"
}

# Rewrite `./0N-name.md` links (and their #anchors) to site URLs. Docs that are
# not published fall back to GitHub so the link still goes somewhere real.
#
# The `:a … ta` loop collapses doubled hyphens inside link anchors, because
# GitHub and Zola slugify headings differently: a dropped character (`&`, an em
# dash) leaves a hyphen behind on GitHub but not in Zola, so `## Directives &
# bindings` is `#directives--bindings` there and `#directives-bindings` here.
# The docs are written against GitHub's rendering; this translates on the way in.
# Zola validates every internal anchor at build time, so a miss fails the build
# rather than shipping a dead link.
# Apostrophes are the other slug divergence, and it goes the opposite way from
# the doubled hyphen below. GitHub *drops* punctuation inside a word, so
# `don't` is `dont`; Zola replaces every run of non-alphanumerics with a hyphen,
# so it is `don-t`. There is one such heading today, `Law 4: Stay close to
# Rust; don't pop the balloon`, and the site shipped a dead link to it. A
# general rule is not possible from the link alone: `dont` in an anchor could be
# a real word. Add a line here when a heading with an apostrophe is linked to.
fix_apostrophe_anchors() {
  sed -E -e 's;#law-4-stay-close-to-rust-dont-pop-the-balloon;#law-4-stay-close-to-rust-don-t-pop-the-balloon;g'
}

rewrite_links() {
  fix_apostrophe_anchors | sed -E \
    -e ':a' -e 's;(\(#[a-z0-9-]*)--;\1-;g' -e 'ta' \
    -e 's;(\(#v[0-9])([0-9]);\1-\2;g' \
    -e 's;\.\/05-as-built\.md#honored-css;/reference/css/;g' \
    -e 's;\.\/05-as-built\.md#routing;/reference/routing/;g' \
    -e 's;\.\/05-as-built\.md#components;/reference/components/;g' \
    -e 's;\(#routing\);(/reference/routing/);g' \
    -e 's;\(#honored-css\);(/reference/css/);g' \
    -e 's;\(#components\);(/reference/components/);g' \
    -e 's;\.\/05-as-built\.md;/reference/;g' \
    -e 's;\.\/02-spec\.md;/reference/spec/;g' \
    -e 's;\.\/07-script\.md;/reference/script/;g' \
    -e 's;\.\/06-roadmap\.md;/roadmap/;g' \
    -e 's;\.\/04-architecture\.md;/contribute/;g' \
    -e 's;\.\/01-rationale\.md;/reference/rationale/;g' \
    -e 's;\.\/03-guide\.md;https://github.com/Aine-dickson/rux/blob/main/docs/03-guide.md;g' \
    -e 's;\.\/README\.md;https://github.com/Aine-dickson/rux/tree/main/docs;g'
}

generated=()

# The headings lifted out of each source, and the contents list that replaces
# them. Both are derived from `splits` so that adding a page there is the only
# edit needed.
declare -A lifted_from=()
declare -A contents_for=()
for entry in "${strip_only[@]}"; do
  IFS='|' read -r src heading <<< "$entry"
  lifted_from["$src"]+="$heading"$'\n'
done

for entry in "${splits[@]}"; do
  IFS='|' read -r src heading out title weight desc <<< "$entry"
  lifted_from["$src"]+="$heading"$'\n'
  # `/reference/elements.md` is served at `/reference/elements/`.
  contents_for["$src"]+="- [$title](/${out%.md}/): $desc"$'\n'
done


for entry in "${splits[@]}"; do
  IFS='|' read -r src heading out title weight desc <<< "$entry"
  dest="$content/$out"
  mkdir -p "$(dirname "$dest")"

  body="$(extract_section "$repo/docs/$src" "$heading")"
  if [[ -z "${body//[[:space:]]/}" ]]; then
    echo "error: '$heading' is not in docs/$src, or is empty." >&2
    echo "The splits table in site/sync-docs.sh names a section that moved or was renamed." >&2
    exit 1
  fi

  {
    echo "+++"
    echo "title = \"$title\""
    echo "description = \"$desc\""
    echo "weight = $weight"
    echo "+++"
    echo
    echo "<!-- GENERATED FROM docs/$src BY site/sync-docs.sh. DO NOT EDIT HERE. -->"
    echo
    printf '%s\n' "$body" | rewrite_links
  } > "$dest"

  generated+=("site/content/$out")
done

for entry in "${pages[@]}"; do
  IFS='|' read -r src out title weight desc <<< "$entry"
  dest="$content/$out"
  mkdir -p "$(dirname "$dest")"

  {
    echo "+++"
    echo "title = \"$title\""
    echo "description = \"$desc\""
    echo "weight = $weight"
    # Sections need a template and a sort key; plain pages take the defaults.
    if [[ "$out" == *_index.md ]]; then
      echo "sort_by = \"weight\""
      echo "template = \"docs-section.html\""
      echo "page_template = \"docs.html\""
    fi
    echo "+++"
    echo
    echo "<!-- GENERATED FROM docs/$src BY site/sync-docs.sh. DO NOT EDIT HERE. -->"
    echo
    # Drop the leading `# NN. Title` heading: the template renders the title
    # from front matter, and a second <h1> would be wrong for both a11y and the
    # search index.
    if [[ -n "${lifted_from[$src]:-}" ]]; then
      # This doc has pages of its own. Strip what moved, and put a contents
      # list where it used to be, so the overview still says what exists.
      # `mapfile` rather than word splitting, because the headings contain
      # spaces and ampersands.
      mapfile -t heads <<< "${lifted_from[$src]}"
      tail -n +2 "$repo/docs/$src" \
        | strip_sections /dev/stdin "${heads[@]}" \
        | awk -v list="${contents_for[$src]}" -v pointer="${pointer_for[$src]:-}" '
            # The contents list replaces the sections that became pages.
            /^## What works$/ && !listed { print; print ""; printf "%s", list; listed = 1; next }
            # The pointer replaces the sections that were dropped in favour of
            # something hand-written, so the overview still says where they went.
            /^## Crates$/ && pointer != "" && !pointed { printf "%s\n", pointer; pointed = 1 }
            { print }
          ' \
        | rewrite_links
    else
      tail -n +2 "$repo/docs/$src" | rewrite_links
    fi
  } > "$dest"

  generated+=("site/content/$out")
done

if [[ "${1:-}" == "--check" ]]; then
  # Two questions, because neither command answers both:
  #
  #   · `git diff` for content. Not `git status --porcelain`, which was used
  #     here and is wrong on Windows: with `core.autocrlf=true` a regenerated
  #     file is written with LF where the checkout would have CRLF, and status
  #     calls that modified *forever* while diff correctly reports the content
  #     as identical. So this check failed on every Windows run whether or not
  #     anything had drifted, and CI never noticed because CI is Linux.
  #   · `ls-files --others` for new pages, which diff is blind to: a page added
  #     to the list above would otherwise pass the check while never being
  #     committed. That was the original reason for using status, and it is
  #     kept rather than lost.
  changed="$(git -C "$repo" diff --name-only -- "${generated[@]}")"
  untracked="$(git -C "$repo" ls-files --others --exclude-standard -- "${generated[@]}")"
  # Written as an `if`, not `[[ … ]] && …`: under `set -e` a trailing test that
  # is simply false would end the script as though it had failed.
  if [[ -n "$changed" || -n "$untracked" ]]; then
    echo "error: generated docs are out of sync with docs/." >&2
    echo "Run ./site/sync-docs.sh and commit the result." >&2
    [[ -n "$changed" ]] && printf 'changed: %s\n' $changed >&2
    [[ -n "$untracked" ]] && printf 'new:     %s\n' $untracked >&2
    exit 1
  fi
  echo "generated docs are up to date."
else
  printf 'wrote %s\n' "${generated[@]}"
fi
