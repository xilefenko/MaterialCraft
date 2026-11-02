//! Opening, replacing and closing the document of the Paint screen (asking about unsaved changes first goes through
//! [`super::unsaved`]).

use super::document_state::OpenDocument;
use super::import_review::{ImportReview, ReviewOutcome};
use super::loader::{Loaded, Loader};
use super::unsaved::Then;
use super::{PaintMode, Pick};
use eframe::egui;
use materialcraft::diag::{Area, SinkExt};
use materialcraft::paint::diag_entries;
use std::path::PathBuf;

impl PaintMode {
    /// Opens the New project dialog (from outside the Paint screen, e.g. the start screen); with unsaved changes
    /// it asks first, like File > New project.
    pub fn new_project(&mut self) {
        self.guard(Then::NewProject);
    }

    /// Shows a file dialog and opens the chosen file. Returns false if the user cancelled. With unsaved changes it
    /// first asks whether to save them (on the Paint screen, so it returns true: show that screen).
    pub fn pick(&mut self, pick: Pick) -> bool {
        if self.is_saving() || self.document.as_ref().is_some_and(|d| d.is_dirty()) {
            self.guard(Then::Open(pick));
            return true;
        }
        self.pick_file(pick)
    }

    /// [`Self::pick`] without asking about unsaved changes.
    pub(super) fn pick_file(&mut self, pick: Pick) -> bool {
        let dialog = match pick {
            Pick::Any => rfd::FileDialog::new()
                .add_filter("Paint document or Substance Painter project", &["mcpaint", "spp"])
                .add_filter("MaterialCraft paint document", &["mcpaint"]),
            Pick::Substance => rfd::FileDialog::new().add_filter("Substance 3D Painter project", &["spp"]),
        };
        match dialog.pick_file() {
            Some(p) => {
                self.open_reviewed(p);
                true
            }
            None => false,
        }
    }

    /// Starts loading `path` in the background (the current document stays until it is done).
    pub fn open(&mut self, path: PathBuf) {
        self.diag.info(Area::Project, format!("Opening {}", path.display()));
        self.loader = Some(Loader::start(path));
    }

    /// Like [`PaintMode::open`], but a `.spp` first goes through the import review (which Substance
    /// procedurals become MaterialCraft generators, which material fills become materials from the tray).
    pub fn open_reviewed(&mut self, path: PathBuf) {
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("spp")) {
            return self.open(path);
        }
        self.diag.info(Area::Import, format!("Analysing {}", path.display()));
        let user = self.tray.user_library().map(std::path::Path::to_path_buf);
        self.review = Some(ImportReview::start(path, self.tray.material_folders(), user));
    }

    /// Advances the import review and starts the import once the user decided.
    pub(super) fn poll_review(&mut self, ctx: &egui::Context) {
        let Some(review) = self.review.as_mut() else { return };
        match review.ui(ctx) {
            ReviewOutcome::Pending => {}
            ReviewOutcome::Cancelled => {
                self.diag.info(Area::Import, format!("Import of {} cancelled.", review.source().display()));
                let source = review.source().to_path_buf();
                self.review = None;
                self.forget_reimport(&source);
            }
            ReviewOutcome::Import(plan, to_library) => {
                let path = review.source().to_path_buf();
                let library = review.library().to_vec();
                self.review = None;
                if let Some(request) = to_library {
                    self.diag.info(Area::Assets, format!("Adding {} item(s) of {} to {}", request.keys.len(), path.display(), request.library.display()));
                    self.library_imports.start(path.clone(), request);
                }
                self.diag.info(Area::Project, format!("Opening {}", path.display()));
                self.loader = Some(Loader::start_with(path, plan, library));
            }
        }
    }

    pub(super) fn poll_loader(&mut self) {
        let Some(result) = self.loader.as_mut().and_then(|l| l.poll()) else {
            return;
        };
        let path = self.loader.take().map(|l| l.source().to_path_buf()).unwrap_or_default();
        let source = path.display().to_string();
        match result {
            Ok(loaded) => {
                let reimported = self.reimport_outcome(&loaded.source, &loaded.document);
                self.show(loaded);
                if let Some(line) = reimported {
                    self.diag.info(Area::Import, line);
                }
            }
            Err(e) => {
                self.forget_reimport(&path);
                self.diag.error(area_of_load(&source), format!("{source}: {e}"));
                if let Some(shot) = &self.shot {
                    shot.abort(&format!("{source}: {e}"));
                }
            }
        }
    }

    /// Replaces the current document with `loaded`.
    pub(super) fn show(&mut self, loaded: Loaded) {
        for note in &loaded.notes {
            self.diag.info(Area::Project, note.clone());
        }
        let mut doc = OpenDocument::new(loaded.document, loaded.source, loaded.summary);
        let d = doc.doc();
        self.diag.info(Area::Project, format!("Opened {}: {} texture sets, {} mesh parts.", doc.source().display(), d.texture_sets.len(), d.mesh.parts.len()));
        if let Some(report) = d.import_report.as_ref().filter(|_| doc.summary.is_some()) {
            // The import report panel keeps the details; the log gets its summary line (LG-12).
            self.diag.emit(diag_entries::import_summary(report, &doc.source().display().to_string(), doc.summary.as_deref()));
        }
        self.layers.reset();
        // the quick mask and the tools' caches belong to the document that was open
        self.painting.forget_document();
        self.scene.set_tint(None);
        self.scene.rebuild(&mut doc, self.display);
        self.open_view(doc.doc().view.as_ref(), doc.summary.is_some());
        self.document = Some(doc);
    }

    /// Shows a document's saved view, or for a document without one the user's defaults for new documents. A freshly
    /// imported project gets the user's defaults with the cameras it brought.
    fn open_view(&mut self, saved: Option<&materialcraft::paint::saved_view::SavedView>, imported: bool) {
        let project = self.shell.settings.get().display.project.clone();
        let (state, _) = self.viewport.settings_mut();
        match saved {
            Some(v) if imported => {
                super::display_prefs::apply_project(state, &project);
                state.apply_cameras(v);
            }
            Some(v) => state.apply_saved(v),
            None => super::display_prefs::apply_project(state, &project),
        }
    }

    pub(super) fn close(&mut self) {
        self.scene.clear(self.viewport.gpu());
        self.layers.reset();
        self.document = None;
        self.diag.info(Area::Project, "Document closed.");
    }
}

/// Failed loads of Substance projects are import problems; anything else is a project problem.
fn area_of_load(source: &str) -> Area {
    if source.to_ascii_lowercase().ends_with(".spp") { Area::Import } else { Area::Project }
}
