// The type table: what `rux check --format json --types` said, read back for
// hover, for completion after a `.`, and for the quick-fix that annotates a
// parameter. The table here is written by hand in the shape the binary prints;
// the binary's side is tested in `crates/rux-script/src/check.rs`.

const test = require('node:test');
const assert = require('node:assert');
const path = require('path');

const types = require('../types');
const completion = require('../completion');
const { paramFixes } = require('../extension');

const FILE = path.resolve('app.rux');

const TASK_FIELDS = [
  { name: 'id', type: 'int', optional: false },
  { name: 'title', type: 'string', optional: false },
  { name: 'note', type: 'string', optional: true },
];

function remember() {
  types.remember(FILE, {
    diagnostics: [],
    types: [
      { file: FILE, line: 5, column: 7, kind: 'let', name: 'sel', path: 'sel', type: 'Task?', nullable: true, fields: TASK_FIELDS },
      { file: FILE, line: 8, column: 5, kind: 'value', name: 't', path: 't', type: 'Task', nullable: false, fields: TASK_FIELDS },
      { file: FILE, line: 8, column: 7, kind: 'field', name: 'title', path: 't.title', type: 'string', nullable: false, fields: [] },
      { file: FILE, line: 9, column: null, kind: 'fn', name: 'shout', path: null, type: 'fn shout(s, n): string', nullable: false, fields: [] },
      { file: FILE, line: 3, column: null, kind: 'value', name: 't', path: 't', type: 'Task', nullable: false, fields: TASK_FIELDS },
    ],
    guesses: [
      { file: FILE, line: 9, function: 'shout', param: 's', type: 'string' },
      { file: FILE, line: 9, function: 'shout', param: 'n', type: 'number' },
    ],
  });
}

test('a name is found on its line, nearest its column', () => {
  remember();
  assert.equal(types.at(FILE, 8, 5, 't').type, 'Task');
  assert.equal(types.at(FILE, 8, 8, 'title').type, 'string');
  // A template piece has no column, so it answers for the whole line.
  assert.equal(types.at(FILE, 3, 40, 't').type, 'Task');
  assert.equal(types.at(FILE, 8, 5, 'nothing'), null);
  assert.equal(types.at(path.resolve('other.rux'), 8, 5, 't'), null);
});

test('a later check of the file replaces what was kept', () => {
  remember();
  types.remember(FILE, { diagnostics: [], types: [], guesses: [] });
  assert.equal(types.at(FILE, 8, 5, 't'), null);
  assert.deepEqual(types.guesses(FILE, 9), []);
});

test('the chain before a dot drops every question mark', () => {
  const at = (src) => types.chainBefore(src, src.length);
  assert.deepEqual(at('x + t.'), { chain: 't', optional: false, dot: 5, typed: '' });
  assert.equal(at('t?.owner?.na').chain, 't.owner');
  assert.equal(at('t?.owner?.na').optional, true);
  assert.equal(at('list[0].'), null);
  assert.equal(at('f().'), null);
});

/** The completion items where `|` sits in `source`, as the editor asks. */
function completeAt(source) {
  const offset = source.indexOf('|');
  const text = source.replace('|', '');
  const vscode = {
    CompletionItem: class {
      constructor(label, kind) {
        this.label = label;
        this.kind = kind;
      }
    },
    CompletionItemKind: new Proxy({}, { get: (_t, k) => String(k) }),
    SnippetString: class {},
    MarkdownString: class {
      constructor(value) {
        this.value = value;
      }
    },
    Range: class {
      constructor(start, end) {
        this.start = start;
        this.end = end;
      }
    },
    TextEdit: { replace: (range, text) => ({ range, text }) },
    languages: { registerCompletionItemProvider: (_l, p) => p },
  };
  const lines = (upTo) => text.slice(0, upTo).split('\n');
  const document = {
    getText: () => text,
    offsetAt: () => offset,
    positionAt: (o) => ({ line: lines(o).length - 1, character: lines(o).pop().length, offset: o }),
    uri: { scheme: 'file', fsPath: FILE },
  };
  return completion.register(vscode).provideCompletionItems(document, {});
}

test('after a dot, a record offers its fields', () => {
  remember();
  const src = '<script>\n\n\n\n\n\n\nfn f(t) { t.| }\n</script>';
  const items = completeAt(src);
  assert.deepEqual(items.map((i) => i.label), ['id', 'title', 'note']);
  assert.equal(items[0].detail, 'id: int');
  assert.equal(items[0].additionalTextEdits, undefined);
  // An optional field is read with `?.`, so choosing it rewrites the dot.
  const note = items[2];
  assert.equal(note.detail, 'note?: string');
  assert.equal(note.additionalTextEdits[0].text, '?.');
  assert.equal(note.additionalTextEdits[0].range.start.offset, src.indexOf('t.|') + 1);
});

test('after a dot on a value that may be null, every field rewrites it', () => {
  remember();
  const items = completeAt('<script>\n\n\n\nlet s = sel.|\n</script>');
  assert.ok(items.every((i) => i.additionalTextEdits && i.additionalTextEdits[0].text === '?.'));
  // Already written `?.`: nothing to rewrite.
  const plain = completeAt('<script>\n\n\n\nlet s = sel?.|\n</script>');
  assert.ok(plain.every((i) => i.additionalTextEdits === undefined));
});

test('the quick-fix writes each guessed type after its parameter', () => {
  remember();
  const text = '<script>\n\n\n\n\n\n\n\nfn shout(s, n: number) { s }\n</script>';
  const inserted = [];
  const vscode = {
    Position: class {
      constructor(line, character) {
        this.line = line;
        this.character = character;
      }
    },
    CodeAction: class {
      constructor(title, kind) {
        this.title = title;
        this.kind = kind;
      }
    },
    CodeActionKind: { QuickFix: 'quickfix' },
    WorkspaceEdit: class {
      insert(_uri, position, value) {
        inserted.push({ ...position, value });
      }
    },
  };
  const document = { uri: { scheme: 'file', fsPath: FILE }, getText: () => text };
  const diagnostic = {
    source: 'rux',
    message: '`s` has no type, so what `shout` is given there is not checked. Annotate it: `fn shout(s: T, n: number)`',
    range: { start: { line: 8 } },
  };
  const actions = paramFixes(vscode, document, { diagnostics: [diagnostic] });
  // `n` is annotated already, so only `s` is offered.
  assert.deepEqual(actions.map((a) => a.title), ['Annotate `s` as `string`, the type its calls hand it']);
  assert.deepEqual(inserted, [{ line: 8, character: 10, value: ': string' }]);
  // Any other warning is left alone.
  assert.deepEqual(paramFixes(vscode, document, { diagnostics: [{ ...diagnostic, message: 'something else' }] }), []);
});
