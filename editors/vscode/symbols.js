// The outline: the three sections, and inside them the things worth jumping to.
//
// A `.rux` file is one file holding three languages, and it gets long in a way
// a `.js` file of the same size does not: the markup, the rules that style it
// and the state that drives it are all in view at once. Ctrl-Shift-O is the
// cheapest fix for that, and it needs somebody to say what a symbol is here.
//
// What is offered:
//
//   <template>  every element that carries a `class` or an `id`, named the way
//               its rule is written (`view.card`, `text#total`), because that
//               is the string you would search for.
//   <style>     every selector, at the top level and inside `@media`.
//   <script>    `let` bindings and `fn` declarations, plus `mounted` and
//               `unmounted`.
//
// Elements with no class and no id are deliberately absent. An outline listing
// every `<view>` in a template is an outline nobody reads.

const context = require('./context');

function register(vscode) {
  return vscode.languages.registerDocumentSymbolProvider('rux', {
    provideDocumentSymbols(document) {
      const text = document.getText();
      return outline(text).map((section) => toSymbol(vscode, document, section));
    },
  });
}

/**
 * The outline as plain data: `[{ name, kind, start, end, children }]`.
 *
 * Exported, and plain, so the tests can assert on the shape without VS Code.
 */
function outline(text) {
  const sections = [];
  for (const name of ['template', 'style', 'script']) {
    const open = new RegExp(`<${name}[^>]*>`, 'g');
    const m = open.exec(text);
    if (!m) continue;
    const start = m.index;
    const contentStart = m.index + m[0].length;
    const close = text.indexOf(`</${name}>`, contentStart);
    const end = close === -1 ? text.length : close + `</${name}>`.length;

    sections.push({
      name: `<${name}>`,
      kind: 'section',
      start,
      end,
      children: children(name, text, contentStart, close === -1 ? text.length : close),
    });
  }
  return sections;
}

function children(section, text, from, to) {
  const body = text.slice(from, to);
  switch (section) {
    case 'template':
      return elements(body, from);
    case 'style':
      return selectors(body, from);
    case 'script':
      return bindings(body, from);
    default:
      return [];
  }
}

/** Elements carrying a `class` or an `id`, named as their selector would be. */
function elements(body, base) {
  const found = [];
  const tag = /<([A-Za-z][\w.-]*)((?:"[^"]*"|'[^']*'|[^>"'])*)>/g;
  let m;
  while ((m = tag.exec(body)) !== null) {
    const [whole, name, attrs] = m;
    const id = /(?:^|\s)id="([^"]*)"/.exec(attrs);
    const cls = /(?:^|\s)class="([^"]*)"/.exec(attrs);
    if (!id && !cls) continue;

    // `id` first: it is the more specific of the two, and an element carrying
    // both is almost always reached by its id.
    const label = id
      ? `${name}#${id[1].trim()}`
      : `${name}.${cls[1].trim().split(/\s+/).join('.')}`;
    found.push({
      name: label,
      kind: id ? 'field' : 'class',
      start: base + m.index,
      end: base + m.index + whole.length,
      children: [],
    });
  }
  return found;
}

/**
 * Every selector in the sheet.
 *
 * Scanned rather than parsed, and the one thing it must not do is treat a
 * declaration's `{` as a rule's. Nesting is not supported by the runtime, so
 * depth tracking is enough: a `{` at depth 0 opens a rule, and anything inside
 * one is a declaration.
 */
function selectors(body, base) {
  const found = [];
  let depth = 0;
  let from = 0;

  for (let i = 0; i < body.length; i++) {
    const c = body[i];
    if (c === '/' && body[i + 1] === '*') {
      const end = body.indexOf('*/', i + 2);
      i = end === -1 ? body.length : end + 1;
      continue;
    }
    if (c === '{') {
      if (depth === 0) {
        const text = body.slice(from, i).trim();
        if (text) {
          const close = matchingBrace(body, i);
          found.push({
            name: text.replace(/\s+/g, ' '),
            // `@media` opens a block of rules rather than of declarations, so
            // it is a namespace and its rules are its children.
            kind: text.startsWith('@') ? 'namespace' : 'class',
            start: base + from + (body.slice(from, i).length - body.slice(from, i).trimStart().length),
            end: base + close + 1,
            children: text.startsWith('@') ? selectors(body.slice(i + 1, close), base + i + 1) : [],
          });
          if (text.startsWith('@')) {
            i = close;
            from = close + 1;
            continue;
          }
        }
      }
      depth++;
    } else if (c === '}') {
      depth = Math.max(0, depth - 1);
      if (depth === 0) from = i + 1;
    }
  }
  return found;
}

/** The index of the `}` closing the `{` at `open`, or the end of the text. */
function matchingBrace(body, open) {
  let depth = 0;
  for (let i = open; i < body.length; i++) {
    if (body[i] === '{') depth++;
    else if (body[i] === '}') {
      depth--;
      if (depth === 0) return i;
    }
  }
  return body.length - 1;
}

/** `let` bindings, `fn` declarations, and the two lifecycle blocks. */
function bindings(body, base) {
  const found = [];
  const patterns = [
    { re: /^[ \t]*let[ \t]+([A-Za-z_][\w]*)/gm, kind: 'variable', label: (m) => m[1] },
    { re: /^[ \t]*fn[ \t]+([A-Za-z_][\w]*)/gm, kind: 'function', label: (m) => `${m[1]}()` },
    { re: /^[ \t]*(mounted|unmounted)[ \t]*\{/gm, kind: 'event', label: (m) => m[1] },
  ];

  for (const { re, kind, label } of patterns) {
    let m;
    while ((m = re.exec(body)) !== null) {
      found.push({
        name: label(m),
        kind,
        start: base + m.index,
        end: base + m.index + m[0].length,
        children: [],
      });
    }
  }
  // One list in document order, so the outline reads down the file rather than
  // grouping by what the scanner happened to look for first.
  return found.sort((a, b) => a.start - b.start);
}

const KINDS = {
  section: 'Namespace',
  namespace: 'Namespace',
  class: 'Class',
  field: 'Field',
  variable: 'Variable',
  function: 'Function',
  event: 'Event',
};

function toSymbol(vscode, document, node) {
  const range = new vscode.Range(
    document.positionAt(node.start),
    document.positionAt(Math.min(node.end, document.getText().length))
  );
  const symbol = new vscode.DocumentSymbol(
    node.name,
    '',
    vscode.SymbolKind[KINDS[node.kind] || 'Variable'],
    range,
    range
  );
  symbol.children = node.children.map((child) => toSymbol(vscode, document, child));
  return symbol;
}

module.exports = { register, outline };
