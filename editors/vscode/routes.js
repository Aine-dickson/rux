// The routes a project declares, and whether a path reaches one.
//
// A `to="/task/7"` is an address, and an address is only meaningful against the
// `<route path="…">` elements somewhere in the app. Those live in the document
// that holds the `<router>`, which is usually not the file being edited: a link
// is written in a page, and the routes are written in `app.rux`. So this scans
// for them rather than reading only the open file.
//
// The rules are the runtime's (`all_route_patterns` and `match_route` in
// `crates/rux-style`): a child path is relative to its parent's, `path=""` is
// the index route standing for the parent's own path, and a `:name` segment
// matches any one segment. Nothing here parses Rux properly, for the reason
// `context.js` gives; it is scanning, and it errs towards finding no routes
// rather than towards inventing one.

/**
 * Every route pattern in one document's text, as the full path it answers to.
 *
 * Returns `[]` for a file with no `<router>`, which is most files.
 */
function patternsIn(text) {
  const router = text.indexOf('<router');
  if (router === -1) return [];
  const end = text.indexOf('</router>', router);
  const body = text.slice(router, end === -1 ? text.length : end);

  // A stack of open `<route>` prefixes, so a child's pattern is built from its
  // ancestors' the way the runtime builds it.
  const out = [];
  const stack = [];
  const tag = /<route\b([^>]*)>|<\/route\s*>/g;
  let m;
  while ((m = tag.exec(body)) !== null) {
    if (m[0].startsWith('</')) {
      stack.pop();
      continue;
    }
    const attrs = m[1];
    const path = /\bpath\s*=\s*"([^"]*)"/.exec(attrs);
    const selfClosing = /\/\s*$/.test(attrs);
    if (!path) {
      // `<route fallback view="…">` has no path. It still opens a level if it
      // is not self-closing, or the stack would unwind against the wrong tag.
      if (!selfClosing) stack.push(stack.length ? stack[stack.length - 1] : '');
      continue;
    }
    const parent = stack.length ? stack[stack.length - 1] : '';
    const pattern = path[1];
    let full;
    if (pattern.startsWith('/')) full = pattern;
    else if (pattern === '') full = parent;
    else full = `${parent.replace(/\/+$/, '')}/${pattern.replace(/^\/+/, '')}`;
    out.push(full);
    if (!selfClosing) stack.push(full);
  }
  return out;
}

/** Split a path into its non-empty segments, as the runtime does. */
function segments(path) {
  return path.split('/').filter((p) => p !== '');
}

/**
 * Whether `path` matches `pattern`, with `:name` taking any one segment.
 *
 * A query or fragment is cut off first: `to="/search?q=rust"` is a link to
 * `/search`, and the router has never matched on anything past the `?`.
 */
function matches(pattern, path) {
  const want = segments(pattern);
  const got = segments(path.split(/[?#]/)[0]);
  if (want.length !== got.length) return false;
  return want.every((w, i) => w.startsWith(':') || w === got[i]);
}

/**
 * Whether any of `patterns` answers to `path`.
 *
 * A `<route fallback>` is deliberately not consulted. It makes every address
 * reachable, which is the right answer for "will this render something" and the
 * wrong one for "did you name a route": a fallback would colour a typo as a
 * destination, and the typo is the thing worth seeing.
 */
function reaches(patterns, path) {
  return patterns.some((p) => matches(p, path));
}

module.exports = { patternsIn, matches, reaches, segments };
