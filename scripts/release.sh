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
  #
  # Matched on `path = "crates/`, not on the dependency's name. The name was the
  # rule until `rhai = { package = "rux-rhai", … }` was added: an intra-workspace
  # crate can be declared under an alias, so a name-shaped rule silently stops
  # seeing it, and a dependency this gate cannot see is exactly the one that
  # ships still saying `-dev`. Every workspace crate has a `crates/` path and no
  # third-party dependency does, so the path is the honest invariant.
  #
  # `rux-highlight` is unversioned (it is `publish = false`), so the `version = `
  # filter drops it rather than failing it.
  local bad
  bad="$(grep -nE '^[a-z_-]+ = \{ .*path = "crates/' Cargo.toml \
         | grep 'version = ' | grep -v "\"$want\"" || true)"
  [[ -z "$bad" ]] || die "workspace.dependencies disagree with the version:"$'\n'"$bad"
  ok "version is $want, and every workspace dependency agrees"
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

  # A fence that says it is a file has to be that file. `/recipes/message-list/`
  # headed a 53-line abridgement of a 179-line example "The whole file", which
  # was a documentation nit right up until every fence grew a Copy button and a
  # Try it link: both hand the fence to the playground, so a reader copying "the
  # whole file" got an unstyled screen that looks exactly like a broken renderer.
  # Running the page's own code in a desktop window rendered it the same way,
  # which is how it was pinned on the page rather than the runtime.
  ./site/sync-examples.sh --check >/dev/null     || die "a code fence does not match the file it names; run ./site/sync-examples.sh"
  ok "every fence that names a file carries that file"

  # Zola validates `@/page.md` links and ignores absolute ones, which is every
  # link `sync-docs.sh` writes. Eleven dead anchors into `/why/` shipped and
  # stayed live under a green build because of that, so this is checked here
  # instead. See the note at the top of site/check-links.py.
  local py=""
  for candidate in python3 python; do
    command -v "$candidate" >/dev/null 2>&1 && { py="$candidate"; break; }
  done
  if [[ -n "$py" ]]; then
    "$py" site/check-links.py >/dev/null || die "the site has broken internal links; run $py site/check-links.py"
    ok "every internal site link resolves"
  else
    ok "generated site pages match docs/ (no python, link check skipped)"
  fi
}

gate_editor() {
  # The extension's completions come from `rux vocab`, so a property honored in
  # `crates/rux-style` and not regenerated here is a property the editor will
  # not offer. Same class of drift as the docs gate above, and the reason it is
  # a gate at all is that this drift already shipped once: the extension's
  # hand-kept void-tag list missed `<image>` for two releases.
  ./scripts/sync-vocabulary.sh --check >/dev/null \
    || die "editors/vscode/vocabulary.json is stale; run ./scripts/sync-vocabulary.sh"
  # The TextMate grammar has two consumers: VS Code, and Zola's highlighter on
  # the website, which reads its own copy under `site/`. They are the same file
  # and nothing but a line in a README kept them the same file, so a grammar
  # edit that coloured the editor and not the docs stayed invisible until
  # somebody happened to look at a code fence.
  #
  # Compared with the carriage returns stripped, for the reason
  # `sync-vocabulary.sh --check` documents at length: with `core.autocrlf=true`
  # a raw `diff` calls the two copies different on every Windows run.
  if ! diff -u <(tr -d '\r' < editors/vscode/syntaxes/rux.tmLanguage.json) \
               <(tr -d '\r' < site/syntaxes/rux.tmLanguage.json) >/dev/null 2>&1; then
    die "site/syntaxes/rux.tmLanguage.json is stale; copy editors/vscode/syntaxes/rux.tmLanguage.json over it"
  fi
  # Pure functions over a string, no framework and no node_modules. A directory
  # argument does not resolve on Windows, so the files are globbed instead,
  # which also means a new test file runs here without this line being edited.
  if command -v node >/dev/null 2>&1; then
    node --test editors/vscode/test/*.test.js >/dev/null \
      || die "the VS Code extension's tests fail"
    ok "extension vocabulary and grammar current, extension tests pass"
  else
    ok "extension vocabulary and grammar current (node absent, tests skipped)"
  fi
}

gate_build() {
  # `--locked` on purpose: the lockfile pins the workspace members by version
  # too, so dropping `-dev` from the manifest without refreshing the lock
  # leaves the two disagreeing. An ordinary `cargo build` silently rewrites the
  # lock and says nothing, which is exactly how that shipped once.
  local out
  out="$(cargo build --workspace --locked 2>&1)" || {
    echo "$out" | tail -5 >&2
    die "cargo build --workspace --locked failed; the lockfile is stale or a dependency moved"
  }
  local warnings
  warnings="$(echo "$out" | grep -c '^warning' || true)"
  # Print them, do not just count them. A bare count is unactionable when the
  # warning only happens on a platform you are not sitting at: v0.6.0's CI said
  # "emitted 2 warning(s)" from a Linux runner and there was no way to learn
  # what they were without another push.
  if [[ "$warnings" != "0" ]]; then
    echo "$out" | grep -A 6 '^warning' >&2
    die "cargo build emitted $warnings warning(s); releases go out warning-clean"
  fi
  ok "cargo build is warning-clean, and the lockfile matches"
}

# Bring Cargo.lock in step with a version that has just changed in Cargo.toml.
relock() {
  cargo metadata --format-version 1 >/dev/null 2>&1 \
    || die "cargo could not resolve the workspace after the version change"
}

gate_tests() {
  # Output is kept and shown on failure. Swallowing it turned a one-off cold
  # -worktree hiccup into "not green" with nothing to act on, which is worse
  # than the failure itself on a release morning.
  local out
  out="$(cargo test --workspace 2>&1)" || {
    echo "$out" | grep -E '^(error|test result: FAILED|---- .* stdout)' -A 6 | head -40 >&2
    die "cargo test --workspace is not green"
  }
  local count
  count="$(echo "$out" | grep -cE '^test .* ok$' || true)"
  ok "cargo test --workspace green ($count tests)"
}

gate_wasm() {
  # Added after the web build was found broken for three weeks: nothing else in
  # the suite compiles for wasm32, so nothing else notices.
  rustup target list --installed | grep -q wasm32-unknown-unknown \
    || { echo "   -- wasm32 target not installed, skipping"; return; }
  local out
  out="$(cargo check -p rux-web --target wasm32-unknown-unknown 2>&1)" || {
    echo "$out" | grep '^error' -A 6 | head -30 >&2
    die "rux-web does not compile for wasm32; the web build is part of the release"
  }
  ok "the web build compiles"
}

# The gates that are true of a healthy tree on any day, release or not.
run_verify() {
  gate_docs_synced
  gate_editor
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
  # The dependency name is deliberately loose here, so an intra-workspace crate
  # declared under an alias (`rhai = { package = "rux-rhai", … }`) is bumped too.
  # Precision comes from `"$version-dev"` on the right: nothing outside this
  # workspace carries that string.
  sed -i -E "s/^([a-z_-]+ = \{ (package = \"[a-z-]+\", )?version = )\"$version-dev\"/\1\"$version\"/" Cargo.toml
  gate_version "$version"
  relock

  # The vocabulary the extension ships carries the workspace version, so
  # dropping `-dev` makes the committed copy stale by definition. `relock` is
  # here for exactly the same reason and was not enough on its own: the first
  # v0.7.0 freeze packed the capsule, ran the gates, and failed on a file the
  # freeze itself had just invalidated. Anything the version number reaches has
  # to be regenerated between the drop and the gates, not by hand afterwards.
  ./scripts/sync-vocabulary.sh >/dev/null     || die "could not regenerate the extension vocabulary after the version drop"

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
  # The capsule to continue *from* is the release just packed, not the version
  # being opened. Given explicitly, or the newest capsule there is.
  # $1: the dispatcher passes the trailing argument through, and this function
  # is reached with it as its first. Reading $3 made "name a capsule
  # explicitly" unreachable, so it always fell back to the newest one.
  local from_version="${1:-}"
  local from
  if [[ -n "$from_version" ]]; then
    from="release/v$from_version"
  else
    from="$(git for-each-ref --sort=-v:refname --format='%(refname:short)' 'refs/heads/release/v*' | head -1)"
  fi
  [[ -n "$from" ]] && git rev-parse --verify --quiet "$from" >/dev/null \
    || die "no capsule to branch from; freeze a release first, or name one: open-next $next <released-version>"

  local line="build/v${next%.*}"
  git rev-parse --verify --quiet "$line" >/dev/null && die "$line already exists"

  step "opening $line at ${next}-dev, from the $from capsule"
  git switch -qc "$line" "$from"
  sed -i -E "s/^version = \"[0-9.]+\"/version = \"$next-dev\"/" Cargo.toml
  sed -i -E "s/^(rux-[a-z]+ = \{ version = )\"[0-9.]+\"/\1\"$next-dev\"/" Cargo.toml
  gate_version "$next-dev"
  relock
  git commit -aqm "Open the $next line

Branched from the $from capsule so the released tree is the ancestor of what
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
  # $2, not $3: the dispatcher hands this function (version, flag). Reading $3
  # meant `--push` was silently ignored on every release that used it.
  local push="${2:-}"
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
# The twelve published crates, in dependency order. `cargo publish --dry-run`
# resolves against the *real* index, so a crate cannot be checked until
# everything it depends on is genuinely up: publishing is a staircase, not a
# batch, and each layer waits for the one below to appear.
#
# rux-web and rux-highlight are `publish = false` on purpose and are not here.
LAYERS=(
  # rux-rhai sits in the first layer because it depends on no workspace crate:
  # it is a fork of rhai, and its dependencies are all upstream. rux-script
  # depends on it, so it has to be on the index before the second layer runs,
  # which the wait at the end of each layer takes care of.
  "rux-parser rux-reactive rux-text rux-layout rux-fmt rux-rhai"
  "rux-script rux-paint"
  "rux-style"
  "rux-runtime"
  "rux-shell"
  "ruxlang"
)

# Whether this exact version is already on the index. Publishing is a one-way
# door: a version cannot be replaced, only yanked, so a re-run must skip what
# it already did rather than fail on it.
#
# The User-Agent is required, not politeness: crates.io answers 403 to an API
# request that does not identify itself, and curl sends none by default here.
# Without it this function returns false for every crate that exists, which
# makes `wait_for_index` block for five minutes and then declare a crate it had
# just published missing. That is exactly how the v0.6.0 publish stalled after
# its first layer.
UA="rux-release-script (https://ruxlang.dev)"

on_index() {
  local crate="$1" want="$2" code
  code="$(curl -s -A "$UA" -o /dev/null -w '%{http_code}' \
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
  # $2, not $3. The same off-by-one as cmd_ship, and worse here: it made
  # `publish --execute` a dry run that exits 0, so a release looked published
  # and nothing had been.
  local mode="${2:-}"
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

  All twelve are up. Check the rendered page for ruxlang: it is the one crate
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
  open-next) cmd_open_next "${3:-}" ;;
  check)     cmd_check "$version" ;;
  ship)      cmd_ship "$version" "${3:-}" ;;
  publish)   cmd_publish "$version" "${3:-}" ;;
  *) die "usage: release.sh {verify|freeze|open-next|check|ship|publish} [<X.Y.Z>] [--push|--execute]" ;;
esac
