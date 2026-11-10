//! Pixel format conversion. Decoders output planar I420; virtual webcams are most widely
//! accepted (Chrome, Firefox, Zoom, OBS…) in packed YUYV 4:2:2.

use openh264::formats::YUVSource;

/// Converts an I420 picture into a YUYV frame of `dst_w`×`dst_h`, reusing `out`'s
/// allocation. Same size: straight copy. Other size (the virtual camera is locked at
/// another resolution): nearest-neighbour scaling — crude but cheap, and only a fallback.
pub fn i420_to_yuyv(src: &impl YUVSource, dst_w: usize, dst_h: usize, out: &mut Vec<u8>) {
    let (w, h) = src.dimensions();
    let (sy, su, sv) = src.strides();
    let (y, u, v) = (src.y(), src.u(), src.v());

    out.resize(dst_w * dst_h * 2, 0);

    if (w, h) == (dst_w, dst_h) {
        let pairs = w / 2;
        for row in 0..h {
            let yr = &y[row * sy..row * sy + w];
            let ur = &u[(row / 2) * su..(row / 2) * su + pairs];
            let vr = &v[(row / 2) * sv..(row / 2) * sv + pairs];
            let o = &mut out[row * w * 2..(row + 1) * w * 2];
            for (i, px) in o.chunks_exact_mut(4).enumerate() {
                px[0] = yr[2 * i];
                px[1] = ur[i];
                px[2] = yr[2 * i + 1];
                px[3] = vr[i];
            }
        }
        return;
    }

    // Source column for every destination column, computed once per frame.
    let src_x: Vec<usize> = (0..dst_w).map(|x| x * w / dst_w).collect();
    for row in 0..dst_h {
        let sr = row * h / dst_h;
        let yr = &y[sr * sy..];
        let ur = &u[(sr / 2) * su..];
        let vr = &v[(sr / 2) * sv..];
        let o = &mut out[row * dst_w * 2..(row + 1) * dst_w * 2];
        for (i, px) in o.chunks_exact_mut(4).enumerate() {
            let x0 = src_x[2 * i];
            let x1 = src_x[2 * i + 1];
            px[0] = yr[x0];
            px[1] = ur[x0 / 2];
            px[2] = yr[x1];
            px[3] = vr[x0 / 2];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openh264::formats::YUVSlices;

    #[test]
    fn converts_a_2x2_picture() {
        let y = [10, 20, 30, 40];
        let u = [100];
        let v = [200];
        let src = YUVSlices::new((&y, &u, &v), (2, 2), (2, 1, 1));
        let mut out = Vec::new();
        i420_to_yuyv(&src, 2, 2, &mut out);
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
        i420_to_yuyv(&src, 2, 2, &mut out);
        assert_eq!(out, vec![1, 5, 2, 6, 3, 5, 4, 6]);
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
        i420_to_yuyv(&src, 2, 2, &mut out);
        assert_eq!(out, vec![1, 10, 2, 50, 3, 30, 4, 70]);
    }

    #[test]
    fn output_always_has_destination_size() {
        let y = vec![0u8; 8 * 6];
        let u = vec![0u8; 4 * 3];
        let v = vec![0u8; 4 * 3];
        let src = YUVSlices::new((&y, &u, &v), (8, 6), (8, 4, 4));
        let mut out = Vec::new();
        i420_to_yuyv(&src, 12, 4, &mut out);
        assert_eq!(out.len(), 12 * 4 * 2);
    }
}
