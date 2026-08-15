// Where in a `.rux` file the cursor is.
//
// A `.rux` document is three languages in one file, so "what should completing
// here offer?" is answered differently in `<template>`, `<style>` and
// `<script>`. Nothing here parses Rux properly: the real parser is in
// `crates/rux-parser` and it needs a whole, valid document, while an editor is
// asked to be useful about a half-typed one. So this is scanning, and it is
// written to fail towards offering nothing rather than towards offering
// nonsense in the wrong section.

/** The three section tags, in the order they conventionally appear. */
const SECTIONS = ['template', 'style', 'script'];

/**
 * Which section `offset` falls inside, or `null` for the gaps between them.
 *
 * Nesting is not a concern: a `<template>` cannot contain another, and a
 * `<style>` inside a template would be a syntax error rather than a section.
 */
function sectionAt(text, offset) {
  let found = null;
  for (const name of SECTIONS) {
    const open = new RegExp(`<${name}[^>]*>`, 'g');
    let m;
    while ((m = open.exec(text)) !== null) {
      const start = m.index + m[0].length;
      if (start > offset) break;
      const close = text.indexOf(`</${name}>`, start);
      const end = close === -1 ? text.length : close;
      if (offset >= start && offset <= end) found = name;
    }
  }
  return found;
}

/**
 * The tags a component file imports, as they are written in a template.
 *
 * `use components::crew_detail;` names the file `components/crew_detail.rux`
 * and the tag `<crew-detail>`; the underscore-to-hyphen swap is the runtime's
 * (`extract_imports` in `crates/rux-runtime`), and getting it wrong here would
 * offer a tag that renders nothing.
 */
function importedComponents(text) {
  const tags = [];
  const line = /^[ \t]*use[ \t]+([A-Za-z_][A-Za-z0-9_:]*)[ \t]*;/gm;
  let m;
  while ((m = line.exec(text)) !== null) {
    if (sectionAt(text, m.index) !== 'script') continue;
    const segments = m[1].split('::');
    const last = segments[segments.length - 1];
    if (!last) continue;
    tags.push({ tag: last.replace(/_/g, '-'), file: `${segments.join('/')}.rux` });
  }
  return tags;
}

/**
 * If `offset` sits inside an unfinished opening tag, describe it.
 *
 * Returns `null` when the cursor is in ordinary content, and otherwise
 * `{ tag, onName, prefix }`: `onName` is true while the tag's own name is
 * still being typed (`<vi|`), false once we are among its attributes
 * (`<view cl|`). `prefix` is the word being typed, which VS Code uses to filter.
 */
function openTagAt(text, offset) {
  // Walk back to the nearest `<`, giving up at the matching `>` of a finished
  // tag, at a newline-heavy gap, or after a distance no real tag spans.
  const LIMIT = 2000;
  const from = Math.max(0, offset - LIMIT);
  const before = text.slice(from, offset);

  const lt = before.lastIndexOf('<');
  if (lt === -1) return null;
  const gt = before.lastIndexOf('>');
  if (gt > lt) return null; // the last tag already closed

  const inside = before.slice(lt + 1);
  if (inside.startsWith('/')) return null; // a closing tag, handled elsewhere
  if (inside.startsWith('!')) return null; // a comment

  // Quotes are the one thing worth tracking: inside `class="…"` the cursor is
  // in a value, not at an attribute name, and offering `r-for` there is noise.
  let quote = null;
  for (const c of inside) {
    if (quote) {
      if (c === quote) quote = null;
    } else if (c === '"' || c === "'") {
      quote = c;
    }
  }
  if (quote) return null;

  const name = /^([A-Za-z][\w.-]*)/.exec(inside);
  if (!name) {
    // `<` with nothing after it yet: the tag name is about to be typed.
    return inside.length === 0 ? { tag: '', onName: true, prefix: '' } : null;
  }
  const onName = inside.length === name[1].length;
  const prefix = onName ? name[1] : /[\S]*$/.exec(inside)[0];
  return { tag: name[1], onName, prefix };
}

/**
 * The innermost tag still open at `offset`, for completing `</`.
 *
 * Self-closing tags and void tags close themselves and never go on the stack,
 * so `</` after an `<image src="…">` offers whatever encloses it, which is the
 * useful answer rather than the literal one.
 */
function unclosedTagAt(text, offset, isVoid) {
  const stack = [];
  const tag = /<(\/?)([A-Za-z][\w.-]*)((?:"[^"]*"|'[^']*'|[^>"'])*)>/g;
  let m;
  while ((m = tag.exec(text)) !== null) {
    if (m.index >= offset) break;
    const [, slash, name, rest] = m;
    if (SECTIONS.includes(name)) continue; // the section tags are not content
    if (slash) {
      const at = stack.lastIndexOf(name);
      if (at !== -1) stack.length = at;
      continue;
    }
    if (rest.trimEnd().endsWith('/')) continue; // self-closed
    if (isVoid && isVoid(name)) continue;
    stack.push(name);
  }
  return stack.length ? stack[stack.length - 1] : null;
}

/**
 * True when `offset` sits where a CSS property name goes: inside a rule's
 * braces, and not part-way through a value. Selectors and at-rules are not
 * property positions and completing property names into them would be wrong.
 */
function inCssDeclaration(text, offset) {
  let depth = 0;
  let sawColon = false;
  for (let i = 0; i < offset; i++) {
    const c = text[i];
    if (c === '{') {
      depth++;
      sawColon = false;
    } else if (c === '}') {
      depth = Math.max(0, depth - 1);
      sawColon = false;
    } else if (depth > 0) {
      if (c === ';') sawColon = false;
      else if (c === ':') sawColon = true;
    }
  }
  return depth > 0 && !sawColon;
}

module.exports = {
  sectionAt,
  importedComponents,
  openTagAt,
  unclosedTagAt,
  inCssDeclaration,
};
