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
const locals = require('./locals');
const snippets = require('./snippets');

/** Sort keys, so the useful things are not buried under the merely valid. */
const ORDER = {
  // What the author wrote themselves outranks anything the runtime provides.
  // Typing `dr` should reach the `draft` on line 4 before it reaches `debug`.
  local: '0',
  element: '1',
  component: '2',
  directive: '3',
  attribute: '4',
  value: '5',
  snippet: '6',
};

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
          // Between the sections, where the only useful thing is a section.
          return snippetItems(vscode, 'document');
      }
    },
  };

  // `<` and `/` open the tag lists, space and `:` and `@` the attribute ones,
  // and `-` so `r-` keeps the directive list up rather than dismissing it.
  return vscode.languages.registerCompletionItemProvider(
    'rux',
    provider,
    '<', '/', ' ', ':', '@', '-', '.', '#'
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

  // Inside `{{ … }}` or an attribute expression, the useful list is the
  // document's own state, not more markup.
  if (context.inTemplateExpression(text, offset)) {
    return expression(vscode, text, offset);
  }

  const tag = context.openTagAt(text, offset);
  if (!tag) return snippetItems(vscode, 'template');
  return tag.onName ? tags(vscode, text) : attributes(vscode, tag.tag);
}

/**
 * What can be written in an expression: an interpolation, a `:bound` attribute,
 * or a handler.
 *
 * The same names in all three because it is the same scope. A loop variable is
 * added when one is open, since inside an `r-for` row it is usually the whole
 * point of the expression.
 */
function expression(vscode, text, offset) {
  const member = members(vscode, text, offset);
  if (member) return member;

  const items = localItems(vscode, locals.declarations(text));
  for (const v of locals.loopVariables(text, offset)) {
    const item = new vscode.CompletionItem(v.name, vscode.CompletionItemKind.Variable);
    item.detail = v.detail;
    item.documentation = new vscode.MarkdownString(v.doc);
    item.sortText = ORDER.local + v.name;
    items.push(item);
  }
  return items.concat(globalItems(vscode));
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
    // The handlers are what a template is mostly made of, so they rank with
    // the directives rather than among `id` and `role`.
    item.sortText = (a.name.startsWith('@') ? ORDER.directive : ORDER.attribute) + a.name;
    // `fallback` is valueless; everything else takes one.
    item.insertText =
      a.name === 'fallback'
        ? a.name
        : new vscode.SnippetString(`${a.name}="$1"`);
    items.push(item);
  }

  return items;
}

// ── <style> ──────────────────────────────────────────────────────────────────

/**
 * Only properties the runtime honors. This is the completion that a general
 * CSS extension cannot give you correctly, because Rux honors a subset and
 * warns about the rest.
 */
function style(vscode, text, offset) {
  if (context.atPseudoClass(text, offset)) return pseudoClasses(vscode);

  const at = context.cssPositionAt(text, offset);
  if (at.where === 'value') return values(vscode, at.property);
  // Outside a rule is where a whole-rule snippet belongs, and where property
  // names would be wrong. Snippets used to be contributed statically by
  // `package.json`, so VS Code offered all 31 of them in every section: typing
  // `s` after `justify-content:` produced `script`, `signal`, `slot`, `sticky`
  // and `style` mixed in with the four values that were actually valid.
  if (at.where !== 'property') return selectors(vscode, text, offset);

  return vocabulary.cssProperties().map((name) => {
    const item = new vscode.CompletionItem(name, vscode.CompletionItemKind.Property);
    const described = vocabulary.cssProperty(name);
    // What it does, not that it is allowed. "honored by the runtime" was the
    // old text and it answered a question nobody asks.
    item.detail = described ? described.detail : 'honored by the runtime';
    if (described) {
      item.documentation = new vscode.MarkdownString(propertyHelp(name, described));
    }
    item.sortText = ORDER.attribute + name;
    item.insertText = new vscode.SnippetString(`${name}: $1;`);
    return item;
  });
}

/**
 * The popup beside a property in the completion list: what it does, a line you
 * could type, where it applies, and what it accepts.
 *
 * In that order deliberately. The example goes second because it is the fastest
 * answer to "how do I use this", and several of them carry the pairing that is
 * the actual lesson: `overflow` beside a `height`, `top` beside a
 * `position: sticky`, `align-items` beside the `display: flex` without which it
 * does nothing at all.
 *
 * Shared with the hover provider, so the same property never explains itself
 * two different ways.
 */
function propertyHelp(name, described) {
  const parts = [`**${described.detail}**`];
  if (described.usage) parts.push('```rux\n' + described.usage + '\n```');
  if (described.doc) parts.push(described.doc);

  const values = vocabulary.cssValues(name);
  if (values.length) {
    parts.push(`Takes ${values.map((v) => `\`${v}\``).join(', ')}.`);
  } else if (name === 'transition') {
    parts.push(
      '`transition: <property> <duration> <easing>`. Animatable: ' +
        vocabulary.animatableProperties().map((v) => `\`${v}\``).join(', ') +
        '. Naming anything else is a warning rather than silence.'
    );
  }

  // Said last and said plainly. It is worth knowing, and it is not what the
  // reader came for, so it does not get the top line the way it used to.
  parts.push('_Honored by the runtime; Rux implements a subset of CSS._');
  return parts.join('\n\n');
}

/**
 * The values for the property being typed, for the properties whose values are
 * a closed set of keywords.
 *
 * This is the completion that catches the mistake `position` spent four
 * releases making silently: before v0.7 `position: sticky` parsed, did not
 * match, and fell through to `relative`. Offering exactly the five words that
 * work is the editor saying what the runtime knows.
 */
function values(vscode, property) {
  if (!property) return undefined;

  // `transition: <property> <time> <easing>` is three vocabularies in one
  // value, and all three are worth having.
  if (property === 'transition') {
    const animatable = vocabulary.animatableProperties().map((name) => {
      const item = new vscode.CompletionItem(name, vscode.CompletionItemKind.Property);
      item.detail = 'can be animated';
      item.sortText = ORDER.attribute + name;
      return item;
    });
    const easings = vocabulary.easings().map((name) => {
      const item = new vscode.CompletionItem(name, vscode.CompletionItemKind.Value);
      item.detail = 'easing';
      item.sortText = ORDER.value + name;
      return item;
    });
    return animatable.concat(easings);
  }

  return vocabulary.cssValues(property).map((name) => {
    const item = new vscode.CompletionItem(name, vscode.CompletionItemKind.Value);
    item.detail = `${property} value`;
    item.sortText = ORDER.value + name;
    return item;
  });
}

/**
 * What can start a selector: the classes and ids the template actually writes,
 * and the element names a tag selector can match.
 *
 * Read from `<template>` rather than from the stylesheet, so what is offered is
 * what will match something. Completing from the sheet would offer back the
 * name of a rule that matches nothing, turning a dead rule into an endorsed
 * one, and a rule that silently never applies is the most expensive kind of
 * mistake this language has.
 */
function selectors(vscode, text, offset) {
  const before = text.slice(Math.max(0, offset - 2), offset);
  const { classes, ids } = context.templateSelectors(text);

  // `.` and `#` are unambiguous, so each offers only its own kind.
  if (/\.$/.test(before)) {
    return classes.map((name) => selectorItem(vscode, name, 'class', 'used in the template'));
  }
  if (/#$/.test(before)) {
    return ids.map((name) => selectorItem(vscode, name, 'id', 'used in the template'));
  }

  // A bare word is a tag selector. Only the built-in elements: a component's
  // tag never reaches the tree, because the component expands to whatever its
  // own root element is, so offering `side-panel` would be offering a selector
  // that cannot match.
  const items = vocabulary.elements().map((e) => {
    const item = new vscode.CompletionItem(e.name, vscode.CompletionItemKind.Class);
    item.detail = 'element selector';
    item.documentation = new vscode.MarkdownString(
      `Matches every \`<${e.name}>\` in this document.\n\n${e.doc}`
    );
    item.sortText = ORDER.element + e.name;
    return item;
  });

  // Offered with the dot and hash already written, so the list is reachable
  // without knowing to type the punctuation first.
  for (const name of classes) {
    items.push(selectorItem(vscode, `.${name}`, 'class', 'used in the template'));
  }
  for (const name of ids) {
    items.push(selectorItem(vscode, `#${name}`, 'id', 'used in the template'));
  }
  return items.concat(snippetItems(vscode, 'style'));
}

function selectorItem(vscode, label, kind, detail) {
  const item = new vscode.CompletionItem(
    label,
    kind === 'class' ? vscode.CompletionItemKind.Value : vscode.CompletionItemKind.Reference
  );
  item.detail = `${kind}, ${detail}`;
  item.documentation = new vscode.MarkdownString(
    kind === 'class'
      ? 'Written on at least one element in this file\'s `<template>`, either as ' +
        'a plain `class` or as a key of a bound `:class`.'
      : 'Written as an `id` in this file\'s `<template>`. Also what `query("#name")` looks up.'
  );
  item.sortText = ORDER.local + label;
  return item;
}

/**
 * The pseudo-classes, offered after a `:` in a selector.
 *
 * An unknown pseudo-class fails closed in the runtime: the rule parses and
 * never matches. So a wrong guess here is a rule that does nothing and says
 * nothing, which is why the list is the runtime's own rather than CSS's.
 */
function pseudoClasses(vscode) {
  return vocabulary.pseudoClasses().map((p) => {
    const item = new vscode.CompletionItem(p.name, vscode.CompletionItemKind.Keyword);
    item.detail = p.detail;
    item.documentation = new vscode.MarkdownString(p.doc);
    item.sortText = ORDER.directive + p.name;
    return item;
  });
}

// ── <script> ─────────────────────────────────────────────────────────────────

function script(vscode, text, offset, document) {
  const importing = usePathBeing(text, offset);
  if (importing !== null) return importPath(vscode, document, importing);

  const member = members(vscode, text, offset);
  if (member) return member;

  return localItems(vscode, locals.declarations(text))
    .concat(globalItems(vscode))
    .concat(snippetItems(vscode, 'script'));
}

/** The globals the runtime provides. */
function globalItems(vscode) {
  return vocabulary.scriptGlobals().map((g) => {
    const item = new vscode.CompletionItem(g.name, vscode.CompletionItemKind.Function);
    item.detail = g.detail;
    item.documentation = new vscode.MarkdownString(g.doc);
    item.sortText = ORDER.element + g.name;
    return item;
  });
}

/** What this document declared, which outranks everything else. */
function localItems(vscode, declared) {
  const KIND = {
    signal: vscode.CompletionItemKind.Variable,
    computed: vscode.CompletionItemKind.Constant,
    binding: vscode.CompletionItemKind.Variable,
    function: vscode.CompletionItemKind.Function,
  };
  return declared.map((d) => {
    const item = new vscode.CompletionItem(d.name, KIND[d.kind] || vscode.CompletionItemKind.Variable);
    item.detail = d.detail;
    item.documentation = new vscode.MarkdownString(d.doc);
    item.sortText = ORDER.local + d.name;
    // A function is being called, so put the cursor between its parentheses.
    if (d.kind === 'function') {
      item.insertText = new vscode.SnippetString(`${d.name}($1)`);
    }
    return item;
  });
}

/**
 * What is available after a `.`, or `null` when the cursor is not at one.
 *
 * Only two receivers can be resolved without type inference, and both are
 * resolved from a single assignment rather than from flow: an element handle
 * (`query(…)[0]`) and the array `query()` itself returns. Everything else gets
 * the string and array methods, which are the ones a `.` in a Rux file is
 * usually reaching for. Being wrong here is cheap in one direction and not the
 * other, so nothing is offered that would not exist on *some* value.
 */
function members(vscode, text, offset) {
  // A dot decides this on its own. The two probes below can only recognise a
  // receiver they can *name*, and after `search_item.map().` neither did, so
  // this returned null and the caller went on to offer every global in the
  // language — `back`, `blur`, `navigate` — none of which can follow a dot.
  //
  // Not knowing which members is a reason to offer a smaller list, never a
  // reason to offer an unrelated one.
  if (!locals.afterDot(text, offset)) return null;

  const indexed = locals.indexedReceiver(text, offset);
  const receiver = locals.memberReceiver(text, offset);

  // What the receiver actually holds, read from its declaration.
  const kind = indexed
    ? 'element'
    : receiver
      ? locals.receiverKind(text, receiver)
      : null;

  if (kind === 'element') {
    return vocabulary.elementMembers().map((m) => {
      const item = new vscode.CompletionItem(
        m.name,
        m.kind === 'method'
          ? vscode.CompletionItemKind.Method
          : vscode.CompletionItemKind.Property
      );
      item.detail = m.detail;
      item.documentation = new vscode.MarkdownString(m.doc);
      item.sortText = ORDER.local + m.name;
      if (m.kind === 'method') item.insertText = new vscode.SnippetString(`${m.name}()`);
      return item;
    });
  }

  if (kind === 'array' || kind === 'string') {
    return vocabulary
      .valueMethods()
      .filter((m) => appliesTo(m.name, kind))
      .map((m) => {
        const item = new vscode.CompletionItem(
          m.name,
          m.name === 'length'
            ? vscode.CompletionItemKind.Property
            : vscode.CompletionItemKind.Method
        );
        item.detail = m.detail;
        item.documentation = new vscode.MarkdownString(m.doc);
        item.sortText = ORDER.local + m.name;
        if (m.name !== 'length') item.insertText = new vscode.SnippetString(`${m.name}($1)`);
        return item;
      });
  }

  // The declaration does not say what this holds, so **nothing** is offered.
  //
  // The tempting answer is the string and array methods, on the grounds that a
  // dot is usually one of those. It is wrong, and a user caught it within
  // minutes: `let handle = setInterval(2000) { … }` holds a timer handle, and
  // the list cheerfully offered `charAt`, `map` and `join` on it. Rux has no
  // type annotations, so an editor that guesses here is endorsing calls that
  // cannot work — the same failure as offering an unhonored CSS property, which
  // is the thing this entire vocabulary exists to prevent.
  //
  // An empty list lets VS Code fall back to words from the document, which are
  // at least the author's own.
  return [];
}

/** Whether a built-in method exists on arrays, on strings, or on both. */
function appliesTo(method, kind) {
  const ARRAY_ONLY = ['map', 'filter', 'reduce', 'forEach', 'find', 'join'];
  const STRING_ONLY = ['charAt', 'repeat', 'split', 'startsWith', 'endsWith', 'trim', 'toLowerCase', 'toUpperCase'];
  if (ARRAY_ONLY.includes(method)) return kind === 'array';
  if (STRING_ONLY.includes(method)) return kind === 'string';
  // `includes`, `indexOf`, `slice` and `length` are on both.
  return true;
}

/** The snippets belonging to one section. */
function snippetItems(vscode, section) {
  return snippets.forSection(section).map((sn) => {
    const item = new vscode.CompletionItem(sn.prefix, vscode.CompletionItemKind.Snippet);
    item.detail = sn.title;
    item.documentation = new vscode.MarkdownString(sn.description);
    item.sortText = ORDER.snippet + sn.prefix;
    item.insertText = new vscode.SnippetString(sn.body);
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
//
// `template` and `style` are exported for the same reason one step removed:
// what they decide to offer is the whole feature, and the only alternative to
// calling them is booting an editor to look at a popup.
module.exports = { register, usePathBeing, importPath, template, style, propertyHelp };
