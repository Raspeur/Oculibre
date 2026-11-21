//! Writes a moving test pattern into the virtual camera's memory, as Oculibre would (Windows):
//! `cargo run -p oculibre-vcam-shm --example pattern -- [seconds] [fps]`.

#[cfg(windows)]
fn main() {
    use std::time::{Duration, Instant};
    let mut args = std::env::args().skip(1);
    let seconds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(10);
    let fps: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(30);
    let shared = oculibre_vcam_shm::Shared::create().expect("shared memory");
    let (w, h) = (1920usize, 1080usize);
    let mut frame = vec![0u8; w * h * 2];
    let start = Instant::now();
    let mut n = 0u64;
    let mut global: Option<oculibre_vcam_shm::Shared> = None;
    while start.elapsed() < Duration::from_secs(seconds) {
        // Colour bars, and a white bar moving down: one picture differs from the next.
        let bar = (n as usize * 8) % h;
        for y in 0..h {
            for x in (0..w).step_by(2) {
                let i = (y * w + x) * 2;
                let (luma, u, v) = if y.abs_diff(bar) < 8 {
                    (235, 128, 128)
                } else {
                    [
                        (180, 44, 136),
                        (168, 156, 44),
                        (145, 54, 34),
                        (106, 202, 222),
                        (81, 90, 240),
                        (41, 240, 110),
                    ][x * 6 / w]
                };
                frame[i..i + 4].copy_from_slice(&[luma, u, luma, v]);
            }
        }
        shared
            .frames()
            .write(w, h, &frame, start.elapsed().as_millis() as u64);
        // The Media Foundation camera's block too, once its source made it.
        if global.is_none() && n.is_multiple_of(30) {
            global = oculibre_vcam_shm::Shared::open_global();
        }
        if let Some(g) = &global {
            g.frames()
                .write(w, h, &frame, start.elapsed().as_millis() as u64);
        }
        n += 1;
        let next = Duration::from_micros(n * 1_000_000 / fps);
        if let Some(wait) = next.checked_sub(start.elapsed()) {
            std::thread::sleep(wait);
        }
    }
    println!("{n} pictures written");
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows only");
}
