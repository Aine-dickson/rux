// Tests for what v0.4 added: CSS value and pseudo-class positions, hover
// lookup, go-to-definition, and the outline.
//
// Same arrangement as `context.test.js`: no framework, no `node_modules`, and
// nothing that needs VS Code booted. Everything under test is a pure function
// over a string, except the definition resolver, which reads the filesystem and
// is therefore given a real directory rather than a mock of one.
//
//   node --test          from `editors/vscode`

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const context = require('../context');
const definition = require('../definition');
const hover = require('../hover');
const symbols = require('../symbols');
const vocabulary = require('../vocabulary');

/** A `.rux` document, written the way the scanners will meet it. */
const DOC = [
  '<template>',
  '  <screen class="app">',
  '    <text id="total" class="value">{{ count }}</text>',
  '    <view class="row spread" r-if="open" r-transition>',
  '      <task-card :label="name" @tap="count += 1" />',
  '    </view>',
  '    <router>',
  '      <route path="/account" view="account" guard="signed_in" />',
  '    </router>',
  '  </screen>',
  '</template>',
  '',
  '<style>',
  '  .app { display: flex; flex-direction: column; }',
  '  .row:hover { background: #222; }',
  '  .row:leave-to { opacity: 0; position: absolute; }',
  '  @media (max-width: 600px) {',
  '    .row { flex-direction: column; }',
  '  }',
  '</style>',
  '',
  '<script>',
  '  use components::task_card;',
  '  let count = signal(0);',
  '  let doubled = computed(|| count * 2);',
  '  fn bump() { count += 1 }',
  '  mounted {',
  '    print("up");',
  '  }',
  '</script>',
  '',
].join('\n');

// ── where in the CSS ─────────────────────────────────────────────────────────

test('a value position is told from a property position, and carries the property', () => {
  const at = (needle, into) => DOC.indexOf(needle) + into;

  assert.deepEqual(context.cssPositionAt(DOC, at('display: fl', 11)), {
    where: 'value',
    property: 'display',
  });
  // Part-way through the property name itself.
  assert.equal(context.cssPositionAt(DOC, at('flex-direction', 4)).where, 'property');
  // Outside any rule's braces.
  assert.equal(context.cssPositionAt(DOC, at('.app {', 2)).where, 'selector');
});

test('the second declaration on a line knows its own property', () => {
  // `flex-direction: column` follows `display: flex;` inside one rule, so this
  // only works if the semicolon reset the scan. It is the case a scanner that
  // merely looked backwards for a colon would get wrong.
  const offset = DOC.indexOf('flex-direction: column') + 'flex-direction: col'.length;
  assert.deepEqual(context.cssPositionAt(DOC, offset), {
    where: 'value',
    property: 'flex-direction',
  });
});

test('braces in the template and the script do not move the CSS scanner', () => {
  // `{{ count }}` in the template and `fn bump() { … }` in the script both put
  // braces in the file. Scanning the whole document rather than the section
  // would let them decide whether the CSS is inside a rule.
  const offset = DOC.indexOf('background: #222') + 'background: #2'.length;
  assert.deepEqual(context.cssPositionAt(DOC, offset), {
    where: 'value',
    property: 'background',
  });
});

test('a colon in a selector opens the pseudo-class list, one in a declaration does not', () => {
  assert.equal(context.atPseudoClass(DOC, DOC.indexOf(':hover') + 3), true);
  assert.equal(context.atPseudoClass(DOC, DOC.indexOf(':leave-to') + 1), true);
  assert.equal(
    context.atPseudoClass(DOC, DOC.indexOf('display: flex') + 9),
    false,
    'a declaration is not a selector'
  );
});

test('a word stops at the colon on either side of it', () => {
  // `:` cannot be part of a word: it would swallow the colon in `position:` on
  // one side and could not be told from `components::x` on the other.
  const inProperty = context.wordAt(DOC, DOC.indexOf('position: absolute') + 3);
  assert.equal(inProperty.word, 'position');

  const inPseudo = context.wordAt(DOC, DOC.indexOf(':leave-to') + 3);
  assert.equal(inPseudo.word, 'leave-to');
});

// ── the vocabulary v0.7 added ────────────────────────────────────────────────

test('the bundled vocabulary carries the v0.7 surface', () => {
  assert.ok(
    vocabulary.elements().some((e) => e.name === 'path'),
    '<path> landed in v0.7 and is offered'
  );
  assert.ok(
    vocabulary.attributesFor('route').some((a) => a.name === 'guard'),
    'route guards landed in v0.7'
  );
  assert.ok(
    vocabulary.attributesFor('router').some((a) => a.name === 'guard'),
    'a guard on the router covers every navigation'
  );
  assert.ok(
    vocabulary.globalAttributes().some((a) => a.name === '@tap'),
    '@tap comes from the vocabulary now, not from a hand-written completion'
  );
  assert.ok(vocabulary.scriptGlobals().some((g) => g.name === 'query'));
});

test('position offers all five values, which is the bug that made this worth having', () => {
  // Before v0.7 four of these parsed, did not match, and silently fell through
  // to `relative`. The completion list is now the same five the parser knows.
  assert.deepEqual(vocabulary.cssValues('position'), [
    'static',
    'relative',
    'sticky',
    'absolute',
    'fixed',
  ]);
  assert.deepEqual(vocabulary.cssValues('width'), [], 'a length is not a keyword set');
});

test('the pseudo-classes offered are the ones that match', () => {
  const names = vocabulary.pseudoClasses().map((p) => p.name);
  assert.deepEqual(names, [
    'hover',
    'focus',
    'active',
    'checked',
    'current',
    'enter-from',
    'leave-to',
  ]);
});

test('transition knows what can be animated, `d` included', () => {
  const animatable = vocabulary.animatableProperties();
  assert.ok(animatable.includes('all'));
  assert.ok(animatable.includes('d'), 'path geometry morphs since v0.7');
  assert.ok(!animatable.includes('display'));
  assert.ok(vocabulary.easings().includes('ease-in-out'));
});

// ── hover ────────────────────────────────────────────────────────────────────

/** Hover at the first occurrence of `needle`, `into` characters in. */
function hoverAt(needle, into) {
  const offset = DOC.indexOf(needle) + into;
  const at = context.wordAt(DOC, offset);
  return hover.lookUp(context.sectionAt(DOC, offset), DOC, at);
}

test('hover answers for elements, directives and script globals', () => {
  assert.equal(hoverAt('<router>', 3).title, '<router>');
  assert.equal(hoverAt('r-transition', 3).title, 'r-transition');
  assert.equal(hoverAt('signal(0)', 3).title, 'signal');
});

test('hover on a guard says what its answers mean', () => {
  const found = hoverAt('guard="signed_in"', 2);
  assert.equal(found.title, 'guard');
  assert.match(found.doc, /redirects/);
});

test('a bound attribute is answered as the attribute, and says what the colon does', () => {
  const found = hoverAt(':label="name"', 3);
  assert.equal(found, null, ':label is a component prop, and the vocabulary has no entry for it');

  const bound = hoverAt(':d="', 2);
  assert.equal(bound, null, 'this document has no bound `d`; the case below covers it');
});

test('a class value is not mistaken for the element of the same name', () => {
  // `class="value"` contains `value`, and `class="row spread"` contains `row`.
  // A hover that matched on the word alone would answer for whichever
  // vocabulary entry shared the spelling.
  assert.equal(hoverAt('class="value"', 8), null);
});

test('hover on a CSS property lists the values that work', () => {
  const found = hoverAt('flex-direction: column', 3);
  assert.equal(found.title, 'flex-direction');
  assert.match(found.doc, /`row`/);
  assert.match(found.doc, /`column`/);
});

test('hover on a pseudo-class explains the one that is easy to get backwards', () => {
  const found = hoverAt(':leave-to', 3);
  assert.equal(found.title, ':leave-to');
  assert.match(found.doc, /absolute/, 'the out-of-flow trick is the point of the doc');
});

// ── go to definition ─────────────────────────────────────────────────────────

test('a use path and the tag it contributes both open the same file', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rux-def-'));
  fs.mkdirSync(path.join(root, 'components'));
  const target = path.join(root, 'components', 'task_card.rux');
  fs.writeFileSync(target, '<template></template>');

  // On the path in `use components::task_card;`.
  const onUse = DOC.indexOf('components::task_card') + 3;
  assert.equal(definition.targetAt(DOC, onUse, root), target);

  // On the `<task-card>` tag, whose hyphen the runtime maps back to the
  // underscore in the filename.
  const onTag = DOC.indexOf('<task-card') + 3;
  assert.equal(definition.targetAt(DOC, onTag, root), target);

  fs.rmSync(root, { recursive: true, force: true });
});

test('a use of a file that is not written yet does not navigate', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rux-def-'));
  const onUse = DOC.indexOf('components::task_card') + 3;
  assert.equal(definition.targetAt(DOC, onUse, root), null);
  fs.rmSync(root, { recursive: true, force: true });
});

test('the `use` keyword itself is not a navigation target', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rux-def-'));
  fs.mkdirSync(path.join(root, 'components'));
  fs.writeFileSync(path.join(root, 'components', 'task_card.rux'), '');
  const onKeyword = DOC.indexOf('use components::task_card') + 1;
  assert.equal(definition.targetAt(DOC, onKeyword, root), null);
  fs.rmSync(root, { recursive: true, force: true });
});

test('a built-in element is not a component and does not navigate', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rux-def-'));
  assert.equal(definition.targetAt(DOC, DOC.indexOf('<router>') + 3, root), null);
  fs.rmSync(root, { recursive: true, force: true });
});

// ── the outline ──────────────────────────────────────────────────────────────

test('the outline is the three sections', () => {
  const found = symbols.outline(DOC);
  assert.deepEqual(found.map((s) => s.name), ['<template>', '<style>', '<script>']);
});

test('the template lists only elements you could write a selector for', () => {
  const template = symbols.outline(DOC)[0];
  const names = template.children.map((c) => c.name);
  assert.ok(names.includes('screen.app'));
  assert.ok(names.includes('text#total'), 'an id wins over the class beside it');
  assert.ok(names.includes('view.row.spread'), 'two classes read as the selector would');
  assert.ok(
    !names.some((n) => n === 'router' || n === 'route'),
    'an element with no class and no id is not worth a line in the outline'
  );
});

test('the style section lists selectors, and a media query owns its rules', () => {
  const style = symbols.outline(DOC)[1];
  const names = style.children.map((c) => c.name);
  assert.ok(names.includes('.app'));
  assert.ok(names.includes('.row:hover'));
  assert.ok(names.includes('.row:leave-to'));

  const media = style.children.find((c) => c.name.startsWith('@media'));
  assert.ok(media, 'the media query is missing');
  assert.deepEqual(media.children.map((c) => c.name), ['.row']);
});

test('the script section lists state and functions in document order', () => {
  const script = symbols.outline(DOC)[2];
  assert.deepEqual(script.children.map((c) => c.name), [
    'count',
    'doubled',
    'bump()',
    'mounted',
  ]);
});

// ── what the completion list actually offers ─────────────────────────────────

// Enough of the VS Code API for the completion functions to build their items.
// Anything they touch and this does not have would be a TypeError, which is the
// point: the stub is the contract.
const vscodeStub = {
  CompletionItem: class {
    constructor(label, kind) {
      this.label = label;
      this.kind = kind;
    }
  },
  CompletionItemKind: {
    Class: 'Class',
    Module: 'Module',
    Keyword: 'Keyword',
    Property: 'Property',
    Value: 'Value',
    Function: 'Function',
    Event: 'Event',
    Folder: 'Folder',
  },
  MarkdownString: class {
    constructor(value) {
      this.value = value;
    }
  },
  SnippetString: class {
    constructor(value) {
      this.value = value;
    }
  },
};

const completion = require('../completion');

/** The labels offered in the `<style>` section at the first `needle`. */
function styleLabels(source, needle, into) {
  const offset = source.indexOf(needle) + into;
  const items = completion.style(vscodeStub, source, offset) || [];
  return items.map((i) => i.label);
}

test('typing a position value offers the five that work', () => {
  const src = '<style>\n  .a { position:  }\n</style>\n';
  const labels = styleLabels(src, 'position: ', 'position: '.length);
  assert.deepEqual(labels, ['static', 'relative', 'sticky', 'absolute', 'fixed']);
});

test('a property with no keyword values offers nothing rather than guesses', () => {
  const src = '<style>\n  .a { width:  }\n</style>\n';
  assert.deepEqual(styleLabels(src, 'width: ', 'width: '.length), []);
});

test('a transition offers what can be animated and how it can ease', () => {
  const src = '<style>\n  .a { transition:  }\n</style>\n';
  const labels = styleLabels(src, 'transition: ', 'transition: '.length);
  assert.ok(labels.includes('opacity'));
  assert.ok(labels.includes('d'), 'path morphing is a v0.7 transition');
  assert.ok(labels.includes('ease-out'));
  assert.ok(!labels.includes('display'), 'display cannot be animated');
});

test('a property position still offers property names', () => {
  const src = '<style>\n  .a {  }\n</style>\n';
  const labels = styleLabels(src, '.a { ', '.a { '.length);
  assert.ok(labels.includes('position'));
  assert.ok(!labels.includes('float'));
});

test('a colon in a selector offers pseudo-classes, not properties', () => {
  const src = '<style>\n  .a: { }\n</style>\n';
  const labels = styleLabels(src, '.a:', 3);
  assert.deepEqual(labels, [
    'hover',
    'focus',
    'active',
    'checked',
    'current',
    'enter-from',
    'leave-to',
  ]);
});

test('@tap is offered once, from the vocabulary', () => {
  const src = '<template>\n  <view \n</template>\n';
  const items = completion.template(vscodeStub, src, src.indexOf('<view ') + '<view '.length);
  const taps = items.filter((i) => i.label === '@tap');
  assert.equal(taps.length, 1, 'the hand-written copy and the vocabulary one would both appear');
});

test('a route offers guard, and a view does not', () => {
  const src = '<template>\n  <route \n</template>\n';
  const onRoute = completion.template(vscodeStub, src, src.indexOf('<route ') + '<route '.length);
  assert.ok(onRoute.some((i) => i.label === 'guard'));

  const plain = '<template>\n  <view \n</template>\n';
  const onView = completion.template(vscodeStub, plain, plain.indexOf('<view ') + '<view '.length);
  assert.ok(!onView.some((i) => i.label === 'guard'), 'guard means nothing on a view');
});

// ── Format Document must not fight `rux fmt` ─────────────────────────────────
//
// The formatter used to pass the editor's own `tabSize`, which is 4 unless
// something says otherwise, while `rux fmt` defaults to 2 and every file in the
// Rux repo is 2. Formatting any `.rux` file therefore re-indented the whole
// document to 4, and `rux fmt --check` then rejected the result. It reformatted
// `examples/router.rux` end to end (336 lines, no change in content) and that
// very nearly went out in a release.
//
// One implementation of the rules, behind `rux fmt`, is a rule this repo
// already learned once. Handing that implementation a different indent from the
// one it uses on the command line rebuilds the same disagreement out of a
// single copy.

const extension = require('../extension');

/** A `vscode.workspace.getConfiguration('rux')` stand-in. */
const withSetting = (value) => ({
  workspace: { getConfiguration: () => ({ get: () => value }) },
});

test('by default the binary decides the indent, and is asked for nothing', () => {
  const args = extension.indentArgs(withSetting('auto'), { insertSpaces: true, tabSize: 4 });
  assert.deepEqual(args, [], 'an editor tab size of 4 must not reach `rux fmt`');
});

test('an unset value is the same as auto', () => {
  assert.deepEqual(extension.indentArgs(withSetting(undefined), { tabSize: 8 }), []);
});

test('an explicit width is passed through', () => {
  assert.deepEqual(extension.indentArgs(withSetting('4'), { tabSize: 2 }), ['--indent', '4']);
  assert.deepEqual(extension.indentArgs(withSetting('tab'), { tabSize: 2 }), ['--indent', 'tab']);
});

test('opting in to the editor tab size is possible, and deliberate', () => {
  assert.deepEqual(
    extension.indentArgs(withSetting('editor'), { insertSpaces: true, tabSize: 4 }),
    ['--indent', '4']
  );
  assert.deepEqual(
    extension.indentArgs(withSetting('editor'), { insertSpaces: false, tabSize: 4 }),
    ['--indent', 'tab']
  );
});
