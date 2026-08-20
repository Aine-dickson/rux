#!/usr/bin/env bash
#
# Fill any code fence on the site that claims to be a file with that file.
#
# Mark a fence by putting a comment on the line above it:
#
#     <!-- FROM: examples/recipes/message-list.rux -->
#     ```rux
#     …replaced by the file's contents…
#     ```
#
#   ./site/sync-examples.sh          regenerate
#   ./site/sync-examples.sh --check  exit 1 if regenerating would change anything
#
# ## Why this exists
#
# `/recipes/message-list/` had a section headed "The whole file" whose fence was
# a 53-line abridgement of a 179-line example: no `.app` rule, no bubble
# backgrounds, no title. The prose three lines below it said so, and nobody
# reads a caveat under a heading that says "whole".
#
# It stopped being a documentation nit the day every fence grew a Copy button
# and a Try it link. Both hand the fence straight to the playground, so a reader
# copying "the whole file" got a screen with no styling on it, which looks
# exactly like a broken renderer and is not one. Confirmed by running the page's
# own code in a desktop window: it renders the same way there, pixel for pixel.
#
# The examples are already under test (`crates/rux-runtime/tests/examples.rs`
# walks `examples/recipes/`). A hand-copied second version on the page is a copy
# of tested code that nothing tests, which is the drift this repo has now been
# bitten by three times: the extension's void-tag list, the site's grammar copy,
# and this.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Every page that carries at least one marker. Kept as a search rather than a
# list so adding a marker to a new page needs no edit here.
mapfile -t pages < <(grep -rl --include='*.md' -- '<!-- FROM: ' "$repo/site/content" | sort)

if [[ ${#pages[@]} -eq 0 ]]; then
  echo "sync-examples: no fences are marked; nothing to do."
  exit 0
fi

# Rewrite one page on stdout.
#
# `repo` is passed in rather than derived, so the awk program never has to guess
# where it is being run from.
fill() {
  awk -v repo="$repo" '
    # The marker. Everything after it up to the closing fence is replaced.
    /^[ \t]*<!-- FROM: .* -->[ \t]*$/ {
      print
      path = $0
      sub(/^[ \t]*<!-- FROM:[ \t]*/, "", path)
      sub(/[ \t]*-->[ \t]*$/, "", path)
      pending = 1
      next
    }

    # The fence opener that follows a marker.
    pending && /^[ \t]*```/ {
      print
      full = repo "/" path
      n = 0
      while ((getline line < full) > 0) { print line; n++ }
      close(full)
      if (n == 0) {
        # A marker naming a file that is not there would otherwise empty the
        # fence in silence, and the check below would then call the page
        # correct once the emptying was committed.
        print "sync-examples: cannot read " path > "/dev/stderr"
        exit 3
      }
      pending = 0
      skipping = 1
      next
    }

    # The old body, dropped.
    skipping && /^[ \t]*```[ \t]*$/ { skipping = 0; print; next }
    skipping { next }

    { print }
  ' "$1"
}

status=0
for page in "${pages[@]}"; do
  generated="$(mktemp)"
  fill "$page" > "$generated"

  if [[ "${1:-}" == "--check" ]]; then
    # Carriage returns stripped before comparing, the same trap
    # `sync-vocabulary.sh --check` and `site/sync-docs.sh` both document: with
    # `core.autocrlf=true` the checked-out copy has CRLF and a freshly generated
    # one has LF, so a raw diff reports every page stale on every Windows run.
    if ! diff -u <(tr -d '\r' < "$page") <(tr -d '\r' < "$generated") > /dev/null 2>&1; then
      echo "error: ${page#"$repo/"} does not match the file its fence names." >&2
      echo "Run ./site/sync-examples.sh and commit the result." >&2
      diff -u <(tr -d '\r' < "$page") <(tr -d '\r' < "$generated") >&2 || true
      status=1
    fi
  else
    cp "$generated" "$page"
    echo "filled ${page#"$repo/"}"
  fi
  rm -f "$generated"
done

if [[ "${1:-}" == "--check" && $status -eq 0 ]]; then
  echo "every marked fence matches the file it names"
fi
exit $status
