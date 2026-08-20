// The same documents, with Windows line endings.
//
// Every fixture in this suite was written with `\n`, and every one of them
// passed, for two sessions, while the extension answered nothing at all in the
// editor. Rux is mostly written on Windows; VS Code hands the provider whatever
// the file has; and `.rux` files on this machine are CRLF.
//
// What broke: the declaration patterns in `locals.js` are anchored with `$`,
// and JavaScript's `$` without the `m` flag matches at the end of the string or
// before a final `\n`, never before a `\r`. `.` does not match a `\r` either,
// so the lazy group in the `let` pattern could not step over one to reach the
// anchor. Every declaration in a CRLF file failed to match. `declarations()`
// returned `[]`, which is indistinguishable from a file that declares nothing,
// so completion offered only the globals, hover answered nothing on a name the
// author had just written, and no error was raised anywhere.
//
// So these are not "an edge case with line endings". This is the file format
// the users of this extension actually have, and a fixture set that only covers
// `\n` is a fixture set that does not cover them.

const test = require('node:test');
const assert = require('node:assert');

const locals = require('../locals');
const hover = require('../hover');
const completion = require('../completion');
const context = require('../context');

const vscode = {
  CompletionItem: class {
    constructor(label, kind) {
      this.label = label;
      this.kind = kind;
    }
  },
  CompletionItemKind: new Proxy({}, { get: (_t, k) => String(k) }),
  SnippetString: class {
    constructor(value) {
      this.value = value;
    }
  },
  MarkdownString: class {
    constructor(value) {
      this.value = value || '';
    }
    appendMarkdown(value) {
      this.value += value;
      return this;
    }
  },
  Range: class {},
  Hover: class {
    constructor(md) {
      this.md = md;
    }
  },
  languages: {
    registerCompletionItemProvider: (_l, p) => p,
    registerHoverProvider: (_l, p) => p,
  },
};

const completions = completion.register(vscode);
const hovers = hover.register(vscode);

const SOURCE = `<template>
  <view class="header">
    <input r-model="search_item"></input>
    <text>{{ label }}</text>
    <view r-for="row in rows" r-key="row.id">{{ row.name }}</view>
  </view>
</template>
<script>
  let search_item = signal([]);
  let label = signal("hi");
  let rows = signal([]);
  let handle = setInterval(2000) {
  };
  fn clear() {
    let scratch = 1;
  }
  computed total = rows.length;
</script>
`;

const crlf = (s) => s.replace(/\n/g, '\r\n');

/** Both spellings of the same document, so a failure names which one broke. */
const endings = [
  ['LF', SOURCE],
  ['CRLF', crlf(SOURCE)],
];

for (const [name, text] of endings) {
  test(`${name}: every declaration is found, and nothing from a function body`, () => {
    const found = locals.declarations(text).map((d) => d.name);
    assert.deepEqual(found, [
      'search_item', 'label', 'rows', 'handle', 'clear', 'total',
    ]);
    assert.ok(!found.includes('scratch'), 'a local inside `fn` is not top-level');
  });

  test(`${name}: an initialiser is captured, so a type can be inferred from it`, () => {
    // The `$` anchor that CRLF defeated is in the *signal* pattern; without it
    // a declaration fell through to the plain-binding pattern and lost the fact
    // that it was a signal, which is what `.` completion keys off.
    const declared = locals.declaration(text, 'search_item');
    assert.equal(declared.kind, 'signal', 'recognised as a signal, not a plain let');
    assert.equal(declared.type, 'array', 'and its initialiser was read');
    assert.equal(locals.declaration(text, 'label').type, 'string');
  });

  test(`${name}: completion in <script> offers what the file declared`, () => {
    const offset = text.indexOf('computed total');
    const document = {
      getText: () => text,
      offsetAt: () => offset,
      uri: { scheme: 'untitled' },
    };
    const offered = (completions.provideCompletionItems(document, {}) || []).map(
      (i) => i.label
    );
    for (const declared of ['search_item', 'label', 'rows', 'handle', 'clear']) {
      assert.ok(offered.includes(declared), `${declared} is offered`);
    }
    assert.ok(offered.includes('setInterval'), 'and the globals are still there');
  });

  test(`${name}: hover answers on a name the document declared`, () => {
    const at = context.memberAt(text, text.indexOf('rows.length') + 2);
    const found = hover.lookUp('script', text, at);
    assert.ok(found, 'hover says nothing about `rows`');
    assert.match(found.detail, /signal/);
    assert.match(found.doc, /line \d+ of this file/, 'and says where it is');
  });

  test(`${name}: hover answers inside a template expression too`, () => {
    const at = context.memberAt(text, text.indexOf('r-model="search_item"') + 10);
    const found = hover.lookUp('template', text, at);
    assert.ok(found, 'hover says nothing about a bound signal');
    assert.match(found.detail, /signal/);
  });

  test(`${name}: a loop variable is still scoped to its element`, () => {
    const offset = text.indexOf('{{ row.name }}') + 4;
    const names = locals.loopVariables(text, offset).map((v) => v.name);
    assert.deepEqual(names, ['row']);
  });
}

test('the two spellings agree on every name they find', () => {
  // The general form of the assertion, so a future line-oriented scanner is
  // caught by this file rather than by a user.
  const a = JSON.stringify(locals.declarations(SOURCE));
  const b = JSON.stringify(locals.declarations(crlf(SOURCE)));
  assert.equal(a, b, 'CRLF and LF disagree about what the document declares');
});
