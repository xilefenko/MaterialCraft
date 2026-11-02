//! [`MaterialChoices`]: the import review's section for Substance material fills (roadmap step 27, ADR 0008
//! rm-material-fills). Per Painter material: where it is used, what Painter's result looks like there (mean
//! base colour, roughness, metallic), and a choice between Painter's result (flattened, the default) and a
//! material from the tray, suggested by name and look with its score. One choice applies to every fill using
//! that material.

use eframe::egui::{self, Color32, RichText};
use materialcraft::paint::Channel;
use materialcraft_substance::tools::material_match::{Look, MaterialMatch, MaterialMatches, Suggestion, TrayMaterials};
use materialcraft_substance::translate::replacements::ProceduralPlan;
use std::sync::Arc;

const SWATCH: f32 = 14.0;

pub struct MaterialChoices {
    matches: MaterialMatches,
    tray: Arc<TrayMaterials>,
    /// The picked reference per Painter material (`None`: Painter's result), in the matches' order.
    choices: Vec<Option<String>>,
}

impl MaterialChoices {
    /// Every material starts as Painter's result, so an import changes nothing unless asked.
    pub fn new(matches: MaterialMatches, tray: Arc<TrayMaterials>) -> MaterialChoices {
        let choices = vec![None; matches.materials.len()];
        MaterialChoices { matches, tray, choices }
    }

    /// Starts each material at what `plan` picks for it (a re-import repeats the earlier choices).
    pub fn prefill(&mut self, plan: &ProceduralPlan) {
        for (m, c) in self.matches.materials.iter().zip(self.choices.iter_mut()) {
            *c = plan.material(&m.name).map(str::to_string);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.matches.materials.is_empty()
    }

    /// `plan` with these choices.
    pub fn apply(&self, plan: ProceduralPlan) -> ProceduralPlan {
        self.matches.materials.iter().zip(&self.choices).fold(plan, |plan, (m, c)| plan.with_material(&m.name, c.as_deref()))
    }

    /// Every material gets its best suggestion.
    pub fn use_best(&mut self) {
        for (m, c) in self.matches.materials.iter().zip(self.choices.iter_mut()) {
            *c = m.best().map(|s| s.reference.clone());
        }
    }

    /// Every material keeps Painter's result.
    pub fn keep_all(&mut self) {
        self.choices.iter_mut().for_each(|c| *c = None);
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.strong("Substance materials");
        ui.small("A MaterialCraft material from the tray: editable, with its own look (a replacement, not Painter's look).");
        ui.small("Painter's result: exactly Painter's look, flattened, not editable.");
        ui.horizontal(|ui| {
            if ui.button("Best suggestion for all").clicked() {
                self.use_best();
            }
            if ui.button("Painter's result for all materials").clicked() {
                self.keep_all();
            }
        });
        if self.tray.is_empty() {
            ui.label(RichText::new("No materials in the asset libraries to choose from.").weak());
        }
        egui::Grid::new("import_review_materials").striped(true).num_columns(4).show(ui, |ui| {
            ui.strong("Material");
            ui.strong("Uses");
            ui.strong("Painter's result");
            ui.strong("Import as");
            ui.end_row();
            for (i, (m, choice)) in self.matches.materials.iter().zip(self.choices.iter_mut()).enumerate() {
                row(ui, i, m, choice, &self.tray);
            }
        });
    }
}

fn row(ui: &mut egui::Ui, i: usize, m: &MaterialMatch, choice: &mut Option<String>, tray: &TrayMaterials) {
    ui.label(&m.name).on_hover_text(format!("texture sets: {}", m.sets.join(", ")));
    ui.label(format!("{} ({} shown)", m.uses, m.visible_uses));
    match &m.painter {
        Some(p) => {
            let hover = format!(
                "Painter's saved result where the material shows: {} layers sampled{}, {:.0} % of the set's texels",
                p.layers,
                if p.open_masks > 0 { format!(", {} under masks that could not be computed (whole set used)", p.open_masks) } else { String::new() },
                p.coverage * 100.0
            );
            look_cell(ui, &p.look).on_hover_text(hover);
        }
        None => {
            ui.label(RichText::new("not measured").weak()).on_hover_text("The material is not visible where Painter saved its result.");
        }
    }
    let selected = match choice {
        None => "Painter's result".to_string(),
        Some(r) => m.ranked.iter().find(|s| &s.reference == r).map_or(r.clone(), label),
    };
    egui::ComboBox::from_id_salt(("import_review_material", i)).selected_text(selected).width(220.0).show_ui(ui, |ui| {
        ui.selectable_value(choice, None, "Painter's result (flattened)");
        ui.separator();
        for s in &m.ranked {
            let picked = choice.as_deref() == Some(s.reference.as_str());
            let response = ui
                .horizontal(|ui| {
                    if let Some(look) = tray.materials().iter().find(|t| t.reference == s.reference).and_then(|t| t.look.as_ref()) {
                        swatch(ui, look);
                    }
                    ui.selectable_label(picked, label(s))
                })
                .inner;
            if response.on_hover_text(details(s)).clicked() {
                *choice = Some(s.reference.clone());
            }
        }
    });
    ui.end_row();
}

/// `Rusted iron (0.62)`.
fn label(s: &Suggestion) -> String {
    format!("{} ({:.2})", s.name, s.score)
}

fn details(s: &Suggestion) -> String {
    let mut text = format!("{}\nscore {:.2}: name {:.2}", s.reference, s.score, s.name_score);
    match &s.look {
        Some(l) => {
            text.push_str(&format!(", look {:.2}", l.overall));
            for (c, v) in &l.channels {
                text.push_str(&format!("\n  {}: {v:.2}", c.id()));
            }
        }
        None => text.push_str(", look not compared"),
    }
    text
}

/// A swatch of the mean base colour and the mean roughness and metallic.
fn look_cell(ui: &mut egui::Ui, look: &Look) -> egui::Response {
    ui.horizontal(|ui| {
        swatch(ui, look);
        let grey = |c: Channel| look.channel(c).map_or("–".to_string(), |l| format!("{:.2}", l.mean[0]));
        ui.small(format!("R {}  M {}", grey(Channel::Roughness), grey(Channel::Metallic)));
    })
    .response
}

fn swatch(ui: &mut egui::Ui, look: &Look) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(SWATCH, SWATCH), egui::Sense::hover());
    let colour = look.channel(Channel::BaseColor).map_or(Color32::GRAY, |c| {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Color32::from_rgb(byte(c.mean[0]), byte(c.mean[1]), byte(c.mean[2]))
    });
    ui.painter().rect_filled(rect, 2.0, colour);
}

#[cfg(test)]
mod tests {
    use super::*;
    use materialcraft_substance::tools::material_match::TrayMaterial;

    fn tray() -> Arc<TrayMaterials> {
        let m = |r: &str, n: &str, tags: &[&str]| TrayMaterial {
            reference: r.into(),
            name: n.into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            look: None,
        };
        Arc::new(TrayMaterials::from_materials(
            vec![],
            vec![m("materials/iron-rusted.mca", "Rusted iron", &["rust"]), m("materials/moss.mca", "Moss", &["moss"])],
        ))
    }

    fn matches(tray: &TrayMaterials) -> MaterialMatches {
        let one = |name: &str| MaterialMatch {
            name: name.into(),
            uses: 1,
            visible_uses: 1,
            sets: vec!["S".into()],
            painter: None,
            ranked: MaterialMatch::rank(name, None, tray),
        };
        MaterialMatches { materials: vec![one("rust_fine"), one("Moss")] }
    }

    #[test]
    fn choices_start_as_painters_result_and_one_choice_covers_a_material() {
        let tray = tray();
        let mut c = MaterialChoices::new(matches(&tray), tray);
        assert_eq!(c.apply(ProceduralPlan::default()), ProceduralPlan::default());
        c.use_best();
        let plan = c.apply(ProceduralPlan::default());
        assert_eq!(plan.material("rust_fine"), Some("materials/iron-rusted.mca"));
        assert_eq!(plan.material("Moss"), Some("materials/moss.mca"));
        c.choices[1] = None;
        assert_eq!(c.apply(ProceduralPlan::default()).material("Moss"), None);
        c.keep_all();
        assert_eq!(c.apply(ProceduralPlan::default()), ProceduralPlan::default());
    }
}
