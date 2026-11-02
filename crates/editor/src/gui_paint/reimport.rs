//! File > Re-import from source: imports an imported document's `.spp` again with the choices it was imported
//! with (its import origin, [`PainterOrigin`]), so layers newer importer support can compute lose their locks.
//!
//! The steps: a confirmation ([`ReimportPrompt`]: edits since the import don't carry over, the result opens as a
//! new unsaved document), picking the `.spp` if it moved, the unsaved-changes question when the open document has
//! some, the usual import review with the earlier choices ticked, the import; once the new document is open the
//! log gets one line on what changed in the locks ([`LockDiff`], the same as `mcsub reimport`).

use super::PaintMode;
use super::dialogs::reimport::{Choice, ReimportPrompt};
use super::import_review::ImportReview;
use super::unsaved::Then;
use eframe::egui;
use materialcraft::diag::{Area, SinkExt};
use materialcraft::paint::{Document, ImportOrigin};
use materialcraft_substance::tools::lock_diff::LockDiff;
use materialcraft_substance::translate::PainterOrigin;
use materialcraft_substance::translate::blocker_summary::BlockerSummary;
use materialcraft_substance::translate::layers::replay::ReplayChoice;
use materialcraft_substance::translate::origin::ORIGIN_KIND;
use materialcraft_substance::translate::replacements::{Choice as PlanChoice, ProceduralPlan};
use std::path::{Path, PathBuf};

/// Whether File > Re-import from source applies to a document with this origin, and if not, why.
pub fn availability(origin: Option<&ImportOrigin>) -> Result<(), &'static str> {
    match origin {
        Some(o) if o.kind == ORIGIN_KIND => Ok(()),
        Some(_) => Err("This document was not imported from a Substance Painter project."),
        None => Err("Only for documents imported from a Substance Painter project (imported with this version or later)."),
    }
}

/// A re-import on its way: the question, then the earlier locks waiting for the new document.
#[derive(Default)]
pub struct Reimports {
    prompt: Option<(ReimportPrompt, ProceduralPlan, ReplayChoice)>,
    /// The re-imported source and the locks of the document it replaces, compared once the new one is open.
    waiting: Option<(PathBuf, BlockerSummary, String)>,
}

impl PaintMode {
    /// File > Re-import from source: asks first.
    pub(super) fn ask_reimport(&mut self) {
        let Some(doc) = &self.document else { return };
        let origin = match PainterOrigin::of(doc.doc()) {
            Ok(Some(o)) => o,
            Ok(None) => return self.diag.info(Area::Import, "This document has no import origin to re-import from."),
            Err(e) => return self.diag.warn(Area::Import, format!("Cannot re-import: {e}.")),
        };
        let state = origin.changed(&origin.source);
        let prompt = ReimportPrompt {
            name: doc.name(),
            source: origin.source.clone(),
            missing: state.is_none(),
            changed: state == Some(true),
            choices: describe(&origin.options.plan),
        };
        self.reimports.prompt = Some((prompt, origin.options.plan, origin.options.replay));
    }

    /// Shows the question and starts the re-import on the answer.
    pub(super) fn poll_reimport(&mut self, ctx: &egui::Context) {
        let Some((prompt, _, _)) = &self.reimports.prompt else { return };
        let Some(choice) = prompt.show(ctx) else { return };
        let Some((prompt, plan, replay)) = self.reimports.prompt.take() else { return };
        let source = match choice {
            Choice::Cancel => return,
            Choice::Reimport => prompt.source,
            Choice::PickSource => {
                let mut dialog = rfd::FileDialog::new().add_filter("Substance 3D Painter project", &["spp"]);
                if let Some(dir) = prompt.source.parent().filter(|d| d.is_dir()) {
                    dialog = dialog.set_directory(dir);
                }
                match dialog.pick_file() {
                    Some(p) => p,
                    None => return,
                }
            }
        };
        let locks = self.document.as_ref().map(|d| BlockerSummary::of(d.doc())).unwrap_or_default();
        self.reimports.waiting = Some((source.clone(), locks, prompt.name));
        self.guard(Then::Reimport(source, plan, replay));
    }

    /// Starts the import review of `source` with `plan`'s and `replay`'s choices ticked (after the unsaved-changes question).
    pub(super) fn start_reimport(&mut self, source: PathBuf, plan: ProceduralPlan, replay: ReplayChoice) {
        self.diag.info(Area::Import, format!("Re-importing {}", source.display()));
        let user = self.tray.user_library().map(Path::to_path_buf);
        self.review = Some(ImportReview::start_with(source, self.tray.material_folders(), user, plan, replay));
    }

    /// The re-import did not happen (cancelled, failed): nothing to compare.
    pub(super) fn forget_reimport(&mut self, source: &Path) {
        if self.reimports.waiting.as_ref().is_some_and(|(s, _, _)| s == source) {
            self.reimports.waiting = None;
        }
    }

    /// The log line on what changed in the locks, when `doc` is the re-import that was waited for.
    pub(super) fn reimport_outcome(&mut self, source: &Path, doc: &Document) -> Option<String> {
        let (_, before, name) = self.reimports.waiting.take_if(|(s, _, _)| s == source)?;
        Some(format!("Re-imported \"{name}\": {}.", LockDiff::since(&before, doc).headline()))
    }
}

/// The plan in a few words: `procedurals: Painter's result, 2 replaced; 1 material picked`.
fn describe(plan: &ProceduralPlan) -> String {
    let default = match plan.default {
        PlanChoice::Keep => "Painter's result",
        PlanChoice::Replace => "MaterialCraft's replacements",
    };
    let differ = plan.choices.iter().filter(|(_, c)| *c != plan.default).count();
    let mut text = format!("procedurals: {default}");
    if differ > 0 {
        text.push_str(&format!(", {differ} chosen otherwise"));
    }
    match plan.materials.len() {
        0 => {}
        1 => text.push_str("; 1 material picked"),
        n => text.push_str(&format!("; {n} materials picked")),
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_painter_imports_can_be_reimported() {
        assert!(availability(None).is_err());
        assert!(availability(Some(&ImportOrigin::new("other", "/a"))).is_err());
        assert!(availability(Some(&ImportOrigin::new(ORIGIN_KIND, "/a.spp"))).is_ok());
    }

    #[test]
    fn plans_are_described_briefly() {
        assert_eq!(describe(&ProceduralPlan::default()), "procedurals: Painter's result");
        let plan = ProceduralPlan::all(PlanChoice::Replace).with("mg_dirt", PlanChoice::Keep).with_material("rust", Some("materials/rust.mca"));
        assert_eq!(describe(&plan), "procedurals: MaterialCraft's replacements, 1 chosen otherwise; 1 material picked");
    }
}
