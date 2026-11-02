//! The one table from Painter's brush (`DataBrush`, its `DataEmitterStroke`, the paint effect's opacity and a
//! stroke's lazy mouse) to ours ([`ReplayBrush`]: a [`BrushPreset`] plus [`EngineExtras`]). Every Painter field
//! is either mapped here, deliberately ignored (with why), or reported as unknown when it holds a value we
//! have not seen the meaning of. Numbers marked *fitted* come from replaying painted masks against Painter's
//! texture cache (`mcparity brush.replay`); the corpus has too few measurable cases to confirm them yet.
//!
//! | Painter | Ours |
//! |---|---|
//! | `brushSize` (`brushSizeSpace` 1) | [`SizeSpace::Model`] size: `brushSize ×` [`SIZE_TO_RADIUS`] (provisional) of the whole mesh's radius in 3D; in the 2D view a fraction of the texture: `brushSize ×` [`UV_SIZE`] |
//! | `brushSizeJitter`, `flowJitter` | jitter size, flow |
//! | `flow` | flow |
//! | `DataActionPaint.opacity` | stroke opacity |
//! | `spacing` | spacing (a fraction of the size on both sides) |
//! | `rotation` (radians), `rotationJitter` (radians, up to π) | angle (degrees), angle jitter (a fraction of ±180°) |
//! | `positionJitter` | position jitter (in stamp sizes) |
//! | `followPath` | follow direction |
//! | `pressureSizeEnabled`/`Minimum`, `pressureFlowEnabled`/`Minimum` | pressure use and [`EngineExtras`] minimums |
//! | `alignment` 3 / 1 / 2 | placement wrapped / UV / facing the view (Inferred) |
//! | `cullingEnabled`, `cullingAngle` | facing limit (the angle is not fitted: the default limit is kept) |
//! | `lazyDistance` | lazy mouse string, a fraction of the view's height |
//! | `paintThrough`, `filtering`, `positionJitterBothAxes`/`Distribution` | not used (no effect measured) |
//! | `DataActionPaint.stampBlendingMode` 1, `aspectRatio` ≠ 1, `alignment` other than 1–3 | reported as unmapped |
//! | `DataEmitterPhysics`, `DataEmitterStroke3D` | not replayable |

use crate::model::{PaintEffect, PainterBrush, PainterEmitter, StrokeEmitter};
use materialcraft::paint::brush::{BrushPreset, Jitter, Placement, PressureUse, SizeSpace};
use materialcraft_brush::{EngineExtras, ReplayBrush};

/// Painter's `brushSize` (3D view) as a fraction of the whole mesh's bounding radius. Provisional: fitted on one
/// project (where fully painted texels of a covering layer agree best, AMULET 3); not confirmed on others.
pub const SIZE_TO_RADIUS: f32 = 0.85;
/// Painter's `brushSize` (2D view) as a fraction of the texture's width. Not fitted (no measurable case).
pub const UV_SIZE: f32 = 1.0;
/// Height of the viewport 3D-view strokes are replayed on, in screen units: fine enough that a stamp's
/// footprint is not coarser than the texels it lands on at 4K.
pub const VIEWPORT_HEIGHT: f32 = 2048.0;

/// A Painter brush as ours, for one view kind, with what did not map.
#[derive(Debug, Clone, PartialEq)]
pub struct MappedBrush {
    /// The brush for 3D-view strokes and the one for 2D-view strokes (they differ in size only).
    pub mesh: ReplayBrush,
    pub uv: ReplayBrush,
    /// Values we have no mapping for (`field value`), for the report.
    pub unknown: Vec<String>,
}

use super::refused;
pub use crate::blockers::Blocker;

/// Maps the brush of `effect`; `None` when the effect has no brush (Painter's defaults are taken then).
pub fn map(effect: &PaintEffect) -> Result<MappedBrush, Blocker> {
    let default = PainterBrush::default();
    let brush = effect.brush.as_ref().unwrap_or(&default);
    let emitter = match &brush.emitter {
        PainterEmitter::Stroke(e) => e,
        PainterEmitter::Physics { .. } => return Err(refused("particle", "particle brush (physics emitter)")),
        PainterEmitter::Stroke3D(_) => return Err(refused("3d-path", "3D path brush")),
        PainterEmitter::Other(class) if class.is_empty() => {
            return Err(refused("no-emitter", "brush without an emitter"));
        }
        PainterEmitter::Other(class) => return Err(Blocker::class(class, format!("brush emitter {class}"))),
    };
    let mut unknown = Vec::new();
    let placement = match brush.alignment {
        3 => Placement::Wrapped,
        1 => Placement::UvAligned,
        2 => Placement::FacingView,
        n => {
            unknown.push(format!("alignment {n}"));
            Placement::Wrapped
        }
    };
    if brush.size_space != 1 {
        unknown.push(format!("brushSizeSpace {}", brush.size_space));
    }
    if effect.stamp_blending != 0 {
        unknown.push(format!("stampBlendingMode {}", effect.stamp_blending));
    }
    if (emitter.aspect_ratio - 1.0).abs() > 1e-3 {
        unknown.push(format!("aspectRatio {}", emitter.aspect_ratio));
    }
    let preset = preset(brush, emitter, effect.opacity, placement);
    let extras = EngineExtras {
        tip: None,
        size_minimum: if emitter.pressure_size.enabled {
            emitter.pressure_size.minimum
        } else {
            0.0
        },
        flow_minimum: if emitter.pressure_flow.enabled {
            emitter.pressure_flow.minimum
        } else {
            0.0
        },
    };
    let mesh = ReplayBrush {
        preset: preset.clone(),
        extras: extras.clone(),
        lazy: 0.0,
        viewport_height: VIEWPORT_HEIGHT,
        occlusion: brush.culling.is_some(),
        model: None,
    };
    let uv = ReplayBrush {
        preset: BrushPreset {
            size: brush.size * UV_SIZE,
            ..preset
        },
        extras,
        lazy: 0.0,
        viewport_height: VIEWPORT_HEIGHT,
        occlusion: false,
        model: None,
    };
    Ok(MappedBrush { mesh, uv, unknown })
}

/// Painter's `lazyDistance` as our lazy string (a fraction of the view's height).
pub fn lazy(distance: f32) -> f32 {
    if distance.is_finite() && distance > 0.0 {
        distance
    } else {
        0.0
    }
}

fn preset(brush: &PainterBrush, e: &StrokeEmitter, opacity: f32, placement: Placement) -> BrushPreset {
    BrushPreset {
        name: format!("Painter brush {}", brush.uid.map_or("?".into(), |u| u.to_string())),
        size: brush.size * SIZE_TO_RADIUS,
        size_space: SizeSpace::Model,
        hardness: 1.0,
        flow: brush.flow,
        opacity,
        spacing: e.spacing,
        angle: e.rotation.to_degrees(),
        follow_direction: e.follow_path,
        roundness: 1.0,
        jitter: Jitter {
            size: brush.size_jitter,
            flow: brush.flow_jitter,
            angle: e.rotation_jitter / std::f32::consts::PI,
            position: e.position_jitter,
        },
        pressure: PressureUse {
            size: e.pressure_size.enabled,
            flow: e.pressure_flow.enabled,
        },
        placement,
        ..BrushPreset::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EffectHeader, PaintTool, PressureResponse};

    fn effect(brush: PainterBrush) -> PaintEffect {
        PaintEffect {
            header: EffectHeader {
                uid: None,
                enabled: true,
                label: String::new(),
                channels: crate::model::ChannelMask(0),
                blending: vec![],
            },
            tool: PaintTool::Brush,
            opacity: 0.5,
            strokes: vec![],
            path_strokes: 0,
            brush_uid: brush.uid,
            brush: Some(brush),
            stamp_blending: 0,
            colors: vec![],
            alpha: vec![],
            stencil: None,
        }
    }

    #[test]
    fn painter_brushes_map_onto_our_presets() {
        let emitter = StrokeEmitter {
            spacing: 0.15,
            rotation: std::f32::consts::FRAC_PI_2,
            rotation_jitter: std::f32::consts::PI,
            position_jitter: 0.3,
            follow_path: true,
            pressure_size: PressureResponse {
                enabled: true,
                minimum: 0.2,
            },
            pressure_flow: PressureResponse {
                enabled: true,
                minimum: 0.1,
            },
            ..StrokeEmitter::default()
        };
        let brush = PainterBrush {
            size: 0.1,
            size_jitter: 0.5,
            flow: 0.8,
            flow_jitter: 0.25,
            alignment: 2,
            emitter: PainterEmitter::Stroke(emitter),
            ..PainterBrush::default()
        };
        let mapped = map(&effect(brush)).unwrap();
        let p = &mapped.mesh.preset;
        assert!((p.size - 0.1 * SIZE_TO_RADIUS).abs() < 1e-6 && p.size_space == SizeSpace::Model);
        assert!((mapped.uv.preset.size - 0.1 * UV_SIZE).abs() < 1e-6);
        assert_eq!(
            (p.flow, p.opacity, p.spacing, p.follow_direction, p.placement),
            (0.8, 0.5, 0.15, true, Placement::FacingView)
        );
        assert!((p.angle - 90.0).abs() < 1e-3 && (p.jitter.angle - 1.0).abs() < 1e-6);
        assert_eq!((p.jitter.size, p.jitter.flow, p.jitter.position), (0.5, 0.25, 0.3));
        assert!(p.pressure.size && p.pressure.flow);
        assert_eq!(
            (mapped.mesh.extras.size_minimum, mapped.mesh.extras.flow_minimum),
            (0.2, 0.1)
        );
        assert!(mapped.unknown.is_empty() && mapped.mesh.occlusion && !mapped.uv.occlusion);
    }

    #[test]
    fn unknown_values_are_reported_and_particle_brushes_refused() {
        let odd = PainterBrush {
            alignment: 7,
            size_space: 2,
            ..PainterBrush::default()
        };
        assert_eq!(map(&effect(odd)).unwrap().unknown, ["alignment 7", "brushSizeSpace 2"]);
        let physics = PainterBrush {
            emitter: PainterEmitter::Physics {
                emitter: "a".into(),
                receiver: "b".into(),
            },
            ..PainterBrush::default()
        };
        assert!(map(&effect(physics)).is_err());
    }
}
