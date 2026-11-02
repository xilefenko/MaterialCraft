//! Builds Painter's model for every `.spp` below `MC_SUBSTANCE_CORPUS` (skipped without it). Every project
//! must build, keep no object raw, and have a layer stack per texture set that reaches the stored layers.

use spp_reader::model::StackSummary;
use spp_reader::project::SppProject;
use std::path::{Path, PathBuf};

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

#[test]
fn every_corpus_project_builds_a_model() {
    let Some(root) = std::env::var_os("MC_SUBSTANCE_CORPUS") else {
        return;
    };
    let mut files = Vec::new();
    spp_files(Path::new(&root), &mut files);
    let mut failures = Vec::new();
    for path in &files {
        let built = SppProject::open(path).and_then(|p| Ok((p.model()?, p.document_census()?)));
        match built {
            Ok(((doc, report), census)) => {
                assert_eq!(report.total(), census.total(), "{}", path.display());
                assert_eq!(report.raw(), 0, "{}: {:?}", path.display(), report.raw_classes());
                let layers = census.count("DataLayerColor") + census.count("DataLayerGroup");
                assert_eq!(StackSummary::of(&doc).layers, layers, "{}", path.display());
            }
            Err(e) => failures.push(format!("{}: {e}", path.display())),
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
