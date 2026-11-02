//! The HDF5 reader against `fixtures/small.h5` (made by `fixtures/make_fixture.c` with libhdf5) and,
//! optionally, against a folder of real `.spp` files named by `MC_SUBSTANCE_CORPUS`.

use spp_reader::container::hdf5::{Entry, EntryKind, Hdf5Error, Hdf5File};
use std::path::{Path, PathBuf};

fn fixture() -> Hdf5File {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small.h5");
    Hdf5File::open(&path).expect("fixture opens")
}

/// The bytes `make_fixture.c` writes for `seed`.
fn pattern(seed: u64, len: u64) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + seed * 7 + (i >> 8)) % 251) as u8).collect()
}

fn group(path: &str) -> Entry {
    Entry {
        path: path.into(),
        kind: EntryKind::Group,
    }
}

fn dataset(path: &str, len: u64) -> Entry {
    Entry {
        path: path.into(),
        kind: EntryKind::Dataset { len },
    }
}

#[test]
fn lists_every_object_depth_first_in_name_order() {
    let mut expected = vec![
        group("/alpha"),
        group("/alpha/beta"),
        dataset("/alpha/beta/chunked.bin", 76700),
        dataset("/alpha/compact.bin", 50),
        dataset("/alpha/contiguous.bin", 1000),
        dataset("/alpha/sparse.bin", 2048),
        dataset("/empty.bin", 0),
        group("/many"),
    ];
    expected.extend((0..300).map(|i| dataset(&format!("/many/item{i:03}"), 3 + i % 5)));
    assert_eq!(fixture().entries().unwrap(), expected);
}

#[test]
fn reads_a_multi_chunk_dataset_with_a_partial_last_chunk() {
    assert_eq!(fixture().read("/alpha/beta/chunked.bin").unwrap(), pattern(1, 76700));
}

#[test]
fn reads_contiguous_compact_and_empty_datasets() {
    let file = fixture();
    assert_eq!(file.read("/alpha/contiguous.bin").unwrap(), pattern(2, 1000));
    assert_eq!(file.read("/alpha/compact.bin").unwrap(), pattern(3, 50));
    assert_eq!(file.read("/empty.bin").unwrap(), Vec::<u8>::new());
    assert_eq!(file.read("/many/item123").unwrap(), pattern(223, 3 + 123 % 5));
}

#[test]
fn unwritten_chunks_read_as_zeros() {
    let mut expected = vec![0; 2048];
    expected[1024..1100].copy_from_slice(&pattern(5, 76));
    assert_eq!(fixture().read("/alpha/sparse.bin").unwrap(), expected);
}

#[test]
fn reads_prefixes() {
    let file = fixture();
    let full = pattern(1, 76700);
    for len in [0, 1, 255, 256, 257, 5000, 76699, 76700, 1 << 20] {
        let prefix = file.read_prefix("/alpha/beta/chunked.bin", len).unwrap();
        assert_eq!(prefix, full[..len.min(full.len())], "prefix of {len} bytes");
    }
    assert_eq!(file.read_prefix("/alpha/contiguous.bin", 10).unwrap(), pattern(2, 10));
}

#[test]
fn paths_may_omit_the_leading_slash() {
    assert_eq!(fixture().read("alpha/compact.bin").unwrap(), pattern(3, 50));
}

#[test]
fn reports_missing_objects_and_groups() {
    let file = fixture();
    assert!(matches!(file.read("/alpha/nope"), Err(Hdf5Error::NotFound(_))));
    assert!(matches!(
        file.read("/alpha/compact.bin/deeper"),
        Err(Hdf5Error::NotFound(_))
    ));
    assert!(matches!(file.read("/alpha/beta"), Err(Hdf5Error::NotADataset(_))));
    assert!(matches!(file.read_prefix("/", 4), Err(Hdf5Error::NotADataset(_))));
}

#[test]
fn can_be_shared_between_threads() {
    let file = fixture();
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| assert_eq!(file.read("/alpha/beta/chunked.bin").unwrap(), pattern(1, 76700)));
        }
    });
}

#[test]
fn rejects_files_that_are_not_hdf5() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/make_fixture.c");
    assert!(matches!(Hdf5File::open(&path), Err(Hdf5Error::NotHdf5)));
}

/// Opens every `.spp` below `$MC_SUBSTANCE_CORPUS`, lists it and reads the start of every dataset.
#[test]
fn corpus_files_list_without_errors() {
    let Some(root) = std::env::var_os("MC_SUBSTANCE_CORPUS") else {
        eprintln!("MC_SUBSTANCE_CORPUS not set; skipping corpus test");
        return;
    };
    let mut files = Vec::new();
    find_spp_files(Path::new(&root), &mut files);
    let mut failures = Vec::new();
    for path in &files {
        if let Err(e) = check_corpus_file(path) {
            failures.push(format!("{}: {e}", path.display()));
        }
    }
    eprintln!("checked {} .spp files, {} failed", files.len(), failures.len());
    assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
}

fn check_corpus_file(path: &Path) -> Result<(), Hdf5Error> {
    let file = Hdf5File::open(path)?;
    for entry in file.entries()? {
        if let EntryKind::Dataset { len } = entry.kind {
            let prefix = file.read_prefix(&entry.path, 64)?;
            assert_eq!(prefix.len() as u64, len.min(64), "{}", entry.path);
        }
    }
    Ok(())
}

fn find_spp_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read_dir) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = read_dir.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            find_spp_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("spp")) {
            out.push(path);
        }
    }
}
