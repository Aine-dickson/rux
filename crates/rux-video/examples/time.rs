//! Decode a whole video file as fast as it goes, timing each frame:
//! `cargo run --release -p rux-video --example time -- some.mp4`.

use std::time::Instant;

fn main() {
    let path = std::path::PathBuf::from(std::env::args().nth(1).expect("a video file"));
    let bytes = std::fs::read(&path).expect("read the file");
    let hint = path.extension().map(|e| e.to_string_lossy().into_owned());
    let t = Instant::now();
    let mut d = rux_video::Decoder::open(rux_video::Input::Bytes {
        bytes: bytes.into(),
        hint,
    })
    .unwrap_or_else(|e| {
        eprintln!("{} ({})", e, e.kind);
        std::process::exit(1);
    });
    println!(
        "{:?}, opened in {:.1} ms",
        d.info(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    let mut ms = Vec::new();
    let mut times = Vec::new();
    loop {
        let t = Instant::now();
        let Some(f) = d.next_frame().expect("decode") else {
            break;
        };
        ms.push(t.elapsed().as_secs_f64() * 1000.0);
        times.push(f.time);
    }
    if let (Some(first), Some(last)) = (times.first(), times.last()) {
        println!("frames from {first:.3} s to {last:.3} s");
    }
    ms.sort_by(f64::total_cmp);
    // A second open, as a second video on the page would be.
    let t = Instant::now();
    let bytes = std::fs::read(&path).expect("read the file");
    let hint = path.extension().map(|e| e.to_string_lossy().into_owned());
    let _ = rux_video::Decoder::open(rux_video::Input::Bytes {
        bytes: bytes.into(),
        hint,
    });
    println!(
        "opened again in {:.1} ms",
        t.elapsed().as_secs_f64() * 1000.0
    );
    let at = |q: f64| ms[((ms.len() - 1) as f64 * q).round() as usize];
    println!(
        "{} frames: median {:.2} ms, p95 {:.2}, max {:.2}",
        ms.len(),
        at(0.5),
        at(0.95),
        at(1.0)
    );
}
