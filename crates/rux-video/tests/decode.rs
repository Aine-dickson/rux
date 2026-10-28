//! Each fixture decoded: two seconds at 10 fps, 160 by 120, made with ffmpeg.
//! The picture's top left is red, its top right green, its bottom half blue
//! until one second in and white after, so a picture upside down, with its
//! channels swapped, or from the wrong time all show.

use std::sync::Arc;

use rux_video::{Decoder, Frame, Input};

fn fixture(name: &str) -> Input {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let bytes: Arc<[u8]> = std::fs::read(&path).unwrap().into();
    Input::Bytes {
        bytes,
        hint: path.extension().map(|e| e.to_string_lossy().into_owned()),
    }
}

/// The colour at the middle of each quarter: top left, top right, bottom.
fn quarters(f: &Frame) -> [[u8; 3]; 3] {
    let at = |x: u32, y: u32| {
        let i = ((y * f.width + x) * 4) as usize;
        [f.rgba[i], f.rgba[i + 1], f.rgba[i + 2]]
    };
    let (w, h) = (f.width, f.height);
    [at(w / 4, h / 4), at(3 * w / 4, h / 4), at(w / 2, 3 * h / 4)]
}

/// `c` is near `want`, within what a lossy codec does to a flat colour.
fn near(c: [u8; 3], want: [u8; 3]) -> bool {
    c.iter()
        .zip(want)
        .all(|(a, b)| (*a as i32 - b as i32).abs() <= 24)
}

const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const WHITE: [u8; 3] = [255, 255, 255];

fn check(name: &str, f: &Frame) {
    let [tl, tr, bottom] = quarters(f);
    assert!(near(tl, RED), "{name} {f:?}: top left {tl:?}");
    assert!(near(tr, GREEN), "{name} {f:?}: top right {tr:?}");
    let want = if f.time < 0.95 { BLUE } else { WHITE };
    assert!(
        near(bottom, want),
        "{name} {f:?}: bottom {bottom:?}, want {want:?}"
    );
    assert!(f.rgba.chunks(4).all(|p| p[3] == 255), "{name}: alpha");
}

#[cfg(windows)]
#[test]
fn h264_and_vp9_decode_every_frame() {
    for gpu in [true, false] {
        for file in ["clip.mp4", "silent.mp4", "clip.webm"] {
            each(file, gpu);
        }
    }
}

#[cfg(windows)]
fn each(file: &str, gpu: bool) {
    {
        let name = &format!("{file}{}", if gpu { "" } else { " in software" });
        let mut d = match Decoder::open_on(fixture(file), gpu) {
            Ok(d) => d,
            // VP9 needs the store's VP9 Video Extensions, which a machine
            // may not have; that has to be the error, not a crash.
            Err(e) if file == "clip.webm" && e.kind == "unsupported" => {
                eprintln!("{name}: {e} (skipped)");
                return;
            }
            Err(e) => panic!("{name}: {e}"),
        };
        let info = d.info().clone();
        assert_eq!((info.width, info.height), (160, 120), "{name}");
        let duration = info
            .duration
            .unwrap_or_else(|| panic!("{name}: no duration"));
        assert!((duration - 2.0).abs() < 0.1, "{name}: {duration} s");
        assert_eq!(info.has_audio, file != "silent.mp4", "{name}");
        let mut times = Vec::new();
        while let Some(f) = d.next_frame().unwrap() {
            assert_eq!(
                (f.width, f.height, f.rgba.len()),
                (160, 120, 160 * 120 * 4),
                "{name}"
            );
            check(name, &f);
            times.push(f.time);
        }
        assert_eq!(times.len(), 20, "{name}: {times:?}");
        assert!(
            times.windows(2).all(|t| (t[1] - t[0] - 0.1).abs() < 0.01),
            "{name}: {times:?}"
        );
    }
}

#[cfg(windows)]
#[test]
fn a_seek_lands_on_the_frame_showing_then() {
    for gpu in [true, false] {
        seeks(Decoder::open_on(fixture("clip.mp4"), gpu).unwrap());
    }
}

#[cfg(windows)]
fn seeks(mut d: Decoder) {
    // Key frames every half second: 1.25 s is mid-group, after the change.
    d.seek(1.25).unwrap();
    let f = d.next_frame().unwrap().unwrap();
    assert!((f.time - 1.2).abs() < 0.01, "{f:?}");
    check("after a seek", &f);
    // And back, before the change.
    d.seek(0.35).unwrap();
    let f = d.next_frame().unwrap().unwrap();
    assert!((f.time - 0.3).abs() < 0.01, "{f:?}");
    check("seeking back", &f);
    // Past the end is the end.
    d.seek(5.0).unwrap();
    let rest = std::iter::from_fn(|| d.next_frame().unwrap()).count();
    assert!(rest <= 1, "{rest} frames after the end");
}

/// Two B-frames, so ffmpeg writes an edit list that Media Foundation does
/// not apply: the pictures still start at 0 and end inside the length.
#[cfg(windows)]
#[test]
fn b_frames_start_at_zero() {
    for gpu in [true, false] {
        let mut d = Decoder::open_on(fixture("bframes.mp4"), gpu).unwrap();
        let mut frames = Vec::new();
        while let Some(f) = d.next_frame().unwrap() {
            check("bframes.mp4", &f);
            frames.push(f.time);
        }
        assert_eq!(frames.len(), 20, "{frames:?}");
        assert!(frames[0].abs() < 1e-3 && (frames[19] - 1.9).abs() < 1e-3, "{frames:?}");
        d.seek(1.25).unwrap();
        let f = d.next_frame().unwrap().unwrap();
        assert!((f.time - 1.2).abs() < 0.01, "{f:?}");
        check("bframes.mp4 after a seek", &f);
    }
}

#[test]
fn not_a_video_is_an_error() {
    let bytes: Arc<[u8]> = b"this is not a video".to_vec().into();
    let e = Decoder::open(Input::Bytes {
        bytes,
        hint: Some("mp4".into()),
    })
    .err()
    .expect("an error");
    assert_eq!(e.kind, "unsupported", "{e}");
}
