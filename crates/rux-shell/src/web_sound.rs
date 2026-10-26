//! `loadSound` and `Sound` in a browser (`docs/13-file-input-and-media-plan.md`,
//! B.1): the same calls as `crate::sound`'s, played by the browser's own
//! audio, an `HTMLAudioElement` per sound that is never put in the page, so
//! a sound plays in every format the browser plays.
//!
//! A `Sound` must be `Send`, and an element is not, so the resource is a
//! number and the element lives in [`SOUNDS`] on the page's one thread. The
//! element's own events (`timeupdate`, `play`, `pause`, `ended`, ...) call
//! [`rux_runtime::native::notify`], so `{{ s.currentTime }}` follows it as
//! the browser reports it, about four times a second.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use rux_runtime::native::{Any, Call, Error, Handle, Resource};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::files::Slot;

/// A loaded sound: the key of its element in [`SOUNDS`].
pub(crate) struct SoundRes(u32);

impl Resource for SoundRes {
    const NAME: &'static str = "Sound";
}

/// The last handle gone: the element stops and lets its bytes go.
impl Drop for SoundRes {
    fn drop(&mut self) {
        forget(self.0, true);
    }
}

/// Take a sound's element out of the page's keeping: its listeners first,
/// since the browser may still queue an event for a closure about to go.
fn forget(id: u32, stop: bool) {
    let Some(s) = SOUNDS.with(|m| m.borrow_mut().remove(&id)) else { return };
    for (event, f) in &s.listeners {
        let _ = s.audio.remove_event_listener_with_callback(event, f.as_ref().unchecked_ref());
    }
    if stop {
        let _ = s.audio.pause();
        s.audio.remove_attribute("src").ok();
        s.audio.load();
    }
    if let Some(url) = s.own_url {
        let _ = web_sys::Url::revoke_object_url(&url);
    }
}

struct WebSound {
    audio: web_sys::HtmlAudioElement,
    /// The object URL made for a project file's bytes, revoked with it; a
    /// picked file's URL is the file's own and outlives the sound.
    own_url: Option<String>,
    /// Kept as long as the element, which calls them.
    listeners: Vec<(&'static str, Closure<dyn FnMut()>)>,
}

thread_local! {
    static SOUNDS: RefCell<HashMap<u32, WebSound>> = RefCell::new(HashMap::new());
}

static NEXT: AtomicU32 = AtomicU32::new(1);

/// The events after which a property may read differently.
const MOVES: &[&str] = &["timeupdate", "play", "pause", "ended", "seeked", "durationchange", "volumechange", "ratechange"];

/// Run `f` on the element of the sound a call's first argument is.
fn with<T>(args: &[Any], f: impl FnOnce(&web_sys::HtmlAudioElement) -> T) -> Result<T, Error> {
    let id = match args.first() {
        Some(Any::Resource(h)) => h.get::<SoundRes>().map(|s| s.0),
        _ => None,
    }
    .ok_or_else(|| Error::new("type", "this is not a `Sound`"))?;
    SOUNDS.with(|m| m.borrow().get(&id).map(|s| f(&s.audio))).ok_or_else(|| Error::new("type", "this `Sound` is gone"))
}

fn number(v: Option<&Any>) -> Result<f64, Error> {
    match v {
        Some(Any::Float(f)) => Ok(*f),
        Some(Any::Int(i)) => Ok(*i as f64),
        _ => Err(Error::new("type", "a number of seconds")),
    }
}

fn flag(v: Option<&Any>) -> Result<bool, Error> {
    match v {
        Some(Any::Bool(b)) => Ok(*b),
        _ => Err(Error::new("type", "`true` or `false`")),
    }
}

/// What a project file's extension says it holds; a browser sniffs a blob
/// with no type, but is told when it can be.
fn mime(src: &str) -> &'static str {
    let ext = std::path::Path::new(src.split(['#', '?']).next().unwrap_or(src))
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase());
    match ext.as_deref() {
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("m4a" | "mp4" | "aac") => "audio/mp4",
        Some("flac") => "audio/flac",
        Some("ogg" | "oga" | "opus") => "audio/ogg",
        Some("webm") => "audio/webm",
        _ => "",
    }
}

/// The URL the element plays `src` by: a picked file's `blob:` URL as it
/// is; a project path's bytes, read as an `<image src>`'s are (a release
/// build embeds them), as a blob of their own; and a path the source does
/// not hold as the page's own relative URL, which the browser fetches.
/// Any other URL waits for a networking story, as on the desktop.
fn url_for(src: &str) -> Result<(String, bool), Error> {
    if src.starts_with("blob:") {
        return Ok((src.to_string(), false));
    }
    let bytes = match rux_runtime::read_media(src) {
        Ok(b) => b,
        Err(_) if !src.contains(':') => return Ok((src.to_string(), false)),
        Err(e) => return Err(Error::new("io", format!("cannot read {src}: {e}"))),
    };
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes.as_slice()));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(mime(src));
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)
        .map_err(|_| Error::new("io", format!("cannot hold {src}")))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(|_| Error::new("io", format!("cannot hold {src}")))?;
    Ok((url, true))
}

/// Load `src`: answered once the browser knows its length, or that it
/// cannot play it.
fn load(src: Any) -> Slot {
    let Any::Str(src) = src else {
        return Slot::failed(Error::new("type", "`loadSound` takes the sound's path or URL"));
    };
    let (url, own) = match url_for(&src) {
        Ok(u) => u,
        Err(e) => return Slot::failed(e),
    };
    let Ok(audio) = web_sys::HtmlAudioElement::new() else {
        return Slot::failed(Error::new("unsupported", "this browser has no audio"));
    };
    audio.set_preload("auto");
    let slot = Slot::default();
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut listeners = Vec::new();
    let mut listen = |event: &'static str, f: Closure<dyn FnMut()>| {
        let _ = audio.add_event_listener_with_callback(event, f.as_ref().unchecked_ref());
        listeners.push((event, f));
    };

    let answer = slot.clone();
    let answered = std::rc::Rc::new(std::cell::Cell::new(false));
    let once = answered.clone();
    listen(
        "loadedmetadata",
        Closure::new(move || {
            if !once.replace(true) {
                answer.set(Ok(Any::Resource(Handle::new(SoundRes(id)))));
            }
        }),
    );
    let answer = slot.clone();
    let failed = audio.clone();
    let name = src.clone();
    listen(
        "error",
        Closure::new(move || {
            if answered.replace(true) {
                return;
            }
            let code = failed.error().map_or(0, |e| e.code());
            let error = match code {
                web_sys::MediaError::MEDIA_ERR_SRC_NOT_SUPPORTED => {
                    Error::new("unsupported", format!("{name} is not a sound this browser can play"))
                }
                web_sys::MediaError::MEDIA_ERR_DECODE => Error::new("decode", format!("{name} is damaged")),
                _ => Error::new("io", format!("cannot read {name}")),
            };
            answer.set(Err(error));
            // No handle was made, so none will drop it. Not from inside this
            // listener, which would drop the closure running: just after.
            wasm_bindgen_futures::spawn_local(async move { forget(id, false) });
        }),
    );
    for event in MOVES {
        listen(event, Closure::new(rux_runtime::native::notify));
    }
    audio.set_src(&url);
    SOUNDS.with(|m| m.borrow_mut().insert(id, WebSound { audio, own_url: own.then_some(url), listeners }));
    slot
}

/// The calls, keyed as `crate::sound`'s are.
pub(crate) fn calls() -> Vec<(String, Call)> {
    let act = |f: fn(&web_sys::HtmlAudioElement)| {
        Call::sync(move |args| {
            with(&args, f)?;
            rux_runtime::native::notify();
            Ok(Any::None)
        })
    };
    let get = |f: fn(&web_sys::HtmlAudioElement) -> Any| Call::sync(move |args| with(&args, f));
    let set = |f: fn(&web_sys::HtmlAudioElement, Option<&Any>) -> Result<(), Error>| {
        Call::sync(move |args| {
            with(&args, |a| f(a, args.get(1)))??;
            rux_runtime::native::notify();
            Ok(Any::None)
        })
    };
    vec![
        ("loadSound".into(), Call::future(|mut args: Vec<Any>| load(if args.is_empty() { Any::None } else { args.swap_remove(0) }))),
        (
            "Sound.play".into(),
            act(|a| {
                if a.ended() {
                    a.set_current_time(0.0);
                }
                // Refused outside a tap's reach (the browser's autoplay
                // rule); said on the console, as the promise is not awaited.
                if let Ok(p) = a.play() {
                    let refused = Closure::once(|e: wasm_bindgen::JsValue| {
                        web_sys::console::warn_2(&"rux: the browser would not play this sound yet:".into(), &e);
                    });
                    let _ = p.catch(&refused);
                    refused.forget();
                }
            }),
        ),
        ("Sound.pause".into(), act(|a| drop(a.pause()))),
        (
            "Sound.stop".into(),
            act(|a| {
                let _ = a.pause();
                a.set_current_time(0.0);
            }),
        ),
        ("Sound.seek".into(), set(|a, v| Ok(a.set_current_time(number(v)?.max(0.0))))),
        ("Sound.volume".into(), get(|a| Any::Float(a.volume()))),
        ("Sound.loop".into(), get(|a| Any::Bool(a.loop_()))),
        ("Sound.currentTime".into(), get(|a| Any::Float(a.current_time()))),
        (
            "Sound.duration".into(),
            get(|a| {
                let d = a.duration();
                if d.is_finite() { Any::Float(d) } else { Any::None }
            }),
        ),
        ("Sound.playing".into(), get(|a| Any::Bool(!a.paused() && !a.ended()))),
        (
            "Sound.volume=".into(),
            set(|a, v| {
                // A browser throws past 0 to 1; the desktop clamps, and so here.
                let v = number(v)?;
                let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 };
                a.set_volume(v);
                // Nothing at all is muted, which is what a browser lets play
                // before a tap: an `<audio muted autoplay>` is this.
                a.set_muted(v == 0.0);
                Ok(())
            }),
        ),
        ("Sound.loop=".into(), set(|a, v| Ok(a.set_loop(flag(v)?)))),
        ("Sound.currentTime=".into(), set(|a, v| Ok(a.set_current_time(number(v)?.max(0.0))))),
    ]
}
