//! Runs `inspect` over every `.spp` below a folder and condenses each result into one table row.

use super::report::{InspectOptions, ProjectReport};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

/// One file's result in the corpus table.
#[derive(Debug, Clone)]
pub struct CorpusRow {
    pub path: PathBuf,
    pub outcome: Result<RowFacts, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowFacts {
    pub file_bytes: u64,
    pub version: Option<String>,
    pub meshes: Option<usize>,
    pub texture_sets: Option<usize>,
    /// Images whose header decoded, out of all images.
    pub images: Option<(usize, usize)>,
    pub cache_entries: Option<usize>,
    pub document_objects: Option<usize>,
    pub failed_sections: usize,
    /// (passed, checked) over images and cache entries; `None` unless pixels were verified.
    pub pixels: Option<(usize, usize)>,
}

impl RowFacts {
    /// Every section decoded and, if pixels were verified, every pixel block too.
    pub fn is_clean(&self) -> bool {
        self.failed_sections == 0 && self.pixels.is_none_or(|(passed, checked)| passed == checked)
    }
}

impl RowFacts {
    fn from_report(r: &ProjectReport) -> RowFacts {
        let settings = r.settings.as_ref().ok().and_then(|s| s.as_ref());
        RowFacts {
            file_bytes: r.file_bytes,
            version: settings.and_then(|s| s.last_saved_with).map(|v| v.to_string()),
            meshes: r.mesh.as_ref().ok().map(|m| m.meshes.len()),
            texture_sets: r.mesh.as_ref().ok().map(|m| m.materials.len()),
            images: r.resources.as_ref().ok().map(|s| {
                let total = s
                    .kinds
                    .rows()
                    .iter()
                    .find(|(k, ..)| *k == "image")
                    .map_or(0, |(_, n, _)| *n);
                (total - s.unreadable_headers.len(), total)
            }),
            cache_entries: r.cache.as_ref().ok().map(|c| c.channels.total()),
            document_objects: r.document.as_ref().ok().map(|d| d.total()),
            failed_sections: r.failed_sections(),
            pixels: pixel_totals(r),
        }
    }
}

fn pixel_totals(r: &ProjectReport) -> Option<(usize, usize)> {
    let checks = [
        r.resources.as_ref().ok().and_then(|s| s.pixels.as_ref()),
        r.cache.as_ref().ok().and_then(|c| c.pixels.as_ref()),
    ];
    checks.into_iter().flatten().fold(None, |acc, v| {
        let (p, c) = acc.unwrap_or((0, 0));
        Some((p + v.passed, c + v.checked()))
    })
}

pub struct Corpus {
    files: Vec<PathBuf>,
}

impl Corpus {
    /// Finds `.spp` files below `root`, sorted. Unreadable folders are skipped.
    pub fn discover(root: &Path) -> Corpus {
        let mut files = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(read) = std::fs::read_dir(&dir) else { continue };
            for entry in read.flatten() {
                let path = entry.path();
                match entry.file_type() {
                    Ok(t) if t.is_dir() => stack.push(path),
                    Ok(t) if t.is_file() && is_spp(&path) => files.push(path),
                    // a folder of links to chosen projects (a corpus list) counts the files linked to
                    Ok(t) if t.is_symlink() && path.is_file() && is_spp(&path) => files.push(path),
                    _ => {}
                }
            }
        }
        files.sort();
        Corpus { files }
    }

    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// Inspects each file; `progress` is called before each one with (index, total, path).
    /// A panic inside a decoder is caught and recorded as that file's failure.
    pub fn run(&self, options: InspectOptions, mut progress: impl FnMut(usize, usize, &Path)) -> Vec<CorpusRow> {
        let total = self.files.len();
        self.files
            .iter()
            .enumerate()
            .map(|(i, path)| {
                progress(i, total, path);
                let result = panic::catch_unwind(AssertUnwindSafe(|| ProjectReport::build(path, options)));
                let outcome = match result {
                    Ok(Ok(report)) => Ok(RowFacts::from_report(&report)),
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(_) => Err("decoder panicked".to_string()),
                };
                CorpusRow {
                    path: path.clone(),
                    outcome,
                }
            })
            .collect()
    }
}

fn is_spp(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("spp"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(failed_sections: usize, pixels: Option<(usize, usize)>) -> RowFacts {
        RowFacts {
            file_bytes: 0,
            version: None,
            meshes: None,
            texture_sets: None,
            images: None,
            cache_entries: None,
            document_objects: None,
            failed_sections,
            pixels,
        }
    }

    #[test]
    fn a_failed_pixel_block_makes_a_row_unclean() {
        assert!(facts(0, None).is_clean());
        assert!(facts(0, Some((5, 5))).is_clean());
        assert!(!facts(0, Some((4, 5))).is_clean());
        assert!(!facts(1, None).is_clean());
    }

    #[cfg(unix)]
    #[test]
    fn discover_follows_links_to_projects() {
        let dir = std::env::temp_dir().join(format!("mcsub-corpus-links-{}", std::process::id()));
        let (real, list) = (dir.join("real"), dir.join("list"));
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(&list).unwrap();
        std::fs::write(real.join("a.spp"), b"x").unwrap();
        std::fs::write(real.join("b.txt"), b"x").unwrap();
        std::os::unix::fs::symlink(real.join("a.spp"), list.join("01_a.spp")).unwrap();
        std::os::unix::fs::symlink(real.join("b.txt"), list.join("02_b.spp")).unwrap();
        std::os::unix::fs::symlink(real.join("missing.spp"), list.join("03_c.spp")).unwrap();
        let found = Corpus::discover(&list).files().to_vec();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found, vec![list.join("01_a.spp"), list.join("02_b.spp")]);
    }
}
