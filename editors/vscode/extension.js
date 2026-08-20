// Rux VS Code extension: completions, tag auto-closing, formatting and
// diagnostics. The last two shell out to the `rux` binary.
//
// This file used to carry its own re-indenter, a port of the one in
// `crates/rux-fmt`. Two implementations of the same rules drifted within a week:
// the JS copy inherited HTML's void-tag list, which has `img` but not Rux's
// `<image>`, so an `<image src="...">` written without a self-closing slash
// over-indented everything after it. It also never formatted CSS, which the Rust
// side does. Both copies are now one copy, behind `rux fmt`.
//
// The cost is a hard dependency on the binary being installed. That is the right
// trade: an editor that formats differently from the project's own tool is worse
// than one that says it cannot find the tool.

const { spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const autoclose = require('./autoclose');
const completion = require('./completion');
const definition = require('./definition');
const hover = require('./hover');
const symbols = require('./symbols');
const vocabulary = require('./vocabulary');
// Named with a trailing underscore only because `context` is already the
// extension context in `activate`, and shadowing that would be worse.
const context_ = require('./context');
const locals = require('./locals');

/**
 * When this module was loaded into the extension host.
 *
 * VS Code reads an extension's JavaScript once, at activation, and never again.
 * Installing a new build into a running window therefore changes the files on
 * disk and changes nothing about what the editor is executing until the window
 * is reloaded — and every symptom of that looks exactly like a broken feature.
 * This project has now spent two sessions on "verified working, reported broken"
 * loops, and a stale extension host is the one explanation neither side can see.
 *
 * So: remember when the code started, compare it with the code on disk, and say
 * so plainly in the diagnostic.
 */
const LOADED_AT = Date.now();


/**
 * Whether the extension's own source on disk is newer than the copy running.
 *
 * Returns the newest such file and its age, or `null` when the running code is
 * current. See `LOADED_AT`: this is the difference between "the feature is
 * broken" and "the window has not been reloaded since the build was installed",
 * and it is invisible from either side without asking.
 */
function stale() {
  let newest = null;
  for (const name of [
    'extension.js', 'completion.js', 'hover.js', 'locals.js', 'context.js',
    'vocabulary.js', 'vocabulary.json', 'definition.js', 'symbols.js',
    'snippets.js', 'autoclose.js', 'package.json',
  ]) {
    let at;
    try {
      at = fs.statSync(path.join(__dirname, name)).mtimeMs;
    } catch (e) {
      continue; // not shipped in this build
    }
    // A second of slack: the install writes these files and starts the host at
    // very nearly the same moment, and a build that lost by 40ms is not stale.
    if (at > LOADED_AT + 1000 && (!newest || at > newest.at)) newest = { name, at };
  }
  return newest;
}

/** Read from the manifest, so the diagnostic cannot claim the wrong version. */
const EXTENSION_VERSION = (() => {
  try {
    return require('./package.json').version;
  } catch (e) {
    return 'unknown';
  }
})();

/**
 * Quote a path for a shell.
 *
 * The repo this was developed in lives under `…/UI research`, and that space
 * has already broken one launcher in this project: `Start-Process
 * -ArgumentList` split it into two arguments and silently opened no window. A
 * path with a space is the normal case on Windows, not the edge case.
 */
function quoted(value) {
  return /[\s"']/.test(value) ? '"' + value.replace(/"/g, '\\"') + '"' : value;
}

/** Configured path to the `rux` binary. */
function ruxPath(vscode) {
  return vscode.workspace.getConfiguration('rux').get('path') || 'rux';
}

/**
 * Run `rux` with `args`, optionally writing `input` to its stdin.
 * Returns `{ ok, stdout, stderr, code }`; `ok` is false only when the binary
 * could not be run at all, which is a different problem from it exiting non-zero.
 */
function runRux(vscode, args, input, cwd) {
  const result = spawnSync(ruxPath(vscode), args, {
    input,
    cwd,
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
  });
  if (result.error) {
    return { ok: false, stdout: '', stderr: String(result.error.message), code: null };
  }
  return {
    ok: true,
    stdout: result.stdout || '',
    stderr: result.stderr || '',
    code: result.status,
  };
}

/**
 * Complain once per session that the binary is missing, with the fix. Repeating
 * it on every keystroke would be its own bug.
 */
function makeMissingBinaryNotice(vscode) {
  let shown = false;
  return () => {
    if (shown) return;
    shown = true;
    vscode.window.showWarningMessage(
      `Rux: could not run \`${ruxPath(vscode)}\`. Formatting and diagnostics need it. ` +
        'Install it with `cargo install ruxlang`, or set `rux.path` to the binary.'
    );
  };
}


/**
 * The `--indent` argument, or nothing at all.
 *
 * Nothing at all is the default, and it is the whole point. This used to pass
 * the editor's own `tabSize`, which is 4 unless something says otherwise, while
 * `rux fmt` defaults to 2 and every file in the Rux repo is 2. So opening any
 * `.rux` file and formatting it silently re-indented the entire document to 4,
 * and `rux fmt --check` in CI then rejected the result. One example file was
 * reformatted end to end that way and very nearly went into a release: 336
 * lines changed, not one of them a change in content.
 *
 * The header of this file already records the rule that was broken here. There
 * was once a re-indenter in JavaScript beside the one in `crates/rux-fmt`, the
 * two drifted within a week, and the fix was to have exactly one implementation
 * behind `rux fmt`. Handing that one implementation a different indent from the
 * one it uses on the command line rebuilds the same disagreement out of one
 * copy: same code, two answers, and the editor's answer fails the project's own
 * check.
 *
 * So the binary decides, and the editor asks for nothing. `rux.format.indent`
 * exists for someone who genuinely wants tabs or a different width, and setting
 * it is a deliberate act rather than a default inherited from an unrelated
 * language's tab size.
 */
function indentArgs(vscode, options) {
  const configured = vscode.workspace.getConfiguration('rux').get('format.indent');
  if (!configured || configured === 'auto') return [];
  if (configured === 'editor') {
    return ['--indent', options && options.insertSpaces === false ? 'tab' : String((options && options.tabSize) || 2)];
  }
  return ['--indent', String(configured)];
}

function activate(context) {
  const vscode = require('vscode');
  const noticeMissingBinary = makeMissingBinaryNotice(vscode);
  const diagnostics = vscode.languages.createDiagnosticCollection('rux');
  context.subscriptions.push(diagnostics);

  // ── Completions and tag auto-closing ───────────────────────────────────────
  // Both read the vocabulary, which is bundled with the extension so that they
  // work before the binary is installed, and refreshed from `rux vocab` when it
  // is there so that someone on a branch build gets their branch's vocabulary.
  // This is the one place the binary is optional: a missing `rux` costs newer
  // completions, not completions.
  vocabulary.refreshFromBinary((args) => runRux(vscode, args, undefined, undefined));
  context.subscriptions.push(
    completion.register(vscode),
    autoclose.register(vscode, context),
    // Hover answers the question the completion popup answered a minute ago and
    // then took away; the other two are the clerical work of a tree of files
    // that refer to each other by name rather than by path.
    hover.register(vscode),
    definition.register(vscode),
    symbols.register(vscode)
  );

  // Which vocabulary is in force. `vocabulary.js` prefers whatever `rux vocab`
  // prints on this machine over the copy it shipped with, and when a completion
  // is missing, "which of the two am I looking at" is the first question worth
  // being able to answer without reading source.
  context.subscriptions.push(
    vscode.commands.registerCommand('rux.showVocabulary', () => {
      vscode.window.showInformationMessage(`Rux vocabulary: ${vocabulary.source()}.`);
    })
  );

  // ── Diagnosing the editor itself ───────────────────────────────────────────
  //
  // This exists because of a stretch of this project where a feature was
  // reported broken, verified working by calling its provider directly, and
  // reported broken again — six times over. Every check ran against code the
  // editor was not reaching, and there was no way to see that from either side.
  //
  // So: put the cursor somewhere and ask what the extension thinks is there.
  // It answers with the version in force, the vocabulary in force, and what
  // each provider returns at that exact position. A report from this is worth
  // more than any amount of describing a symptom.
  context.subscriptions.push(
    vscode.commands.registerCommand('rux.diagnose', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor || editor.document.languageId !== 'rux') {
        vscode.window.showWarningMessage('Rux: put the cursor in a `.rux` file first.');
        return;
      }
      const document = editor.document;
      const position = editor.selection.active;
      const text = document.getText();
      const offset = document.offsetAt(position);

      const section = context_.sectionAt(text, offset);
      const inExpression = context_.inTemplateExpression(text, offset);
      const at = section === 'script' || inExpression
        ? context_.memberAt(text, offset)
        : context_.wordAt(text, offset);

      const declared = locals.declarations(text);
      const hovered = at ? hover.lookUp(section, text, at) : null;

      let completions = [];
      try {
        const provider = completion.register({
          ...vscode,
          languages: { registerCompletionItemProvider: (_l, p) => p },
        });
        completions = (provider.provideCompletionItems(document, position) || []).map((i) => i.label);
      } catch (e) {
        completions = [`(threw: ${e.message})`];
      }

      // The same two questions, asked of **VS Code** rather than of this file.
      //
      // Everything above is the provider called directly, which answers "what
      // would this code return" — and that is precisely the answer that has
      // been right while the editor was wrong, over and over. It cannot see a
      // stale extension host, a provider that failed to register, another
      // extension outranking this one, or a suggestion the widget filtered out
      // after we handed it over.
      //
      // These two go through the editor's own provider registry and return what
      // it actually served at this position. When the two halves disagree, the
      // fault is in the editor's side of the wire and not in the vocabulary,
      // and that is the single most useful thing this command can establish.
      const served = async (command, ...args) => {
        try {
          return await vscode.commands.executeCommand(command, document.uri, position, ...args);
        } catch (e) {
          return { failed: e.message };
        }
      };

      const servedCompletions = await served('vscode.executeCompletionItemProvider');
      const servedHover = await served('vscode.executeHoverProvider');

      const labels = (list) => {
        if (!list || list.failed) return `(failed: ${list && list.failed})`;
        return (list.items || []).map((i) =>
          typeof i.label === 'string' ? i.label : i.label.label
        );
      };
      const editorOffers = labels(servedCompletions);
      const editorHover =
        Array.isArray(servedHover) && servedHover.length
          ? servedHover
              .flatMap((h) => h.contents.map((c) => (typeof c === 'string' ? c : c.value)))
              .join(' ')
              .replace(/\s+/g, ' ')
              .slice(0, 160)
          : '(nothing)';

      const missing = Array.isArray(editorOffers)
        ? completions.filter((l) => !editorOffers.includes(l))
        : [];

      const outOfDate = stale();

      const lines = [
        `extension        ${EXTENSION_VERSION}`,
        `code on disk     ${
          outOfDate
            ? `NEWER THAN THIS WINDOW — ${outOfDate.name} was written after the ` +
              'extension host loaded. Run "Developer: Reload Window"; until you ' +
              'do, the editor is running the previous build and nothing below ' +
              'reflects the code you have installed.'
            : 'matches what is running'
        }`,
        `vocabulary       ${vocabulary.source()}`,
        `rux binary       ${ruxPath(vscode)}`,
        '',
        `line, column     ${position.line + 1}, ${position.character + 1}`,
        `section          ${section || '(between sections)'}`,
        `expression       ${inExpression}`,
        `word at cursor   ${at ? JSON.stringify(at.word) : '(none)'}`,
        `receiver         ${at && at.receiver ? JSON.stringify(at.receiver) : '(none)'}`,
        '',
        `declared here    ${declared.length ? declared.map((d) => `${d.name} (${d.kind}${d.type ? ', ' + d.type : ''})`).join(', ') : '(none found)'}`,
        '',
        `hover says       ${hovered ? `${hovered.title} — ${hovered.detail}` : '(nothing)'}`,
        `completions      ${completions.length} item(s)`,
        `                 ${completions.slice(0, 20).join(' ')}${completions.length > 20 ? ' …' : ''}`,
        '',
        '── and what the editor actually served here ──',
        `hover            ${editorHover}`,
        `completions      ${
          Array.isArray(editorOffers) ? `${editorOffers.length} item(s)` : editorOffers
        }`,
        `                 ${
          Array.isArray(editorOffers)
            ? editorOffers.slice(0, 20).join(' ') + (editorOffers.length > 20 ? ' …' : '')
            : ''
        }`,
        `dropped          ${
          missing.length
            ? `${missing.length} item(s) this file offered that the editor did not ` +
              `serve: ${missing.slice(0, 20).join(' ')}`
            : '(nothing — the editor served everything this code offered)'
        }`,
      ];

      const shown = await vscode.workspace.openTextDocument({
        content: lines.join('\n'),
        language: 'text',
      });
      await vscode.window.showTextDocument(shown, { preview: true });
    })
  );

  // ── Running a document ─────────────────────────────────────────────────────
  //
  // In a terminal rather than through `spawn`, deliberately. `rux <file>` opens
  // a window and **keeps running**: it is a watcher as much as a launcher, and
  // it hot-reloads on save. Capturing that into an output channel would give a
  // process nobody can see, nobody can Ctrl-C, and whose warnings arrive after
  // the fact. A terminal is the thing that already solves all three.
  //
  // One terminal per session, reused, so driving a file half a dozen times does
  // not leave half a dozen dead panes behind.
  let terminal = null;
  const runTerminal = () => {
    if (terminal && terminal.exitStatus === undefined) return terminal;
    terminal = vscode.window.createTerminal({ name: 'Rux' });
    return terminal;
  };
  context.subscriptions.push({
    dispose: () => {
      if (terminal) terminal.dispose();
    },
  });

  /**
   * The `.rux` file a command should act on: the one right-clicked in the
   * explorer, or failing that the active editor.
   */
  const targetFile = (uri) => {
    if (uri && uri.fsPath && uri.fsPath.endsWith('.rux')) return uri.fsPath;
    const editor = vscode.window.activeTextEditor;
    if (editor && editor.document.languageId === 'rux' && editor.document.uri.scheme === 'file') {
      return editor.document.uri.fsPath;
    }
    return null;
  };

  context.subscriptions.push(
    vscode.commands.registerCommand('rux.run', async (uri) => {
      const file = targetFile(uri);
      if (!file) {
        vscode.window.showWarningMessage('Rux: open or select a `.rux` file to run.');
        return;
      }
      // Saved first, because `rux` reads the file from disk and resolves `use`
      // imports relative to it. Running the last saved version of a file you
      // are looking at is the kind of confusion that costs an hour.
      const open = vscode.workspace.textDocuments.find((d) => d.uri.fsPath === file);
      if (open && open.isDirty) await open.save();

      const shell = runTerminal();
      shell.show(true);
      shell.sendText(`${quoted(ruxPath(vscode))} ${quoted(file)}`);
    }),

    vscode.commands.registerCommand('rux.check', async (uri) => {
      const file = targetFile(uri);
      if (!file) {
        vscode.window.showWarningMessage('Rux: open or select a `.rux` file to check.');
        return;
      }
      const open = vscode.workspace.textDocuments.find((d) => d.uri.fsPath === file);
      if (open && open.isDirty) await open.save();

      const result = runRux(vscode, ['check', file], undefined, path.dirname(file));
      if (!result.ok) {
        noticeMissingBinary();
        return;
      }
      const said = (result.stdout + result.stderr).trim();
      if (result.code === 0) {
        vscode.window.showInformationMessage(`Rux: ${said || 'no problems found'}`);
      } else {
        vscode.window.showErrorMessage(`Rux: ${said || 'check failed'}`);
      }
      // The squiggles come from the same binary, so refresh them rather than
      // leaving the panel disagreeing with the message just shown.
      if (open) refresh(open);
    })
  );

  // ── Formatting ─────────────────────────────────────────────────────────────
  // The document is piped in rather than read from disk, because what needs
  // formatting is the buffer, which is usually unsaved.
  const formatter = {
    provideDocumentFormattingEdits(document, options) {
      const result = runRux(vscode, ['fmt', ...indentArgs(vscode, options), '-'], document.getText(), workspaceDir(vscode, document));
      if (!result.ok) {
        noticeMissingBinary();
        return [];
      }
      // A non-zero exit means it refused to format. Returning no edits leaves
      // the buffer alone, which is the only safe answer: half a document would
      // be worse than an unformatted one.
      if (result.code !== 0) {
        vscode.window.showErrorMessage(`Rux: ${result.stderr.trim() || 'formatting failed'}`);
        return [];
      }
      const full = new vscode.Range(
        document.positionAt(0),
        document.positionAt(document.getText().length)
      );
      return [vscode.TextEdit.replace(full, result.stdout)];
    },
  };
  context.subscriptions.push(
    vscode.languages.registerDocumentFormattingEditProvider('rux', formatter)
  );

  // ── Diagnostics ────────────────────────────────────────────────────────────
  // `rux check` reads the file from disk, and deliberately so: it resolves
  // `use` imports relative to the file's own directory, which a buffer piped in
  // over stdin no longer has. So diagnostics refresh when a document is opened
  // and when it is saved, not on every keystroke.
  function refresh(document) {
    if (!document || document.languageId !== 'rux' || document.uri.scheme !== 'file') return;
    if (!vscode.workspace.getConfiguration('rux').get('check.enable', true)) {
      diagnostics.delete(document.uri);
      return;
    }
    const file = document.uri.fsPath;
    const result = runRux(vscode, ['check', '--format', 'json', file], undefined, path.dirname(file));
    if (!result.ok) {
      noticeMissingBinary();
      return;
    }

    let found;
    try {
      found = JSON.parse(result.stdout || '[]');
    } catch (e) {
      // Exit code 2 is a usage error, where stdout is legitimately empty and
      // stderr says why. Anything else parsing badly is worth surfacing.
      if (result.code !== 2) {
        vscode.window.showErrorMessage(`Rux: could not read check output: ${e.message}`);
      }
      return;
    }

    diagnostics.set(
      document.uri,
      found
        .filter((d) => samePath(d.file, file))
        .map((d) => toDiagnostic(vscode, document, d))
    );
  }

  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument(refresh),
    vscode.workspace.onDidSaveTextDocument(refresh),
    vscode.workspace.onDidCloseTextDocument((document) => diagnostics.delete(document.uri)),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration('rux')) {
        vscode.workspace.textDocuments.forEach(refresh);
      }
    })
  );
  vscode.workspace.textDocuments.forEach(refresh);
}

/**
 * Turn one `rux check` diagnostic into a VS Code one.
 *
 * Rux counts lines and columns from 1 and VS Code from 0. A diagnostic with no
 * position (CSS and expression warnings carry only a file so far) is put on the
 * first line rather than dropped: an unplaced warning is still worth seeing.
 */
function toDiagnostic(vscode, document, d) {
  const line = Math.max(0, (d.line || 1) - 1);
  const column = Math.max(0, (d.column || 1) - 1);
  const range = d.line
    ? document.lineAt(Math.min(line, document.lineCount - 1)).range.with(
        new vscode.Position(Math.min(line, document.lineCount - 1), column)
      )
    : new vscode.Range(0, 0, 0, 0);
  const diagnostic = new vscode.Diagnostic(
    range,
    d.message,
    d.severity === 'error'
      ? vscode.DiagnosticSeverity.Error
      : vscode.DiagnosticSeverity.Warning
  );
  diagnostic.source = 'rux';
  return diagnostic;
}

/** Compare two paths without tripping over separators or drive-letter case. */
function samePath(a, b) {
  if (!a || !b) return false;
  const norm = (p) => path.resolve(p).replace(/\\/g, '/').toLowerCase();
  return norm(a) === norm(b);
}

/** Run in the document's own folder, so relative paths resolve as it expects. */
function workspaceDir(vscode, document) {
  const folder = vscode.workspace.getWorkspaceFolder(document.uri);
  if (folder) return folder.uri.fsPath;
  return document.uri.scheme === 'file' ? path.dirname(document.uri.fsPath) : undefined;
}

function deactivate() {}

// `indentArgs` is exported for the tests. What it returns is the difference
// between the editor agreeing with `rux fmt --check` and quietly reformatting
// every file it is asked to format, which is worth holding down.
module.exports = { activate, deactivate, indentArgs };
