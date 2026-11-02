//! [`PaintTarget`]: what a brush tool paints into (ADR 0008 mc2-q-pt-02c): a paint layer's channel images, or a
//! layer's or folder's mask (its topmost paint effect, added first when the mask has none), and the images, values
//! and stroke-record owner that follow from it.

use crate::BrushError;
use materialcraft::paint::brush::Retouch;
use materialcraft::paint::edit::ops::{NewEffect, new_effect};
use materialcraft::paint::edit::{Edit, ImageRef, LayerRef, StackId};
use materialcraft::paint::effect::EffectKind;
use materialcraft::paint::stroke::{StrokeOwner, StrokeTool};
use materialcraft::paint::{Channel, ColorSpace, Layer, LayerImage, LayerKind, TextureSet};
use materialcraft::stamp::{ChannelTarget, ProjectedPaint, StampMode};

/// Which part of a layer is painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetPart {
    /// A paint layer's own channel images.
    Content,
    /// The layer's mask.
    Mask,
}

/// The values a stroke paints (PT-02, PT-03): per channel a value and whether the channel is painted at all, and
/// the grey value for masks.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintValues {
    /// Straight RGBA in each channel's stored encoding; channels not listed or switched off are left alone.
    pub channels: Vec<(Channel, [f32; 4], bool)>,
    /// Painted into masks, 0 black … 1 white.
    pub mask: f32,
}

impl PaintValues {
    /// Every channel of `set` on, at its default value; masks painted white.
    pub fn defaults_for(set: &TextureSet) -> PaintValues {
        PaintValues { channels: set.channels.iter().map(|s| (s.channel, s.channel.default_value(), true)).collect(), mask: 1.0 }
    }

    /// The value for `channel` if it is switched on.
    pub fn value(&self, channel: Channel) -> Option<[f32; 4]> {
        self.channels.iter().find(|(c, _, on)| *c == channel && *on).map(|(_, v, _)| *v)
    }
}

/// A layer part to paint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintTarget {
    pub layer: LayerRef,
    pub part: TargetPart,
}

/// One image of a resolved target.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetImage {
    pub image: ImageRef,
    pub value: [f32; 4],
    pub srgb: bool,
}

/// A target resolved against the document: the images a stroke writes, in a fixed order.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub images: Vec<TargetImage>,
    /// Whose stroke record the stroke is appended to.
    pub owner: StrokeOwner,
    /// Channels switched on that the layer has no image for (left unpainted).
    pub missing: Vec<Channel>,
}

impl PaintTarget {
    /// The edit that must happen before painting: a mask without a paint effect gets one on top. `None` when the
    /// target can be painted as it is.
    pub fn preparation(&self, set: &TextureSet) -> Result<Option<Edit>, BrushError> {
        let layer = self.find(set)?;
        match self.part {
            TargetPart::Content => Ok(None),
            TargetPart::Mask => {
                let mask = layer.mask.as_ref().ok_or(BrushError::NotPaintable("the layer has no mask"))?;
                if mask_paint_effect(layer).is_some() {
                    return Ok(None);
                }
                let effect = new_effect(set, NewEffect::Paint, StackId::Mask);
                Ok(Some(Edit::InsertEffect { layer: self.layer.clone(), index: mask.effects.len(), effect }))
            }
        }
    }

    /// The images to paint with `values` and the stroke-record owner. For [`TargetPart::Mask`] run the
    /// [`Self::preparation`] edit first.
    pub fn resolve(&self, set: &TextureSet, values: &PaintValues) -> Result<Resolved, BrushError> {
        let layer = self.find(set)?;
        match self.part {
            TargetPart::Content => {
                let LayerKind::Paint(p) = &layer.kind else { return Err(BrushError::NotPaintable("only paint layers have pixels to paint")) };
                let images = p
                    .images
                    .iter()
                    .filter_map(|(c, _)| {
                        let value = values.value(*c)?;
                        let srgb = set.channel_spec(*c).map_or(c.default_color_space(), |s| s.color_space) == ColorSpace::Srgb;
                        Some(TargetImage { image: ImageRef::new(self.layer.clone(), LayerImage::Paint(*c)), value, srgb })
                    })
                    .collect();
                let missing = values.channels.iter().filter(|(c, _, on)| *on && p.image(*c).is_none()).map(|(c, _, _)| *c).collect();
                Ok(Resolved { images, owner: StrokeOwner::Paint, missing })
            }
            TargetPart::Mask => {
                let effect = mask_paint_effect(layer).ok_or(BrushError::NotPaintable("the mask has no paint effect"))?;
                let v = values.mask.clamp(0.0, 1.0);
                let image = TargetImage { image: ImageRef::new(self.layer.clone(), LayerImage::Mask { effect }), value: [v, v, v, 1.0], srgb: false };
                Ok(Resolved { images: vec![image], owner: StrokeOwner::Mask { effect }, missing: Vec::new() })
            }
        }
    }

    /// The resolved images as stamp targets in `mode` ([`mode_for`]), borrowed from `set` (they must still exist:
    /// resolve again after the document changed). On masks the eraser paints black (PT-20).
    pub fn channel_targets<'a>(&self, set: &'a mut TextureSet, resolved: &Resolved, mode: StampMode) -> Result<Vec<ChannelTarget<'a>>, BrushError> {
        self.projected_targets(set, resolved, mode, None)
    }

    /// [`Self::channel_targets`] painting through `projected` (projection painting, PJ-26): each channel image
    /// takes its values from the projection's image for that channel, a mask from its grey image; images the
    /// projection has nothing for paint their resolved value.
    pub fn projected_targets<'a>(
        &self,
        set: &'a mut TextureSet,
        resolved: &Resolved,
        mode: StampMode,
        projected: Option<&'a ProjectedPaint<'_>>,
    ) -> Result<Vec<ChannelTarget<'a>>, BrushError> {
        let layer = set.layer_mut(&self.layer.layer).ok_or(BrushError::NotPaintable("the layer is gone"))?;
        let erase = mode == StampMode::Erase;
        let mode = if erase && self.part == TargetPart::Mask { StampMode::Paint } else { mode };
        let mut out = Vec::with_capacity(resolved.images.len());
        match (self.part, &mut layer.kind) {
            (TargetPart::Content, LayerKind::Paint(p)) => {
                for (c, img) in p.images.iter_mut() {
                    if let Some(t) = resolved.images.iter().find(|t| t.image.image == LayerImage::Paint(*c)) {
                        let source = projected.and_then(|p| p.channel(*c));
                        out.push(ChannelTarget { image: img, value: t.value, mode, srgb: t.srgb, source });
                    }
                }
            }
            (TargetPart::Mask, _) => {
                let (Some(t), StrokeOwner::Mask { effect }) = (resolved.images.first(), resolved.owner) else {
                    return Err(BrushError::NotPaintable("the mask has no paint effect"));
                };
                let effect = layer.mask.as_mut().and_then(|m| m.effects.get_mut(effect));
                let Some(EffectKind::Paint(px)) = effect.map(|e| &mut e.kind) else { return Err(BrushError::NotPaintable("the mask has no paint effect")) };
                let value = if erase { [0.0, 0.0, 0.0, 1.0] } else { t.value };
                out.push(ChannelTarget { image: &mut px.image, value, mode, srgb: false, source: projected.and_then(ProjectedPaint::mask) });
            }
            _ => return Err(BrushError::NotPaintable("only paint layers have pixels to paint")),
        }
        if out.len() != resolved.images.len() {
            return Err(BrushError::NotPaintable("the layer's images changed"));
        }
        Ok(out)
    }

    fn find<'a>(&self, set: &'a TextureSet) -> Result<&'a Layer, BrushError> {
        if set.id != self.layer.set {
            return Err(BrushError::NotPaintable("the layer belongs to another texture set"));
        }
        let layer = set.layer(&self.layer.layer).ok_or(BrushError::NotPaintable("the layer is gone"))?;
        if layer.lock.is_some() {
            return Err(BrushError::NotPaintable("the layer is locked: unlock it to paint on it"));
        }
        Ok(layer)
    }
}

/// How a stroke of `tool` lands in its images: the eraser erases, smudge and blur run local kernels, clone copies
/// from the recorded offset, everything else paints.
pub fn mode_for(tool: StrokeTool, retouch: &Retouch) -> StampMode {
    match tool {
        StrokeTool::Eraser => StampMode::Erase,
        StrokeTool::Smudge | StrokeTool::Blur => StampMode::Local,
        StrokeTool::Clone => StampMode::Clone { offset: retouch.clone_offset },
        StrokeTool::Paint | StrokeTool::Projection => StampMode::Paint,
    }
}

/// The topmost paint effect of the layer's mask.
fn mask_paint_effect(layer: &Layer) -> Option<usize> {
    layer.mask.as_ref()?.effects.iter().rposition(|e| matches!(e.kind, EffectKind::Paint(_)))
}
