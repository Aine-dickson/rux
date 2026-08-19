// Tests for the scanning the editor features are built on.
//
// Run with `node --test editors/vscode/test`. There is no test framework and no
// `node_modules`: these are pure functions over a string, and the point of
// keeping them pure was so they could be tested without booting VS Code.
//
// What is worth testing here is the set of things a half-typed document does
// that a valid one never does, because that is all an editor ever sees.

const test = require('node:test');
const assert = require('node:assert');

const context = require('../context');
const vocabulary = require('../vocabulary');

const isVoid = vocabulary.isVoid;

// ── sections ─────────────────────────────────────────────────────────────────

test('the three sections are told apart', () => {
  const src = '<template>\n  <view></view>\n</template>\n<style>\n.a { }\n</style>\n<script>\nlet x = 1;\n</script>\n';
  assert.equal(context.sectionAt(src, src.indexOf('<view')), 'template');
  assert.equal(context.sectionAt(src, src.indexOf('.a')), 'style');
  assert.equal(context.sectionAt(src, src.indexOf('let x')), 'script');
  assert.equal(context.sectionAt(src, 0), null, 'before any section');
});

test('an unclosed section still reports its contents', () => {
  // Half-typed is the normal case in an editor: the closing tag arrives later.
  const src = '<template>\n  <view\n';
  assert.equal(context.sectionAt(src, src.length), 'template');
});

// ── imported components ──────────────────────────────────────────────────────

test('use statements become tags, with underscores hyphenated', () => {
  const src =
    '<template><view></view></template>\n<script>\nuse components::stat;\nuse components::crew_detail;\n</script>';
  const found = context.importedComponents(src);
  assert.deepEqual(
    found.map((c) => c.tag),
    ['stat', 'crew-detail'],
    'the runtime maps `crew_detail` to `<crew-detail>`; offering `crew_detail` would render nothing'
  );
  assert.equal(found[1].file, 'components/crew_detail.rux');
});

test('a use written in the template is not an import', () => {
  const src = '<template><text>use components::stat;</text></template>';
  assert.deepEqual(context.importedComponents(src), []);
});

// ── open tags ────────────────────────────────────────────────────────────────

test('a tag name being typed is distinguished from its attributes', () => {
  const src = '<template><vi';
  const at = context.openTagAt(src, src.length);
  assert.deepEqual(at, { tag: 'vi', onName: true, prefix: 'vi' });

  const withAttr = '<template><view cl';
  const attr = context.openTagAt(withAttr, withAttr.length);
  assert.equal(attr.tag, 'view');
  assert.equal(attr.onName, false);
});

test('inside an attribute value is not an attribute position', () => {
  // Offering `r-for` in the middle of `class="` would be noise.
  const src = '<template><view class="ca';
  assert.equal(context.openTagAt(src, src.length), null);
});

test('a closed tag is not an open one', () => {
  const src = '<template><view class="a">';
  assert.equal(context.openTagAt(src, src.length), null);
});

// ── the closing-tag stack ────────────────────────────────────────────────────

test('the innermost open tag is the one offered', () => {
  const src = '<template><screen><view><text>hi';
  assert.equal(context.unclosedTagAt(src, src.length, isVoid), 'text');
});

test('a self-closed tag never goes on the stack', () => {
  const src = '<template><view><image src="a.png" />';
  assert.equal(context.unclosedTagAt(src, src.length, isVoid), 'view');
});

test('a void tag written without a slash still never goes on the stack', () => {
  // This is the `<image>` bug, in the shape it would take here: writing
  // `</image>` produces a document the parser rejects.
  const src = '<template><view><image src="a.png">';
  assert.equal(
    context.unclosedTagAt(src, src.length, isVoid),
    'view',
    '`<image>` never nests, so the enclosing tag is the right answer'
  );
});

test('a closed tag pops the stack', () => {
  const src = '<template><screen><view></view>';
  assert.equal(context.unclosedTagAt(src, src.length, isVoid), 'screen');
});

test('the section tags are not content tags', () => {
  const src = '<template><view>';
  assert.equal(context.unclosedTagAt(src, src.length, isVoid), 'view');
});

// ── CSS positions ────────────────────────────────────────────────────────────

test('property names are offered inside a rule and not in its selector', () => {
  const src = '<style>\n.card { dis';
  assert.equal(context.inCssDeclaration(src, src.length), true);

  const selector = '<style>\n.car';
  assert.equal(context.inCssDeclaration(selector, selector.length), false);
});

test('a property value is not a property name position', () => {
  const src = '<style>\n.card { display: fl';
  assert.equal(context.inCssDeclaration(src, src.length), false);
  const next = '<style>\n.card { display: flex; ali';
  assert.equal(context.inCssDeclaration(next, next.length), true, 'the `;` starts a new declaration');
});

// ── `use` paths ──────────────────────────────────────────────────────────────

const completion = require('../completion');

test('a use path is recognised at every stage of typing', () => {
  const at = (line) => {
    const src = `<template></template>\n<script>\n${line}`;
    return completion.usePathBeing(src, src.length);
  };
  assert.equal(at('  use '), '', 'right after the keyword');
  assert.equal(at('  use comp'), 'comp', 'part-way through the first segment');
  assert.equal(at('  use components::'), 'components::', 'after the separator');
  assert.equal(at('  use components::hea'), 'components::hea', 'part-way through a leaf');
  assert.equal(at('use components::a::b'), 'components::a::b', 'nested');
});

test('things that are not a use path are left alone', () => {
  const at = (line) => {
    const src = `<template></template>\n<script>\n${line}`;
    return completion.usePathBeing(src, src.length);
  };
  assert.equal(at('  let n = signal(0)'), null);
  assert.equal(at('  // use components::x'), null, 'a commented-out import');
  assert.equal(at('  fn used() {'), null, "a word merely starting with `use`");
  assert.equal(at('  use components::x;'), null, 'already terminated');
});

/**
 * Stand-ins for the handful of VS Code constructors `importPath` uses. Small
 * enough to be obviously right, which is the point: the thing under test is the
 * directory walk, not the editor API.
 */
const vscodeStub = {
  CompletionItem: class {
    constructor(label, kind) {
      this.label = label;
      this.kind = kind;
    }
  },
  CompletionItemKind: { Folder: 'Folder', Module: 'Module' },
  MarkdownString: class {
    constructor(value) {
      this.value = value;
    }
  },
};

test('a use path offers folders that hold components, and skips ones that do not', () => {
  const fs = require('node:fs');
  const os = require('node:os');
  const path = require('node:path');

  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rux-use-'));
  fs.mkdirSync(path.join(root, 'components'));
  fs.mkdirSync(path.join(root, 'assets'));
  fs.writeFileSync(path.join(root, 'app.rux'), '<template></template>');
  fs.writeFileSync(path.join(root, 'components', 'task.rux'), '<template></template>');
  fs.writeFileSync(path.join(root, 'components', 'crew_detail.rux'), '<template></template>');
  fs.writeFileSync(path.join(root, 'assets', 'logo.png'), '');

  const doc = { uri: { scheme: 'file', fsPath: path.join(root, 'app.rux') } };

  const top = completion.importPath(vscodeStub, doc, '').map((i) => i.label);
  assert.ok(top.includes('components'), 'the components folder was not offered');
  assert.ok(
    !top.includes('assets'),
    'assets has no .rux under it, so importing from it is a dead end'
  );
  assert.ok(!top.includes('app'), 'a document must not offer to import itself');

  const inside = completion.importPath(vscodeStub, doc, 'components::');
  const labels = inside.map((i) => i.label).sort();
  assert.deepEqual(labels, ['crew_detail', 'task'], 'the .rux files, without extensions');

  // The runtime maps `crew_detail` to `<crew-detail>`; the completion has to say
  // so, or the tag someone types next renders nothing.
  const crew = inside.find((i) => i.label === 'crew_detail');
  assert.match(crew.detail, /<crew-detail>/, `detail was ${crew.detail}`);

  fs.rmSync(root, { recursive: true, force: true });
});

// ── the vocabulary itself ────────────────────────────────────────────────────

test('the bundled vocabulary is loadable and honest', () => {
  assert.ok(vocabulary.elements().some((e) => e.name === 'view'));
  assert.ok(vocabulary.cssProperties().includes('display'));
  assert.ok(
    !vocabulary.cssProperties().includes('float'),
    'offering a property the runtime warns about would defeat the point'
  );
  assert.equal(vocabulary.isVoid('image'), true);
  assert.equal(vocabulary.isVoid('view'), false);
});
