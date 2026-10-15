//! The CSS colour functions: `rgb()`, `hsl()`, `hwb()`, `lab()`, `lch()`,
//! `oklab()`, `oklch()`, `color()` and `color-mix()`, each to straight sRGB.
//!
//! lightningcss already folds most of these to hex when they are written out
//! in a stylesheet. What reaches here is what it cannot fold: the wide-gamut
//! spaces it leaves for the browser, and any colour that only exists once a
//! `var()` has been substituted or a `:style` evaluated.
//!
//! A colour outside sRGB is brought into it the way CSS Color 4 says a
//! browser does (its "gamut mapping"): chroma is reduced in OKLCh, keeping
//! lightness and hue, until clipping what is left changes it by less than a
//! just-noticeable difference. Clipping each channel straight off would turn
//! a vivid `oklch()` orange visibly yellow.
//!
//! The conversions and their matrices are CSS Color 4's own sample code.

use rux_layout::Rgba;

type V3 = [f64; 3];
type M3 = [V3; 3];

fn mul(m: &M3, v: V3) -> V3 {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

const SRGB_TO_XYZ: M3 = [
    [0.41239079926595934, 0.357584339383878, 0.1804807884018343],
    [0.21263900587151027, 0.715168678767756, 0.07219231536073371],
    [0.01933081871559182, 0.11919477979462598, 0.9505321522496607],
];
const XYZ_TO_SRGB: M3 = [
    [3.2409699419045226, -1.537383177570094, -0.4986107602930034],
    [-0.9692436362808796, 1.8759675015077202, 0.04155505740717559],
    [0.05563007969699366, -0.20397695888897652, 1.0569715142428786],
];
const P3_TO_XYZ: M3 = [
    [0.4865709486482162, 0.26566769316909306, 0.1982172852343625],
    [0.2289745640697488, 0.6917385218365064, 0.079286914093745],
    [0.0, 0.04511338185890264, 1.043944368900976],
];
const A98_TO_XYZ: M3 = [
    [0.5766690429101305, 0.1855582379065463, 0.1882286462349947],
    [0.29734497525053605, 0.6273635662554661, 0.0752914584939978],
    [0.02703136138641234, 0.07068885253582723, 0.9913375368376388],
];
const REC2020_TO_XYZ: M3 = [
    [0.6369580483012914, 0.14461690358620832, 0.1688809751641721],
    [0.2627002120112671, 0.6779980715188708, 0.05930171646986196],
    [0.0, 0.028072693049087428, 1.060985057710791],
];
/// ProPhoto is defined against D50, so this lands in XYZ-D50.
const PROPHOTO_TO_XYZ_D50: M3 = [
    [0.7977666449006423, 0.13518129740053308, 0.0313477341283922],
    [0.2880748288194013, 0.711835234241873, 0.00008993693872564],
    [0.0, 0.0, 0.8251046025104602],
];
const D50_TO_D65: M3 = [
    [0.955473421488075, -0.02309845494876471, 0.06325924320057072],
    [-0.0283697093338637, 1.0099953980813041, 0.021041441191917323],
    [0.012314014864481998, -0.020507649298898964, 1.330365926242124],
];
const D65_TO_D50: M3 = [
    [1.0479297925449969, 0.022946870601609652, -0.05019226628920524],
    [0.02962780877005599, 0.9904344267538799, -0.017073799063418826],
    [-0.009243040646204504, 0.015055191490298152, 0.7518742814281371],
];
const XYZ_TO_LMS: M3 = [
    [0.819022437996703, 0.3619062600528904, -0.1288737815209879],
    [0.0329836539323885, 0.9292868615863434, 0.0361446663506424],
    [0.0481771893596242, 0.2642395317527308, 0.6335478284694309],
];
const LMS_TO_OKLAB: M3 = [
    [0.210454268309314, 0.7936177747023054, -0.0040720430116193],
    [1.9779985324311684, -2.42859224204858, 0.450593709617411],
    [0.0259040424655478, 0.7827717124575296, -0.8086757549230774],
];
const OKLAB_TO_LMS: M3 = [
    [1.0, 0.3963377773761749, 0.2158037573099136],
    [1.0, -0.1055613458156586, -0.0638541728258133],
    [1.0, -0.0894841775298119, -1.2914855480194092],
];
const LMS_TO_XYZ: M3 = [
    [1.2268798758459243, -0.5578149944602171, 0.2813910456659647],
    [-0.0405757452148008, 1.112286803280317, -0.0717110580655164],
    [-0.0763729366746601, -0.4214933324022432, 1.5869240198367816],
];
/// The D50 white point, which `lab()` and `lch()` are relative to.
const D50_WHITE: V3 = [0.3457 / 0.3585, 1.0, (1.0 - 0.3457 - 0.3585) / 0.3585];

fn srgb_to_linear(c: f64) -> f64 {
    let a = c.abs();
    if a <= 0.04045 {
        c / 12.92
    } else {
        c.signum() * ((a + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f64) -> f64 {
    let a = c.abs();
    if a > 0.0031308 {
        c.signum() * (1.055 * a.powf(1.0 / 2.4) - 0.055)
    } else {
        12.92 * c
    }
}

fn a98_to_linear(c: f64) -> f64 {
    c.signum() * c.abs().powf(563.0 / 256.0)
}

fn linear_to_a98(c: f64) -> f64 {
    c.signum() * c.abs().powf(256.0 / 563.0)
}

fn rec2020_to_linear(c: f64) -> f64 {
    const ALPHA: f64 = 1.09929682680944;
    const BETA: f64 = 0.018053968510807;
    let a = c.abs();
    if a < BETA * 4.5 {
        c / 4.5
    } else {
        c.signum() * ((a + ALPHA - 1.0) / ALPHA).powf(1.0 / 0.45)
    }
}

fn linear_to_rec2020(c: f64) -> f64 {
    const ALPHA: f64 = 1.09929682680944;
    const BETA: f64 = 0.018053968510807;
    let a = c.abs();
    if a > BETA {
        c.signum() * (ALPHA * a.powf(0.45) - (ALPHA - 1.0))
    } else {
        4.5 * c
    }
}

fn prophoto_to_linear(c: f64) -> f64 {
    let a = c.abs();
    if a <= 16.0 / 512.0 {
        c / 16.0
    } else {
        c.signum() * a.powf(1.8)
    }
}

fn linear_to_prophoto(c: f64) -> f64 {
    let a = c.abs();
    if a >= 1.0 / 512.0 {
        c.signum() * a.powf(1.0 / 1.8)
    } else {
        16.0 * c
    }
}

fn map3(v: V3, f: fn(f64) -> f64) -> V3 {
    [f(v[0]), f(v[1]), f(v[2])]
}

const KAPPA: f64 = 24389.0 / 27.0;
const EPSILON: f64 = 216.0 / 24389.0;

fn lab_to_xyz_d50([l, a, b]: V3) -> V3 {
    let f1 = (l + 16.0) / 116.0;
    let f0 = a / 500.0 + f1;
    let f2 = f1 - b / 200.0;
    let x = if f0.powi(3) > EPSILON { f0.powi(3) } else { (116.0 * f0 - 16.0) / KAPPA };
    let y = if l > KAPPA * EPSILON { f1.powi(3) } else { l / KAPPA };
    let z = if f2.powi(3) > EPSILON { f2.powi(3) } else { (116.0 * f2 - 16.0) / KAPPA };
    [x * D50_WHITE[0], y * D50_WHITE[1], z * D50_WHITE[2]]
}

fn xyz_d50_to_lab(xyz: V3) -> V3 {
    let f = |i: usize| {
        let v = xyz[i] / D50_WHITE[i];
        if v > EPSILON { v.cbrt() } else { (KAPPA * v + 16.0) / 116.0 }
    };
    let (f0, f1, f2) = (f(0), f(1), f(2));
    [116.0 * f1 - 16.0, 500.0 * (f0 - f1), 200.0 * (f1 - f2)]
}

fn oklab_to_xyz(lab: V3) -> V3 {
    mul(&LMS_TO_XYZ, map3(mul(&OKLAB_TO_LMS, lab), |c| c * c * c))
}

fn xyz_to_oklab(xyz: V3) -> V3 {
    mul(&LMS_TO_OKLAB, map3(mul(&XYZ_TO_LMS, xyz), f64::cbrt))
}

/// Polar to rectangular: `lch` to `lab`, `oklch` to `oklab`.
fn from_polar([l, c, h]: V3) -> V3 {
    let r = h.to_radians();
    [l, c * r.cos(), c * r.sin()]
}

/// Rectangular to polar, with the hue in `0..360`.
fn to_polar([l, a, b]: V3) -> V3 {
    let h = b.atan2(a).to_degrees();
    [l, (a * a + b * b).sqrt(), if h < 0.0 { h + 360.0 } else { h }]
}

fn hsl_to_srgb([h, s, l]: V3) -> V3 {
    let h = h.rem_euclid(360.0);
    let f = |n: f64| {
        let k = (n + h / 30.0) % 12.0;
        let a = s * l.min(1.0 - l);
        l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    [f(0.0), f(8.0), f(4.0)]
}

fn srgb_to_hsl([r, g, b]: V3) -> V3 {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (min + max) / 2.0;
    let d = max - min;
    let (mut h, mut s) = (0.0, 0.0);
    if d != 0.0 {
        s = if l == 0.0 || l == 1.0 { 0.0 } else { (max - l) / l.min(1.0 - l) };
        h = if max == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        h *= 60.0;
    }
    [h, s, l]
}

fn hwb_to_srgb([h, w, b]: V3) -> V3 {
    if w + b >= 1.0 {
        let gray = w / (w + b);
        return [gray; 3];
    }
    map3_with(hsl_to_srgb([h, 1.0, 0.5]), |c| c * (1.0 - w - b) + w)
}

fn srgb_to_hwb(rgb: V3) -> V3 {
    let h = srgb_to_hsl(rgb)[0];
    let w = rgb[0].min(rgb[1]).min(rgb[2]);
    let b = 1.0 - rgb[0].max(rgb[1]).max(rgb[2]);
    [h, w, b]
}

fn map3_with(v: V3, f: impl Fn(f64) -> f64) -> V3 {
    [f(v[0]), f(v[1]), f(v[2])]
}

/// A colour space the functions here are written in, and the one
/// `color-mix()` interpolates in.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Space {
    Srgb,
    SrgbLinear,
    DisplayP3,
    A98,
    ProPhoto,
    Rec2020,
    XyzD50,
    XyzD65,
    Lab,
    Lch,
    Oklab,
    Oklch,
    Hsl,
    Hwb,
}

impl Space {
    fn named(name: &str) -> Option<Space> {
        Some(match name {
            "srgb" => Space::Srgb,
            "srgb-linear" => Space::SrgbLinear,
            "display-p3" => Space::DisplayP3,
            "a98-rgb" => Space::A98,
            "prophoto-rgb" => Space::ProPhoto,
            "rec2020" => Space::Rec2020,
            "xyz" | "xyz-d65" => Space::XyzD65,
            "xyz-d50" => Space::XyzD50,
            "lab" => Space::Lab,
            "lch" => Space::Lch,
            "oklab" => Space::Oklab,
            "oklch" => Space::Oklch,
            "hsl" => Space::Hsl,
            "hwb" => Space::Hwb,
            _ => return None,
        })
    }

    /// Which component is a hue, if any.
    fn hue(self) -> Option<usize> {
        match self {
            Space::Hsl | Space::Hwb => Some(0),
            Space::Lch | Space::Oklch => Some(2),
            _ => None,
        }
    }

    fn to_xyz(self, c: V3) -> V3 {
        match self {
            Space::Srgb => mul(&SRGB_TO_XYZ, map3(c, srgb_to_linear)),
            Space::SrgbLinear => mul(&SRGB_TO_XYZ, c),
            Space::DisplayP3 => mul(&P3_TO_XYZ, map3(c, srgb_to_linear)),
            Space::A98 => mul(&A98_TO_XYZ, map3(c, a98_to_linear)),
            Space::ProPhoto => mul(&D50_TO_D65, mul(&PROPHOTO_TO_XYZ_D50, map3(c, prophoto_to_linear))),
            Space::Rec2020 => mul(&REC2020_TO_XYZ, map3(c, rec2020_to_linear)),
            Space::XyzD50 => mul(&D50_TO_D65, c),
            Space::XyzD65 => c,
            Space::Lab => mul(&D50_TO_D65, lab_to_xyz_d50(c)),
            Space::Lch => mul(&D50_TO_D65, lab_to_xyz_d50(from_polar(c))),
            Space::Oklab => oklab_to_xyz(c),
            Space::Oklch => oklab_to_xyz(from_polar(c)),
            Space::Hsl => Space::Srgb.to_xyz(hsl_to_srgb(c)),
            Space::Hwb => Space::Srgb.to_xyz(hwb_to_srgb(c)),
        }
    }

    fn from_xyz(self, xyz: V3) -> V3 {
        let inv = |m: &M3| invert(m);
        match self {
            Space::Srgb => map3(mul(&XYZ_TO_SRGB, xyz), linear_to_srgb),
            Space::SrgbLinear => mul(&XYZ_TO_SRGB, xyz),
            Space::DisplayP3 => map3(mul(&inv(&P3_TO_XYZ), xyz), linear_to_srgb),
            Space::A98 => map3(mul(&inv(&A98_TO_XYZ), xyz), linear_to_a98),
            Space::ProPhoto => map3(mul(&inv(&PROPHOTO_TO_XYZ_D50), mul(&D65_TO_D50, xyz)), linear_to_prophoto),
            Space::Rec2020 => map3(mul(&inv(&REC2020_TO_XYZ), xyz), linear_to_rec2020),
            Space::XyzD50 => mul(&D65_TO_D50, xyz),
            Space::XyzD65 => xyz,
            Space::Lab => xyz_d50_to_lab(mul(&D65_TO_D50, xyz)),
            Space::Lch => to_polar(xyz_d50_to_lab(mul(&D65_TO_D50, xyz))),
            Space::Oklab => xyz_to_oklab(xyz),
            Space::Oklch => to_polar(xyz_to_oklab(xyz)),
            Space::Hsl => srgb_to_hsl(Space::Srgb.from_xyz(xyz)),
            Space::Hwb => srgb_to_hwb(Space::Srgb.from_xyz(xyz)),
        }
    }
}

fn invert(m: &M3) -> M3 {
    let [[a, b, c], [d, e, f], [g, h, i]] = *m;
    let co = [e * i - f * h, -(d * i - f * g), d * h - e * g];
    let det = a * co[0] + b * co[1] + c * co[2];
    [
        [co[0] / det, -(b * i - c * h) / det, (b * f - c * e) / det],
        [co[1] / det, (a * i - c * g) / det, -(a * f - c * d) / det],
        [co[2] / det, -(a * h - b * g) / det, (a * e - b * d) / det],
    ]
}

/// A colour on its way to sRGB: its components in `space`, and its alpha.
#[derive(Clone, Copy, Debug)]
struct Color {
    space: Space,
    c: V3,
    alpha: f64,
}

impl Color {
    fn to(self, space: Space) -> Color {
        if self.space == space {
            return self;
        }
        Color { space, c: space.from_xyz(self.space.to_xyz(self.c)), alpha: self.alpha }
    }

    /// Into sRGB, gamut-mapped (CSS Color 4, section 13.2).
    fn to_rgba(self) -> Rgba {
        let rgb = gamut_map(self);
        let ch = |v: f64| v.clamp(0.0, 1.0) as f32;
        Rgba::new(ch(rgb[0]), ch(rgb[1]), ch(rgb[2]), self.alpha.clamp(0.0, 1.0) as f32)
    }
}

fn in_srgb(rgb: V3) -> bool {
    const E: f64 = 0.000_001;
    rgb.iter().all(|c| (-E..=1.0 + E).contains(c))
}

fn delta_eok(a: V3, b: V3) -> f64 {
    let (x, y) = (Space::Oklab.from_xyz(Space::Srgb.to_xyz(a)), Space::Oklab.from_xyz(Space::Srgb.to_xyz(b)));
    ((x[0] - y[0]).powi(2) + (x[1] - y[1]).powi(2) + (x[2] - y[2]).powi(2)).sqrt()
}

fn gamut_map(color: Color) -> V3 {
    let rgb = color.to(Space::Srgb).c;
    if in_srgb(rgb) {
        return rgb;
    }
    let lch = color.to(Space::Oklch).c;
    if lch[0] >= 1.0 {
        return [1.0; 3];
    }
    if lch[0] <= 0.0 {
        return [0.0; 3];
    }
    const JND: f64 = 0.02;
    const EPS: f64 = 0.0001;
    let clip = |c: V3| map3(c, |v| v.clamp(0.0, 1.0));
    let srgb_of = |chroma: f64| Space::Srgb.from_xyz(Space::Oklch.to_xyz([lch[0], chroma, lch[2]]));
    let (mut min, mut max) = (0.0, lch[1]);
    let mut min_in_gamut = true;
    let current = srgb_of(max);
    let mut clipped = clip(current);
    if delta_eok(clipped, current) < JND {
        return clipped;
    }
    while max - min > EPS {
        let chroma = (min + max) / 2.0;
        let current = srgb_of(chroma);
        if min_in_gamut && in_srgb(current) {
            min = chroma;
            continue;
        }
        clipped = clip(current);
        let e = delta_eok(clipped, current);
        if e < JND {
            if JND - e < EPS {
                return clipped;
            }
            min_in_gamut = false;
            min = chroma;
        } else {
            max = chroma;
        }
    }
    clipped
}

/// One argument: a number, a percentage, an angle, or `none`.
#[derive(Clone, Copy, Debug)]
enum Arg {
    Num(f64),
    Pct(f64),
    /// In degrees.
    Deg(f64),
    None,
}

impl Arg {
    fn parse(s: &str) -> Option<Arg> {
        let s = s.trim().to_ascii_lowercase();
        if s == "none" {
            return Some(Arg::None);
        }
        if let Some(p) = s.strip_suffix('%') {
            return p.parse().ok().map(Arg::Pct);
        }
        for (unit, per) in [("deg", 1.0), ("grad", 0.9), ("rad", 180.0 / std::f64::consts::PI), ("turn", 360.0)] {
            if let Some(n) = s.strip_suffix(unit) {
                return n.parse::<f64>().ok().map(|v| Arg::Deg(v * per));
            }
        }
        s.parse().ok().map(Arg::Num)
    }

    /// A number, with `100%` worth `full`.
    fn num(self, full: f64) -> f64 {
        match self {
            Arg::Num(n) | Arg::Deg(n) => n,
            Arg::Pct(p) => p / 100.0 * full,
            Arg::None => 0.0,
        }
    }

    /// A hue in degrees; a percentage is not one.
    fn hue(self) -> Option<f64> {
        match self {
            Arg::Num(n) | Arg::Deg(n) => Some(n),
            Arg::None => Some(0.0),
            Arg::Pct(_) => None,
        }
    }

    fn alpha(self) -> f64 {
        self.num(1.0)
    }
}

/// A function's arguments, in the modern form (`a b c / alpha`) or the legacy
/// one (`a, b, c, alpha`): three components and the alpha, if written.
fn components(args: &str) -> Option<([Arg; 3], Option<Arg>)> {
    let parts: Vec<&str> = if args.contains(',') {
        args.split(',').map(str::trim).collect()
    } else {
        let (main, alpha) = match args.split_once('/') {
            Some((m, a)) => (m, Some(a.trim())),
            None => (args, None),
        };
        let mut p: Vec<&str> = main.split_whitespace().collect();
        if let Some(a) = alpha {
            if p.len() != 3 {
                return None;
            }
            p.push(a);
        }
        p
    };
    if !(3..=4).contains(&parts.len()) {
        return None;
    }
    let a = |i: usize| Arg::parse(parts[i]);
    let alpha = match parts.get(3) {
        Some(s) => Some(Arg::parse(s)?),
        None => None,
    };
    Some(([a(0)?, a(1)?, a(2)?], alpha))
}

/// Splits on commas outside brackets.
pub(crate) fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c == sep && depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// A colour function, `name(args)`, to sRGB. `None` when it is not one or
/// its arguments are not a colour. `parse` reads a nested colour, as
/// `color-mix()` takes them.
pub(crate) fn function(name: &str, args: &str, parse: &dyn Fn(&str) -> Option<Rgba>) -> Option<Rgba> {
    let name = name.trim().to_ascii_lowercase();
    if name == "color-mix" {
        return mix(args, parse).map(Color::to_rgba);
    }
    color_function(&name, args).map(Color::to_rgba)
}

fn color_function(name: &str, args: &str) -> Option<Color> {
    let alpha = |a: Option<Arg>| a.map_or(1.0, Arg::alpha);
    let make = |space, c, a| Some(Color { space, c, alpha: alpha(a) });
    if name == "color" {
        let (space, rest) = args.trim().split_once(char::is_whitespace)?;
        let space = Space::named(&space.to_ascii_lowercase())?;
        if space.hue().is_some() || matches!(space, Space::Lab | Space::Oklab) {
            return None;
        }
        let ([x, y, z], a) = components(rest)?;
        return make(space, [x.num(1.0), y.num(1.0), z.num(1.0)], a);
    }
    let ([x, y, z], a) = components(args)?;
    match name {
        "rgb" | "rgba" => make(Space::Srgb, [x.num(255.0) / 255.0, y.num(255.0) / 255.0, z.num(255.0) / 255.0], a),
        // Saturation and lightness are percentages; the modern syntax also
        // takes them as plain numbers out of 100.
        "hsl" | "hsla" => make(Space::Hsl, [x.hue()?, y.num(100.0) / 100.0, z.num(100.0) / 100.0], a),
        "hwb" => make(Space::Hwb, [x.hue()?, y.num(100.0) / 100.0, z.num(100.0) / 100.0], a),
        // 100% is 100 for `L`, 125 for `a` and `b`, and 150 for chroma.
        "lab" => make(Space::Lab, [x.num(100.0).clamp(0.0, 100.0), y.num(125.0), z.num(125.0)], a),
        "lch" => make(Space::Lch, [x.num(100.0).clamp(0.0, 100.0), y.num(150.0).max(0.0), z.hue()?], a),
        // And for OKLab, 1 for `L` and 0.4 for the rest.
        "oklab" => make(Space::Oklab, [x.num(1.0).clamp(0.0, 1.0), y.num(0.4), z.num(0.4)], a),
        "oklch" => make(Space::Oklch, [x.num(1.0).clamp(0.0, 1.0), y.num(0.4).max(0.0), z.hue()?], a),
        _ => None,
    }
}

/// `color-mix(in <space> [<hue> hue], <color> [<p>], <color> [<p>])`.
fn mix(args: &str, parse: &dyn Fn(&str) -> Option<Rgba>) -> Option<Color> {
    let parts = split_top(args, ',');
    let [how, one, two] = parts[..] else { return None };
    let mut words = how.split_whitespace().map(str::to_ascii_lowercase);
    if words.next()? != "in" {
        return None;
    }
    let space = Space::named(&words.next()?)?;
    let method = match (words.next(), words.next()) {
        (None, _) => "shorter".to_string(),
        (Some(m), Some(h)) if h == "hue" && space.hue().is_some() => m,
        _ => return None,
    };
    // A stop is a colour and an optional percentage, in either order.
    let stop = |s: &str| -> Option<(Color, Option<f64>)> {
        let s = s.trim();
        let (color, pct) = match s.rsplit_once(char::is_whitespace) {
            Some((c, p)) if p.ends_with('%') && !c.trim().is_empty() => (c, Some(p)),
            _ => match s.split_once(char::is_whitespace) {
                Some((p, c)) if p.ends_with('%') => (c, Some(p)),
                _ => (s, None),
            },
        };
        let pct = match pct {
            Some(p) => Some(p.trim_end_matches('%').parse::<f64>().ok()? / 100.0),
            None => None,
        };
        let rgba = parse(color.trim())?;
        let c = [rgba.r as f64, rgba.g as f64, rgba.b as f64];
        Some((Color { space: Space::Srgb, c, alpha: rgba.a as f64 }, pct))
    };
    let (a, pa) = stop(one)?;
    let (b, pb) = stop(two)?;
    let (pa, pb) = match (pa, pb) {
        (None, None) => (0.5, 0.5),
        (Some(p), None) => (p, 1.0 - p),
        (None, Some(p)) => (1.0 - p, p),
        (Some(p), Some(q)) => (p, q),
    };
    let sum = pa + pb;
    if sum <= 0.0 {
        return None;
    }
    // Over 100% is scaled down; under it leaves the result that much
    // transparent.
    let (t, multiplier) = (pb / sum, sum.min(1.0));
    let (a, b) = (a.to(space), b.to(space));
    let mut ca = a.c;
    let mut cb = b.c;
    if let Some(h) = space.hue() {
        let (x, y) = adjust_hues(ca[h], cb[h], &method)?;
        ca[h] = x;
        cb[h] = y;
    }
    // Interpolated premultiplied, so a transparent end does not grey the mix.
    let alpha = a.alpha * (1.0 - t) + b.alpha * t;
    let mut c = [0.0; 3];
    for i in 0..3 {
        if Some(i) == space.hue() {
            c[i] = ca[i] * (1.0 - t) + cb[i] * t;
        } else {
            let v = ca[i] * a.alpha * (1.0 - t) + cb[i] * b.alpha * t;
            c[i] = if alpha == 0.0 { 0.0 } else { v / alpha };
        }
    }
    Some(Color { space, c, alpha: alpha * multiplier })
}

/// The two hues to interpolate between, by CSS's hue interpolation method.
fn adjust_hues(a: f64, b: f64, method: &str) -> Option<(f64, f64)> {
    let (mut a, mut b) = (a.rem_euclid(360.0), b.rem_euclid(360.0));
    match method {
        "shorter" => {
            if b - a > 180.0 {
                a += 360.0;
            } else if b - a < -180.0 {
                b += 360.0;
            }
        }
        "longer" => {
            if 0.0 < b - a && b - a < 180.0 {
                a += 360.0;
            } else if -180.0 < b - a && b - a <= 0.0 {
                b += 360.0;
            }
        }
        "increasing" => {
            if b < a {
                b += 360.0;
            }
        }
        "decreasing" => {
            if a < b {
                a += 360.0;
            }
        }
        _ => return None,
    }
    Some((a, b))
}

/// `#rrggbbaa`, as the cascade writes a resolved colour back into a value.
pub(crate) fn hex(c: Rgba) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b), b(c.a))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(name: &str, args: &str) -> [u8; 4] {
        let c = function(name, args, &|_| None).unwrap_or_else(|| panic!("{name}({args}) did not parse"));
        let b = |v: f32| (v * 255.0).round() as u8;
        [b(c.r), b(c.g), b(c.b), b(c.a)]
    }

    #[test]
    fn the_legacy_and_modern_forms_agree() {
        assert_eq!(rgb("hsl", "120, 50%, 50%"), [64, 191, 64, 255]);
        assert_eq!(rgb("hsl", "120deg 50% 50% / .5"), [64, 191, 64, 128]);
        assert_eq!(rgb("hsla", "120, 50%, 50%, 0.5"), [64, 191, 64, 128]);
        assert_eq!(rgb("rgb", "0 0 255 / 50%"), [0, 0, 255, 128]);
        assert_eq!(rgb("rgb", "100%, 0%, 0%"), [255, 0, 0, 255]);
        assert_eq!(rgb("hwb", "120 10% 20%"), [26, 204, 26, 255]);
    }

    /// Against what a browser draws for the same function (and lightningcss
    /// prints for the ones it folds).
    #[test]
    fn the_wide_spaces_land_where_a_browser_puts_them() {
        assert_eq!(rgb("oklab", "0.627955 0.224863 0.125846"), [255, 0, 0, 255]);
        assert_eq!(rgb("oklch", "0.627955 0.257683 29.2339"), [255, 0, 0, 255]);
        assert_eq!(rgb("lab", "54.2905 80.8049 69.891"), [255, 0, 0, 255]);
        assert_eq!(rgb("lch", "54.2905 106.837 40.8526"), [255, 0, 0, 255]);
        assert_eq!(rgb("color", "srgb 0 0.5 1"), [0, 128, 255, 255]);
        assert_eq!(rgb("color", "xyz 0.9505 1 1.089"), [255, 255, 255, 255]);
        assert_eq!(rgb("oklch", "100% 0 0"), [255, 255, 255, 255]);
        assert_eq!(rgb("oklch", "50% none none"), rgb("oklch", "50% 0 0"));
    }

    /// Out of gamut, chroma gives way and the hue holds: P3's pure red is
    /// mapped to a red, not clipped to an orange.
    #[test]
    fn a_colour_outside_srgb_keeps_its_hue() {
        let [r, g, b, _] = rgb("color", "display-p3 1 0 0");
        assert!(r == 255 && g < 20 && b < 20, "{r} {g} {b}");
        // A vivid orange stays orange rather than turning yellow.
        let [r, g, _, _] = rgb("oklch", "70% 0.3 50");
        assert!(r == 255 && (90..150).contains(&g), "{r} {g}");
    }

    #[test]
    fn color_mix_interpolates_in_the_space_it_names() {
        let named = |s: &str| match s {
            "red" => Some(Rgba::new(1.0, 0.0, 0.0, 1.0)),
            "blue" => Some(Rgba::new(0.0, 0.0, 1.0, 1.0)),
            "transparent" => Some(Rgba::new(0.0, 0.0, 0.0, 0.0)),
            _ => None,
        };
        let mixed = |args: &str| {
            let c = function("color-mix", args, &named).unwrap();
            let b = |v: f32| (v * 255.0).round() as u8;
            [b(c.r), b(c.g), b(c.b), b(c.a)]
        };
        // lightningcss's own answer for this one.
        assert_eq!(mixed("in srgb, red 30%, blue"), [77, 0, 179, 255]);
        assert_eq!(mixed("in srgb, red, 70% blue"), [77, 0, 179, 255]);
        // Under 100% in all is that much transparent.
        assert_eq!(mixed("in srgb, red 25%, blue 25%")[3], 128);
        // Premultiplied: mixing with transparent fades, it does not darken.
        assert_eq!(mixed("in srgb, red, transparent"), [255, 0, 0, 128]);
        // A hue space goes round the wheel, not through grey.
        let [r, g, b, _] = mixed("in oklch, red, blue");
        assert!(r > 150 && b > 150 && g < 60, "{r} {g} {b}");
        assert!(function("color-mix", "in nowhere, red, blue", &named).is_none());
    }

    #[test]
    fn hex_rounds_each_channel() {
        assert_eq!(hex(Rgba::new(1.0, 0.5, 0.0, 1.0)), "#ff8000ff");
    }
}
