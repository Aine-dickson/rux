//! The app's one output stream, opened by the first sound played.
//!
//! **It follows the default device.** Headphones pulled, or another output
//! chosen, and the platform says the stream is no longer good: it is built
//! again on whatever is the default now, and what was playing carries on
//! from where it was. With no output left at all, every sound pauses.
//!
//! **It closes when the app is put away** ([`pause_all`]): every sound
//! pauses and the device is let go, and the next `play` opens it again.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, FromSample, SampleFormat, SizedSample, StreamConfig};

use crate::mixer::Voice;
use crate::{AudioError, Mixer};

struct Output {
    /// Kept so the stream keeps playing; dropped, it stops.
    _stream: cpal::Stream,
    mixer: Arc<Mutex<Mixer>>,
    rate: u32,
    /// Which opening this is, so a late error from a stream already
    /// replaced does not replace its successor.
    generation: u64,
}

static OUTPUT: Mutex<Option<Output>> = Mutex::new(None);

static GENERATION: AtomicU64 = AtomicU64::new(1);

/// The mixer every sound plays into, and its rate, opening the default
/// output device the first time.
pub(crate) fn mixer() -> Result<(Arc<Mutex<Mixer>>, u32), AudioError> {
    let mut out = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(o) = out.as_ref() {
        return Ok((Arc::clone(&o.mixer), o.rate));
    }
    let o = open()?;
    let answer = (Arc::clone(&o.mixer), o.rate);
    *out = Some(o);
    Ok(answer)
}

/// Whether any sound is playing.
pub(crate) fn any_playing() -> bool {
    let out = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    out.as_ref().is_some_and(|o| o.mixer.lock().unwrap_or_else(|e| e.into_inner()).any_playing())
}

/// See [`crate::audible`].
pub(crate) fn audible() -> u8 {
    let out = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    out.as_ref().map_or(0, |o| o.mixer.lock().unwrap_or_else(|e| e.into_inner()).audible())
}

/// Pause every sound and let the output device go.
pub(crate) fn pause_all() {
    let old = OUTPUT.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(old) = old {
        let sounds = old.mixer.lock().unwrap_or_else(|e| e.into_inner()).sounds();
        for s in &sounds {
            s.state().playing = false;
        }
        // The stream is dropped with no lock held: its audio thread, which
        // takes the mixer's, is joined on the way.
        drop(old);
        crate::notify();
    }
}

/// The stream of `generation` is no longer good: build it again on the
/// default device, on a thread of its own, since this is called from the
/// failed stream's, which dropping it joins.
fn lost(generation: u64) {
    let _ = std::thread::Builder::new().name("rux-audio-reopen".into()).spawn(move || {
        let mut out = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
        if out.as_ref().map(|o| o.generation) != Some(generation) {
            return;
        }
        let Some(old) = out.take() else { return };
        let sounds = old.mixer.lock().unwrap_or_else(|e| e.into_inner()).sounds();
        drop(old);
        match open() {
            Ok(new) => {
                // Voices again at the new device's rate, each from where its
                // sound is.
                let voices: Vec<Voice> = sounds.iter().filter_map(|s| Voice::new(s, new.rate).ok()).collect();
                let mut m = new.mixer.lock().unwrap_or_else(|e| e.into_inner());
                for v in voices {
                    m.add(v);
                }
                drop(m);
                *out = Some(new);
            }
            Err(e) => {
                eprintln!("rux: sound output lost: {e}");
                for s in &sounds {
                    s.state().playing = false;
                }
            }
        }
        drop(out);
        crate::notify();
    });
}

fn open() -> Result<Output, AudioError> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| AudioError::new("device", "there is no sound output on this device"))?;
    let supported = device
        .default_output_config()
        .map_err(|e| AudioError::new("device", format!("the sound output cannot be opened: {e}")))?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let rate = config.sample_rate;
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
    let mixer = Arc::new(Mutex::new(Mixer::new(rate, config.channels as usize)));
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &config, &mixer, generation),
        SampleFormat::I16 => build::<i16>(&device, &config, &mixer, generation),
        SampleFormat::U16 => build::<u16>(&device, &config, &mixer, generation),
        SampleFormat::I32 => build::<i32>(&device, &config, &mixer, generation),
        other => return Err(AudioError::new("device", format!("the sound output wants {other} samples"))),
    }?;
    stream.play().map_err(|e| AudioError::new("device", format!("the sound output will not start: {e}")))?;
    Ok(Output { _stream: stream, mixer, rate, generation })
}

fn build<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mixer: &Arc<Mutex<Mixer>>,
    generation: u64,
) -> Result<cpal::Stream, AudioError>
where
    T: SizedSample + FromSample<f32>,
{
    let mixer = Arc::clone(mixer);
    let mut scratch: Vec<f32> = Vec::new();
    device
        .build_output_stream::<T, _, _>(
            *config,
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                scratch.resize(data.len(), 0.0);
                let when = info.timestamp();
                let latency = when.playback.duration_since(when.callback);
                mixer.lock().unwrap_or_else(|e| e.into_inner()).mix_heard_after(&mut scratch, latency);
                for (d, s) in data.iter_mut().zip(&scratch) {
                    *d = T::from_sample(s.clamp(-1.0, 1.0));
                }
            },
            move |e| match e.kind() {
                // Rerouted by the platform itself: nothing to do.
                ErrorKind::DeviceChanged | ErrorKind::Xrun => {}
                ErrorKind::StreamInvalidated | ErrorKind::DeviceNotAvailable => lost(generation),
                _ => eprintln!("rux: sound output: {e}"),
            },
            None,
        )
        .map_err(|e| AudioError::new("device", format!("the sound output cannot be opened: {e}")))
}
