//! Play a sound file through the default output, printing where it is:
//! `cargo run -p rux-audio --example play -- some.mp3`.

use std::time::{Duration, Instant};

fn main() {
    let path = std::env::args().nth(1).expect("a sound file to play");
    let sound = rux_audio::Sound::load(rux_audio::Input::Path(path.into())).unwrap_or_else(|e| {
        eprintln!("{} ({})", e, e.kind);
        std::process::exit(1);
    });
    println!("duration {:?}", sound.duration());
    sound.play().expect("play");
    let start = Instant::now();
    while sound.playing() {
        std::thread::sleep(Duration::from_millis(250));
        println!("{:6.2} s played, {:6.2} s by the clock", sound.current_time(), start.elapsed().as_secs_f64());
    }
    println!("ended {} time(s)", sound.ended());
}
