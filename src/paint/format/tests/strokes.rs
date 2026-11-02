//! Format 0.4: stroke records next to paint pixels, stored as binary entries. They round trip byte-identically;
//! 0.3 files load; unreadable, unknown and missing records are reported and never stop a load.

use super::corruption::rewrite;
use super::fixtures::full_document;
use super::round_trip::{from_bytes, to_bytes};
use crate::json::{self, Value};
use crate::paint::Document;
use crate::paint::format::strokes::tests::sample_record;
use crate::paint::stroke::{self, StrokeData, StrokeOwner, StrokeRecord, UnreadableStrokes};
use crate::zip::ZipReader;
use std::io::Cursor;

const OWNERS: [(usize, &str, StrokeOwner); 6] = [
    (0, "p1", StrokeOwner::Paint),
    (0, "p0", StrokeOwner::Mask { effect: 0 }),
    (0, "deep", StrokeOwner::Paint),
    (0, "effects", StrokeOwner::Effect { effect: 0 }),
    (0, "effects", StrokeOwner::Mask { effect: 0 }),
    (2, "x y", StrokeOwner::Paint),
];

/// The full document with stroke records on paint layers (also nested), a content paint effect and mask
/// paint effects.
fn with_strokes() -> Document {
    let mut doc = full_document();
    for (i, (set, layer, owner)) in OWNERS.into_iter().enumerate() {
        let slot = doc.texture_sets[set].layer_mut(layer).unwrap().strokes_mut(owner).unwrap();
        *slot = Some(StrokeData::Record(sample_record(i as u32 + 1)));
    }
    doc
}

fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipReader::open(Cursor::new(bytes)).unwrap();
    zip.entries().to_vec().into_iter().map(|e| (e.name.clone(), zip.read_entry(&e).unwrap())).collect()
}

fn manifest(bytes: &[u8]) -> Value {
    let all = entries(bytes);
    json::parse(std::str::from_utf8(&all.iter().find(|(n, _)| n == "document.json").unwrap().1).unwrap()).unwrap()
}

fn layer_json<'v>(v: &'v Value, set: usize, id: &str) -> &'v Value {
    fn find<'v>(layers: &'v Value, id: &str) -> Option<&'v Value> {
        layers.as_array()?.iter().find_map(|l| if l["id"].as_str() == Some(id) { Some(l) } else { find(&l["children"], id) })
    }
    find(&v["texture_sets"][set]["layers"], id).unwrap()
}

#[test]
fn records_round_trip_and_files_stay_byte_identical() {
    let doc = with_strokes();
    let bytes = to_bytes(&doc);
    let back = from_bytes(&bytes).unwrap();
    assert_eq!(back, doc);
    assert_eq!(to_bytes(&back), bytes, "load → save gives the same file");
    assert_eq!(stroke::totals(&back), (6 * 4, (1 + 7 + 3) * 5 * 6));
    assert!(stroke::problems(&back).is_empty());
}

#[test]
fn records_are_binary_entries_next_to_the_pixels() {
    let bytes = to_bytes(&with_strokes());
    let v = manifest(&bytes);
    assert_eq!(v["format_version"].as_str(), Some(super::super::FORMAT_VERSION));
    let paths = [
        layer_json(&v, 0, "p1")["strokes"].as_str(),
        layer_json(&v, 0, "p0")["mask"]["effects"][0]["strokes"].as_str(),
        layer_json(&v, 0, "deep")["strokes"].as_str(),
        layer_json(&v, 0, "effects")["effects"][0]["strokes"].as_str(),
        layer_json(&v, 0, "effects")["mask"]["effects"][0]["strokes"].as_str(),
        layer_json(&v, 2, "x y")["strokes"].as_str(),
    ];
    assert_eq!(
        paths.map(Option::unwrap),
        [
            "textures/body/layers/p1/strokes.mcstroke",
            "textures/body/layers/p0/mask-0-strokes.mcstroke",
            "textures/body/layers/deep/strokes.mcstroke",
            "textures/body/layers/effects/effect-0-strokes.mcstroke",
            "textures/body/layers/effects/mask-0-strokes.mcstroke",
            "textures/odd_.._id/layers/x_y/strokes.mcstroke",
        ]
    );
    assert!(layer_json(&v, 0, "p2").get("strokes").is_none(), "no record, no field");
    let all = entries(&bytes);
    let stored = all.iter().find(|(n, _)| n == paths[0].unwrap()).unwrap();
    assert_eq!(stored.1, crate::paint::format::strokes::codec::encode(&sample_record(1)).unwrap());
    // Records come after the images and before the foreign files.
    let pos = |name: &str| all.iter().position(|(n, _)| n == name).unwrap();
    let last_image = all.iter().rposition(|(n, _)| n.ends_with(".png") && n.starts_with("textures/")).unwrap();
    assert!(pos(paths[0].unwrap()) > last_image);
    assert!(pos(paths[5].unwrap()) < pos("foreign/substance/document.bin"));
}

#[test]
fn files_without_records_load_as_before() {
    let doc = full_document();
    let old = rewrite(&to_bytes(&doc), |name, data| {
        if name != "document.json" {
            return Some(data);
        }
        Some(String::from_utf8(data).unwrap().replace("\"format_version\": \"0.18\"", "\"format_version\": \"0.3\"").into_bytes())
    });
    assert_eq!(manifest(&old)["format_version"].as_str(), Some("0.3"));
    let back = from_bytes(&old).unwrap();
    assert_eq!(stroke::totals(&back), (0, 0));
    assert_eq!(back.texture_sets, doc.texture_sets);
}

/// The archive with the record entry of `p1` replaced (or dropped when `edit` gives `None`).
fn edit_p1_record(edit: impl Fn(Vec<u8>) -> Option<Vec<u8>>) -> Vec<u8> {
    rewrite(&to_bytes(&with_strokes()), |name, data| if name == "textures/body/layers/p1/strokes.mcstroke" { edit(data) } else { Some(data) })
}

fn p1(doc: &Document) -> Option<&StrokeData> {
    doc.texture_sets[0].layer("p1").unwrap().stroke_records().into_iter().map(|(_, d)| d).next()
}

#[test]
fn unreadable_records_are_reported_and_written_back_unchanged() {
    for garbage in [b"MCSTROKE\x02\x00\x00\x00 a newer layout".to_vec(), b"garbled".to_vec(), Vec::new()] {
        let bytes = edit_p1_record(|_| Some(garbage.clone()));
        let back = from_bytes(&bytes).unwrap();
        let Some(StrokeData::Unreadable(u)) = p1(&back) else { panic!("expected an unreadable record") };
        assert_eq!(u.bytes, garbage);
        let problems = stroke::problems(&back);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!((problems[0].layer.as_str(), problems[0].owner), ("p1", StrokeOwner::Paint));
        assert!(problems[0].to_string().contains("stroke record not readable"), "{}", problems[0]);
        // The other records are untouched, and saving keeps the unreadable bytes.
        assert_eq!(stroke::totals(&back).0, 5 * 4);
        let again = to_bytes(&back);
        if garbage.is_empty() {
            assert!(layer_json(&manifest(&again), 0, "p1").get("strokes").is_none(), "nothing to keep");
        } else {
            let kept = entries(&again).into_iter().find(|(n, _)| n == "textures/body/layers/p1/strokes.mcstroke").unwrap();
            assert_eq!(kept.1, garbage);
            assert_eq!(from_bytes(&again).unwrap(), back);
        }
    }
}

#[test]
fn a_missing_record_entry_does_not_stop_the_load() {
    let bytes = edit_p1_record(|_| None);
    let back = from_bytes(&bytes).unwrap();
    assert!(matches!(p1(&back), Some(StrokeData::Unreadable(UnreadableStrokes { bytes, reason })) if bytes.is_empty() && reason.contains("missing")));
    let again = to_bytes(&back);
    assert!(layer_json(&manifest(&again), 0, "p1").get("strokes").is_none());
    let mut expected = with_strokes();
    *expected.texture_sets[0].layer_mut("p1").unwrap().strokes_mut(StrokeOwner::Paint).unwrap() = None;
    assert_eq!(from_bytes(&again).unwrap(), expected, "the lost record is dropped on the next save");
}

#[test]
fn a_strokes_field_on_a_non_string_is_a_manifest_error() {
    let bytes = rewrite(&to_bytes(&with_strokes()), |name, data| {
        if name != "document.json" {
            return Some(data);
        }
        let text = String::from_utf8(data).unwrap().replace("\"textures/body/layers/p1/strokes.mcstroke\"", "17");
        Some(text.into_bytes())
    });
    assert!(matches!(from_bytes(&bytes), Err(crate::paint::format::FormatError::Manifest { at, .. }) if at.ends_with(".strokes")));
}

#[test]
fn empty_records_are_kept_as_records() {
    let mut doc = full_document();
    *doc.texture_sets[0].layer_mut("p3").unwrap().strokes_mut(StrokeOwner::Paint).unwrap() = Some(StrokeRecord::default().into());
    let back = from_bytes(&to_bytes(&doc)).unwrap();
    assert_eq!(back, doc);
}

/// Format 0.11: see-through paint content keeps its flag, and clone and blur strokes keep their tools.
#[test]
fn see_through_content_and_retouch_tools_round_trip() {
    let mut doc = full_document();
    let layer = doc.texture_sets[0].layer_mut("p3").unwrap();
    let crate::paint::LayerKind::Paint(p) = &mut layer.kind else { panic!("p3 is a paint layer") };
    p.see_through = true;
    let mut record = sample_record(5);
    record.strokes[0].tool = stroke::StrokeTool::Clone;
    record.strokes[1].tool = stroke::StrokeTool::Blur;
    p.strokes = Some(record.into());
    let bytes = to_bytes(&doc);
    assert_eq!(layer_json(&manifest(&bytes), 0, "p3")["see_through"].as_bool(), Some(true));
    assert!(layer_json(&manifest(&bytes), 0, "p1").get("see_through").is_none(), "written only when set");
    let back = from_bytes(&bytes).unwrap();
    assert_eq!(back, doc);
    assert_eq!(to_bytes(&back), bytes);
}
