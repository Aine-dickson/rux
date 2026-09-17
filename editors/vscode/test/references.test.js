// Tests for the two names a `.rux` template holds: `view=""` and `to=""`.
//
// Run with `node --test` from `editors/vscode`.
//
// Both are references rather than text, and the editor colours one that
// resolves. So the thing worth pinning is the resolution, not the painting:
// which spellings count, which do not, and — the half that actually matters —
// that a name which does not resolve is left alone. Colour means resolved, and
// a rule that over-paints says nothing at all.

const test = require('node:test');
const assert = require('node:assert');

const routes = require('../routes');
const semantic = require('../semantic');
const comments = require('../comments');

// ── reading a project's routes ───────────────────────────────────────────────

test('a router yields the full path of every route in it', () => {
  const src = [
    '<template><screen><router>',
    '  <route path="/" view="home" />',
    '  <route path="/task/:id" view="detail" />',
    '</router></screen></template>',
  ].join('\n');
  assert.deepEqual(routes.patternsIn(src), ['/', '/task/:id']);
});

test('a child path is relative to its parent, and path="" is the parent itself', () => {
  const src = [
    '<template><screen><router>',
    '  <route path="/crew" view="crew-section">',
    '    <route path="" view="crew-pick" />',
    '    <route path=":id" view="crew-member" />',
    '  </route>',
    '</router></screen></template>',
  ].join('\n');
  // The runtime builds these the same way; see `all_route_patterns` in
  // `crates/rux-style`.
  assert.deepEqual(routes.patternsIn(src), ['/crew', '/crew', '/crew/:id']);
});

test('a file with no router declares no routes', () => {
  assert.deepEqual(routes.patternsIn('<template><view to="/a">x</view></template>'), []);
});

test('a `:param` takes one segment and no more', () => {
  assert.ok(routes.matches('/task/:id', '/task/7'));
  assert.ok(!routes.matches('/task/:id', '/task/7/notes'), 'not two segments');
  assert.ok(!routes.matches('/task/:id', '/task'), 'not none');
});

test('a query or fragment is not part of the path', () => {
  assert.ok(routes.reaches(['/search'], '/search?q=rust'));
  assert.ok(routes.reaches(['/task/:id'], '/task/7#notes'));
});

// ── which names get painted ──────────────────────────────────────────────────

const APP = [
  '<template>',
  '  <screen>',
  '    <text to="/new">Add</text>',
  '    <text to="/typo">Nowhere</text>',
  '    <router>',
  '      <route path="/" view="home" />',
  '      <route path="/new" view="new_task" />',
  '    </router>',
  '  </screen>',
  '</template>',
  '<script>',
  'use pages::home;',
  '</script>',
].join('\n');

/** The painted names, which is the whole observable behaviour. */
function painted(text, patterns) {
  return semantic.referencesIn(text, patterns || routes.patternsIn(text)).map((r) => r.name);
}

test('an imported view is painted and an unimported one is not', () => {
  const names = painted(APP);
  assert.ok(names.includes('home'), 'the imported view');
  assert.ok(!names.includes('new_task'), 'the one with no `use` stays an ordinary string');
});

test('a link is painted when a route answers to it', () => {
  const names = painted(APP);
  assert.ok(names.includes('/new'), 'a declared route');
  assert.ok(!names.includes('/typo'), 'and a typo is left to read as a typo');
});

test('the painted span is the value inside the quotes', () => {
  const src = [
    '<template><screen><router><route path="/" view="home" /></router></screen></template>',
    '<script>',
    'use pages::home;',
    '</script>',
  ].join('\n');
  const [ref] = semantic.referencesIn(src, routes.patternsIn(src));
  assert.equal(src.slice(ref.start, ref.end), 'home');
});

test('either spelling of a view resolves, because either one runs', () => {
  // `use pages::new_task;` imports the tag `<new-task>`, and the runtime takes
  // both spellings in the template (`find_component` in `crates/rux-style`).
  // The editor has to agree with what would run: painting one and not the other
  // would say a working file was broken.
  const src = (view) =>
    [
      '<template><screen><router>',
      `  <route path="/" view="${view}" />`,
      '</router></screen></template>',
      '<script>',
      'use pages::new_task;',
      '</script>',
    ].join('\n');
  assert.deepEqual(painted(src('new_task')), ['new_task'], 'the import spelling');
  assert.deepEqual(painted(src('new-task')), ['new-task'], 'and the tag spelling');
  assert.deepEqual(painted(src('netask')), [], 'a typo is neither');
});

test('nothing outside the template is a reference', () => {
  const src = [
    '<template><screen><router><route path="/" view="home" /></router></screen></template>',
    '<style>',
    '.a { view: "home"; }',
    '</style>',
    '<script>',
    'let s = "view=\\"home\\"";',
    'use pages::home;',
    '</script>',
  ].join('\n');
  assert.deepEqual(painted(src), ['home'], 'only the one in the markup');
});

test('a fallback route does not make every address a destination', () => {
  // `examples/router.rux` links to `/nowhere` on purpose, to show the fallback
  // working. Painting it would say the opposite of what the example is for.
  const src = [
    '<template><screen>',
    '  <text to="/nowhere">x</text>',
    '  <router>',
    '    <route path="/" view="home" />',
    '    <route fallback view="lost" />',
    '  </router>',
    '</screen></template>',
    '<script>',
    'use pages::home;',
    'use pages::lost;',
    '</script>',
  ].join('\n');
  assert.ok(!painted(src).includes('/nowhere'));
});

// ── which comment Ctrl+/ writes ──────────────────────────────────────────────

test('each section gets the comment its language actually has', () => {
  assert.deepEqual(comments.rulesFor('template').blockComment, ['<!--', '-->']);
  assert.deepEqual(comments.rulesFor('style').blockComment, ['/*', '*/']);
  assert.equal(comments.rulesFor('script').lineComment, '//');
});

test('only the script section has a line comment', () => {
  // `//` is not a comment in markup and is not one in CSS either: the CSS
  // parser drops the line without saying so, which is how a "commented out"
  // rule stays commented out and nobody finds out.
  assert.equal(comments.rulesFor('template').lineComment, undefined);
  assert.equal(comments.rulesFor('style').lineComment, undefined);
});

test('between sections is treated as markup', () => {
  // A note written above `<style>` is markup-level, and the SFC splitter looks
  // for the section tags and ignores what sits between them.
  assert.deepEqual(comments.rulesFor(null).blockComment, ['<!--', '-->']);
});

test('go-to-definition and hover take the underscored tag too', () => {
  // Same rule, same helper. A `<new_task>` that renders and cannot be navigated
  // to is the editor disagreeing with the runtime about the same line.
  const src = [
    '<template><screen><new_task /></screen></template>',
    '<script>',
    'use pages::new_task;',
    '</script>',
  ].join('\n');
  const context = require('../context');
  assert.equal(context.componentNamed(src, 'new_task').file, 'pages/new_task.rux');
  assert.equal(context.componentNamed(src, 'new-task').file, 'pages/new_task.rux');
  assert.equal(context.componentNamed(src, 'netask'), undefined);
});
