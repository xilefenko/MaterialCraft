//! Import origin (`import_origin`, since 0.18).

use super::corruption::rewrite;
use super::fixtures::full_document;
use super::round_trip::{from_bytes, to_bytes};
use crate::json;
use crate::paint::ImportOrigin;
use crate::paint::format::FormatError;
use crate::zip::ZipReader;
use std::io::Cursor;

fn manifest(bytes: &[u8]) -> json::Value {
    let mut zip = ZipReader::open(Cursor::new(bytes)).unwrap();
    json::parse(std::str::from_utf8(&zip.read("document.json").unwrap()).unwrap()).unwrap()
}

fn edited(bytes: &[u8], edit: impl Fn(&mut json::Value)) -> Vec<u8> {
    rewrite(bytes, |name, data| {
        if name != "document.json" {
            return Some(data);
        }
        let mut v = json::parse(std::str::from_utf8(&data).unwrap()).unwrap();
        edit(&mut v);
        Some(v.to_string_pretty().into_bytes())
    })
}

fn origin() -> ImportOrigin {
    let options = json::parse(r#"{"plan": {"default": "replace", "procedurals": {"mg_dirt": "keep"}}, "strokes": "replay"}"#).unwrap();
    ImportOrigin {
        kind: "substance-painter".into(),
        importer: "materialcraft-substance 0.1.0".into(),
        path: "/projects/robot.spp".into(),
        size: Some(52_428_800),
        modified_ms: Some(1_730_000_000_123),
        options,
    }
}

#[test]
fn origin_round_trips() {
    let mut doc = full_document();
    doc.import_origin = Some(origin());
    let bytes = to_bytes(&doc);
    let v = manifest(&bytes);
    let o = v.get("import_origin").unwrap();
    assert_eq!(o.get("kind").and_then(json::Value::as_str), Some("substance-painter"));
    assert_eq!(o.get("modified_ms").and_then(json::Value::as_u64), Some(1_730_000_000_123));
    assert_eq!(o.get("options").and_then(|p| p.get("strokes")).and_then(json::Value::as_str), Some("replay"));
    let back = from_bytes(&bytes).unwrap();
    assert_eq!(back.import_origin, doc.import_origin);
    assert_eq!(to_bytes(&back), bytes, "load → save reproduces the file");
}

#[test]
fn absent_origin_is_not_written_and_optional_fields_may_be_missing() {
    let doc = full_document();
    let bytes = to_bytes(&doc);
    assert!(manifest(&bytes).get("import_origin").is_none());
    assert_eq!(from_bytes(&bytes).unwrap().import_origin, None);
    let mut doc = full_document();
    doc.import_origin = Some(ImportOrigin::new("substance-painter", "/a.spp"));
    let bytes = to_bytes(&doc);
    let o = manifest(&bytes).get("import_origin").cloned().unwrap();
    assert_eq!(o.as_object().map(Vec::len), Some(2), "only kind and path: {o}");
    assert_eq!(from_bytes(&bytes).unwrap().import_origin, doc.import_origin);
}

#[test]
fn unknown_fields_are_kept_and_bad_values_refused() {
    let mut doc = full_document();
    doc.import_origin = Some(origin());
    let bytes = to_bytes(&doc);
    let future = edited(&bytes, |v| {
        v.get_mut("import_origin").unwrap().insert("hash", "abc");
    });
    let loaded = from_bytes(&future).unwrap();
    assert_eq!(loaded.import_origin, doc.import_origin);
    let again = manifest(&to_bytes(&loaded));
    assert_eq!(again.get("import_origin").and_then(|o| o.get("hash")).and_then(json::Value::as_str), Some("abc"), "unknown field kept");
    for (key, bad) in [("size", json::Value::from(-1)), ("modified_ms", json::Value::from(1.5)), ("path", json::Value::from(3))] {
        let broken = edited(&bytes, |v| {
            v.get_mut("import_origin").unwrap().insert(key, bad.clone());
        });
        assert!(matches!(from_bytes(&broken), Err(FormatError::Manifest { .. })), "{key}");
    }
}
