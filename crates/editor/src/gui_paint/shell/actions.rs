//! Every action of the Paint screen with its default bindings (WS-12, PT-59), and what a
//! key press of each one does. Tool keys are the defaults the specs name (ADR 0008 mc2-q-pt-15);
//! navigation is Alt-based with a separate light gesture (mc2-vq12). The node editor keeps its own keys.

use super::panels::PaintPanel;
use super::tools::Tool;
use crate::gui_paint::channel_view::ChannelView;
use crate::gui_paint::panels::PanelEvent;
use crate::gui_paint::panels::layers::LayerAction;
use crate::gui_paint::tools::ToolEvent;
use crate::gui_workspace::shortcuts::{ActionSpec, Kind};
use materialcraft::paint::edit::ops::NewLayer;
use materialcraft::paint::effect::PickMode;

const fn press(id: &'static str, label: &'static str, group: &'static str, defaults: &'static [&'static str]) -> ActionSpec {
    ActionSpec { id, label, group, kind: Kind::Press, defaults }
}

const fn hold(id: &'static str, label: &'static str, group: &'static str, defaults: &'static [&'static str]) -> ActionSpec {
    ActionSpec { id, label, group, kind: Kind::Hold, defaults }
}

const fn gesture(id: &'static str, label: &'static str, group: &'static str, defaults: &'static [&'static str]) -> ActionSpec {
    ActionSpec { id, label, group, kind: Kind::Gesture, defaults }
}

pub const ORBIT: &str = "paint.nav.orbit";
pub const ORBIT_SNAP: &str = "paint.nav.orbit_snap";
pub const PAN: &str = "paint.nav.pan";
pub const ZOOM: &str = "paint.nav.zoom";
pub const LIGHT: &str = "paint.nav.light";
/// The brush tools' stroke gesture.
pub const STROKE: &str = "paint.stroke";
/// A straight line from the previous stroke's end (PT-31).
pub const STROKE_LINE: &str = "paint.stroke.line";
/// The same in 5° steps (PT-32).
pub const STROKE_LINE_SNAP: &str = "paint.stroke.line_snap";
/// Held: drags move (middle), turn (left; with Shift in 90° steps) or scale (right) the stencil or the projection
/// image (PT-15, PJ-26).
pub const STENCIL: &str = "paint.tool.stencil";
/// Held: the stencil is hidden from the view.
pub const STENCIL_HIDE: &str = "paint.tool.stencil_hide";

const FILE: &str = "File";
const EDIT: &str = "Edit";
const LAYERS: &str = "Layers";
const VIEW: &str = "View";
const NAV: &str = "Navigation";
const WINDOW: &str = "Window";
const TOOLS: &str = "Tools";
const TOOL_KEYS: &str = "Tool modifiers";

/// The Paint screen's actions, in the order the shortcuts page lists them.
pub const ACTIONS: &[ActionSpec] = &[
    press("paint.new", "New project from a mesh", FILE, &["Mod+N"]),
    press("paint.open", "Open document", FILE, &["Mod+O"]),
    press("paint.import", "Import Substance Painter project", FILE, &["Mod+I"]),
    press("paint.save", "Save document", FILE, &["Mod+S"]),
    press("paint.save_as", "Save document as", FILE, &["Mod+Shift+S"]),
    press("paint.close", "Close document", FILE, &["Mod+W"]),
    press("paint.home", "Home screen", FILE, &[]),
    press("paint.edit.undo", "Undo", EDIT, &["Mod+Z"]),
    press("paint.edit.redo", "Redo", EDIT, &["Mod+Shift+Z", "Mod+Y"]),
    press("paint.layer.new_paint", "New paint layer", LAYERS, &["Mod+Shift+N"]),
    press("paint.layer.new_fill", "New fill layer", LAYERS, &[]),
    press("paint.layer.new_folder", "New folder", LAYERS, &[]),
    press("paint.layer.new_see_through", "New see-through layer", LAYERS, &[]),
    press("paint.layer.duplicate", "Duplicate the selected layers", LAYERS, &["Mod+D"]),
    press("paint.layer.copy", "Copy the focused layer (to paste as an instance)", LAYERS, &["Mod+C"]),
    press("paint.layer.paste_instance", "Paste as instance", LAYERS, &["Mod+Shift+V"]),
    press("paint.layer.group", "Group the selected layers into a folder", LAYERS, &["Mod+G"]),
    press("paint.layer.delete", "Delete the selected layers", LAYERS, &["Delete"]),
    press("paint.layer.rename", "Rename the layer", LAYERS, &["F2"]),
    press("paint.view.reset_camera", "Reset camera", VIEW, &["F"]),
    press("paint.view.projection", "Perspective / orthographic", VIEW, &["O"]),
    press("paint.view.next_camera", "Next camera", VIEW, &["Shift+F"]),
    press("paint.view.material", "Show the material", VIEW, &["M"]),
    press("paint.view.next_channel", "Next channel", VIEW, &["C"]),
    press("paint.view.previous_channel", "Previous channel", VIEW, &["Shift+C"]),
    gesture(ORBIT, "Orbit", NAV, &["Alt+LeftDrag"]),
    gesture(ORBIT_SNAP, "Orbit, snapping to axis views", NAV, &["Alt+Shift+LeftDrag"]),
    gesture(PAN, "Pan", NAV, &["Alt+MiddleDrag"]),
    gesture(ZOOM, "Zoom", NAV, &["Alt+RightDrag", "Wheel"]),
    gesture(LIGHT, "Turn the light", NAV, &["Shift+RightDrag"]),
    press("paint.window.texture_sets", "Show or hide Texture sets", WINDOW, &[]),
    press("paint.window.layers", "Show or hide Layers", WINDOW, &[]),
    press("paint.window.import_report", "Show or hide Import report", WINDOW, &[]),
    press("paint.window.blockers", "Show or hide Blockers", WINDOW, &[]),
    press("paint.window.display_settings", "Show or hide Display settings", WINDOW, &[]),
    press("paint.window.shortcuts", "Show or hide Shortcuts", WINDOW, &["Mod+Alt+K"]),
    press("paint.window.interface_settings", "Show or hide Interface settings", WINDOW, &[]),
    // Session log panel (roadmap step 29).
    press("paint.window.log", "Show or hide Log", WINDOW, &[]),
    // Asset tray (roadmap step 15).
    press("paint.window.assets", "Show or hide Assets", WINDOW, &[]),
    press("paint.window.bakers", "Show or hide Bakers", WINDOW, &[]),
    press("paint.window.brush", "Show or hide Brush", WINDOW, &[]),
    press("paint.window.projection", "Show or hide Projection", WINDOW, &[]),
    press("paint.window.shader_settings", "Show or hide Shader settings", WINDOW, &[]),
    press("paint.layout.painting", "Painting layout", WINDOW, &[]),
    press("paint.layout.baking", "Baking layout", WINDOW, &["F8"]),
    press("paint.layout.reset", "Reset the current layout", WINDOW, &[]),
    press("paint.tool.paint", "Paint", TOOLS, &["1"]),
    press("paint.tool.eraser", "Eraser", TOOLS, &["2"]),
    press("paint.tool.projection", "Projection", TOOLS, &["3"]),
    press("paint.tool.polygon_fill", "Polygon fill", TOOLS, &["4"]),
    press("paint.tool.smudge", "Smudge", TOOLS, &["5"]),
    press("paint.tool.clone", "Clone", TOOLS, &["6"]),
    press("paint.tool.material_picker", "Material picker", TOOLS, &["P"]),
    gesture(STROKE, "Paint a stroke", TOOL_KEYS, &["LeftDrag"]),
    gesture(STROKE_LINE, "Straight line from the last stroke", TOOL_KEYS, &["Shift+LeftDrag"]),
    gesture(STROKE_LINE_SNAP, "Straight line in 5° steps", TOOL_KEYS, &["Mod+Shift+LeftDrag"]),
    hold(STENCIL, "Move, turn or scale the stencil or projection", TOOL_KEYS, &["S"]),
    hold(STENCIL_HIDE, "Hide the stencil while held", TOOL_KEYS, &["N"]),
    press("paint.tool.clone_source", "Set the clone source", TOOL_KEYS, &["V"]),
    press("paint.tool.swap_fill", "Swap white and black (polygon fill)", TOOL_KEYS, &["X"]),
    press("paint.quickmask.toggle", "Quick mask on or off", TOOL_KEYS, &["Y"]),
    press("paint.quickmask.previous_tool", "Back to the previous tool", TOOL_KEYS, &["U"]),
    press("paint.quickmask.invert", "Invert the quick mask", TOOL_KEYS, &["I"]),
    press("paint.quickmask.clear", "Clear the quick mask", TOOL_KEYS, &[]),
    press("paint.polyfill.triangle", "Polygon fill picks triangles", TOOL_KEYS, &[]),
    press("paint.polyfill.polygon", "Polygon fill picks faces", TOOL_KEYS, &[]),
    press("paint.polyfill.mesh", "Polygon fill picks mesh pieces", TOOL_KEYS, &[]),
    press("paint.polyfill.uv_island", "Polygon fill picks UV islands", TOOL_KEYS, &[]),
];

/// What a key press of action `id` does; `None` for gestures and for keys that act while held (stencil).
pub fn event_for(id: &str) -> Option<PanelEvent> {
    if let Some(tool) = Tool::ALL.iter().find(|t| t.action() == id) {
        return Some(PanelEvent::SelectTool(*tool));
    }
    if let Some(panel) = id.strip_prefix("paint.window.").and_then(panel_for) {
        return Some(PanelEvent::TogglePanel(panel));
    }
    Some(match id {
        "paint.new" => PanelEvent::NewProject,
        "paint.open" => PanelEvent::Open,
        "paint.import" => PanelEvent::Import,
        "paint.save" => PanelEvent::Save,
        "paint.save_as" => PanelEvent::SaveAs,
        "paint.close" => PanelEvent::Close,
        "paint.home" => PanelEvent::Home,
        "paint.edit.undo" => PanelEvent::Undo,
        "paint.edit.redo" => PanelEvent::Redo,
        "paint.layer.new_paint" => PanelEvent::LayerAction(LayerAction::New(NewLayer::Paint)),
        "paint.layer.new_fill" => PanelEvent::LayerAction(LayerAction::New(NewLayer::Fill)),
        "paint.layer.new_folder" => PanelEvent::LayerAction(LayerAction::New(NewLayer::Folder)),
        "paint.layer.new_see_through" => PanelEvent::LayerAction(LayerAction::New(NewLayer::SeeThrough)),
        "paint.tool.clone_source" => PanelEvent::Tool(ToolEvent::MarkCloneSource),
        "paint.layer.duplicate" => PanelEvent::LayerAction(LayerAction::Duplicate),
        "paint.layer.copy" => PanelEvent::LayerAction(LayerAction::Copy),
        "paint.layer.paste_instance" => PanelEvent::LayerAction(LayerAction::PasteInstance),
        "paint.layer.group" => PanelEvent::LayerAction(LayerAction::Group),
        "paint.layer.delete" => PanelEvent::LayerAction(LayerAction::Delete),
        "paint.layer.rename" => PanelEvent::LayerAction(LayerAction::Rename),
        "paint.view.reset_camera" => PanelEvent::ResetCamera,
        "paint.view.projection" => PanelEvent::ToggleProjection,
        "paint.view.next_camera" => PanelEvent::NextCamera,
        "paint.view.material" => PanelEvent::ShowChannel(ChannelView::Material),
        "paint.view.next_channel" => PanelEvent::CycleChannel(1),
        "paint.view.previous_channel" => PanelEvent::CycleChannel(-1),
        "paint.layout.painting" => PanelEvent::SwitchLayout("Painting".into()),
        "paint.layout.baking" => PanelEvent::SwitchLayout("Baking".into()),
        "paint.layout.reset" => PanelEvent::ResetLayout,
        "paint.quickmask.previous_tool" => PanelEvent::PreviousTool,
        "paint.quickmask.toggle" => PanelEvent::Tool(ToolEvent::QuickMaskToggle),
        "paint.quickmask.invert" => PanelEvent::Tool(ToolEvent::QuickMaskInvert),
        "paint.quickmask.clear" => PanelEvent::Tool(ToolEvent::QuickMaskClear),
        "paint.tool.swap_fill" => PanelEvent::Tool(ToolEvent::SwapFill),
        "paint.polyfill.triangle" => PanelEvent::Tool(ToolEvent::PickMode(PickMode::Triangle)),
        "paint.polyfill.polygon" => PanelEvent::Tool(ToolEvent::PickMode(PickMode::Polygon)),
        "paint.polyfill.mesh" => PanelEvent::Tool(ToolEvent::PickMode(PickMode::Mesh)),
        "paint.polyfill.uv_island" => PanelEvent::Tool(ToolEvent::PickMode(PickMode::UvIsland)),
        _ => return None,
    })
}

fn panel_for(id: &str) -> Option<PaintPanel> {
    use crate::gui_workspace::PanelKind;
    PaintPanel::from_id(id).filter(|p| !p.is_canvas())
}

/// The action that shows or hides `panel`.
pub fn toggle_action(panel: PaintPanel) -> &'static str {
    match panel {
        PaintPanel::Viewport => "",
        PaintPanel::TextureSets => "paint.window.texture_sets",
        PaintPanel::Layers => "paint.window.layers",
        PaintPanel::ImportReport => "paint.window.import_report",
        PaintPanel::Blockers => "paint.window.blockers",
        PaintPanel::Display => "paint.window.display_settings",
        PaintPanel::Shortcuts => "paint.window.shortcuts",
        PaintPanel::Interface => "paint.window.interface_settings",
        PaintPanel::Log => "paint.window.log",
        PaintPanel::Assets => "paint.window.assets",
        PaintPanel::Bakers => "paint.window.bakers",
        PaintPanel::Brush => "paint.window.brush",
        PaintPanel::Projection => "paint.window.projection",
        PaintPanel::Shader => "paint.window.shader_settings",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui_paint::shell::panels::PRESETS;
    use crate::gui_workspace::PanelKind;
    use crate::gui_workspace::shortcuts::ShortcutTable;
    use crate::gui_workspace::shortcuts::page::rows;
    use std::collections::BTreeMap;

    /// Actions that are listed and rebindable and act through something else than a key press: gestures and keys
    /// held (the stencil's, read by the viewport).
    const RESERVED: &[&str] = &["paint.stroke", "paint.tool.stencil", "paint.tool.stencil_hide"];

    #[test]
    fn every_action_is_on_the_shortcuts_page_and_its_defaults_parse() {
        let (table, problems) = ShortcutTable::new(ACTIONS, &BTreeMap::new());
        assert!(problems.is_empty(), "{problems:?}");
        let listed: Vec<&str> = rows(&table, "").iter().map(|s| s.id).collect();
        let all: Vec<&str> = ACTIONS.iter().map(|s| s.id).collect();
        assert_eq!(listed, all);
        let mut ids = all.clone();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ACTIONS.len(), "ids are unique");
    }

    #[test]
    fn every_key_action_does_something_or_is_reserved_for_a_tool() {
        for spec in ACTIONS.iter().filter(|s| s.kind != Kind::Gesture) {
            assert!(event_for(spec.id).is_some() || RESERVED.contains(&spec.id), "{} has no effect", spec.id);
        }
        for id in RESERVED {
            assert!(ACTIONS.iter().any(|s| s.id == *id), "{id}");
        }
        for t in Tool::ALL {
            assert_eq!(event_for(t.action()), Some(PanelEvent::SelectTool(t)));
        }
        for p in PaintPanel::ALL.iter().filter(|p| !p.is_canvas()) {
            assert_eq!(event_for(toggle_action(*p)), Some(PanelEvent::TogglePanel(*p)), "{p:?}");
        }
        for preset in PRESETS {
            assert!(ACTIONS.iter().any(|s| event_for(s.id) == Some(PanelEvent::SwitchLayout(preset.name.into()))), "{}", preset.name);
        }
    }

    #[test]
    fn default_bindings_do_not_clash() {
        let (table, _) = ShortcutTable::new(ACTIONS, &BTreeMap::new());
        for spec in ACTIONS {
            assert!(table.conflicts(spec.id).is_empty(), "{} clashes with {:?}", spec.id, table.conflicts(spec.id).iter().map(|s| s.id).collect::<Vec<_>>());
        }
    }
}
