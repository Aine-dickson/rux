//! The app's one output stream, opened by the first sound played.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};

use crate::{AudioError, Mixer};

struct Output {
    /// Kept so the stream keeps playing; dropped, it stops.
    _stream: cpal::Stream,
    mixer: Arc<Mutex<Mixer>>,
    rate: u32,
}

static OUTPUT: Mutex<Option<Output>> = Mutex::new(None);

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
    let mixer = Arc::new(Mutex::new(Mixer::new(rate, config.channels as usize)));
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &config, &mixer),
        SampleFormat::I16 => build::<i16>(&device, &config, &mixer),
        SampleFormat::U16 => build::<u16>(&device, &config, &mixer),
        SampleFormat::I32 => build::<i32>(&device, &config, &mixer),
        other => return Err(AudioError::new("device", format!("the sound output wants {other} samples"))),
    }?;
    stream.play().map_err(|e| AudioError::new("device", format!("the sound output will not start: {e}")))?;
    Ok(Output { _stream: stream, mixer, rate })
}

fn build<T>(device: &cpal::Device, config: &StreamConfig, mixer: &Arc<Mutex<Mixer>>) -> Result<cpal::Stream, AudioError>
where
    T: SizedSample + FromSample<f32>,
{
    let mixer = Arc::clone(mixer);
    let mut scratch: Vec<f32> = Vec::new();
    device
        .build_output_stream::<T, _, _>(
            *config,
            move |data: &mut [T], _| {
                scratch.resize(data.len(), 0.0);
                mixer.lock().unwrap_or_else(|e| e.into_inner()).mix(&mut scratch);
                for (d, s) in data.iter_mut().zip(&scratch) {
                    *d = T::from_sample(s.clamp(-1.0, 1.0));
                }
            },
            |e| eprintln!("rux: sound output: {e}"),
            None,
        )
        .map_err(|e| AudioError::new("device", format!("the sound output cannot be opened: {e}")))
}
