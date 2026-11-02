//! The import origin of a Painter import: which `.spp` a document came from and the [`ImportOptions`] it was
//! imported with, stored in [`Document::import_origin`] so it can be imported again the same way.
//!
//! The options are kept as JSON in the document (the paint model treats them as opaque):
//!
//! ```json
//! {"plan": {"default": "keep", "procedurals": {"mg_dirt": "replace"}, "materials": {…}}, "strokes": "keep"}
//! ```
//!
//! `plan` is a `choices.json` ([`ProceduralPlan`]), `strokes` the [`ReplayChoice`]. The asset library the plan's
//! materials were read from is not part of it: it belongs to the machine, not to the document.

use super::layers::replay::ReplayChoice;
use super::replacements::ProceduralPlan;
use materialcraft::json::Value;
use materialcraft::paint::{Document, ImportOrigin};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// [`ImportOrigin::kind`] of a Painter import.
pub const ORIGIN_KIND: &str = "substance-painter";
/// [`ImportOrigin::importer`]: this crate and its version.
pub const IMPORTER: &str = concat!("materialcraft-substance ", env!("CARGO_PKG_VERSION"));

/// What the importer takes besides the file: everything needed to repeat an import.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportOptions {
    pub plan: ProceduralPlan,
    pub replay: ReplayChoice,
}

impl ImportOptions {
    pub fn to_json(&self) -> Value {
        Value::object()
            .with("plan", self.plan.to_json())
            .with("strokes", self.replay.id())
    }

    /// Reads [`Self::to_json`]'s form; a missing part takes its default.
    pub fn from_json(v: &Value) -> Result<ImportOptions, String> {
        let plan = match v.get("plan") {
            Some(p) => ProceduralPlan::from_json(p)?,
            None => ProceduralPlan::default(),
        };
        let replay = match v.get("strokes") {
            None => ReplayChoice::default(),
            Some(s) => s
                .as_str()
                .and_then(ReplayChoice::from_id)
                .ok_or("import options: \"strokes\" must be \"keep\" or \"replay\"")?,
        };
        Ok(ImportOptions { plan, replay })
    }
}

/// The source file's size and modification time (milliseconds since the Unix epoch), where they can be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SourceStamp {
    pub size: Option<u64>,
    pub modified_ms: Option<u64>,
}

impl SourceStamp {
    /// The stamp of `path` now; `None` when the file can't be read (missing, no permission).
    pub fn of(path: &Path) -> Option<SourceStamp> {
        let meta = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .and_then(|d| u64::try_from(d.as_millis()).ok());
        Some(SourceStamp {
            size: Some(meta.len()),
            modified_ms,
        })
    }
}

/// The origin of an import of `path` with `options`.
pub fn origin(path: &Path, options: &ImportOptions) -> ImportOrigin {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let stamp = SourceStamp::of(path).unwrap_or_default();
    ImportOrigin {
        kind: ORIGIN_KIND.to_string(),
        importer: IMPORTER.to_string(),
        path: absolute.to_string_lossy().into_owned(),
        size: stamp.size,
        modified_ms: stamp.modified_ms,
        options: options.to_json(),
    }
}

/// A document's Painter origin, read back: the source and the options.
#[derive(Debug, Clone, PartialEq)]
pub struct PainterOrigin {
    pub source: PathBuf,
    pub options: ImportOptions,
    /// The stored size and time, to compare with the file now.
    pub stored: ImportOrigin,
}

impl PainterOrigin {
    /// The Painter origin of `doc`: `Ok(None)` when it has none (not imported, or imported before origins were
    /// kept), an error when it came from another importer or its options can't be read.
    pub fn of(doc: &Document) -> Result<Option<PainterOrigin>, String> {
        let Some(o) = &doc.import_origin else { return Ok(None) };
        if o.kind != ORIGIN_KIND {
            return Err(format!(
                "the document was imported from \"{}\", not a Painter project",
                o.kind
            ));
        }
        let options = if o.options.is_null() {
            ImportOptions::default()
        } else {
            ImportOptions::from_json(&o.options)?
        };
        Ok(Some(PainterOrigin {
            source: PathBuf::from(&o.path),
            options,
            stored: o.clone(),
        }))
    }

    /// Whether `path` (the source, or the file standing in for it) differs from the one imported: `None` when
    /// it can't be read, else whether its size or time changed.
    pub fn changed(&self, path: &Path) -> Option<bool> {
        let now = SourceStamp::of(path)?;
        Some(self.stored.changed(now.size, now.modified_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::replacements::Choice;

    #[test]
    fn options_round_trip_through_json() {
        let options = ImportOptions {
            plan: ProceduralPlan::all(Choice::Replace)
                .with("mg_dirt", Choice::Keep)
                .with_material("rust", Some("materials/rust.mca")),
            replay: ReplayChoice::Replay,
        };
        let v = options.to_json();
        assert_eq!(v.get("strokes").and_then(Value::as_str), Some("replay"));
        assert_eq!(ImportOptions::from_json(&v), Ok(options));
        assert_eq!(ImportOptions::from_json(&Value::object()), Ok(ImportOptions::default()));
        assert!(ImportOptions::from_json(&Value::object().with("strokes", "maybe")).is_err());
    }

    #[test]
    fn origin_of_a_file_and_back() {
        let dir = std::env::temp_dir().join(format!("mc-origin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.spp");
        std::fs::write(&file, b"12345").unwrap();
        let options = ImportOptions {
            plan: ProceduralPlan::all(Choice::Replace),
            replay: ReplayChoice::Keep,
        };
        let mut doc = Document {
            import_origin: Some(origin(&file, &options)),
            ..Document::default()
        };
        let o = doc.import_origin.as_ref().unwrap();
        assert_eq!((o.kind.as_str(), o.size), (ORIGIN_KIND, Some(5)));
        assert!(Path::new(&o.path).is_absolute());
        let back = PainterOrigin::of(&doc).unwrap().unwrap();
        assert_eq!((back.source.as_path(), &back.options), (file.as_path(), &options));
        assert_eq!(back.changed(&file), Some(false));
        std::fs::write(&file, b"123456").unwrap();
        assert_eq!(back.changed(&file), Some(true));
        std::fs::remove_file(&file).unwrap();
        assert_eq!(back.changed(&file), None);
        doc.import_origin.as_mut().unwrap().kind = "other".into();
        assert!(PainterOrigin::of(&doc).is_err());
        assert_eq!(PainterOrigin::of(&Document::default()), Ok(None));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
