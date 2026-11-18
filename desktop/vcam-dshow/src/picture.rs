//! Pictures for the app, YUY2: the phone's picture resized to the size the app chose, or the
//! "waiting" picture. Plain Rust, tested on every system.

/// Black in YUY2 (Y 16, U and V 128): bars around a picture of another shape.
const BLACK: [u8; 4] = [16, 128, 16, 128];

/// Fits `src` (`sw`×`sh`, YUY2) into `dst` (`dw`×`dh`), keeping its shape: black bars where it
/// does not fill. Nearest pixel: cheap, and the sizes apps pick are close to ours anyway.
pub fn fit(src: &[u8], sw: usize, sh: usize, dst: &mut [u8], dw: usize, dh: usize) {
    if (sw, sh) == (dw, dh) {
        dst[..dw * dh * 2].copy_from_slice(&src[..sw * sh * 2]);
        return;
    }
    // The largest even-width area of the source's shape inside the destination.
    let (fw, fh) = if sw * dh > dw * sh {
        (dw, (dw * sh / sw).max(1))
    } else {
        (((dh * sw / sh) & !1).max(2), dh)
    };
    let (x0, y0) = (((dw - fw) / 2) & !1, (dh - fh) / 2);
    for y in 0..dh {
        let row = &mut dst[y * dw * 2..(y + 1) * dw * 2];
        if y < y0 || y >= y0 + fh {
            fill(row);
            continue;
        }
        let src_row = &src[((y - y0) * sh / fh) * sw * 2..][..sw * 2];
        fill(&mut row[..x0 * 2]);
        fill(&mut row[(x0 + fw) * 2..]);
        for m in 0..fw / 2 {
            // Output pixels 2m and 2m+1 from their nearest source pixels; chroma from the first.
            let a = (2 * m) * sw / fw;
            let b = ((2 * m + 1) * sw / fw).min(sw - 1);
            let out = &mut row[(x0 + 2 * m) * 2..][..4];
            out[0] = src_row[a * 2];
            out[1] = src_row[(a & !1) * 2 + 1];
            out[2] = src_row[b * 2];
            out[3] = src_row[(a & !1) * 2 + 3];
        }
    }
}

fn fill(row: &mut [u8]) {
    for (i, b) in row.iter_mut().enumerate() {
        *b = BLACK[i % 4];
    }
}

/// Shown while no phone sends pictures: dark, with a lighter band in the middle so that it does
/// not look like a broken camera. (A real "Waiting for the phone" picture comes later.)
pub fn waiting(dst: &mut [u8], w: usize, h: usize) {
    for y in 0..h {
        let band = y >= h * 9 / 20 && y < h * 11 / 20;
        let luma = if band { 60 } else { 28 };
        for px in dst[y * w * 2..(y + 1) * w * 2].as_chunks_mut::<4>().0 {
            *px = [luma, 128, luma, 128];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: usize, h: usize, luma: u8) -> Vec<u8> {
        [luma, 100, luma, 150].repeat(w * h / 2)
    }

    #[test]
    fn same_size_is_a_copy() {
        let src = picture(8, 4, 200);
        let mut dst = vec![0; 8 * 4 * 2];
        fit(&src, 8, 4, &mut dst, 8, 4);
        assert_eq!(src, dst);
    }

    #[test]
    fn smaller_same_shape_fills_everything() {
        let src = picture(1920, 1080, 200);
        let mut dst = vec![0; 640 * 360 * 2];
        fit(&src, 1920, 1080, &mut dst, 640, 360);
        assert!(dst.chunks(4).all(|px| px == [200, 100, 200, 150]));
    }

    /// A portrait picture in a landscape camera: bars left and right, picture in the middle.
    #[test]
    fn other_shape_gets_bars() {
        let src = picture(1080, 1920, 200);
        let (w, h) = (1280, 720);
        let mut dst = vec![0; w * h * 2];
        fit(&src, 1080, 1920, &mut dst, w, h);
        let at = |x: usize, y: usize| dst[(y * w + x) * 2];
        assert_eq!(at(0, 360), 16); // bar
        assert_eq!(at(w - 1, 360), 16);
        assert_eq!(at(w / 2, 360), 200); // picture
        assert_eq!(at(w / 2, 0), 200); // full height
    }

    #[test]
    fn waiting_picture_is_valid_yuy2() {
        let mut dst = vec![0; 64 * 40 * 2];
        waiting(&mut dst, 64, 40);
        assert!(dst.chunks(4).all(|px| px[1] == 128 && px[3] == 128));
    }
}
