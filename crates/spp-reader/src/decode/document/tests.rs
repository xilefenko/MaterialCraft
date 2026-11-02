use super::test_writer::{DocWriter, sample};
use super::*;
use crate::decode::DecodeError;

#[test]
fn counts_classes_of_all_nesting_levels() {
    let census = ClassCensus::scan(&sample()).unwrap();
    assert_eq!(census.count("DataDocument"), 1);
    assert_eq!(census.count("DataLayerColor"), 2);
    assert_eq!(census.count("Data2DTransformation"), 2);
    assert_eq!(census.total(), 5);
    assert!(census.is_complete());
    assert_eq!(census.by_frequency()[2], ("DataDocument", 1));
}

#[test]
fn field_names_are_not_classes() {
    let census = ClassCensus::scan(&sample()).unwrap();
    assert_eq!(census.count("colorManagementACE"), 0);
    assert_eq!(census.count("uvTransformation"), 0);
}

#[test]
fn reads_the_format_version() {
    assert_eq!(DocumentHeader::parse(&sample()).unwrap().format_version, 78);
}

#[test]
fn rejects_other_signatures() {
    let mut data = sample();
    data[0] = 0;
    assert_eq!(ClassCensus::scan(&data), Err(DecodeError::BadMagic { record: RECORD }));
}

#[test]
fn unknown_tags_skip_only_the_enclosing_object() {
    let mut w = DocWriter::new(78);
    w.object(0x12, "DataDocument", 2);
    w.field("first").object(0x14, "DataFuture", 2);
    w.field("mystery").raw(&[0x7f, 1, 2, 3]);
    w.field("hidden").object(0x14, "DataHidden", 0).close();
    w.close();
    w.field("after").object(0x14, "DataAfter", 0).close();
    w.close();
    let census = ClassCensus::scan(&w.bytes).unwrap();
    assert_eq!(census.count("DataFuture"), 1);
    assert_eq!(census.count("DataHidden"), 0);
    assert_eq!(census.count("DataAfter"), 1);
    assert_eq!(census.unknown_tags().get(&0x7f), Some(&1));
    assert!(!census.is_complete());
}

#[test]
fn every_truncation_is_an_error_not_a_panic() {
    let data = sample();
    for len in 0..data.len() {
        assert!(ClassCensus::scan(&data[..len]).is_err(), "length {len}");
    }
}

#[test]
fn corrupt_counts_and_offsets_are_rejected() {
    let data = sample();
    // Field count of the root object: after header (12), tag, flag, end (4), class (4 + 12).
    let mut many_fields = data.clone();
    many_fields[34..36].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(matches!(
        ClassCensus::scan(&many_fields),
        Err(DecodeError::Invalid { .. })
    ));
    let mut bad_end = data;
    bad_end[14..18].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(ClassCensus::scan(&bad_end), Err(DecodeError::Invalid { .. })));
}

#[test]
fn deep_nesting_is_bounded() {
    let mut w = DocWriter::new(78);
    for _ in 0..300 {
        w.object(0x14, "DataNest", 1).field("child");
    }
    w.raw(&[0x0a, 0]);
    for _ in 0..300 {
        w.close();
    }
    assert!(matches!(ClassCensus::scan(&w.bytes), Err(DecodeError::Invalid { .. })));
}

/// Real files are Adobe and user content, so they stay outside the repository.
/// Run with `DOCUMENT_SAMPLES=<dir> cargo test -p spp-reader -- --ignored`.
#[test]
#[ignore]
fn scans_real_samples() {
    let Ok(dir) = std::env::var("DOCUMENT_SAMPLES") else {
        return;
    };
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "bin") {
            let data = std::fs::read(&path).unwrap();
            let version = DocumentHeader::parse(&data).map(|h| h.format_version);
            match ClassCensus::scan(&data) {
                Ok(c) => println!(
                    "{} {version:?} total {} unknown {:?} {:?}",
                    path.display(),
                    c.total(),
                    c.unknown_tags(),
                    c.by_frequency()
                ),
                Err(e) => println!("{} {version:?} error {e}", path.display()),
            }
        }
    }
}
