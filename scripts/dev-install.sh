#!/usr/bin/env bash
#
# Put a coherent set of Rux dev tools on this machine, in one step.
#
#   ./scripts/dev-install.sh            build, install, package, install
#   ./scripts/dev-install.sh --check    report what is installed and whether it is current
#
# ## Why this exists
#
# Editor support has **four** moving parts, and they go stale independently:
#
#   1. `~/.cargo/bin/rux`              what the editor shells out to
#   2. `editors/vscode/vocabulary.json` what the extension ships with
#   3. the packaged `.vsix`             what is actually installed
#   4. the running extension host       what is actually loaded
#
# Every one of those was stale at some point during the 0.4.x work, each time
# producing "the fix does not work" against a fix that was real:
#
# - The extension prefers the **live** `rux vocab` per field, so a `rux` on PATH
#   built before a vocabulary change silently overrode the corrected copy the
#   extension shipped with. `./scripts/sync-vocabulary.sh` rebuilds
#   `target/debug/rux` and **not** the one on PATH, so running it is not enough.
# - Reinstalling a `.vsix` with the **same version number** does not reliably
#   replace a running extension, and `code --uninstall-extension` can leave the
#   directory behind, which then shows up as a second copy.
# - A grammar change needs a full editor restart, not a window reload.
#
# So this script does all of it, bumps the patch version so the install cannot
# be confused with a cached one, and prints what to do by hand at the end,
# because the last step is not scriptable.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ext="$repo/editors/vscode"

# Git Bash hands out `/c/Users/...`, which the Windows Python behind `python3`
# cannot open. `cygpath` is the translation, and is absent on Linux, where the
# path is already right.
winpath() {
  if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else printf '%s' "$1"; fi
}

version_of() {
  python3 -c "import json,io,sys;print(json.load(io.open(sys.argv[1],encoding='utf-8'))['version'])" "$(winpath "$1")"
}

# The directory VS Code actually loads, read from its own `extensions.json`
# rather than from the directory listing.
#
# The listing was the first answer and it was wrong in the one case that
# matters. An install leaves the previous version's directory behind, so
# `ls … | head -n1` returned `ruxlang.ruxlang-0.4.8` while the editor was
# running 0.4.9, and `--check` reported the machine as stale when it was
# current. A check that reports the wrong version is worse than no check: this
# is the file everyone turns to when a fix "did not land".
installed_dir() {
  local registered
  registered="$(python3 - "$(winpath "$HOME/.vscode/extensions/extensions.json")" <<'PY' 2>/dev/null || true
import io, json, sys
try:
    entries = json.load(io.open(sys.argv[1], encoding='utf-8'))
except Exception:
    sys.exit(0)
for e in entries:
    if e.get('identifier', {}).get('id', '').lower() == 'ruxlang.ruxlang':
        print(e.get('relativeLocation') or '')
        break
PY
)"
  if [[ -n "$registered" && -d "$HOME/.vscode/extensions/$registered" ]]; then
    printf '%s' "$HOME/.vscode/extensions/$registered"
    return
  fi
  # Not registered: fall back to the newest directory on disk, which is at
  # least more likely to be the current one than the alphabetically first.
  ls -dt "$HOME/.vscode/extensions/ruxlang.ruxlang-"* 2>/dev/null | head -n1 || true
}

# Every ruxlang directory except the one VS Code has registered. Each is a
# previous install that was never removed, and each is a thing that can be
# mistaken for the live one.
leftover_dirs() {
  local live
  live="$(installed_dir)"
  local d
  for d in "$HOME/.vscode/extensions/ruxlang.ruxlang-"*; do
    [[ -d "$d" ]] || continue
    [[ "$d" == "$live" ]] && continue
    printf '%s
' "$d"
  done
}

report() {
  echo "── what is on this machine ─────────────────────────────────────────"
  if command -v rux >/dev/null 2>&1; then
    echo "  rux on PATH         $(rux --version 2>/dev/null || echo '?')  [$(command -v rux)]"
  else
    echo "  rux on PATH         NOT INSTALLED"
  fi
  echo "  workspace version   $(grep -m1 -E '^version[[:space:]]*=' "$repo/Cargo.toml" | cut -d'"' -f2)"
  echo "  extension source    $(version_of "$ext/package.json")"
  local dir
  dir="$(installed_dir)"
  if [[ -n "$dir" ]]; then
    echo "  extension installed $(version_of "$dir/package.json")  [$(basename "$dir")]"
  else
    echo "  extension installed NONE"
  fi
  local stale_dirs
  stale_dirs="$(leftover_dirs)"
  if [[ -n "$stale_dirs" ]]; then
    echo "  left behind         $(echo "$stale_dirs" | xargs -n1 basename | tr '
' ' ')"
    echo "                      (not loaded, but they are what makes a listing lie)"
  fi

  # The check that actually mattered: does the binary the editor will call agree
  # with the vocabulary the extension ships?
  if command -v rux >/dev/null 2>&1 && rux --help 2>/dev/null | grep -q vocab; then
    if diff -q <(rux vocab | tr -d '\r') <(tr -d '\r' < "$ext/vocabulary.json") >/dev/null 2>&1; then
      echo "  vocabulary          the PATH binary and the extension agree"
    else
      echo "  vocabulary          ** DISAGREE ** - the PATH binary is stale, or the extension is"
      echo "                      Run this script with no arguments to fix it."
    fi
  fi
  echo "────────────────────────────────────────────────────────────────────"
}

if [[ "${1:-}" == "--check" ]]; then
  report
  exit 0
fi

echo "1/5  building and installing the rux binary onto PATH"
cargo install --path "$repo/crates/rux-cli" --force --quiet

echo "2/5  regenerating the extension's vocabulary from it"
"$repo/scripts/sync-vocabulary.sh" >/dev/null

echo "3/5  bumping the extension's patch version"
# A fresh version every time, because reinstalling the same one does not
# reliably replace what is running.
python3 - "$ext/package.json" <<'PY'
import io, json, sys
p = sys.argv[1]
d = json.load(io.open(p, encoding='utf-8'))
major, minor, patch = (int(x) for x in d['version'].split('.'))
d['version'] = f"{major}.{minor}.{patch + 1}"
io.open(p, 'w', encoding='utf-8', newline='\n').write(json.dumps(d, indent=2, ensure_ascii=False) + '\n')
print(f"     {d['version']}")
PY

echo "4/5  packaging"
new_version="$(version_of "$ext/package.json")"
(cd "$ext" && mkdir -p superseded && mv -f ./*.vsix superseded/ 2>/dev/null || true)
(cd "$ext" && npx --yes @vscode/vsce package >/dev/null)

echo "5/5  installing, after removing every trace of the old one"
code --uninstall-extension ruxlang.ruxlang >/dev/null 2>&1 || true
# `code --uninstall-extension` reports success and can leave the directory in
# place; an orphaned directory shows up in the Extensions view as a second copy.
rm -rf "$HOME/.vscode/extensions/ruxlang.ruxlang-"* 2>/dev/null || true
code --install-extension "$(winpath "$ext/ruxlang-$new_version.vsix")" >/dev/null 2>&1

echo
report
echo
echo "Now QUIT VS Code completely and reopen it."
echo "Reload Window reuses the extension host and does not reliably pick up a"
echo "new grammar. Then check with: Rux: Diagnose Editor Support At Cursor"
