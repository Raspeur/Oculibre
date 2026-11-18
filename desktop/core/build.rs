//! `cfg(ffmpeg)`: the `ffmpeg` feature on a system where FFmpeg is used (not Windows, which
//! decodes with Media Foundation). The feature is on by default and Cargo cannot turn a default
//! feature off for one system only: so it is simply ignored there.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(ffmpeg)");
    let windows = std::env::var("CARGO_CFG_WINDOWS").is_ok();
    if std::env::var("CARGO_FEATURE_FFMPEG").is_ok() && !windows {
        println!("cargo::rustc-cfg=ffmpeg");
    }
}
