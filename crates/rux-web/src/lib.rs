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
//! npm — the site's "no JS toolchain" rule survives intact:
//!
//! ```text
//! cargo build -p rux-web --target wasm32-unknown-unknown --release
//! wasm-bindgen --target web --no-typescript \
//!   --out-dir site/static/playground \
//!   target/wasm32-unknown-unknown/release/rux_web.wasm
//! ```

/// The UI font, embedded in the binary.
///
/// Inter, under the SIL Open Font License 1.1 — see `assets/Inter-OFL.txt`,
/// which must ship wherever these bytes do. It is a variable font, so one file
/// covers every weight the examples ask for (400 through 700) instead of
/// needing a static face per weight.
pub const DEFAULT_FONT: &[u8] = include_bytes!("../assets/Inter-Variable.ttf");

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
    use wasm_bindgen::prelude::*;

    /// Boot Rux onto a canvas and start rendering `source`.
    ///
    /// Returns immediately — the event loop is handed to the browser, not
    /// blocked on. Call it once; use [`set_source`] afterwards.
    #[wasm_bindgen]
    pub fn start(canvas: web_sys::HtmlCanvasElement, source: Option<String>) {
        set_panic_hook();
        let source = source.unwrap_or_else(|| super::PLACEHOLDER.to_string());
        rux_shell::start_web(canvas, source, super::DEFAULT_FONT.to_vec());
    }

    /// Replace the running document with new source — the playground editor's
    /// equivalent of saving a file. A parse error leaves the last good document
    /// on screen and reports to the console, because in an editor the source is
    /// invalid most of the time you are typing.
    #[wasm_bindgen(js_name = setSource)]
    pub fn set_source(source: String) {
        rux_shell::set_web_source(source);
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
