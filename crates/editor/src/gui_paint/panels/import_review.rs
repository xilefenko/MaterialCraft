//! [`ReviewDialog`]: the import review. Per Substance procedural the project uses: MaterialCraft's replacement
//! (editable, with its measured accuracy against Painter, median MAE in 8-bit steps) or Painter's own result
//! (exact look, flattened, not editable). Procedurals without a replacement are listed as kept. Substance
//! material fills get their own section ([`MaterialChoices`]): a material from the tray or Painter's result. What the
//! project brings for the user's own library has the last one ([`LibraryChoices`]): a checkbox per item, unticked.

use super::import_review_library::LibraryChoices;
use super::import_review_materials::MaterialChoices;
use crate::gui_paint::import_review::Findings;
use eframe::egui::{self, RichText};
use materialcraft_substance::tools::analyze::{Analysis, ProceduralSummary};
use materialcraft_substance::translate::library::LibraryCandidates;
use materialcraft_substance::translate::replacements::{Choice, ProceduralPlan};
use std::path::Path;
use std::sync::Arc;

/// What the user decided.
pub enum ReviewResult {
    /// Import with `plan`; write the `library` candidates (their keys) into the user's library.
    Import {
        plan: ProceduralPlan,
        library: Vec<String>,
    },
    Cancel,
}

pub struct ReviewDialog {
    analysis: Analysis,
    /// The choice per replaceable procedural, by name.
    choices: Vec<(String, Choice)>,
    materials: MaterialChoices,
    library: LibraryChoices,
}

impl ReviewDialog {
    /// Every replaceable procedural and every material starts as Painter's result and no library item is ticked, so
    /// an import changes nothing unless asked. `writable`: there is a user library to add items to.
    pub fn new(findings: Findings, writable: bool) -> ReviewDialog {
        let Findings { analysis, materials, tray, library } = findings;
        let choices = analysis.procedurals.iter().filter(|p| p.replacement.is_some()).map(|p| (p.name.clone(), Choice::Keep)).collect();
        ReviewDialog { analysis, choices, materials: MaterialChoices::new(materials, tray), library: LibraryChoices::new(Arc::new(library), writable) }
    }

    /// Starts every choice at what `plan` picks (a re-import repeats the choices of the earlier import).
    pub fn prefilled(mut self, plan: &ProceduralPlan) -> ReviewDialog {
        self.choices.iter_mut().for_each(|(name, c)| *c = plan.choice(name));
        self.materials.prefill(plan);
        self
    }

    /// The candidates the library section offers.
    pub fn library_candidates(&self) -> &Arc<LibraryCandidates> {
        self.library.candidates()
    }

    /// The plan the current choices make.
    pub fn plan(&self) -> ProceduralPlan {
        let plan = self.choices.iter().fold(ProceduralPlan::default(), |plan, (name, c)| plan.with(name, *c));
        self.materials.apply(plan)
    }

    fn set_all(&mut self, choice: Choice) {
        self.choices.iter_mut().for_each(|(_, c)| *c = choice);
    }

    /// Draws the dialog; `Some` once the user imports or cancels.
    pub fn show(&mut self, ctx: &egui::Context, source: &Path) -> Option<ReviewResult> {
        let mut result = None;
        let name = source.file_name().map_or_else(|| source.display().to_string(), |n| n.to_string_lossy().into_owned());
        egui::Window::new("Import review").collapsible(false).resizable(true).default_width(620.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(
            ctx,
            |ui| {
                if self.choices.is_empty() && self.materials.is_empty() {
                    ui.label(format!("{name} brings things you can add to your own library:"));
                } else {
                    ui.label(format!("{name} uses Substance procedurals. Choose per procedural how it is imported:"));
                }
                egui::ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
                    if !self.choices.is_empty() {
                        self.procedurals(ui);
                    }
                    if !self.materials.is_empty() {
                        ui.add_space(6.0);
                        ui.separator();
                        self.materials.show(ui);
                    }
                    let kept: Vec<&ProceduralSummary> = self.analysis.procedurals.iter().filter(|p| p.replacement.is_none() && !p.is_material()).collect();
                    if !kept.is_empty() {
                        ui.add_space(6.0);
                        ui.collapsing(format!("{} more without a MaterialCraft replacement yet (Painter's result)", kept.len()), |ui| {
                            for p in kept {
                                ui.small(format!("{} — {} uses", p.name, p.uses.len()));
                            }
                        });
                    }
                    if !self.library.is_empty() {
                        ui.add_space(6.0);
                        ui.separator();
                        self.library.show(ui);
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Import").clicked() {
                        result = Some(ReviewResult::Import { plan: self.plan(), library: self.library.ticked() });
                    }
                    if ui.button("Cancel").clicked() {
                        result = Some(ReviewResult::Cancel);
                    }
                });
            },
        );
        result
    }

    /// The replaceable procedurals (generators, textures, filters).
    fn procedurals(&mut self, ui: &mut egui::Ui) {
        ui.small("MaterialCraft's replacement: editable layers, close to Painter (accuracy measured on your projects).");
        ui.small("Painter's result: exactly Painter's look, flattened, not editable.");
        ui.horizontal(|ui| {
            if ui.button("Replace all").clicked() {
                self.set_all(Choice::Replace);
            }
            if ui.button("Painter's result for all").clicked() {
                self.set_all(Choice::Keep);
            }
        });
        egui::Grid::new("import_review_grid").striped(true).num_columns(4).show(ui, |ui| {
            ui.strong("Procedural");
            ui.strong("Uses");
            ui.strong("Accuracy");
            ui.strong("Import as");
            ui.end_row();
            for p in self.analysis.procedurals.iter().filter(|p| p.replacement.is_some()) {
                let Some((_, choice)) = self.choices.iter_mut().find(|(n, _)| *n == p.name) else { continue };
                row(ui, p, choice);
            }
        });
    }
}

fn row(ui: &mut egui::Ui, p: &ProceduralSummary, choice: &mut Choice) {
    let label = p.replacement.map_or("", |r| r.label);
    ui.label(&p.name).on_hover_text(p.replacement.map_or(String::new(), |r| format!("{label}\n{}", r.note)));
    ui.label(format!("{} ({} shown)", p.uses.len(), p.visible_uses()));
    match &p.accuracy {
        Some(a) => ui.label(format!("{:.1}/255", a.median_mae)).on_hover_text(format!("median MAE over {} measured cases (mcparity run {})", a.cases, a.run)),
        None => ui.label(RichText::new("not measured").weak()),
    };
    ui.horizontal(|ui| {
        ui.radio_value(choice, Choice::Replace, "MaterialCraft");
        ui.radio_value(choice, Choice::Keep, "Painter's result");
    });
    ui.end_row();
}

#[cfg(test)]
mod tests {
    use super::*;
    use materialcraft_substance::tools::accuracy::ReplacementAccuracy;
    use materialcraft_substance::tools::analyze::{ProceduralUse, Role};
    use materialcraft_substance::tools::material_match::{MaterialMatches, PainterLooks, TrayMaterial, TrayMaterials};
    use std::sync::Arc;

    fn use_of(name: &str) -> ProceduralUse {
        ProceduralUse {
            name: name.into(),
            role: Role::MaskFill,
            set: "S".into(),
            layer: "L".into(),
            visible: true,
            params: vec![],
            inputs: vec![],
            material: false,
        }
    }

    fn findings(uses: Vec<ProceduralUse>, tray: Vec<TrayMaterial>) -> Findings {
        let analysis = Analysis::of_uses("p.spp", uses, &ReplacementAccuracy::default());
        let tray = Arc::new(TrayMaterials::from_materials(vec![], tray));
        let materials = MaterialMatches::from_looks(&analysis, &PainterLooks::default(), &tray);
        Findings { analysis, materials, tray, library: Default::default() }
    }

    #[test]
    fn choices_start_as_painters_result_and_make_the_plan() {
        let mut d = ReviewDialog::new(findings(vec![use_of("mg_dirt"), use_of("PBR_to_Skyrim"), use_of("mg_dirt")], vec![]), true);
        assert_eq!(d.choices, vec![("mg_dirt".to_string(), Choice::Keep)]);
        assert!(d.materials.is_empty());
        assert_eq!(d.plan().choice("mg_dirt"), Choice::Keep);
        d.set_all(Choice::Replace);
        let plan = d.plan();
        assert_eq!((plan.choice("mg_dirt"), plan.choice("PBR_to_Skyrim")), (Choice::Replace, Choice::Keep));
    }

    #[test]
    fn material_choices_flow_into_the_plan() {
        let rust = ProceduralUse { role: Role::Fill, material: true, ..use_of("rust_fine") };
        let iron = TrayMaterial { reference: "materials/iron-rusted.mca".into(), name: "Rusted iron".into(), tags: vec!["rust".into()], look: None };
        let mut d = ReviewDialog::new(findings(vec![use_of("mg_dirt"), rust], vec![iron]), true);
        assert!(!d.materials.is_empty());
        assert_eq!(d.plan().material("rust_fine"), None, "Painter's result unless asked");
        d.materials.use_best();
        assert_eq!(d.plan().material("rust_fine"), Some("materials/iron-rusted.mca"));
        assert_eq!(d.plan().choice("mg_dirt"), Choice::Keep, "procedural choices stay apart");
    }

    #[test]
    fn a_plan_prefills_the_choices() {
        let rust = ProceduralUse { role: Role::Fill, material: true, ..use_of("rust_fine") };
        let iron = TrayMaterial { reference: "materials/iron-rusted.mca".into(), name: "Rusted iron".into(), tags: vec!["rust".into()], look: None };
        let earlier = ProceduralPlan::all(Choice::Replace).with_material("rust_fine", Some("materials/iron-rusted.mca"));
        let d = ReviewDialog::new(findings(vec![use_of("mg_dirt"), rust], vec![iron]), true).prefilled(&earlier);
        assert_eq!(d.choices, vec![("mg_dirt".to_string(), Choice::Replace)]);
        assert_eq!(d.plan().material("rust_fine"), Some("materials/iron-rusted.mca"));
    }
}
