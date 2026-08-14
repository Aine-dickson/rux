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
#   01-rationale  -> the hand-written site/content/why.md covers this ground
#   03-guide      -> opens by saying it does not work as written; /learn is
#                    authored against 05-as-built instead, by hand
# See docs/README.md.
#
#   <source>|<output path>|<title>|<weight>|<description>
pages=(
  "05-as-built.md|reference/_index.md|Reference|1|What Rux actually does today: the authoritative honored-CSS set, elements, and directives."
  "02-spec.md|reference/spec.md|Design surface (v0.1)|2|The original v0.1 spec, kept as design history. Not a description of the built runtime."
  "06-roadmap.md|roadmap/_index.md|Roadmap|3|Where Rux goes next: milestones, the release cadence, and what is deliberately not being built."
  "04-architecture.md|contribute/_index.md|Architecture|4|How a .rux file becomes pixels, and which crate owns which stage."
)

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
rewrite_links() {
  sed -E \
    -e ':a' -e 's;(\(#[a-z0-9-]*)--;\1-;g' -e 'ta' \
    -e 's;(\(#v[0-9])([0-9]);\1-\2;g' \
    -e 's;\.\/05-as-built\.md;/reference/;g' \
    -e 's;\.\/02-spec\.md;/reference/spec/;g' \
    -e 's;\.\/06-roadmap\.md;/roadmap/;g' \
    -e 's;\.\/04-architecture\.md;/contribute/;g' \
    -e 's;\.\/01-rationale\.md;/why/;g' \
    -e 's;\.\/03-guide\.md;https://github.com/Aine-dickson/rux/blob/main/docs/03-guide.md;g' \
    -e 's;\.\/README\.md;https://github.com/Aine-dickson/rux/tree/main/docs;g'
}

generated=()

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
    tail -n +2 "$repo/docs/$src" | rewrite_links
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
