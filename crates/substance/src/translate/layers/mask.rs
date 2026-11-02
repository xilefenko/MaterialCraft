//! [`MaskEvaluator`]: a Painter mask (`maskActions` with `maskEnabled` and `maskInitial`) as our [`Mask`],
//! effect for effect, and how exactly it reproduces Painter.
//!
//! Every Painter mask effect becomes one of ours: an effect group becomes a paint effect (its brush work)
//! followed by its other entries, uniform fills grey fills, bitmap fills painted images, procedural fills
//! generators, reference fills references; filters, levels, colour selection, polygon fill and anchors keep
//! their kind; a polygon fill becomes one polygon fill per run of picks with the same value ([`super::decal`]).
//! Painted masks are computable when their strokes are replayed ([`super::replay`], status "replaced").
//! Computable are uniform and bitmap fills, empty paint, levels, colour selection (on the set's baked ID map),
//! reference fills whose anchor is computable, polygon fills resolved on the mesh, plus the procedural
//! fills the plan replaces with MaterialCraft's generators ([`crate::translate::replacements`], status
//! "replaced"); the first effect that is not gives the reason (generators, filters, strokes, levels, references …).

use super::anchors::AnchorTable;
use super::blend_table::BlendTable;
use super::blending::ResolvedBlend;
use super::computability::Computability;
use super::decal::{self, geometry::SetGeometry};
use super::effect_data::{self as data, AnchorIds};
use super::images::{ImageSource, grey_values, resample};
use super::projection_map::ProjectionTable;
use super::replay::StrokeReplay;
use super::set_surface::{SurfaceSource, project_image};
use super::strokes;
use crate::blockers::{Blocker, BlockerKey, BlockerKind};
use crate::model::{EffectHeader, MaskInitial, PainterEffect, PainterMask, PainterSource};
use crate::translate::replacements::{self, ProceduralPlan};
use materialcraft::json::Value;
use materialcraft::paint::effect::{Grey, PolygonFill, Selection};
use materialcraft::paint::stroke::StrokeData;
use materialcraft::paint::{
    BlendMode, EffectKind, GreyBlend, Mask, MaskEffect, MaskPixels, PixelFormat, PixelLayout, SampleType, TiledImage,
};

/// Stored format of imported bitmap masks (16 bits: constant masks stay exact to well below 8-bit rounding).
const MASK_FORMAT: PixelFormat = PixelFormat::new(PixelLayout::Gray, SampleType::U16);
/// Format of mask paint effects whose strokes are not replayed (nothing painted: alpha 0 everywhere).
const PAINT_FORMAT: PixelFormat = PixelFormat::new(PixelLayout::GrayAlpha, SampleType::U8);

/// The mask for a layer (`None` = no mask) and how exact it is.
pub struct MaskOutcome {
    pub mask: Option<Mask>,
    pub status: Computability,
}

pub struct MaskEvaluator<'a> {
    pub table: &'a BlendTable,
    pub projections: &'a ProjectionTable,
    pub images: &'a dyn ImageSource,
    /// Which Substance procedurals become MaterialCraft generators.
    pub plan: &'a ProceduralPlan,
    pub resolution: (u32, u32),
    /// The set's mesh, for polygon-fill picks.
    pub geometry: Option<&'a SetGeometry<'a>>,
    /// The set's surface at `resolution`, which bitmaps projected from a volume are laid on over.
    pub surface: Option<&'a dyn SurfaceSource>,
    /// Paints painted masks again; `None` keeps them empty with their stroke records.
    pub replay: Option<&'a StrokeReplay<'a>>,
    /// What the set's anchors hold, for fills that read one.
    pub anchors: &'a AnchorTable,
}

impl MaskEvaluator<'_> {
    pub fn evaluate(&self, mask: Option<&PainterMask>, anchors: &mut AnchorIds) -> MaskOutcome {
        let Some(painter) = mask else {
            return MaskOutcome {
                mask: None,
                status: Computability::Exact,
            };
        };
        let initial = match painter.initial {
            MaskInitial::Black => 0.0,
            MaskInitial::White => 1.0,
            // An initial value we cannot read: the layer gets no mask (a switched-off one does not matter).
            MaskInitial::Other(n) => {
                let status = match painter.enabled {
                    true => Computability::not_computable(Blocker::feature(
                        "mask-initial-value",
                        format!("mask initial value {n}"),
                    )),
                    false => Computability::Exact,
                };
                return MaskOutcome { mask: None, status };
            }
        };
        let mut walk = Walk {
            evaluator: self,
            anchors,
            effects: Vec::new(),
            status: Computability::Exact,
            counts: painter.enabled,
        };
        walk.effects(&painter.effects, true);
        let mask = Mask {
            enabled: painter.enabled,
            initial,
            effects: walk.effects,
            asset: None,
        };
        MaskOutcome {
            mask: Some(mask),
            status: walk.status,
        }
    }
}

/// One pass over a mask's effects, collecting ours and the status.
struct Walk<'e, 'a> {
    evaluator: &'e MaskEvaluator<'a>,
    anchors: &'e mut AnchorIds,
    effects: Vec<MaskEffect>,
    status: Computability,
    /// False for a switched-off mask: its effects are kept but do not count.
    counts: bool,
}

impl Walk<'_, '_> {
    fn effects(&mut self, effects: &[PainterEffect], enabled: bool) {
        for effect in effects {
            self.effect(effect, enabled && effect.is_enabled());
        }
    }

    /// Adds the status of a part that counts; nothing is added once the mask is not computable (the first
    /// reason is the one reported).
    fn note(&mut self, counts: bool, status: Computability) {
        if counts && self.counts && self.status.is_computable() {
            self.status.and(&status);
        }
    }

    fn stop(&mut self, counts: bool, why: Blocker) {
        self.note(counts, Computability::not_computable(why));
    }

    fn push(&mut self, header: Option<&EffectHeader>, blend: GreyBlend, kind: EffectKind<Grey>, enabled: bool) {
        let label = header.map(|h| h.label.clone()).unwrap_or_default();
        self.effects.push(MaskEffect {
            enabled,
            label,
            blend,
            kind,
        });
    }

    fn blend(&self, header: &EffectHeader) -> ResolvedBlend {
        ResolvedBlend::of_mask(&header.blending, self.evaluator.table)
    }

    fn effect(&mut self, effect: &PainterEffect, on: bool) {
        let header = effect.header();
        let grey = |b: ResolvedBlend| GreyBlend {
            mode: b.mode(),
            opacity: b.opacity,
        };
        let blend = header.map_or(GreyBlend::default(), |h| grey(self.blend(h)));
        match effect {
            PainterEffect::Group(g) => {
                self.push(header, blend, self.empty_paint(None), on);
                self.effects(&g.effects, on);
            }
            PainterEffect::Paint(p) => {
                let record = strokes::record([p]);
                if p.stroke_count() == 0 {
                    self.push(header, blend, self.empty_paint(record), on);
                    return;
                }
                match self.evaluator.replay.map(|r| r.mask(p)) {
                    Some(Ok(replayed)) => {
                        self.note(on, replayed.status("painted mask"));
                        let pixels = MaskPixels {
                            image: replayed.pixels,
                            strokes: record,
                        };
                        self.push(header, blend, EffectKind::Paint(pixels), on);
                    }
                    Some(Err(why)) => {
                        self.push(header, blend, self.empty_paint(record), on);
                        self.stop(
                            on,
                            Blocker::new(
                                why.key.clone(),
                                format!("painted mask (strokes not replayable: {})", why.text),
                            ),
                        );
                    }
                    None => {
                        self.push(header, blend, self.empty_paint(record), on);
                        self.stop(
                            on,
                            Blocker::new(
                                BlockerKey::new(BlockerKind::Strokes, "painted-mask"),
                                "painted mask (strokes)",
                            ),
                        );
                    }
                }
            }
            PainterEffect::Anchor(h) => {
                let anchor = self.anchors.anchor(h);
                self.push(header, blend, EffectKind::Anchor(anchor), on);
            }
            PainterEffect::Fill(fill) => {
                let resolved = self.blend(&fill.header);
                let counts = on && resolved.contributes();
                let Some(source) = fill.sources.first() else {
                    return; // a fill without a source does nothing
                };
                let kind = match source {
                    PainterSource::Uniform(u) => {
                        self.note(counts, resolved.status());
                        EffectKind::Fill(u.color[0])
                    }
                    PainterSource::Bitmap(b) => match self.evaluator.images.load(&b.url) {
                        Ok(loaded) => {
                            // Mask fills hold one grey value, so the bitmap is laid on here, as the compositor
                            // would sample it (over the set's surface in a volume mode; without it, in texture
                            // space).
                            let mapped = self.evaluator.projections.map(fill);
                            let size = self.evaluator.resolution;
                            let surface = mapped
                                .projection
                                .mode
                                .needs_mesh()
                                .then(|| self.evaluator.surface?.surface())
                                .flatten()
                                .filter(|(m, _)| (m.width() as u32, m.height() as u32) == size);
                            let image = match surface {
                                Some(s) => project_image(&loaded.image, &mapped.projection, s),
                                None => resample(&loaded.image, &mapped.projection, size),
                            };
                            let image = image.unwrap_or_else(|| loaded.image.clone());
                            self.note(counts, mapped.status_for(loaded.image.size(), size));
                            self.note(counts, resolved.status());
                            EffectKind::Paint(MaskPixels::new(grey_image(&image, size)))
                        }
                        Err(e) => {
                            self.stop(counts, Blocker::feature("unreadable-bitmap", format!("mask {e}")));
                            let url = Value::object().with("url", b.url.as_str());
                            EffectKind::Unknown(data::unknown_named("substance:bitmap_fill", url))
                        }
                    },
                    PainterSource::Procedural(p) => {
                        let name = p.procedural.as_ref().map_or("?", |p| p.name());
                        let mapped = self.evaluator.projections.map(fill);
                        let mut imported = data::procedural(p.procedural.as_ref());
                        imported.params =
                            data::with_placement(std::mem::take(&mut imported.params), &mapped.projection);
                        match replacements::replace(&imported, self.evaluator.plan) {
                            Some((r, ours)) => {
                                let why = format!("mask generator \"{name}\" replaced by {}", r.generator);
                                self.note(counts, Computability::replaced(why));
                                if r.is_texture() {
                                    // a look-alike is approximate anyway: projection guesses add reasons, not a worse state
                                    for reason in mapped.status.reasons() {
                                        self.note(
                                            counts,
                                            Computability::replaced(format!("texture placement: {reason}")),
                                        );
                                    }
                                }
                                self.note(counts, resolved.status());
                                EffectKind::Generator(ours)
                            }
                            None => {
                                self.stop(counts, Blocker::substance(name, format!("mask generator \"{name}\"")));
                                EffectKind::Generator(imported)
                            }
                        }
                    }
                    PainterSource::Reference(r) => {
                        let status = self.evaluator.anchors.status(r.anchor_uid);
                        match status.is_computable() {
                            true => {
                                self.note(counts, status);
                                self.note(counts, resolved.status());
                            }
                            false => {
                                for b in status.blockers() {
                                    self.stop(counts, b.through("mask reads an anchor"));
                                }
                            }
                        }
                        EffectKind::Reference(data::reference(r))
                    }
                    PainterSource::Other(raw) => {
                        self.stop(counts, Blocker::class(&raw.class, format!("mask source {}", raw.class)));
                        EffectKind::Unknown(data::unknown(raw))
                    }
                };
                self.push(header, blend, kind, on);
            }
            PainterEffect::Filter(f) => {
                let name = f.procedural.as_ref().map_or("?", |p| p.name());
                let imported = data::procedural(f.procedural.as_ref());
                let kind = match replacements::replace(&imported, self.evaluator.plan)
                    .filter(|(r, _)| replacements::is_filter(r))
                {
                    Some((r, ours)) => {
                        self.note(
                            on,
                            Computability::replaced(format!("mask filter \"{name}\" replaced by {}", r.generator)),
                        );
                        if let Some(h) = header {
                            let status = self.blend(h).status();
                            self.note(on, status);
                        }
                        EffectKind::Filter(ours)
                    }
                    None => {
                        self.stop(on, Blocker::substance(name, format!("mask filter \"{name}\"")));
                        EffectKind::Filter(imported)
                    }
                };
                self.push(header, blend, kind, on);
            }
            PainterEffect::Levels(l) => {
                if let Some(h) = header {
                    self.note(on, self.blend(h).status());
                }
                self.push(header, blend, EffectKind::Levels(data::levels(l.levels.as_ref())), on);
            }
            PainterEffect::ColorSelection(c) => {
                if let Some(h) = header {
                    self.note(on, self.blend(h).status());
                }
                self.push(header, blend, EffectKind::ColorSelection(data::color_selection(c)), on);
            }
            PainterEffect::PolygonFill(p) => {
                let decal = decal::runs(self.evaluator.geometry, p);
                let counts = on && blend.opacity > 0.0 && blend.mode != BlendMode::Disable;
                self.note(counts, decal.status.clone());
                if let Some(h) = header {
                    self.note(counts, self.blend(h).status());
                }
                let normal = blend.mode == BlendMode::Normal && blend.opacity >= 1.0;
                if decal.runs.len() > 1 && !normal {
                    let why = "polygon fill: picks with several values blended one by one";
                    self.note(counts, Computability::approximated(why));
                }
                if decal.runs.is_empty() {
                    let selection = Selection::default();
                    self.push(
                        header,
                        blend,
                        EffectKind::PolygonFill(PolygonFill { selection, values: 1.0 }),
                        on,
                    );
                }
                for run in decal.runs {
                    let values = run.color[0];
                    let kind = EffectKind::PolygonFill(PolygonFill {
                        selection: run.selection,
                        values,
                    });
                    self.push(header, blend, kind, on);
                }
            }
            PainterEffect::Other(raw) => {
                self.stop(on, Blocker::class(&raw.class, format!("mask effect {}", raw.class)));
                self.push(None, blend, EffectKind::Unknown(data::unknown(raw)), on);
            }
        }
    }

    /// A paint effect without pixels (strokes not replayed), with the stroke record of the brush work.
    fn empty_paint(&self, strokes: Option<StrokeData>) -> EffectKind<Grey> {
        let (w, h) = self.evaluator.resolution;
        match TiledImage::new(w, h, PAINT_FORMAT) {
            Ok(image) => EffectKind::Paint(MaskPixels { image, strokes }),
            Err(_) => EffectKind::Unknown(data::unknown_named("substance:paint", Value::object())),
        }
    }
}

/// The grey values (red × alpha) of `image` as a 16-bit grey image of `size`.
fn grey_image(image: &TiledImage, size: (u32, u32)) -> TiledImage {
    let values = grey_values(image);
    let (w, h) = size;
    let Ok(mut out) = TiledImage::new(w, h, MASK_FORMAT) else {
        return image.clone();
    };
    let all = out.all_tiles();
    out.update_tiles(&all, |r, px| {
        for (i, p) in px.iter_mut().enumerate() {
            let (x, y) = (r.x as usize + i % r.w as usize, r.y as usize + i / r.w as usize);
            let v = values.get(y * w as usize + x).copied().unwrap_or(0.0);
            *p = [v, v, v, 1.0];
        }
    });
    out
}
