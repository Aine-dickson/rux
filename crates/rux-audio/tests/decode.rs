//! Each format Rux plays, read from a fixture and played into a mixer with
//! no device: a quarter second of a 440 Hz tone, in stereo at 44.1 kHz,
//! made with ffmpeg.

use std::sync::{Arc, Mutex};

use rux_audio::{Input, Mixer, Sound};

fn fixture(name: &str) -> Input {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    let bytes: Arc<[u8]> = std::fs::read(&path).unwrap().into();
    Input::Bytes { bytes, hint: path.extension().map(|e| e.to_string_lossy().into_owned()) }
}

/// The left channel of all of `sound`, played at 44.1 kHz.
fn left(sound: &Sound) -> Vec<f32> {
    let m = Mutex::new(Mixer::new(44_100, 2));
    sound.play_on(&m, 44_100).unwrap();
    let mut out = vec![0.0; 2 * 44_100];
    m.lock().unwrap().mix(&mut out);
    out.chunks(2).map(|f| f[0]).collect()
}

#[test]
fn every_format_plays_its_tone() {
    for name in ["tone.wav", "tone.mp3", "tone.m4a", "tone.flac", "tone.ogg"] {
        let sound = Sound::load(fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let d = sound.duration().unwrap_or_else(|| panic!("{name}: no duration"));
        assert!((d - 0.25).abs() < 0.06, "{name}: {d} s");
        let samples = left(&sound);
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.05, "{name}: silent, peak {peak}");
        // 440 Hz for a quarter second crosses zero about 220 times. Counted
        // on what is audible: AAC in MP4 keeps its encoder's near-silent
        // priming (see the crate's docs), whose noise crosses zero too.
        let audible: Vec<f32> = samples.into_iter().filter(|s| s.abs() > 0.005).collect();
        let crossings = audible.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        assert!((200..=240).contains(&crossings), "{name}: {crossings} crossings");
        assert!(!sound.playing() && sound.ended() == 1, "{name}: played to its end");
    }
}

#[test]
fn opus_is_not_played() {
    let e = Sound::load(fixture("tone.opus")).err().expect("an error");
    assert_eq!(e.kind, "unsupported", "{e}");
}

#[test]
fn what_is_not_a_sound_says_so() {
    let bytes: Arc<[u8]> = b"this is not a sound at all, only text".to_vec().into();
    let e = Sound::load(Input::Bytes { bytes, hint: None }).err().expect("an error");
    assert_eq!(e.kind, "unsupported", "{e}");
}
