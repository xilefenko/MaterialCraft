//! Arranges the Paint screen: menu and status bars, the tools and panel strips on the window
//! edges, and the docking workspace (viewport, texture sets, import report, log, settings) between them.

use super::super::channel_view::ChannelView;
use super::super::document_state::OpenDocument;
use super::super::input::pen_status::PenStatus;
use super::super::projection_edit::GizmoSettings;
use super::super::shell::Shell;
use super::super::tools::BrushTools;
use super::super::tools::screen_image::ScreenImages;
use super::super::viewport::Viewport;
use super::bakers::BakersPanel;
use super::blockers::BlockersPanel;
use super::dock_content::Bodies;
use super::import_report::ReportPanel;
use super::layers::LayerPanel;
use super::log::LogPanel;
use super::status_bar::Activity;
use super::{PanelEvent, Strip, menu_bar, panel_strip, status_bar, tools_strip};
use crate::gui_assets::AssetTray;
use crate::gui_settings::Side;
use crate::gui_theme;
use crate::gui_widgets::strip::STRIP_WIDTH;
use crate::gui_workspace::dock_style;
use eframe::egui;
use materialcraft::diag::RingBuffer;

/// Everything one frame of the screen shows.
pub struct Screen<'a> {
    pub document: Option<&'a OpenDocument>,
    pub report: &'a mut ReportPanel,
    pub blockers: &'a mut BlockersPanel,
    pub layers: &'a mut LayerPanel,
    pub viewport: &'a mut Viewport,
    pub display: ChannelView,
    /// The session's diagnostics: the newest entry for the status bar, all for the log panel.
    pub history: &'a RingBuffer,
    pub log: &'a mut LogPanel,
    pub tray: &'a mut AssetTray,
    /// A texture set to highlight this frame.
    pub focus_set: Option<(usize, bool)>,
    pub activity: Activity<'a>,
    pub bakers: &'a mut BakersPanel,
    pub shell: &'a mut Shell,
    pub brush: &'a mut BrushTools,
    pub images: &'a ScreenImages,
    /// The quick mask's state, for its controls.
    pub quick_mask: materialcraft_brush::QuickMaskState,
    /// The projection gizmo's settings, for the projection panel.
    pub projection: &'a GizmoSettings,
    /// The shader files and the model's size, for the shader settings panel.
    pub shaders: &'a materialcraft::shading::ShaderLibrary,
    pub model: &'a crate::gui_paint::scene_shading::ModelScale,
    /// Whether pen pressure reaches the brushes, for the pen settings and the brush panel.
    pub pen_status: &'a PenStatus,
}

/// Draws the screen and returns what the user asked for.
pub fn show(ui: &mut egui::Ui, s: Screen) -> Vec<PanelEvent> {
    let mut events = Vec::new();
    let title = s.document.map(|d| d.title()).unwrap_or_default();
    let has_document = s.document.is_some();
    let paintable = s.document.is_some_and(|d| s.layers.paintable(d.doc()));
    let origin = s.document.and_then(|d| d.doc().import_origin.as_ref());
    menu_bar::show(ui, &title, has_document, origin, s.document.map(|d| d.history()), s.shell, &mut events);
    status_bar::show(ui, s.history.last().map(|(_, e)| e), &s.activity, &mut events);
    let strip_frame = egui::Frame::NONE.fill(gui_theme::STRIP_BG);
    for strip in [Strip::Tools, Strip::Panels] {
        let id = match strip {
            Strip::Tools => "paint_tools_strip",
            Strip::Panels => "paint_panel_strip",
        };
        let panel = match s.shell.strip_side(strip) {
            Side::Left => egui::Panel::left(id),
            Side::Right => egui::Panel::right(id),
        };
        panel.frame(strip_frame).exact_size(STRIP_WIDTH).resizable(false).show(ui, |ui| match strip {
            Strip::Tools => tools_strip::show(ui, s.shell, has_document, paintable, &mut events),
            Strip::Panels => panel_strip::show(ui, s.shell, &mut events),
        });
    }
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(gui_theme::BORDER)).show(ui, |ui| {
        let style = dock_style(ui);
        let shell = &mut *s.shell;
        let mut bodies = Bodies {
            document: s.document,
            report: s.report,
            blockers: s.blockers,
            layers: s.layers,
            viewport: s.viewport,
            display: s.display,
            shortcuts: &shell.shortcuts,
            page: &mut shell.page,
            tools: &shell.tools,
            brush: s.brush,
            images: s.images,
            quick_mask: s.quick_mask,
            projection: s.projection,
            shaders: s.shaders,
            model: s.model,
            scale_draft: &mut shell.scale_draft,
            pen: &shell.settings.get().pen,
            pen_status: s.pen_status,
            log: s.log,
            tray: s.tray,
            history: s.history,
            focus_set: s.focus_set,
            bakers: s.bakers,
            bake: s.activity.bake,
            events: &mut events,
        };
        if let Some(e) = shell.workspace.show(ui, &style, &mut bodies) {
            events.push(PanelEvent::DockRefused(e));
        }
    });
    events
}

/// Shown in the viewport when no document is open.
pub fn empty_state(ui: &mut egui::Ui, events: &mut Vec<PanelEvent>) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.35);
        ui.label(egui::RichText::new("Paint mode").size(19.0).strong().color(egui::Color32::from_gray(235)));
        ui.add_space(8.0);
        ui.label("Start a project from a mesh, or open a MaterialCraft paint document or a Substance 3D Painter project.");
        ui.add_space(12.0);
        if ui.button("New project...").clicked() {
            events.push(PanelEvent::NewProject);
        }
        if ui.button("Open in Paint mode...").clicked() {
            events.push(PanelEvent::Open);
        }
        if ui.button("Import Substance Painter project...").clicked() {
            events.push(PanelEvent::Import);
        }
    });
}
