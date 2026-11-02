//! The Paint screen's panels as the workspace knows them, and its built-in layouts.

use craft_ui::docking::{Layout, Node};
use craft_ui::layout::{SplitAxis, SplitSize};

use crate::gui_widgets::Icon;
use crate::gui_workspace::{PanelKind, Preset};

/// Every panel of the Paint screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaintPanel {
    /// The 3D view with its toolbar: the protected canvas.
    Viewport,
    TextureSets,
    /// The layer stack of the focused texture set.
    Layers,
    ImportReport,
    /// The locked layers grouped by what blocks them.
    Blockers,
    /// Display settings (how the viewport shows the document).
    Display,
    Shortcuts,
    /// Interface settings (WS-10).
    Interface,
    /// The session log (LG-01).
    Log,
    /// The asset tray (AS-08).
    Assets,
    /// The bakers panel (BK2-01).
    Bakers,
    /// The brush tools' settings (PT-04 … PT-11).
    Brush,
    /// The selected fill's projection (PJ-01 … PJ-25).
    Projection,
    /// The focused texture set's shader instance (VS-02 … VS-12).
    Shader,
}

impl PanelKind for PaintPanel {
    const ALL: &'static [PaintPanel] = &[
        PaintPanel::Viewport,
        PaintPanel::TextureSets,
        PaintPanel::Layers,
        PaintPanel::ImportReport,
        PaintPanel::Blockers,
        PaintPanel::Display,
        PaintPanel::Shortcuts,
        PaintPanel::Interface,
        PaintPanel::Log,
        PaintPanel::Assets,
        PaintPanel::Bakers,
        PaintPanel::Brush,
        PaintPanel::Projection,
        PaintPanel::Shader,
    ];

    fn id(self) -> &'static str {
        match self {
            PaintPanel::Viewport => "viewport",
            PaintPanel::TextureSets => "texture_sets",
            PaintPanel::Layers => "layers",
            PaintPanel::ImportReport => "import_report",
            PaintPanel::Blockers => "blockers",
            PaintPanel::Display => "display_settings",
            PaintPanel::Shortcuts => "shortcuts",
            PaintPanel::Interface => "interface_settings",
            PaintPanel::Log => "log",
            PaintPanel::Assets => "assets",
            PaintPanel::Bakers => "bakers",
            PaintPanel::Brush => "brush",
            PaintPanel::Projection => "projection",
            PaintPanel::Shader => "shader_settings",
        }
    }

    fn title(self) -> &'static str {
        match self {
            PaintPanel::Viewport => "Viewport",
            PaintPanel::TextureSets => "Texture sets",
            PaintPanel::Layers => "Layers",
            PaintPanel::ImportReport => "Import report",
            PaintPanel::Blockers => "Blockers",
            PaintPanel::Display => "Display settings",
            PaintPanel::Shortcuts => "Shortcuts",
            PaintPanel::Interface => "Interface settings",
            PaintPanel::Log => "Log",
            PaintPanel::Assets => "Assets",
            PaintPanel::Bakers => "Bakers",
            PaintPanel::Brush => "Brush",
            PaintPanel::Projection => "Projection",
            PaintPanel::Shader => "Shader settings",
        }
    }

    fn icon(self) -> Icon {
        match self {
            PaintPanel::Viewport => Icon::Viewport,
            PaintPanel::TextureSets => Icon::Layers,
            PaintPanel::Layers => Icon::Stack,
            PaintPanel::ImportReport => Icon::Report,
            PaintPanel::Blockers => Icon::Lock,
            PaintPanel::Display => Icon::Display,
            PaintPanel::Shortcuts => Icon::Keyboard,
            PaintPanel::Interface => Icon::Interface,
            PaintPanel::Log => Icon::Log,
            PaintPanel::Assets => Icon::Assets,
            PaintPanel::Bakers => Icon::Bake,
            PaintPanel::Brush => Icon::Brush,
            PaintPanel::Projection => Icon::Projection,
            PaintPanel::Shader => Icon::Shader,
        }
    }

    fn is_canvas(self) -> bool {
        self == PaintPanel::Viewport
    }
}

/// The layout a new user starts with.
pub const DEFAULT_LAYOUT: &str = "Painting";

/// The built-in layouts. "Baking" is bake mode: the bakers panel next to the viewport (ADR 0008 VQ18).
pub const PRESETS: &[Preset<PaintPanel>] = &[Preset { name: DEFAULT_LAYOUT, build: painting }, Preset { name: "Baking", build: baking }];

fn tabs(panels: &[PaintPanel]) -> Node<PaintPanel> {
    Node::Tabs { panels: panels.to_vec(), active: 0 }
}

fn split(axis: SplitAxis, size: SplitSize, first: Node<PaintPanel>, second: Node<PaintPanel>) -> Node<PaintPanel> {
    Node::Split { axis, size, first: Box::new(first), second: Box::new(second) }
}

/// Texture sets above the asset tray (with the import report, the blockers and the log) on the left (AS-08), the viewport,
/// layers above the brush, projection, display and shader settings on the right.
fn painting() -> Layout<PaintPanel> {
    let left = split(
        SplitAxis::Vertical,
        SplitSize::Ratio(0.3),
        tabs(&[PaintPanel::TextureSets]),
        tabs(&[PaintPanel::Assets, PaintPanel::ImportReport, PaintPanel::Blockers, PaintPanel::Log]),
    );
    let right = split(
        SplitAxis::Vertical,
        SplitSize::Ratio(0.6),
        tabs(&[PaintPanel::Layers]),
        tabs(&[PaintPanel::Brush, PaintPanel::Projection, PaintPanel::Display, PaintPanel::Shader]),
    );
    let centre = split(SplitAxis::Horizontal, SplitSize::FixedSecond(320.0), tabs(&[PaintPanel::Viewport]), right);
    Layout { root: Some(split(SplitAxis::Horizontal, SplitSize::FixedFirst(280.0), left, centre)), floating: Vec::new() }
}

/// A wide viewport with the bakers panel on the right, above texture sets, display settings and the log.
fn baking() -> Layout<PaintPanel> {
    let right = split(
        SplitAxis::Vertical,
        SplitSize::Ratio(0.62),
        tabs(&[PaintPanel::Bakers]),
        tabs(&[
            PaintPanel::TextureSets,
            PaintPanel::Log,
            PaintPanel::Display,
            PaintPanel::Assets,
            PaintPanel::Layers,
            PaintPanel::ImportReport,
            PaintPanel::Blockers,
        ]),
    );
    Layout { root: Some(split(SplitAxis::Horizontal, SplitSize::FixedSecond(340.0), tabs(&[PaintPanel::Viewport]), right)), floating: Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui_workspace::dock::check;

    #[test]
    fn ids_are_unique_and_every_preset_is_valid() {
        for p in PaintPanel::ALL {
            assert_eq!(PaintPanel::from_id(p.id()), Some(*p));
        }
        for preset in PRESETS {
            check(&(preset.build)()).unwrap_or_else(|e| panic!("{}: {e}", preset.name));
        }
    }
}
