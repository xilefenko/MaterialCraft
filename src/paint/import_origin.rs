//! [`ImportOrigin`]: where an imported document came from and how it was imported, so it can be imported again
//! from the same source with the same choices (for example once more of it can be computed).
//!
//! The paint model knows no importer: `kind` names the program the source belongs to and `options` is whatever
//! that importer needs to repeat the import, both opaque here. Plain data, no I/O: the importer fills in the
//! source's size and modification time, and whoever re-imports compares them with the file as it is now.

use crate::json::Value;

/// The source file and the options of an import.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportOrigin {
    /// What the source is, defined by the importer (e.g. `substance-painter`).
    pub kind: String,
    /// The importer and its version (e.g. `materialcraft-substance 0.1.0`); may be empty.
    pub importer: String,
    /// The source file's absolute path, as the importer saw it.
    pub path: String,
    /// The source's size in bytes when it was imported, if known.
    pub size: Option<u64>,
    /// The source's modification time when it was imported, in milliseconds since the Unix epoch, if known.
    pub modified_ms: Option<u64>,
    /// What the importer needs to repeat the import (its choices); opaque here. `Null` for none.
    pub options: Value,
}

impl ImportOrigin {
    pub fn new(kind: impl Into<String>, path: impl Into<String>) -> ImportOrigin {
        ImportOrigin { kind: kind.into(), path: path.into(), ..ImportOrigin::default() }
    }

    /// Whether a source of `size` bytes modified at `modified_ms` differs from the one imported. Unknown values
    /// (on either side) don't count as a difference.
    pub fn changed(&self, size: Option<u64>, modified_ms: Option<u64>) -> bool {
        let differs = |a: Option<u64>, b: Option<u64>| matches!((a, b), (Some(a), Some(b)) if a != b);
        differs(self.size, size) || differs(self.modified_ms, modified_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_compares_only_what_is_known() {
        let origin = ImportOrigin { size: Some(10), modified_ms: Some(1000), ..ImportOrigin::new("x", "/a.x") };
        assert!(!origin.changed(Some(10), Some(1000)));
        assert!(origin.changed(Some(11), Some(1000)));
        assert!(origin.changed(Some(10), Some(2000)));
        assert!(!origin.changed(None, None));
        assert!(!ImportOrigin::new("x", "/a.x").changed(Some(1), Some(2)));
    }
}
