// Completions for `.rux`: tags, attributes, directives, honored CSS, and the
// script globals.
//
// Everything offered comes from `vocabulary.js`, which comes from `rux vocab`,
// which reads the honored-CSS list out of the same slice the runtime's
// unhonored-property warning consults. That is the whole point of the
// arrangement: **if the editor offers it, it works.** A completion list that
// suggests `float` and then leaves a warning in the dev overlay would be worse
// than no completion list.
//
// Component tags are the exception, and they are read from the open document
// rather than from the binary: `use components::crew_detail;` in `<script>`
// means `<crew-detail>` is available in `<template>`, and only this file knows
// that.

const fs = require('fs');
const path = require('path');

const context = require('./context');
const vocabulary = require('./vocabulary');

/** Sort keys, so the useful things are not buried under the merely valid. */
const ORDER = { element: '0', component: '1', directive: '2', attribute: '3', value: '4' };

function register(vscode) {
  const provider = {
    provideCompletionItems(document, position) {
      const text = document.getText();
      const offset = document.offsetAt(position);
      switch (context.sectionAt(text, offset)) {
        case 'template':
          return template(vscode, text, offset);
        case 'style':
          return style(vscode, text, offset);
        case 'script':
          return script(vscode, text, offset, document);
        default:
          return undefined;
      }
    },
  };

  // `<` and `/` open the tag lists, space and `:` and `@` the attribute ones,
  // and `-` so `r-` keeps the directive list up rather than dismissing it.
  return vscode.languages.registerCompletionItemProvider(
    'rux',
    provider,
    '<', '/', ' ', ':', '@', '-'
  );
}

// ── <template> ───────────────────────────────────────────────────────────────

function template(vscode, text, offset) {
  const before = text.slice(0, offset);

  // `</` offers the tag that is actually open, and only that one. A list of
  // every element here would be a list of mostly-wrong answers.
  const closing = /<\/([A-Za-z][\w.-]*)?$/.exec(before);
  if (closing) {
    const open = context.unclosedTagAt(text, closing.index, vocabulary.isVoid);
    if (!open) return undefined;
    const item = new vscode.CompletionItem(open, vscode.CompletionItemKind.Property);
    item.detail = 'close this element';
    item.insertText = `${open}>`;
    return [item];
  }

  const tag = context.openTagAt(text, offset);
  if (!tag) return undefined;
  return tag.onName ? tags(vscode, text) : attributes(vscode, tag.tag);
}

/** Every element, plus every component this document imported. */
function tags(vscode, text) {
  const items = vocabulary.elements().map((e) => {
    const item = new vscode.CompletionItem(e.name, vscode.CompletionItemKind.Class);
    item.detail = e.detail;
    item.documentation = new vscode.MarkdownString(e.doc);
    item.sortText = ORDER.element + e.name;
    return item;
  });

  for (const component of context.importedComponents(text)) {
    const item = new vscode.CompletionItem(component.tag, vscode.CompletionItemKind.Module);
    item.detail = 'component';
    item.documentation = new vscode.MarkdownString(
      `Imported from \`${component.file}\`. Props are passed bound: ` +
        `\`:label="title"\`, evaluated in this file's scope.`
    );
    item.sortText = ORDER.component + component.tag;
    items.push(item);
  }
  return items;
}

/** Directives first, then this element's own attributes, then the global ones. */
function attributes(vscode, tag) {
  const items = [];

  for (const d of vocabulary.directives()) {
    const item = new vscode.CompletionItem(d.name, vscode.CompletionItemKind.Keyword);
    item.detail = d.detail;
    item.documentation = new vscode.MarkdownString(d.doc);
    item.sortText = ORDER.directive + d.name;
    item.insertText = new vscode.SnippetString(`${d.name}="$1"`);
    items.push(item);
  }

  const own = vocabulary.attributesFor(tag);
  for (const a of own.concat(vocabulary.globalAttributes())) {
    const item = new vscode.CompletionItem(a.name, vscode.CompletionItemKind.Property);
    item.detail = a.detail;
    item.documentation = new vscode.MarkdownString(a.doc);
    item.sortText = ORDER.attribute + a.name;
    // `fallback` is valueless; everything else takes one.
    item.insertText =
      a.name === 'fallback'
        ? a.name
        : new vscode.SnippetString(`${a.name}="$1"`);
    items.push(item);
  }

  // `@tap` is the entire event vocabulary. Saying so in the completion list is
  // more honest than letting someone type `@click` and wait for nothing.
  const tap = new vscode.CompletionItem('@tap', vscode.CompletionItemKind.Event);
  tap.detail = 'run script when this element is tapped';
  tap.documentation = new vscode.MarkdownString(
    'The only event Rux has. There are no pointer, hover, key or gesture events: ' +
      '`@tap` covers a mouse click and a finger, and that is the whole vocabulary today.'
  );
  tap.sortText = ORDER.directive + '@tap';
  tap.insertText = new vscode.SnippetString('@tap="$1"');
  items.push(tap);

  return items;
}

// ── <style> ──────────────────────────────────────────────────────────────────

/**
 * Only properties the runtime honors. This is the completion that a general
 * CSS extension cannot give you correctly, because Rux honors a subset and
 * warns about the rest.
 */
function style(vscode, text, offset) {
  if (!context.inCssDeclaration(text, offset)) return undefined;
  return vocabulary.cssProperties().map((name) => {
    const item = new vscode.CompletionItem(name, vscode.CompletionItemKind.Property);
    item.detail = 'honored by the runtime';
    item.sortText = ORDER.attribute + name;
    item.insertText = new vscode.SnippetString(`${name}: $1;`);
    return item;
  });
}

// ── <script> ─────────────────────────────────────────────────────────────────

function script(vscode, text, offset, document) {
  const importing = usePathBeing(text, offset);
  if (importing !== null) return importPath(vscode, document, importing);

  return vocabulary.scriptGlobals().map((g) => {
    const item = new vscode.CompletionItem(g.name, vscode.CompletionItemKind.Function);
    item.detail = g.detail;
    item.documentation = new vscode.MarkdownString(g.doc);
    item.sortText = ORDER.element + g.name;
    return item;
  });
}

// ── `use` paths ──────────────────────────────────────────────────────────────

/**
 * If the cursor sits in a `use …` path, return the part typed so far.
 *
 * `""` right after `use `, `"components::"` after the separator, and
 * `"components::hea"` part-way through a name. `null` when this is not a `use`
 * line at all, which is the common case and has to stay cheap.
 */
function usePathBeing(text, offset) {
  const lineStart = text.lastIndexOf('\n', offset - 1) + 1;
  const line = text.slice(lineStart, offset);
  const m = /^[ \t]*use[ \t]+([A-Za-z0-9_:]*)$/.exec(line);
  return m ? m[1] : null;
}

/**
 * The importable names under the path typed so far.
 *
 * The rules are the runtime's, not a guess: `use components::task;` names the
 * file `components/task.rux` **relative to the importing document**, and a `_`
 * in the path becomes a `-` in the tag. So directories map to `::` segments and
 * `.rux` files map to leaf names, and neither is invented here.
 *
 * A directory is only offered if there is a `.rux` file somewhere under it.
 * Offering `assets::` because it exists would be offering a dead end.
 */
function importPath(vscode, document, typed) {
  if (!document || document.uri.scheme !== 'file') return undefined;

  const segments = typed.split('::');
  // The last segment is what is being typed; the ones before it are the folder.
  const partial = segments.pop();
  const dir = path.join(path.dirname(document.uri.fsPath), ...segments);

  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch (e) {
    return undefined; // not a directory yet; nothing to offer
  }

  const items = [];
  for (const entry of entries) {
    if (entry.name.startsWith('.')) continue;

    if (entry.isDirectory()) {
      if (!holdsRux(path.join(dir, entry.name), 0)) continue;
      const item = new vscode.CompletionItem(entry.name, vscode.CompletionItemKind.Folder);
      item.detail = 'folder';
      item.insertText = `${entry.name}::`;
      item.sortText = '1' + entry.name;
      // Reopen the list after `::` so the next level can be picked without
      // retyping. This is the whole reason the separator is a trigger character.
      item.command = { command: 'editor.action.triggerSuggest', title: 'suggest' };
      items.push(item);
      continue;
    }

    if (!entry.name.endsWith('.rux')) continue;
    // A document importing itself is legal to type and never useful.
    if (path.join(dir, entry.name) === document.uri.fsPath) continue;

    const stem = entry.name.slice(0, -'.rux'.length);
    const item = new vscode.CompletionItem(stem, vscode.CompletionItemKind.Module);
    item.detail = `component <${stem.replace(/_/g, '-')}>`;
    item.documentation = new vscode.MarkdownString(
      `Imports \`${[...segments, entry.name].join('/')}\`, usable as ` +
        `\`<${stem.replace(/_/g, '-')} />\`.\n\n` +
        (stem.includes('_')
          ? 'The underscore becomes a hyphen in the tag; that is the runtime\'s rule, not a convention.'
          : '')
    );
    item.sortText = '0' + stem;
    items.push(item);
  }

  // `partial` is left to VS Code to filter on, which keeps the list narrowing as
  // you type without this having to re-read the directory on every keystroke.
  void partial;
  return items;
}

/** Whether a directory holds a `.rux` file, looking a few levels down. */
function holdsRux(dir, depth) {
  if (depth > 3) return false; // a component tree that deep is not a thing
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch (e) {
    return false;
  }
  return entries.some(
    (e) =>
      (e.isFile() && e.name.endsWith('.rux')) ||
      (e.isDirectory() && !e.name.startsWith('.') && holdsRux(path.join(dir, e.name), depth + 1))
  );
}

// `importPath` is exported for the tests, which run it against a real directory
// tree with a stand-in for the VS Code API. It reads the filesystem, so testing
// it any other way would be testing a mock.
module.exports = { register, usePathBeing, importPath };
