//! Painter's polygon fill (`DataActionDecal`) → our polygon fill effects. Painter stores the hits (clicks and
//! dragged rectangles, see [`crate::model::DecalHit`]), not the triangles; the importer resolves them on the set's
//! mesh ([`geometry::SetGeometry`], [`pick`]) into the triangles each covers. Consecutive hits writing the same
//! value with the same granularity become one effect; later ones paint over earlier ones, as in Painter.

pub mod geometry;
pub mod pick;

use super::computability::Computability;
use crate::model::{HitGranularity, PainterView, PolygonFillEffect};
use crate::translate::blockers::Blocker;
use geometry::SetGeometry;
use materialcraft::paint::effect::{Pick, PickMode, Selection};
use std::collections::{BTreeMap, BTreeSet};

/// One effect's worth of hits: the value they write and the triangles they cover.
#[derive(Debug, Clone, PartialEq)]
pub struct DecalRun {
    /// The hits' `color` (a grey in masks).
    pub color: [f32; 3],
    pub selection: Selection,
}

/// The runs of one decal and how exactly they reproduce it.
#[derive(Debug, Clone, PartialEq)]
pub struct DecalRuns {
    pub runs: Vec<DecalRun>,
    pub status: Computability,
}

fn mode(g: HitGranularity) -> PickMode {
    match g {
        HitGranularity::Triangle | HitGranularity::Other(_) => PickMode::Triangle,
        HitGranularity::Polygon => PickMode::Polygon,
        HitGranularity::Mesh => PickMode::Mesh,
        HitGranularity::UvIsland => PickMode::UvIsland,
    }
}

/// Resolves the decal's hits on `geometry` (`None`: the set's mesh is not available).
pub fn runs(geometry: Option<&SetGeometry>, decal: &PolygonFillEffect) -> DecalRuns {
    let mut status = Computability::Exact;
    let Some(geometry) = geometry else {
        return DecalRuns {
            runs: Vec::new(),
            status: Computability::not_computable(Blocker::feature(
                "polygon-fill-without-mesh",
                "polygon fill without the mesh",
            )),
        };
    };
    if decal.unread > 0 {
        status.and(&Computability::not_computable(Blocker::feature(
            "polygon-fill-unread-picks",
            format!("polygon fill: {} picks not read", decal.unread),
        )));
    }
    // (colour, granularity, triangles by part name) per run
    let mut runs: Vec<([f32; 3], HitGranularity, BTreeMap<String, BTreeSet<u32>>)> = Vec::new();
    for hit in &decal.hits {
        if hit.view == PainterView::Mesh {
            // fitted on one project, where one island still differs (see `pick`)
            status.and(&Computability::approximated("polygon fill picked in the 3D view"));
        }
        let picked = match pick::pick(geometry, hit) {
            Ok(p) => p,
            Err(e) => {
                status.and(&Computability::not_computable(Blocker::feature(
                    "polygon-fill-pick",
                    format!("polygon fill pick: {e:?}"),
                )));
                continue;
            }
        };
        if !runs
            .last()
            .is_some_and(|(c, g, _)| *c == hit.color && *g == hit.granularity)
        {
            runs.push((hit.color, hit.granularity, BTreeMap::new()));
        }
        let Some((_, _, parts)) = runs.last_mut() else { continue };
        for (p, tris) in picked {
            if let Some(part) = geometry.parts.get(p) {
                parts.entry(part.name().to_string()).or_default().extend(tris);
            }
        }
    }
    let runs = runs
        .into_iter()
        .map(|(color, g, parts)| DecalRun {
            color,
            selection: Selection {
                mode: mode(g),
                picks: parts
                    .into_iter()
                    .map(|(part, tris)| Pick {
                        part,
                        triangles: tris.into_iter().collect(),
                    })
                    .collect(),
            },
        })
        .collect();
    DecalRuns { runs, status }
}
