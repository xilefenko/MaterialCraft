//! [`ContentEvaluator`]: what a layer's content (its effect stack, bottom → top) gives each declared channel:
//! a constant from uniform fills, an image from a bitmap fill, a stack the compositor evaluates from the mapped
//! effects (levels, fills stacked over images, references to anchors whose content is computable), or nothing
//! computable (procedurals, strokes, filters, …) with the reason. Anchors only mark a place and change nothing.
//! Brush work is computable when its strokes are replayed ([`super::replay`]): one replay per effect group (or
//! bare paint effect), the same one the effect mapping puts into the layer's pixels.

use super::anchors::AnchorTable;
use super::blend_table::{BlendTable, Holder};
use super::blending::ResolvedBlend;
use super::channels::SetChannels;
use super::computability::Computability;
use super::effect_data;
use super::images::{ImageSource, channel_pixels};
use super::projection_map::ProjectionTable;
use super::replay::StrokeReplay;
use crate::model::{EffectHeader, FillEffect, FilterEffect, PaintEffect, PainterEffect, PainterSource};
use crate::translate::blockers::{Blocker, BlockerKey, BlockerKind};
use crate::translate::replacements::{self, ProceduralPlan, Replacement, channel_textures, materials};
use materialcraft::paint::{BlendMode, Channel, TiledImage};
use materialcraft::projection::Projection;

/// The value one channel gets.
pub enum ChannelValue {
    /// A constant in the channel's stored encoding, with the content's coverage (0..1).
    Uniform { value: [f32; 4], coverage: f32 },
    /// A bitmap in the channel's layer format and encoding (alpha = coverage, any size), laid on by
    /// `projection`, and an extra coverage factor.
    Image {
        image: TiledImage,
        coverage: f32,
        projection: Projection,
    },
    /// Computed by the compositor from the layer's effects (no single value to show here).
    Composited,
    /// Nothing that can be shown.
    Nothing,
}

/// One channel the content writes.
pub struct ChannelContent {
    pub channel: Channel,
    pub value: ChannelValue,
    pub status: Computability,
}

/// Working state for one channel while walking the effects. Uniform values stay in Painter's space
/// (linear) until the end.
struct State {
    value: Acc,
    status: Computability,
    written: bool,
}

enum Acc {
    Empty,
    Uniform([f32; 4]),
    Image(TiledImage, f32, Projection),
    /// The compositor evaluates the effects (status says how exactly).
    Composited,
    Opaque,
}

pub struct ContentEvaluator<'a> {
    pub channels: &'a SetChannels,
    pub table: &'a BlendTable,
    pub projections: &'a ProjectionTable,
    pub images: &'a dyn ImageSource,
    pub resolution: (u32, u32),
    /// What the set's anchors hold, for fills that read one.
    pub anchors: &'a AnchorTable,
    /// Which Substance filters are replaced by MaterialCraft's, and which material fills by our materials.
    pub plan: &'a ProceduralPlan,
    /// Paints brush work again (`None`: strokes are not computable).
    pub replay: Option<&'a StrokeReplay<'a>>,
}

impl ContentEvaluator<'_> {
    /// Every declared channel the effects write, in declaration order.
    pub fn evaluate(&self, effects: &[PainterEffect]) -> Vec<ChannelContent> {
        let mut out = Vec::new();
        for channel in self.channels.all() {
            let mut state = State {
                value: Acc::Empty,
                status: Computability::Exact,
                written: false,
            };
            self.walk(effects, channel, &mut state);
            if state.written {
                out.push(self.finish(channel, state));
            }
        }
        out
    }

    fn finish(&self, channel: Channel, state: State) -> ChannelContent {
        let value = match state.value {
            Acc::Uniform(v) => ChannelValue::Uniform {
                value: self.channels.stored_value(channel, [v[0], v[1], v[2]]),
                coverage: v[3],
            },
            Acc::Image(image, coverage, projection) => ChannelValue::Image {
                image,
                coverage,
                projection,
            },
            Acc::Composited => ChannelValue::Composited,
            Acc::Empty | Acc::Opaque => ChannelValue::Nothing,
        };
        ChannelContent {
            channel,
            value,
            status: state.status,
        }
    }

    fn walk(&self, effects: &[PainterEffect], channel: Channel, state: &mut State) {
        let painter = self.channels.painter(channel);
        for effect in effects.iter().filter(|e| e.is_enabled()) {
            match effect {
                PainterEffect::Fill(fill) => self.fill(fill, channel, state),
                PainterEffect::Group(group) => {
                    let blend = ResolvedBlend::of(&group.header.blending, painter, Holder::EffectGroup, self.table);
                    if !blend.contributes() {
                        continue;
                    }
                    if self.replayed(&brush_work(&group.effects), channel, Some(&blend), state) {
                        // the group's other entries follow it as effects of their own
                        let rest: Vec<PainterEffect> = group
                            .effects
                            .iter()
                            .filter(|e| !matches!(e, PainterEffect::Paint(_)))
                            .cloned()
                            .collect();
                        self.walk(&rest, channel, state);
                        continue;
                    }
                    if blend.mode() == BlendMode::Passthrough {
                        self.walk(&group.effects, channel, state);
                        continue;
                    }
                    let mut inner = State {
                        value: Acc::Empty,
                        status: Computability::Exact,
                        written: false,
                    };
                    self.walk(&group.effects, channel, &mut inner);
                    if inner.written {
                        opaque(
                            state,
                            Blocker::feature("effect-group-blending", "effect group with its own blending"),
                        );
                    }
                }
                PainterEffect::Paint(p) if self.replayed(&[p], channel, None, state) => {}
                PainterEffect::Paint(p) if p.stroke_count() > 0 && self.writes(&p.header, channel) => {
                    let colours = p.colors.is_empty() || p.colors.iter().any(|c| c.channels().contains(painter));
                    if colours {
                        opaque(
                            state,
                            Blocker::new(
                                BlockerKey::new(BlockerKind::Strokes, p.tool.name()),
                                format!("{} strokes", p.tool.name()),
                            ),
                        );
                    }
                }
                PainterEffect::Filter(f) if self.writes(&f.header, channel) => {
                    let name = f.procedural.as_ref().map_or("?", |p| p.name());
                    match filter_replacement(f, self.plan) {
                        Some((r, _)) => {
                            let blend = ResolvedBlend::of(&f.header.blending, painter, Holder::Filter, self.table);
                            let mut status =
                                Computability::replaced(format!("filter \"{name}\" replaced by {}", r.generator));
                            status.and(&blend.status());
                            composited(state, status);
                        }
                        None => opaque(state, Blocker::substance(name, format!("filter \"{name}\""))),
                    }
                }
                PainterEffect::Levels(l) if self.writes(&l.header, channel) => {
                    let blend = ResolvedBlend::of(&l.header.blending, painter, Holder::Filter, self.table);
                    composited(state, blend.status());
                }
                PainterEffect::ColorSelection(c) if self.writes(&c.header, channel) => {
                    opaque(state, Blocker::feature("colour-selection-content", "colour selection"))
                }
                PainterEffect::PolygonFill(p) if self.writes(&p.header, channel) => {
                    opaque(state, Blocker::feature("polygon-fill-content", "polygon fill"))
                }
                PainterEffect::Other(raw) => opaque(
                    state,
                    Blocker::class(&raw.class, format!("unknown effect {}", raw.class)),
                ),
                _ => {}
            }
        }
    }

    /// Brush work replayed for `channel` (bare, or an effect group's with `blend`): true when the replay took care of
    /// the channel (composited with the replay's status, or not painted at all), false when there is no replay or
    /// no stroke in `paints`.
    fn replayed(
        &self,
        paints: &[&PaintEffect],
        channel: Channel,
        blend: Option<&ResolvedBlend>,
        state: &mut State,
    ) -> bool {
        let Some(replay) = self.replay else { return false };
        if !paints.iter().any(|p| p.stroke_count() > 0) {
            return false;
        }
        match replay.content(paints, self.channels) {
            Ok(r) => {
                if r.pixels.iter().any(|(c, _)| *c == channel) {
                    let mut status = r.status("paint layer");
                    if let Some(b) = blend {
                        status.and(&b.status());
                    }
                    composited(state, status);
                }
            }
            Err(why) => {
                if paints.iter().any(|p| self.writes(&p.header, channel)) {
                    opaque(state, why.through("brush strokes not replayable"));
                }
            }
        }
        true
    }

    /// True if the effect's `channelTypes` includes the channel and its blending does not switch it off.
    /// An empty mask outside a mask stack is read as "all channels" (conservative).
    fn writes(&self, header: &EffectHeader, channel: Channel) -> bool {
        let painter = self.channels.painter(channel);
        let listed = header.channels.is_empty() || self.channels.mask_has(header.channels, channel);
        listed && ResolvedBlend::of(&header.blending, painter, Holder::Layer, self.table).contributes()
    }

    fn fill(&self, fill: &FillEffect, channel: Channel, state: &mut State) {
        let painter = self.channels.painter(channel);
        if !fill.header.channels.is_empty() && !self.channels.mask_has(fill.header.channels, channel) {
            return;
        }
        let Some(source) = fill.sources.iter().find(|s| {
            let c = s.channels();
            c.contains(painter) || (c.is_all() && matches!(s, PainterSource::Procedural(_)))
        }) else {
            return;
        };
        let blend = ResolvedBlend::of(&fill.header.blending, painter, Holder::Layer, self.table);
        if !blend.contributes() {
            return;
        }
        match source {
            PainterSource::Uniform(u) => {
                let top = [u.color[0], u.color[1], u.color[2], 1.0];
                state.written = true;
                match state.value {
                    Acc::Empty => state.value = Acc::Uniform([top[0], top[1], top[2], blend.opacity]),
                    Acc::Uniform(below) => {
                        if !harmless(&blend, top) {
                            state.status.and(&blend.status());
                        }
                        if below[3] < 1.0 {
                            state
                                .status
                                .and(&Computability::approximated("uniform fills over partial coverage"));
                        }
                        state.value = Acc::Uniform(blend.mode().mix(below, top, blend.opacity));
                    }
                    Acc::Opaque => {}
                    Acc::Image(..) | Acc::Composited => {
                        let status = if harmless(&blend, top) {
                            Computability::Exact
                        } else {
                            blend.status()
                        };
                        composited(state, status);
                    }
                }
            }
            PainterSource::Bitmap(b) => match state.value {
                Acc::Empty => self.bitmap(fill, &b.url, channel, blend.opacity, state),
                Acc::Opaque => {}
                _ => {
                    // a bitmap over other content: the compositor stacks the fills
                    let mut alone = State {
                        value: Acc::Empty,
                        status: Computability::Exact,
                        written: false,
                    };
                    self.bitmap(fill, &b.url, channel, blend.opacity, &mut alone);
                    alone.status.and(&blend.status());
                    match alone.value {
                        Acc::Opaque => opaque(
                            state,
                            Blocker::feature("unreadable-bitmap", "bitmap fill that cannot be read"),
                        ),
                        _ => composited(state, alone.status),
                    }
                }
            },
            PainterSource::Procedural(p) => {
                let name = p.procedural.as_ref().map_or("?", |p| p.name());
                match materials::chosen(p, self.plan) {
                    // the user's pick from the tray: computable, a replacement (not Painter's look)
                    Some(reference) => {
                        let mut status =
                            Computability::replaced(format!("material \"{name}\" replaced by {reference}"));
                        status.and(&self.projections.map(fill).status);
                        if !matches!(state.value, Acc::Empty) {
                            status.and(&blend.status());
                        }
                        composited(state, status);
                    }
                    None => match channel_textures::chosen(p, self.plan) {
                        // one of our textures, laid on by the fill's projection: a look-alike, so projection
                        // guesses add reasons, not a worse state
                        Some(t) => {
                            let why = format!("procedural fill \"{name}\" replaced by {}", t.replacement.generator);
                            let mut status = Computability::replaced(why);
                            for reason in self.projections.map(fill).status.reasons() {
                                status.and(&Computability::replaced(format!("texture placement: {reason}")));
                            }
                            if !matches!(state.value, Acc::Empty) {
                                status.and(&blend.status());
                            }
                            composited(state, status);
                        }
                        None => opaque(state, Blocker::substance(name, format!("procedural fill \"{name}\""))),
                    },
                }
            }
            PainterSource::Reference(r) => {
                let status = self.anchors.status(r.anchor_uid);
                match status.is_computable() {
                    true => {
                        let mut s = status;
                        s.and(&blend.status());
                        composited(state, s);
                    }
                    false => {
                        for b in status.blockers() {
                            opaque(state, b.through("fill from an anchor"));
                        }
                    }
                }
            }
            PainterSource::Other(raw) => opaque(
                state,
                Blocker::class(&raw.class, format!("unknown source {}", raw.class)),
            ),
        }
    }

    fn bitmap(&self, fill: &FillEffect, url: &str, channel: Channel, coverage: f32, state: &mut State) {
        state.written = true;
        let Some(spec) = self.channels.spec(channel) else {
            return opaque(state, Blocker::feature("undeclared-channel", "undeclared channel"));
        };
        let loaded = match self.images.load(url) {
            Ok(l) => l,
            Err(e) => return opaque(state, Blocker::feature("unreadable-bitmap", e)),
        };
        let mapped = self.projections.map(fill);
        let image = channel_pixels(&loaded, spec);
        state.status.and(&mapped.status_for(image.size(), self.resolution));
        state.value = Acc::Image(image, coverage, mapped.projection);
    }
}

/// The replacement of a filter under `plan` (a filter entry of the replacement table), with our procedural.
pub(super) fn filter_replacement(
    f: &FilterEffect,
    plan: &ProceduralPlan,
) -> Option<(&'static Replacement, materialcraft::paint::effect::Procedural)> {
    let imported = effect_data::procedural(f.procedural.as_ref());
    replacements::replace(&imported, plan).filter(|(r, _)| replacements::is_filter(r))
}

/// A guessed normal-map mode with a flat normal on top cannot change anything (every such mode keeps the base).
pub(super) fn harmless(blend: &ResolvedBlend, top: [f32; 4]) -> bool {
    let flat = (top[0] - 0.5).abs() < 1e-3 && (top[1] - 0.5).abs() < 1e-3 && (top[2] - 1.0).abs() < 1e-3;
    blend.mode().is_normal_map() && flat
}

/// The paint effects directly in `effects` (an effect group's brush work).
pub(super) fn brush_work(effects: &[PainterEffect]) -> Vec<&PaintEffect> {
    effects
        .iter()
        .filter_map(|e| match e {
            PainterEffect::Paint(p) if p.header.enabled => Some(p),
            _ => None,
        })
        .collect()
}

/// The compositor takes over the channel from here, unless something before is already not computable.
fn composited(state: &mut State, status: Computability) {
    state.written = true;
    if !matches!(state.value, Acc::Opaque) {
        state.value = Acc::Composited;
    }
    state.status.and(&status);
}

fn opaque(state: &mut State, blocker: Blocker) {
    state.written = true;
    state.value = Acc::Opaque;
    state.status.and(&Computability::not_computable(blocker));
}
