//! Reading a sound's samples: symphonia's probe, its track's decoder, and
//! seeking, as interleaved `f32` at the file's own rate.

use std::io::Cursor;

use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, TimeBase};

use crate::{AudioError, Input};

/// A sound being read: its container, its audio track's decoder, and what
/// that track is.
pub(crate) struct Decoding {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track: u32,
    /// Frames per second.
    pub rate: u32,
    /// Samples per frame.
    pub channels: usize,
    /// Its length, when the container says.
    pub duration: Option<f64>,
    /// Frames still to drop after an accurate seek, which lands on the packet
    /// before the time asked for.
    skip: usize,
    /// The track's time base, which a packet's durations are counted in.
    time_base: Option<TimeBase>,
}

impl Decoding {
    pub fn open(input: &Input) -> Result<Self, AudioError> {
        let (source, hint): (Box<dyn MediaSource>, Option<String>) = match input {
            Input::Bytes { bytes, hint } => (Box::new(Cursor::new(bytes.clone())), hint.clone()),
            Input::Path(path) => {
                let file = std::fs::File::open(path)
                    .map_err(|e| AudioError::new("io", format!("cannot open {}: {e}", path.display())))?;
                (Box::new(file), path.extension().map(|e| e.to_string_lossy().to_lowercase()))
            }
        };
        let mut h = Hint::new();
        if let Some(ext) = &hint {
            h.with_extension(ext);
        }
        let mss = MediaSourceStream::new(source, Default::default());
        let format = symphonia::default::get_probe()
            .probe(&h, mss, FormatOptions::default(), MetadataOptions::default())
            .map_err(|e| AudioError::new("unsupported", format!("not a sound Rux can read: {e}")))?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| AudioError::new("unsupported", "it has no sound in it"))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| AudioError::new("unsupported", "its sound's format is not known"))?;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .map_err(|e| AudioError::new("unsupported", format!("its sound is in a format Rux cannot play: {e}")))?;
        let rate = params.sample_rate.unwrap_or(0);
        let channels = params.channels.as_ref().map_or(0, |c| c.count());
        let duration = match (track.time_base, track.duration) {
            (Some(tb), Some(d)) => tb.calc_duration(d).map(|t| t.as_secs_f64()),
            _ => None,
        }
        .or_else(|| match (track.num_frames, rate) {
            (Some(n), r) if r > 0 => Some(n as f64 / r as f64),
            _ => None,
        });
        let time_base = track.time_base;
        let track = track.id;
        Ok(Decoding { format, decoder, track, rate, channels, duration, skip: 0, time_base })
    }

    /// A packet's duration as frames of the decoded sound.
    fn frames(&self, d: symphonia::core::units::Duration) -> usize {
        match self.time_base {
            Some(tb) => (tb.calc_duration_saturating(d).as_secs_f64() * self.rate as f64).round() as usize,
            None => 0,
        }
    }

    /// The next stretch of samples, interleaved; `None` at the end. A packet
    /// that does not decode is skipped, as a player skips a damaged frame.
    pub fn next(&mut self) -> Result<Option<Vec<f32>>, AudioError> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) | Err(SymError::ResetRequired) => return Ok(None),
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(e) => return Err(AudioError::new("decode", e.to_string())),
            };
            if packet.track_id != self.track {
                continue;
            }
            let (trim_start, trim_end, valid) =
                (self.frames(packet.trim_start), self.frames(packet.trim_end), self.frames(packet.dur));
            let buf = match self.decoder.decode(&packet) {
                Ok(b) => b,
                Err(SymError::DecodeError(_)) | Err(SymError::IoError(_)) => continue,
                Err(e) => return Err(AudioError::new("decode", e.to_string())),
            };
            // A container that left them out is answered by the first buffer.
            if self.rate == 0 {
                self.rate = buf.spec().rate();
            }
            if self.channels == 0 {
                self.channels = buf.spec().channels().count();
            }
            let mut out = Vec::new();
            buf.copy_to_vec_interleaved(&mut out);
            let ch = self.channels.max(1);
            // The encoder's delay and padding, which a packet marks to be
            // trimmed. Most decoders trim them; one that gave the untrimmed
            // frames is trimmed here.
            let decoded = out.len() / ch;
            if trim_start + trim_end > 0 && decoded.abs_diff(valid + trim_start + trim_end) <= 1 {
                out.truncate(decoded.saturating_sub(trim_end) * ch);
                out.drain(..trim_start.min(out.len() / ch) * ch);
            }
            if self.skip > 0 {
                let drop = self.skip.min(out.len() / ch);
                self.skip -= drop;
                out.drain(..drop * ch);
            }
            if out.is_empty() {
                continue;
            }
            return Ok(Some(out));
        }
    }

    /// Go to `secs` from the start.
    pub fn seek(&mut self, secs: f64) -> Result<(), AudioError> {
        let time = Time::try_from_secs_f64(secs.max(0.0)).unwrap_or(Time::ZERO);
        let to = SeekTo::Time { time, track_id: Some(self.track) };
        let seeked = self.format.seek(SeekMode::Accurate, to).map_err(|e| AudioError::new("decode", e.to_string()))?;
        self.decoder.reset();
        // The packet found starts before the time asked for: drop the frames
        // between, counted in the track's own time base.
        let track = self.format.tracks().iter().find(|t| t.id == self.track);
        let tb = track.and_then(|t| t.time_base);
        self.skip = match tb {
            Some(tb) => {
                let required = tb.calc_time_saturating(seeked.required_ts).as_secs_f64();
                let actual = tb.calc_time_saturating(seeked.actual_ts).as_secs_f64();
                ((required - actual).max(0.0) * self.rate as f64).round() as usize
            }
            None => 0,
        };
        Ok(())
    }
}

/// The whole of a sound, decoded.
pub(crate) fn decode_all(d: &mut Decoding) -> Result<Vec<f32>, AudioError> {
    let mut all = Vec::new();
    while let Some(chunk) = d.next()? {
        all.extend_from_slice(&chunk);
    }
    Ok(all)
}
