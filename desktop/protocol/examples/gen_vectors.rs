//! Writes the shared test vectors to `tests/protocol/vectors/` (repository root).
//! Run after a deliberate wire-format change: `cargo run -p oculibre-protocol --example gen_vectors`.

use std::fs;
use std::path::Path;

use oculibre_protocol::vectors;

fn main() -> std::io::Result<()> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/protocol/vectors");
    fs::create_dir_all(&dir)?;
    // Start clean so that removed vectors disappear.
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("hex" | "json")
        ) {
            fs::remove_file(path)?;
        }
    }
    let all = vectors::all();
    for v in &all {
        fs::write(
            dir.join(format!("{}.hex", v.name)),
            vectors::to_hex(&v.bytes) + "\n",
        )?;
        let json = serde_json::to_string_pretty(&vectors::describe(v)).expect("json") + "\n";
        fs::write(dir.join(format!("{}.json", v.name)), json)?;
    }
    println!(
        "{} vectors written to {}",
        all.len(),
        dir.canonicalize()?.display()
    );
    Ok(())
}
