#!/usr/bin/env bash
#
# Regenerate the vocabulary the VS Code extension ships with.
#
# `rux vocab` is the source of truth: it reads the honored-CSS list out of
# `crates/rux-style` and the void-tag list out of `crates/rux-fmt`, so the
# editor cannot offer a property the runtime warns is unhonored, or write
# `</image>` after a tag that never nests.
#
# The generated file IS committed, because completions have to work the moment
# someone installs the extension from the Marketplace, before `cargo install
# ruxlang` has finished. CI re-runs this and fails if the result differs from
# what is committed, which is what stops the two from drifting apart silently.
# That drift is not hypothetical: it is exactly how the `<image>` indentation
# bug survived two releases.
#
#   ./scripts/sync-vocabulary.sh          regenerate
#   ./scripts/sync-vocabulary.sh --check  exit 1 if regenerating would change anything
#
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="$repo/editors/vscode/vocabulary.json"

# Built rather than `cargo run`, so the check does not print cargo's progress
# into the JSON it is about to compare.
cargo build --quiet --manifest-path "$repo/Cargo.toml" -p ruxlang --bin rux

generated="$(mktemp)"
trap 'rm -f "$generated"' EXIT

# `target/debug/rux` on Unix, `.exe` on Windows; both are checked so this runs
# from Git Bash on a developer machine as well as on a Linux runner.
for candidate in "$repo/target/debug/rux" "$repo/target/debug/rux.exe"; do
  if [[ -x "$candidate" ]]; then
    "$candidate" vocab > "$generated"
    break
  fi
done

if [[ ! -s "$generated" ]]; then
  echo "sync-vocabulary: could not run the freshly built \`rux\`" >&2
  exit 2
fi

if [[ "${1:-}" == "--check" ]]; then
  if ! diff -u "$out" "$generated" > /dev/null 2>&1; then
    echo "editors/vscode/vocabulary.json is out of date." >&2
    echo "The runtime's vocabulary changed and the extension's copy did not." >&2
    echo "Run ./scripts/sync-vocabulary.sh and commit the result." >&2
    diff -u "$out" "$generated" >&2 || true
    exit 1
  fi
  echo "vocabulary.json is current"
  exit 0
fi

cp "$generated" "$out"
echo "wrote editors/vscode/vocabulary.json"
