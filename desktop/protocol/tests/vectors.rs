//! Shared test vectors (§16): the files on disk must match the generator, and every file must
//! decode to its description. The Kotlin implementation runs the same checks on the same files.

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

use oculibre_protocol::vectors::{self, describe, fields, from_hex, Expect};
use oculibre_protocol::{Header, Message, MessageReader, Preamble, HEADER_LEN};
use serde_json::Value;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/protocol/vectors")
}

#[test]
fn files_match_the_generator() {
    let all = vectors::all();
    for v in &all {
        let hex = fs::read_to_string(dir().join(format!("{}.hex", v.name))).unwrap_or_else(|_| {
            panic!(
                "missing {}.hex: run `cargo run -p oculibre-protocol --example gen_vectors`",
                v.name
            )
        });
        assert_eq!(
            hex.trim(),
            vectors::to_hex(&v.bytes),
            "{}.hex is stale: regenerate the vectors",
            v.name
        );
        let json: Value = serde_json::from_str(
            &fs::read_to_string(dir().join(format!("{}.json", v.name))).unwrap(),
        )
        .unwrap();
        assert_eq!(
            json,
            describe(v),
            "{}.json is stale: regenerate the vectors",
            v.name
        );
    }
    let on_disk = fs::read_dir(dir()).unwrap().filter(|e| {
        e.as_ref()
            .unwrap()
            .path()
            .extension()
            .and_then(|x| x.to_str())
            == Some("hex")
    });
    assert_eq!(on_disk.count(), all.len(), "extra vector files on disk");
}

/// Decodes every `.hex` file with the public API only, and compares with its `.json`.
#[test]
fn every_file_decodes_to_its_description() {
    let mut checked = 0;
    for entry in fs::read_dir(dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|x| x.to_str()) != Some("hex") {
            continue;
        }
        let bytes = from_hex(&fs::read_to_string(&path).unwrap()).expect("valid hex");
        let desc: Value =
            serde_json::from_str(&fs::read_to_string(path.with_extension("json")).unwrap())
                .unwrap();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let mut reader = MessageReader::new(Cursor::new(bytes.clone()));

        match desc["kind"].as_str().unwrap() {
            "preamble" => {
                let p = reader.read_preamble().unwrap();
                assert_eq!(p.major as u64, desc["major"].as_u64().unwrap(), "{name}");
                assert_eq!(p.minor as u64, desc["minor"].as_u64().unwrap(), "{name}");
                assert_eq!(p.role.name(), desc["role"].as_str().unwrap(), "{name}");
                assert_eq!(p.encode().to_vec(), bytes, "{name}: re-encoding differs");
            }
            "message" => {
                let m = reader
                    .read_message()
                    .unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!(fields(&m), desc["fields"], "{name}: decoded fields differ");
                let mut hb = [0u8; HEADER_LEN];
                hb.copy_from_slice(&bytes[..HEADER_LEN]);
                let h = Header::decode(&hb).unwrap();
                assert_eq!(
                    h.msg_type as u64,
                    desc["header"]["type"].as_u64().unwrap(),
                    "{name}"
                );
                assert_eq!(
                    h.flags as u64,
                    desc["header"]["flags"].as_u64().unwrap(),
                    "{name}"
                );
                assert_eq!(
                    h.stream as u64,
                    desc["header"]["stream"].as_u64().unwrap(),
                    "{name}"
                );
                assert_eq!(
                    h.timestamp,
                    desc["header"]["timestamp"].as_u64().unwrap(),
                    "{name}"
                );
                if !matches!(m, Message::Unknown { .. }) && name != "hello_future_keys" {
                    assert_eq!(m.encode(), bytes, "{name}: re-encoding differs");
                }
            }
            "error" => {
                let err = match desc["stage"].as_str().unwrap() {
                    "preamble" => reader.read_preamble().map(|_| ()).unwrap_err(),
                    _ => reader.read_message().map(|_| ()).unwrap_err(),
                };
                assert_eq!(err.kind(), desc["expect_error"].as_str().unwrap(), "{name}");
            }
            other => panic!("{name}: unknown kind {other}"),
        }
        checked += 1;
    }
    assert!(checked >= 25, "only {checked} vectors found");
}

#[test]
fn generator_expectations_hold() {
    for v in vectors::all() {
        let mut r = MessageReader::new(Cursor::new(v.bytes.clone()));
        match v.expect {
            Expect::Preamble(p) => assert_eq!(r.read_preamble().unwrap(), p, "{}", v.name),
            Expect::Message(m) => assert_eq!(r.read_message().unwrap(), *m, "{}", v.name),
            Expect::Error { stage, kind } => {
                let e = if stage == "preamble" {
                    Preamble::decode(&v.bytes[..8].try_into().unwrap()).unwrap_err()
                } else {
                    r.read_message().unwrap_err()
                };
                assert_eq!(e.kind(), kind, "{}", v.name);
            }
        }
    }
}
