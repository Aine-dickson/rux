#!/usr/bin/env bash
#
# Pack a release now, ship it on its date.
#
# Rux releases on Fridays, but the work does not stop on Thursday. Without a
# discipline for it, a release either drags whatever landed after the milestone
# closed into the tag, or has to be reconstructed on the day from memory. This
# script is the discipline: the release is *packed* when the work closes, into a
# branch nothing lands on afterwards, and shipping it is a fixed sequence that
# reads the same whether it runs an hour later or a fortnight later.
#
#   ./scripts/release.sh freeze 0.6.0     pack it: the capsule branch + version drop
#   ./scripts/release.sh open-next 0.7.0  start the next line, off the capsule
#   ./scripts/release.sh check 0.6.0      prove the capsule still ships, any day
#   ./scripts/release.sh ship 0.6.0       release day: date, draft, merge, tag
#   ./scripts/release.sh ship 0.6.0 --push  ...and push it, the one-way half
#
# The capsule is a branch, `release/vX.Y.Z`, and the rule is that **nothing
# lands on it after the freeze** except the single dated commit `ship` makes.
# `check` is what makes that rule worth having: it runs every gate inside a
# throwaway git worktree of the capsule, so its answer cannot be influenced by
# the working tree, the current branch, or anything built since.
#
# See RELEASING.md for the checklist this automates the mechanical half of.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo"

die() { echo "release: $*" >&2; exit 1; }
step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
ok() { printf '   ok  %s\n' "$*"; }

version="${2:-}"
# `verify` is the one subcommand that is not about a particular release: it is
# the version-independent half of the gates, so CI can run exactly what a
# release runs and the two cannot drift apart.
if [[ "${1:-}" != "verify" ]]; then
  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "need a version like 0.6.0, got '${version:-nothing}'"
fi
tag="v$version"
branch="release/$tag"
post="site/content/blog/v${version//./-}.md"

# ---------------------------------------------------------------------------
# Gates. Each one is a thing that has actually gone wrong at least once.
# ---------------------------------------------------------------------------

gate_version() {
  local want="$1"
  local have
  have="$(grep -m1 '^version = ' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')"
  [[ "$have" == "$want" ]] || die "[workspace.package] version is '$have', expected '$want'"
  # Every intra-workspace dependency must match exactly: a plain X.Y.Z
  # requirement does not match an X.Y.Z-dev prerelease, so a half-done bump
  # builds locally and breaks on publish.
  local bad
  bad="$(grep -nE '^rux-[a-z]+ = \{ version = ' Cargo.toml | grep -v "\"$want\"" || true)"
  [[ -z "$bad" ]] || die "workspace.dependencies disagree with the version:"$'\n'"$bad"
  ok "version is $want, and all rux-* dependencies agree"
}

gate_post() {
  [[ -f "$post" ]] || die "no release post at $post (no post, no tag)"
  grep -q "version = \"$tag\"" "$post" || die "$post does not carry extra.version = \"$tag\""
  # A post that still says TODO is a post nobody finished.
  local left
  left="$(grep -nE 'TODO|FIXME|vX\.Y\.Z|XXX' "$post" || true)"
  [[ -z "$left" ]] || die "$post still has placeholders:"$'\n'"$left"
  ok "release post present and filled in"
}

gate_docs_synced() {
  ./site/sync-docs.sh --check >/dev/null || die "site/ is out of sync with docs/; run ./site/sync-docs.sh"
  ok "generated site pages match docs/"
}

gate_build() {
  local warnings
  warnings="$(cargo build --workspace 2>&1 | grep -c '^warning' || true)"
  [[ "$warnings" == "0" ]] || die "cargo build emitted $warnings warning(s); releases go out warning-clean"
  ok "cargo build is warning-clean"
}

gate_tests() {
  cargo test --workspace >/dev/null 2>&1 || die "cargo test --workspace is not green"
  local count
  count="$(cargo test --workspace 2>&1 | grep -cE '^test .* ok$' || true)"
  ok "cargo test --workspace green ($count tests)"
}

gate_wasm() {
  # Added after the web build was found broken for three weeks: nothing else in
  # the suite compiles for wasm32, so nothing else notices.
  rustup target list --installed | grep -q wasm32-unknown-unknown \
    || { echo "   -- wasm32 target not installed, skipping"; return; }
  cargo check -p rux-web --target wasm32-unknown-unknown >/dev/null 2>&1 \
    || die "rux-web does not compile for wasm32; the web build is part of the release"
  ok "the web build compiles"
}

# The gates that are true of a healthy tree on any day, release or not.
run_verify() {
  gate_docs_synced
  gate_build
  gate_tests
  gate_wasm
}

run_gates() {
  gate_version "$1"
  gate_post
  run_verify
}

# ---------------------------------------------------------------------------
# freeze: pack the capsule
# ---------------------------------------------------------------------------

cmd_freeze() {
  [[ -z "$(git status --porcelain)" ]] || die "working tree is dirty; commit or stash first"
  git rev-parse --verify --quiet "$branch" >/dev/null && die "$branch already exists; the capsule is packed"

  local from
  from="$(git rev-parse --abbrev-ref HEAD)"
  step "packing $tag from $from"

  git switch -c "$branch" >/dev/null 2>&1
  # Releasing is what drops the -dev suffix: between releases the workspace
  # carries X.Y.Z-dev, which is the line being built rather than a release.
  sed -i -E "s/^version = \"$version-dev\"/version = \"$version\"/" Cargo.toml
  sed -i -E "s/^(rux-[a-z]+ = \{ version = )\"$version-dev\"/\1\"$version\"/" Cargo.toml
  gate_version "$version"

  # The post is packed as a draft. Zola builds future-dated pages, so a date
  # alone will not hold it; `draft = true` is what keeps it unpublished, and
  # `ship` is what flips it.
  [[ -f "$post" ]] || die "write the release post at $post before freezing (no post, no tag)"
  grep -q '^draft = true' "$post" || die "$post must be draft = true at freeze; ship flips it"

  git commit -aqm "Release $tag: drop the -dev suffix

Packed on $(date +%Y-%m-%d), to ship on its announced date. Nothing lands on
this branch afterwards: the dated commit \`release.sh ship\` makes is the only
change it should ever see again."
  step "gates"
  run_gates "$version"
  git switch -q "$from"
  cat <<EOF

  packed: $branch  ($(git rev-parse --short "$branch"))

  Nothing may land on that branch. Continue the next milestone with:
      ./scripts/release.sh open-next <next-version>
  Re-prove the capsule any day with:
      ./scripts/release.sh check $version
EOF
}

# ---------------------------------------------------------------------------
# open-next: start the following line, off the capsule
# ---------------------------------------------------------------------------

cmd_open_next() {
  local next="$version"
  git rev-parse --verify --quiet "$branch" >/dev/null \
    || die "no capsule for $tag; freeze it before opening the next line"
  local line="build/v${next%.*}"
  git rev-parse --verify --quiet "$line" >/dev/null && die "$line already exists"

  step "opening $line at ${next}-dev, from the $tag capsule"
  git switch -qc "$line" "$branch"
  sed -i -E "s/^version = \"[0-9.]+\"/version = \"$next-dev\"/" Cargo.toml
  sed -i -E "s/^(rux-[a-z]+ = \{ version = )\"[0-9.]+\"/\1\"$next-dev\"/" Cargo.toml
  gate_version "$next-dev"
  git commit -aqm "Open the $next line

Branched from the $tag capsule so the released tree is the ancestor of what
comes next, and so nothing here can reach back into the release."
  ok "$line is open; work for the next milestone goes here"
}

# ---------------------------------------------------------------------------
# check: prove the capsule still ships, from anywhere
# ---------------------------------------------------------------------------

cmd_check() {
  git rev-parse --verify --quiet "$branch" >/dev/null || die "no capsule branch $branch"
  local work
  work="$(mktemp -d)/rux-$tag"
  step "checking $branch in an isolated worktree"
  echo "   $work"
  git worktree add -q --detach "$work" "$branch"
  # shellcheck disable=SC2064
  trap "git worktree remove --force '$work' >/dev/null 2>&1 || true" EXIT
  (
    cd "$work"
    run_gates "$version"
  )
  cat <<EOF

  $tag is shippable. It was checked against $branch alone: the working tree,
  the current branch and anything built since had no say in that answer.
EOF
}

# ---------------------------------------------------------------------------
# ship: release day
# ---------------------------------------------------------------------------

cmd_ship() {
  local push="${3:-}"
  [[ -z "$(git status --porcelain)" ]] || die "working tree is dirty; commit or stash first"
  git rev-parse --verify --quiet "$branch" >/dev/null || die "no capsule branch $branch"
  git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null && die "$tag already exists"

  cmd_check "$version"

  local today
  today="$(date +%Y-%m-%d)"
  step "shipping $tag, dated $today"
  git switch -q "$branch"
  # The date is stamped now rather than at freeze, so the post reads as written
  # on the day it went out, which is the whole point of packing it early.
  sed -i -E "s/^date = .*/date = $today/" "$post"
  sed -i -E "s/^draft = true/draft = false/" "$post"
  grep -q '^draft = false' "$post" || die "failed to undraft $post"
  git commit -aqm "Publish the $tag post

Dated and undrafted on the day. A merge to main deploys the site, so this is
the commit that makes the release public."

  step "merging to main and tagging"
  git switch -q main
  git merge -q --no-ff "$branch" -m "Release $tag"
  git tag -a "$tag" -m "Rux $tag"
  ok "main is at $tag ($(git rev-parse --short HEAD))"

  if [[ "$push" == "--push" ]]; then
    step "pushing (this is the one-way half)"
    git push origin main
    git push origin "$tag"
    ok "pushed; the site deploys from main"
  else
    cat <<EOF

  Nothing has been pushed. Everything so far is local and undoable.
  When you are ready:
      git push origin main && git push origin $tag

  Then, by hand, because neither is scriptable safely:
    - cut the GitHub Release and link the post
    - publish to crates.io in the staircase order in RELEASING.md
EOF
  fi
}

# ---------------------------------------------------------------------------
# publish: crates.io, from the tag and nothing else
# ---------------------------------------------------------------------------
#
# The eleven published crates, in dependency order. `cargo publish --dry-run`
# resolves against the *real* index, so a crate cannot be checked until
# everything it depends on is genuinely up: publishing is a staircase, not a
# batch, and each layer waits for the one below to appear.
#
# rux-web and rux-highlight are `publish = false` on purpose and are not here.
LAYERS=(
  "rux-parser rux-reactive rux-text rux-layout rux-fmt"
  "rux-script rux-paint"
  "rux-style"
  "rux-runtime"
  "rux-shell"
  "ruxlang"
)

# Whether this exact version is already on the index. Publishing is a one-way
# door: a version cannot be replaced, only yanked, so a re-run must skip what
# it already did rather than fail on it.
on_index() {
  local crate="$1" want="$2" code
  code="$(curl -s -o /dev/null -w '%{http_code}' \
    "https://crates.io/api/v1/crates/$crate/$want" 2>/dev/null || echo 000)"
  [[ "$code" == "200" ]]
}

wait_for_index() {
  local crate="$1" want="$2"
  for _ in $(seq 1 60); do
    on_index "$crate" "$want" && { ok "$crate $want is on the index"; return; }
    sleep 5
  done
  die "$crate $want never appeared on the index; re-run publish to pick up here"
}

cmd_publish() {
  local mode="${3:-}"
  git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null \
    || die "no tag $tag; publish what was shipped, not what is lying around"

  # From the tag, in a throwaway worktree. This is the isolation that matters
  # most: crates.io is permanent, and publishing from a working tree that has
  # moved on would put the next milestone's code under this version number
  # with no way to take it back.
  local work
  work="$(mktemp -d)/rux-publish-$tag"
  step "publishing $tag from the tag, in an isolated worktree"
  echo "   $work"
  git worktree add -q --detach "$work" "refs/tags/$tag"
  # shellcheck disable=SC2064
  trap "git worktree remove --force '$work' >/dev/null 2>&1 || true" EXIT

  cd "$work"
  gate_version "$version"

  local execute=0
  [[ "$mode" == "--execute" ]] && execute=1
  (( execute )) || step "dry run: nothing will be published (pass --execute to publish)"

  for layer in "${LAYERS[@]}"; do
    step "layer: $layer"
    for crate in $layer; do
      if on_index "$crate" "$version"; then
        ok "$crate $version already published, skipping"
        continue
      fi
      if (( execute )); then
        echo "   publishing $crate $version"
        # Rate limiting is expected and survivable precisely because the order
        # is a staircase: a failed run picks up where it stopped.
        cargo publish -p "$crate" || die "publishing $crate failed; re-run to resume from here"
      else
        echo "   dry-run $crate $version"
        cargo publish --dry-run -p "$crate" >/dev/null 2>&1 \
          && ok "$crate would publish" \
          || echo "   -- $crate cannot be dry-run yet (its dependencies are not on the index)"
      fi
    done
    # Only wait when something was actually published, and only for a layer
    # something below still depends on.
    if (( execute )); then
      for crate in $layer; do wait_for_index "$crate" "$version"; done
    fi
  done

  if (( execute )); then
    cat <<EOF

  All eleven are up. Check the rendered page for ruxlang: it is the one crate
  with a readme, and it is what anyone finding Rux by search reads first.
      https://crates.io/crates/ruxlang
EOF
  fi
}

case "${1:-}" in
  verify)
    step "verifying the tree"
    run_verify
    echo
    ;;
  freeze)    cmd_freeze ;;
  open-next) cmd_open_next ;;
  check)     cmd_check "$version" ;;
  ship)      cmd_ship "$version" "${3:-}" ;;
  publish)   cmd_publish "$version" "${3:-}" ;;
  *) die "usage: release.sh {verify|freeze|open-next|check|ship|publish} [<X.Y.Z>] [--push|--execute]" ;;
esac
