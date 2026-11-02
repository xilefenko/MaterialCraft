//! Locked layers (`lock: {"source", "reasons"}`, since 0.18).

use super::corruption::rewrite;
use super::fixtures::full_document;
use super::round_trip::{from_bytes, to_bytes};
use crate::json;
use crate::paint::LayerLock;
use crate::paint::format::FormatError;
use crate::zip::ZipReader;
use std::io::Cursor;

fn document() -> crate::paint::Document {
    let mut doc = full_document();
    let lock = LayerLock::new("Substance 3D Painter import", vec!["procedural \"Dirt\"".into(), "no replacement yet".into()]);
    doc.texture_sets[0].layers[0].lock = Some(lock);
    doc
}

fn manifest(bytes: &[u8]) -> json::Value {
    let mut zip = ZipReader::open(Cursor::new(bytes)).unwrap();
    json::parse(&String::from_utf8(zip.read("document.json").unwrap()).unwrap()).unwrap()
}

#[test]
fn locks_round_trip_and_are_written_only_when_set() {
    let doc = document();
    let bytes = to_bytes(&doc);
    let back = from_bytes(&bytes).unwrap();
    assert_eq!(back, doc);
    assert_eq!(to_bytes(&back), bytes, "deterministic");
    let v = manifest(&bytes);
    let layers = v.get("texture_sets").unwrap().as_array().unwrap()[0].get("layers").unwrap().as_array().unwrap();
    let lock = layers[0].get("lock").unwrap();
    assert_eq!(lock.get("source").and_then(|s| s.as_str()), Some("Substance 3D Painter import"));
    assert_eq!(lock.get("reasons").and_then(|r| r.as_array()).map(Vec::len), Some(2));
    assert!(layers[1..].iter().all(|l| l.get("lock").is_none()));
}

#[test]
fn a_lock_reason_that_is_not_text_is_an_error() {
    let bytes = to_bytes(&document());
    let broken = rewrite(&bytes, |name, data| {
        if name != "document.json" {
            return Some(data);
        }
        Some(String::from_utf8(data).unwrap().replacen("\"no replacement yet\"", "7", 1).into_bytes())
    });
    assert!(matches!(from_bytes(&broken), Err(FormatError::Manifest { .. })));
}
