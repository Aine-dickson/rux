// Rux VS Code extension: formatting and diagnostics, both by shelling out to
// the `rux` binary.
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
const path = require('path');

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

function activate(context) {
  const vscode = require('vscode');
  const noticeMissingBinary = makeMissingBinaryNotice(vscode);
  const diagnostics = vscode.languages.createDiagnosticCollection('rux');
  context.subscriptions.push(diagnostics);

  // ── Formatting ─────────────────────────────────────────────────────────────
  // The document is piped in rather than read from disk, because what needs
  // formatting is the buffer, which is usually unsaved.
  const formatter = {
    provideDocumentFormattingEdits(document, options) {
      const indent = options.insertSpaces ? String(options.tabSize) : 'tab';
      const result = runRux(vscode, ['fmt', '--indent', indent, '-'], document.getText(), workspaceDir(vscode, document));
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

module.exports = { activate, deactivate };
