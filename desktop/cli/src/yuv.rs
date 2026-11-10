//! Pixel format conversion. Decoders output planar I420; virtual webcams are most widely
//! accepted (Chrome, Firefox, Zoom, OBS…) in packed YUYV 4:2:2.

use openh264::formats::YUVSource;

/// Converts an I420 picture into YUYV (Y0 U Y1 V), reusing `out`'s allocation.
pub fn i420_to_yuyv(src: &impl YUVSource, out: &mut Vec<u8>) {
    let (w, h) = src.dimensions();
    let (sy, su, sv) = src.strides();
    let (y, u, v) = (src.y(), src.u(), src.v());

    out.resize(w * h * 2, 0);
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
        i420_to_yuyv(&src, &mut out);
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
        i420_to_yuyv(&src, &mut out);
        assert_eq!(out, vec![1, 5, 2, 6, 3, 5, 4, 6]);
    }
}
