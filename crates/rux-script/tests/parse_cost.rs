//! What Rux's parser costs, over every `<script>` in `examples/`.
//!
//! Until step 5 of `docs/11-next.md` removed the fork, this timed Rux's parse
//! against the fork's compile, since every script was read by both. The last
//! such run (step 3.0, 098d7b3) had Rux's parse at 26% to 28% of the fork's
//! compile in a release build.
//!
//! Ignored by default, since a timing is not a pass or a fail:
//!
//! ```text
//! cargo test --release -p rux-script --test parse_cost -- --ignored --nocapture
//! ```

use std::time::Instant;

fn scripts() -> Vec<(String, String)> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
    let mut out = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(dir)];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rux") {
                let text = std::fs::read_to_string(&path).unwrap();
                if let (Some(a), Some(b)) = (text.find("<script>"), text.rfind("</script>")) {
                    if a + 8 <= b {
                        out.push((path.display().to_string(), text[a + 8..b].to_string()));
                    }
                }
            }
        }
    }
    out
}

#[test]
#[ignore]
fn rux_parse() {
    let scripts = scripts();
    let opts = rux_syntax::Options { declarations: true };
    const ROUNDS: u32 = 200;
    let (mut parsing, mut lexing, mut bytes, mut read) = (0u128, 0u128, 0usize, 0usize);
    for (_, src) in &scripts {
        if rux_syntax::parse(src, opts).is_err() {
            continue;
        }
        read += 1;
        bytes += src.len();
        let t = Instant::now();
        for _ in 0..ROUNDS {
            std::hint::black_box(rux_syntax::parse(src, opts).unwrap());
        }
        parsing += t.elapsed().as_nanos();
        let t = Instant::now();
        for _ in 0..ROUNDS {
            std::hint::black_box(rux_syntax::lexer::lex(src).unwrap());
        }
        lexing += t.elapsed().as_nanos();
    }
    let per = |n: u128| n as f64 / ROUNDS as f64 / 1000.0;
    println!(
        "{read} of {} scripts, {bytes} bytes: rux-syntax {:.1} µs (lexing {:.1} µs) per pass over all of them",
        scripts.len(),
        per(parsing),
        per(lexing),
    );
}
