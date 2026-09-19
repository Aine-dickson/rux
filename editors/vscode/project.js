// Where a `use a::b;` actually points, on disk.
//
// One copy, because more than one is how the editor comes to offer an import
// that does not resolve, or to refuse to navigate to a file that does. The
// rules are `resolve_import` and `workspace_root` in `crates/rux-runtime`, and
// this file exists to be the single place they are mirrored.

const fs = require('fs');
const path = require('path');

/** The entry points that mark the top of a project, as `rux run` looks for them. */
const WORKSPACE_ENTRIES = ['app.rux', 'index.rux'];

/**
 * The directory holding this project's entry point, walking up from `from`.
 *
 * Mirrors `workspace_root` in `rux-runtime`. If the two ever disagree the
 * completion list offers imports that do not resolve, which is the one thing
 * this list must never do.
 */
function projectRoot(from) {
  let dir = from;
  for (;;) {
    for (const name of WORKSPACE_ENTRIES) {
      try {
        if (fs.statSync(path.join(dir, name)).isFile()) return dir;
      } catch (e) {
        // not here; keep walking
      }
    }
    const up = path.dirname(dir);
    if (up === dir) return null;
    dir = up;
  }
}

/**
 * The file `segments` names from a document in `dir`, or `null`.
 *
 * Four candidates, in the runtime's order: beside the importing file first,
 * then the project root, and each of those again with the path's underscores
 * as hyphens.
 *
 * **Beside-first** so that an import which resolves today goes on meaning the
 * same file; the root is only ever a fallback. **Exact-before-hyphenated** for
 * the same reason, one level down: a `use` path is script and has to be snake,
 * because `-` is the subtraction operator there, but the file beside it is
 * named by a person who has been writing `<new-task>` all morning. So
 * `use new_task;` finds `new-task.rux`, and nothing that already worked moves.
 */
function resolveImport(dir, segments) {
  if (!segments.length) return null;
  const root = projectRoot(dir);
  const at = (base, parts) => path.join(base, ...parts) + '.rux';
  const swapped = segments.map((s) => s.replace(/_/g, '-'));
  const hyphenated = swapped.join('/') !== segments.join('/');

  const candidates = [at(dir, segments)];
  if (root) candidates.push(at(root, segments));
  if (hyphenated) {
    candidates.push(at(dir, swapped));
    if (root) candidates.push(at(root, swapped));
  }
  for (const file of candidates) {
    try {
      if (fs.statSync(file).isFile()) return file;
    } catch (e) {
      // not there; try the next
    }
  }
  return null;
}

module.exports = { projectRoot, resolveImport, WORKSPACE_ENTRIES };
