// What the type checker worked out, as `rux check --format json --types`
// prints it, kept per file for hover, completion and the parameter quick-fix.
//
// The types live only in the Rust checker, so the extension does not work them
// out; it reads what the check it already runs on open and save reports. That
// makes every answer here as old as the last save. A table is looked up by name
// and line rather than by exact position, so an edit a few lines up moves the
// answers only as far as the lines moved, and a name that is not on the line
// any more is simply not answered. See `docs/10-types.md`, "Editor".

const path = require('path');

/** file key -> { types, guesses } */
const tables = new Map();

function key(file) {
  return path.resolve(file).replace(/\\/g, '/').toLowerCase();
}

/**
 * Keep what one `rux check --types` run said. `output` is the parsed JSON
 * object; every file it mentions replaces what was kept for that file, and
 * `checked` is emptied even when nothing in it was typed.
 */
function remember(checked, output) {
  const byFile = new Map([[key(checked), { types: [], guesses: [] }]]);
  const slot = (file) => {
    const k = key(file);
    if (!byFile.has(k)) byFile.set(k, { types: [], guesses: [] });
    return byFile.get(k);
  };
  for (const t of output.types || []) slot(t.file).types.push(t);
  for (const g of output.guesses || []) slot(g.file).guesses.push(g);
  for (const [k, table] of byFile) tables.set(k, table);
}

function forget(file) {
  tables.delete(key(file));
}

function table(file) {
  return (file && tables.get(key(file))) || null;
}

/**
 * The entry for `name` on 1-based `line` nearest 1-based `column`, or null.
 * An entry with no column (a template piece, a function's parameters) stands
 * for the whole line.
 */
function at(file, line, column, name) {
  const t = table(file);
  if (!t) return null;
  const here = t.types.filter((e) => e.line === line && e.name === name);
  if (here.length === 0) return null;
  const distance = (e) => (e.column == null ? 1e6 : Math.abs(e.column - column));
  return here.reduce((best, e) => (distance(e) < distance(best) ? e : best));
}

/**
 * The entry for the chain `chain` (`t.owner`) nearest 1-based `line`,
 * preferring one at or above it: what completion after `t.owner.` asks for.
 * Only entries that say something about fields count.
 */
function chain(file, line, chainText) {
  const t = table(file);
  if (!t) return null;
  let best = null;
  let bestScore = Infinity;
  for (const e of t.types) {
    if (e.path !== chainText) continue;
    // Above the cursor is where the value was last seen; below is a guess.
    const score = e.line <= line ? line - e.line : (e.line - line) * 4 + 1;
    if (score < bestScore) {
      best = e;
      bestScore = score;
    }
  }
  return best;
}

/** The parameter guesses for the function written on 1-based `line`. */
function guesses(file, line) {
  const t = table(file);
  if (!t) return [];
  return t.guesses.filter((g) => g.line === line);
}

/**
 * Where `: T` goes for `param` of `fn name(` in `lines`, searching up from
 * 0-based `from` for the function's head. Returns `{ line, character }`, or
 * null when the parameter is already annotated or cannot be found.
 */
function insertionPoint(lines, from, name, param) {
  for (let l = from; l >= Math.max(0, from - 5); l--) {
    const text = lines[l];
    const head = new RegExp(`\\bfn\\s+${escape(name)}\\s*\\(`).exec(text);
    if (!head) continue;
    const open = head.index + head[0].length;
    const close = text.indexOf(')', open);
    const list = text.slice(open, close === -1 ? text.length : close);
    const found = new RegExp(`(^|[\\s,])(${escape(param)})\\b(\\s*)(:?)`).exec(list);
    if (!found || found[4] === ':') return null;
    return { line: l, character: open + found.index + found[1].length + found[2].length };
  }
  return null;
}

function escape(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/**
 * The chain a `.` or `?.` at `offset` follows, with every `?` dropped: `t` for
 * `t.`, `t.owner` for `t?.owner?.`. `optional` says whether the dot being
 * typed was written `?.`. Null when what precedes the dot is not a plain chain
 * of names (a call, an index), which the table has no path for.
 */
function chainBefore(text, offset) {
  let i = offset;
  while (i > 0 && /\w/.test(text[i - 1])) i--;
  if (text[i - 1] !== '.') return null;
  const dot = i - 1;
  const optional = text[dot - 1] === '?';
  let j = optional ? dot - 1 : dot;
  const end = j;
  while (j > 0 && /[\w.?]/.test(text[j - 1])) j--;
  const written = text.slice(j, end);
  if (!/^[A-Za-z_]\w*(\??\.\w+)*$/.test(written)) return null;
  return { chain: written.replace(/\?/g, ''), optional, dot, typed: text.slice(dot + 1, offset) };
}

module.exports = { remember, forget, at, chain, guesses, insertionPoint, chainBefore };
