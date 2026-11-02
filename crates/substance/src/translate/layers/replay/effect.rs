//! One paint effect's strokes painted into target images: what can be replayed at all ([`check`]), and the
//! strokes laid down one after another with the effect's mapped brush and tip ([`paint_strokes`]).

use super::super::images::ImageSource;
use super::super::strokes;
use super::brush_map::{self, MappedBrush};
use super::scene::SetScene;
use super::tip;
use super::{failed, refused};
use crate::blockers::Blocker;
use crate::model::{PaintEffect, PaintTool};
use materialcraft::paint::stroke::{StrokeData, ViewKind};
use materialcraft::stamp::ChannelTarget;
use materialcraft_brush::{PaintValues, replay_imported};

/// Why `effect` cannot be replayed, if it cannot.
pub fn check(effect: &PaintEffect) -> Result<(), Blocker> {
    match effect.tool {
        PaintTool::Brush | PaintTool::Eraser => {}
        PaintTool::Projection => return Err(refused("projection", "projection painting")),
        PaintTool::Smudge => return Err(refused("smudge", "smudge strokes")),
    }
    if effect.path_strokes > 0 {
        return Err(refused("3d-path", "3D path strokes"));
    }
    if effect.stencil.is_some() {
        return Err(refused("stencil", "painted through a stencil"));
    }
    if effect.strokes.iter().any(|s| !s.complete) {
        return Err(refused("unreadable-points", "stroke points not fully readable"));
    }
    Ok(())
}

/// Paints every stroke of `effect` into `targets` (built for the effect's values and tool), in order; returns
/// the strokes painted and the brush values that did not map.
pub fn paint_strokes(
    effect: &PaintEffect,
    scene: &SetScene,
    images: &dyn ImageSource,
    targets: &mut [ChannelTarget],
) -> Result<(usize, Vec<String>), Blocker> {
    check(effect)?;
    let mapped: MappedBrush = brush_map::map(effect)?;
    let (tip, _) = tip::tip(&effect.alpha, images)?;
    let Some(StrokeData::Record(record)) = strokes::record([effect]) else {
        return Ok((0, mapped.unknown));
    };
    let replay = scene.replay();
    // the values are in the targets; the session only records them
    let values = PaintValues {
        channels: Vec::new(),
        mask: 1.0,
    };
    for (stroke, painter) in record.strokes.iter().zip(&effect.strokes) {
        let mut brush = match stroke.view {
            ViewKind::Mesh => mapped.mesh.clone(),
            ViewKind::Uv => mapped.uv.clone(),
        };
        brush.extras.tip = Some(tip.clone());
        brush.model = scene.model;
        brush.lazy = brush_map::lazy(painter.lazy_distance);
        replay_imported(stroke, &brush, &values, &replay, targets).map_err(|e| failed(e.to_string()))?;
    }
    Ok((record.strokes.len(), mapped.unknown))
}
