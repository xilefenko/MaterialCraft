//! `mcsub reimport`: imports a document's `.spp` again, from the source and with the options its import origin
//! recorded (either can be overridden), saves the result and compares the locks of both ([`LockDiff`]): what
//! newer importer support unlocked.

use super::lock_diff::LockDiff;
use crate::translate::layers::replay::ReplayChoice;
use crate::translate::origin::{ImportOptions, PainterOrigin};
use crate::translate::replacements::ProceduralPlan;
use crate::translate::{Cancel, Importer};
use materialcraft::material_fill::{FolderResolver, MaterialResolver};
use materialcraft::paint::Document;
use materialcraft::paint::format::{load_path, save_path};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Share of the progress bar for the import; saving takes the rest.
const IMPORT_SHARE: f32 = 0.9;

/// What to re-import and what to change.
#[derive(Debug, Clone, Default)]
pub struct ReimportRequest {
    /// Where the source is now, when it moved (default: the stored path).
    pub source: Option<PathBuf>,
    /// A different plan (default: the stored one).
    pub plan: Option<ProceduralPlan>,
    /// A different stroke replay choice (default: the stored one).
    pub replay: Option<ReplayChoice>,
    /// Asset library folders to embed the plan's materials from (none: not embedded).
    pub library: Vec<PathBuf>,
}

impl ReimportRequest {
    /// The source to import and the options, from `origin` with this request's overrides.
    pub fn resolve(&self, origin: &PainterOrigin) -> (PathBuf, ImportOptions) {
        let source = self.source.clone().unwrap_or_else(|| origin.source.clone());
        let options = ImportOptions {
            plan: self.plan.clone().unwrap_or_else(|| origin.options.plan.clone()),
            replay: self.replay.unwrap_or(origin.options.replay),
        };
        (source, options)
    }
}

/// What a re-import did.
pub struct ReimportSummary {
    pub source: PathBuf,
    pub options: ImportOptions,
    /// Warnings (a changed source, materials not embedded).
    pub notes: Vec<String>,
    pub diff: LockDiff,
}

impl fmt::Display for ReimportSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "source: {}", self.source.display())?;
        writeln!(f, "options: {}", self.options.to_json())?;
        for n in &self.notes {
            writeln!(f, "note: {n}")?;
        }
        writeln!(f)?;
        write!(f, "{}", self.diff)
    }
}

/// The Painter origin of `doc`, or why it can't be re-imported.
pub fn origin_of(doc: &Document) -> Result<PainterOrigin, String> {
    PainterOrigin::of(doc)?.ok_or_else(|| {
        "the document has no import origin (not imported from a .spp, or imported before origins were recorded)".into()
    })
}

/// Checks that `source` can be read and says whether it changed since `origin` was recorded.
pub fn check_source(origin: &PainterOrigin, source: &Path) -> Result<Option<String>, String> {
    match origin.changed(source) {
        None => Err(format!(
            "the source {} cannot be read (moved? pass --source <file.spp>)",
            source.display()
        )),
        Some(true) => Ok(Some(format!(
            "{} changed since the document was imported (size or modification time differ)",
            source.display()
        ))),
        Some(false) => Ok(None),
    }
}

/// Imports `before`'s source again as `request` says; the new document and what happened.
pub fn reimport_document(
    before: &Document,
    request: &ReimportRequest,
    progress: &mut dyn FnMut(f32, &str),
    cancel: Cancel<'_>,
) -> Result<(Document, ReimportSummary), String> {
    let origin = origin_of(before)?;
    let (source, options) = request.resolve(&origin);
    let mut notes: Vec<String> = check_source(&origin, &source)?.into_iter().collect();
    let mut importer = Importer::open(&source)
        .map_err(|e| format!("{}: {e}", source.display()))?
        .with_options(options.clone());
    if !request.library.is_empty() {
        let library: Arc<dyn MaterialResolver> = Arc::new(FolderResolver::new(request.library.iter().cloned()));
        importer = importer.with_library(library);
    } else if !options.plan.materials.is_empty() {
        notes.push("the plan picks materials but no library was given: they are not embedded".into());
    }
    let after = importer
        .run(progress, cancel)
        .map_err(|e| format!("import failed: {e}"))?;
    let diff = LockDiff::of(before, &after);
    Ok((
        after,
        ReimportSummary {
            source,
            options,
            notes,
            diff,
        },
    ))
}

/// Loads `input`, re-imports it as `request` says and saves the result as `output`.
pub fn reimport(
    input: &Path,
    output: &Path,
    request: &ReimportRequest,
    progress: &mut dyn FnMut(f32, &str),
) -> Result<ReimportSummary, String> {
    let before = load_path(input).map_err(|e| format!("{}: {e}", input.display()))?;
    let mut on = |f: f32, m: &str| progress(f * IMPORT_SHARE, m);
    let (after, summary) = reimport_document(&before, request, &mut on, Cancel::NEVER)?;
    progress(IMPORT_SHARE, "saving");
    save_path(&after, output).map_err(|e| format!("save failed: {e}"))?;
    progress(1.0, "done");
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::replacements::Choice;

    #[test]
    fn overrides_replace_the_stored_options() {
        let origin = PainterOrigin {
            source: PathBuf::from("/old/a.spp"),
            options: ImportOptions {
                plan: ProceduralPlan::all(Choice::Keep).with("mg_dirt", Choice::Replace),
                replay: ReplayChoice::Replay,
            },
            stored: Default::default(),
        };
        let (source, options) = ReimportRequest::default().resolve(&origin);
        assert_eq!((source, &options), (PathBuf::from("/old/a.spp"), &origin.options));
        let request = ReimportRequest {
            source: Some("/new/a.spp".into()),
            plan: Some(ProceduralPlan::all(Choice::Replace)),
            ..Default::default()
        };
        let (source, options) = request.resolve(&origin);
        assert_eq!(source, PathBuf::from("/new/a.spp"));
        assert_eq!(options.plan, ProceduralPlan::all(Choice::Replace));
        assert_eq!(options.replay, ReplayChoice::Replay, "kept");
    }

    #[test]
    fn a_document_without_origin_or_source_is_refused() {
        assert!(origin_of(&Document::default()).is_err());
        let origin = PainterOrigin {
            source: PathBuf::from("/nowhere/a.spp"),
            options: ImportOptions::default(),
            stored: Default::default(),
        };
        let e = check_source(&origin, Path::new("/nowhere/a.spp")).unwrap_err();
        assert!(e.contains("--source"), "{e}");
    }
}
