//! Runs the record decoders over real Painter projects. Skipped unless `MC_SUBSTANCE_CORPUS` names a folder
//! (searched recursively for `.spp` files). No bytes from those files are kept in the repository.

use spp_reader::container::hdf5::{EntryKind, Hdf5File};
use spp_reader::decode::image::ImageHeader;
use spp_reader::decode::resource_toc::ResourceToc;
use spp_reader::decode::settings::ProjectSettings;
use spp_reader::decode::texture_cache::CacheHeader;
use std::path::{Path, PathBuf};

/// Enough for every header seen so far (they are under 500 bytes).
const PREFIX: usize = 4096;

fn spp_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            spp_files(&path, out);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("spp")) {
            out.push(path);
        }
    }
}

/// Embedded files that are not image records (7z `.sbsar`, PopcornFX `CPar…`, shaders, presets).
fn is_image_resource(path: &str, prefix: &[u8]) -> bool {
    let other_ext = [".sbsar", ".shader", ".spexp", ".s4sbs", ".pkfx", ".hfx"];
    !path.starts_with("/resources/.alg_meta/")
        && !other_ext.iter().any(|e| path.ends_with(e))
        && !prefix.starts_with(b"7z")
        && !prefix.starts_with(b"CPar")
}

fn check_project(path: &Path) -> Vec<String> {
    let mut failures = Vec::new();
    let file = Hdf5File::open(path).expect("open");
    for entry in file.entries().expect("entries") {
        let EntryKind::Dataset { .. } = entry.kind else {
            continue;
        };
        let p = entry.path.as_str();
        let result = if p == "/projectsettings.ini" {
            ProjectSettings::parse(&file.read(p).expect("read")).map(drop)
        } else if p == "/resources.toc" {
            ResourceToc::parse(&file.read(p).expect("read")).map(drop)
        } else if let Some(rest) = p.strip_prefix("/resources/") {
            let prefix = file.read_prefix(p, PREFIX).expect("read");
            if !is_image_resource(p, &prefix) || rest.contains('/') {
                continue;
            }
            ImageHeader::parse(&prefix).map(drop)
        } else if p.starts_with("/texture-cache/") && !p.ends_with("/list") && !p.ends_with("/header") {
            CacheHeader::parse(&file.read_prefix(p, PREFIX).expect("read")).map(drop)
        } else {
            continue;
        };
        if let Err(e) = result {
            failures.push(format!("{}: {p}: {e}", path.display()));
        }
    }
    failures
}

#[test]
fn decodes_records_of_every_corpus_project() {
    let Some(dir) = std::env::var_os("MC_SUBSTANCE_CORPUS") else {
        eprintln!("MC_SUBSTANCE_CORPUS not set; skipping");
        return;
    };
    let mut files = Vec::new();
    spp_files(Path::new(&dir), &mut files);
    let failures: Vec<String> = files.iter().flat_map(|f| check_project(f)).collect();
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    eprintln!("checked {} projects", files.len());
}
