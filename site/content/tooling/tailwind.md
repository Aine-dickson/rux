+++
title = "Tailwind CSS"
description = "Use Tailwind's classes in a Rux app: the standalone CLI, putting it on PATH, rux.toml, and completions in VS Code."
weight = 10
+++

Tailwind reads the class names in your `.rux` files and writes a stylesheet
holding exactly those classes. Rux does not run Tailwind's code. It loads the
CSS file Tailwind writes, like any other stylesheet, and `rux.toml` can start
Tailwind for you so `rux run` stays the only command.

A project ends up with three Tailwind files:

| File | What it is |
|---|---|
| `assets/app.css` | Tailwind's **input**: one `@import` line. No document loads it |
| `assets/tailwind.css` | Tailwind's **output**, written by the CLI. Documents load this one |
| `.vscode/settings.json` | Points VS Code's Tailwind extension at the input, for completions |

The names are only examples. What matters is that `rux.toml`, the documents and
the editor settings all name the same two files.

## 1. Get the CLI

Tailwind's **standalone CLI** is one executable and needs no Node. Download it
from [github.com/tailwindlabs/tailwindcss/releases](https://github.com/tailwindlabs/tailwindcss/releases),
taking the file for your machine from the latest release:

| Machine | File |
|---|---|
| Windows | `tailwindcss-windows-x64.exe` |
| macOS, Apple silicon | `tailwindcss-macos-arm64` |
| macOS, Intel | `tailwindcss-macos-x64` |
| Linux | `tailwindcss-linux-x64` (or `-arm64`) |

Rename it to `tailwindcss` (`tailwindcss.exe` on Windows), so the commands
below work as written. Each release also lists a `sha256sums.txt`, if you want
to check the download.

If you already use Node, `npx @tailwindcss/cli` works in place of
`tailwindcss` in every command on this page, and you can skip the PATH step.

## 2. Put it on PATH

`rux` looks for `tailwindcss` on PATH, the same way your terminal does.

**Windows.** Put `tailwindcss.exe` in a folder of its own, such as `C:\bin`,
then add that folder to your user PATH. In PowerShell:

```powershell
$bin = "C:\bin"
$path = [Environment]::GetEnvironmentVariable("Path", "User")
[Environment]::SetEnvironmentVariable("Path", "$path;$bin", "User")
```

Or by hand: Start, search for **Edit environment variables for your account**,
select **Path**, **Edit**, **New**, and type the folder.

**macOS and Linux.** Put it in `~/.local/bin` (or any folder on PATH) and make
it executable:

```bash
mkdir -p ~/.local/bin
mv ~/Downloads/tailwindcss-macos-arm64 ~/.local/bin/tailwindcss
chmod +x ~/.local/bin/tailwindcss
```

If `~/.local/bin` is not on PATH yet, add `export PATH="$HOME/.local/bin:$PATH"`
to `~/.zshrc` or `~/.bashrc`. On macOS, a downloaded binary may be blocked the
first time it runs; `xattr -d com.apple.quarantine ~/.local/bin/tailwindcss`
clears that.

**Then open a new terminal**, because a terminal reads PATH when it starts.
That includes VS Code's terminals, and Git Bash. Check it:

```bash
tailwindcss --help
```

**Without PATH**, name the binary by its path in `rux.toml` instead, relative to
`rux.toml`, like `../tools/tailwindcss`. Keep it **outside the project folder**
either way: a release build embeds every file in the project, and the CLI is
about 30 MB.

## 3. Write the input file

`assets/app.css`:

```css
@import "tailwindcss";
```

That is the whole file. Tailwind scans every file under the folder it runs in
(the one holding `rux.toml`) for class names, `.rux` files included, and skips
whatever `.gitignore` lists, so there is no list of files to keep up to date.
For a `.rux` file that is ignored, or lives outside the project, name it:

```css
@import "tailwindcss";
@source "../shared/**/*.rux";
```

Your own `@theme` colours, fonts and spacing go in this file too, as in any
Tailwind v4 project.

## 4. Tell `rux.toml` to run it

```toml
[dev]
watch = ["tailwindcss -i assets/app.css -o assets/tailwind.css --watch"]

[build]
before = ["tailwindcss -i assets/app.css -o assets/tailwind.css --minify"]
```

`rux run` runs `before` once, so the output exists before the window opens,
then keeps `watch` running and stops it when the window closes. Save a `.rux`
file, Tailwind rewrites `assets/tailwind.css`, and the window reloads.
`rux build` runs `before` before it gathers the project. Both are described in
[`rux build`](@/tooling/build.md#other-tools-dev-and-build).

Tailwind's own output is printed to the same terminal as `rux`'s, so a line
like `Specified input file ... does not exist` means the `-i` path does not
match your input file.

## 5. Load the output in your documents

At the end of `app.rux`:

```html
<style src="assets/tailwind.css"></style>
```

The path is relative to the file that names it. A document's rules reach the
components it uses, as described in
[the reference](@/reference/_index.md), so one include in the entry document
covers them; a component with `<style scoped>` sees none of them and needs its
own. Your own rules can sit beside it in a second `<style>` block, and they win
a tie, since an include cascades first.

### `@apply` in a `<style>` block

A `<style>` block can use `@apply` and Tailwind's other directives, as a Vue
or Svelte component does under Vite. Start the block with `@reference`, naming
your input file relative to the `.rux` file, so the block knows your theme
without writing the whole of Tailwind into it again:

```html
<template>
  <view class="bar"><text>Title</text></view>
</template>

<style scoped>
  @reference "../assets/app.css";

  .bar {
    @apply rounded-lg bg-green-500 px-4 hover:bg-green-700;
  }
</style>
```

`rux run`, `rux check` and `rux build` hand any block that uses a directive to
the `tailwindcss` CLI, the same one on your PATH, and use the CSS it writes
back. A block with no directive is left alone, so a project that never writes
one never needs the CLI for it.

- **Each result is cached** in `.rux-build/tailwind/`, by the block's text
  and the file its `@reference` names. The first compile takes about half a
  second; after that the block costs nothing until you edit it or the input
  file. Deleting the folder starts over.
- **`rux build` carries the results**, so a release on another machine, a
  phone or the web never runs Tailwind. Hot reload on a phone
  (`rux run --device`) compiles on your machine and sends the result too.
- **Mistakes land on their line.** `@apply bg-gren-500` is reported on the
  line of the `<style>` it is written on, with Tailwind's own message. So is
  a block with a directive when the CLI is not on PATH.
- **No line numbers inside a compiled block.** A warning about a property Rux
  does not honor, raised from a compiled block, names the file but not the
  line, because the CSS Rux reads is the CLI's, not yours.
- **The [playground](@/playground.md) cannot run Tailwind.** There, a
  directive in a `<style>` block warns and does nothing; put the rule in
  Tailwind's input file instead, under the `@import`, where it needs no
  `@reference`.

Then write classes as you would anywhere:

```html
<template>
  <screen class="flex h-full items-center justify-center bg-slate-100">
    <view class="rounded-xl bg-white p-6 shadow-md">
      <text class="text-2xl font-bold text-indigo-600">Hello</text>
    </view>
  </screen>
</template>

<style src="assets/tailwind.css"></style>
```

`rux check` warns about any CSS Tailwind wrote that Rux does not honor yet. A
warning there is not an error: the app runs, without that declaration.

## 6. Completions in VS Code

Install the **Tailwind CSS IntelliSense** extension (`bradlc.vscode-tailwindcss`).
It does not know the `rux` language, and it needs to be told where the input
file is, since a v4 project has no `tailwind.config.js` for it to find. Create
`.vscode/settings.json` in the project:

```jsonc
{
  // Read .rux files as HTML, so class="..." gets completions.
  "tailwindCSS.includeLanguages": { "rux": "html" },

  // The input file: the one holding @import "tailwindcss".
  "tailwindCSS.experimental.configFile": "assets/app.css",

  // Show completions inside quoted attribute values.
  "[rux]": {
    "editor.quickSuggestions": { "strings": "on" }
  }
}
```

`configFile` is relative to the folder opened in VS Code, and must name the
**input**, not the output. Run **Developer: Reload Window** after changing it.
Typing `bg-` inside `class="..."` should then list colours; `Ctrl+Space` asks
for the list if it does not appear.

The extension uses its own copy of Tailwind when the project has no
`node_modules`, and says `Loading fallback stylesheet for: tailwindcss` in its
log when it does. That line is expected.

If there are still no completions, open the **Output** panel and pick
**Tailwind CSS IntelliSense**:

| The log says | Meaning |
|---|---|
| `Initialized 0 projects` | `configFile` names a file that is not there, or not the input |
| `No matching project for document` | the same, or VS Code was opened on a different folder than the project |
| `Initialized 1 projects` | it is set up; reload the window and try `Ctrl+Space` |

## When nothing changes

| Symptom | Look at |
|---|---|
| Unstyled window, no errors | the document has no `<style src>` naming the output |
| `reading stylesheet ...: cannot find the file` | the output does not exist yet: add `[build] before`, or run the command once |
| `does not exist` from Tailwind | the `-i` path in `rux.toml` |
| `... which is not on PATH` from `rux` | step 2, then a new terminal |
| A class has no effect | it is missing from the output (a class built from pieces at runtime cannot be found by a scanner), or `rux check` warns about the CSS it needs |
