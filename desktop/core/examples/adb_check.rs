//! Which adb Oculibre would use here, and whether it runs:
//! `cargo run -p oculibre-core --example adb_check`.

fn main() {
    println!("adb: {:?}", oculibre_core::adb::found());
}
