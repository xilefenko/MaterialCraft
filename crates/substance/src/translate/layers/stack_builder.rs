//! [`StackBuilder`]: a Painter layer tree (bottom → top) → our layers with the same order, nesting, names,
//! visibility, per-channel blending, content effects and masks, plus the [`StackVerdict`] on how exact the
//! result is. Every Painter effect becomes one of ours ([`ContentMapper`], [`MaskEvaluator`]); the verdict
//! decides which channels they reproduce. Blend entries go on every declared channel for folders, and on the
//! channels their content writes for layers.

use super::anchors::AnchorTable;
use super::blend_table::{BlendTable, Holder};
use super::blending::ResolvedBlend;
use super::channels::SetChannels;
use super::computability::Computability;
use super::content::{ChannelContent, ContentEvaluator};
use super::decal::geometry::SetGeometry;
use super::effect_data::AnchorIds;
use super::effect_map::ContentMapper;
use super::images::ImageSource;
use super::layer_content::is_flat_normal;
use super::mask::MaskEvaluator;
use super::projection_map::ProjectionTable;
use super::replay::{ReplayTally, StrokeReplay};
use super::set_surface::SurfaceSource;
use super::verdict::{LayerNote, MaskFailure, StackVerdict};
use crate::model::{LayerKind as PainterKind, PainterLayer};
use crate::translate::replacements::ProceduralPlan;
use materialcraft::paint::{Channel, Layer, LayerKind, LayerLock};

/// What locked an imported layer ([`LayerLock::source`]).
pub const LOCK_SOURCE: &str = "Substance 3D Painter import";
use std::collections::BTreeSet;

/// What a folder passes down to its children: visibility, its path, and per channel the status of its own
/// blending and mask (`None` = the folder switches the channel off).
#[derive(Clone)]
struct Context {
    visible: bool,
    path: String,
    channels: Vec<(Channel, Option<Computability>)>,
}

impl Context {
    fn inherited(&self, channel: Channel) -> Option<&Computability> {
        self.channels
            .iter()
            .find(|(c, _)| *c == channel)
            .and_then(|(_, s)| s.as_ref())
    }
}

/// The imported layers of one set and the verdict on them.
pub struct ImportedStack {
    /// Bottom → top.
    pub layers: Vec<Layer>,
    pub verdict: StackVerdict,
    /// What the stroke replay did (empty without one).
    pub replayed: ReplayTally,
}

pub struct StackBuilder<'a> {
    pub channels: &'a SetChannels,
    pub table: &'a BlendTable,
    pub projections: &'a ProjectionTable,
    pub images: &'a dyn ImageSource,
    pub plan: &'a ProceduralPlan,
    pub resolution: (u32, u32),
    /// The set's mesh, for polygon-fill picks.
    pub geometry: Option<&'a SetGeometry<'a>>,
    /// The set's surface, for mask bitmaps projected from a volume.
    pub surface: Option<&'a dyn SurfaceSource>,
    /// Paints painted masks again (`None`: they stay empty with their stroke records).
    pub replay: Option<&'a StrokeReplay<'a>>,
    /// What the set's anchors hold (see [`AnchorTable::measure`]).
    pub anchors: &'a AnchorTable,
}

impl StackBuilder<'_> {
    /// The anchors of `layers`, measured with this builder's evaluators.
    pub fn anchor_table(&self, layers: &[PainterLayer]) -> AnchorTable {
        AnchorTable::measure(layers, &self.evaluator(), &self.masks())
    }

    fn masks(&self) -> MaskEvaluator<'_> {
        MaskEvaluator {
            table: self.table,
            projections: self.projections,
            images: self.images,
            plan: self.plan,
            resolution: self.resolution,
            geometry: self.geometry,
            surface: self.surface,
            replay: self.replay,
            anchors: self.anchors,
        }
    }

    pub fn build(&self, layers: &[PainterLayer]) -> ImportedStack {
        let mut verdict = StackVerdict::new(self.channels.all());
        let top = Context {
            visible: true,
            path: String::new(),
            channels: self.channels.all().map(|c| (c, Some(Computability::Exact))).collect(),
        };
        let mut ids = Ids::default();
        let layers = self.stack(layers, &top, &mut verdict, &mut ids);
        let replayed = self.replay.map(StrokeReplay::tally).unwrap_or_default();
        ImportedStack {
            layers,
            verdict,
            replayed,
        }
    }

    fn stack(&self, layers: &[PainterLayer], ctx: &Context, v: &mut StackVerdict, ids: &mut Ids) -> Vec<Layer> {
        layers.iter().map(|l| self.layer(l, ctx, v, ids)).collect()
    }

    fn layer(&self, l: &PainterLayer, ctx: &Context, v: &mut StackVerdict, ids: &mut Ids) -> Layer {
        v.layer_count += 1;
        let path = match ctx.path.is_empty() {
            true => l.name.clone(),
            false => format!("{} / {}", ctx.path, l.name),
        };
        let masks = self.masks();
        let mask = masks.evaluate(l.mask.as_ref(), &mut ids.anchors);
        let visible = ctx.visible && l.visible;
        let mut note = mask.status.clone();
        let evaluator = self.evaluator();
        let mapper = ContentMapper {
            evaluator: &evaluator,
            geometry: self.geometry,
        };
        let (kind, effects, written) = match &l.kind {
            PainterKind::Group(children) => {
                let mut inner = Context {
                    visible,
                    path: path.clone(),
                    channels: Vec::new(),
                };
                for channel in self.channels.all() {
                    let blend = self.blend(l, channel);
                    let mut status = ctx.inherited(channel).cloned();
                    if let Some(s) = status.as_mut() {
                        s.and(&mask.status);
                        s.and(&self.blend_status(&blend, channel, visible, v));
                    }
                    inner.channels.push((channel, status.filter(|_| blend.contributes())));
                }
                self.folder_content(l, &inner, v, &mut note);
                let effects = mapper.folder_effects(l, &mut ids.anchors);
                let all: Vec<Channel> = self.channels.all().collect();
                (LayerKind::Group(self.stack(children, &inner, v, ids)), effects, all)
            }
            painter => {
                let content = self.content(l);
                let mut unknown = Computability::Exact;
                if let PainterKind::Other(class) = painter {
                    unknown = Computability::not_computable(format!("unknown layer class {class}"));
                }
                for c in &content {
                    note.and(&c.status);
                    let blend = self.blend(l, c.channel);
                    note.and(&blend.status());
                    let Some(inherited) = ctx.inherited(c.channel).filter(|_| visible && blend.contributes()) else {
                        continue;
                    };
                    let mut status = c.status.clone();
                    status.and(inherited);
                    status.and(&mask.status);
                    status.and(&unknown);
                    status.and(&self.blend_status(&blend, c.channel, true, v));
                    v.contribute(c.channel, &status);
                    if !is_flat_normal(&c.value) {
                        v.not_flat(c.channel);
                    }
                }
                note.and(&unknown);
                let written = content.iter().map(|c| c.channel).collect();
                let mapped = mapper.map(l, &mut ids.anchors);
                (mapped.base, mapped.effects, written)
            }
        };
        // Not computable: what we hold of it is not what Painter shows, and editing it would build on a wrong start.
        let lock = (!note.is_computable()).then(|| LayerLock::new(LOCK_SOURCE, note.reasons().to_vec()));
        if !note.is_exact() {
            v.note(LayerNote {
                path: path.clone(),
                visible,
                status: note,
            });
        }
        let mut layer = Layer::new(unique_id(l.uid, &mut ids.layers), l.name.clone(), kind);
        layer.effects = effects;
        layer.visible = l.visible;
        layer.lock = lock;
        if l.visible && !mask.status.is_computable() {
            v.mask_failures.push(MaskFailure {
                id: layer.id.clone(),
                path: path.clone(),
                reasons: mask.status.reasons().to_vec(),
            });
        }
        layer.mask = mask.mask;
        for channel in written {
            let blend = self.blend(l, channel);
            layer.set_blend(channel, blend.mode(), blend.opacity);
        }
        layer
    }

    fn evaluator(&self) -> ContentEvaluator<'_> {
        ContentEvaluator {
            channels: self.channels,
            table: self.table,
            projections: self.projections,
            images: self.images,
            resolution: self.resolution,
            anchors: self.anchors,
            plan: self.plan,
            replay: self.replay,
        }
    }

    fn content(&self, l: &PainterLayer) -> Vec<ChannelContent> {
        self.evaluator().evaluate(&l.content)
    }

    /// Effects on a folder itself (rare) are not computable; they count against every channel they write.
    fn folder_content(&self, l: &PainterLayer, inner: &Context, v: &mut StackVerdict, note: &mut Computability) {
        for c in self.content(l) {
            let status = Computability::not_computable("effects on a folder");
            note.and(&status);
            if inner.visible && inner.inherited(c.channel).is_some() {
                v.contribute(c.channel, &status);
            }
        }
    }

    fn blend(&self, l: &PainterLayer, channel: Channel) -> ResolvedBlend {
        ResolvedBlend::of(&l.blending, self.channels.painter(channel), Holder::Layer, self.table)
    }

    /// The blend's status for the channel verdict; guessed normal-map modes are kept apart (they only matter
    /// once a normal value is not flat). Every value used is recorded for fits.
    fn blend_status(
        &self,
        blend: &ResolvedBlend,
        channel: Channel,
        visible: bool,
        v: &mut StackVerdict,
    ) -> Computability {
        let status = blend.status();
        if visible && blend.contributes() {
            v.blend_value(channel, blend.painter.raw());
        }
        if visible && blend.contributes() && !status.is_exact() && blend.mode().is_normal_map() {
            v.normal_map_guess(channel, &status);
            return Computability::Exact;
        }
        status
    }
}

/// Ids handed out so far in one set: layers and anchors.
#[derive(Default)]
struct Ids {
    layers: BTreeSet<String>,
    anchors: AnchorIds,
}

/// `layer-<uid>`, made unique; `layer-n<k>` without a uid.
fn unique_id(uid: Option<u64>, ids: &mut BTreeSet<String>) -> String {
    let base = uid.map_or_else(|| format!("layer-n{}", ids.len()), |u| format!("layer-{u}"));
    let id = (1..)
        .map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") })
        .find(|id| !ids.contains(id))
        .unwrap_or(base);
    ids.insert(id.clone());
    id
}
