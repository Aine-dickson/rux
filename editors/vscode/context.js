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
 * The span of the section `offset` is inside, as `[start, end]`, or `null`.
 *
 * The CSS scanners below need this. Counting braces from the top of the file
 * would count the template's `{{ interpolation }}` and the script's blocks as
 * well, and while those happen to balance most of the time, "most of the time"
 * is not a property worth relying on when the answer decides what gets offered.
 */
function sectionSpan(text, offset) {
  for (const name of SECTIONS) {
    const open = new RegExp(`<${name}[^>]*>`, 'g');
    let m;
    while ((m = open.exec(text)) !== null) {
      const start = m.index + m[0].length;
      if (start > offset) break;
      const close = text.indexOf(`</${name}>`, start);
      const end = close === -1 ? text.length : close;
      if (offset >= start && offset <= end) return [start, end];
    }
  }
  return null;
}

/**
 * Walk the CSS from the start of its own section and report where `offset` is:
 * `'selector'` outside any rule's braces, `'property'` where a name goes, and
 * `'value'` after the colon that follows one.
 *
 * Strings and comments are skipped, because a `{` inside `content: "{"` or a
 * `;` inside a comment would otherwise move the whole file into the wrong
 * state, and every completion after it would be wrong rather than missing.
 */
function cssPositionAt(text, offset) {
  const span = sectionSpan(text, offset);
  if (!span) return { where: null, property: null };

  let depth = 0;
  let inValue = false;
  let property = null;
  let declStart = span[0];

  for (let i = span[0]; i < offset; i++) {
    const c = text[i];

    if (c === '/' && text[i + 1] === '*') {
      const end = text.indexOf('*/', i + 2);
      i = end === -1 ? offset : end + 1;
      continue;
    }
    if (c === '"' || c === "'") {
      let j = i + 1;
      while (j < offset && text[j] !== c) j += text[j] === '\\' ? 2 : 1;
      i = j;
      continue;
    }

    if (c === '{') {
      depth++;
      inValue = false;
      declStart = i + 1;
    } else if (c === '}') {
      depth = Math.max(0, depth - 1);
      inValue = false;
      declStart = i + 1;
    } else if (depth > 0 && c === ';') {
      inValue = false;
      declStart = i + 1;
    } else if (depth > 0 && c === ':' && !inValue) {
      // The colon that opens a value, as opposed to the one in `a:hover`. At
      // `depth > 0` there are no selectors, so every colon here is the first.
      inValue = true;
      property = text.slice(declStart, i).trim().split(/[\s;{}]/).pop() || null;
    }
  }

  if (depth === 0) return { where: 'selector', property: null };
  return inValue ? { where: 'value', property } : { where: 'property', property: null };
}

/**
 * Every class and id the template actually writes, so a selector can be
 * completed from the markup it will match rather than from nothing.
 *
 * Both the literal and the bound spellings are read. `:class="#{ mine: m.mine }"`
 * applies `mine` conditionally, and a rule for it is exactly as real as one for
 * a name in a plain `class=`; leaving the bound form out would quietly miss the
 * conditional half of every component's styling.
 *
 * Only `<template>` is read. A name that appears solely in the stylesheet is a
 * rule matching nothing, and offering it back would launder a dead rule into
 * an endorsed one.
 */
function templateSelectors(text) {
  const span = (() => {
    const open = /<template[^>]*>/.exec(text);
    if (!open) return null;
    const start = open.index + open[0].length;
    const close = text.indexOf('</template>', start);
    return text.slice(start, close === -1 ? text.length : close);
  })();
  if (!span) return { classes: [], ids: [] };

  const classes = new Set();
  const ids = new Set();

  let m;
  // Whitespace before `class`, not a word boundary: `\b` also matches between
  // `:` and `c`, so `:class="#{ mine: m.mine }"` was read as a literal list and
  // split into `#{`, `mine:`, `m.mine,` and `}`. The bound form is handled
  // below, on its own terms.
  const literal = /(?:^|\s)class\s*=\s*"([^"]*)"/g;
  while ((m = literal.exec(span)) !== null) {
    for (const name of m[1].split(/\s+/)) if (name) classes.add(name);
  }

  // `:class="#{ a: x, b: y }"` and `:class="cond ? 'a' : 'b'"`: the keys of the
  // map, and any quoted literal.
  const bound = /:class\s*=\s*"([^"]*)"/g;
  while ((m = bound.exec(span)) !== null) {
    const value = m[1];
    let k;
    const key = /([A-Za-z_][\w-]*)\s*:/g;
    while ((k = key.exec(value)) !== null) classes.add(k[1]);
    const quoted = /'([^']+)'/g;
    while ((k = quoted.exec(value)) !== null) {
      for (const name of k[1].split(/\s+/)) if (name) classes.add(name);
    }
  }

  const idAttr = /(?:^|\s)id\s*=\s*"([^"]*)"/g;
  while ((m = idAttr.exec(span)) !== null) if (m[1].trim()) ids.add(m[1].trim());

  return { classes: [...classes].sort(), ids: [...ids].sort() };
}

/**
 * True when `offset` sits right after a `:` in a *selector*, so a pseudo-class
 * is what comes next. `::` is included: Rux has no pseudo-elements, and someone
 * who typed one is better told by an empty list than by a property list.
 */
function atPseudoClass(text, offset) {
  if (cssPositionAt(text, offset).where !== 'selector') return false;
  return /:{1,2}[a-zA-Z-]*$/.test(text.slice(Math.max(0, offset - 40), offset));
}

/**
 * The identifier `offset` sits inside, with its span, or `null`. Hover and
 * go-to-definition both need "what word is under the cursor", and both want
 * the Rux spelling of a word, so `r-for`, `@tap` and `stroke-width` are one
 * word each rather than three.
 *
 * A `:` is deliberately not part of a word. It would make `position:` the word
 * under the cursor in a declaration, and it cannot be included on the left
 * only: `.a:hover` and `components::x` put the same character in the same
 * place and mean different things. Callers that want the pseudo-class form
 * look at what precedes `start` themselves, knowing which section they are in.
 */
function wordAt(text, offset) {
  const isWord = (c) => c !== undefined && /[A-Za-z0-9_@.-]/.test(c);
  let start = offset;
  while (start > 0 && isWord(text[start - 1])) start--;
  let end = offset;
  while (end < text.length && isWord(text[end])) end++;
  if (start === end) return null;
  return { word: text.slice(start, end), start, end };
}

/**
 * The single name under the cursor, with any `a.b.c` path split off.
 *
 * [`wordAt`] treats `.` as part of a word, because a tag legitimately contains
 * one and so does an `@handler`. That is right for markup and wrong for an
 * expression: hovering `scrollIntoView` in `row.scrollIntoView()` produced the
 * word `row.scrollIntoView`, which matches nothing in any list, so **every**
 * hover on a member silently did nothing.
 *
 * Returns the last segment as `word` with its own range, and whatever preceded
 * the final dot as `receiver`.
 */
function memberAt(text, offset) {
  const at = wordAt(text, offset);
  if (!at) return null;
  if (at.word.indexOf('.') === -1) return { ...at, receiver: null };

  // The segment the cursor is actually in, not the last one. Hovering `msg` in
  // `msg.id` asks about `msg`; taking the last segment answered about `id` and
  // then found nothing, so the useful half of every dotted expression was
  // unreachable.
  const where = offset - at.start;
  let from = 0;
  for (const piece of at.word.split('.')) {
    const to = from + piece.length;
    if (where <= to) {
      return {
        word: piece,
        start: at.start + from,
        end: at.start + to,
        receiver: from === 0 ? null : at.word.slice(0, from - 1),
      };
    }
    from = to + 1; // the dot
  }
  return { ...at, receiver: null };
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

/**
 * True when `offset` sits inside a place a script expression is written:
 * `{{ … }}`, a `:bound` attribute's value, or an `@handler`'s.
 *
 * These are the three spellings of the same thing, evaluated in the same scope,
 * so they get the same completions. Getting this right is what makes the
 * document's own signals reachable from the markup, which is where most of them
 * are actually typed.
 */
function inTemplateExpression(text, offset) {
  // An interpolation is the easy half: the nearest `{{` with no `}}` after it.
  const open = text.lastIndexOf('{{', offset);
  if (open !== -1) {
    const close = text.indexOf('}}', open);
    if (close === -1 || close >= offset) return true;
  }

  // The other half is an attribute value, which means finding the quote we are
  // inside and asking what its attribute was called. Anything not bound (a
  // plain `class="…"`) is a literal, not an expression.
  const from = Math.max(0, offset - 2000);
  const before = text.slice(from, offset);
  const lt = before.lastIndexOf('<');
  if (lt === -1) return false;
  const gt = before.lastIndexOf('>');
  if (gt > lt) return false;

  const inside = before.slice(lt + 1);
  let quote = null;
  let valueStart = -1;
  for (let i = 0; i < inside.length; i++) {
    const c = inside[i];
    if (quote) {
      if (c === quote) {
        quote = null;
        valueStart = -1;
      }
    } else if (c === '"' || c === "'") {
      quote = c;
      valueStart = i;
    }
  }
  if (!quote || valueStart === -1) return false;

  // The attribute name is the word before the `=` that opened this value.
  const head = inside.slice(0, valueStart);
  const name = /([@:]?[\w.-]+)\s*=\s*$/.exec(head);
  if (!name) return false;
  const attribute = name[1];
  return (
    attribute.startsWith('@') ||
    attribute.startsWith(':') ||
    EXPRESSION_ATTRIBUTES.includes(attribute)
  );
}

/**
 * Directives whose value is an expression even without a `:`, because the
 * directive itself is the binding.
 *
 * `r-key` is in here and `r-for` is not: `r-for="item in items"` is a form of
 * its own, and completing a bare expression into it would be completing into
 * the wrong half.
 */
const EXPRESSION_ATTRIBUTES = ['r-if', 'r-elif', 'r-show', 'r-model', 'r-key', 'guard'];

module.exports = {
  sectionAt,
  templateSelectors,
  memberAt,
  inTemplateExpression,
  sectionSpan,
  importedComponents,
  openTagAt,
  unclosedTagAt,
  inCssDeclaration,
  cssPositionAt,
  atPseudoClass,
  wordAt,
};
