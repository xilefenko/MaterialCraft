//! What each docked panel shows: the bridge from the workspace's panel ids to the Paint
//! screen's panels.

use craft_ui::docking::DockContent;
use eframe::egui::{self, Ui};

use super::super::channel_view::ChannelView;
use super::super::document_state::OpenDocument;
use super::super::input::pen_status::PenStatus;
use super::super::projection_edit::GizmoSettings;
use super::super::shell::actions;
use super::super::shell::panels::PaintPanel;
use super::super::shell::tools::ToolState;
use super::super::tools::BrushTools;
use super::super::tools::screen_image::ScreenImages;
use super::super::viewport::Viewport;
use super::bakers::{BakeStatus, BakersPanel};
use super::blockers::BlockersPanel;
use super::import_report::ReportPanel;
use super::layers::LayerPanel;
use super::log::LogPanel;
use super::{
    PanelEvent, brush_panel, display_settings, interface_settings, pen_settings, projection, shader_settings, texture_sets, timeline, viewport_toolbar,
};
use crate::gui_assets::AssetTray;
use crate::gui_settings::PenSettings;
use crate::gui_workspace::shortcuts::{ShortcutTable, ShortcutsPage};
use materialcraft::diag::RingBuffer;

/// Everything the panel bodies need for one frame.
pub struct Bodies<'a> {
    pub document: Option<&'a OpenDocument>,
    pub report: &'a mut ReportPanel,
    pub blockers: &'a mut BlockersPanel,
    pub layers: &'a mut LayerPanel,
    pub viewport: &'a mut Viewport,
    pub display: ChannelView,
    pub shortcuts: &'a ShortcutTable,
    pub page: &'a mut ShortcutsPage,
    pub tools: &'a ToolState,
    /// The brush tools' settings (brush panel, quick-access bar).
    pub brush: &'a mut BrushTools,
    /// The stencil and projection image, for the brush panel.
    pub images: &'a ScreenImages,
    pub quick_mask: materialcraft_brush::QuickMaskState,
    /// The projection gizmo's settings, for the projection panel.
    pub projection: &'a GizmoSettings,
    /// The shader files and the model's size, for the shader settings panel.
    pub shaders: &'a materialcraft::shading::ShaderLibrary,
    pub model: &'a crate::gui_paint::scene_shading::ModelScale,
    pub scale_draft: &'a mut f32,
    /// The pen settings, shown under the interface settings.
    pub pen: &'a PenSettings,
    /// Whether pen pressure reaches the brushes (pen settings, brush panel).
    pub pen_status: &'a PenStatus,
    pub log: &'a mut LogPanel,
    pub tray: &'a mut AssetTray,
    /// The session's diagnostics, for the log panel.
    pub history: &'a RingBuffer,
    /// A texture set to highlight this frame (going to a log entry's target).
    pub focus_set: Option<(usize, bool)>,
    pub bakers: &'a mut BakersPanel,
    /// (set name, done, total) of the running bake.
    pub bake: Option<(&'a str, usize, usize)>,
    pub events: &'a mut Vec<PanelEvent>,
}

impl DockContent<PaintPanel> for Bodies<'_> {
    fn body(&mut self, ui: &mut Ui, panel: &PaintPanel) {
        let margin = egui::Frame::NONE.inner_margin(egui::Margin::same(6));
        match panel {
            PaintPanel::Viewport => self.viewport_body(ui),
            PaintPanel::TextureSets => {
                margin.show(ui, |ui| match self.document {
                    Some(doc) => texture_sets::show(ui, doc, self.focus_set, self.events),
                    None => no_document(ui),
                });
            }
            PaintPanel::Layers => {
                margin.show(ui, |ui| match self.document {
                    Some(doc) => self.layers.show(ui, doc, self.events),
                    None => no_document(ui),
                });
            }
            PaintPanel::ImportReport => {
                margin.show(ui, |ui| match self.document {
                    Some(doc) => self.report.show(ui, doc.doc().import_report.as_ref(), doc.summary.as_deref()),
                    None => no_document(ui),
                });
            }
            PaintPanel::Blockers => {
                margin.show(ui, |ui| match self.document {
                    Some(doc) => self.blockers.show(ui, doc, self.events),
                    None => no_document(ui),
                });
            }
            PaintPanel::Display => {
                margin.show(ui, |ui| {
                    egui::ScrollArea::vertical().id_salt("paint_display").show(ui, |ui| {
                        let status = self.viewport.environment_status().clone();
                        let (state, gpu) = self.viewport.settings_mut();
                        display_settings::show(ui, state, gpu, &status, self.display, self.events);
                    })
                });
            }
            PaintPanel::Shortcuts => {
                margin.show(ui, |ui| {
                    if let Some(edit) = self.page.show(ui, self.shortcuts) {
                        self.events.push(PanelEvent::Shortcut(edit));
                    }
                });
            }
            PaintPanel::Interface => {
                margin.show(ui, |ui| {
                    interface_settings::show(ui, self.scale_draft, self.events);
                    pen_settings::show(ui, self.pen, self.pen_status, self.events);
                });
            }
            PaintPanel::Bakers => {
                margin.show(ui, |ui| match self.document {
                    Some(doc) => self.bakers.show(ui, doc, &BakeStatus { running: self.bake }, self.events),
                    None => no_document(ui),
                });
            }
            PaintPanel::Log => {
                margin.show(ui, |ui| self.log.show(ui, self.history, self.events));
            }
            PaintPanel::Assets => {
                margin.show(ui, |ui| {
                    let mut out = Vec::new();
                    self.tray.show(ui, &mut out);
                    self.events.extend(out.into_iter().map(PanelEvent::Assets));
                });
            }
            PaintPanel::Brush => {
                let set = self.document.and_then(|d| self.layers.current_set(d.doc()));
                let images = brush_panel::ImagesUi { images: self.images, key: self.shortcuts.label(actions::STENCIL), events: self.events };
                margin.show(ui, |ui| brush_panel::show(ui, self.brush, self.tools.current, set, images, self.pen_status));
            }
            PaintPanel::Shader => {
                margin.show(ui, |ui| match self.document {
                    Some(doc) => {
                        let s = shader_settings::ShaderUi {
                            doc: doc.doc(),
                            set: self.layers.current_set(doc.doc()),
                            history: doc.shading_history(),
                            library: self.shaders,
                            model: self.model,
                        };
                        shader_settings::show(ui, s, self.events);
                    }
                    None => no_document(ui),
                });
            }
            PaintPanel::Projection => {
                margin.show(ui, |ui| match self.document {
                    Some(doc) => {
                        let p = projection::ProjectionUi {
                            doc: doc.doc(),
                            target: self.layers.selected_fill(doc.doc()),
                            gizmo: self.projection,
                            symmetry: self.brush.helpers.symmetry,
                        };
                        projection::show(ui, p, self.events);
                    }
                    None => no_document(ui),
                });
            }
        }
    }
}

impl Bodies<'_> {
    fn viewport_body(&mut self, ui: &mut Ui) {
        let has_document = self.document.is_some();
        let paintable = self.document.is_some_and(|d| self.layers.paintable(d.doc()));
        let camera = super::camera_menu::CameraBar::of(self.viewport.state());
        let view = viewport_toolbar::ViewControls { display: self.display, camera: &camera, layout: self.viewport.state().layout };
        viewport_toolbar::show(ui, self.tools, self.brush, self.quick_mask, self.shortcuts, view, has_document, paintable, self.events);
        if let Some(doc) = self.document {
            timeline::show(ui, doc.history(), self.events);
        }
        if has_document {
            self.viewport.ui(ui, self.display, self.shortcuts, self.events);
        } else {
            super::layout::empty_state(ui, self.events);
        }
    }
}

fn no_document(ui: &mut Ui) {
    ui.weak("No document is open.");
}
