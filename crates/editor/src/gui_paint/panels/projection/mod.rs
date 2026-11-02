//! The fill projection panel (roadmap step 38, PJ-01 … PJ-25): the projection of the selected fill layer or fill
//! effect: its mode, the image placement block (the same in every mode, ADR 0008 mc2-q-pj-10), the volume and the
//! mode's own settings (falloffs, blend, coverage), fill symmetry, sampling, and the view gizmo's settings. It
//! changes nothing itself: a change goes out as a [`ProjectionEvent`], which `PaintMode` applies as a history step
//! (a slider drag as one step).
//!
//! - [`placement`]: size mode, tiling, rotation, offset, filter and repetition;
//! - [`volume`]: the volume, the mode's settings and fill symmetry;
//! - [`gizmo`]: the gizmo's settings;
//! - [`textures`]: the fill's channel textures and their parameters (roadmap step 48).

mod gizmo;
mod placement;
mod textures;
mod volume;

use super::PanelEvent;
use super::layers::content_lock;
use super::layers::lock_text;
use crate::gui_paint::projection_edit::GizmoSettings;
use crate::gui_paint::tools::helpers::SymmetrySettings;
use crate::gui_theme;
use eframe::egui::{self, RichText, Ui};
use materialcraft::paint::edit::ops::{FillAt, FillRef};
use materialcraft::paint::{Document, FillSource};
use materialcraft::projection::{Projection, ProjectionMode};

/// What the panel asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum ProjectionEvent {
    /// Give `target` this projection; with `gesture` (a slider being dragged) the change joins the running step.
    Set {
        target: FillRef,
        projection: Box<Projection>,
        gesture: bool,
    },
    /// Give `target` this source (a channel's texture or its parameters); with `gesture` (a slider being dragged)
    /// the change joins the running step.
    Source {
        target: FillRef,
        source: Box<FillSource>,
        gesture: bool,
    },
    /// The slider drag ended.
    EndGesture,
    Gizmo(GizmoSettings),
}

/// The modes offered, in the order shown.
const MODES: [(&str, &str); 7] = [
    ("texture_space", "Texture space (per UV tile)"),
    ("uv", "UV"),
    ("tri_planar", "Tri-planar"),
    ("planar", "Planar"),
    ("spherical", "Spherical"),
    ("cylindrical", "Cylindrical"),
    ("warp", "Warp"),
];

/// What the panel shows this frame.
pub struct ProjectionUi<'a> {
    pub doc: &'a Document,
    pub target: Option<FillRef>,
    pub gizmo: &'a GizmoSettings,
    /// The symmetry tool's settings (fill symmetry takes them over, PJ-22).
    pub symmetry: SymmetrySettings,
}

pub fn show(ui: &mut Ui, p: ProjectionUi, events: &mut Vec<PanelEvent>) {
    let mut gizmo_settings = *p.gizmo;
    egui::ScrollArea::vertical().id_salt("paint_projection_panel").show(ui, |ui| {
        match p.target.as_ref().and_then(|t| Some((t, t.fill(p.doc)?))) {
            None => {
                ui.weak("Select a fill layer, or click a fill effect in the layer list, to edit its projection.");
            }
            Some((target, fill)) => {
                ui.label(RichText::new(describe(p.doc, target)).color(gui_theme::LABEL_TEXT));
                let lock = content_lock::content_lock(p.doc, &target.layer);
                if let Some(lock) = lock {
                    let why = lock_text::reasons(lock, 12).join("\n");
                    ui.colored_label(gui_theme::ACCENT, content_lock::NOTE).on_hover_text(format!("{}\n{why}", lock_text::heading(lock)));
                }
                if matches!(fill.source, FillSource::Uniform(_)) {
                    ui.weak("A uniform fill looks the same in every projection.");
                }
                let editable = lock.is_none();
                let mut edited = fill.projection.clone();
                let mut source = fill.source.clone();
                ui.add_enabled_ui(editable, |ui| {
                    mode(ui, &mut edited);
                    if let ProjectionMode::Unknown(u) = &edited.mode {
                        ui.weak(format!("This projection ({}) comes from a newer version and can't be edited here.", u.mode));
                    } else {
                        placement::show(ui, &mut edited);
                        volume::show(ui, &mut edited, p.symmetry, &mut gizmo_settings);
                    }
                    textures::show(ui, &mut source, fill.original.as_deref());
                });
                if editable && edited != fill.projection {
                    let gesture = ui.input(|i| i.pointer.any_down());
                    events.push(PanelEvent::Projection(ProjectionEvent::Set { target: target.clone(), projection: Box::new(edited), gesture }));
                }
                if editable && source != fill.source {
                    let gesture = ui.input(|i| i.pointer.any_down());
                    events.push(PanelEvent::Projection(ProjectionEvent::Source { target: target.clone(), source: Box::new(source), gesture }));
                }
            }
        }
        heading(ui, "View gizmo");
        gizmo::show(ui, &mut gizmo_settings);
    });
    if ui.input(|i| i.pointer.any_released()) {
        events.push(PanelEvent::Projection(ProjectionEvent::EndGesture));
    }
    if gizmo_settings != *p.gizmo {
        events.push(PanelEvent::Projection(ProjectionEvent::Gizmo(gizmo_settings)));
    }
}

/// "Fill layer “Rust”" or "Fill effect 2 on “Rust”".
fn describe(doc: &Document, target: &FillRef) -> String {
    let name = doc.texture_set(&target.layer.set).and_then(|s| s.layer(&target.layer.layer)).map_or("?", |l| l.name.as_str());
    match target.at {
        FillAt::Layer => format!("Fill layer “{name}”"),
        FillAt::Effect(i) => format!("Fill effect {} on “{name}”", i + 1),
    }
}

/// The mode picker: switching keeps the shared settings and, between volume modes, the volume.
fn mode(ui: &mut Ui, p: &mut Projection) {
    let current = p.mode.id().to_string();
    let label = MODES.iter().find(|(id, _)| *id == current).map_or(current.as_str(), |(_, l)| *l).to_string();
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.label("Projection");
        egui::ComboBox::from_id_salt("projection_mode").selected_text(label).show_ui(ui, |ui| {
            for (id, l) in MODES {
                if ui.selectable_label(id == current, l).clicked() && id != current {
                    chosen = Some(id);
                }
            }
        });
    });
    let Some(id) = chosen else { return };
    let Some(mut next) = ProjectionMode::default_of(id) else { return };
    if let (Some(old), Some(new)) = (p.mode.volume().copied(), next.volume_mut()) {
        *new = old;
    }
    p.mode = next;
}

pub(super) fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).strong().color(gui_theme::HEADER_TEXT));
}
