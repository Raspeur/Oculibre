//! Pixel format conversion. Decoders output planar I420; virtual webcams are most widely
//! accepted (Chrome, Firefox, Zoom, OBS…) in packed YUYV 4:2:2.
//!
//! One pass does everything: rotation, mirror, scaling and I420 → YUYV. Every source coordinate
//! depends on only one output coordinate (a row or a column), so the source index of a pixel is
//! `row_table[y] + column_table[x]`, computed once per frame: no per-pixel arithmetic.

use openh264::formats::YUVSource;

/// What to do to the picture, decided by the session (`--rotate`, `--mirror`, `auto`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Transform {
    /// Clockwise rotation in degrees: 0, 90, 180 or 270.
    pub rotation: u16,
    /// Horizontal flip, applied after the rotation (left ↔ right of the final picture).
    pub mirror: bool,
}

impl Transform {
    /// Size of a `w`×`h` picture after this transform.
    pub fn output_size(self, w: usize, h: usize) -> (usize, usize) {
        if self.rotation % 180 == 90 {
            (h, w)
        } else {
            (w, h)
        }
    }
}

/// A decoded I420 picture that we own (planes packed, no padding), so that the decoder can go on
/// with the next frame while this one is converted and written. Its allocation is reused.
#[derive(Debug, Default)]
pub struct Picture {
    w: usize,
    h: usize,
    data: Vec<u8>,
}

impl Picture {
    /// Copies `src` (any strides) into this picture.
    pub fn copy_from(&mut self, src: &impl YUVSource) {
        let (w, h) = src.dimensions();
        let (sy, su, sv) = src.strides();
        self.copy_i420(w, h, (src.y(), sy), (src.u(), su), (src.v(), sv));
    }

    /// Copies planar I420 planes given as (data, stride).
    pub fn copy_i420(
        &mut self,
        w: usize,
        h: usize,
        y: (&[u8], usize),
        u: (&[u8], usize),
        v: (&[u8], usize),
    ) {
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        self.w = w;
        self.h = h;
        self.data.clear();
        self.data.reserve(w * h + 2 * cw * ch);
        for r in 0..h {
            self.data.extend_from_slice(&y.0[r * y.1..r * y.1 + w]);
        }
        for r in 0..ch {
            self.data.extend_from_slice(&u.0[r * u.1..r * u.1 + cw]);
        }
        for r in 0..ch {
            self.data.extend_from_slice(&v.0[r * v.1..r * v.1 + cw]);
        }
    }

    /// Copies an NV12 picture (luma plane + interleaved UV plane, what hardware decoders output),
    /// splitting U and V.
    #[cfg_attr(not(feature = "ffmpeg"), allow(dead_code))]
    pub fn copy_nv12(&mut self, w: usize, h: usize, y: (&[u8], usize), uv: (&[u8], usize)) {
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        self.w = w;
        self.h = h;
        let chroma = cw * ch;
        self.data.clear();
        self.data.resize(w * h + 2 * chroma, 0);
        let (luma, rest) = self.data.split_at_mut(w * h);
        let (u, v) = rest.split_at_mut(chroma);
        for r in 0..h {
            luma[r * w..(r + 1) * w].copy_from_slice(&y.0[r * y.1..r * y.1 + w]);
        }
        for r in 0..ch {
            let row = &uv.0[r * uv.1..r * uv.1 + 2 * cw];
            let (ur, vr) = (&mut u[r * cw..(r + 1) * cw], &mut v[r * cw..(r + 1) * cw]);
            for (i, pair) in row.as_chunks::<2>().0.iter().enumerate() {
                ur[i] = pair[0];
                vr[i] = pair[1];
            }
        }
    }

    fn chroma_len(&self) -> usize {
        self.w.div_ceil(2) * self.h.div_ceil(2)
    }
}

impl YUVSource for Picture {
    fn dimensions(&self) -> (usize, usize) {
        (self.w, self.h)
    }

    fn strides(&self) -> (usize, usize, usize) {
        let cw = self.w.div_ceil(2);
        (self.w, cw, cw)
    }

    fn y(&self) -> &[u8] {
        &self.data[..self.w * self.h]
    }

    fn u(&self) -> &[u8] {
        let start = self.w * self.h;
        &self.data[start..start + self.chroma_len()]
    }

    fn v(&self) -> &[u8] {
        let start = self.w * self.h + self.chroma_len();
        &self.data[start..start + self.chroma_len()]
    }
}

/// (luma index, chroma index) contributed by a source row or column.
type Offsets = (usize, usize);

/// Black in limited-range YUV.
const BLACK: [u8; 4] = [16, 128, 16, 128];

/// Converts an I420 picture into a YUYV frame of `dst_w`×`dst_h`, reusing `out`'s allocation.
///
/// The transformed picture is scaled to fit the frame *without distortion* (nearest neighbour)
/// and centred, with black bars if the aspect ratios differ: a portrait picture in a landscape
/// virtual camera (or the reverse) stays correct. `dst_w` must be even (YUYV pairs pixels).
pub fn convert(src: &impl YUVSource, t: Transform, dst_w: usize, dst_h: usize, out: &mut Vec<u8>) {
    let (w, h) = src.dimensions();
    let (sy, su, sv) = src.strides();
    let (y, u, v) = (src.y(), src.u(), src.v());
    out.resize(dst_w * dst_h * 2, 0);
    if w == 0 || h == 0 || dst_w < 2 || dst_h == 0 {
        return;
    }
    debug_assert_eq!(su, sv, "U and V strides differ");

    if t == Transform::default() && (w, h) == (dst_w, dst_h) && w % 2 == 0 {
        straight_copy(src, out);
        return;
    }

    // Content rectangle: the transformed picture, fitted and centred.
    let (rw, rh) = t.output_size(w, h);
    let (cw, ch) = if rw * dst_h > rh * dst_w {
        (dst_w, (rh * dst_w / rw).max(1))
    } else {
        ((rw * dst_h / rh).max(2), dst_h)
    };
    let cw = cw & !1; // whole YUYV pairs
    let ox = ((dst_w - cw) / 2) & !1;
    let oy = (dst_h - ch) / 2;

    // Coordinates in the transformed picture (rx, ry) → source coordinates (sx, sy).
    // Each of sx, sy depends on rx only or on ry only.
    let rx_of = |x: usize| {
        let rx = x * rw / cw;
        if t.mirror {
            rw - 1 - rx
        } else {
            rx
        }
    };
    let ry_of = |row: usize| row * rh / ch;
    // (luma offset, chroma offset) contributed by a source column or a source row.
    let col = |sx: usize| (sx, sx / 2);
    let row = |sy_: usize| (sy_ * sy, (sy_ / 2) * su);
    let (col_table, row_table): (Vec<Offsets>, Vec<Offsets>) = match t.rotation % 360 {
        // R(rx, ry) = S(rx, ry)
        0 => (
            (0..cw).map(|x| col(rx_of(x))).collect(),
            (0..ch).map(|r| row(ry_of(r))).collect(),
        ),
        // Clockwise: R(rx, ry) = S(ry, h-1-rx)
        90 => (
            (0..cw).map(|x| row(h - 1 - rx_of(x))).collect(),
            (0..ch).map(|r| col(ry_of(r))).collect(),
        ),
        // R(rx, ry) = S(w-1-rx, h-1-ry)
        180 => (
            (0..cw).map(|x| col(w - 1 - rx_of(x))).collect(),
            (0..ch).map(|r| row(h - 1 - ry_of(r))).collect(),
        ),
        // R(rx, ry) = S(w-1-ry, rx)
        _ => (
            (0..cw).map(|x| row(rx_of(x))).collect(),
            (0..ch).map(|r| col(w - 1 - ry_of(r))).collect(),
        ),
    };

    let line = dst_w * 2;
    if (cw, ch) != (dst_w, dst_h) {
        for px in out.as_chunks_mut::<4>().0 {
            *px = BLACK;
        }
    }
    for (r, &(yr, cr)) in row_table.iter().enumerate() {
        let start = (oy + r) * line + ox * 2;
        let o = &mut out[start..start + cw * 2];
        for (i, px) in o.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (y0, c0) = col_table[2 * i];
            let (y1, _) = col_table[2 * i + 1];
            px[0] = y[yr + y0];
            px[1] = u[cr + c0];
            px[2] = y[yr + y1];
            px[3] = v[cr + c0];
        }
    }
}

/// Same size, no transform: one row at a time.
fn straight_copy(src: &impl YUVSource, out: &mut [u8]) {
    let (w, h) = src.dimensions();
    let (sy, su, sv) = src.strides();
    let (y, u, v) = (src.y(), src.u(), src.v());
    let pairs = w / 2;
    for row in 0..h {
        let yr = &y[row * sy..row * sy + w];
        let ur = &u[(row / 2) * su..(row / 2) * su + pairs];
        let vr = &v[(row / 2) * sv..(row / 2) * sv + pairs];
        let o = &mut out[row * w * 2..(row + 1) * w * 2];
        for (i, px) in o.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            px[0] = yr[2 * i];
            px[1] = ur[i];
            px[2] = yr[2 * i + 1];
            px[3] = vr[i];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openh264::formats::YUVSlices;

    fn t(rotation: u16, mirror: bool) -> Transform {
        Transform { rotation, mirror }
    }

    /// Luma plane of a YUYV frame.
    fn luma(out: &[u8]) -> Vec<u8> {
        out.as_chunks::<2>().0.iter().map(|p| p[0]).collect()
    }

    #[test]
    fn picture_copy_drops_the_stride_padding() {
        // 4x2 picture stored with strides 6 (luma) and 4 (chroma), like a decoder may.
        let y = [1, 2, 3, 4, 0, 0, 5, 6, 7, 8, 0, 0];
        let u = [9, 10, 0, 0];
        let v = [11, 12, 0, 0];
        let src = YUVSlices::new((&y, &u, &v), (4, 2), (6, 4, 4));
        let mut p = Picture::default();
        p.copy_from(&src);
        assert_eq!(p.dimensions(), (4, 2));
        assert_eq!(p.strides(), (4, 2, 2));
        assert_eq!(p.y(), &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(p.u(), &[9, 10]);
        assert_eq!(p.v(), &[11, 12]);
        // Reused for a smaller picture: no stale bytes.
        let small = YUVSlices::new((&[1, 2, 3, 4][..], &[5][..], &[6][..]), (2, 2), (2, 1, 1));
        p.copy_from(&small);
        assert_eq!(
            (p.y(), p.u(), p.v()),
            (&[1, 2, 3, 4][..], &[5][..], &[6][..])
        );
    }

    #[test]
    fn nv12_is_split_into_u_and_v() {
        // 4x2, luma stride 5, UV stride 6: one row of 2 interleaved UV pairs.
        let y = [1, 2, 3, 4, 0, 5, 6, 7, 8, 0];
        let uv = [10, 20, 11, 21, 0, 0];
        let mut p = Picture::default();
        p.copy_nv12(4, 2, (&y, 5), (&uv, 6));
        assert_eq!(p.y(), &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(p.u(), &[10, 11]);
        assert_eq!(p.v(), &[20, 21]);
    }

    #[test]
    fn converts_a_2x2_picture() {
        let y = [10, 20, 30, 40];
        let u = [100];
        let v = [200];
        let src = YUVSlices::new((&y, &u, &v), (2, 2), (2, 1, 1));
        let mut out = Vec::new();
        convert(&src, t(0, false), 2, 2, &mut out);
        assert_eq!(out, vec![10, 100, 20, 200, 30, 100, 40, 200]);
    }

    #[test]
    fn respects_strides() {
        // Width 2, but rows padded to 4 bytes (Y) and 2 bytes (U/V), as decoders do.
        let y = [1, 2, 0, 0, 3, 4, 0, 0];
        let u = [5, 0];
        let v = [6, 0];
        let src = YUVSlices::new((&y, &u, &v), (2, 2), (4, 2, 2));
        let mut out = Vec::new();
        convert(&src, t(0, false), 2, 2, &mut out);
        assert_eq!(out, vec![1, 5, 2, 6, 3, 5, 4, 6]);
        // The general path gives the same result.
        convert(&src, t(0, true), 2, 2, &mut out);
        assert_eq!(out, vec![2, 5, 1, 6, 4, 5, 3, 6]);
    }

    #[test]
    fn downscales_4x4_to_2x2() {
        #[rustfmt::skip]
        let y = [
            1, 0, 2, 0,
            0, 0, 0, 0,
            3, 0, 4, 0,
            0, 0, 0, 0,
        ];
        let u = [10, 20, 30, 40];
        let v = [50, 60, 70, 80];
        let src = YUVSlices::new((&y, &u, &v), (4, 4), (4, 2, 2));
        let mut out = Vec::new();
        convert(&src, t(0, false), 2, 2, &mut out);
        assert_eq!(out, vec![1, 10, 2, 50, 3, 30, 4, 70]);
    }

    /// 4×2 source, luma = position:
    /// ```text
    /// 1 2 3 4
    /// 5 6 7 8
    /// ```
    fn sample() -> ([u8; 8], [u8; 2], [u8; 2]) {
        ([1, 2, 3, 4, 5, 6, 7, 8], [100, 101], [200, 201])
    }

    #[test]
    fn rotations() {
        let (y, u, v) = sample();
        let src = YUVSlices::new((&y, &u, &v), (4, 2), (4, 2, 2));
        let mut out = Vec::new();

        // Clockwise 90°: 2×4, the bottom-left pixel goes to the top-left.
        convert(&src, t(90, false), 2, 4, &mut out);
        assert_eq!(luma(&out), vec![5, 1, 6, 2, 7, 3, 8, 4]);

        convert(&src, t(180, false), 4, 2, &mut out);
        assert_eq!(luma(&out), vec![8, 7, 6, 5, 4, 3, 2, 1]);

        // 270° clockwise = 90° counter-clockwise: the top-right pixel goes to the top-left.
        convert(&src, t(270, false), 2, 4, &mut out);
        assert_eq!(luma(&out), vec![4, 8, 3, 7, 2, 6, 1, 5]);

        // Mirror after rotation: left and right of the final picture are swapped.
        convert(&src, t(90, true), 2, 4, &mut out);
        assert_eq!(luma(&out), vec![1, 5, 2, 6, 3, 7, 4, 8]);
        convert(&src, t(0, true), 4, 2, &mut out);
        assert_eq!(luma(&out), vec![4, 3, 2, 1, 8, 7, 6, 5]);
    }

    #[test]
    fn chroma_follows_the_pixels() {
        let (y, u, v) = sample();
        let src = YUVSlices::new((&y, &u, &v), (4, 2), (4, 2, 2));
        let mut out = Vec::new();
        convert(&src, t(180, false), 4, 2, &mut out);
        // First pair = source pixels 8, 7 (right half): chroma 101 / 201.
        assert_eq!(&out[..4], &[8, 101, 7, 201]);
    }

    #[test]
    fn portrait_in_a_landscape_frame_gets_black_bars() {
        let (y, u, v) = sample();
        let src = YUVSlices::new((&y, &u, &v), (4, 2), (4, 2, 2));
        let mut out = Vec::new();
        // The rotated picture is 2×4: in an 8×4 frame it fills columns 2..4 (pairs stay whole).
        convert(&src, t(90, false), 8, 4, &mut out);
        #[rustfmt::skip]
        assert_eq!(luma(&out), vec![
            16, 16, 5, 1, 16, 16, 16, 16,
            16, 16, 6, 2, 16, 16, 16, 16,
            16, 16, 7, 3, 16, 16, 16, 16,
            16, 16, 8, 4, 16, 16, 16, 16,
        ]);
        // Bars are black: neutral chroma.
        assert_eq!(&out[..4], &[16, 128, 16, 128]);
    }

    #[test]
    fn landscape_in_a_portrait_frame_is_scaled_down() {
        let (y, u, v) = sample();
        let src = YUVSlices::new((&y, &u, &v), (4, 2), (4, 2, 2));
        let mut out = Vec::new();
        // 4×2 in 2×4: scaled to 2×1, centred vertically (row 1).
        convert(&src, t(0, false), 2, 4, &mut out);
        assert_eq!(luma(&out), vec![16, 16, 1, 3, 16, 16, 16, 16]);
    }

    #[test]
    fn output_always_has_destination_size() {
        let y = vec![0u8; 8 * 6];
        let u = vec![0u8; 4 * 3];
        let v = vec![0u8; 4 * 3];
        let src = YUVSlices::new((&y, &u, &v), (8, 6), (8, 4, 4));
        let mut out = Vec::new();
        for r in [0, 90, 180, 270] {
            for (dw, dh) in [(12, 4), (2, 30), (8, 6), (6, 8), (1920, 1080)] {
                convert(&src, t(r, r == 90), dw, dh, &mut out);
                assert_eq!(out.len(), dw * dh * 2);
            }
        }
    }
}
