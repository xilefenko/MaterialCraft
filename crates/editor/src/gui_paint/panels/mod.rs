//! The Paint screen's panels. Each draws one part of the screen and reports what the user asked
//! for as [`PanelEvent`]s; `PaintMode` applies them.

pub mod bakers;
pub mod blockers;
mod brush_panel;
pub mod camera_menu;
pub mod channel_bar;
mod display_settings;
mod dock_content;
pub mod drop_zones;
pub mod import_report;
pub mod import_review;
mod import_review_library;
mod import_review_materials;
mod interface_settings;
pub mod layers;
pub mod layout;
pub mod log;
pub mod menu_bar;
mod panel_strip;
mod pen_settings;
mod polygon_bar;
pub mod projection;
mod quick_bar;
mod quick_mask_bar;
pub mod shader_settings;
pub mod status_bar;
mod symmetry_menu;
pub mod texture_sets;
mod timeline;
mod tools_strip;
mod viewport_toolbar;

use super::channel_view::ChannelView;
use super::shell::panels::PaintPanel;
use super::shell::tools::Tool;
use super::view_state::ViewLayout;
use crate::gui_assets::TrayEvent;
use crate::gui_settings::Side;
use crate::gui_workspace::shortcuts::PageEdit;
use materialcraft::diag::Target;

/// One of the two vertical strips.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Strip {
    Tools,
    Panels,
}

/// A request from a panel.
#[derive(Clone, PartialEq, Debug)]
pub enum PanelEvent {
    /// File > New project: a document from a mesh file.
    NewProject,
    /// Save the open document's settings as a project template.
    SaveTemplate,
    /// Pick a `.mcpaint` or `.spp` file.
    Open,
    /// Pick a Substance Painter project.
    Import,
    /// Import the open document's `.spp` again with its earlier choices (it has an import origin).
    Reimport,
    Close,
    /// Save to the document's file (Save As if it has none).
    Save,
    /// Save under a new `.mcpaint` file.
    SaveAs,
    Home,
    ResetCamera,
    /// Switch the camera between perspective and orthographic (VP-06).
    ToggleProjection,
    /// Make the current camera, exposure and environment the defaults for new documents.
    UseDisplayDefaults,
    /// Show camera `n` of the document's camera list (CM-02).
    SelectCamera(usize),
    /// Show the next camera of the list.
    NextCamera,
    /// Pick a glTF file and add its cameras to the list (CM-05).
    ImportCameras,
    ShowChannel(ChannelView),
    /// Show the 3D view, the 2D view or both.
    ViewLayout(ViewLayout),
    SetVisible(usize, bool),
    /// Cancel the running load or import.
    CancelLoad,
    /// Step through the view modes (+1 next, -1 previous).
    CycleChannel(i32),
    /// Show or hide a panel where it was.
    TogglePanel(PaintPanel),
    /// The panel strip's button: show or hide the panel as a popover at `[x, y, w, h]`.
    Popover(PaintPanel, [f32; 4]),
    SwitchLayout(String),
    /// Save the current layout under a name.
    SaveLayout(String),
    DeleteLayout(String),
    /// Go back to the current layout as it was saved.
    ResetLayout,
    MoveStrip(Strip, Side),
    SelectTool(Tool),
    PreviousTool,
    Shortcut(PageEdit),
    SetUiScale(f32),
    /// New pen settings (use pressure, pressure curve).
    SetPen(crate::gui_settings::PenSettings),
    /// The workspace refused a docking change (shown in the status bar).
    DockRefused(String),
    /// Go to what a log entry concerns (LG-10).
    Reveal(Target),
    Undo,
    Redo,
    /// Return the document to the state after this many steps (a timeline block).
    JumpHistory(usize),
    /// A change to the layer stack or a layer panel view state.
    Layers(layers::LayerEvent),
    /// A layer shortcut, resolved against the layer panel's selection.
    LayerAction(layers::LayerAction),
    /// A request from the asset tray: a dropped asset, changed library settings.
    Assets(TrayEvent),
    /// A request from the bakers panel.
    Bakers(bakers::BakerEvent),
    /// A request from the projection panel.
    Projection(projection::ProjectionEvent),
    /// A request from the shader settings panel.
    Shader(shader_settings::ShaderEvent),
    /// A tool key or quick-access control: quick mask, polygon fill settings.
    Tool(crate::gui_paint::tools::ToolEvent),
}
