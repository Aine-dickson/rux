// Does `activate()` actually run, and does it register everything?
//
// This is the test that was missing, and its absence cost six rounds of "the
// fix does not work" against code that was correct in isolation. Providers were
// verified by calling them directly; nothing checked that the editor ever
// reaches them. Two separate faults hid behind that gap:
//
//   1. `activationEvents` was absent from `package.json`, so VS Code never
//      called `activate()` at all when a `.rux` file was opened.
//   2. An exception part-way through `activate()` leaves everything registered
//      after the throw silently missing, with no error the user ever sees. The
//      symptom is "some features work and others do not", which reads as a bug
//      in the missing feature.
//
// So: run `activate` against a stand-in for the VS Code API and assert on what
// it registered. A mock is exactly right here — what is under test is the
// wiring, not the editor.

const test = require('node:test');
const assert = require('node:assert');
const path = require('path');

/**
 * A stand-in for the VS Code API surface `activate` touches.
 *
 * Deliberately complete rather than minimal: a missing method throws, and a
 * throw is indistinguishable from the bug this file exists to catch. Anything
 * added to `extension.js` that needs a new API will fail here first, which is
 * the point.
 */
function makeVscode(record) {
  const disposable = { dispose() {} };
  const event = () => disposable;
  return {
    workspace: {
      getConfiguration: () => ({ get: (key, fallback) => (key === 'path' ? 'rux' : fallback) }),
      onDidOpenTextDocument: event,
      onDidSaveTextDocument: event,
      onDidCloseTextDocument: event,
      onDidChangeTextDocument: event,
      onDidChangeConfiguration: event,
      textDocuments: [],
      getWorkspaceFolder: () => undefined,
      openTextDocument: async () => ({}),
    },
    window: {
      showInformationMessage: () => {},
      showWarningMessage: () => {},
      showErrorMessage: () => {},
      createTerminal: () => ({ show() {}, sendText() {}, dispose() {}, exitStatus: undefined }),
      showTextDocument: async () => ({}),
      activeTextEditor: undefined,
    },
    languages: {
      createDiagnosticCollection: () => ({ dispose() {}, set() {}, delete() {} }),
      registerCompletionItemProvider: () => (record.providers.push('completion'), disposable),
      registerHoverProvider: () => (record.providers.push('hover'), disposable),
      registerDefinitionProvider: () => (record.providers.push('definition'), disposable),
      registerDocumentSymbolProvider: () => (record.providers.push('symbols'), disposable),
      registerDocumentFormattingEditProvider: () => (record.providers.push('formatter'), disposable),
    },
    commands: {
      registerCommand: (id) => (record.commands.push(id), disposable),
    },
    CompletionItem: class {
      constructor(label, kind) {
        this.label = label;
        this.kind = kind;
      }
    },
    CompletionItemKind: new Proxy({}, { get: (_t, k) => String(k) }),
    SnippetString: class {
      constructor(value) {
        this.value = value;
      }
    },
    MarkdownString: class {
      constructor(value) {
        this.value = value;
      }
      appendMarkdown(v) {
        this.value = (this.value || '') + v;
        return this;
      }
    },
    Hover: class {},
    Range: class {},
    Position: class {},
    Location: class {},
    Diagnostic: class {},
    DiagnosticSeverity: { Error: 0, Warning: 1 },
    DocumentSymbol: class {},
    SymbolKind: new Proxy({}, { get: (_t, k) => String(k) }),
    TextEdit: { replace: () => ({}) },
    Uri: { file: (p) => ({ fsPath: p, scheme: 'file' }) },
  };
}

/** Run `activate` with the stand-in in place of the real `vscode` module. */
function activate() {
  const record = { commands: [], providers: [], threw: null };
  const vscode = makeVscode(record);

  const Module = require('module');
  const realResolve = Module._resolveFilename;
  Module._resolveFilename = function (request, ...rest) {
    if (request === 'vscode') return 'vscode';
    return realResolve.call(this, request, ...rest);
  };
  require.cache.vscode = { id: 'vscode', filename: 'vscode', loaded: true, exports: vscode };

  // A fresh copy, so one test's activation does not satisfy another's.
  for (const key of Object.keys(require.cache)) {
    if (key.includes(`${path.sep}editors${path.sep}vscode${path.sep}`)) delete require.cache[key];
  }

  const extension = require('../extension');
  const context = { subscriptions: [] };
  try {
    extension.activate(context);
  } catch (e) {
    record.threw = e;
  } finally {
    Module._resolveFilename = realResolve;
    delete require.cache.vscode;
  }
  record.subscriptions = context.subscriptions.length;
  return record;
}

test('activate() runs to completion', () => {
  const result = activate();
  assert.equal(
    result.threw && result.threw.stack,
    undefined,
    'activate() threw, so everything after the throw was never registered'
  );
});

test('every provider the extension advertises is registered', () => {
  const { providers } = activate();
  for (const name of ['completion', 'hover', 'definition', 'symbols', 'formatter']) {
    assert.ok(providers.includes(name), `the ${name} provider was never registered`);
  }
});

test('every contributed command is actually registered', () => {
  // A command in `package.json` with no `registerCommand` behind it shows up in
  // menus and fails when clicked. The user hit exactly this: "Run Rux File"
  // appeared in the context menu and "Check Rux File" did not.
  const fs = require('fs');
  const pkg = JSON.parse(
    fs.readFileSync(path.join(__dirname, '..', 'package.json'), 'utf8')
  );
  const contributed = pkg.contributes.commands.map((c) => c.command);
  const { commands } = activate();

  for (const id of contributed) {
    assert.ok(commands.includes(id), `${id} is contributed in package.json but never registered`);
  }
  for (const id of commands) {
    assert.ok(contributed.includes(id), `${id} is registered but not contributed, so nothing can invoke it`);
  }
});
