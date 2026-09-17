// Which comment Ctrl+/ writes, decided by the section the cursor is in.
//
// A `.rux` file is three languages in one file and VS Code's comment commands
// read one set of rules per *language*. So the manifest's static
// `language-configuration.json` had to pick one, it picked `//`, and Ctrl+/ in
// `<template>` wrote `// <view>` — which is not a comment, it is a syntax
// error, and the same keystroke in `<style>` wrote `//` into CSS, where it is
// also not a comment and is silently dropped by the parser rather than
// reported. Both were quiet: the file still looked commented.
//
// The fix is the one Vue's tooling uses: re-declare the language configuration
// as the cursor moves. It is global to the language rather than per-editor, so
// what it follows is the *active* cursor, which is the one about to press the
// key.
//
// Line comments are deliberately absent for two of the three. Neither HTML-style
// markup nor CSS has one, and VS Code's Toggle Line Comment falls back to the
// block comment when a language declares none — so Ctrl+/ in a template wraps
// the selection in `<!-- -->` instead of writing something that does not work.

const context = require('./context');

/**
 * What a comment is, per section.
 *
 * The gaps between sections take the template's rules: a note written above
 * `<style>` is markup-level, and `<!-- -->` parses there (the SFC splitter
 * looks for the section tags and ignores what sits between them).
 */
const RULES = {
  template: { blockComment: ['<!--', '-->'] },
  style: { blockComment: ['/*', '*/'] },
  script: { lineComment: '//', blockComment: ['/*', '*/'] },
};

/** The rules for `section`, with the between-sections case folded in. */
function rulesFor(section) {
  return RULES[section] || RULES.template;
}

/**
 * Keep the `rux` language configuration pointed at the cursor's section.
 *
 * Returns a disposable that tears down both the listeners and whatever
 * configuration was last applied.
 */
function register(vscode) {
  let applied = null;
  let current = null;

  const apply = (editor) => {
    if (!editor || editor.document.languageId !== 'rux') return;
    const section = context.sectionAt(
      editor.document.getText(),
      editor.document.offsetAt(editor.selection.active)
    );
    // Nothing to do while the cursor stays in one section, and this fires on
    // every cursor movement.
    if (section === current && applied) return;
    current = section;
    if (applied) applied.dispose();
    // Only `comments`. Registered configurations are merged rather than
    // replaced, so the brackets, auto-closing pairs and indentation rules in
    // `language-configuration.json` stay in force; restating them here would
    // mean restating the indentation regexes as `RegExp` objects, and a second
    // copy of those is exactly the drift this project has paid for before.
    applied = vscode.languages.setLanguageConfiguration('rux', {
      comments: rulesFor(section),
    });
  };

  const listeners = [
    vscode.window.onDidChangeTextEditorSelection((e) => apply(e.textEditor)),
    vscode.window.onDidChangeActiveTextEditor(apply),
  ];
  apply(vscode.window.activeTextEditor);

  return {
    dispose() {
      listeners.forEach((l) => l.dispose());
      if (applied) applied.dispose();
    },
  };
}

module.exports = { register, rulesFor, RULES };
