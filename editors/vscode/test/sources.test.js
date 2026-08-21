// The extension's own source files are well-formed.
//
// This exists because of a specific accident, hit three times in one session
// while editing these files through a shell heredoc: a `\b` written in a regex
// arrived as a literal **backspace** (U+0008) instead of the two characters
// `\` and `b`. The file still parsed, the regex still compiled, and it simply
// never matched anything:
//
//     const literal = /\bclass\s*=/    became    /<BS>class\s*=/
//
// Nothing catches that. There is no syntax error, no lint, and no test failure
// unless a test happens to cover the exact pattern. It cost two rounds of "the
// fix does not work" before the bytes were looked at directly.
//
// The same class of accident produces a real newline where `\n` was meant,
// which is a syntax error inside a normal string and *silent* inside a template
// literal.

const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');

const ROOT = path.join(__dirname, '..');

/** Every JavaScript and JSON file the extension ships. */
function sourceFiles() {
  const found = [];
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      if (entry.name === 'node_modules' || entry.name === 'superseded') continue;
      if (entry.name.startsWith('.')) continue;
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        walk(full);
      } else if (/\.(js|json)$/.test(entry.name)) {
        found.push(full);
      }
    }
  };
  walk(ROOT);
  return found;
}

test('no source file carries a stray control character', () => {
  // Tab, newline and carriage return are the only ones that belong in source.
  const allowed = new Set(['\t', '\n', '\r']);
  const offenders = [];

  for (const file of sourceFiles()) {
    const text = fs.readFileSync(file, 'utf8');
    for (let i = 0; i < text.length; i++) {
      const code = text.charCodeAt(i);
      if (code < 32 && !allowed.has(text[i])) {
        const line = text.slice(0, i).split('\n').length;
        offenders.push(
          `${path.relative(ROOT, file)}:${line} has U+${code
            .toString(16)
            .padStart(4, '0')} ` +
            `(likely an escape such as \\b or \\f that was written as a literal ` +
            `character; the regex or string will never behave as intended)`
        );
      }
    }
  }

  assert.deepEqual(offenders, [], `\n${offenders.join('\n')}`);
});

test('every shipped module loads', () => {
  // A module that throws on require disables the whole extension, and the only
  // signal is that nothing works. Cheap to check, and it caught a broken string
  // literal in `hover.js` before it was packaged.
  for (const file of sourceFiles()) {
    if (!file.endsWith('.js')) continue;
    if (file.includes(`${path.sep}test${path.sep}`)) continue;
    assert.doesNotThrow(() => require(file), `${path.relative(ROOT, file)} does not load`);
  }
});

test('every JSON file the extension ships parses', () => {
  for (const file of sourceFiles()) {
    if (!file.endsWith('.json')) continue;
    assert.doesNotThrow(
      () => JSON.parse(fs.readFileSync(file, 'utf8')),
      `${path.relative(ROOT, file)} is not valid JSON`
    );
  }
});
