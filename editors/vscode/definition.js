// Go to definition: `use components::task;` and `<task />` both open
// `components/task.rux`.
//
// A Rux app is a tree of files that only ever refer to one another by name, and
// the name is not the path: `use components::crew_detail;` names
// `components/crew_detail.rux` **relative to the importing document**, and the
// tag it contributes is `<crew-detail>`, with the underscore swapped for a
// hyphen. Three spellings of one file. Resolving them by hand is exactly the
// clerical work an editor should be doing.
//
// The rules here are the runtime's (`extract_imports` in `crates/rux-runtime`),
// not a convention: a resolver that guessed would send someone to the wrong
// file, which is worse than not offering to navigate at all.

const fs = require('fs');
const path = require('path');

const context = require('./context');

function register(vscode) {
  return vscode.languages.registerDefinitionProvider('rux', {
    provideDefinition(document, position) {
      if (document.uri.scheme !== 'file') return undefined;
      const text = document.getText();
      const offset = document.offsetAt(position);

      const target = targetAt(text, offset, path.dirname(document.uri.fsPath));
      if (!target) return undefined;

      return new vscode.Location(vscode.Uri.file(target), new vscode.Position(0, 0));
    },
  });
}

/**
 * The file the cursor points at, or `null`.
 *
 * Exported for the tests, which run it against a real directory tree: it is a
 * pure function of the text, the offset and a directory, and testing it through
 * VS Code's API would be testing a mock of the part that is not interesting.
 */
function targetAt(text, offset, dir) {
  const section = context.sectionAt(text, offset);
  if (section === 'script') return fromUseLine(text, offset, dir);
  if (section === 'template') return fromTag(text, offset, dir);
  return null;
}

/** A `use a::b::c;` line: the cursor anywhere in the path opens the file. */
function fromUseLine(text, offset, dir) {
  const lineStart = text.lastIndexOf('\n', offset - 1) + 1;
  let lineEnd = text.indexOf('\n', offset);
  if (lineEnd === -1) lineEnd = text.length;
  const line = text.slice(lineStart, lineEnd);

  const m = /^[ \t]*use[ \t]+([A-Za-z_][A-Za-z0-9_:]*)[ \t]*;/.exec(line);
  if (!m) return null;
  // Only when the cursor is actually on the path, not on the `use` keyword or
  // out past the semicolon.
  const from = lineStart + m.index + m[0].indexOf(m[1]);
  if (offset < from || offset > from + m[1].length) return null;

  return resolve(dir, m[1].split('::'));
}

/**
 * A component tag: `<crew-detail>` opens whichever `use` line contributed it.
 *
 * The tag alone is not enough to find the file, because the hyphen could have
 * come from any of several paths; the `use` line in this document is what says
 * which, and it is the thing the runtime consults too.
 */
function fromTag(text, offset, dir) {
  const at = context.wordAt(text, offset);
  if (!at) return null;
  if (!/<\/?$/.test(text.slice(Math.max(0, at.start - 2), at.start))) return null;

  const component = context.importedComponents(text).find((c) => c.tag === at.word);
  if (!component) return null;
  return resolve(dir, component.file.replace(/\.rux$/, '').split('/'));
}

/**
 * `['components', 'task']` under `dir` is `dir/components/task.rux`.
 *
 * `null` when the file is not there, so a `use` of a file that has not been
 * written yet simply does not navigate, rather than opening an empty editor on
 * a path that does not exist.
 */
function resolve(dir, segments) {
  if (!segments.length) return null;
  const file = path.join(dir, ...segments) + '.rux';
  try {
    return fs.statSync(file).isFile() ? file : null;
  } catch (e) {
    return null;
  }
}

module.exports = { register, targetAt };
