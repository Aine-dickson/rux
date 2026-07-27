// Rux VS Code extension: a basic, bracket/tag-aware re-indenter.
//
// This is deliberately NOT the real formatter. It only fixes *indentation* by
// tracking nesting depth across tags (<x> / </x>), braces, brackets and parens.
// It does not touch spacing inside a line, wrap, or reorder anything. The real
// `rux fmt` (parse -> pretty-print, via rux-parser/rux-style/rux-script) is the
// planned Tier-1 replacement; see docs/06-roadmap.md "Dev tooling".

// Tags that never nest, so they must not increase indent.
//
// `image` is the one that matters: this list started as HTML's, which has `img`,
// but Rux's element is `<image>`. Without it, an `<image src="...">` written
// without a self-closing slash over-indents everything after it. Keep in step
// with VOID_TAGS in crates/rux-fmt/src/lib.rs, which is what the web playground
// uses.
const VOID_TAGS = new Set([
  'image', 'input',
  'area', 'base', 'br', 'col', 'embed', 'hr', 'img',
  'link', 'meta', 'param', 'source', 'track', 'wbr',
]);

// Blank out string and comment contents so brackets inside them aren't counted.
function sanitize(line) {
  return line
    .replace(/<!--.*?-->/g, '')
    .replace(/\/\*.*?\*\//g, '')
    // An unterminated block comment: strip from its opener to end of line, so
    // brackets living inside the comment prose aren't counted as nesting.
    .replace(/<!--.*$/, '')
    .replace(/\/\*.*$/, '')
    .replace(/"(?:\\.|[^"\\])*"/g, '""')
    .replace(/'(?:\\.|[^'\\])*'/g, "''")
    .replace(/\/\/.*$/, '');
}

// Ordered nesting tokens on a (sanitized) line: +1 open, -1 close, 0 neutral.
const TOKEN = /<\/[a-zA-Z][\w.-]*\s*>|<[a-zA-Z][\w.-]*(?:\s[^<>]*?)?\/>|<([a-zA-Z][\w.-]*)(?:\s[^<>]*?)?>|[{[(]|[}\])]/g;

function classify(line) {
  const tokens = [];
  let m;
  TOKEN.lastIndex = 0;
  while ((m = TOKEN.exec(line)) !== null) {
    const s = m[0];
    if (s.startsWith('</')) tokens.push(-1);
    else if (s.endsWith('/>')) tokens.push(0);
    else if (s[0] === '<') tokens.push(VOID_TAGS.has((m[1] || '').toLowerCase()) ? 0 : 1);
    else if (s === '{' || s === '[' || s === '(') tokens.push(1);
    else tokens.push(-1);
  }
  let delta = 0;
  for (const t of tokens) delta += t;
  // How many closers lead the line (they dedent this line itself).
  let leadingClose = 0;
  for (const t of tokens) {
    if (t === -1) leadingClose++;
    else break;
  }
  return { delta, leadingClose };
}

// Does an unterminated block comment open on this line? Returns its closing
// delimiter, or null. (Complete comments on the line are removed first.)
function opensComment(line) {
  const s = line.replace(/<!--.*?-->/g, '').replace(/\/\*.*?\*\//g, '');
  if (s.lastIndexOf('<!--') !== -1) return '-->';
  if (s.lastIndexOf('/*') !== -1) return '*/';
  return null;
}

function reindent(text, unit) {
  const eol = text.includes('\r\n') ? '\r\n' : '\n';
  const lines = text.split(/\r?\n/);
  const out = [];
  let depth = 0;
  let inComment = null; // pending closing delimiter while inside a multi-line comment
  for (const raw of lines) {
    // Inside a multi-line comment: preserve the author's alignment verbatim.
    if (inComment) {
      out.push(raw);
      if (raw.indexOf(inComment) !== -1) inComment = null;
      continue;
    }
    const trimmed = raw.trim();
    if (trimmed === '') { out.push(''); continue; }
    const { delta, leadingClose } = classify(sanitize(trimmed));
    const indent = Math.max(0, depth - leadingClose);
    out.push(unit.repeat(indent) + trimmed);
    depth = Math.max(0, depth + delta);
    inComment = opensComment(trimmed);
  }
  return out.join(eol);
}

function activate(context) {
  const vscode = require('vscode');
  const provider = {
    provideDocumentFormattingEdits(document, options) {
      const unit = options.insertSpaces ? ' '.repeat(options.tabSize) : '\t';
      const formatted = reindent(document.getText(), unit);
      const full = new vscode.Range(
        document.positionAt(0),
        document.positionAt(document.getText().length)
      );
      return [vscode.TextEdit.replace(full, formatted)];
    },
  };
  context.subscriptions.push(
    vscode.languages.registerDocumentFormattingEditProvider('rux', provider)
  );
}

function deactivate() {}

module.exports = { activate, deactivate, reindent };
