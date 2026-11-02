//! Renders corpus rows as a table with a totals line.

use super::corpus::{CorpusRow, RowFacts};
use super::units::megabytes;
use std::fmt::{self, Display, Formatter};
use std::path::Path;

pub struct CorpusTable<'a> {
    pub rows: &'a [CorpusRow],
    /// Shown paths are made relative to this folder.
    pub root: &'a Path,
}

impl Display for CorpusTable<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{:<52} {:>10} {:>8} {:>6} {:>5} {:>9} {:>6} {:>9}  status",
            "file", "size", "version", "meshes", "sets", "images", "cache", "objects"
        )?;
        for row in self.rows {
            let name = row
                .path
                .strip_prefix(self.root)
                .unwrap_or(&row.path)
                .display()
                .to_string();
            match &row.outcome {
                Ok(facts) => writeln!(f, "{} {}", clip(&name, 52), FactsCells(facts))?,
                Err(e) => writeln!(f, "{} FAILED: {e}", clip(&name, 52))?,
            }
        }
        let opened = self.rows.iter().filter(|r| r.outcome.is_ok()).count();
        let clean = self
            .rows
            .iter()
            .filter(|r| matches!(&r.outcome, Ok(x) if x.is_clean()))
            .count();
        let verified = self
            .rows
            .iter()
            .any(|r| matches!(&r.outcome, Ok(x) if x.pixels.is_some()));
        let what = if verified {
            "every section and pixel block decoded"
        } else {
            "every section decoded"
        };
        writeln!(
            f,
            "\n{} files · {} opened · {} with {what}",
            self.rows.len(),
            opened,
            clean
        )
    }
}

struct FactsCells<'a>(&'a RowFacts);

impl Display for FactsCells<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let x = self.0;
        let n = |v: Option<usize>| v.map_or("-".to_string(), |v| v.to_string());
        let images = x.images.map_or("-".to_string(), |(ok, all)| format!("{ok}/{all}"));
        let mut problems = Vec::new();
        if x.failed_sections > 0 {
            problems.push(format!("{} section(s) failed", x.failed_sections));
        }
        if let Some((passed, checked)) = x.pixels.filter(|(p, c)| p != c) {
            problems.push(format!("{} of {checked} pixel blocks failed", checked - passed));
        }
        let status = if problems.is_empty() {
            "ok".to_string()
        } else {
            problems.join(", ")
        };
        write!(
            f,
            "{:>10} {:>8} {:>6} {:>5} {:>9} {:>6} {:>9}  {status}",
            megabytes(x.file_bytes),
            x.version.as_deref().unwrap_or("-"),
            n(x.meshes),
            n(x.texture_sets),
            images,
            n(x.cache_entries),
            n(x.document_objects)
        )
    }
}

fn clip(s: &str, width: usize) -> String {
    let count = s.chars().count();
    if count <= width {
        format!("{s:<width$}")
    } else {
        let tail: String = s.chars().skip(count - (width - 1)).collect();
        format!("…{tail}")
    }
}
