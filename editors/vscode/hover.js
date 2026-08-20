// Hover documentation, from the same vocabulary the completions come from.
//
// The completion list already carries a `documentation` for every entry, and
// that popup is only on screen while the list is. Once the word is written,
// the question "what does `r-key` actually do" has nowhere to go but the docs
// site, and the answer is two lines long.
//
// The rule this shares with completions: everything shown here is something the
// runtime honors. There is no fallback to "looks like CSS, here is what CSS
// says", because the whole point of a Rux-specific vocabulary is that Rux
// honors a subset and the general answer would be wrong exactly where it
// matters most.

const context = require('./context');
const vocabulary = require('./vocabulary');
const locals = require('./locals');
// For `propertyHelp`, so a property's explanation has exactly one source.
const completion = require('./completion');

function register(vscode) {
  return vscode.languages.registerHoverProvider('rux', {
    provideHover(document, position) {
      const text = document.getText();
      const offset = document.offsetAt(position);
      const section = context.sectionAt(text, offset);
      // Markup keeps the whole dotted word (a tag may contain one); an
      // expression wants only the segment under the cursor.
      const inExpr =
        section === 'script' || context.inTemplateExpression(text, offset);
      const at = inExpr ? context.memberAt(text, offset) : context.wordAt(text, offset);
      if (!at) return undefined;

      const found = lookUp(section, text, at);
      if (!found) return undefined;

      const md = new vscode.MarkdownString();
      md.appendMarkdown(`**${found.title}** — ${found.detail}\n\n${found.doc}`);
      const range = new vscode.Range(
        document.positionAt(at.start),
        document.positionAt(at.end)
      );
      return new vscode.Hover(md, range);
    },
  });
}

/** What `word` means in `section`, or `null` if this is not a word we know. */
function lookUp(section, text, at) {
  switch (section) {
    case 'template':
      return inTemplate(text, at);
    case 'style':
      return inStyle(text, at);
    case 'script':
      return inScript(text, at);
    default:
      return null;
  }
}

function inTemplate(text, at) {
  const word = at.word;

  // Inside `{{ … }}`, a `:bound` value or a handler, the word is script, not
  // markup, and the answer is whatever the document declared. Checked first
  // because some names collide with attribute names: a signal called `title`
  // is not the `title` attribute.
  if (context.inTemplateExpression(text, at.start)) {
    return inExpression(text, at);
  }

  // A tag is a word with a `<` or `</` immediately before it. Checking that is
  // what keeps `view` inside `class="view"` from being documented as an
  // element, which would be confidently wrong rather than merely unhelpful.
  const isTag = /<\/?$/.test(text.slice(Math.max(0, at.start - 2), at.start));
  if (isTag) {
    const element = vocabulary.elements().find((e) => e.name === word);
    if (element) return entry(`<${word}>`, element);

    const component = context.importedComponents(text).find((c) => c.tag === word);
    if (component) {
      return {
        title: `<${word}>`,
        detail: 'component',
        doc:
          `Imported from \`${component.file}\`. Props are passed bound ` +
          `(\`:label="title"\`) and evaluated in this file's scope; what it ` +
          'sends back arrives as `@name` handlers, from its `emit`.',
      };
    }
    return null;
  }

  // `:label` and `:class` are the bound form of whatever follows the colon, so
  // a hover on one should answer for the attribute rather than say nothing.
  const bound = text[at.start - 1] === ':';

  const directive = vocabulary.directives().find((d) => d.name === word);
  if (directive) return entry(word, directive);

  // Inside `r-for="item in items"`, which is a form of its own rather than an
  // expression, so it is not covered above. Both halves are worth answering:
  // this is where the loop variable is introduced, and so where someone is
  // most likely to ask what it is.
  const loop = inForDeclaration(text, at);
  if (loop) return loop;

  // The element this attribute is written on, so `src` on an `<image>` and
  // `path` on a `<route>` each get their own answer.
  const tag = context.openTagAt(text, at.start);
  const own = tag ? vocabulary.attributesFor(tag.tag) : [];
  const attribute =
    own.find((a) => a.name === word) ||
    vocabulary.globalAttributes().find((a) => a.name === word);
  if (!attribute) return null;

  const found = entry(bound ? `:${word}` : word, attribute);
  if (bound) {
    found.doc += '\n\nThe leading `:` makes the value an expression, re-evaluated when the signals it reads change.';
  }
  return found;
}

/**
 * `r-for="item in items"`: the name being introduced, or the list it comes
 * from. `null` when the cursor is not inside such an attribute.
 */
function inForDeclaration(text, at) {
  const pattern = /r-for[ 	]*=[ 	]*"([^"]*)"/g;
  let m;
  while ((m = pattern.exec(text)) !== null) {
    const open = m.index + m[0].indexOf('"') + 1;
    const close = open + m[1].length;
    if (at.start < open || at.end > close) continue;

    const parsed = /^\s*([A-Za-z_][\w]*)\s+in\s+(.+?)\s*$/.exec(m[1]);
    if (!parsed) return null;
    const [, name, list] = parsed;
    if (at.word === name) {
      return {
        title: name,
        detail: 'the current row',
        doc:
          `One item of \`${list}\`, in scope on this element and everything ` +
          'inside it, and nowhere else.\n\nPair the loop with `r-key` so a ' +
          'change can be matched row to row; without one a keyed change is a ' +
          'rebuild, and input state inside a row moves to the wrong row.',
      };
    }
    if (at.word === list) {
      return {
        title: list,
        detail: 'the list being repeated over',
        doc: `Each item is bound to \`${name}\` for this element's subtree.`,
      };
    }
    return null;
  }
  return null;
}

function inStyle(text, at) {
  // `:hover` and the rest, recognised by the colon the word does not include.
  const colons = /:{1,2}$/.exec(text.slice(Math.max(0, at.start - 2), at.start));
  if (colons) {
    const pseudo = vocabulary.pseudoClasses().find((p) => p.name === at.word);
    return pseudo ? entry(`:${at.word}`, pseudo) : null;
  }

  if (context.cssPositionAt(text, at.start).where === 'selector') {
    return inSelector(text, at);
  }

  const where = context.cssPositionAt(text, at.start).where;
  if (where === 'property' && vocabulary.cssProperties().includes(at.word)) {
    const described = vocabulary.cssProperty(at.word);
    if (!described) {
      return { title: at.word, detail: 'honored by the runtime', doc: valueHelp(at.word) };
    }
    // The same text the completion popup shows, built by the same function, so
    // a property cannot explain itself one way in the list and another on hover.
    return {
      title: at.word,
      detail: described.detail,
      doc: completion.propertyHelp(at.word, described),
    };
  }
  return null;
}

/**
 * A selector: what it matches, and **whether anything in this file's template
 * actually has it**.
 *
 * That second half is the reason this is worth writing. A rule whose selector
 * matches nothing is silent: it parses, it cascades, it applies to no box, and
 * nothing anywhere says so. It is the most expensive shape of mistake in this
 * language, and hovering the selector is the moment to catch it.
 *
 * The answer is scoped to this file's `<template>` and says so, because a rule
 * here may legitimately be styling a component's markup: a document's rules
 * reach the components it uses unless the sheet is `scoped`.
 */
function inSelector(text, at) {
  const { classes, ids } = context.templateSelectors(text);
  const before = text.slice(Math.max(0, at.start - 1), at.start);

  // `wordAt` keeps a leading `.` (it is a word character for tags) and drops a
  // leading `#` (it is not), so both spellings have to be recognised.
  const isClass = at.word.startsWith('.') || before === '.';
  const isId = before === '#';
  const name = at.word.replace(/^\./, '');

  if (isClass) {
    const used = classes.includes(name);
    return {
      title: `.${name}`,
      detail: used ? 'a class used in this template' : 'a class nothing here has',
      doc: used
        ? `Matches every element written with \`class="${name}"\`, and every one ` +
          `whose bound \`:class\` turns \`${name}\` on.`
        : `**No element in this file's \`<template>\` carries \`${name}\`.** The rule ` +
          'parses and applies to nothing, which is silent: there is no warning ' +
          'for a selector that matches no box.\n\nThat is fine if it is meant for ' +
          "a component's markup, since a document's rules reach the components it " +
          'uses unless the sheet is `scoped`. Otherwise it is a typo.',
    };
  }

  if (isId) {
    const used = ids.includes(name);
    return {
      title: `#${name}`,
      detail: used ? 'an id used in this template' : 'an id nothing here has',
      doc: used
        ? `Matches the element written with \`id="${name}"\`, and it is what ` +
          `\`query("#${name}")\` looks up from a handler.`
        : `**No element in this file's \`<template>\` carries \`id="${name}"\`.** ` +
          'The rule applies to nothing, and says nothing about it.',
    };
  }

  const element = vocabulary.elements().find((e) => e.name === at.word);
  if (element) {
    const written = new RegExp(`<${at.word}[\\s/>]`).test(text);
    return {
      title: at.word,
      detail: 'an element selector',
      doc:
        `Matches every \`<${at.word}>\` in reach of this sheet.\n\n${element.doc}` +
        (written
          ? ''
          : `\n\n**This file's \`<template>\` writes no \`<${at.word}>\`.** Which is ` +
            'fine if the rule is for a component, and a typo otherwise.'),
    };
  }

  // A component tag written as a selector never matches: a component expands to
  // whatever its own root element is, so its tag never reaches the tree.
  const component = context.importedComponents(text).find((c) => c.tag === at.word);
  if (component) {
    return {
      title: at.word,
      detail: 'a component tag, which is not a selector',
      doc:
        `\`<${at.word}>\` is a component. It expands to whatever its own root ` +
        'element is, so **no box in the tree ever has this tag** and this rule ' +
        'matches nothing.\n\nStyle it with a class instead: put one on the ' +
        `component's root inside \`${component.file}\`, or pass one in.`,
    };
  }
  return null;
}

/** The values a property takes, when they are a set worth listing. */
function valueHelp(property) {
  const values = vocabulary.cssValues(property);
  if (values.length) {
    return `Takes ${values.map((v) => `\`${v}\``).join(', ')}.`;
  }
  if (property === 'transition') {
    const animatable = vocabulary.animatableProperties();
    return (
      '`transition: <property> <duration> <easing>`. Animatable: ' +
      animatable.map((v) => `\`${v}\``).join(', ') +
      '.\n\nA property not in that list does nothing, and says so as a warning ' +
      'rather than failing quietly.'
    );
  }
  return 'Its value is not a fixed set of keywords, so there is nothing to list.';
}

/**
 * The words the script language is made of.
 *
 * `let` had no hover at all, which is a strange gap: it is the first keyword
 * anyone types and the one whose meaning here differs most from the languages
 * it is borrowed from. A top-level `let` in Rux is not a local variable, it is
 * a **signal**, and reading it in a binding subscribes to it. That is worth
 * saying where someone points at it.
 */
const KEYWORDS = {
  let: {
    detail: 'declare a binding',
    doc:
      'At the **top level of `<script>` a `let` is a signal**: reading it in a ' +
      'binding or an interpolation subscribes to it, and writing it repaints ' +
      'whatever read it. Wrap the value in `signal(…)` to say so explicitly; ' +
      '`signal` is identity and exists to mark the declaration.\n\nInside a ' +
      'function body a `let` is an ordinary local, scoped to that body.',
  },
  fn: {
    detail: 'declare a function',
    doc:
      'A function can read **and write** the signals around it, which is what ' +
      'lets a handler have a name instead of being an inline expression. That ' +
      'is a v0.7 change: before lexical scoping landed, a `fn` could not touch ' +
      'a signal at all.',
  },
  computed: {
    detail: 'computed name = expr;',
    doc:
      'Derived state. A **declaration, not a call**. Refreshed in one pass in ' +
      'declaration order, so it may read a computed declared above it and not ' +
      'below.',
  },
  return: {
    detail: 'return from a function',
    doc:
      'In a **route guard** the returned value is the decision: `false` cancels ' +
      'the navigation, a string redirects to that path, and anything else ' +
      'allows it, including falling off the end.',
  },
  if: { detail: 'a conditional', doc: 'Ordinary control flow. In a template, use `r-if` instead.' },
  else: { detail: 'the other branch', doc: 'Pairs with `if`. In a template, `r-else`.' },
  for: { detail: 'a loop', doc: 'Ordinary control flow. To repeat markup, use `r-for`.' },
  in: { detail: 'part of `for` and `r-for`', doc: '`for x in xs`, and `r-for="x in xs"`.' },
  while: { detail: 'a loop', doc: 'Runs while its condition holds.' },
  break: { detail: 'leave the loop', doc: 'Stops the nearest enclosing loop.' },
  continue: { detail: 'skip to the next turn', doc: 'Goes straight to the next iteration.' },
};

function inScript(text, at) {
  const keyword = KEYWORDS[at.word];
  if (keyword && isKeywordPosition(text, at)) {
    return { title: at.word, detail: keyword.detail, doc: keyword.doc };
  }

  const imported = inUseStatement(text, at);
  if (imported) return imported;

  // A lambda's parameter: `search_item.map(a => { a.length })`. Not a
  // declaration and not a global, so it used to answer nothing at all, in a
  // position where "what is `a`" is the obvious question.
  const parameter = inLambda(text, at);
  if (parameter) return parameter;

  const member = asMember(text, at);
  if (member) return member;

  // The document's own names first. A `let print = …` shadowing the global is
  // pathological and the answer would still be the local one, which is what
  // the reader is looking at.
  const declared = locals.declaration(text, at.word);
  if (declared) return fromDeclaration(declared);

  const global = vocabulary.scriptGlobals().find((g) => g.name === at.word);
  return global ? entry(at.word, global) : null;
}

/**
 * The `use` keyword, or any segment of the path after it.
 *
 * Worth answering because the mapping is not guessable from the syntax: the
 * path names a **file relative to this one**, an underscore in it becomes a
 * hyphen in the tag, and the tag is the only thing the template ever sees.
 * Three rules, none of them visible in `use components::crew_detail;`.
 */
function inUseStatement(text, at) {
  const lineStart = text.lastIndexOf('\n', at.start - 1) + 1;
  let lineEnd = text.indexOf('\n', at.start);
  if (lineEnd === -1) lineEnd = text.length;
  const line = text.slice(lineStart, lineEnd);

  const parsed = /^[ \t]*use[ \t]+([A-Za-z_][A-Za-z0-9_:]*)?/.exec(line);
  if (!parsed) return null;

  const path = parsed[1] || '';
  const segments = path.split('::').filter(Boolean);
  const leaf = segments.length ? segments[segments.length - 1] : null;
  const file = segments.length ? `${segments.join('/')}.rux` : null;
  const tag = leaf ? leaf.replace(/_/g, '-') : null;

  const shared =
    'The path names a **file relative to this one**, not a package: ' +
    '`components::crew_detail` is `components/crew_detail.rux` beside this ' +
    'file.\n\n**An underscore becomes a hyphen in the tag**, so that one is ' +
    'written `<crew-detail>`. That is the runtime\'s rule, not a convention.\n\n' +
    'Imports only reach **downward**, into this file\'s own directory and below. ' +
    'There is no `super::` and no `..`, so a file in a parent directory cannot ' +
    'be imported at all.';

  if (at.word === 'use') {
    return {
      title: 'use',
      detail: 'import a component',
      doc: file
        ? `Makes \`${file}\` available in this file's template as \`<${tag}>\`.\n\n${shared}`
        : shared,
    };
  }

  if (leaf && segments.includes(at.word)) {
    const isLeaf = at.word === leaf;
    return {
      title: at.word,
      detail: isLeaf ? `the component, written <${tag}>` : 'a folder in the path',
      doc: isLeaf
        ? `\`${file}\`, usable as \`<${tag}>\`.\n\nProps are passed bound ` +
          `(\`:label="title"\`), evaluated in this file's scope; what it sends ` +
          'back arrives as `@name` handlers, from its `emit`.'
        : `A directory beside this file. The whole path resolves to \`${file}\`.`,
    };
  }
  return null;
}

/** The same question, asked from inside a template expression. */
function inExpression(text, at) {
  const member = asMember(text, at);
  if (member) return member;

  const declared = locals.declaration(text, at.word);
  if (declared) return fromDeclaration(declared);

  const loop = locals.loopVariables(text, at.start).find((v) => v.name === at.word);
  if (loop) return { title: at.word, detail: loop.detail, doc: loop.doc };

  const global = vocabulary.scriptGlobals().find((g) => g.name === at.word);
  return global ? entry(at.word, global) : null;
}

/**
 * A word immediately after a `.`, answered from the element API or the value
 * methods.
 *
 * Which of the two is decided the same way completion decides it, so the hover
 * and the list cannot disagree about what `row.` is.
 */
function asMember(text, at) {
  // `at.receiver` is set by `memberAt` when the word was `a.b`; the text probe
  // is the fallback for `query(…)[0].b`, where there is no plain identifier in
  // front of the dot for the splitter to find.
  const indexed = locals.indexedReceiver(text, at.start);
  if (!at.receiver && !indexed) return null;
  const receiver = at.receiver ? at.receiver.split('.').pop() : null;

  // The same inference the completion list uses, so hover cannot describe a
  // method the list would not have offered on this receiver.
  const kind = indexed ? 'element' : receiver ? locals.receiverKind(text, receiver) : null;
  const isElement = kind === 'element';

  if (kind) {
    const list = isElement ? vocabulary.elementMembers() : vocabulary.valueMethods();
    const found = list.find((m) => m.name === at.word);
    if (found) {
      return {
        title: isElement ? `element.${at.word}` : at.word,
        detail: found.detail,
        doc: found.doc,
      };
    }
  }

  // The receiver's type is unknown, but the *member* may still be a name the
  // runtime provides. Completion must stay silent here, because offering
  // `charAt` on a timer handle endorses a call that cannot work — but hover is
  // answering about a word already written, and "what is `map`" has an answer
  // whatever it was written on. Saying nothing was the wrong trade: it left
  // every built-in method undocumented in exactly the place someone asks.
  const anyMember =
    vocabulary.elementMembers().find((m) => m.name === at.word) ||
    vocabulary.valueMethods().find((m) => m.name === at.word);
  if (anyMember) {
    const onElement = vocabulary.elementMembers().some((m) => m.name === at.word);
    const where = onElement
      ? 'On an element handle, from `query()`.'
      : `On ${appliesToText(at.word)}.`;
    return {
      title: at.word,
      detail: anyMember.detail,
      doc: `${anyMember.doc}\n\n${where}`,
    };
  }

  // Not a name the runtime provides. If the receiver is something this file
  // declared, it is a property read on that value, and saying so is worth more
  // than saying nothing: it names the thing to go and check, and it is the
  // truth. Rux has no type annotations, so this is as far as an editor can
  // honestly go without inventing a type.
  if (!receiver) return null;
  const declared = locals.declaration(text, receiver);
  const loop = locals.loopVariables(text, at.start).find((v) => v.name === receiver);
  if (!declared && !loop) return null;
  return {
    title: `${receiver}.${at.word}`,
    detail: 'a property read',
    doc:
      `Reads \`${at.word}\` from \`${receiver}\`, which is ` +
      (declared ? `${declared.detail} declared in this file` : 'the current row') +
      '.\n\nThere are no type annotations in Rux, so what a value carries is ' +
      'whatever was put in it; this cannot be checked here. A property that is ' +
      'genuinely absent **raises** rather than evaluating to nothing, so write ' +
      `\`${receiver}?.${at.word}\` or \`"${at.word}" in ${receiver}\` if absent ` +
      'is a legitimate answer.',
  };
}

/**
 * Whether the word is being used as a keyword rather than as a name.
 *
 * A signal called `count` is not the keyword `continue`, and a property called
 * `in` is not the loop word. Being wrong here means confidently explaining the
 * wrong thing, so the check is "is it followed by what that keyword requires",
 * and anything unclear falls through to the ordinary lookups.
 */
function isKeywordPosition(text, at) {
  // A member never is: `a.in` is a property read.
  if (at.receiver) return false;
  // Nor is anything immediately preceded by a dot.
  if (/\.\s*$/.test(text.slice(Math.max(0, at.start - 2), at.start))) return false;
  return true;
}

/**
 * A lambda parameter: the `a` in `xs.map(a => …)` or `xs.reduce(|acc, x| …)`.
 *
 * Only parameters whose lambda encloses the cursor are answered. A parameter
 * from an unrelated lambda earlier in the file is not in scope, and offering it
 * would be the same mistake as offering a loop variable outside its row.
 */
function inLambda(text, at) {
  const before = text.slice(0, at.start);

  // The nearest unclosed `(` before the cursor, so the lambda being described
  // is the one the cursor is inside.
  let depth = 0;
  let open = -1;
  for (let i = before.length - 1; i >= 0; i--) {
    const c = before[i];
    if (c === ')') depth++;
    else if (c === '(') {
      if (depth === 0) {
        open = i;
        break;
      }
      depth--;
    }
  }
  if (open === -1) return null;

  // From the `(` forwards, not up to the cursor: the cursor is often *on* the
  // parameter, and slicing to it left the name outside the window being
  // matched, so the parameter it was asking about was never found.
  const head = text.slice(open + 1, open + 200);
  // `a =>`, `(a, b) =>`, and rhai's `|a, b|`.
  const arrow = /^\s*\(?\s*([A-Za-z_][\w]*(?:\s*,\s*[A-Za-z_][\w]*)*)\s*\)?\s*=>/.exec(head);
  const pipes = /^\s*\|\s*([A-Za-z_][\w]*(?:\s*,\s*[A-Za-z_][\w]*)*)\s*\|/.exec(head);
  const found = arrow || pipes;
  if (!found) return null;

  const names = found[1].split(',').map((n) => n.trim());
  if (!names.includes(at.word)) return null;

  // What the lambda was called on, and therefore what its parameter holds.
  // Every trailing `(` is stripped, not just one: `forEach((item, i) => …)`
  // puts the parameter list in its own parentheses, so the call's `(` is one
  // further out and a single strip left the method name unreachable.
  const receiver = /([A-Za-z_][\w]*)\s*\.\s*([A-Za-z_][\w]*)\s*$/.exec(
    text.slice(0, open + 1).replace(/[(\s]+$/, '')
  );
  const over = receiver ? receiver[1] : null;
  const method = receiver ? receiver[2] : null;

  // `reduce` is the one whose first parameter is not an item: it carries the
  // running total, and its *second* is the item. Everywhere else the second
  // parameter is the index.
  const position = names.indexOf(at.word);
  let role = 'one item of the list';
  if (method === 'reduce') {
    role = position === 0 ? 'the accumulator carried between turns' : 'one item of the list';
  } else if (position === 1) {
    role = 'the index of the current item';
  }

  return {
    title: at.word,
    detail: `a parameter of this ${method ? `\`${method}\`` : 'lambda'}`,
    doc:
      `${role[0].toUpperCase()}${role.slice(1)}` +
      (over ? `, from \`${over}\`.` : '.') +
      '\n\nIn scope inside this lambda only. Rux has no type annotations, so ' +
      'what it holds is whatever the list holds; the editor cannot check a ' +
      'property read on it.',
  };
}

/** Which kinds of value a built-in method exists on, in prose. */
function appliesToText(method) {
  const ARRAY_ONLY = ['map', 'filter', 'reduce', 'forEach', 'find', 'join'];
  const STRING_ONLY = [
    'charAt', 'repeat', 'split', 'startsWith', 'endsWith', 'trim',
    'toLowerCase', 'toUpperCase',
  ];
  if (ARRAY_ONLY.includes(method)) return 'arrays';
  if (STRING_ONLY.includes(method)) return 'strings';
  return 'arrays and strings';
}

/**
 * What a name declared in this file is.
 *
 * The initialiser is shown because for a signal it is the only type
 * information there is: Rux has no annotations, so `signal("")` is how you know
 * `draft` holds a string.
 */
function fromDeclaration(d) {
  const where = `Declared on line ${d.line} of this file.`;
  if (d.kind === 'function') {
    return {
      title: `${d.name}(${d.params})`,
      detail: 'function in this file',
      doc: `${where}\n\nCall it from a handler, a binding or another function.`,
    };
  }
  const shown = d.init ? `\n\n\`\`\`rux\n${declarationText(d)}\n\`\`\`` : '';

  // What it holds, inferred from the initialiser, and what that gives you.
  // Rux has no type annotations, so the declaration is the only evidence there
  // is, and this is the same inference the completion list uses to decide what
  // a `.` may offer. Saying it here is what makes that decision legible instead
  // of mysterious.
  const kind = d.type ? typeNote(d.type) : null;

  return {
    title: d.name,
    detail: label(d.kind) + (d.type ? `, holding ${article(d.type)}` : ''),
    doc: `${where}${shown}\n\n${d.doc}${kind ? `\n\n${kind}` : ''}`,
  };
}

function article(type) {
  // 'an array', not 'a array'.
  if (type === 'element') return 'an element';
  return /^[aeiou]/.test(type) ? `an ${type}` : `a ${type}`;
}

/** What an inferred type buys you, in the editor and in the language. */
function typeNote(type) {
  if (type === 'array') {
    return 'Its initialiser is a list, so `.` offers the array methods and `.length`.';
  }
  if (type === 'string') {
    return 'Its initialiser is a string, so `.` offers the string methods and `.length`.';
  }
  if (type === 'element') {
    return (
      'It holds an element handle from `query()`, so `.` offers that handle\'s ' +
      'reads and actions. The reads come from the frame already on screen and ' +
      'are one frame stale, which is the guarantee rather than a defect.'
    );
  }
  return null;
}

/** The declaration written back out, as the author wrote it. */
function declarationText(d) {
  if (d.kind === 'signal') return `let ${d.name} = signal(${d.init});`;
  if (d.kind === 'computed') return `computed ${d.name} = ${d.init};`;
  return `let ${d.name} = ${d.init};`;
}

function label(kind) {
  if (kind === 'signal') return 'signal — reactive state';
  if (kind === 'computed') return 'computed — derived state';
  return 'let — a binding';
}

/** A vocabulary entry, in the shape the hover renderer wants. */
function entry(title, e) {
  return { title, detail: e.detail, doc: e.doc };
}

module.exports = { register, lookUp };
