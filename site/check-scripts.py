#!/usr/bin/env python3
"""Syntax-check every inline <script> in the built site.

This exists because a literal newline inside a JS string literal shipped to
production and took the whole playground down. An ES module that fails to parse
runs *nothing*: no init, no highlight, no start. The page still rendered, the
editor still took keystrokes and still showed a caret, so it looked alive while
doing nothing at all. It survived a Zola build, a deploy, and a review, because
none of those has any opinion about JavaScript.

Run after `zola build`, from the `site/` directory:

    python check-scripts.py

Node is used only as a syntax checker here. It is not a build dependency: the
site is still produced by Zola alone, and this script skips itself with a clear
message if node is absent rather than failing the build.
"""

import glob
import os
import re
import shutil
import subprocess
import sys
import tempfile

PUBLIC = 'public'
# Inline modules use bare specifiers resolved by the browser, which node cannot
# follow. The import lines are stripped; everything after them is what matters.
IMPORT = re.compile(r'^\s*import\s.*?;\s*$', re.M | re.S)
SCRIPT = re.compile(r'<script(?![^>]*\ssrc=)([^>]*)>(.*?)</script>', re.S | re.I)


def main() -> int:
    node = shutil.which('node')
    if not node:
        print('check-scripts: node not found, skipping (this is a linter, not a build step)')
        return 0
    if not os.path.isdir(PUBLIC):
        print('check-scripts: no public/ directory; run `zola build` first', file=sys.stderr)
        return 1

    pages = sorted(glob.glob(os.path.join(PUBLIC, '**', '*.html'), recursive=True))
    checked = failed = 0

    for page in pages:
        with open(page, encoding='utf-8') as fh:
            html = fh.read()
        for n, (attrs, body) in enumerate(SCRIPT.findall(html)):
            if not body.strip():
                continue
            source = IMPORT.sub('', body)
            suffix = '.mjs' if 'module' in attrs else '.js'
            with tempfile.NamedTemporaryFile(
                'w', suffix=suffix, delete=False, encoding='utf-8'
            ) as tmp:
                tmp.write(source)
                path = tmp.name
            try:
                result = subprocess.run(
                    [node, '--check', path], capture_output=True, text=True
                )
            finally:
                os.unlink(path)
            checked += 1
            if result.returncode != 0:
                failed += 1
                rel = os.path.relpath(page, PUBLIC)
                print('check-scripts: FAILED in %s (script %d)' % (rel, n + 1), file=sys.stderr)
                print(result.stderr.strip(), file=sys.stderr)

    if failed:
        print('\ncheck-scripts: %d of %d inline scripts do not parse' % (failed, checked),
              file=sys.stderr)
        return 1

    print('check-scripts: %d inline scripts parse' % checked)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
