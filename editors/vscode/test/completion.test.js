// What the completion list offers, per section.
//
// These are regression tests for four things found by writing Rux in the editor
// rather than by reading the code, which is the only way any of them could have
// been found:
//
//   1. All thirty-one snippets were offered in every section, because
//      `package.json` contributed them statically and a static contribution has
//      no idea what a section is. Typing `s` after `justify-content:` produced
//      `script`, `signal`, `slot`, `sticky` and `style` among the four values
//      that were valid, so a working value list looked like a broken one.
//   2. Nothing the document itself declared was ever offered. `let draft =
//      signal("")` on line 4 and no `draft` on line 9.
//   3. Nothing was offered after a `.`, so an element handle from `query()`
//      was a dead end.
//   4. A loop variable was unreachable, which is most of what gets typed
//      inside an `r-for`.

const test = require('node:test');
const assert = require('node:assert');

const completion = require('../completion');

/**
 * Stand-ins for the VS Code constructors the provider uses. Deliberately
 * minimal: what is under test is which names come back, not the editor API.
 */
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
      this.value = value;
    }
  },
  languages: { registerCompletionItemProvider: (_lang, provider) => provider },
};

const provider = completion.register(vscode);

/** The labels offered where `marker` sits in `source`. */
function offeredAt(source, marker) {
  const offset = source.indexOf(marker);
  assert.notEqual(offset, -1, `the marker ${marker} is not in the source`);
  const document = {
    getText: () => source.replace(marker, ''),
    offsetAt: () => offset,
    uri: { scheme: 'untitled' },
  };
  return (provider.provideCompletionItems(document, {}) || []).map((i) => i.label);
}

const DOC = `<template>
  <screen>
    <view r-for="msg in messages" r-key="msg.id">
      <text>{{ IN_INTERP }}</text>
      <view :class="IN_BOUND"></view>
    </view>
  </screen>
</template>

<style>
  .a { justify-content: IN_VALUE }
  AT_RULE_LEVEL
</style>

<script>
  let draft = signal("");
  computed shout = draft;
  fn send() { }
  IN_SCRIPT
</script>`;

test('a CSS value list is values, with no snippets mixed in', () => {
  const offered = offeredAt(DOC, 'IN_VALUE');
  for (const value of ['space-between', 'center', 'flex-start', 'flex-end']) {
    assert.ok(offered.includes(value), `${value} is a justify-content value`);
  }
  // The five that made the list look broken.
  for (const noise of ['script', 'signal', 'slot', 'sticky', 'style']) {
    assert.ok(!offered.includes(noise), `${noise} is a snippet and must not be here`);
  }
});

test('rule level offers whole-rule snippets and not property names', () => {
  const offered = offeredAt(DOC, 'AT_RULE_LEVEL');
  assert.ok(offered.includes('sticky'), 'a whole rule belongs at rule level');
  assert.ok(!offered.includes('signal'), 'a script snippet does not');
  assert.ok(!offered.includes('rux'), 'nor does a whole-document one');
});

test('script offers what the document declared, and ranks it first', () => {
  const offered = offeredAt(DOC, 'IN_SCRIPT');
  for (const name of ['draft', 'shout', 'send']) {
    assert.ok(offered.includes(name), `${name} was declared in this file`);
  }
  assert.ok(offered.includes('query'), 'the runtime globals are still there');
  assert.ok(!offered.includes('view'), 'a template snippet is not');
});

test('an interpolation reaches the document state and the loop variable', () => {
  const offered = offeredAt(DOC, 'IN_INTERP');
  for (const name of ['draft', 'shout', 'send', 'msg']) {
    assert.ok(offered.includes(name), `${name} is in scope in an interpolation`);
  }
});

test('a bound attribute is an expression, like an interpolation', () => {
  const offered = offeredAt(DOC, 'IN_BOUND');
  assert.ok(offered.includes('draft'), ':class takes an expression');
  assert.ok(offered.includes('msg'), 'and the row is in scope');
});

test('an element handle offers its own members and nothing global', () => {
  const source = `<script>
  fn m() {
    let row = query(".r")[0];
    row.HERE
  }
</script>`;
  const offered = offeredAt(source, 'HERE');
  for (const member of ['scrollIntoView', 'tap', 'focus', 'width', 'classes']) {
    assert.ok(offered.includes(member), `${member} is on an element`);
  }
  assert.ok(!offered.includes('navigate'), 'a global is not a member');
});

test('an indexed query result is an element without being bound first', () => {
  const source = `<script>
  fn m() { query(".r")[0].HERE }
</script>`;
  const offered = offeredAt(source, 'HERE');
  assert.ok(offered.includes('scrollIntoView'));
});

test('a dot on anything else offers the string and array methods', () => {
  const source = `<script>
  let name = signal("ada");
  fn m() { name.HERE }
</script>`;
  const offered = offeredAt(source, 'HERE');
  for (const method of ['toUpperCase', 'trim', 'includes', 'length']) {
    assert.ok(offered.includes(method), `${method} is a value method`);
  }
});

// ── The snippet map ──────────────────────────────────────────────────────────

const snippets = require('../snippets');

test('every snippet is placed in a section', () => {
  assert.deepEqual(
    snippets.unplaced(),
    [],
    'a snippet with no section is offered nowhere; add it to SECTION in snippets.js'
  );
});

test('the sections partition the snippets', () => {
  const total = snippets.all().length;
  const placed = ['document', 'template', 'style', 'script']
    .map((s) => snippets.forSection(s).length)
    .reduce((a, b) => a + b, 0);
  assert.equal(placed, total, 'every snippet is in exactly one section');
});

// ── The forms that were wrong ────────────────────────────────────────────────

test('computed and effect are not offered as calls', () => {
  // They were `computed(|| expr)` and `effect(|| …)` in the vocabulary, which
  // is rhai and not Rux: a file written that way fails with "Function not
  // found: computed (Fn)". The Rust side pins this too; this is the editor's
  // half of the same guarantee.
  for (const name of ['computed', 'effect', 'mounted']) {
    const entry = snippets.all().find((s) => s.prefix === name);
    if (!entry) continue;
    assert.ok(
      !/\(\s*\|\|/.test(entry.body),
      `the ${name} snippet reads as a call: ${entry.body}`
    );
  }
});

// ── The vocabulary merge ─────────────────────────────────────────────────────

const vocabulary = require('../vocabulary');

test('a property description says what it does and shows a line to type', () => {
  const completion = require('../completion');
  for (const name of ['display', 'overflow', 'align-items', 'position']) {
    const described = vocabulary.cssProperty(name);
    assert.ok(described, `${name} has no description`);
    assert.ok(
      !/honored/i.test(described.detail),
      `${name}'s one-liner says it is allowed rather than what it does`
    );
    const help = completion.propertyHelp(name, described);
    assert.ok(help.includes('```rux'), `${name}'s popup shows no example`);
    assert.ok(help.includes(name), `${name}'s example does not mention it`);
  }
});

test('an older binary cannot strip a capability the extension shipped with', () => {
  // This is a regression test for a real hour lost. `current()` was
  // `live || baked`: all or nothing. `cssPropertyDocs` was added to the
  // extension and to `rux vocab` in the same change, but the `rux` on PATH had
  // been built earlier. It emitted every *other* field, so it won outright, the
  // bundled descriptions were discarded, and every property fell back to
  // "honored by the runtime" with no way to tell why.
  const before = vocabulary.cssProperty('display');
  assert.ok(before, 'the bundled vocabulary describes display');

  // A binary that knows about elements but not about property docs.
  vocabulary.__setLiveForTest({
    version: '0.6.0',
    elements: [{ name: 'view', detail: 'a box', doc: 'x' }],
    cssProperties: ['display', 'width'],
  });
  try {
    assert.deepEqual(
      vocabulary.elements().map((e) => e.name),
      ['view'],
      'the binary still wins for what it does say'
    );
    assert.deepEqual(vocabulary.cssProperties(), ['display', 'width']);
    assert.ok(
      vocabulary.cssProperty('display'),
      'a field the binary never mentioned must survive from the bundled copy'
    );
  } finally {
    vocabulary.__setLiveForTest(null);
  }
});

// ── What a dot may offer ─────────────────────────────────────────────────────

const locals = require('../locals');

const TYPED = `<script>
    let items = signal([]);
    let name = signal("ada");
    let count = signal(0);
    let handle = setInterval(2000){
    };
    let row = query(".r")[0];
    HERE
</script>`;

test('a dot on a value of unknown type offers nothing at all', () => {
  // Found by a user within minutes of getting the build: `let handle =
  // setInterval(2000) { … }` holds a timer handle, and the list offered
  // `charAt`, `map` and `join` on it. Guessing "probably a string or an array"
  // endorses calls that cannot work, which is the same failure as offering an
  // unhonored CSS property.
  assert.deepEqual(offeredAt(TYPED.replace('HERE', 'handle.HERE'), 'HERE'), []);
  assert.deepEqual(offeredAt(TYPED.replace('HERE', 'count.HERE'), 'HERE'), []);
});

test('a dot offers only the methods that receiver actually has', () => {
  const onArray = offeredAt(TYPED.replace('HERE', 'items.HERE'), 'HERE');
  assert.ok(onArray.includes('map'), 'map is an array method');
  assert.ok(!onArray.includes('charAt'), 'charAt is not');

  const onString = offeredAt(TYPED.replace('HERE', 'name.HERE'), 'HERE');
  assert.ok(onString.includes('toUpperCase'), 'toUpperCase is a string method');
  assert.ok(!onString.includes('map'), 'map is not');

  const onElement = offeredAt(TYPED.replace('HERE', 'row.HERE'), 'HERE');
  assert.ok(onElement.includes('scrollIntoView'), 'an element handle');
  assert.ok(!onElement.includes('map'), 'and not an array');
});

test('what a name holds is read from its declaration', () => {
  assert.equal(locals.receiverKind(TYPED, 'items'), 'array');
  assert.equal(locals.receiverKind(TYPED, 'name'), 'string');
  assert.equal(locals.receiverKind(TYPED, 'row'), 'element');
  assert.equal(locals.receiverKind(TYPED, 'count'), null, 'a number has no members here');
  assert.equal(locals.receiverKind(TYPED, 'handle'), null, 'a timer handle is opaque');
  assert.equal(locals.receiverKind(TYPED, 'nope'), null, 'an undeclared name');
});

// ── Activation ───────────────────────────────────────────────────────────────

const fs = require('fs');
const path = require('path');

test('the extension activates when a .rux file is opened', () => {
  // The bug that made this whole session confusing. `activationEvents` was
  // absent from `package.json` — and had been since 0.3.0, the version on the
  // Marketplace. With no activation event VS Code never calls `activate()` on
  // opening a `.rux` file, so completions, hover, diagnostics and formatting
  // were all dead until the user happened to invoke a command.
  //
  // It was invisible because the *declarative* contributions do not need
  // activation: the grammar coloured the file, and the static snippets answered
  // completions well enough to look like a working, if odd, extension. Removing
  // the static snippets took even that away.
  const pkg = JSON.parse(
    fs.readFileSync(path.join(__dirname, '..', 'package.json'), 'utf8')
  );
  assert.ok(
    Array.isArray(pkg.activationEvents),
    'package.json declares no activationEvents, so activate() never runs'
  );
  assert.ok(
    pkg.activationEvents.includes('onLanguage:rux'),
    `activationEvents must include onLanguage:rux, got ${JSON.stringify(pkg.activationEvents)}`
  );
  assert.ok(pkg.main, 'and there must be a main for it to activate');
});

test('the run and check commands are contributed with menus', () => {
  const pkg = JSON.parse(
    fs.readFileSync(path.join(__dirname, '..', 'package.json'), 'utf8')
  );
  const commands = pkg.contributes.commands.map((c) => c.command);
  assert.ok(commands.includes('rux.run'), 'a right-click Run needs a command');
  assert.ok(commands.includes('rux.check'));

  const menus = pkg.contributes.menus || {};
  const inEditor = (menus['editor/context'] || []).map((m) => m.command);
  const inExplorer = (menus['explorer/context'] || []).map((m) => m.command);
  assert.ok(inEditor.includes('rux.run'), 'right-click in the editor');
  assert.ok(inExplorer.includes('rux.run'), 'right-click in the explorer');
});

// ── Hover coverage ───────────────────────────────────────────────────────────

const hover = require('../hover');
const contextModule = require('../context');

const HOVER_DOC = `<script>
    let xs = signal([]);
    let label = signal("hi");
    computed total = xs.reduce(|acc, x| acc + x, 0);
    computed shown = xs.map(a => { a.length });
    fn go() { return 1; }
</script>`;

function hoverAt(source, needle, plus) {
  const idx = source.indexOf(needle) + (plus || 0);
  const at = contextModule.memberAt(source, idx);
  return at ? hover.lookUp('script', source, at) : null;
}

test('the script keywords explain themselves', () => {
  // `let` had no hover at all, which is a strange gap for the first keyword
  // anyone types, and the one whose meaning differs most from the languages it
  // is borrowed from: a top-level `let` here is a signal.
  for (const word of ['let', 'fn', 'computed', 'return']) {
    const found = hoverAt(HOVER_DOC, word, 1);
    assert.ok(found, `\`${word}\` explains nothing`);
  }
  assert.match(hoverAt(HOVER_DOC, 'let', 1).doc, /signal/i, '`let` must mention what it really declares');
});

test('a declared name reports what it holds', () => {
  const xs = hoverAt(HOVER_DOC, 'let xs', 4);
  assert.match(xs.detail, /array/, 'inferred from the initialiser');
  const label = hoverAt(HOVER_DOC, 'let label', 4);
  assert.match(label.detail, /string/);
  assert.doesNotMatch(xs.detail, /a array/, 'and reads as English');
});

test('a lambda parameter is explained, with its role', () => {
  // `search_item.map(a => { a.length })` answered nothing for `a`, which is the
  // obvious question in that position.
  const acc = hoverAt(HOVER_DOC, '|acc', 1);
  assert.match(acc.doc, /accumulator/, "reduce's first parameter");
  const x = hoverAt(HOVER_DOC, ', x|', 2);
  assert.match(x.doc, /one item/i, "reduce's second is the item, not an index");
  const a = hoverAt(HOVER_DOC, '(a =>', 1);
  assert.match(a.doc, /one item/i);
  assert.match(a.detail, /map/, 'and names the method it belongs to');
});

test('length is not described as an element property', () => {
  // It was filed under the element handle, so hovering `a.length` on an
  // ordinary array explained `query()`.
  const found = hoverAt(HOVER_DOC, 'a.length', 3);
  assert.ok(found, 'length explains nothing');
  assert.doesNotMatch(found.doc, /query\(\)/, 'length is not about query()');
  const elementProps = vocabulary.elementMembers().map((m) => m.name);
  assert.ok(!elementProps.includes('length'), 'and it is not in the element list');
});

// ── The scanner does not go blind partway down a file ────────────────────────
//
// Brace depth is how a declaration inside a function body is kept out of the
// list. It was counted on the raw line, so a brace inside a *string* counted
// too, and one `let tpl = "a { b";` left the scanner one level deep for the
// rest of the file: every name below it vanished from completions, from hover,
// and from the outline, with nothing on screen to say why.
//
// This is the shape of failure that is indistinguishable from a dead extension,
// which is what makes it worth a test of its own rather than a line in another.

test('a brace inside a string does not hide the rest of the file', () => {
  const withBrace = `<script>
    let tpl = "a { b";
    let draft = signal("");
    fn send() { print(draft); }
    computed n = draft.length;
</script>`;
  const found = locals.declarations(withBrace).map((d) => d.name);
  assert.deepEqual(found, ['tpl', 'draft', 'send', 'n']);
});

test('a lone closing brace in a string does not lift the scanner out of a body', () => {
  // The other direction: an unmatched `}` in a string must not cancel a real
  // `{`, or a function's locals leak into the top-level list.
  const source = `<script>
    fn f() {
      let close = "}";
      let inner = 1;
    }
    let top = signal(0);
</script>`;
  const found = locals.declarations(source).map((d) => d.name);
  assert.deepEqual(found, ['f', 'top'], 'inner and close are scoped to the body');
});

test('an escaped quote does not end the string it is inside', () => {
  const source = `<script>
    let quoted = "say \\" { ";
    let after = signal(0);
</script>`;
  const found = locals.declarations(source).map((d) => d.name);
  assert.ok(found.includes('after'), 'the file did not go quiet');
});
