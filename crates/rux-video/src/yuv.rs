//! A decoder's YUV picture as RGBA: Android's `MediaCodec` hands its
//! pictures out this way, in a buffer. Apart from the platform so it is
//! tested where Rux is built.

use crate::VideoError;

/// `COLOR_FormatYUV420Planar`: I420, a Y plane then a U and a V plane.
pub(crate) const COLOR_I420: i32 = 19;
/// `COLOR_FormatYUV420SemiPlanar`: NV12, a Y plane then U and V interleaved.
pub(crate) const COLOR_NV12: i32 = 21;
/// `COLOR_FormatYUV420Flexible`, which in a buffer is laid out as NV12.
pub(crate) const COLOR_FLEXIBLE: i32 = 0x7F42_0888;

/// How a decoder lays out a picture in its buffer.
#[derive(Clone, Copy, Default)]
pub(crate) struct Layout {
    pub color: i32,
    pub stride: usize,
    pub rows: usize,
    pub left: usize,
    pub top: usize,
    pub width: usize,
    pub height: usize,
    /// BT.709 rather than BT.601, and full range rather than limited.
    pub hd: bool,
    pub full: bool,
}

impl Layout {
    /// The shown picture of one buffer as RGBA, top row first.
    pub fn rgba(&self, data: &[u8]) -> Result<Vec<u8>, VideoError> {
        let semi = match self.color {
            COLOR_I420 => false,
            COLOR_NV12 | COLOR_FLEXIBLE => true,
            other => {
                return Err(VideoError::new(
                    "unsupported",
                    format!("this phone's decoder hands out pictures in a layout Rux does not read yet ({other:#x})"),
                ))
            }
        };
        let (s, rows) = (self.stride, self.rows);
        let luma = s * rows;
        // I420's V plane follows its U plane; NV12 interleaves U and V.
        let v_plane = luma + (s / 2) * (rows / 2);
        if data.len() < luma {
            return Err(VideoError::new("decode", "a picture came out smaller than its size"));
        }
        // A buffer cut short of its last chroma rows reads them as grey.
        let at = |i: usize| data.get(i).copied().unwrap_or(128);
        // Y scale, then V to red, U and V to green, U to blue, all times 256.
        let (ys, rv, gu, gv, bu) = match (self.hd, self.full) {
            (false, false) => (298, 409, 100, 208, 516),
            (true, false) => (298, 459, 55, 136, 541),
            (false, true) => (256, 359, 88, 183, 454),
            (true, true) => (256, 403, 48, 120, 475),
        };
        let base = if self.full { 0 } else { 16 };
        let mut out = vec![0u8; self.width * self.height * 4];
        for y in 0..self.height {
            let row = self.top + y;
            for x in 0..self.width {
                let col = self.left + x;
                let c = (at(row * s + col) as i32 - base) * ys;
                let (u, v) = if semi {
                    let i = luma + (row / 2) * s + (col / 2) * 2;
                    (at(i) as i32 - 128, at(i + 1) as i32 - 128)
                } else {
                    let i = (row / 2) * (s / 2) + col / 2;
                    (at(luma + i) as i32 - 128, at(v_plane + i) as i32 - 128)
                };
                let r = (c + rv * v + 128) >> 8;
                let g = (c - gu * u - gv * v + 128) >> 8;
                let b = (c + bu * u + 128) >> 8;
                let o = (y * self.width + x) * 4;
                out[o] = r.clamp(0, 255) as u8;
                out[o + 1] = g.clamp(0, 255) as u8;
                out[o + 2] = b.clamp(0, 255) as u8;
                out[o + 3] = 255;
            }
        }
        Ok(out)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A 4x2 picture, left half one colour and right half another, laid
    /// out by `color` with a stride of 8 and 4 rows a plane.
    fn picture(color: i32, left: [u8; 3], right: [u8; 3]) -> Vec<u8> {
        let (s, rows) = (8, 4);
        let mut d = vec![0u8; s * rows * 3 / 2];
        for r in 0..2 {
            for c in 0..4 {
                d[r * s + c] = if c < 2 { left[0] } else { right[0] };
            }
        }
        let luma = s * rows;
        for c in 0..2 {
            let [_, u, v] = if c == 0 { left } else { right };
            if color == COLOR_I420 {
                d[luma + c] = u;
                d[luma + (s / 2) * (rows / 2) + c] = v;
            } else {
                d[luma + c * 2] = u;
                d[luma + c * 2 + 1] = v;
            }
        }
        d
    }

    fn layout(color: i32, hd: bool, full: bool) -> Layout {
        Layout { color, stride: 8, rows: 4, left: 0, top: 0, width: 4, height: 2, hd, full }
    }

    fn near(px: &[u8], want: [u8; 3]) -> bool {
        px.iter().zip(want).all(|(a, b)| (*a as i32 - b as i32).abs() <= 2) && px[3] == 255
    }

    /// Limited-range BT.601 red and BT.709 green, as an encoder writes them,
    /// come back as those colours from both layouts.
    #[test]
    fn i420_and_nv12_give_the_colours_back() {
        for color in [COLOR_I420, COLOR_NV12, COLOR_FLEXIBLE] {
            // BT.601 limited: red is Y 81, U 90, V 240; white Y 235, U V 128.
            let rgba = layout(color, false, false).rgba(&picture(color, [81, 90, 240], [235, 128, 128])).unwrap();
            assert!(near(&rgba[0..4], [255, 0, 0]), "{color}: {:?}", &rgba[0..4]);
            assert!(near(&rgba[12..16], [255, 255, 255]), "{color}: {:?}", &rgba[12..16]);
            // BT.709 limited: green is Y 173, U 42, V 26.
            let rgba = layout(color, true, false).rgba(&picture(color, [173, 42, 26], [16, 128, 128])).unwrap();
            assert!(near(&rgba[0..4], [0, 255, 0]), "{color}: {:?}", &rgba[0..4]);
            assert!(near(&rgba[12..16], [0, 0, 0]), "{color}: {:?}", &rgba[12..16]);
        }
    }

    #[test]
    fn a_crop_is_cut_from_the_buffer() {
        let mut l = layout(COLOR_I420, false, false);
        (l.left, l.width) = (2, 2);
        let rgba = l.rgba(&picture(COLOR_I420, [81, 90, 240], [235, 128, 128])).unwrap();
        assert_eq!(rgba.len(), 2 * 2 * 4);
        assert!(near(&rgba[0..4], [255, 255, 255]), "{:?}", &rgba[0..4]);
    }

    #[test]
    fn a_vendor_layout_is_unsupported() {
        let e = layout(0x7FA3_0C04, false, false).rgba(&[0; 64]).unwrap_err();
        assert_eq!(e.kind, "unsupported");
    }
}
