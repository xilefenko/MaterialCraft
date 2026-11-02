//! Painter's baking settings for one texture set mapped onto ours: [`MappedBake::of`] turns a
//! [`MaterialBaking`] into the baker's [`BakeSettings`] (and the document's [`SetBakeConfig`]), listing every
//! setting we cannot reproduce as [`Unmapped`]. The importer stores the result in each texture set; `mcparity`
//! bakes with the very same mapping, so there is one table only. Per-baker settings are mapped in the `bakers` module, the
//! high-poly source in `high`.

mod bakers;
mod high;
mod project;
#[cfg(test)]
mod tests;
mod uids;

pub use bakers::flag;
pub use high::HighPolyRequest;
pub use project::ProjectBaking;
pub use uids::material_uids;

use crate::model::baking::{MaterialBaking, TweakList, TweakValue};
use bakers::map_baker;
use materialcraft::bake::{BakeSettings, MapKind};
use materialcraft::paint::BakedKind;
use materialcraft::paint::bake_config::{HighPlacement, SetBakeConfig};

/// Painter's map usages (`DataAdditionalMap.usage`, `BakingDefinition.usage`).
pub mod usage {
    pub const ID: u64 = 0;
    pub const AO: u64 = 1;
    pub const CURVATURE: u64 = 2;
    pub const WORLD_NORMAL: u64 = 3;
    pub const POSITION: u64 = 4;
    pub const THICKNESS: u64 = 5;
    pub const NORMAL: u64 = 6;
}

/// Painter's baker usage of a baked map kind (`None` for kinds Painter has no baker for).
pub fn baked_usage(kind: &BakedKind) -> Option<u64> {
    Some(match kind {
        BakedKind::Id => usage::ID,
        BakedKind::AmbientOcclusion => usage::AO,
        BakedKind::Curvature => usage::CURVATURE,
        BakedKind::WorldSpaceNormal => usage::WORLD_NORMAL,
        BakedKind::Position => usage::POSITION,
        BakedKind::Thickness => usage::THICKNESS,
        BakedKind::Normal => usage::NORMAL,
        BakedKind::Other(_) => return None,
    })
}

/// Our map kind of Painter's baker `usage`.
pub fn usage_kind(usage: u64) -> Option<MapKind> {
    Some(match usage {
        usage::ID => MapKind::Id,
        usage::AO => MapKind::AmbientOcclusion,
        usage::CURVATURE => MapKind::Curvature,
        usage::WORLD_NORMAL => MapKind::WorldNormal,
        usage::POSITION => MapKind::Position,
        usage::THICKNESS => MapKind::Thickness,
        usage::NORMAL => MapKind::Normal,
        _ => return None,
    })
}

/// A Painter setting our baker has no counterpart for. `usage` = the baker it belongs to (`None` = all).
#[derive(Debug, Clone, PartialEq)]
pub struct Unmapped {
    pub usage: Option<u64>,
    pub text: String,
}

/// Detail settings that change nothing inside the islands or only steer inputs we resolve elsewhere.
const NO_EFFECT: &[&str] = &[
    "Detail.LowAsHigh",
    "Detail.HipolyMesh",
    "Detail.CageMesh",
    "Detail.LowpolySuffix",
    "Detail.HipolySuffix",
    "Detail.BackfaceCullingSuffix",
];

/// Our samples per texel side for Painter's antialiasing setting (`Detail.SubSampling` > 0).
const ANTIALIASED_SAMPLES: u32 = 2;

/// One set's settings mapped onto ours.
#[derive(Debug, Clone)]
pub struct MappedBake {
    /// What our baker runs with (every map selected; [`Self::config`] keeps which bakers Painter had on).
    pub settings: BakeSettings,
    /// Bakers Painter had enabled (all when the project has no settings).
    pub enabled: Vec<MapKind>,
    /// Where Painter took the high-poly mesh from.
    pub high: HighPolyRequest,
    pub unmapped: Vec<Unmapped>,
}

impl MappedBake {
    /// Maps `painter` (the set's settings, or none) onto a bake of `size`² texels.
    pub fn of(painter: Option<&MaterialBaking>, size: usize) -> MappedBake {
        let mut bake = BakeSettings {
            size,
            ..BakeSettings::default()
        };
        let mut unmapped = Vec::new();
        let mut note = |usage: Option<u64>, text: String| unmapped.push(Unmapped { usage, text });
        let Some(p) = painter else {
            note(None, "no baking settings in the project; our defaults used".into());
            return MappedBake {
                settings: bake,
                enabled: MapKind::ALL.to_vec(),
                high: HighPolyRequest::default(),
                unmapped,
            };
        };
        if let Some(w) = p.common.f32("Common.DilationWidth") {
            bake.padding = w.max(0.0) as usize;
        }
        for (k, v) in &p.detail.tweaks {
            if !map_detail(k, v, &mut bake) {
                note(None, format!("{k} = {v}"));
            }
        }
        for baker in p.bakers.iter().filter(|b| b.enabled) {
            for (k, v) in &baker.params.tweaks {
                if !map_baker(baker.usage, k, v, &mut bake) {
                    note(baker.usage, format!("{} {k} = {v}", short_baker(&baker.params)));
                }
            }
        }
        let enabled = p
            .bakers
            .iter()
            .filter(|b| b.enabled)
            .filter_map(|b| usage_kind(b.usage?))
            .collect();
        MappedBake {
            settings: bake,
            enabled,
            high: HighPolyRequest::of(p),
            unmapped,
        }
    }

    /// The document's configuration of the set: our settings, Painter's enabled bakers included, the high-poly
    /// source moved into the document's space by `placement` (the mesh's normalisation).
    pub fn config(&self, placement: Option<HighPlacement>) -> SetBakeConfig {
        let mut config = SetBakeConfig::from_settings(&self.settings);
        for kind in MapKind::ALL {
            config.maps.flags_mut(kind).include = self.enabled.contains(&kind);
        }
        config.common.high = self.high.to_high(placement);
        config
    }
}

/// One detail (common ray) setting; false when we have no counterpart.
fn map_detail(key: &str, v: &TweakValue, bake: &mut BakeSettings) -> bool {
    match key {
        "Detail.MaxHeight" => v.as_f32().map(|d| bake.max_dist = d.max(1e-5)).is_some(),
        "Detail.MaxDepth" => v.as_f32().map(|d| bake.rear_dist = Some(d.max(1e-5))).is_some(),
        "Detail.SmoothNormals" => flag(v).map(|b| bake.average_normals = b).is_some(),
        "Detail.IgnoreBackface" => flag(v).map(|b| bake.ignore_backfaces = b).is_some(),
        "Detail.SubSampling" => v
            .as_f32()
            .map(|s| bake.samples = if s > 0.0 { ANTIALIASED_SAMPLES } else { 1 })
            .is_some(),
        // distances relative to the mesh size, as ours
        "Detail.RelativeScale" => flag(v) == Some(true),
        "Detail.UseCage" => flag(v) == Some(false),
        "Detail.FilterMethod" => v.as_f32() == Some(0.0),
        key => NO_EFFECT.contains(&key),
    }
}

/// `GLMapBakerManager.AOFromDetail` → `AOFromDetail`.
fn short_baker(list: &TweakList) -> &str {
    list.baker.rsplit('.').next().unwrap_or(&list.baker)
}
