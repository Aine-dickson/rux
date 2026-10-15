//! `clip-path`: a basic shape, `inset()`, `circle()`, `ellipse()`,
//! `polygon()` or `path()`, against one of the element's boxes, outside of
//! which nothing of the element or its subtree is drawn.

use crate::path::PathCmd;
use crate::Len;

/// A `clip-path` value.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipPath {
    /// `None` is the reference box itself: `clip-path: padding-box`.
    pub shape: Option<ClipShape>,
    pub reference: RefBox,
}

/// The box a shape's percentages and origin are of. `border-box` unless
/// one is named.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RefBox {
    Margin,
    #[default]
    Border,
    Padding,
    Content,
}

/// A circle's or an ellipse's radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Radius {
    Len(Len),
    ClosestSide,
    FarthestSide,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClipShape {
    /// Top, right, bottom and left, and the corners' radii in CSS order.
    Inset { sides: [Len; 4], round: [Len; 4] },
    Circle { r: Radius, at: (Len, Len) },
    Ellipse { rx: Radius, ry: Radius, at: (Len, Len) },
    Polygon { evenodd: bool, points: Vec<(Len, Len)> },
    /// In pixels from the reference box's corner.
    Path { evenodd: bool, commands: Vec<PathCmd> },
}

impl ClipPath {
    /// Whether the shape fills by the even-odd rule.
    pub fn evenodd(&self) -> bool {
        matches!(self.shape, Some(ClipShape::Polygon { evenodd: true, .. } | ClipShape::Path { evenodd: true, .. }))
    }

    /// The outline to clip to, in absolute coordinates, for a reference box
    /// at `(x, y)` sized `w` by `h`.
    pub fn commands(&self, (x, y, w, h): (f32, f32, f32, f32), vp: (f32, f32)) -> Vec<PathCmd> {
        let px = |l: Len, of: f32| l.px(of, vp);
        match &self.shape {
            None => rounded_rect(x, y, w, h, [0.0; 4]),
            Some(ClipShape::Inset { sides, round }) => {
                let (t, r) = (px(sides[0], h), px(sides[1], w));
                let (b, l) = (px(sides[2], h), px(sides[3], w));
                let (iw, ih) = ((w - l - r).max(0.0), (h - t - b).max(0.0));
                // A corner's percentage is of the inset rectangle's width;
                // a radius larger than the room for it is scaled down, as a
                // border-radius is.
                let mut radii = round.map(|c| px(c, iw).max(0.0));
                let fit = [
                    iw / (radii[0] + radii[1]),
                    ih / (radii[1] + radii[2]),
                    iw / (radii[2] + radii[3]),
                    ih / (radii[3] + radii[0]),
                ]
                .into_iter()
                .filter(|f| f.is_finite())
                .fold(1.0f32, f32::min);
                radii.iter_mut().for_each(|r| *r *= fit);
                rounded_rect(x + l, y + t, iw, ih, radii)
            }
            Some(ClipShape::Circle { r, at }) => {
                let (cx, cy) = (px(at.0, w), px(at.1, h));
                // A percentage of a circle's radius is of the box's diagonal
                // over the square root of two.
                let r = radius(*r, cx, cy, w, h, (w * w + h * h).sqrt() / std::f32::consts::SQRT_2, vp, true);
                ellipse(x + cx, y + cy, r, r)
            }
            Some(ClipShape::Ellipse { rx, ry, at }) => {
                let (cx, cy) = (px(at.0, w), px(at.1, h));
                let rx = radius(*rx, cx, cy, w, h, w, vp, false);
                let ry = radius(*ry, cy, cx, h, w, h, vp, false);
                ellipse(x + cx, y + cy, rx, ry)
            }
            Some(ClipShape::Polygon { points, .. }) => {
                let mut out = Vec::with_capacity(points.len() + 1);
                let mut last = (0.0, 0.0);
                for (i, &(px_, py_)) in points.iter().enumerate() {
                    let p = (x + px(px_, w), y + px(py_, h));
                    if i == 0 {
                        out.push(PathCmd::Move { x: p.0, y: p.1 });
                    } else {
                        out.push(line(last, p));
                    }
                    last = p;
                }
                out.push(PathCmd::Close);
                out
            }
            Some(ClipShape::Path { commands, .. }) => commands
                .iter()
                .map(|c| match *c {
                    PathCmd::Move { x: px_, y: py_ } => PathCmd::Move { x: x + px_, y: y + py_ },
                    PathCmd::Curve { x1, y1, x2, y2, x: ex, y: ey } => {
                        PathCmd::Curve { x1: x + x1, y1: y + y1, x2: x + x2, y2: y + y2, x: x + ex, y: y + ey }
                    }
                    PathCmd::Close => PathCmd::Close,
                })
                .collect(),
        }
    }
}

/// A radius along one axis: `closest-side` and `farthest-side` measure from
/// the centre to the box's edges on that axis, or on both for a circle.
#[allow(clippy::too_many_arguments)]
fn radius(r: Radius, c: f32, other_c: f32, len: f32, other_len: f32, of: f32, vp: (f32, f32), circle: bool) -> f32 {
    let along = [c.abs(), (len - c).abs()];
    let across = [other_c.abs(), (other_len - other_c).abs()];
    // An ellipse's radius measures on its own axis; a circle's on both.
    let sides: Vec<f32> = if circle { along.into_iter().chain(across).collect() } else { along.to_vec() };
    match r {
        Radius::Len(l) => l.px(of, vp).max(0.0),
        Radius::ClosestSide => sides.iter().copied().fold(f32::INFINITY, f32::min),
        Radius::FarthestSide => sides.iter().copied().fold(0.0, f32::max),
    }
}

fn line(a: (f32, f32), b: (f32, f32)) -> PathCmd {
    PathCmd::Curve { x1: a.0, y1: a.1, x2: b.0, y2: b.1, x: b.0, y: b.1 }
}

/// How far a cubic's control points sit along the tangent to draw a
/// quarter circle.
const KAPPA: f32 = 0.552_284_8;

fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<PathCmd> {
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    vec![
        PathCmd::Move { x: cx + rx, y: cy },
        PathCmd::Curve { x1: cx + rx, y1: cy + ky, x2: cx + kx, y2: cy + ry, x: cx, y: cy + ry },
        PathCmd::Curve { x1: cx - kx, y1: cy + ry, x2: cx - rx, y2: cy + ky, x: cx - rx, y: cy },
        PathCmd::Curve { x1: cx - rx, y1: cy - ky, x2: cx - kx, y2: cy - ry, x: cx, y: cy - ry },
        PathCmd::Curve { x1: cx + kx, y1: cy - ry, x2: cx + rx, y2: cy - ky, x: cx + rx, y: cy },
        PathCmd::Close,
    ]
}

/// A rectangle with its corners rounded, top left, top right, bottom right,
/// bottom left.
fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: [f32; 4]) -> Vec<PathCmd> {
    let (x1, y1) = (x + w, y + h);
    let mut out = vec![PathCmd::Move { x: x + r[0], y }];
    out.push(line((x + r[0], y), (x1 - r[1], y)));
    if r[1] > 0.0 {
        out.push(PathCmd::Curve { x1: x1 - r[1] + r[1] * KAPPA, y1: y, x2: x1, y2: y + r[1] - r[1] * KAPPA, x: x1, y: y + r[1] });
    }
    out.push(line((x1, y + r[1]), (x1, y1 - r[2])));
    if r[2] > 0.0 {
        out.push(PathCmd::Curve { x1: x1, y1: y1 - r[2] + r[2] * KAPPA, x2: x1 - r[2] + r[2] * KAPPA, y2: y1, x: x1 - r[2], y: y1 });
    }
    out.push(line((x1 - r[2], y1), (x + r[3], y1)));
    if r[3] > 0.0 {
        out.push(PathCmd::Curve { x1: x + r[3] - r[3] * KAPPA, y1: y1, x2: x, y2: y1 - r[3] + r[3] * KAPPA, x: x, y: y1 - r[3] });
    }
    out.push(line((x, y1 - r[3]), (x, y + r[0])));
    if r[0] > 0.0 {
        out.push(PathCmd::Curve { x1: x, y1: y + r[0] - r[0] * KAPPA, x2: x + r[0] - r[0] * KAPPA, y2: y, x: x + r[0], y });
    }
    out.push(PathCmd::Close);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(cmds: &[PathCmd]) -> (f32, f32, f32, f32) {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for c in cmds {
            if let PathCmd::Move { x, y } | PathCmd::Curve { x, y, .. } = *c {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
        (x0, y0, x1, y1)
    }

    fn clip(shape: ClipShape) -> ClipPath {
        ClipPath { shape: Some(shape), reference: RefBox::Border }
    }

    #[test]
    fn each_shape_lands_where_css_puts_it() {
        let at = (Len::Pct(0.5), Len::Pct(0.5));
        let rect = (10.0, 20.0, 200.0, 100.0);
        let vp = (800.0, 600.0);
        let c = clip(ClipShape::Circle { r: Radius::ClosestSide, at }).commands(rect, vp);
        assert_eq!(bounds(&c), (60.0, 20.0, 160.0, 120.0));
        let c = clip(ClipShape::Circle { r: Radius::FarthestSide, at }).commands(rect, vp);
        assert_eq!(bounds(&c), (10.0, -30.0, 210.0, 170.0));
        let c = clip(ClipShape::Ellipse { rx: Radius::Len(Len::Pct(0.5)), ry: Radius::ClosestSide, at }).commands(rect, vp);
        assert_eq!(bounds(&c), (10.0, 20.0, 210.0, 120.0));
        let c = clip(ClipShape::Inset { sides: [Len::Px(10.0), Len::Pct(0.1), Len::Px(5.0), Len::Px(0.0)], round: [Len::Px(4.0); 4] })
            .commands(rect, vp);
        assert_eq!(bounds(&c), (10.0, 30.0, 190.0, 115.0));
        let c = clip(ClipShape::Polygon {
            evenodd: false,
            points: vec![(Len::Pct(0.5), Len::Px(0.0)), (Len::Pct(1.0), Len::Pct(1.0)), (Len::Px(0.0), Len::Pct(1.0))],
        })
        .commands(rect, vp);
        assert_eq!(bounds(&c), (10.0, 20.0, 210.0, 120.0));
        assert!(matches!(c.last(), Some(PathCmd::Close)));
    }
}
