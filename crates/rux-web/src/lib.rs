//! Rux in the browser.
//!
//! This crate is deliberately thin. It does not re-implement anything: the
//! canvas runs the *same* `rux-shell` the desktop window runs, so input, focus,
//! the caret, selection and scrolling behave identically by construction rather
//! than by careful synchronisation. A second renderer for the web would be a
//! second source of truth, and would drift.
//!
//! What this crate adds is the two things a browser needs and a desktop does
//! not: a font (there is no system font source to discover) and a JavaScript
//! surface for the host page to call.
//!
//! Build it with `wasm-bindgen`, which is `cargo install`ed and needs no node or
//! npm, the site's "no JS toolchain" rule survives intact:
//!
//! ```text
//! cargo build -p rux-web --target wasm32-unknown-unknown --release
//! wasm-bindgen --target web --no-typescript \
//!   --out-dir site/static/playground \
//!   target/wasm32-unknown-unknown/release/rux_web.wasm
//! ```

/// The UI font, embedded in the binary.
///
/// Inter, under the SIL Open Font License 1.1, see `assets/Inter-OFL.txt`,
/// which must ship wherever these bytes do. It is a variable font, so one file
/// covers every weight the examples ask for (400 through 700) instead of
/// needing a static face per weight.
pub const DEFAULT_FONT: &[u8] = include_bytes!("../assets/Inter-Variable.ttf");

/// The TextMate grammar, embedded from the file the site and the VS Code
/// extension both read. Including it by path rather than copying it is the whole
/// point: three consumers, one grammar, nothing to keep in sync.
pub const GRAMMAR: &str = include_str!("../../../site/syntaxes/rux.tmLanguage.json");

/// The document shown before the host page supplies anything of its own.
pub const PLACEHOLDER: &str = r#"<template>
  <screen class="app">
    <text class="title">Rux</text>
    <text class="sub">Running in a browser, on the GPU.</text>
  </screen>
</template>
<style>
  .app  { display: flex; flex-direction: column; gap: 8px; padding: 32px; background: #1e1e2e; }
  .title { color: #cdd6f4; font-size: 28px; font-weight: 700; }
  .sub   { color: #9399b2; font-size: 15px; }
</style>
"#;

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;

    use wasm_bindgen::prelude::*;

    thread_local! {
        /// Compiled once. Building the Oniguruma scanners is the expensive part,
        /// and the editor re-highlights on every keystroke.
        static GRAMMAR: RefCell<Option<rux_highlight::Grammar>> = const { RefCell::new(None) };
    }

    /// Highlight Rux source, returning HTML with one `<span class="hl-…">` per
    /// coloured run. The caller renders it underneath a transparent textarea.
    ///
    /// The output always reproduces the input text exactly, so the overlay stays
    /// aligned with the textarea character for character, that invariant is
    /// enforced by tests in `rux-highlight`.
    #[wasm_bindgen]
    pub fn highlight(source: &str) -> String {
        GRAMMAR.with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_none() {
                match rux_highlight::Grammar::from_json(super::GRAMMAR) {
                    Ok(g) => *slot = Some(g),
                    Err(e) => {
                        web_sys::console::error_1(&format!("rux: grammar: {e}").into());
                        return escape(source);
                    }
                }
            }
            rux_highlight::to_html(slot.as_mut().expect("just built"), source)
        })
    }

    /// Fallback when the grammar will not compile: uncoloured, but still correct
    /// and still escaped.
    fn escape(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
    }

    /// The runtime version this bundle was built from.
    ///
    /// Taken from the crate version rather than passed in by the page, so it
    /// cannot claim to be a release the wasm was not built from.
    #[wasm_bindgen]
    pub fn version() -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    /// Resize the canvas, in logical (CSS) pixels.
    ///
    /// The page calls this on load and whenever the pane changes size, which is
    /// what makes the playground usable on a phone: the canvas fits the column
    /// instead of overflowing it.
    #[wasm_bindgen]
    pub fn resize(width: f64, height: f64) {
        rux_shell::resize_web(width, height);
    }

    /// One indent level in the playground.
    const UNIT: &str = "  ";

    /// Re-indent a whole document, the Format button.
    ///
    /// Indentation only: nothing on a line is rewritten, so this can never eat
    /// what someone was in the middle of typing.
    #[wasm_bindgen]
    pub fn format(source: &str) -> String {
        rux_fmt::reindent(source, UNIT)
    }

    /// How many spaces a new line should start with, given the line it follows.
    /// Used for auto-indent on Enter, where re-formatting the whole document
    /// would throw the caret around.
    #[wasm_bindgen(js_name = indentAfter)]
    pub fn indent_after(line: &str) -> usize {
        let current = rux_fmt::indent_of(line, UNIT);
        rux_fmt::indent_after(line, current) * UNIT.len()
    }

    /// Boot Rux onto a canvas and start rendering `source`.
    ///
    /// Returns immediately, the event loop is handed to the browser, not
    /// blocked on. Call it once; use [`set_source`] afterwards.
    ///
    /// `base` is optional and off by default. Passing the path the app is
    /// served under (`"/"` at the root of a domain, `"/app/"` from a
    /// subdirectory) hands the URL bar to the router: the document opens on the
    /// route the URL names, so a shared link arrives on the page it points at,
    /// navigating adds a history entry, and the browser's Back and Forward walk
    /// the app.
    ///
    /// Leaving it out leaves the URL untouched. That is the right default and
    /// not merely a compatible one: the playground runs documents written by
    /// whoever is typing into it, and one of them containing a `<router>` must
    /// not rewrite the address of the page hosting it.
    ///
    /// ```js
    /// start(canvas, source, "/");    // the URL bar is this app's address
    /// start(canvas, source);         // the URL bar is not ours to touch
    /// ```
    #[wasm_bindgen]
    pub fn start(
        canvas: web_sys::HtmlCanvasElement,
        source: Option<String>,
        base: Option<String>,
    ) {
        set_panic_hook();
        let source = source.unwrap_or_else(|| super::PLACEHOLDER.to_string());
        rux_shell::start_web(canvas, source, super::DEFAULT_FONT.to_vec(), base);
    }

    /// Replace the running document with new source, the playground editor's
    /// equivalent of saving a file.
    ///
    /// Returns the parse error, if there is one, so the page can show it. On an
    /// error the last good document stays on screen rather than the canvas going
    /// blank, because while you are typing the source is invalid most of the
    /// time and a flickering canvas helps nobody.
    #[wasm_bindgen(js_name = setSource)]
    pub fn set_source(source: String) -> Option<String> {
        rux_shell::set_web_source(source)
    }

    /// Replace the running document and report everything wrong with it, as a
    /// JSON string:
    ///
    /// ```json
    /// {"error": {"message": "…", "line": 6, "column": 16}, "warnings":
    ///  [{"message": "…", "line": 11}]}
    /// ```
    ///
    /// `error` is null when the document loaded. `line` and `column` are null
    /// when that stage did not know a position, so a consumer never has to tell
    /// an absent field from an unplaced one.
    ///
    /// This is what [`set_source`] should always have been. That one returns a
    /// bare error message, so the playground could show no line to jump to and
    /// no warnings at all, while the desktop window had both. It stays because
    /// the deployed page runs against the runtime from the latest *tag*, which
    /// predates this.
    ///
    /// JSON as a string rather than a built object keeps `js-sys` out of the
    /// bundle for a payload this shape.
    #[wasm_bindgen]
    pub fn diagnose(source: String) -> String {
        rux_shell::diagnose_web_source(source)
    }

    /// Send panics to `console.error` instead of the default, which on wasm is
    /// an unadorned "unreachable executed" with no message or location. Written
    /// out rather than pulling in `console_error_panic_hook`, which is a
    /// dependency for six lines.
    fn set_panic_hook() {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            std::panic::set_hook(Box::new(|info| {
                web_sys::console::error_1(&format!("rux panicked: {info}").into());
            }));
        });
    }
}
