//! Windows (MSVC): exports the COM entry points through exports.def.
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows-msvc") {
        let def =
            std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("exports.def");
        println!("cargo:rustc-cdylib-link-arg=/DEF:{}", def.display());
        println!("cargo:rerun-if-changed=exports.def");
    }
}
