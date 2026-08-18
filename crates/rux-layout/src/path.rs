//! Path geometry: the `d` attribute of `<path>`, parsed once into something
//! the painter can draw and the animator can interpolate.
//!
//! Two decisions shape everything here.
//!
//! **The full SVG grammar is accepted and none of it survives parsing.** All
//! ten commands are read, in both cases, and every one of them is normalised to
//! absolute moves, absolute cubics and closes. Arcs become cubics, quadratics
//! become cubics, `H`/`V` become cubics, the smooth forms have their reflected
//! control points worked out and become cubics. Nothing downstream has ever
//! heard of an arc.
//!
//! That is not tidiness. A path drawn from a design tool is full of arcs and
//! smooth continuations, and had they reached the painter, every consumer would
//! need the whole grammar: the painter to draw it, the animator to interpolate
//! it, and any future measurement to bound it. Normalising once at the edge
//! means three places speak three commands instead of ten.
//!
//! **Straight lines are cubics too**, with their control points at the thirds.
//! Geometrically identical, and it is what makes morphing work at all: two
//! paths interpolate when their command sequences match, and a rectangle whose
//! sides are cubics can become a circle. Keeping `L` as its own command would
//! make that pair incompatible for no gain.

/// One drawing command, absolute, in the element's own coordinates.
///
/// Three variants, because everything reduces to three. See the module note for
/// why the other seven do not survive parsing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathCmd {
    /// Start a new subpath at this point.
    Move { x: f32, y: f32 },
    /// Cubic Bézier from the current point, via two controls, to `(x, y)`.
    Curve {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x: f32,
        y: f32,
    },
    /// Close the current subpath back to where it started.
    Close,
}

/// The geometry carried by a `<path>` leaf.
///
/// `d` is kept alongside the parsed commands because it is the identity the
/// animator compares: two builds whose `d` strings are equal cannot have
/// different geometry, and string equality is far cheaper than walking two
/// command lists on every build of every path on screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathContent {
    pub d: String,
    pub commands: Vec<PathCmd>,
}

impl PathContent {
    pub fn parse(d: &str) -> Self {
        Self {
            d: d.to_string(),
            commands: parse_path(d),
        }
    }

    /// The bounding box of the geometry, as `(min_x, min_y, max_x, max_y)`.
    ///
    /// Control points are included rather than the true curve extrema, so this
    /// is conservative: a curve never leaves the hull of its control points. It
    /// is what gives a `<path>` an intrinsic size when CSS names no width or
    /// height, and being a little generous there costs a few empty pixels
    /// rather than a clipped drawing, which is the right way round to be wrong.
    pub fn bounds(&self) -> Option<(f32, f32, f32, f32)> {
        let mut b: Option<(f32, f32, f32, f32)> = None;
        let mut add = |x: f32, y: f32| {
            b = Some(match b {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        };
        for c in &self.commands {
            match *c {
                PathCmd::Move { x, y } => add(x, y),
                PathCmd::Curve {
                    x1,
                    y1,
                    x2,
                    y2,
                    x,
                    y,
                } => {
                    add(x1, y1);
                    add(x2, y2);
                    add(x, y);
                }
                PathCmd::Close => {}
            }
        }
        b
    }
}

/// Interpolate between two paths, or say that they cannot be.
///
/// The rule is the one tier 1 already states for every other animatable value:
/// same shape interpolates, anything else jumps. For a length that means the
/// same unit; for a path it means the same sequence of commands. Two paths with
/// matching sequences move point by point; two that disagree anywhere return
/// `None` and the caller cuts to the end.
///
/// A cheap sequence check rather than a clever one is deliberate. Resampling
/// two differently-shaped paths onto a common command list is a real technique
/// and a real research problem, and guessing which point of a circle
/// corresponds to which corner of a square produces a fold as often as a morph.
/// An author who wants two shapes to morph writes them with the same commands,
/// which is exactly the discipline every morphing tool already imposes.
pub fn lerp(a: &[PathCmd], b: &[PathCmd], t: f32) -> Option<Vec<PathCmd>> {
    if a.len() != b.len() {
        return None;
    }
    let mut out = Vec::with_capacity(a.len());
    for (u, v) in a.iter().zip(b) {
        let m = |p: f32, q: f32| p + (q - p) * t;
        out.push(match (*u, *v) {
            (PathCmd::Move { x: ax, y: ay }, PathCmd::Move { x: bx, y: by }) => PathCmd::Move {
                x: m(ax, bx),
                y: m(ay, by),
            },
            (
                PathCmd::Curve {
                    x1: ax1,
                    y1: ay1,
                    x2: ax2,
                    y2: ay2,
                    x: ax,
                    y: ay,
                },
                PathCmd::Curve {
                    x1: bx1,
                    y1: by1,
                    x2: bx2,
                    y2: by2,
                    x: bx,
                    y: by,
                },
            ) => PathCmd::Curve {
                x1: m(ax1, bx1),
                y1: m(ay1, by1),
                x2: m(ax2, bx2),
                y2: m(ay2, by2),
                x: m(ax, bx),
                y: m(ay, by),
            },
            (PathCmd::Close, PathCmd::Close) => PathCmd::Close,
            // The sequences diverge here, so there is no correspondence to
            // interpolate along and the whole path jumps.
            _ => return None,
        });
    }
    Some(out)
}

/// A quadratic raised to a cubic. Exact, not an approximation: every quadratic
/// is a cubic whose controls sit two thirds of the way to the quadratic's.
fn quad_to_cubic(p0: (f32, f32), q: (f32, f32), p: (f32, f32)) -> PathCmd {
    PathCmd::Curve {
        x1: p0.0 + 2.0 / 3.0 * (q.0 - p0.0),
        y1: p0.1 + 2.0 / 3.0 * (q.1 - p0.1),
        x2: p.0 + 2.0 / 3.0 * (q.0 - p.0),
        y2: p.1 + 2.0 / 3.0 * (q.1 - p.1),
        x: p.0,
        y: p.1,
    }
}

/// A straight line as a cubic, controls at the thirds. See the module note.
fn line_to_cubic(p0: (f32, f32), p: (f32, f32)) -> PathCmd {
    PathCmd::Curve {
        x1: p0.0 + (p.0 - p0.0) / 3.0,
        y1: p0.1 + (p.1 - p0.1) / 3.0,
        x2: p0.0 + (p.0 - p0.0) * 2.0 / 3.0,
        y2: p0.1 + (p.1 - p0.1) * 2.0 / 3.0,
        x: p.0,
        y: p.1,
    }
}

/// A scanner over path data.
///
/// SVG path data is not whitespace-delimited and cannot be split on one. A
/// number ends where the next one begins, so `1-2` is two numbers, `.5.5` is
/// two numbers, and `M0 0L1 1` has no separator at all between the command and
/// its arguments. That is why this is a character scanner and not a `split`.
struct Scan<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Scan<'a> {
    fn new(s: &'a str) -> Self {
        Scan {
            s: s.as_bytes(),
            i: 0,
        }
    }

    fn skip_sep(&mut self) {
        while self.i < self.s.len() {
            match self.s[self.i] {
                b' ' | b'\t' | b'\r' | b'\n' | b',' => self.i += 1,
                _ => break,
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_sep();
        self.s.get(self.i).copied()
    }

    fn command(&mut self) -> Option<u8> {
        let c = self.peek()?;
        if c.is_ascii_alphabetic() {
            self.i += 1;
            Some(c)
        } else {
            None
        }
    }

    /// A number, in any of the forms SVG allows: `1`, `-1`, `.5`, `1.`, `1e3`,
    /// `1.5E-3`.
    fn number(&mut self) -> Option<f32> {
        self.skip_sep();
        let start = self.i;
        if matches!(self.s.get(self.i), Some(b'+') | Some(b'-')) {
            self.i += 1;
        }
        let mut digits = false;
        while matches!(self.s.get(self.i), Some(c) if c.is_ascii_digit()) {
            self.i += 1;
            digits = true;
        }
        if self.s.get(self.i) == Some(&b'.') {
            self.i += 1;
            while matches!(self.s.get(self.i), Some(c) if c.is_ascii_digit()) {
                self.i += 1;
                digits = true;
            }
        }
        if !digits {
            self.i = start;
            return None;
        }
        // An exponent, but only a complete one: `1e` is the number 1 followed
        // by a stray letter, and backing up is how `M1e` stays parseable.
        if matches!(self.s.get(self.i), Some(b'e') | Some(b'E')) {
            let save = self.i;
            self.i += 1;
            if matches!(self.s.get(self.i), Some(b'+') | Some(b'-')) {
                self.i += 1;
            }
            if matches!(self.s.get(self.i), Some(c) if c.is_ascii_digit()) {
                while matches!(self.s.get(self.i), Some(c) if c.is_ascii_digit()) {
                    self.i += 1;
                }
            } else {
                self.i = save;
            }
        }
        std::str::from_utf8(&self.s[start..self.i])
            .ok()?
            .parse::<f32>()
            .ok()
    }

    /// An arc's two flags, which are single characters and need no separator.
    ///
    /// `a1 1 0 011 1` is a real and legal spelling: `0`, `1`, `1` are the
    /// large-arc flag, the sweep flag and the x coordinate, run together. Read
    /// as numbers, `011` would be eleven and the arc would be silently wrong,
    /// which is why flags get their own reader.
    fn flag(&mut self) -> Option<bool> {
        match self.peek()? {
            b'0' => {
                self.i += 1;
                Some(false)
            }
            b'1' => {
                self.i += 1;
                Some(true)
            }
            _ => None,
        }
    }
}

/// Parse `d` into absolute moves, cubics and closes.
///
/// Malformed data stops the parse and keeps what was read. That is what SVG
/// itself specifies, and it is the better behaviour here too: a path with a
/// typo three quarters of the way along still draws its first three quarters,
/// which shows the author where the typo is far better than an empty box does.
pub fn parse_path(d: &str) -> Vec<PathCmd> {
    let mut out: Vec<PathCmd> = Vec::new();
    let mut sc = Scan::new(d);
    // The current point, the start of the current subpath (where `Z` returns
    // to), and the previous control point that `S` and `T` reflect.
    let mut cur = (0.0f32, 0.0f32);
    let mut start = (0.0f32, 0.0f32);
    let mut last_cubic_ctrl: Option<(f32, f32)> = None;
    let mut last_quad_ctrl: Option<(f32, f32)> = None;
    let mut cmd = 0u8;

    loop {
        let next = match sc.peek() {
            None => break,
            Some(c) => c,
        };
        if next.is_ascii_alphabetic() {
            cmd = sc.command().unwrap();
        } else if cmd == 0 {
            // Numbers before any command at all. Nothing to do with them.
            break;
        } else if cmd == b'M' || cmd == b'm' {
            // A repeated `M` is an implicit `L`, which is the one place the
            // repeat rule does not simply repeat the command. Easy to miss and
            // it makes every polygon written the short way come out wrong.
            cmd = if cmd == b'M' { b'L' } else { b'l' };
        } else if cmd == b'Z' || cmd == b'z' {
            break;
        }

        let rel = cmd.is_ascii_lowercase();
        let up = cmd.to_ascii_uppercase();
        // Where a relative coordinate is measured from. Absolute commands
        // measure from the origin, which is the same arithmetic with zero.
        let (ox, oy) = if rel { cur } else { (0.0, 0.0) };

        macro_rules! num {
            () => {
                match sc.number() {
                    Some(n) => n,
                    None => return out,
                }
            };
        }

        match up {
            b'M' => {
                let x = num!() + ox;
                let y = num!() + oy;
                out.push(PathCmd::Move { x, y });
                cur = (x, y);
                start = cur;
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
            }
            b'L' => {
                let x = num!() + ox;
                let y = num!() + oy;
                out.push(line_to_cubic(cur, (x, y)));
                cur = (x, y);
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
            }
            b'H' => {
                let x = num!() + ox;
                out.push(line_to_cubic(cur, (x, cur.1)));
                cur = (x, cur.1);
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
            }
            b'V' => {
                let y = num!() + oy;
                out.push(line_to_cubic(cur, (cur.0, y)));
                cur = (cur.0, y);
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
            }
            b'C' => {
                let x1 = num!() + ox;
                let y1 = num!() + oy;
                let x2 = num!() + ox;
                let y2 = num!() + oy;
                let x = num!() + ox;
                let y = num!() + oy;
                out.push(PathCmd::Curve {
                    x1,
                    y1,
                    x2,
                    y2,
                    x,
                    y,
                });
                cur = (x, y);
                last_cubic_ctrl = Some((x2, y2));
                last_quad_ctrl = None;
            }
            b'S' => {
                // The first control is the reflection of the previous curve's
                // second control about the current point. After anything that
                // was not a cubic there is nothing to reflect, and the spec
                // says the control coincides with the current point.
                let (x1, y1) = match last_cubic_ctrl {
                    Some((px, py)) => (2.0 * cur.0 - px, 2.0 * cur.1 - py),
                    None => cur,
                };
                let x2 = num!() + ox;
                let y2 = num!() + oy;
                let x = num!() + ox;
                let y = num!() + oy;
                out.push(PathCmd::Curve {
                    x1,
                    y1,
                    x2,
                    y2,
                    x,
                    y,
                });
                cur = (x, y);
                last_cubic_ctrl = Some((x2, y2));
                last_quad_ctrl = None;
            }
            b'Q' => {
                let qx = num!() + ox;
                let qy = num!() + oy;
                let x = num!() + ox;
                let y = num!() + oy;
                out.push(quad_to_cubic(cur, (qx, qy), (x, y)));
                cur = (x, y);
                last_quad_ctrl = Some((qx, qy));
                last_cubic_ctrl = None;
            }
            b'T' => {
                // The same reflection rule as `S`, against the quadratic's one
                // control. Tracked separately because a `T` after a `C` has
                // nothing to reflect even though a cubic control exists.
                let (qx, qy) = match last_quad_ctrl {
                    Some((px, py)) => (2.0 * cur.0 - px, 2.0 * cur.1 - py),
                    None => cur,
                };
                let x = num!() + ox;
                let y = num!() + oy;
                out.push(quad_to_cubic(cur, (qx, qy), (x, y)));
                cur = (x, y);
                last_quad_ctrl = Some((qx, qy));
                last_cubic_ctrl = None;
            }
            b'A' => {
                let rx = num!();
                let ry = num!();
                let rot = num!();
                let large = match sc.flag() {
                    Some(f) => f,
                    None => return out,
                };
                let sweep = match sc.flag() {
                    Some(f) => f,
                    None => return out,
                };
                let x = num!() + ox;
                let y = num!() + oy;
                arc_to_cubics(cur, rx, ry, rot, large, sweep, (x, y), &mut out);
                cur = (x, y);
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
            }
            b'Z' => {
                out.push(PathCmd::Close);
                cur = start;
                last_cubic_ctrl = None;
                last_quad_ctrl = None;
            }
            // A letter that is not a command. Stop, and keep what was read.
            _ => return out,
        }
    }
    out
}

/// An elliptical arc as a chain of cubics.
///
/// This is the endpoint parameterisation from the SVG specification's implementation
/// notes, turned into a centre and a pair of angles, then cut into pieces of at
/// most a quarter turn. A quarter turn is where a cubic's error against a true
/// ellipse is still far below a pixel; a half turn is visibly not a circle.
#[allow(clippy::too_many_arguments)]
fn arc_to_cubics(
    from: (f32, f32),
    rx: f32,
    ry: f32,
    rot_deg: f32,
    large: bool,
    sweep: bool,
    to: (f32, f32),
    out: &mut Vec<PathCmd>,
) {
    // An arc that goes nowhere is not drawn at all, which is the spec's rule
    // and matters: it is not a zero-length line, and emitting one would put a
    // stray cap or join on screen under `stroke-linecap: round`.
    if (from.0 - to.0).abs() < f32::EPSILON && (from.1 - to.1).abs() < f32::EPSILON {
        return;
    }
    // A degenerate radius is a straight line, again by the spec.
    if rx == 0.0 || ry == 0.0 {
        out.push(line_to_cubic(from, to));
        return;
    }
    let (mut rx, mut ry) = (rx.abs() as f64, ry.abs() as f64);
    let phi = (rot_deg as f64).to_radians();
    let (cos_p, sin_p) = (phi.cos(), phi.sin());
    let (x1, y1) = (from.0 as f64, from.1 as f64);
    let (x2, y2) = (to.0 as f64, to.1 as f64);

    let dx = (x1 - x2) / 2.0;
    let dy = (y1 - y2) / 2.0;
    let x1p = cos_p * dx + sin_p * dy;
    let y1p = -sin_p * dx + cos_p * dy;

    // Radii too small to reach: scaled up until they exactly do. Without this
    // the square root below goes imaginary and the arc vanishes, which is a
    // common shape to hand-write by accident.
    let lambda = x1p * x1p / (rx * rx) + y1p * y1p / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }

    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let coef = if large == sweep { -1.0 } else { 1.0 } * (num / den).max(0.0).sqrt();
    let cxp = coef * rx * y1p / ry;
    let cyp = -coef * ry * x1p / rx;
    let cx = cos_p * cxp - sin_p * cyp + (x1 + x2) / 2.0;
    let cy = sin_p * cxp + cos_p * cyp + (y1 + y2) / 2.0;

    let ang = |ux: f64, uy: f64, vx: f64, vy: f64| {
        let dot = ux * vx + uy * vy;
        let len = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt();
        let mut a = (dot / len).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            a = -a;
        }
        a
    };
    let ux = (x1p - cxp) / rx;
    let uy = (y1p - cyp) / ry;
    let vx = (-x1p - cxp) / rx;
    let vy = (-y1p - cyp) / ry;
    let theta1 = ang(1.0, 0.0, ux, uy);
    let mut delta = ang(ux, uy, vx, vy);
    if !sweep && delta > 0.0 {
        delta -= std::f64::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f64::consts::TAU;
    }

    let point = |t: f64| {
        (
            cx + rx * cos_p * t.cos() - ry * sin_p * t.sin(),
            cy + rx * sin_p * t.cos() + ry * cos_p * t.sin(),
        )
    };
    let deriv = |t: f64| {
        (
            -rx * cos_p * t.sin() - ry * sin_p * t.cos(),
            -rx * sin_p * t.sin() + ry * cos_p * t.cos(),
        )
    };

    let segments = (delta.abs() / (std::f64::consts::FRAC_PI_2)).ceil().max(1.0) as usize;
    let step = delta / segments as f64;
    // The classic magic number: the control-point distance that makes a cubic
    // hug a circular arc of this angle. It is exactly 4/3 tan(a/4), and at a
    // quarter turn it is the 0.5523 that shows up in every hand-written circle.
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    for i in 0..segments {
        let a = theta1 + step * i as f64;
        let b = a + step;
        let (px, py) = point(a);
        let (qx, qy) = point(b);
        let (dax, day) = deriv(a);
        let (dbx, dby) = deriv(b);
        out.push(PathCmd::Curve {
            x1: (px + k * dax) as f32,
            y1: (py + k * day) as f32,
            x2: (qx - k * dbx) as f32,
            y2: (qy - k * dby) as f32,
            x: qx as f32,
            y: qy as f32,
        });
    }
}
