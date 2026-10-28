//! `loadSound` and the `Sound` it gives (`docs/13-file-input-and-media-plan.md`,
//! B.1): the calls behind Rux's own module's sound items, played by
//! `rux-audio` on the desktop and Android.
//!
//! A `Sound` is a resource. Its methods play it; its properties are read
//! through getters, and the mixer reports through [`rux_runtime::native::notify`] as
//! it plays, so `{{ s.currentTime }}` follows it.

use rux_runtime::native::{Any, Call, Error, Handle, Resource};

/// A loaded sound, as Rux holds it.
pub(crate) struct SoundRes(rux_audio::Sound);

impl Resource for SoundRes {
    const NAME: &'static str = "Sound";
}

fn error(e: rux_audio::AudioError) -> Error {
    Error::new(e.kind, e.message)
}

/// The sound a call's first argument is.
fn sound(args: &[Any]) -> Result<std::sync::Arc<SoundRes>, Error> {
    match args.first() {
        Some(Any::Resource(h)) => h.get::<SoundRes>().ok_or_else(|| Error::new("type", "this is not a `Sound`")),
        _ => Err(Error::new("type", "this is not a `Sound`")),
    }
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

/// Where `src`'s bytes come from: read now, on the script's own thread,
/// since what a picked file's URL holds is kept there.
fn input(src: &str) -> Result<rux_audio::Input, Error> {
    #[cfg(target_os = "android")]
    if src.starts_with("content://") {
        let bytes = crate::android_read_uri(src).ok_or_else(|| Error::new("io", format!("cannot read {src}")))?;
        return Ok(rux_audio::Input::Bytes { bytes: bytes.into(), hint: None });
    }
    let bytes = rux_runtime::read_media(src).map_err(|e| Error::new("io", format!("cannot read {src}: {e}")))?;
    let hint = std::path::Path::new(src.split(['#', '?']).next().unwrap_or(src))
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase());
    Ok(rux_audio::Input::Bytes { bytes: bytes.into(), hint })
}

/// Android's audio focus, as what can be heard now asks: held while a sound
/// plays at some volume, so a call or another app's music pauses Rux's, and
/// given back when none does. `2` for a long sound, `1` for a short one,
/// `0` for none (`RuxActivity.ruxAudioFocus`). A sound playing at volume 0
/// holds none: a silent sound has no business pausing the person's music.
pub(crate) fn refocus() {
    #[cfg(target_os = "android")]
    crate::android_audio_focus(i32::from(rux_audio::audible()));
}

/// The calls, keyed as the module's are.
pub(crate) fn calls() -> Vec<(String, Call)> {
    rux_audio::set_notify(rux_runtime::native::notify);
    // The last sound ended by itself: the focus goes back, from a thread of
    // its own rather than the audio thread, and only if nothing started
    // playing in the meantime.
    #[cfg(target_os = "android")]
    rux_audio::set_quiet(|| {
        let _ = std::thread::Builder::new().name("rux-audio-focus".into()).spawn(refocus);
    });
    // Each of these stops a sound, so the focus may go with it.
    let act = |f: fn(&rux_audio::Sound)| {
        Call::sync(move |args| {
            f(&sound(&args)?.0);
            refocus();
            rux_runtime::native::notify();
            Ok(Any::None)
        })
    };
    let get = |f: fn(&rux_audio::Sound) -> Any| Call::sync(move |args| Ok(f(&sound(&args)?.0)));
    vec![
        (
            "loadSound".into(),
            Call::future(|args: Vec<Any>| {
                let src = match args.first() {
                    Some(Any::Str(s)) => Ok(s.clone()),
                    _ => Err(Error::new("type", "`loadSound` takes the sound's path or URL")),
                };
                let read = src.and_then(|s| input(&s));
                // Decoded on the thread an async native call runs on.
                async move {
                    let sound = rux_audio::Sound::load(read?).map_err(error)?;
                    Ok(Any::Resource(Handle::new(SoundRes(sound))))
                }
            }),
        ),
        (
            "Sound.play".into(),
            Call::sync(|args| {
                let s = sound(&args)?;
                s.0.play().map_err(error)?;
                refocus();
                rux_runtime::native::notify();
                Ok(Any::None)
            }),
        ),
        ("Sound.pause".into(), act(rux_audio::Sound::pause)),
        ("Sound.stop".into(), act(rux_audio::Sound::stop)),
        (
            "Sound.seek".into(),
            Call::sync(|args| {
                sound(&args)?.0.seek(number(args.get(1))?);
                rux_runtime::native::notify();
                Ok(Any::None)
            }),
        ),
        ("Sound.volume".into(), get(|s| Any::Float(s.volume() as f64))),
        ("Sound.loop".into(), get(|s| Any::Bool(s.looping()))),
        ("Sound.currentTime".into(), get(|s| Any::Float(s.current_time()))),
        ("Sound.duration".into(), get(|s| s.duration().map_or(Any::None, Any::Float))),
        ("Sound.playing".into(), get(|s| Any::Bool(s.playing()))),
        (
            "Sound.volume=".into(),
            Call::sync(|args| {
                sound(&args)?.0.set_volume(number(args.get(1))? as f32);
                // Muted or brought back, a playing sound is heard or not.
                refocus();
                rux_runtime::native::notify();
                Ok(Any::None)
            }),
        ),
        (
            "Sound.loop=".into(),
            Call::sync(|args| {
                sound(&args)?.0.set_looping(flag(args.get(1))?);
                rux_runtime::native::notify();
                Ok(Any::None)
            }),
        ),
        (
            "Sound.currentTime=".into(),
            Call::sync(|args| {
                sound(&args)?.0.seek(number(args.get(1))?);
                rux_runtime::native::notify();
                Ok(Any::None)
            }),
        ),
    ]
}
