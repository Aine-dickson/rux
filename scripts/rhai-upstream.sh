#!/usr/bin/env bash
# Say whether upstream rhai has moved past what the fork has reviewed.
#
# The fork (crates/rux-rhai) receives no upstream fixes by itself, and RustSec
# files advisories under `rhai`, a name the fork does not carry, so neither
# `cargo audit` nor `cargo deny` can ever match one to it. This does it by
# hand: the newest rhai on crates.io against the "Upstream reviewed through"
# line in crates/rux-rhai/DIVERGENCE.md, and any RustSec advisory for `rhai`.
#
#   ./scripts/rhai-upstream.sh     exit 1 if there is something to review
#
# See DIVERGENCE.md, "Keeping up with upstream", for what reviewing means.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
divergence="$repo/crates/rux-rhai/DIVERGENCE.md"
agent="rux-ci (https://ruxlang.dev)"

reviewed="$(sed -n 's/^\*\*Upstream reviewed through: \([0-9.]*\)\*\*$/\1/p' "$divergence" | tr -d '\r')"
if [[ -z "$reviewed" ]]; then
  echo "rhai-upstream: no 'Upstream reviewed through' line in $divergence" >&2
  exit 2
fi

# python3 for the JSON, since it is on every runner and jq is not everywhere.
latest="$(curl -fsS -A "$agent" https://crates.io/api/v1/crates/rhai \
  | python3 -c 'import sys, json; print(json.load(sys.stdin)["crate"]["max_stable_version"])')"

# The advisory database keeps one directory per crate. A 404 is the answer
# "none", not a failure.
advisories="$(curl -sS -A "$agent" \
  https://api.github.com/repos/rustsec/advisory-db/contents/crates/rhai \
  | python3 -c '
import sys, json
data = json.load(sys.stdin)
if isinstance(data, list):
    print("\n".join(entry["name"] for entry in data))
')"

newer="$(python3 -c '
import sys
def key(v): return tuple(int(p) for p in v.split("."))
print("yes" if key(sys.argv[1]) > key(sys.argv[2]) else "no")
' "$latest" "$reviewed")"

echo "rhai on crates.io: $latest; the fork has reviewed through $reviewed"
status=0
if [[ "$newer" == "yes" ]]; then
  echo "upstream has a release nobody has read: review $reviewed..$latest (see DIVERGENCE.md)"
  status=1
fi
if [[ -n "$advisories" ]]; then
  echo "RustSec advisories for rhai (check each against the fork):"
  echo "$advisories" | sed 's/^/  /'
  status=1
fi
exit "$status"
