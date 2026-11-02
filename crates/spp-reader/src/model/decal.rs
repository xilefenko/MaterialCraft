//! [`DecalHit`]: one pick of Painter's polygon fill tool (`DataDecalHitRect`, `DataDecalHitPunctual` in
//! `DataActionDecal.hits`): a click or a dragged rectangle, in the 2D (UV) view or in the 3D view with the
//! camera it was made with, the value it writes and how far the pick grows. Field meanings and counts:
//! `interop/painter-document-catalogue.md` (`DataDecalHit*`).

use super::stroke::{PainterView, StoredMat4};

/// Where a hit was made.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HitShape {
    /// `DataDecalHitPunctual.position`: one click.
    Point([f32; 2]),
    /// `DataDecalHitRect.position0` / `position1`: two opposite corners of a dragged rectangle.
    Rect([f32; 2], [f32; 2]),
}

/// `granularity`: what one picked triangle grows to. The numbers follow the tool's order (Inferred, fitted
/// against Painter's texture cache by `mcsub verify-layers`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitGranularity {
    Triangle,
    Polygon,
    Mesh,
    UvIsland,
    Other(u64),
}

impl HitGranularity {
    pub fn from_value(v: u64) -> HitGranularity {
        match v {
            0 => HitGranularity::Triangle,
            1 => HitGranularity::Polygon,
            2 => HitGranularity::Mesh,
            3 => HitGranularity::UvIsland,
            other => HitGranularity::Other(other),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecalHit {
    pub uid: Option<u64>,
    pub shape: HitShape,
    /// `color`: the value written into the picked triangles (a grey in masks: white adds, black removes).
    pub color: [f32; 3],
    pub granularity: HitGranularity,
    /// `viewType`.
    pub view: PainterView,
    /// `projViewInvMatrix`: the inverse of the 3D view's projection × view matrix (zero in the 2D view).
    pub proj_view_inv: StoredMat4,
    /// `duplicateCount` / `duplicateMatrix`: the symmetry copy (count 0: none).
    pub duplicate_count: u64,
    pub duplicate_matrix: StoredMat4,
    /// `paintThrough` (punctual hits of newer files): the pick also reaches hidden faces.
    pub paint_through: bool,
}
