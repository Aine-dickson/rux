//! Where a frame's time goes, measured only when `RUX_PROFILE` is set.
//!
//! The question it answers is whether script execution is a cost worth
//! engineering against at all, so it splits what the script tier does into its
//! parts: parsing source, lowering it to the typed IR, and running it. The
//! runtime and the shell add their own phases (rebuild, patch, layout, scene,
//! GPU) to the same counters, and the shell drains them once a frame, so everything that happened between two frames is
//! charged to the second.
//!
//! Off by default and a no-op on the web, where `Instant` does not exist. With
//! the variable unset, a probe is one relaxed load of a cached flag.

use std::cell::Cell;

/// A slice of work the profile charges time to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Source to Rux's AST: every script, binding and handler, the first
    /// time it is seen.
    Parse,
    /// Rux's AST lowered to the typed IR: a file when it loads, and each
    /// binding or handler the first time it runs (`docs/11-next.md`, steps 4
    /// and 5).
    Lower,
    /// The IR run by Rux's interpreter.
    Run,
    /// The styled tree rebuilt from state (script time inside it included).
    Rebuild,
    /// Changed bindings patched in place instead of a rebuild.
    Patch,
    /// Transitions and enter/leave advanced.
    Animate,
    /// Boxes laid out and text measured.
    Layout,
    /// The Vello scene built from the paints.
    Scene,
    /// Vello's render, the blit, submit and present.
    Gpu,
    /// Stylesheets parsed into rules, inside a rebuild or patch.
    Css,
    /// Selectors matched against elements, inside a rebuild or patch.
    Match,
}

pub const PHASES: usize = 11;

/// The phases that are the script tier's own; they lead the list.
const SCRIPT: usize = 3;

pub const NAMES: [&str; PHASES] =
    ["parse", "lower", "run", "rebuild", "patch", "animate", "layout", "scene", "gpu", "css", "match"];

thread_local! {
    static SPENT: [Cell<u64>; PHASES] = Default::default();
    static CALLS: [Cell<u32>; PHASES] = Default::default();
    /// Script time spent outside a rebuild or patch: a handler's own body.
    static LOOSE: Cell<u64> = const { Cell::new(0) };
    /// How many rebuilds or patches are running, so script inside one is not
    /// counted again as loose.
    static TREE: Cell<u32> = const { Cell::new(0) };
}

/// Whether profiling is on: `RUX_PROFILE` set in the environment, or set when
/// this crate was compiled. The second is for Android, where the system starts
/// the app and there is no environment to pass it; `RUX_PROFILE=1 rux build`
/// builds it in, and cargo rebuilds when the variable changes.
fn on() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ON.get_or_init(|| {
            option_env!("RUX_PROFILE").is_some() || std::env::var_os("RUX_PROFILE").is_some()
        })
    }
}

/// Whether the report should print every frame rather than a summary:
/// `RUX_PROFILE=each`.
pub fn each_frame() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        option_env!("RUX_PROFILE") == Some("each")
            || std::env::var("RUX_PROFILE").is_ok_and(|v| v == "each")
    }
}

/// Run `f`, charging its wall time to `phase` when profiling.
///
/// Nested phases are charged to both: a rebuild's figure includes the script
/// it ran. The report subtracts where that matters.
#[inline]
pub fn time<T>(phase: Phase, f: impl FnOnce() -> T) -> T {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = phase;
        f()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !on() {
            return f();
        }
        let tree = matches!(phase, Phase::Rebuild | Phase::Patch);
        if tree {
            TREE.with(|t| t.set(t.get() + 1));
        }
        let start = std::time::Instant::now();
        let out = f();
        let nanos = start.elapsed().as_nanos() as u64;
        if tree {
            TREE.with(|t| t.set(t.get() - 1));
        }
        add(phase, nanos);
        if (phase as usize) < SCRIPT && TREE.with(Cell::get) == 0 {
            LOOSE.with(|l| l.set(l.get() + nanos));
        }
        out
    }
}

/// Charge `nanos` to `phase` directly.
pub fn add(phase: Phase, nanos: u64) {
    let i = phase as usize;
    SPENT.with(|s| s[i].set(s[i].get() + nanos));
    CALLS.with(|c| c[i].set(c[i].get() + 1));
}

/// One frame's counters: nanoseconds and calls per phase, and the script time
/// that ran outside any rebuild or patch.
pub struct Taken {
    pub spent: [u64; PHASES],
    pub calls: [u32; PHASES],
    pub loose: u64,
}

/// What was charged since the last take, and zero the counters.
pub fn take() -> Taken {
    let mut spent = [0; PHASES];
    let mut calls = [0; PHASES];
    SPENT.with(|s| {
        for (i, c) in s.iter().enumerate() {
            spent[i] = c.replace(0);
        }
    });
    CALLS.with(|s| {
        for (i, c) in s.iter().enumerate() {
            calls[i] = c.replace(0);
        }
    });
    Taken { spent, calls, loose: LOOSE.with(|l| l.replace(0)) }
}

/// Whether profiling is on, for callers that need to skip their own setup.
pub fn active() -> bool {
    on()
}

/// Frames folded into a running summary, which the shell prints and resets.
#[derive(Default)]
pub struct Report {
    frames: u32,
    spent: [u64; PHASES],
    calls: [u64; PHASES],
    worst: [u64; PHASES],
    worst_frame: u64,
    total: u64,
    loose: u64,
}

impl Report {
    /// Fold one frame's counters in.
    pub fn frame(&mut self, taken: Taken) {
        let Taken { spent, calls, loose } = taken;
        self.frames += 1;
        let mut total = loose;
        for i in 0..PHASES {
            self.spent[i] += spent[i];
            self.calls[i] += u64::from(calls[i]);
            self.worst[i] = self.worst[i].max(spent[i]);
        }
        // A frame's total counts each moment once: script inside a rebuild is
        // already in the rebuild's figure, so only loose script is added.
        for i in [Phase::Rebuild, Phase::Patch, Phase::Animate, Phase::Layout, Phase::Scene, Phase::Gpu] {
            total += spent[i as usize];
        }
        self.total += total;
        self.loose += loose;
        self.worst_frame = self.worst_frame.max(total);
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// The summary as text, one line per phase, and reset.
    pub fn flush(&mut self) -> String {
        let n = self.frames.max(1) as f64;
        let ms = |ns: u64| ns as f64 / 1e6;
        let mut out = format!(
            "rux profile: {} frames, worst frame {:.2} ms\n  phase      avg ms   worst ms   calls/frame\n",
            self.frames,
            ms(self.worst_frame)
        );
        for i in 0..PHASES {
            out += &format!(
                "  {:<9} {:>7.3} {:>10.3} {:>13.1}\n",
                NAMES[i],
                ms(self.spent[i]) / n,
                ms(self.worst[i]),
                self.calls[i] as f64 / n
            );
        }
        let script: u64 = self.spent[..SCRIPT].iter().sum();
        out += &format!(
            "  script {:.3} ms/frame of {:.3} ms/frame measured ({:.0}%), {:.3} of it in handlers\n",
            ms(script) / n,
            ms(self.total) / n,
            100.0 * script as f64 / self.total.max(1) as f64,
            ms(self.loose) / n
        );
        *self = Report::default();
        out
    }
}
