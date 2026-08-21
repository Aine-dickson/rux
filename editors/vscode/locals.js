// What *this document* declares: its signals, computeds and functions.
//
// The vocabulary covers what the runtime provides and knew nothing about what
// the author had just written, so the completion list went quiet at exactly the
// point it should have been most useful. `let draft = signal("")` on line 4 and
// no `draft` offered on line 9 is the editor being less informed than the file.
//
// Nothing here parses rhai. The real compiler is `crates/rux-script` and it
// needs a whole valid script, while an editor is asked about a half-typed one.
// So this is line-oriented scanning, written to miss a declaration rather than
// to invent one: a name that is not offered costs a keystroke, and a name that
// is offered and does not exist costs a debugging session.

/** `let x = signal(…)` — reactive state. The initial value is captured too. */
const SIGNAL = /^[ \t]*let[ \t]+([A-Za-z_][\w]*)[ \t]*=[ \t]*signal[ \t]*\((.*)\)[ \t]*;?[ \t]*$/;
/** `let x = …` — an ordinary binding. */
const BINDING = /^[ \t]*let[ \t]+([A-Za-z_][\w]*)[ \t]*=[ \t]*(.*?)[ \t]*;?[ \t]*$/;
/** `computed x = …` — a declaration, not a call. */
const COMPUTED = /^[ \t]*computed[ \t]+([A-Za-z_][\w]*)[ \t]*=[ \t]*(.*?)[ \t]*;?[ \t]*$/;
/** `fn name(a, b) {` */
const FUNCTION = /^[ \t]*fn[ \t]+([A-Za-z_][\w]*)[ \t]*\(([^)]*)\)/;

/**
 * Every name the document's `<script>` declares, innermost kind first.
 *
 * Only top-level declarations are collected. A `let` inside a function body is
 * scoped to it, and offering it everywhere would be offering a name that is not
 * in scope where it was accepted. Depth is tracked by counting braces rather
 * than by parsing, which is crude and errs towards *fewer* names.
 *
 * That erring used to be described here as "the safe direction". It is not.
 * Offering nothing is what a broken extension looks like from the outside, and
 * an author who has just watched the list go quiet has no way to tell the two
 * apart. So the counting is worth some care: string literals are blanked before
 * their braces are counted (see `blankStrings`), because one `"{"` in a string
 * silenced every declaration below it for the rest of the file.
 */
function declarations(text) {
  const script = sectionBody(text, 'script');
  if (!script) return [];

  const found = [];
  const seen = new Set();
  let depth = 0;

  // The file line the section body starts on, so a declaration can say where
  // it is rather than only what it is. Hover is the caller that needs this:
  // "a signal" answers less than "a signal, declared on line 12".
  const firstLine = countLines(text.slice(0, script.start));
  // Split on either ending, so a line from a CRLF file does not arrive with a
  // carriage return still on it.
  //
  // This is what made every pattern above fail on a Windows-authored file,
  // which is to say on the file this language is mostly written on. The three
  // declaration patterns are anchored with `$`, and JavaScript's `$` without
  // the `m` flag matches at the end of the string or before a final newline,
  // never before a carriage return. `.` does not match one either, so the lazy
  // group in BINDING could not step over it to reach the anchor. Every `let`
  // and every `computed` in the file failed to match, `declarations()` came
  // back empty, and completion, hover and the inferred types all went quiet
  // together, while the outline kept working because it scans with `/m` and
  // no `$`.
  //
  // Nothing said so. An empty declaration list is indistinguishable from a
  // file that declares nothing, which is how this survived two sessions of
  // being reported, verified against LF fixtures, and reported again.
  const body = script.body.split(/\r?\n/);

  for (let n = 0; n < body.length; n++) {
    const raw = body[n];
    const at = firstLine + n;
    const line = stripComment(raw);

    // Depth is measured *before* this line's own braces for a declaration, so
    // `fn f() {` is itself top-level while its body is not.
    if (depth === 0) {
      let m;
      if ((m = FUNCTION.exec(line))) {
        add(found, seen, {
          name: m[1],
          kind: 'function',
          detail: `fn ${m[1]}(${m[2].trim()})`,
          doc: 'A function declared in this file.',
          line: at,
          init: null,
          params: m[2].trim(),
        });
      } else if ((m = SIGNAL.exec(line))) {
        add(found, seen, {
          name: m[1],
          kind: 'signal',
          detail: 'signal',
          doc:
            'Reactive state declared in this file. Reading it in a binding *is* ' +
            'subscribing to it.',
          line: at,
          init: (m[2] || '').trim(),
        });
      } else if ((m = COMPUTED.exec(line))) {
        add(found, seen, {
          name: m[1],
          kind: 'computed',
          detail: 'computed',
          doc:
            'Derived state declared in this file. Refreshed in declaration ' +
            'order, so it may read a computed declared above it and not below.',
          line: at,
          init: (m[2] || '').trim(),
        });
      } else if ((m = BINDING.exec(line))) {
        add(found, seen, {
          name: m[1],
          kind: 'binding',
          detail: 'let',
          doc: 'A binding declared in this file.',
          line: at,
          init: (m[2] || '').trim(),
        });
      }
    }

    // Braces are counted on the line with its string literals blanked out.
    // Without that, one `let tpl = "a { b";` left the scanner permanently one
    // level deep and it never offered another name in the file — and a
    // completion list that goes quiet partway down a file looks like an
    // extension that does not work, not like a scanner being careful.
    const braces = blankStrings(line);
    depth += count(braces, '{') - count(braces, '}');
    if (depth < 0) depth = 0;
  }
  return found;
}

/** 1-based line number of the offset that `text` ends at. */
function countLines(text) {
  let n = 1;
  for (const c of text) if (c === '\n') n++;
  return n;
}

/**
 * One declaration by name, with its inferred type attached, or `null`.
 *
 * The type is added here rather than in `declarations` because inferring it
 * needs the whole document (a `query()` binding is recognised by scanning), and
 * the completion list asks for every declaration on every keystroke.
 */
function declaration(text, name) {
  const found = declarations(text).find((d) => d.name === name);
  if (!found) return null;
  return { ...found, type: receiverKind(text, name) };
}

/**
 * The loop variable in scope at `offset`, if any.
 *
 * `r-for="item in items"` puts `item` in scope for that element's subtree and
 * nowhere else. Offering it everywhere would be wrong, and not offering it at
 * all leaves the single most-typed name in a template out of the list.
 *
 * Scope is approximated by the enclosing element, which is what `r-for` scopes
 * to. Being approximate is acceptable here in a way it is not for declarations:
 * the worst case is a loop variable offered one element too widely, inside the
 * same template, where the author can see the loop.
 */
function loopVariables(text, offset) {
  const names = [];
  const seen = new Set();
  const before = text.slice(0, offset);
  const pattern = /r-for[ \t]*=[ \t]*"([^"]*)"/g;
  let m;
  while ((m = pattern.exec(before)) !== null) {
    const parsed = /^\s*([A-Za-z_][\w]*)\s+in\s+/.exec(m[1]);
    if (!parsed || seen.has(parsed[1])) continue;
    seen.add(parsed[1]);
    names.push({
      name: parsed[1],
      kind: 'loop',
      detail: `each ${parsed[1]} of the list`,
      doc: `The current row, from \`r-for="${m[1]}"\`. In scope inside that element only.`,
    });
  }
  return names;
}

/**
 * Whether `offset` sits just after `something.`, and if so what came before the
 * dot.
 *
 * Returns `null` when this is not a member position. The receiver is returned
 * unresolved: deciding what it *is* belongs to the caller, which has the
 * declarations to check it against.
 */
function memberReceiver(text, offset) {
  const before = text.slice(Math.max(0, offset - 200), offset);
  // `a.b` and `a?.b`, with a partly-typed member after the dot.
  const m = /([A-Za-z_][\w]*)\s*(?:\?)?\.\s*[\w]*$/.exec(before);
  if (!m) return null;
  // An index first: `query(".x")[0].` has no plain identifier before the dot.
  return m[1];
}

/** Whether `offset` sits just after `]` and a dot, as in `query(…)[0].`. */
function indexedReceiver(text, offset) {
  const before = text.slice(Math.max(0, offset - 200), offset);
  return /\]\s*(?:\?)?\.\s*[\w]*$/.test(before);
}

/**
 * Whether `offset` is anywhere after a `.`, whatever the receiver looks like.
 *
 * This is the question that actually decides what a completion list may
 * contain, and it is deliberately looser than the two probes above. They only
 * recognise a receiver they can *name* — a plain identifier, or an index. After
 * `search_item.map().` neither matched, the member branch declined, and the
 * list fell through to every global in the language: `back`, `blur`,
 * `clearInterval`, `navigate` and the rest, none of which can follow a dot.
 *
 * A dot means members. Not knowing *which* members is a reason to offer a
 * smaller list, never a reason to offer an unrelated one.
 */
function afterDot(text, offset) {
  const before = text.slice(Math.max(0, offset - 200), offset);
  return /(?:\?)?\.\s*[A-Za-z_]?[\w]*$/.test(before);
}

/**
 * What kind of value a name holds, as far as can be told from its declaration
 * alone: `'element'`, `'array'`, `'string'`, or `null` for unknown.
 *
 * `null` is the common answer and it is a real one. Rux has no type
 * annotations, so a declaration is all there is, and most declarations do not
 * say. **Unknown must mean "offer nothing"**, not "offer the string and array
 * methods and hope": `let handle = setInterval(2000) { … }` holds a timer
 * handle, and completing `handle.charAt(` on it is the editor endorsing a call
 * that cannot work. That is the same failure as offering an unhonored CSS
 * property, which this whole vocabulary exists to prevent.
 *
 * Inference is one step from the initialiser and no further. Anything cleverer
 * is guessing with more machinery.
 */
function receiverKind(text, name) {
  if (elementBindings(text).has(name)) return 'element';
  if (arrayBindings(text).has(name)) return 'array';

  // The raw scan, not `declaration()`: that one attaches the type by calling
  // back into here, and the two would recurse until the stack gave out.
  const declared = declarations(text).find((d) => d.name === name);
  if (!declared || !declared.init) return null;
  const init = declared.init.trim();

  // `signal(x)` is identity, so the kind is whatever it wraps.
  const inner = /^signal\s*\((.*)\)$/s.exec(init);
  const value = inner ? inner[1].trim() : init;

  if (/^\[/.test(value)) return 'array';
  if (/^["'`]/.test(value)) return 'string';
  if (/^query\s*\(/.test(value)) return value.includes('[') ? 'element' : 'array';
  // A number, a boolean, a map, a call whose result is unknown: no answer.
  return null;
}

/**
 * Which names a `query()` result was bound to, so `let row = query(".r")[0]`
 * makes `row` an element for completion purposes.
 *
 * Deliberately shallow: one assignment, no flow analysis. Anything cleverer
 * would be guessing, and a wrong guess offers `scrollIntoView` on a number.
 */
function elementBindings(text) {
  const script = sectionBody(text, 'script');
  if (!script) return new Set();
  const names = new Set();
  // `\b` rather than a line start: `fn m() { let row = query(".r")[0]; … }` is
  // all one line, was not matched, and so `row.` offered nothing at all.
  const pattern = /\blet[ \t]+([A-Za-z_][\w]*)[ \t]*=[ \t]*query[ \t]*\([^)]*\)[ \t]*\[/g;
  let m;
  while ((m = pattern.exec(script.body)) !== null) names.add(m[1]);
  return names;
}

/** The names bound to a whole `query()` call, which is an array. */
function arrayBindings(text) {
  const script = sectionBody(text, 'script');
  if (!script) return new Set();
  const names = new Set();
  const pattern = /\blet[ \t]+([A-Za-z_][\w]*)[ \t]*=[ \t]*query[ \t]*\([^)]*\)[ \t]*;/g;
  let m;
  while ((m = pattern.exec(script.body)) !== null) names.add(m[1]);
  return names;
}

/** The text of one section, with the offset it starts at. */
function sectionBody(text, name) {
  const open = new RegExp(`<${name}[^>]*>`).exec(text);
  if (!open) return null;
  const start = open.index + open[0].length;
  const close = text.indexOf(`</${name}>`, start);
  return { body: text.slice(start, close === -1 ? text.length : close), start };
}

/**
 * The line with the contents of every string literal replaced by spaces.
 *
 * Only used for counting braces. A brace inside a string is text, and counting
 * it is how a single `"{"` in a template string convinced the scanner it was
 * inside a function body for the rest of the file.
 *
 * Unterminated strings are blanked to the end of the line, which is the common
 * mid-edit case and the one where guessing wrong is worst.
 */
function blankStrings(line) {
  let out = '';
  let quote = null;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quote) {
      // A backslash escape, so `"\""` does not read as two strings.
      if (c === '\\') {
        out += '  ';
        i++;
        continue;
      }
      out += c === quote ? c : ' ';
      if (c === quote) quote = null;
      continue;
    }
    if (c === '"' || c === "'" || c === '`') {
      quote = c;
      out += c;
      continue;
    }
    out += c;
  }
  return out;
}

/** Drop a `//` comment, so a name mentioned in prose is not read as a declaration. */
function stripComment(line) {
  const at = line.indexOf('//');
  return at === -1 ? line : line.slice(0, at);
}

function count(s, ch) {
  let n = 0;
  for (const c of s) if (c === ch) n++;
  return n;
}

function add(list, seen, entry) {
  if (seen.has(entry.name)) return;
  seen.add(entry.name);
  list.push(entry);
}

module.exports = {
  declarations,
  declaration,
  afterDot,
  receiverKind,
  loopVariables,
  memberReceiver,
  indexedReceiver,
  elementBindings,
  arrayBindings,
  sectionBody,
};
