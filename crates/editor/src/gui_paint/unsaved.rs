//! Keeping unsaved changes: closing the document, opening another one and quitting first ask "Save changes?"
//! when the document is dirty ([`Then`] is what waits for the answer, or for the save). Quitting is caught in two
//! places: the window's close request while the Paint screen is shown (with Cancel), and the editor's exit hook
//! for any other way out (a native Yes/No question, then a blocking save).

use super::dialogs::unsaved::{Choice, UnsavedPrompt};
use super::saving::{SaveJob, prepare, suggested_path, with_extension};
use super::{PaintMode, Pick};
use eframe::egui::{self, ViewportCommand};
use materialcraft::diag::{Area, SinkExt};
use materialcraft::paint::format;
use materialcraft_substance::translate::layers::replay::ReplayChoice;
use materialcraft_substance::translate::replacements::ProceduralPlan;
use std::path::PathBuf;

/// What happens after the document was saved or its changes were given up.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Then {
    Close,
    /// Pick another file to open.
    Open(Pick),
    /// Open this file (dropped onto the window).
    OpenPath(PathBuf),
    /// Open the New project dialog.
    NewProject,
    /// Re-import this `.spp` with these choices (File > Re-import from source).
    Reimport(PathBuf, ProceduralPlan, ReplayChoice),
    /// Close the window.
    Quit,
}

impl Then {
    /// Completes "Save the changes before …?".
    fn doing(&self) -> &'static str {
        match self {
            Then::Close => "closing it",
            Then::Open(_) | Then::OpenPath(_) => "opening another document",
            Then::Reimport(..) => "re-importing it",
            Then::NewProject => "making a new project",
            Then::Quit => "quitting",
        }
    }
}

/// The save in progress and the question waiting for an answer.
#[derive(Default)]
pub struct Saving {
    pub(super) job: Option<SaveJob>,
    prompt: Option<(UnsavedPrompt, Then)>,
    /// The user agreed to quit: the next close request goes through.
    quit_confirmed: bool,
    /// Close the window at the end of this frame.
    close_window: bool,
}

impl PaintMode {
    /// Does `then` now if nothing would be lost; otherwise asks first (or waits for the running save).
    pub(super) fn guard(&mut self, then: Then) {
        if let Some(job) = &mut self.saving.job {
            self.diag.info(Area::Project, format!("Waiting for the save of {} to finish.", job.path().display()));
            job.then_after(then);
            return;
        }
        match self.document.as_ref().filter(|d| d.is_dirty()) {
            Some(doc) => self.saving.prompt = Some((UnsavedPrompt::new(doc.name(), then.doing()), then)),
            None => self.run(then),
        }
    }

    /// Does `then` without asking.
    pub(super) fn run(&mut self, then: Then) {
        match then {
            Then::Close => {
                self.baking.stop();
                self.close();
            }
            Then::Open(pick) => {
                self.pick_file(pick);
            }
            Then::OpenPath(path) => self.open_reviewed(path),
            Then::NewProject => self.open_new_project(),
            Then::Reimport(source, plan, replay) => self.start_reimport(source, plan, replay),
            Then::Quit => {
                self.saving.quit_confirmed = true;
                self.saving.close_window = true;
            }
        }
    }

    /// Shows the "Save changes?" question and acts on the answer; catches the window's close request.
    pub(super) fn poll_unsaved(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.saving.quit_confirmed {
            let dirty = self.document.as_ref().is_some_and(|d| d.is_dirty());
            if dirty || self.is_saving() {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                self.guard(Then::Quit);
            }
        }
        if let Some((prompt, _)) = &self.saving.prompt
            && let Some(choice) = prompt.show(ctx)
            && let Some((_, then)) = self.saving.prompt.take()
        {
            match choice {
                Choice::Save => self.save(Some(then)),
                Choice::Discard => self.run(then),
                Choice::Cancel => {
                    if then == Then::Quit {
                        self.diag.info(Area::Project, "Quitting cancelled.");
                    }
                }
            }
        }
        if std::mem::take(&mut self.saving.close_window) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    /// The editor is exiting (its exit hook): waits for a running save, and if the document still has unsaved
    /// changes the user did not give up, asks with a native dialog whether to save them and saves on this thread.
    pub(super) fn save_on_exit(&mut self) {
        if let Some(job) = self.saving.job.take() {
            let path = job.path().to_path_buf();
            if let Err(e) = job.wait() {
                self.diag.error(Area::Project, format!("{} was not saved: {e}", path.display()));
            } else if let Some(doc) = &mut self.document {
                doc.saved_to(path);
            }
        }
        if self.saving.quit_confirmed {
            return;
        }
        let Some(doc) = &self.document else { return };
        if !doc.is_dirty() {
            return;
        }
        let answer = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Unsaved changes")
            .set_description(format!("Save the changes to \"{}\" before quitting?", doc.name()))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        if answer != rfd::MessageDialogResult::Yes {
            return;
        }
        let path = match doc.file() {
            Some(p) => Some(p.to_path_buf()),
            None => {
                let suggested = suggested_path(doc.source());
                let mut dialog = rfd::FileDialog::new().add_filter("MaterialCraft paint document", &["mcpaint"]);
                if let Some(name) = suggested.file_name() {
                    dialog = dialog.set_file_name(name.to_string_lossy());
                }
                if let Some(dir) = suggested.parent().filter(|d| d.is_dir()) {
                    dialog = dialog.set_directory(dir);
                }
                dialog.save_file().map(with_extension)
            }
        };
        let Some(path) = path else { return };
        let lookup = self.asset_lookup();
        let Some(doc) = &mut self.document else { return };
        let (mut copy, _) = prepare(doc, lookup);
        copy.view = Some(self.viewport.state().saved());
        match format::save_path(&copy, &path) {
            Ok(()) => {
                doc.saved_to(path.clone());
                self.diag.info(Area::Project, format!("Saved {}.", path.display()));
            }
            Err(e) => {
                doc.save_failed();
                self.diag.error(Area::Project, format!("{} was not saved: {e}", path.display()));
            }
        }
    }
}
