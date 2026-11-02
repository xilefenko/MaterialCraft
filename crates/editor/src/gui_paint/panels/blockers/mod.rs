//! The blockers panel: the open document's locked layers grouped by what blocks them (the importer's
//! `BlockerSummary`), with the support catalog's status and what would unlock each blocker. Expanding a blocker lists
//! the layers it locks; clicking one goes to it in the layer panel (a [`PanelEvent::Reveal`]). Read-only.
//!
//! The summary walks every layer, so it is worked out again only when the document changed: when the history moved
//! (every edit, undo and redo goes through it) or another document was opened ([`BlockersPanel::reset`]).

mod view;

use super::PanelEvent;
use crate::gui_paint::document_state::OpenDocument;
use crate::gui_paint::save_point::SavePoint;
use eframe::egui::collapsing_header::CollapsingState;
use eframe::egui::{self, Color32, RichText, Ui};
use materialcraft::diag::Target;
use materialcraft::history::PaintHistory;
use materialcraft_substance::translate::catalog::Support;
use view::BlockersView;

/// A state of the document's history: when it changes, the document may have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    point: SavePoint,
    done: usize,
    len: usize,
}

impl Stamp {
    fn of(history: &PaintHistory) -> Stamp {
        Stamp { point: SavePoint::of(history), done: history.done(), len: history.len() }
    }
}

/// Panel state: the view of the document state it was last worked out for.
#[derive(Default)]
pub struct BlockersPanel {
    cached: Option<(Stamp, BlockersView)>,
}

impl BlockersPanel {
    /// Forgets the previous document.
    pub fn reset(&mut self) {
        self.cached = None;
    }

    /// The view of `open`, worked out again only if its history moved since the last frame.
    fn view(&mut self, open: &OpenDocument) -> &BlockersView {
        let stamp = Stamp::of(open.history());
        let cached = self.cached.take().filter(|(s, _)| *s == stamp);
        let (_, view) = self.cached.insert(cached.unwrap_or_else(|| (stamp, BlockersView::of(open.doc()))));
        view
    }

    pub fn show(&mut self, ui: &mut Ui, open: &OpenDocument, events: &mut Vec<PanelEvent>) {
        let view = self.view(open);
        if view.summary.is_empty() {
            ui.label("Nothing is locked.");
            ui.weak("Layers an import couldn't compute are locked and listed here with what blocks them.");
            return;
        }
        ui.label(view.totals.line());
        ui.separator();
        egui::ScrollArea::vertical().id_salt("paint_blockers").auto_shrink([false, false]).show(ui, |ui| {
            for group in &view.groups {
                ui.label(RichText::new(format!("{} ({})", group.label, group.blockers.len())).strong());
                for &i in &group.blockers {
                    blocker(ui, view, i, events);
                }
                ui.add_space(6.0);
            }
        });
    }
}

/// One blocker: a header line (status, name, how many layers it locks), its text, and when expanded its layers.
fn blocker(ui: &mut Ui, view: &BlockersView, i: usize, events: &mut Vec<PanelEvent>) {
    let Some(b) = view.blocker(i) else { return };
    let id = ui.make_persistent_id(("blocker", &b.key));
    let header = CollapsingState::load_with_default_open(ui.ctx(), id, false).show_header(ui, |ui| {
        badge(ui, b.support);
        ui.label(RichText::new(view::name(b)).strong());
        ui.weak(view::uses(b));
    });
    let tip = format!("{}\n{}: {}", b.key, view::status(b), view::action(b));
    let (_, header, _) = header.body(|ui| {
        for &l in &b.layers {
            let Some(layer) = view.layer(l) else { continue };
            if ui.link(view::layer_label(layer)).on_hover_text("Select this layer").clicked() {
                events.push(PanelEvent::Reveal(Target::Layer { texture_set: layer.set.clone(), layer: layer.layer.clone() }));
            }
        }
    });
    header.response.on_hover_text(tip);
    if !b.text.is_empty() {
        ui.indent(id.with("text"), |ui| ui.label(RichText::new(&b.text).small().color(Color32::from_gray(150))));
    }
}

fn color(support: Option<Support>) -> Color32 {
    match support {
        Some(Support::Replaceable { .. }) => Color32::from_rgb(70, 150, 90),
        Some(Support::Replayable) => Color32::from_rgb(70, 120, 190),
        Some(Support::Unsupported) | None => Color32::from_rgb(170, 80, 80),
    }
}

fn badge(ui: &mut Ui, support: Option<Support>) {
    let label = support.map_or("Unknown", Support::label);
    let text = RichText::new(label).small().strong().color(Color32::WHITE);
    egui::Frame::NONE.fill(color(support)).corner_radius(3).inner_margin(egui::Margin::symmetric(4, 0)).show(ui, |ui| ui.label(text));
}
