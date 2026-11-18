//! Pictures for the apps, YUY2: the phone's picture resized to the size the app chose, or the
//! "waiting" picture; and NV12 for Media Foundation. Shared by both Windows cameras (DirectShow,
//! Media Foundation). Plain Rust, tested on every system.

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

/// The logo, rendered by `scripts/vcam-waiting-logo.py`: LOGO_SIZE² pixels of Y, U, V, alpha.
const LOGO: &[u8] = include_bytes!("../assets/waiting-logo.yuva");
const LOGO_SIZE: usize = 288;

/// The app's background (`--oc-bg`, #0B1220) and its orange accent (#FE8A6A), in YUV.
const BACKGROUND: [u8; 3] = [31, 135, 124];
const ACCENT: [u8; 3] = [161, 97, 181];
const DOT: [u8; 3] = [72, 128, 128];

/// Shown while no phone sends pictures: the logo on the app's dark background, and three dots
/// under it, one lit in turn (`ms`: a clock), so that the camera looks alive, not broken. No
/// text: the DLL does not know the user's language.
pub fn waiting(dst: &mut [u8], w: usize, h: usize, ms: u64) {
    for px in dst[..w * h * 2].as_chunks_mut::<4>().0 {
        *px = [BACKGROUND[0], BACKGROUND[1], BACKGROUND[0], BACKGROUND[2]];
    }
    // The logo: about a quarter of the height, a little above the middle.
    let s = (h * 27 / 100) & !1;
    if s < 4 {
        return;
    }
    let (x0, y0) = (((w - s) / 2) & !1, (h * 42 / 100).saturating_sub(s / 2));
    for y in 0..s.min(h - y0) {
        let ly = y * LOGO_SIZE / s;
        let row = &mut dst[((y0 + y) * w + x0) * 2..][..s * 2];
        for (m, out) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let at = |x: usize| &LOGO[(ly * LOGO_SIZE + x * LOGO_SIZE / s) * 4..][..4];
            let (a, b) = (at(2 * m), at(2 * m + 1));
            let alpha = (u16::from(a[3]) + u16::from(b[3])) / 2;
            out[0] = blend(out[0], a[0], a[3].into());
            out[2] = blend(out[2], b[0], b[3].into());
            out[1] = blend(
                out[1],
                ((u16::from(a[1]) + u16::from(b[1])) / 2) as u8,
                alpha,
            );
            out[3] = blend(
                out[3],
                ((u16::from(a[2]) + u16::from(b[2])) / 2) as u8,
                alpha,
            );
        }
    }
    // Three dots under it; the lit one moves every 400 ms.
    let r = (s / 22).max(2);
    let cy = y0 + s + s / 5;
    let lit = (ms / 400 % 3) as usize;
    for i in 0..3 {
        let cx = w / 2 + i * r * 4 - r * 4;
        dot(dst, w, h, cx, cy, r, if i == lit { ACCENT } else { DOT });
    }
}

fn blend(under: u8, over: u8, alpha: u16) -> u8 {
    ((u16::from(under) * (255 - alpha) + u16::from(over) * alpha) / 255) as u8
}

/// A filled circle (chroma per pair of pixels, from the left one).
fn dot(dst: &mut [u8], w: usize, h: usize, cx: usize, cy: usize, r: usize, yuv: [u8; 3]) {
    for y in cy.saturating_sub(r)..(cy + r + 1).min(h) {
        for x in cx.saturating_sub(r)..(cx + r + 1).min(w) {
            let (dx, dy) = (x.abs_diff(cx), y.abs_diff(cy));
            if dx * dx + dy * dy <= r * r {
                let i = (y * w + x) * 2;
                dst[i] = yuv[0];
                dst[(i & !3) + 1] = yuv[1];
                dst[(i & !3) + 3] = yuv[2];
            }
        }
    }
}

/// YUY2 → NV12, same size (Media Foundation's format): full luma plane, then chroma averaged over
/// each pair of rows, interleaved U V. `pitch`: bytes per row of the destination's planes.
pub fn yuy2_to_nv12(src: &[u8], w: usize, h: usize, dst: &mut [u8], pitch: usize) {
    let (luma, chroma) = dst.split_at_mut(pitch * h);
    for y in 0..h {
        let row = &src[y * w * 2..][..w * 2];
        for (x, px) in luma[y * pitch..][..w].iter_mut().enumerate() {
            *px = row[x * 2];
        }
    }
    for y in 0..h / 2 {
        let (a, b) = (
            &src[2 * y * w * 2..][..w * 2],
            &src[(2 * y + 1) * w * 2..][..w * 2],
        );
        let out = &mut chroma[y * pitch..][..w];
        for m in 0..w / 2 {
            out[2 * m] = (u16::from(a[4 * m + 1]) + u16::from(b[4 * m + 1])).div_ceil(2) as u8;
            out[2 * m + 1] = (u16::from(a[4 * m + 3]) + u16::from(b[4 * m + 3])).div_ceil(2) as u8;
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
    fn nv12_from_yuy2() {
        let (w, h) = (4, 2);
        // Row 0: U 100 V 150; row 1: U 110 V 160; luma 1..8.
        let src = [
            1, 100, 2, 150, 3, 100, 4, 150, 5, 110, 6, 160, 7, 110, 8, 160,
        ];
        let pitch = 6; // wider than the picture, like a 2D buffer
        let mut dst = vec![0; pitch * h * 3 / 2];
        yuy2_to_nv12(&src, w, h, &mut dst, pitch);
        assert_eq!(&dst[..4], &[1, 2, 3, 4]);
        assert_eq!(&dst[pitch..pitch + 4], &[5, 6, 7, 8]);
        assert_eq!(&dst[2 * pitch..2 * pitch + 4], &[105, 155, 105, 155]);
    }

    /// Background in the corners, the logo in the middle, a dot that moves; every size we offer.
    #[test]
    fn waiting_picture() {
        for (w, h) in [(1920, 1080), (1280, 720), (640, 360)] {
            let mut a = vec![0; w * h * 2];
            let mut b = vec![0; w * h * 2];
            waiting(&mut a, w, h, 0);
            waiting(&mut b, w, h, 400);
            assert_eq!(&a[..4], &[31, 135, 31, 124]); // corner: background
                                                      // The logo's row through its middle is mostly not background.
            let row = &a[(h * 42 / 100) * w * 2..][..w * 2];
            let lit = row.iter().step_by(2).filter(|&&y| y != 31).count();
            assert!(lit > h * 27 / 100 / 4, "no logo at {w}x{h} ({lit} pixels)");
            assert_ne!(a, b, "the dots do not move at {w}x{h}");
        }
        assert_eq!(LOGO.len(), LOGO_SIZE * LOGO_SIZE * 4);
    }
}
