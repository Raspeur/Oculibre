//! Writes the waiting picture as raw YUY2, to look at it:
//! `cargo run -p oculibre-vcam-shm --example waiting -- out.yuy2 1280 720 [ms]`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (w, h): (usize, usize) = (args[2].parse().unwrap(), args[3].parse().unwrap());
    let ms = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut out = vec![0u8; w * h * 2];
    oculibre_vcam_shm::picture::waiting(&mut out, w, h, ms);
    std::fs::write(&args[1], out).unwrap();
}
