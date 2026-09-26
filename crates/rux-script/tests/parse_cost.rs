//! What the front door costs: Rux's parse against the fork's compile, over
//! every `<script>` in `examples/`. Step 2 of `docs/11-next.md` parses every
//! script twice until the fork goes in step 5, so the second parse should be
//! a small fraction of the first compile, not a second one.
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
fn rux_parse_against_the_forks_compile() {
    let scripts = scripts();
    let engine = rhai::Engine::new();
    let opts = rux_syntax::Options { declarations: true };
    const ROUNDS: u32 = 200;
    let (mut ours, mut theirs, mut lexing, mut bytes, mut both) = (0u128, 0u128, 0u128, 0usize, 0usize);
    for (_, src) in &scripts {
        // Only scripts both can read are compared: a document script still
        // holds `prop`, `computed` and the lifecycle blocks, which only Rux's
        // parser knows.
        if rux_syntax::parse(src, opts).is_err() || engine.compile(src).is_err() {
            continue;
        }
        both += 1;
        bytes += src.len();
        let t = Instant::now();
        for _ in 0..ROUNDS {
            std::hint::black_box(rux_syntax::parse(src, opts).unwrap());
        }
        ours += t.elapsed().as_nanos();
        let t = Instant::now();
        for _ in 0..ROUNDS {
            std::hint::black_box(rux_syntax::lexer::lex(src).unwrap());
        }
        lexing += t.elapsed().as_nanos();
        let t = Instant::now();
        for _ in 0..ROUNDS {
            std::hint::black_box(engine.compile(src).unwrap());
        }
        theirs += t.elapsed().as_nanos();
    }
    let per = |n: u128| n as f64 / ROUNDS as f64 / 1000.0;
    println!(
        "{both} of {} scripts, {bytes} bytes: rux-syntax {:.1} µs (lexing {:.1} µs), the fork {:.1} µs, per pass over all of them ({:.0}%)",
        scripts.len(),
        per(ours),
        per(lexing),
        per(theirs),
        100.0 * ours as f64 / theirs as f64
    );
}
