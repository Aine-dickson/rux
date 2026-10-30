//! `clipboard.writeText` and `clipboard.readText`: the system clipboard a
//! text field's Copy and Paste already use, for a script's copy button
//! (Tracer's AW-025).
//!
//! `writeText` is a plain call, as Cut's write is, so a `@tap` can make it.
//! On the web it starts the browser's write and does not wait for it, which
//! is what Cut does there too. `readText` is awaited: a browser asks before
//! it hands the clipboard over, and answers later.
//!
//! A clipboard holding no text reads as `""`, as a browser's does. One that
//! can't be reached, or a read the person refused, throws.

use rux_runtime::native::{install, Any, Call, Error, Module};

/// Install the two calls for every document this process loads.
pub(crate) fn install_clipboard() {
    let interface = rux_runtime::builtins::clipboard_interface();
    let mut calls = std::collections::HashMap::new();
    calls.insert(
        "writeText".to_string(),
        Call::sync(|args: Vec<Any>| {
            let text = match args.into_iter().next() {
                Some(Any::Str(s)) => s,
                _ => return Err(Error::new("argument", "`clipboard.writeText` takes the text to copy, a `string`")),
            };
            write(&text)?;
            Ok(Any::None)
        }),
    );
    calls.insert("readText".to_string(), Call::future(|_args: Vec<Any>| read()));
    let _ = install(Module { interface, calls });
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn write(text: &str) -> Result<(), Error> {
    arboard::Clipboard::new()
        .and_then(|mut cb| cb.set_text(text.to_string()))
        .map_err(|e| Error::new("clipboard", format!("could not copy to the clipboard: {e}")))
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
async fn read() -> Result<Any, Error> {
    let mut cb = arboard::Clipboard::new()
        .map_err(|e| Error::new("clipboard", format!("could not read the clipboard: {e}")))?;
    match cb.get_text() {
        Ok(text) => Ok(Any::Str(text)),
        // Empty, or holding an image: no text, which reads as "".
        Err(arboard::Error::ContentNotAvailable) => Ok(Any::Str(String::new())),
        Err(e) => Err(Error::new("clipboard", format!("could not read the clipboard: {e}"))),
    }
}

#[cfg(target_os = "android")]
fn write(text: &str) -> Result<(), Error> {
    if crate::android_clipboard_write(text) {
        Ok(())
    } else {
        Err(Error::new("clipboard", "could not copy to the clipboard"))
    }
}

/// Android reads an empty clipboard and an unreachable one the same, so
/// both are "".
#[cfg(target_os = "android")]
async fn read() -> Result<Any, Error> {
    Ok(Any::Str(crate::android_clipboard_read().unwrap_or_default()))
}

#[cfg(target_arch = "wasm32")]
fn write(text: &str) -> Result<(), Error> {
    let Some(clipboard) = crate::web_clipboard() else {
        return Err(Error::new("clipboard", "this page has no clipboard: it is not served over https"));
    };
    let _ = clipboard.write_text(text);
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn read() -> crate::files::Slot {
    use crate::files::Slot;
    let Some(clipboard) = crate::web_clipboard() else {
        return Slot::failed(Error::new("clipboard", "this page has no clipboard: it is not served over https"));
    };
    let slot = Slot::default();
    let answer = slot.clone();
    let promise = clipboard.read_text();
    wasm_bindgen_futures::spawn_local(async move {
        answer.set(match wasm_bindgen_futures::JsFuture::from(promise).await {
            Ok(text) => Ok(Any::Str(text.as_string().unwrap_or_default())),
            Err(_) => Err(Error::new("clipboard", "the browser did not hand over the clipboard: the person refused, or the page was not focused")),
        });
    });
    slot
}
