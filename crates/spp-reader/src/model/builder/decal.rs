//! `DataDecalHitRect` / `DataDecalHitPunctual` → [`DecalHit`].

use super::ModelBuilder;
use crate::model::decal::{DecalHit, HitGranularity, HitShape};
use crate::model::raw::RawObject;
use crate::model::stroke::{PainterView, StoredMat4};

impl ModelBuilder {
    /// One hit; `None` for a class or shape that is not a known hit (it then stays unrecognised).
    pub(super) fn decal_hit(&mut self, h: &RawObject) -> Option<DecalHit> {
        let vec2 = |name: &str| match h.floats(name) {
            Some([x, y, ..]) => Some([*x, *y]),
            _ => None,
        };
        let shape = match h.class.as_str() {
            "DataDecalHitPunctual" => HitShape::Point(vec2("position")?),
            "DataDecalHitRect" => HitShape::Rect(vec2("position0")?, vec2("position1")?),
            _ => return None,
        };
        self.seen(h);
        let mat4 = |name: &str| -> StoredMat4 {
            match h.floats(name) {
                Some(m) if m.len() == 16 => std::array::from_fn(|i| m.get(i).copied().unwrap_or(0.0)),
                _ => [0.0; 16],
            }
        };
        Some(DecalHit {
            uid: h.uid(),
            shape,
            color: h.vec3("color").unwrap_or([1.0; 3]),
            granularity: HitGranularity::from_value(h.u64("granularity").unwrap_or(0)),
            view: PainterView::from_value(h.u64("viewType").unwrap_or(1)),
            proj_view_inv: mat4("projViewInvMatrix"),
            duplicate_count: h.u64("duplicateCount").unwrap_or(0),
            duplicate_matrix: mat4("duplicateMatrix"),
            paint_through: h.u64("paintThrough").unwrap_or(0) != 0,
        })
    }
}
