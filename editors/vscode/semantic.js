// Colouring the names inside `view=""` and `to=""`.
//
// Both hold a *reference*, not text. `view="detail"` names a component the same
// way `<detail>` names it, and `to="/task/:id"` names a route somebody declared
// in markup. Painted as ordinary attribute strings, they read like the
// `placeholder="Add new colour"` two lines above them, and the one thing worth
// knowing at a glance — is this a name that resolves — is invisible.
//
// A TextMate grammar cannot answer that: it sees one file at a time and has no
// idea what the `<script>` imported or what `app.rux` declares. So this is a
// semantic token provider, which runs after the grammar and may override it.
// A name that resolves is painted; a name that does not is left exactly as the
// grammar painted it, an ordinary string. That asymmetry is the feature:
// **colour means resolved**, so a typo stays green and says so without a
// squiggle having to appear.
//
// The token type is mapped in `package.json` to `support.class.component`, the
// scope a component tag already carries, so the colour matches `<detail>` in
// whatever theme is in force rather than being guessed at here.

const fs = require('fs');
const path = require('path');

const context = require('./context');
const routes = require('./routes');

/** Our one token type. See `contributes.semanticTokenTypes` in the manifest. */
const LEGEND_TYPES = ['ruxReference'];

/**
 * Every route pattern the project declares, cached per workspace folder.
 *
 * A link is written in a page and the routes are written in `app.rux`, so the
 * open document is usually the wrong place to look. Re-reading every `.rux`
 * file on every keystroke is the wrong answer too, so the scan is cached and
 * thrown away when any `.rux` file is saved, which is when routes change.
 */
class RouteIndex {
  constructor() {
    this.cache = new Map(); // folder -> string[]
  }

  clear() {
    this.cache.clear();
  }

  /**
   * The patterns in force for `document`: its own, or failing that the
   * project's.
   *
   * Its own first, because a document that holds a `<router>` is the authority
   * on its own routes and needs no scan at all.
   */
  forDocument(vscode, document) {
    const own = routes.patternsIn(document.getText());
    if (own.length) return own;
    const folder = vscode.workspace.getWorkspaceFolder(document.uri);
    const root = folder
      ? folder.uri.fsPath
      : document.uri.scheme === 'file'
        ? path.dirname(document.uri.fsPath)
        : null;
    if (!root) return [];
    if (!this.cache.has(root)) this.cache.set(root, scan(root));
    return this.cache.get(root);
  }
}

/**
 * Collect route patterns from the `.rux` files under `root`.
 *
 * Synchronous and shallow on purpose. A semantic token provider is called on a
 * document that is already in memory and is expected to answer quickly, and
 * `node_modules`-sized trees are not what a Rux project looks like. The depth
 * limit is what keeps a stray `target/` directory from turning a keystroke into
 * a filesystem walk.
 */
function scan(root, depth = 0) {
  if (depth > 4) return [];
  let entries;
  try {
    entries = fs.readdirSync(root, { withFileTypes: true });
  } catch (e) {
    return [];
  }
  const found = [];
  for (const entry of entries) {
    const full = path.join(root, entry.name);
    if (entry.isDirectory()) {
      if (entry.name.startsWith('.') || entry.name === 'node_modules' || entry.name === 'target') {
        continue;
      }
      found.push(...scan(full, depth + 1));
      continue;
    }
    if (!entry.name.endsWith('.rux')) continue;
    try {
      found.push(...routes.patternsIn(fs.readFileSync(full, 'utf8')));
    } catch (e) {
      // Unreadable is not an error worth surfacing from a highlighter.
    }
  }
  return found;
}

/**
 * The references in `text` worth colouring, as `{ start, end }` offsets over
 * the value **inside** the quotes.
 *
 * Exported for the tests, which is where the resolution rules are worth
 * pinning: this is a pure function of the text, the imports and the routes, and
 * testing it through VS Code's token encoding would be testing the encoding.
 */
function referencesIn(text, patterns) {
  const found = [];

  const attr = /\b(view|to)\s*=\s*"([^"]*)"/g;
  let m;
  while ((m = attr.exec(text)) !== null) {
    const start = m.index + m[0].indexOf('"') + 1;
    const value = m[2];
    if (!value) continue;
    // Only in markup. A `to` in a `<style>` rule or a `view` in a string
    // literal in `<script>` is not a reference to anything.
    if (context.sectionAt(text, m.index) !== 'template') continue;

    const resolves =
      m[1] === 'view'
        ? // Either spelling. `view="new_task"` and `view="new-task"` are the
          // same component to the runtime, so they have to look the same here.
          context.componentNamed(text, value) !== undefined
        : routes.reaches(patterns, value);
    if (resolves) found.push({ start, end: start + value.length, name: value, attr: m[1] });
  }
  return found;
}

function register(vscode, index) {
  const legend = new vscode.SemanticTokensLegend(LEGEND_TYPES, []);
  return vscode.languages.registerDocumentSemanticTokensProvider(
    'rux',
    {
      provideDocumentSemanticTokens(document) {
        const text = document.getText();
        const builder = new vscode.SemanticTokensBuilder(legend);
        const patterns = index.forDocument(vscode, document);
        for (const ref of referencesIn(text, patterns)) {
          const at = document.positionAt(ref.start);
          // One token per reference, and a reference never wraps a line: an
          // attribute value with a newline in it is not a name.
          builder.push(at.line, at.character, ref.end - ref.start, 0, 0);
        }
        return builder.build();
      },
    },
    legend
  );
}

module.exports = { register, referencesIn, RouteIndex, LEGEND_TYPES };
