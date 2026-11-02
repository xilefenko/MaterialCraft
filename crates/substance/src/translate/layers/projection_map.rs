//! The one mapping of Painter's fill projection settings (`DataActionFill`) to ours ([`Projection`]), with how
//! exact it is: [`ProjectionTable`]. "Exact" means measured against Painter's cache on real projects; the rest is
//! approximated until it is.
//!
//! | Painter | Ours | Status |
//! |---|---|---|
//! | `projection` 6 | texture space | exact (no placement can be edited in this mode; a stale guess would show up as a cache disagreement) |
//! | `projection` 0, identity `uvTransformation0` | UV, identity (the same pixels as texture space on one tile) | exact (bitmap fills measured within 0.33 steps on the mesh's texels; the cache's padding off the islands is not layer content, see [`super::island_texels`]) |
//! | `projection` 0 with tiling/offset/rotation | UV with that placement (radians → degrees) | approximated: the rotation/offset convention is not verified |
//! | `projection` 2 + `sideHardness` | tri-planar + blend hardness ([`VolumeConvention::hardness`]) | approximated until fitted by pixels |
//! | `projection` 1 + culling fields | planar + depth falloff (`depthCulling*`) + facing falloff (`backfaceCulling*`, angle × 180°) | approximated until fitted |
//! | `projection` 3, 4 | spherical, cylindrical (`cylinderCullingAngle` × 360°, `cylinderHideCaps`) | approximated: the mode itself is unconfirmed (no Painter test file) |
//! | `ddd*` of modes 1–4 | volume: `dddPosition` × [`VolumeConvention::position`], `dddEulerAngles` (radians, [`VolumeConvention::euler`] order) → our x-y-z degrees, `dddSize` × [`VolumeConvention::size`]; `dddWrap` 0 → clip | with the mode |
//! | `projection` 7 + `dddGrid` | warp: the volume as for modes 1–4, `gridSize` = (columns, rows), the vertices row by row with `position3D` as grid positions, `positionUV` (v up) → our image coordinate (u, 1 − v), the four tangents as handles (column = along u); `depth` → projection depth; `depthCulling*` → depth falloff | approximated: our own warp, no pixel matching |
//! | `projection` 7 with a malformed grid | texture space | approximated |
//! | other values | texture space | approximated (unknown mode) |
//! | `filtering` 2 / 0 | smooth / bilinear | 0 is a guess (nearest or bilinear): approximated where it matters |
//! | `uvSamplingWrap` 0, 1, 2, 3 | repeat none, u, v, both (tri-planar always repeats) | 1 vs 2 unverified: approximated when 1 or 2 |
//!
//! Painter's raw fields stay in the kept `document.bin` (`foreign/substance`); size modes and the second UV
//! transform are not mapped yet.

use super::computability::Computability;
use crate::model::{FillEffect, FillPlacement, GridDeformation};
use materialcraft::projection::rotation::{self, EulerOrder};
use materialcraft::projection::{
    Cylindrical, FacingFalloff, Falloff, Filter, ImagePlacement, Planar, Projection, ProjectionMode, Repeat, Spherical,
    TriPlanar, Volume, Warp, WarpGrid, WarpPoint,
};

/// How Painter's volume numbers relate to ours (see `materialcraft::projection::volume`: one scene unit is the
/// scene's largest extent, a volume of scale 1 is one unit wide). The values are fitted by pixels
/// (`mcsub verify-layers --fit`); [`VolumeConvention::PAINTER`] is the current best reading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VolumeConvention {
    /// Our volume offset per `dddPosition` unit.
    pub position: f32,
    /// Our volume scale per `dddSize` unit.
    pub size: f32,
    /// The order Painter applies `dddEulerAngles` in.
    pub euler: EulerOrder,
    /// Painter's `sideHardness` → our tri-planar blend hardness.
    pub hardness: HardnessCurve,
    /// Painter's image runs the other way along u / v of our projection coordinates (applied as a negative tiling).
    pub flip: [bool; 2],
    /// True once the convention reproduced Painter's pixels (the volume modes are reported exact then).
    pub verified: bool,
}

/// Candidate relations between Painter's `sideHardness` and our blend hardness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardnessCurve {
    Same,
    Square,
    Sqrt,
}

impl HardnessCurve {
    pub const ALL: [HardnessCurve; 3] = [HardnessCurve::Same, HardnessCurve::Square, HardnessCurve::Sqrt];

    pub fn apply(self, h: f32) -> f32 {
        let h = if h.is_finite() { h.clamp(0.0, 1.0) } else { 0.5 };
        match self {
            HardnessCurve::Same => h,
            HardnessCurve::Square => h * h,
            HardnessCurve::Sqrt => h.sqrt(),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            HardnessCurve::Same => "same",
            HardnessCurve::Square => "square",
            HardnessCurve::Sqrt => "sqrt",
        }
    }
}

impl VolumeConvention {
    /// Painter's volume as we read it: `dddPosition` in half scene units (the corpus' planar decals sit within
    /// ±1 on the model), `dddSize` 1 = a volume enclosing the scene, angles applied x, y, z. Inferred, not yet
    /// verified by pixels (requirements/projections.md §2.3).
    pub const PAINTER: VolumeConvention = VolumeConvention {
        position: 0.5,
        size: 1.0,
        euler: EulerOrder::Xyz,
        hardness: HardnessCurve::Same,
        flip: [false; 2],
        verified: false,
    };

    /// A short description for fit reports.
    pub fn label(&self) -> String {
        let flip = match self.flip {
            [false, false] => "",
            [true, false] => " flip-u",
            [false, true] => " flip-v",
            [true, true] => " flip-uv",
        };
        format!(
            "pos×{} size×{} euler {} hardness {}{flip}",
            self.position,
            self.size,
            self.euler.id(),
            self.hardness.id()
        )
    }

    /// Our volume for a fill's `ddd*` fields.
    pub fn volume(&self, p: &FillPlacement) -> Volume {
        let finite3 = |v: [f32; 3], d: f32| v.map(|c| if c.is_finite() { c } else { d });
        let position = finite3(p.position.unwrap_or([0.0; 3]), 0.0);
        let angles = finite3(p.euler_angles.unwrap_or([0.0; 3]), 0.0);
        let size = finite3(p.size.unwrap_or([1.0; 3]), 1.0);
        let ours = rotation::angles_xyz(&rotation::matrix(angles, self.euler));
        Volume {
            offset: position.map(|v| v * self.position),
            rotation: ours.map(f32::to_degrees),
            scale: size.map(|v| v * self.size),
            clip: p.volume_wrap == Some(0),
        }
    }
}

/// Our projection for a Painter fill and how exactly it stands in for Painter's.
pub struct MappedProjection {
    pub projection: Projection,
    /// Exact or approximated (never not computable: the fill is still shown).
    pub status: Computability,
    /// True if the result depends on the filter (approximated filters only matter then).
    filter_guess: bool,
}

impl MappedProjection {
    /// The status for an image of `image_size` on a set of `set_size`: a guessed filter only matters when the
    /// image is resampled or placed.
    pub fn status_for(&self, image_size: (u32, u32), set_size: (u32, u32)) -> Computability {
        let mut status = self.status.clone();
        let resampled = image_size != set_size || self.projection.mode != ProjectionMode::TextureSpace;
        if self.filter_guess && resampled {
            status.and(&Computability::approximated("Painter filtering 0 assumed bilinear"));
        }
        status
    }
}

/// Painter's mode numbers by our name, for messages (Inferred, see the module table).
fn painter_mode_name(mode: u64) -> &'static str {
    match mode {
        1 => "planar",
        2 => "tri-planar",
        3 => "spherical",
        4 => "cylindrical",
        7 => "warp",
        _ => "unknown",
    }
}

/// The projection mapping with its volume convention. [`ProjectionTable::painter`] is the importer's; the fit
/// tries others.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectionTable {
    pub volume: VolumeConvention,
}

impl Default for ProjectionTable {
    fn default() -> ProjectionTable {
        ProjectionTable::painter()
    }
}

impl ProjectionTable {
    pub fn painter() -> ProjectionTable {
        ProjectionTable {
            volume: VolumeConvention::PAINTER,
        }
    }

    pub fn with_volume(volume: VolumeConvention) -> ProjectionTable {
        ProjectionTable { volume }
    }

    pub fn map(&self, fill: &FillEffect) -> MappedProjection {
        let mut status = Computability::Exact;
        let filter = match fill.filtering {
            2 => Filter::Smooth,
            0 => Filter::Bilinear,
            other => {
                status.and(&Computability::approximated(format!(
                    "Painter filtering {other} assumed smooth"
                )));
                Filter::Smooth
            }
        };
        let p = &fill.placement;
        let volume_mode = matches!(fill.projection, 1..=4);
        let warp = (fill.projection == 7).then(|| self.warp(fill));
        let repeat = match p.uv_wrap.unwrap_or(3) {
            0 => Repeat::None,
            3 => Repeat::Both,
            1 | 2 => {
                if fill.projection != 2 {
                    status.and(&Computability::approximated(
                        "single-axis repetition (u/v order unverified)",
                    ));
                }
                if p.uv_wrap == Some(1) { Repeat::U } else { Repeat::V }
            }
            other => {
                status.and(&Computability::approximated(format!(
                    "Painter repetition {other} assumed both"
                )));
                Repeat::Both
            }
        };
        let mode = match fill.projection {
            6 => ProjectionMode::TextureSpace,
            0 => ProjectionMode::Uv,
            1..=4 => self.volume_mode(fill),
            other => {
                let why = match &warp {
                    Some(Ok(_)) => "warp grid imported as our own Bézier warp (no pixel matching)".to_string(),
                    Some(Err(e)) => format!("projection 7 (warp) with {e}; imported as texture space"),
                    None => format!(
                        "projection {other} ({}) not known; imported as texture space",
                        painter_mode_name(other)
                    ),
                };
                status.and(&Computability::approximated(why));
                match warp {
                    Some(Ok(w)) => ProjectionMode::Warp(w),
                    _ => ProjectionMode::TextureSpace,
                }
            }
        };
        let volume_mode = volume_mode || matches!(mode, ProjectionMode::Warp(_));
        if volume_mode {
            if !self.volume.verified {
                status.and(&Computability::approximated(format!(
                    "{} projection: Painter's volume convention not verified by pixels",
                    painter_mode_name(fill.projection)
                )));
            }
            if matches!(fill.projection, 3 | 4) {
                status.and(&Computability::approximated(format!(
                    "projection {} read as {} (mode unconfirmed without a Painter test file)",
                    fill.projection,
                    painter_mode_name(fill.projection)
                )));
            }
        }
        if mode == ProjectionMode::TextureSpace && fill.uv_transform.is_some_and(|t| !t.is_identity()) {
            status.and(&Computability::approximated(
                "UV tiling/offset/rotation of a texture-space fill ignored",
            ));
        }
        let mut image = ImagePlacement::default();
        if (mode == ProjectionMode::Uv || volume_mode)
            && let Some(t) = fill.uv_transform.filter(|t| !t.is_identity())
        {
            image.tiling = t.scale;
            image.rotation = t.rotation.to_degrees();
            image.offset = t.translation;
            if mode == ProjectionMode::Uv {
                status.and(&Computability::approximated(
                    "UV tiling/offset/rotation applied (convention not verified)",
                ));
            }
        }
        if volume_mode {
            for (t, flip) in image.tiling.iter_mut().zip(self.volume.flip) {
                if flip {
                    *t = -*t;
                }
            }
        }
        MappedProjection {
            projection: Projection {
                mode,
                filter,
                repeat,
                image,
                ..Projection::default()
            },
            status,
            filter_guess: fill.filtering == 0,
        }
    }

    /// The warp of a mode-7 fill, or why its grid cannot be used.
    fn warp(&self, fill: &FillEffect) -> Result<Warp, String> {
        let p = &fill.placement;
        let grid = match &p.grid {
            Some(g) => warp_grid(g)?,
            None => WarpGrid::default(),
        };
        grid.validate().map_err(|e| e.to_string())?;
        Ok(Warp {
            volume: self.volume.volume(p),
            depth_falloff: Falloff {
                on: p.depth_culling.unwrap_or(false),
                hardness: p.depth_culling_hardness.unwrap_or(0.5),
            },
            grid,
        })
    }

    fn volume_mode(&self, fill: &FillEffect) -> ProjectionMode {
        let p = &fill.placement;
        let volume = self.volume.volume(p);
        let depth = Falloff {
            on: p.depth_culling.unwrap_or(false),
            hardness: p.depth_culling_hardness.unwrap_or(0.5),
        };
        let facing = FacingFalloff {
            on: p.backface_culling.unwrap_or(false),
            angle: p.backface_culling_angle.unwrap_or(0.5) * 180.0,
            hardness: p.backface_culling_hardness.unwrap_or(0.5),
        };
        match fill.projection {
            1 => ProjectionMode::Planar(Planar {
                volume,
                depth_falloff: depth,
                facing_falloff: facing,
            }),
            2 => ProjectionMode::TriPlanar(TriPlanar {
                volume,
                blend_hardness: self.volume.hardness.apply(p.side_hardness.unwrap_or(0.5)),
            }),
            3 => ProjectionMode::Spherical(Spherical { volume }),
            _ => ProjectionMode::Cylindrical(Cylindrical {
                volume,
                facing_falloff: facing,
                coverage_angle: p.cylinder_culling_angle.unwrap_or(1.0) * 360.0,
                hide_caps: p.cylinder_hide_caps.unwrap_or(false),
            }),
        }
    }
}

/// Our warp grid for Painter's `dddGrid`.
fn warp_grid(g: &GridDeformation) -> Result<WarpGrid, String> {
    let [columns, rows] = g.size;
    let (Ok(columns), Ok(rows)) = (u32::try_from(columns), u32::try_from(rows)) else {
        return Err(format!("a warp grid of size {:?}", g.size));
    };
    let points = g
        .vertices
        .iter()
        .map(|v| WarpPoint {
            pos: v.position,
            uv: [v.uv[0], 1.0 - v.uv[1]],
            tangents: v.tangents,
        })
        .collect();
    Ok(WarpGrid {
        rows,
        columns,
        depth: if g.depth.is_finite() { g.depth.max(0.0) } else { 0.1 },
        points,
    })
}

/// [`ProjectionTable::painter`]'s mapping of `fill`.
pub fn map_projection(fill: &FillEffect) -> MappedProjection {
    ProjectionTable::painter().map(fill)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EffectHeader, UvTransform};
    use materialcraft::projection::{Projector, SurfacePoint};

    fn fill(projection: u64, filtering: u64, uv: Option<UvTransform>) -> FillEffect {
        FillEffect {
            header: EffectHeader::default(),
            sources: Vec::new(),
            projection,
            filtering,
            uv_transform: uv,
            placement: Default::default(),
        }
    }

    #[test]
    fn texture_space_and_identity_uv_are_exact() {
        for (mode, ours) in [(6, ProjectionMode::TextureSpace), (0, ProjectionMode::Uv)] {
            let m = map_projection(&fill(mode, 2, None));
            assert!(m.status.is_exact());
            assert_eq!(m.projection.mode, ours);
            assert!(m.projection.image.is_identity());
            assert!(m.status_for((8, 8), (4, 4)).is_exact());
        }
        let bilinear = map_projection(&fill(6, 0, None));
        assert_eq!(bilinear.projection.filter, Filter::Bilinear);
        assert!(bilinear.status_for((4, 4), (4, 4)).is_exact());
        assert!(!bilinear.status_for((8, 8), (4, 4)).is_exact());
    }

    #[test]
    fn placements_are_approximated() {
        let t = UvTransform {
            scale: [2.0, 3.0],
            translation: [0.1, 0.0],
            rotation: std::f32::consts::FRAC_PI_2,
        };
        let m = map_projection(&fill(0, 2, Some(t)));
        assert_eq!(m.projection.mode, ProjectionMode::Uv);
        assert_eq!(m.projection.image.tiling, [2.0, 3.0]);
        assert!((m.projection.image.rotation - 90.0).abs() < 1e-4);
        assert!(!m.status.is_exact());
        let mut wrapped = fill(6, 2, None);
        wrapped.placement.uv_wrap = Some(0);
        assert_eq!(map_projection(&wrapped).projection.repeat, Repeat::None);
    }

    #[test]
    fn warp_grids_are_imported_approximated() {
        use crate::model::GridVertex;
        // Painter's default 3 × 3 grid: rows from the bottom, image v up, handles a third of the spacing
        let mut vertices = Vec::new();
        for r in 0..3 {
            for c in 0..3 {
                let (x, y) = (c as f32 - 1.0, r as f32 - 1.0);
                vertices.push(GridVertex {
                    position: [x, y, 0.0],
                    uv: [c as f32 / 2.0, r as f32 / 2.0],
                    tangents: [
                        [-1.0 / 3.0, 0.0, 0.0],
                        [1.0 / 3.0, 0.0, 0.0],
                        [0.0, -1.0 / 3.0, 0.0],
                        [0.0, 1.0 / 3.0, 0.0],
                    ],
                });
            }
        }
        let mut warp = fill(7, 2, None);
        warp.placement.grid = Some(GridDeformation {
            size: [3, 3],
            depth: 0.1,
            vertices,
        });
        warp.placement.depth_culling = Some(true);
        warp.placement.size = Some([1.0, 1.0, 0.5]);
        let m = map_projection(&warp);
        let ProjectionMode::Warp(w) = &m.projection.mode else {
            panic!("{:?}", m.projection.mode)
        };
        assert_eq!((w.grid.rows, w.grid.columns, w.grid.depth), (3, 3, 0.1));
        assert!(w.depth_falloff.on && w.volume.scale == [1.0, 1.0, 0.5]);
        // the bottom-left vertex shows the image's bottom-left corner (v = 1 top first)
        assert_eq!(w.grid.points[0].uv, [0.0, 1.0]);
        assert!(m.status.reasons().iter().any(|r| r.contains("warp")));
        // it projects the image as the planar projection would
        let frame = materialcraft::projection::SceneFrame {
            centre: [0.0; 3],
            size: [2.0; 3],
        };
        let prepared = m.projection.prepare(Some(&frame), true).unwrap();
        let taps = prepared.project(&SurfacePoint {
            pos: [-0.5, -0.5, 0.0],
            ..SurfacePoint::default()
        });
        assert!((taps.taps[0].uv[0] - 0.25).abs() < 1e-5 && (taps.taps[0].uv[1] - 0.75).abs() < 1e-5);
        // a grid whose points do not fill it falls back to texture space
        let mut broken = warp.clone();
        if let Some(g) = &mut broken.placement.grid {
            g.vertices.truncate(4);
        }
        let b = map_projection(&broken);
        assert_eq!(b.projection.mode, ProjectionMode::TextureSpace);
        assert!(b.status.reasons().iter().any(|r| r.contains("warp")));
        // no grid stored: the regular grid
        let plain = map_projection(&fill(7, 2, None));
        assert!(matches!(plain.projection.mode, ProjectionMode::Warp(_)));
    }

    #[test]
    fn volume_modes_carry_their_fields() {
        let mut planar = fill(
            1,
            2,
            Some(UvTransform {
                scale: [2.0, 2.0],
                translation: [0.0, 0.0],
                rotation: 0.0,
            }),
        );
        planar.placement.position = Some([0.2, -1.0, 0.0]);
        planar.placement.size = Some([1.0, 1.0, 0.06]);
        planar.placement.euler_angles = Some([0.0, 0.0, std::f32::consts::FRAC_PI_2]);
        planar.placement.volume_wrap = Some(0);
        planar.placement.depth_culling = Some(true);
        planar.placement.depth_culling_hardness = Some(0.75);
        let m = map_projection(&planar);
        let ProjectionMode::Planar(p) = &m.projection.mode else {
            panic!("{:?}", m.projection.mode)
        };
        assert_eq!(p.volume.offset, [0.1, -0.5, 0.0]);
        assert_eq!(p.volume.scale, [1.0, 1.0, 0.06]);
        assert!((p.volume.rotation[2] - 90.0).abs() < 1e-3);
        assert!(p.volume.clip && p.depth_falloff.on && p.depth_falloff.hardness == 0.75);
        assert_eq!(m.projection.image.tiling, [2.0, 2.0]);
        assert!(m.status.reasons().iter().any(|r| r.contains("not verified")));
        let mut tri = fill(2, 2, None);
        tri.placement.side_hardness = Some(0.9);
        tri.placement.uv_wrap = Some(1);
        let t = map_projection(&tri);
        assert!(
            matches!(t.projection.mode, ProjectionMode::TriPlanar(TriPlanar { blend_hardness, .. }) if (blend_hardness - 0.9).abs() < 1e-6)
        );
        assert_eq!(t.projection.sampling_repeat(), Repeat::Both);
        let mut cyl = fill(4, 2, None);
        cyl.placement.cylinder_culling_angle = Some(0.5);
        let c = map_projection(&cyl);
        assert!(matches!(
            c.projection.mode,
            ProjectionMode::Cylindrical(Cylindrical {
                coverage_angle: 180.0,
                ..
            })
        ));
        assert!(c.status.reasons().iter().any(|r| r.contains("unconfirmed")));
    }
}
