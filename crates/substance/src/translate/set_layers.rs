//! [`SetLayers`]: the final layer stack of one texture set: the imported Painter layers (when the document
//! model has the set), the flattened cache layer and Painter's defaults, arranged by
//! [`StackStrategy`]. Without a model the set keeps Painter's flattened result as its only layer.

use super::blocker_summary::BlockerSummary;
use super::channel_table::painter_default;
use super::layers::cache_agreement::CacheAgreement;
use super::layers::cache_agreement::Reference;
use super::layers::correction::{CORRECTION_NAME, Correction, describe};
use super::layers::decal::geometry::SetGeometry;
use super::layers::island_texels::IslandTexels;
use super::layers::report::{report_cache_disagreement, report_hidden_masks, report_stack};
use super::layers::stack_builder::LOCK_SOURCE;
use super::layers::strokes::{StrokeCount, report_strokes};
use super::layers::{LayerImport, LazySurface, PAINTER_RESULT_NAME, SetChannels, StackParts, StackStrategy};
use super::report::Reporter;
use super::texture_sets::SetSource;
use crate::model::{PainterDocument, PainterTextureSet};
use materialcraft::mesh::{FaceGroups, Mesh, SurfaceKey, SurfaceMap};
use materialcraft::paint::{BlendMode, Fidelity, Fill, FillContext, Layer, LayerKind, NoFillSources, TextureSet};
use materialcraft::projection::SceneFrame;

pub const PAINTER_RESULT_ID: &str = "painter-result";
pub const DEFAULTS_LAYER_ID: &str = "painter-defaults";
pub const DEFAULTS_LAYER_NAME: &str = "Painter defaults";

/// The flattened cache layer of a set and how much it covers.
pub struct Flattened {
    pub layer: Option<Layer>,
    /// No texel of any cached channel has coverage below 1 (and there is at least one channel).
    pub fully_covered: bool,
}

pub struct SetLayers<'a> {
    pub import: &'a LayerImport<'a>,
    /// Painter's model of `document.bin`, if it could be built.
    pub document: Option<&'a PainterDocument>,
    /// Painter's source polygons per mesh part, for polygon-fill picks.
    pub faces: &'a [FaceGroups],
}

impl SetLayers<'_> {
    /// Fills `set.layers` (bottom → top) and reports the layer stack. `mesh` (with the set's slots merged)
    /// limits the cache agreement to the texels on the mesh.
    pub fn build(
        &self,
        set: &mut TextureSet,
        source: &SetSource,
        flattened: Flattened,
        mesh: &Mesh,
        report: &mut Reporter,
    ) {
        let name = source.record.name.clone();
        match self.painter_set(source) {
            Some(painter) => {
                let channels = SetChannels::new(&set.channels);
                let geometry = SetGeometry::new(mesh, self.faces, &[set.material_slot]);
                let surface = LazySurface::new(mesh, set.material_slot, set.uv_set, set.resolution);
                let stack = self
                    .import
                    .import_on(painter, &channels, set.resolution, Some(&geometry), Some(&surface));
                let defaults = defaults_layer(set);
                let islands = IslandTexels::rasterise(mesh, &[set.material_slot], set.uv_set, set.resolution);
                let picks = uses_picks(&stack.layers).then(|| surface_map(mesh, set)).flatten();
                let reference = Reference {
                    islands: islands.as_ref(),
                    surface: picks.as_ref(),
                };
                let agreement = match (&flattened.layer, stack.verdict.all_exact()) {
                    (Some(result), true) => CacheAgreement::measure(set, &defaults, &stack.layers, result, reference),
                    _ => CacheAgreement::default(),
                };
                let strategy = StackStrategy::choose(&stack.verdict, flattened.layer.is_some(), &agreement);
                report_stack(report, &name, &stack, strategy, &channels);
                report_strokes(
                    report,
                    &name,
                    &StrokeCount::of_set(painter),
                    &stack.layers,
                    &stack.replayed,
                );
                report_cache_disagreement(report, &name, &agreement, &channels);
                report_hidden_masks(report, &name, &stack.verdict.mask_failures);
                let parts = StackParts {
                    uncomputable_masks: stack.verdict.mask_failures.iter().map(|f| f.id.clone()).collect(),
                    imported: stack.layers,
                    painter_result: flattened.layer,
                    defaults,
                };
                set.layers = strategy.assemble(parts, &channels);
                report_blockers(report, &name, set);
                if strategy.corrects() {
                    // Every fill laid on as the editor lays it: the surface map in the scene's frame.
                    let surface = surface_map(mesh, set);
                    let fills = match &surface {
                        Some(s) => FillContext::with_sources(&NoFillSources).with_surface(s, SceneFrame::of_mesh(mesh)),
                        None => FillContext::with_sources(&NoFillSources),
                    };
                    add_correction(set, &fills, islands.as_ref(), &name, report);
                }
            }
            None => {
                if !flattened.fully_covered {
                    set.layers.push(defaults_layer(set));
                }
                set.layers.extend(flattened.layer);
                report.layers(&name, source.record.layer_count, source.record.action_count);
            }
        }
        if set.layers.iter().any(|l| l.id == DEFAULTS_LAYER_ID) {
            report.add(
                format!("texture set \"{name}\" / {DEFAULTS_LAYER_NAME}"),
                Fidelity::Approximated,
                "uncovered texels show Painter's channel defaults (uniform fill layer)",
            );
        }
    }

    /// The model's set for a document-backed source, by material uid.
    fn painter_set(&self, source: &SetSource) -> Option<&PainterTextureSet> {
        let uid = source.record.scene_uid.filter(|_| source.in_document)?;
        self.document?.texture_set(uid)
    }
}

/// One report line per blocker of the set: what it is, how many layers it locks (alone), and what unlocks it.
fn report_blockers(report: &mut Reporter, name: &str, set: &TextureSet) {
    let summary = BlockerSummary::of_set(set);
    for b in &summary.blockers {
        let text = summary
            .locked
            .get(b.layers[0])
            .and_then(|l| set.layer(&l.layer))
            .and_then(|l| l.lock.as_ref());
        let text = text
            .and_then(|lock| lock.reasons.iter().find(|r| r.key == b.key))
            .map_or(b.key.as_str(), |r| r.text.as_str());
        let action = b.support.map_or_else(|| "unknown blocker".to_string(), |s| s.action());
        let note = format!(
            "{text}: locks {} layers ({} by it alone); {action}",
            b.layers.len(),
            b.only
        );
        report.add(
            format!("texture set \"{name}\" / blocker {}", b.key),
            Fidelity::KeptRaw,
            note,
        );
    }
}

/// Puts the correction layer ([`Correction`]) below the reference on top of `set.layers` and reports how much it
/// covers.
fn add_correction(
    set: &mut TextureSet,
    fills: &FillContext,
    islands: Option<&IslandTexels>,
    name: &str,
    report: &mut Reporter,
) {
    let Some(result) = set.layers.last().filter(|l| l.id == PAINTER_RESULT_ID) else {
        return;
    };
    let Some(correction) = Correction::build(set, result, fills, islands, LOCK_SOURCE) else {
        return;
    };
    let fidelity = if correction.is_empty() {
        Fidelity::Exact
    } else {
        Fidelity::Baked
    };
    report.add(
        format!("texture set \"{name}\" / {CORRECTION_NAME}"),
        fidelity,
        describe(&correction.coverage),
    );
    let at = set.layers.len() - 1;
    set.layers.insert(at, correction.layer);
}

/// True if a layer (or a folder's child) has a polygon fill, which needs the surface map to composite.
fn uses_picks(layers: &[Layer]) -> bool {
    let picks = |k: &str| k == "polygon_fill";
    layers.iter().any(|l| {
        l.effects.iter().any(|e| picks(e.kind.id()))
            || l.mask
                .as_ref()
                .is_some_and(|m| m.effects.iter().any(|e| picks(e.kind.id())))
            || uses_picks(l.children())
    })
}

/// The set's surface map at its resolution, without gutter.
fn surface_map(mesh: &Mesh, set: &TextureSet) -> Option<SurfaceMap> {
    let (width, height) = set.resolution;
    let key = SurfaceKey {
        material: set.material_slot,
        uv_set: set.uv_set,
        width,
        height,
        gutter: 0,
        mesh_revision: 0,
    };
    SurfaceMap::build(mesh, key).ok()
}

/// The flattened cache layer's name and id.
pub fn painter_result_layer(kind: LayerKind) -> Layer {
    Layer::new(PAINTER_RESULT_ID, PAINTER_RESULT_NAME, kind)
}

/// A uniform fill with Painter's default value for every declared channel.
pub fn defaults_layer(set: &TextureSet) -> Layer {
    let values = set
        .channels
        .iter()
        .map(|s| (s.channel, painter_default(s.channel)))
        .collect();
    let mut layer = Layer::new(
        DEFAULTS_LAYER_ID,
        DEFAULTS_LAYER_NAME,
        LayerKind::Fill(Fill::uniform(values)),
    );
    for spec in &set.channels {
        layer.set_blend(spec.channel, BlendMode::Normal, 1.0);
    }
    layer
}
