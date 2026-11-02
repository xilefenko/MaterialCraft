//! `DataBrush` and its `stampsEmitter` → [`PainterBrush`].

use super::ModelBuilder;
use crate::model::brush::{PainterBrush, PainterEmitter, PressureResponse, StrokeEmitter};
use crate::model::raw::RawObject;

impl ModelBuilder {
    pub(super) fn brush(&mut self, b: &RawObject) -> PainterBrush {
        self.seen(b);
        let d = PainterBrush::default();
        let culling = match b.bool("cullingEnabled").unwrap_or(true) {
            true => Some(b.f32("cullingAngle").unwrap_or(0.5)),
            false => None,
        };
        PainterBrush {
            uid: b.uid(),
            size: b.f32("brushSize").unwrap_or(d.size),
            size_space: b.u64("brushSizeSpace").unwrap_or(d.size_space),
            size_jitter: b.f32("brushSizeJitter").unwrap_or(0.0),
            flow: b.f32("flow").unwrap_or(1.0),
            flow_jitter: b.f32("flowJitter").unwrap_or(0.0),
            alignment: b.u64("alignment").unwrap_or(d.alignment),
            culling,
            paint_through: b.u64("paintThrough").unwrap_or(0),
            filtering: b.u64("filtering").unwrap_or(0),
            emitter: match b.object("stampsEmitter") {
                Some(e) => self.emitter(e),
                None => PainterEmitter::Other(String::new()),
            },
        }
    }

    fn emitter(&mut self, e: &RawObject) -> PainterEmitter {
        match e.class.as_str() {
            "DataEmitterStroke" => {
                self.seen(e);
                PainterEmitter::Stroke(stroke_emitter(e))
            }
            "DataEmitterStroke3D" => {
                self.seen(e);
                PainterEmitter::Stroke3D(stroke_emitter(e))
            }
            "DataEmitterPhysics" => {
                self.seen(e);
                PainterEmitter::Physics {
                    emitter: e.string("urlEmitter").unwrap_or_default().to_string(),
                    receiver: e.string("urlReceiver").unwrap_or_default().to_string(),
                }
            }
            other => PainterEmitter::Other(other.to_string()),
        }
    }
}

fn stroke_emitter(e: &RawObject) -> StrokeEmitter {
    let d = StrokeEmitter::default();
    // old files (format 51) store `pressure2Size` / `pressure2Flow` (0 or 1) instead of the switches
    let response = |enabled: &str, old: &str, minimum: &str, default: PressureResponse| PressureResponse {
        enabled: e
            .bool(enabled)
            .or_else(|| e.f32(old).map(|v| v > 0.0))
            .unwrap_or(default.enabled),
        minimum: e.f32(minimum).unwrap_or(default.minimum),
    };
    StrokeEmitter {
        spacing: e.f32("spacing").unwrap_or(d.spacing),
        position_jitter: e.f32("positionJitter").unwrap_or(0.0),
        position_jitter_both_axes: e.bool("positionJitterBothAxes").unwrap_or(true),
        position_jitter_distribution: e.u64("positionJitterDistribution").unwrap_or(1),
        rotation: e.f32("rotation").unwrap_or(0.0),
        rotation_jitter: e.f32("rotationJitter").unwrap_or(0.0),
        follow_path: e.bool("followPath").unwrap_or(false),
        pressure_size: response(
            "pressureSizeEnabled",
            "pressure2Size",
            "pressureSizeMinimum",
            d.pressure_size,
        ),
        pressure_flow: response(
            "pressureFlowEnabled",
            "pressure2Flow",
            "pressureFlowMinimum",
            d.pressure_flow,
        ),
        aspect_ratio: e.f32("aspectRatio").unwrap_or(1.0),
    }
}
