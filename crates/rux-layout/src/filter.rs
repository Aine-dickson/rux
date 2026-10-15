//! `filter` and `backdrop-filter`: what is done to the pixels of a box and
//! its subtree, or of what is behind it, once they are drawn.
//!
//! The colour functions (`brightness()`, `contrast()`, `grayscale()`,
//! `sepia()`, `saturate()`, `hue-rotate()`, `invert()` and `opacity()`) are
//! each a 4x5 matrix over straight (not premultiplied) sRGB, CSS Filter
//! Effects' own, and a run of them is one matrix. `blur()` and
//! `drop-shadow()` work across pixels, so they are steps of their own.

use crate::Rgba;

/// One step of a filter, in the order written.
#[derive(Clone, Debug)]
pub enum FilterOp {
    /// `blur()`: a Gaussian with this standard deviation, in logical px.
    Blur(f32),
    /// The colour functions, folded: `[r, g, b, a]` out, each a row of
    /// `[r, g, b, a, 1]` in.
    Matrix([f32; 20]),
    /// `drop-shadow()`: the alpha of what is there, offset, blurred and
    /// tinted, drawn under it.
    DropShadow { dx: f32, dy: f32, blur: f32, color: Rgba },
}

/// A box's `filter` and `backdrop-filter`, boxed together on a `Style`
/// (watchlist #41).
#[derive(Clone, Debug, Default)]
pub struct Filters {
    pub filter: Vec<FilterOp>,
    pub backdrop: Vec<FilterOp>,
}

impl FilterOp {
    pub const IDENTITY: [f32; 20] = [
        1.0, 0.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 0.0, 1.0, 0.0,
    ];

    fn rgb(m: [[f32; 3]; 3]) -> [f32; 20] {
        let mut out = Self::IDENTITY;
        for (r, row) in m.iter().enumerate() {
            out[r * 5..r * 5 + 3].copy_from_slice(row);
        }
        out
    }

    pub fn brightness(a: f32) -> [f32; 20] {
        Self::rgb([[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]])
    }

    pub fn contrast(a: f32) -> [f32; 20] {
        let mut m = Self::brightness(a);
        for r in 0..3 {
            m[r * 5 + 4] = 0.5 - 0.5 * a;
        }
        m
    }

    pub fn grayscale(a: f32) -> [f32; 20] {
        let b = 1.0 - a.clamp(0.0, 1.0);
        Self::rgb([
            [0.2126 + 0.7874 * b, 0.7152 - 0.7152 * b, 0.0722 - 0.0722 * b],
            [0.2126 - 0.2126 * b, 0.7152 + 0.2848 * b, 0.0722 - 0.0722 * b],
            [0.2126 - 0.2126 * b, 0.7152 - 0.7152 * b, 0.0722 + 0.9278 * b],
        ])
    }

    pub fn sepia(a: f32) -> [f32; 20] {
        let b = 1.0 - a.clamp(0.0, 1.0);
        Self::rgb([
            [0.393 + 0.607 * b, 0.769 - 0.769 * b, 0.189 - 0.189 * b],
            [0.349 - 0.349 * b, 0.686 + 0.314 * b, 0.168 - 0.168 * b],
            [0.272 - 0.272 * b, 0.534 - 0.534 * b, 0.131 + 0.869 * b],
        ])
    }

    pub fn saturate(s: f32) -> [f32; 20] {
        Self::rgb([
            [0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s],
            [0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s],
            [0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s],
        ])
    }

    /// `hue-rotate()`, the angle in degrees.
    pub fn hue_rotate(deg: f32) -> [f32; 20] {
        let (s, c) = deg.to_radians().sin_cos();
        Self::rgb([
            [0.213 + c * 0.787 - s * 0.213, 0.715 - c * 0.715 - s * 0.715, 0.072 - c * 0.072 + s * 0.928],
            [0.213 - c * 0.213 + s * 0.143, 0.715 + c * 0.285 + s * 0.140, 0.072 - c * 0.072 - s * 0.283],
            [0.213 - c * 0.213 - s * 0.787, 0.715 - c * 0.715 + s * 0.715, 0.072 + c * 0.928 + s * 0.072],
        ])
    }

    pub fn invert(a: f32) -> [f32; 20] {
        let a = a.clamp(0.0, 1.0);
        let mut m = Self::brightness(1.0 - 2.0 * a);
        for r in 0..3 {
            m[r * 5 + 4] = a;
        }
        m
    }

    pub fn opacity(a: f32) -> [f32; 20] {
        let mut m = Self::IDENTITY;
        m[18] = a.clamp(0.0, 1.0);
        m
    }

    /// `b` applied after `a`.
    pub fn then(a: [f32; 20], b: [f32; 20]) -> [f32; 20] {
        let mut out = [0.0; 20];
        for r in 0..4 {
            for c in 0..5 {
                let mut v = (0..4).map(|k| b[r * 5 + k] * a[k * 5 + c]).sum::<f32>();
                if c == 4 {
                    v += b[r * 5 + 4];
                }
                out[r * 5 + c] = v;
            }
        }
        out
    }

    /// A colour through a matrix, as the GPU pass does it.
    pub fn apply(m: &[f32; 20], c: Rgba) -> Rgba {
        let v = [c.r, c.g, c.b, c.a];
        let row = |r: usize| (0..4).map(|k| m[r * 5 + k] * v[k]).sum::<f32>() + m[r * 5 + 4];
        Rgba::new(row(0).clamp(0.0, 1.0), row(1).clamp(0.0, 1.0), row(2).clamp(0.0, 1.0), row(3).clamp(0.0, 1.0))
    }
}

/// A list of steps with each run of colour functions folded into one matrix.
pub fn fold(ops: Vec<FilterOp>) -> Vec<FilterOp> {
    let mut out: Vec<FilterOp> = Vec::with_capacity(ops.len());
    for op in ops {
        match (out.last_mut(), op) {
            (Some(FilterOp::Matrix(a)), FilterOp::Matrix(b)) => *a = FilterOp::then(*a, b),
            (_, op) => out.push(op),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(c: Rgba) -> [u8; 4] {
        let b = |v: f32| (v * 255.0).round() as u8;
        [b(c.r), b(c.g), b(c.b), b(c.a)]
    }

    #[test]
    fn each_colour_function_does_what_css_says() {
        let c = Rgba::new(1.0, 0.5, 0.0, 1.0);
        assert_eq!(bytes(FilterOp::apply(&FilterOp::invert(1.0), c)), [0, 128, 255, 255]);
        assert_eq!(bytes(FilterOp::apply(&FilterOp::brightness(0.5), c)), [128, 64, 0, 255]);
        assert_eq!(bytes(FilterOp::apply(&FilterOp::opacity(0.5), c)), [255, 128, 0, 128]);
        let g = FilterOp::apply(&FilterOp::grayscale(1.0), c);
        assert!((g.r - g.g).abs() < 1e-6 && (g.g - g.b).abs() < 1e-6);
        assert_eq!(bytes(FilterOp::apply(&FilterOp::contrast(0.0), c)), [128, 128, 128, 255]);
        assert_eq!(bytes(FilterOp::apply(&FilterOp::hue_rotate(0.0), c)), bytes(c));
        assert_eq!(bytes(FilterOp::apply(&FilterOp::saturate(1.0), c)), bytes(c));
    }

    #[test]
    fn a_run_of_colour_functions_is_one_matrix_in_order() {
        let ops = fold(vec![
            FilterOp::Matrix(FilterOp::invert(1.0)),
            FilterOp::Matrix(FilterOp::brightness(0.5)),
            FilterOp::Blur(2.0),
            FilterOp::Matrix(FilterOp::opacity(0.5)),
        ]);
        assert_eq!(ops.len(), 3);
        let FilterOp::Matrix(m) = ops[0].clone() else { panic!() };
        // Invert, then halve: white goes to black and stays black; black to
        // white, then grey.
        assert_eq!(bytes(FilterOp::apply(&m, Rgba::new(0.0, 0.0, 0.0, 1.0))), [128, 128, 128, 255]);
    }
}
