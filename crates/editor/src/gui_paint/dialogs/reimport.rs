//! "Re-import from source": confirms importing a document's `.spp` again, before anything happens. It says what
//! the re-import does and does not do: the earlier choices are repeated, edits made since the import don't carry
//! over, and the result opens as a new, unsaved document (the open document's file is not touched).

use eframe::egui::{self, Id, RichText};
use std::path::PathBuf;

/// What the user chose.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Choice {
    /// Re-import from the stored source.
    Reimport,
    /// The source is missing (or the user wants another file): pick the `.spp` first.
    PickSource,
    Cancel,
}

/// The question for one document.
pub struct ReimportPrompt {
    /// The open document's name.
    pub name: String,
    pub source: PathBuf,
    /// The source can't be read where it was.
    pub missing: bool,
    /// The source's size or time differ from the import's.
    pub changed: bool,
    /// The earlier choices in a few words.
    pub choices: String,
}

impl ReimportPrompt {
    /// Draws the dialog; `Some` once the user chose. Escape or a click beside it cancels.
    pub fn show(&self, ctx: &egui::Context) -> Option<Choice> {
        let mut choice = None;
        let modal = egui::Modal::new(Id::new("paint_reimport_prompt")).show(ctx, |ui| {
            ui.set_max_width(440.0);
            ui.heading("Re-import from source");
            ui.label(format!("Imports the Substance Painter project of \"{}\" again, with the choices it was imported with ({}).", self.name, self.choices));
            ui.label(RichText::new(self.source.display().to_string()).monospace());
            if self.missing {
                ui.colored_label(ui.visuals().warn_fg_color, "The project is no longer there. Choose where it is now.");
            } else if self.changed {
                ui.colored_label(ui.visuals().warn_fg_color, "The project changed since it was imported: the result follows the project as it is now.");
            }
            ui.add_space(4.0);
            ui.label("Edits made since the import will not carry over.");
            ui.label(format!("The result opens as a new, unsaved document; \"{}\" stays as it was saved.", self.name));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if !self.missing && ui.button("Re-import…").clicked() {
                    choice = Some(Choice::Reimport);
                }
                let pick = if self.missing { "Choose the .spp…" } else { "Another .spp…" };
                if ui.button(pick).clicked() {
                    choice = Some(Choice::PickSource);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
        });
        if choice.is_none() && modal.should_close() {
            choice = Some(Choice::Cancel);
        }
        choice
    }
}
