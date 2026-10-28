//! `loadSound` and `Sound` in a browser (`docs/13-file-input-and-media-plan.md`,
//! B.1): the same calls as `crate::sound`'s, played by the browser's own
//! audio, an `HTMLAudioElement` per sound that is never put in the page, so
//! a sound plays in every format the browser plays.
//!
//! `<video>`'s `Video` (C.1, the web) is the same with an `HTMLVideoElement`,
//! also never put in the page: the browser decodes it and plays its sound,
//! and before each frame the shell copies its current picture into the
//! video's texture with `copyExternalImageToTexture` ([`upload`]), the GPU
//! to the GPU, which the painter draws as the desktop's are.
//!
//! A `Sound` must be `Send`, and an element is not, so the resource is a
//! number and the element lives in [`SOUNDS`] on the page's one thread. The
//! element's own events (`timeupdate`, `play`, `pause`, `ended`, ...) call
//! [`rux_runtime::native::notify`], so `{{ s.currentTime }}` follows it as
//! the browser reports it, about four times a second.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
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

/// A loaded video: the key of its element in [`SOUNDS`], as a sound's.
pub(crate) struct VideoRes(u32);

impl Resource for VideoRes {
    const NAME: &'static str = "Video";
}

impl Drop for VideoRes {
    fn drop(&mut self) {
        forget(self.0, true);
    }
}

/// Take a sound's element out of the page's keeping: its listeners first,
/// since the browser may still queue an event for a closure about to go.
fn forget(id: u32, stop: bool) {
    let Some(s) = SOUNDS.with(|m| m.borrow_mut().remove(&id)) else { return };
    for (event, f) in &s.listeners {
        let _ = s.media.remove_event_listener_with_callback(event, f.as_ref().unchecked_ref());
    }
    if stop {
        let _ = s.media.pause();
        s.media.remove_attribute("src").ok();
        s.media.load();
    }
    if let Some(url) = s.own_url {
        let _ = web_sys::Url::revoke_object_url(&url);
    }
}

struct WebSound {
    media: web_sys::HtmlMediaElement,
    /// The same element as a video's, for its size and its picture.
    video: Option<web_sys::HtmlVideoElement>,
    /// The object URL made for a project file's bytes, revoked with it; a
    /// picked file's URL is the file's own and outlives the sound.
    own_url: Option<String>,
    /// Kept as long as the element, which calls them.
    listeners: Vec<(&'static str, Closure<dyn FnMut()>)>,
    /// Played or sought at least once: the poster gives way to the picture.
    started: Rc<Cell<bool>>,
    /// Its picture moved without playing (loaded, sought), so it is copied
    /// in once more.
    moved: Rc<Cell<bool>>,
    /// When it was last stirred (`Date.now()`): its controls stay up a while.
    stirred: Cell<f64>,
    fullscreen: Cell<bool>,
    /// The painter drew it in the last frame.
    shown: Cell<bool>,
}

impl WebSound {
    fn playing(&self) -> bool {
        !self.media.paused() && !self.media.ended()
    }
}

thread_local! {
    static SOUNDS: RefCell<HashMap<u32, WebSound>> = RefCell::new(HashMap::new());
}

static NEXT: AtomicU32 = AtomicU32::new(1);

/// The events after which a property may read differently.
const MOVES: &[&str] = &["timeupdate", "play", "pause", "ended", "seeked", "durationchange", "volumechange", "ratechange"];

/// How long a playing video's controls stay up after it was last stirred,
/// in milliseconds, as on the desktop.
const CONTROLS_IDLE: f64 = 3000.0;

/// The element store's key of the sound or video a call's first argument is.
fn id_of(args: &[Any]) -> Option<u32> {
    match args.first() {
        Some(Any::Resource(h)) => h.get::<SoundRes>().map(|s| s.0).or_else(|| h.get::<VideoRes>().map(|v| v.0)),
        _ => None,
    }
}

/// Run `f` on the sound or video a call's first argument is.
fn with<T>(args: &[Any], f: impl FnOnce(&WebSound) -> T) -> Result<T, Error> {
    let id = id_of(args).ok_or_else(|| Error::new("type", "this is not a `Sound` or a `Video`"))?;
    SOUNDS.with(|m| m.borrow().get(&id).map(f)).ok_or_else(|| Error::new("type", "it is gone"))
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
fn mime(src: &str, video: bool) -> &'static str {
    let ext = std::path::Path::new(src.split(['#', '?']).next().unwrap_or(src))
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase());
    match (ext.as_deref(), video) {
        (Some("mp4" | "m4v"), true) => "video/mp4",
        (Some("webm"), true) => "video/webm",
        (Some("mov"), true) => "video/quicktime",
        (Some("ogv"), true) => "video/ogg",
        (_, true) => "",
        (Some("mp3"), _) => "audio/mpeg",
        (Some("wav"), _) => "audio/wav",
        (Some("m4a" | "mp4" | "aac"), _) => "audio/mp4",
        (Some("flac"), _) => "audio/flac",
        (Some("ogg" | "oga" | "opus"), _) => "audio/ogg",
        (Some("webm"), _) => "audio/webm",
        _ => "",
    }
}

/// The URL the element plays `src` by: a picked file's `blob:` URL as it
/// is; a project path's bytes, read as an `<image src>`'s are (a release
/// build embeds them), as a blob of their own; and a path the source does
/// not hold as the page's own relative URL, which the browser fetches.
/// Any other URL waits for a networking story, as on the desktop.
fn url_for(src: &str, video: bool) -> Result<(String, bool), Error> {
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
    options.set_type(mime(src, video));
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)
        .map_err(|_| Error::new("io", format!("cannot hold {src}")))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(|_| Error::new("io", format!("cannot hold {src}")))?;
    Ok((url, true))
}

/// A new element to play a video by: never put in the page, and inline, so
/// a phone's browser does not take it fullscreen when it plays.
fn video_element() -> Option<web_sys::HtmlVideoElement> {
    let v = web_sys::window()?.document()?.create_element("video").ok()?.dyn_into::<web_sys::HtmlVideoElement>().ok()?;
    let _ = v.set_attribute("playsinline", "");
    Some(v)
}

/// Load `src`: answered once the browser knows its length, or that it
/// cannot play it.
fn load(src: Any, video: bool) -> Slot {
    let what = if video { "video" } else { "sound" };
    let Any::Str(src) = src else {
        let call = if video { "loadVideo" } else { "loadSound" };
        return Slot::failed(Error::new("type", format!("`{call}` takes the {what}'s path or URL")));
    };
    let (url, own) = match url_for(&src, video) {
        Ok(u) => u,
        Err(e) => return Slot::failed(e),
    };
    let (media, picture) = if video {
        match video_element() {
            Some(v) => (v.clone().unchecked_into::<web_sys::HtmlMediaElement>(), Some(v)),
            None => return Slot::failed(Error::new("unsupported", "this browser has no video")),
        }
    } else {
        match web_sys::HtmlAudioElement::new() {
            Ok(a) => (a.unchecked_into::<web_sys::HtmlMediaElement>(), None),
            Err(_) => return Slot::failed(Error::new("unsupported", "this browser has no audio")),
        }
    };
    media.set_preload("auto");
    let slot = Slot::default();
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut listeners = Vec::new();
    let mut listen = |event: &'static str, f: Closure<dyn FnMut()>| {
        let _ = media.add_event_listener_with_callback(event, f.as_ref().unchecked_ref());
        listeners.push((event, f));
    };

    let answer = slot.clone();
    let answered = Rc::new(Cell::new(false));
    let once = answered.clone();
    listen(
        "loadedmetadata",
        Closure::new(move || {
            if !once.replace(true) {
                let h = if video { Handle::new(VideoRes(id)) } else { Handle::new(SoundRes(id)) };
                answer.set(Ok(Any::Resource(h)));
            }
        }),
    );
    let answer = slot.clone();
    let failed = media.clone();
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
                    Error::new("unsupported", format!("{name} is not a {what} this browser can play"))
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
    let started = Rc::new(Cell::new(false));
    let moved = Rc::new(Cell::new(true));
    if video {
        // Played or sought: the poster goes. A first picture or a seek is a
        // picture to copy in, playing or not.
        for event in ["play", "seeked"] {
            let (started, moved) = (started.clone(), moved.clone());
            listen(
                event,
                Closure::new(move || {
                    started.set(true);
                    moved.set(true);
                }),
            );
        }
        let again = moved.clone();
        listen(
            "loadeddata",
            Closure::new(move || {
                again.set(true);
                rux_runtime::native::notify();
            }),
        );
    }
    media.set_src(&url);
    SOUNDS.with(|m| {
        m.borrow_mut().insert(
            id,
            WebSound {
                media,
                video: picture,
                own_url: own.then_some(url),
                listeners,
                started,
                moved,
                stirred: Cell::new(js_sys::Date::now()),
                fullscreen: Cell::new(false),
                shown: Cell::new(true),
            },
        )
    });
    slot
}

/// The calls every sound and video has, keyed as `crate::sound`'s are with
/// `prefix` (`Sound` or `Video`).
fn shared(prefix: &str) -> Vec<(String, Call)> {
    let act = |f: fn(&WebSound)| {
        Call::sync(move |args| {
            with(&args, f)?;
            rux_runtime::native::notify();
            Ok(Any::None)
        })
    };
    let get = |f: fn(&WebSound) -> Any| Call::sync(move |args| with(&args, f));
    let set = |f: fn(&WebSound, Option<&Any>) -> Result<(), Error>| {
        Call::sync(move |args| {
            with(&args, |s| f(s, args.get(1)))??;
            rux_runtime::native::notify();
            Ok(Any::None)
        })
    };
    let calls: Vec<(&str, Call)> = vec![
        (
            "play",
            act(|s| {
                let a = &s.media;
                if a.ended() {
                    a.set_current_time(0.0);
                }
                s.stirred.set(js_sys::Date::now());
                // Refused outside a tap's reach (the browser's autoplay
                // rule); said on the console, as the promise is not awaited.
                if let Ok(p) = a.play() {
                    let refused = Closure::once(|e: wasm_bindgen::JsValue| {
                        web_sys::console::warn_2(&"rux: the browser would not play this yet:".into(), &e);
                    });
                    let _ = p.catch(&refused);
                    refused.forget();
                }
            }),
        ),
        ("pause", act(|s| drop(s.media.pause()))),
        (
            "stop",
            act(|s| {
                let _ = s.media.pause();
                s.media.set_current_time(0.0);
            }),
        ),
        ("seek", set(|s, v| Ok(s.media.set_current_time(number(v)?.max(0.0))))),
        ("volume", get(|s| Any::Float(s.media.volume()))),
        ("loop", get(|s| Any::Bool(s.media.loop_()))),
        ("currentTime", get(|s| Any::Float(s.media.current_time()))),
        (
            "duration",
            get(|s| {
                let d = s.media.duration();
                if d.is_finite() { Any::Float(d) } else { Any::None }
            }),
        ),
        ("playing", get(|s| Any::Bool(s.playing()))),
        (
            "volume=",
            set(|s, v| {
                // A browser throws past 0 to 1; the desktop clamps, and so here.
                let v = number(v)?;
                let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 };
                s.media.set_volume(v);
                // Nothing at all is muted, which is what a browser lets play
                // before a tap: an `<audio muted autoplay>` is this.
                s.media.set_muted(v == 0.0);
                Ok(())
            }),
        ),
        ("loop=", set(|s, v| Ok(s.media.set_loop(flag(v)?)))),
        ("currentTime=", set(|s, v| Ok(s.media.set_current_time(number(v)?.max(0.0))))),
    ];
    calls.into_iter().map(|(name, call)| (format!("{prefix}.{name}"), call)).collect()
}

/// The calls, keyed as `crate::sound`'s and `crate::video`'s are.
pub(crate) fn calls() -> Vec<(String, Call)> {
    let get = |f: fn(&WebSound) -> Any| Call::sync(move |args| with(&args, f));
    let size = |f: fn(&web_sys::HtmlVideoElement) -> u32| {
        Call::sync(move |args| with(&args, |s| Any::Int(s.video.as_ref().map_or(0, |v| i64::from(f(v))))))
    };
    let mut all = vec![
        ("loadSound".into(), Call::future(|mut args: Vec<Any>| load(if args.is_empty() { Any::None } else { args.swap_remove(0) }, false))),
        ("loadVideo".into(), Call::future(|mut args: Vec<Any>| load(if args.is_empty() { Any::None } else { args.swap_remove(0) }, true))),
        ("Video.videoWidth".into(), size(web_sys::HtmlVideoElement::video_width)),
        ("Video.videoHeight".into(), size(web_sys::HtmlVideoElement::video_height)),
        ("Video.started".into(), get(|s| Any::Bool(s.started.get()))),
        // The browser's own failures part way come as the element's `error`
        // event, which only a load listens for: none to report here.
        ("Video.error".into(), get(|_| Any::None)),
        ("Video.soundError".into(), get(|_| Any::None)),
        (
            "Video.controlsShown".into(),
            get(|s| Any::Bool(!s.playing() || js_sys::Date::now() - s.stirred.get() < CONTROLS_IDLE)),
        ),
        ("Video.fullscreen".into(), get(|s| Any::Bool(s.fullscreen.get()))),
        (
            "Video.stir".into(),
            Call::sync(|args| {
                let hidden = with(&args, |s| {
                    let hidden = s.playing() && js_sys::Date::now() - s.stirred.get() >= CONTROLS_IDLE;
                    s.stirred.set(js_sys::Date::now());
                    hidden
                })?;
                if hidden {
                    rux_runtime::native::notify();
                }
                Ok(Any::None)
            }),
        ),
        (
            "Video.fullscreen=".into(),
            Call::sync(|args| {
                let on = flag(args.get(1))?;
                with(&args, |s| {
                    s.fullscreen.set(on);
                    s.stirred.set(js_sys::Date::now());
                })?;
                rux_runtime::native::notify();
                Ok(Any::None)
            }),
        ),
    ];
    all.extend(shared("Sound"));
    all.extend(shared("Video"));
    all
}

/// A video's picture on the GPU: its texture, and the image vello draws it
/// by, as the desktop's.
struct OnGpu {
    texture: vello::wgpu::Texture,
    image: vello::peniko::ImageData,
}

thread_local! {
    static ON_GPU: RefCell<HashMap<String, OnGpu>> = RefCell::new(HashMap::new());
}

/// Before a frame is drawn: copy each video's current picture into its
/// texture, and tell the painter what to draw for it. `handles` is every
/// `<video>` on the page with its `Video`, by key; one no longer there lets
/// its texture go. A paused video is copied only when its picture moved;
/// one out of sight not at all, once it has a picture.
pub(crate) fn upload(
    handles: &[(String, Handle)],
    device: &vello::wgpu::Device,
    queue: &vello::wgpu::Queue,
    renderer: &mut vello::Renderer,
    images: &mut rux_paint::ImageCache,
) {
    use vello::wgpu;
    ON_GPU.with(|on| {
        let mut on = on.borrow_mut();
        on.retain(|key, gpu| {
            let keep = handles.iter().any(|(k, _)| k == key);
            if !keep {
                renderer.unregister_texture(gpu.image.clone());
                images.set_frame(&format!("{}{key}", rux_layout::VIDEO_FRAME), None);
            }
            keep
        });
        SOUNDS.with(|m| {
            let m = m.borrow();
            for (key, h) in handles {
                let Some(s) = h.get::<VideoRes>().and_then(|v| m.get(&v.0)) else { continue };
                let Some(v) = &s.video else { continue };
                // HAVE_CURRENT_DATA: there is a picture to copy.
                if v.ready_state() < 2 || v.video_width() == 0 || v.video_height() == 0 {
                    continue;
                }
                let have = on.contains_key(key);
                if have && (!s.shown.get() || !(s.playing() || s.moved.get())) {
                    continue;
                }
                s.moved.set(false);
                let size = wgpu::Extent3d { width: v.video_width(), height: v.video_height(), depth_or_array_layers: 1 };
                if on.get(key).is_some_and(|g| g.texture.size() != size) {
                    let gone = on.remove(key).expect("there");
                    renderer.unregister_texture(gone.image);
                }
                let gpu = on.entry(key.clone()).or_insert_with(|| {
                    let texture = device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("rux: video"),
                        size,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        // What the browser's copy in needs, and vello's out.
                        usage: wgpu::TextureUsages::COPY_SRC
                            | wgpu::TextureUsages::COPY_DST
                            | wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    });
                    let image = renderer.register_texture(texture.clone());
                    OnGpu { texture, image }
                });
                queue.copy_external_image_to_texture(
                    &wgpu::CopyExternalImageSourceInfo {
                        source: wgpu::ExternalImageSource::HTMLVideoElement(v.clone()),
                        origin: wgpu::Origin2d::ZERO,
                        flip_y: false,
                    },
                    wgpu::CopyExternalImageDestInfo {
                        texture: &gpu.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                        color_space: wgpu::PredefinedColorSpace::Srgb,
                        premultiplied_alpha: false,
                    },
                    size,
                );
                renderer.mark_override_image_dirty(&gpu.image);
                images.set_frame(
                    &format!("{}{key}", rux_layout::VIDEO_FRAME),
                    Some(vello::peniko::ImageBrush::new(gpu.image.clone())),
                );
            }
        });
    });
}

/// After a frame's scene is built: each video learns whether the painter
/// drew it. Says whether another frame is wanted at once: a video in sight
/// is playing, so its next picture is due, or one came back into sight.
pub(crate) fn after_scene(handles: &[(String, Handle)], images: &mut rux_paint::ImageCache) -> bool {
    let drawn = images.take_frames_drawn();
    SOUNDS.with(|m| {
        let m = m.borrow();
        let mut again = false;
        for (key, h) in handles {
            let Some(s) = h.get::<VideoRes>().and_then(|v| m.get(&v.0)) else { continue };
            let shown = drawn.contains(&format!("{}{key}", rux_layout::VIDEO_FRAME));
            let was = s.shown.replace(shown);
            again |= shown && (s.playing() || !was);
        }
        again
    })
}

thread_local! {
    /// The browser left fullscreen by itself: its Escape, which the page
    /// never sees, or its own controls.
    static LEFT_FULLSCREEN: Cell<bool> = const { Cell::new(false) };
    static WATCHING: Cell<bool> = const { Cell::new(false) };
}

/// Listen, once, for the browser leaving fullscreen, so a video in it
/// leaves too ([`left_fullscreen`]).
pub(crate) fn watch_fullscreen() {
    if WATCHING.with(|w| w.replace(true)) {
        return;
    }
    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
    let watched = document.clone();
    let f = Closure::<dyn FnMut()>::new(move || {
        if watched.fullscreen_element().is_none() {
            LEFT_FULLSCREEN.with(|l| l.set(true));
            rux_runtime::native::notify();
        }
    });
    let _ = document.add_event_listener_with_callback("fullscreenchange", f.as_ref().unchecked_ref());
    // For the page's life.
    f.forget();
}

/// The browser left fullscreen since this was last asked.
pub(crate) fn left_fullscreen() -> bool {
    LEFT_FULLSCREEN.with(|l| l.replace(false))
}
