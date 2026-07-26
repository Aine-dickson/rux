//! The embedded font, verified on the host.
//!
//! On the web this font is the *only* one there is — a browser exposes no system
//! font source, so if these bytes don't parse, every string measures to zero and
//! the canvas renders blank boxes with no error anywhere. That failure is
//! miserable to diagnose through a wasm bundle, so it is caught here instead.

use rux_text::{TextEngine, TextStyle, Wrap};

#[test]
fn embedded_font_registers_and_shapes_text() {
    let mut engine = TextEngine::new();
    assert!(
        engine.register_font(rux_web::DEFAULT_FONT.to_vec()),
        "the vendored font produced no usable faces"
    );

    // Measured through the generic family, which is what an unstyled <text>
    // resolves to — the path that silently breaks when nothing is registered.
    let style = TextStyle { family: Some("sans-serif"), ..TextStyle::new(16.0, 400, Wrap::Normal) };
    let (w, h) = engine.measure("Hello, Rux", &style, None);
    assert!(w > 0.0 && h > 0.0, "text measured to nothing: {w}x{h}");

    // A longer string must be wider, or we are measuring a constant rather than
    // actually shaping glyphs.
    let (wide, _) = engine.measure("Hello, Rux — a longer line", &style, None);
    assert!(wide > w, "shaping is not responding to content: {wide} vs {w}");
}

/// The examples lean on `font-weight: 700`. Inter is a variable font, so bold
/// has to come from the weight axis rather than a second file — if that stopped
/// working, headings would silently render at regular weight.
#[test]
fn bold_differs_from_regular() {
    let mut engine = TextEngine::new();
    assert!(engine.register_font(rux_web::DEFAULT_FONT.to_vec()));

    let regular = TextStyle { family: Some("sans-serif"), ..TextStyle::new(32.0, 400, Wrap::Normal) };
    let bold = TextStyle { family: Some("sans-serif"), ..TextStyle::new(32.0, 700, Wrap::Normal) };

    let (rw, _) = engine.measure("Tasks", &regular, None);
    let (bw, _) = engine.measure("Tasks", &bold, None);
    assert!(bw > rw, "bold ({bw}) should be wider than regular ({rw})");
}

/// The placeholder document is what a reader sees before touching anything, so
/// it must at least parse.
#[test]
fn placeholder_document_is_valid() {
    rux_runtime::Document::from_source(rux_web::PLACEHOLDER).expect("placeholder parses");
}
