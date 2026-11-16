//! Which H.264 decoder Oculibre would use on this computer, and why:
//! `cargo run --release -p oculibre-core --example decoder_check`.

use oculibre_core::decoder::{self, Choice, VideoDecoder};

fn main() {
    decoder::prepare_environment();
    match decoder::ffmpeg_available() {
        Ok(()) => println!("FFmpeg: found"),
        Err(why) => println!("FFmpeg: not used ({why})"),
    }
    match VideoDecoder::with(Choice::Auto) {
        Ok(d) => println!("Decoder: {}", d.name()),
        Err(e) => println!("No decoder: {e}"),
    }
}
