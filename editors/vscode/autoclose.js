// Auto-closing tags: typing `<view>` writes `</view>` and leaves the cursor
// between them, and typing `</` finishes the nearest tag still open.
//
// VS Code does this for HTML and JSX in the built-in language extensions, not
// in the editor core, so a language contributed by an extension gets none of it
// for free. `language-configuration.json` can auto-close brackets and quotes
// and it cannot auto-close tags: there is no declarative form for "the closing
// text depends on what was typed". So it is done here, on text changes.
//
// The one rule this must not get wrong is `<image>`. Rux's void tags come from
// `rux vocab`, which reads them from `crates/rux-fmt`, which is the same list
// the formatter indents by. Writing `</image>` would produce a document the
// parser rejects, which is worse than doing nothing at all.

const context = require('./context');
const vocabulary = require('./vocabulary');

/**
 * Watch a document for the two keystrokes worth reacting to.
 *
 * Returns the disposable, so `activate` can register it and forget about it.
 */
function register(vscode, context_) {
  return vscode.workspace.onDidChangeTextDocument((event) => {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document !== event.document) return;
    if (event.document.languageId !== 'rux') return;
    if (!vscode.workspace.getConfiguration('rux').get('autoClosingTags', true)) return;

    // Exactly one change, exactly one character typed. Pastes, multi-cursor
    // edits and undo all arrive here too, and none of them should sprout tags.
    if (event.contentChanges.length !== 1) return;
    const change = event.contentChanges[0];
    if (change.text !== '>' && change.text !== '/') return;
    if (change.rangeLength !== 0) return;
    if (editor.selections.length !== 1) return;

    const document = event.document;
    const after = document.positionAt(document.offsetAt(change.range.start) + change.text.length);

    if (change.text === '>') closeOpenedTag(vscode, editor, document, after);
    else finishClosingTag(vscode, editor, document, after);
  });
}

/**
 * `<view>` just became complete: insert `</view>` after the cursor.
 */
function closeOpenedTag(vscode, editor, document, position) {
  const text = document.getText();
  const offset = document.offsetAt(position);
  if (context.sectionAt(text, offset) !== 'template') return;

  const before = text.slice(0, offset);
  // The tag that just closed, with its attributes. Quotes are skipped so a
  // `>` inside `placeholder="a > b"` is not mistaken for the end of a tag.
  const opened = /<([A-Za-z][\w.-]*)((?:"[^"]*"|'[^']*'|[^<>"'])*)>$/.exec(before);
  if (!opened) return;
  const [, tag, attrs] = opened;

  if (attrs.trimEnd().endsWith('/')) return; // `<image />` closed itself
  if (vocabulary.isVoid(tag)) return; // `<image src="…">` never nests
  if (tag === 'template' || tag === 'style' || tag === 'script') return; // the snippets own these

  // If the very next thing is already this tag's closing form, the author is
  // editing an existing element rather than writing a new one.
  if (/^\s*<\//.test(text.slice(offset))) return;

  // A snippet rather than an edit, because `$0` is how the cursor is left
  // *before* the inserted text instead of after it.
  editor.insertSnippet(new vscode.SnippetString(`$0</${tag}>`), position, {
    undoStopBefore: false,
    undoStopAfter: true,
  });
}

/**
 * `</` was just typed: finish it with whatever is still open, and nothing if
 * that is nothing. Guessing here would be worse than leaving the author to it.
 */
function finishClosingTag(vscode, editor, document, position) {
  const text = document.getText();
  const offset = document.offsetAt(position);
  if (context.sectionAt(text, offset) !== 'template') return;
  if (!text.slice(0, offset).endsWith('</')) return;

  const open = context.unclosedTagAt(text, offset - 2, vocabulary.isVoid);
  if (!open) return;

  editor.insertSnippet(new vscode.SnippetString(`${open}>$0`), position, {
    undoStopBefore: false,
    undoStopAfter: true,
  });
}

module.exports = { register };
