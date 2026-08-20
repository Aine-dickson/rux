// The snippets, and which section each one belongs in.
//
// These used to be contributed statically through `package.json`'s
// `contributes.snippets`, which has no idea what a section is: VS Code offered
// all thirty-one of them everywhere in the file. Typing `s` after
// `justify-content:` produced `script`, `signal`, `slot`, `sticky` and `style`
// mixed in among the four values that were actually valid, which is how a
// working CSS value list came to look like a broken one.
//
// So they are served from the completion provider instead, filtered by the
// section the cursor is in. `snippets/rux.json` is still the data; the mapping
// below is what `package.json` could not express.

const fs = require('fs');
const path = require('path');

/**
 * Which section each snippet is written in.
 *
 * A snippet with no entry here is offered nowhere, which is the safe default:
 * adding a snippet and forgetting to place it costs a missing completion, and
 * the alternative default costs the noise this file exists to remove.
 */
const SECTION = {
  // Whole documents and the sections themselves: only outside any section.
  rux: 'document',
  template: 'document',
  style: 'document',
  script: 'document',

  // Markup.
  rfor: 'template',
  rif: 'template',
  rmodel: 'template',
  tap: 'template',
  interp: 'template',
  text: 'template',
  view: 'template',
  button: 'template',
  path: 'template',
  pathbound: 'template',
  router: 'template',
  guard: 'template',
  to: 'template',
  rtransition: 'template',
  slot: 'template',
  rselect: 'template',
  rcheckbox: 'template',

  // Whole CSS rules, so they belong at rule level and not inside a declaration.
  transitionrules: 'style',
  sticky: 'style',

  // Script.
  signal: 'script',
  computed: 'script',
  effect: 'script',
  mounted: 'script',
  query: 'script',
  emit: 'script',
  use: 'script',
  fn: 'script',
};

let loaded = null;

/** Read and flatten `snippets/rux.json` once. */
function all() {
  if (loaded) return loaded;
  loaded = [];
  let raw;
  try {
    raw = JSON.parse(fs.readFileSync(path.join(__dirname, 'snippets', 'rux.json'), 'utf8'));
  } catch (e) {
    // No snippets is a smaller failure than no extension.
    return loaded;
  }
  for (const [title, entry] of Object.entries(raw)) {
    if (!entry || !entry.prefix) continue;
    loaded.push({
      title,
      prefix: entry.prefix,
      // VS Code's snippet files allow a body to be a string or an array of
      // lines; both spellings are in this file already.
      body: Array.isArray(entry.body) ? entry.body.join('\n') : entry.body,
      description: entry.description || '',
      section: SECTION[entry.prefix] || null,
    });
  }
  return loaded;
}

/** The snippets written in `section`. */
function forSection(section) {
  return all().filter((s) => s.section === section);
}

/** Every prefix with no section, for the test that keeps the map complete. */
function unplaced() {
  return all().filter((s) => s.section === null).map((s) => s.prefix);
}

module.exports = { forSection, unplaced, all };
