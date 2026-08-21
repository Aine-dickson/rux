#!/usr/bin/env bash
#
# Build the playground bundle: crates/rux-web compiled to wasm, emitted into
# site/static/wasm/ where Zola copies it like any other static asset.
#
# It is built rather than committed: ~8 MB of binary does not belong in the
# history, and `site/static/wasm/` is gitignored for that reason.
#
#   ./site/build-playground.sh              from the working tree (the tip)
#   ./site/build-playground.sh --from-tag   from the latest release tag
#
# ## Why there are two modes
#
# The deployed playground runs the latest *released* runtime, so ruxlang.dev
# never demonstrates behaviour nobody can install yet. That is right for the
# public site and wrong for the machine a milestone is being built on: during
# v0.7, `/recipes/` and `/reference/` describe the tip, and a recipe copied from
# the page documenting it failed in the playground beside it with
# `Unknown operator: '++'`. The version badge said `v0.6.1` in small grey text
# in the other corner, and nothing connected the two.
#
# So: **local builds track the tip, CI builds track the tag.** A developer
# checks their own work; the public site shows what a visitor can install. This
# script is the one implementation of both, so the two cannot drift on anything
# except the flag.
#
# Needs `wasm-bindgen` on PATH at the version cargo resolves, and the
# wasm32-unknown-unknown target installed. Both are checked below, because the
# failure otherwise arrives as a wall of linker output.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="$repo/site/static/wasm"

from_tag=false
if [[ "${1:-}" == "--from-tag" ]]; then
  from_tag=true
elif [[ -n "${1:-}" ]]; then
  echo "usage: $0 [--from-tag]" >&2
  exit 2
fi

# ── the tools ────────────────────────────────────────────────────────────────

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  echo "build-playground: wasm-bindgen is not on PATH." >&2
  echo "  cargo install wasm-bindgen-cli --version <the version below>" >&2
  exit 2
fi

if ! rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then
  echo "build-playground: the wasm target is missing." >&2
  echo "  rustup target add wasm32-unknown-unknown" >&2
  exit 2
fi

# ── what to build from ───────────────────────────────────────────────────────

build_from() {
  local dir="$1" what="$2"

  # The CLI refuses a binary produced by a different version of the crate, and
  # the message it gives is clear only if you already know to look for it. So it
  # is named before ten minutes of compiling rather than after.
  #
  # Read from the lockfile of the tree being *built*, not the repo's: under
  # `--from-tag` those are different files, and an older tag can resolve an
  # older wasm-bindgen than main does.
  local resolved installed
  resolved="$(sed -n '/^name = "wasm-bindgen"$/{n;s/^version = "\(.*\)"$/\1/p;q}' "$dir/Cargo.lock")"
  installed="$(wasm-bindgen --version | awk '{print $2}')"
  if [[ -n "$resolved" && "$installed" != "$resolved" ]]; then
    echo "build-playground: wasm-bindgen $installed is installed, $what resolves $resolved." >&2
    echo "  cargo install wasm-bindgen-cli --version $resolved --force" >&2
    exit 2
  fi

  echo "build-playground: building the playground from $what"
  (
    cd "$dir"
    cargo build -p rux-web --target wasm32-unknown-unknown --profile wasm-release
    wasm-bindgen --target web --no-typescript \
      --out-dir "$out" \
      target/wasm32-unknown-unknown/wasm-release/rux_web.wasm
  )
}

if $from_tag; then
  tag="$(git -C "$repo" tag --list 'v*' --sort=-v:refname | head -n1)"
  # A tag from before the playground existed carries no `crates/rux-web`, and
  # checking that out would fail with a cargo error about a missing package
  # rather than anything to do with tags.
  if [[ -z "$tag" ]] || ! git -C "$repo" cat-file -e "$tag:crates/rux-web" 2>/dev/null; then
    echo "build-playground: no tag carries crates/rux-web; building from HEAD instead." >&2
    build_from "$repo" "HEAD (no usable tag)"
  else
    work="$(mktemp -d)/rux-release"
    # Removed on every exit, including a failed compile: a stray worktree makes
    # the *next* run fail with "already exists", which reads as a git problem.
    trap 'git -C "$repo" worktree remove --force "$work" >/dev/null 2>&1 || true' EXIT
    git -C "$repo" worktree add --detach "$work" "$tag" >/dev/null
    build_from "$work" "$tag"
  fi
else
  build_from "$repo" "the working tree"
fi

# ── say what actually landed ─────────────────────────────────────────────────

# The bundle reports its own version through `rux_web::version()`, taken from
# CARGO_PKG_VERSION, and the page prints it in the badge. So the version is a
# literal in the binary, and confirming the expected one is in there is what
# stops this script from claiming a build it did not do: a wasm-bindgen step
# that silently reused a stale `--out-dir` looks identical otherwise, and that
# is exactly the confusion this whole two-mode arrangement exists to end.
#
# `strings` is not present in Git Bash, and a bare version-shaped grep over the
# binary matches every dependency's version too. So the expected value is
# derived first and then looked for, which is a stronger check anyway.
if $from_tag && [[ -n "${tag:-}" ]]; then
  expected="${tag#v}"
else
  # `version = "0.7.0-dev"` from [workspace.package], which every crate inherits.
  expected="$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"$/\1/p;}' "$repo/Cargo.toml" | head -n1)"
fi

ls -lh "$out"

if [[ -z "$expected" ]]; then
  echo "build-playground: built, but the expected version could not be read from Cargo.toml" >&2
elif grep -a -q -F "$expected" "$out/rux_web_bg.wasm"; then
  echo "build-playground: the bundle carries $expected"
else
  echo "build-playground: the bundle does NOT carry $expected." >&2
  echo "  The emitted wasm is not the one just built. Delete $out and re-run." >&2
  exit 1
fi

echo "build-playground: reload the page with a hard refresh; the old bundle is cached."
