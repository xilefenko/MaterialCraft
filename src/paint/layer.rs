//! Layers of a texture set's stack: paint, fill and group layers, their per-channel blending, their content
//! effects and masks.
//!
//! A layer's content is its base (the [`LayerKind`]: painted pixels, a fill, or for a group its children)
//! followed by [`Layer::effects`], an ordered stack applied on top of the base inside the layer (see
//! [`super::effect`]). The result is blended over the stack below by the layer's own entries, scaled by its
//! [`Mask`].

use super::blend::BlendMode;
use super::channel::Channel;
use super::effect::{ContentEffect, EffectKind, Procedural};
use super::fill_image::ImageFill;
use super::fill_texture::TextureFill;
use super::geometry_mask::GeometryMask;
use super::image::TiledImage;
use super::instance::InstanceLink;
use super::lock::LayerLock;
use super::mask::Mask;
use super::stroke::StrokeData;
use crate::projection::Projection;

/// How a layer (or a content effect) affects one channel. Without an entry for a channel it does not affect it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelBlend {
    pub channel: Channel,
    pub mode: BlendMode,
    /// 0..1, multiplied with the layer opacity.
    pub opacity: f32,
}

/// Painted pixels, one image per channel. Images are straight (not premultiplied); alpha is coverage.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PaintContent {
    pub images: Vec<(Channel, TiledImage)>,
    /// The strokes the pixels were painted with, if recorded (see [`super::stroke`]).
    pub strokes: Option<StrokeData>,
    /// See-through content (ADR 0008 mc2-q-pt-03c, PT-25, PT-29): the strokes work on the composite of the layers
    /// below, and the images are what the recorded strokes made of it; they are painted again from the record
    /// whenever the layers below change (see [`super::see_through`]). Only the tiles the strokes touched hold pixels.
    pub see_through: bool,
}

impl PaintContent {
    /// Pixels without a stroke record.
    pub fn from_images(images: Vec<(Channel, TiledImage)>) -> PaintContent {
        PaintContent { images, strokes: None, see_through: false }
    }

    pub fn image(&self, channel: Channel) -> Option<&TiledImage> {
        self.images.iter().find(|(c, _)| *c == channel).map(|(_, i)| i)
    }

    pub fn image_mut(&mut self, channel: Channel) -> Option<&mut TiledImage> {
        self.images.iter_mut().find(|(c, _)| *c == channel).map(|(_, i)| i)
    }

    /// Adds or replaces the image of `channel`.
    pub fn set_image(&mut self, channel: Channel, image: TiledImage) {
        match self.image_mut(channel) {
            Some(slot) => *slot = image,
            None => self.images.push((channel, image)),
        }
    }
}

/// What a fill layer fills with.
#[derive(Debug, Clone, PartialEq)]
pub enum FillSource {
    /// A constant value per channel (normalised RGBA; alpha is ignored, coverage is 1).
    Uniform(Vec<(Channel, [f32; 4])>),
    /// A procedural material, evaluated outside the core document (path inside the document, e.g.
    /// `materials/rust.mca`, plus parameter overrides as strings).
    Material { path: String, params: Vec<(String, String)> },
    /// Images per channel (any size, sampled through the fill's projection) and constants for the other channels.
    Image(ImageFill),
    /// Procedural textures per channel (generators of the effect registry evaluated at points, sampled through
    /// the fill's projection) and constants for the other channels. Since format 0.16.
    Texture(TextureFill),
}

/// A fill: its source and how the source is laid onto the texture set. Uniform values ignore the projection.
#[derive(Debug, Clone, PartialEq)]
pub struct Fill {
    pub source: FillSource,
    pub projection: Projection,
    /// The procedural this fill stands in for (an imported Substance material or texture replaced by one of
    /// ours), kept unchanged so the choice can be made again later.
    pub original: Option<Box<Procedural>>,
}

impl Fill {
    /// A fill from `source` in texture space.
    pub fn new(source: FillSource) -> Fill {
        Fill { source, projection: Projection::default(), original: None }
    }

    /// Builder: the fill standing in for `original`.
    pub fn replacing(mut self, original: Procedural) -> Fill {
        self.original = Some(Box::new(original));
        self
    }

    /// A uniform fill.
    pub fn uniform(values: Vec<(Channel, [f32; 4])>) -> Fill {
        Fill::new(FillSource::Uniform(values))
    }

    /// Builder: the fill with `projection`.
    pub fn with_projection(mut self, projection: Projection) -> Fill {
        self.projection = projection;
        self
    }

    /// The channels the fill sets (none for a material).
    pub fn channels(&self) -> Vec<Channel> {
        match &self.source {
            FillSource::Uniform(v) => v.iter().map(|(c, _)| *c).collect(),
            FillSource::Material { .. } => Vec::new(),
            FillSource::Image(i) => i.channels(),
            FillSource::Texture(t) => t.channels(),
        }
    }

    /// The uniform value for `channel`, if this is a uniform fill that sets it.
    pub fn uniform_value(&self, channel: Channel) -> Option<[f32; 4]> {
        match &self.source {
            FillSource::Uniform(v) => v.iter().find(|(c, _)| *c == channel).map(|(_, v)| *v),
            FillSource::Material { .. } | FillSource::Image(_) | FillSource::Texture(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LayerKind {
    Paint(PaintContent),
    Fill(Fill),
    /// Child layers, bottom to top. The group's blend mode `Passthrough` blends children straight into
    /// the stack; any other mode composites them in isolation first.
    Group(Vec<Layer>),
    /// An instance of another layer, in this texture set or another (IN-01): composited as its source with this
    /// layer's id, name and visibility, without the source's painted pixels, on this set's own baked maps (see
    /// [`super::instance`]). The layer's own blend entries, mask and effects are not used.
    Instance(InstanceLink),
}

/// One layer of a stack.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// Unique within its texture set.
    pub id: String,
    pub name: String,
    pub visible: bool,
    /// 0..1 for all channels.
    pub opacity: f32,
    pub blend: Vec<ChannelBlend>,
    pub mask: Option<Mask>,
    /// Limits the layer to parts of the geometry (data only for now).
    pub geometry_mask: Option<GeometryMask>,
    /// The base content.
    pub kind: LayerKind,
    /// Content effects on top of the base, bottom → top.
    pub effects: Vec<ContentEffect>,
    /// The asset this layer was made from: a smart-material folder records the fragment it came from (a library
    /// reference such as `smart-materials/worn-paint.mcfrag`, also the key of the document's embedded copy, see
    /// [`crate::paint::embedded`]). `None` for layers made by hand.
    pub asset: Option<String>,
    /// Set when the layer's content must not be edited (see [`super::lock`]); `None` for ordinary layers.
    pub lock: Option<LayerLock>,
}

impl Layer {
    /// A copy of the layer without its children (a folder's are left out; other layers are copied whole). Images
    /// share their tiles with the original.
    pub fn without_children(&self) -> Layer {
        // destructured in full, so a new field is not forgotten here
        let Layer { id, name, visible, opacity, blend, mask, geometry_mask, kind, effects, asset, lock } = self;
        let kind = match kind {
            LayerKind::Group(_) => LayerKind::Group(Vec::new()),
            other => other.clone(),
        };
        Layer {
            id: id.clone(),
            name: name.clone(),
            visible: *visible,
            opacity: *opacity,
            blend: blend.clone(),
            mask: mask.clone(),
            geometry_mask: geometry_mask.clone(),
            kind,
            effects: effects.clone(),
            asset: asset.clone(),
            lock: lock.clone(),
        }
    }

    /// True if the layers are equal apart from their children (two folders with different children are; a folder
    /// and another kind of layer are not).
    pub fn same_without_children(&self, other: &Layer) -> bool {
        let Layer { id, name, visible, opacity, blend, mask, geometry_mask, kind, effects, asset, lock } = self;
        let kinds = match (kind, &other.kind) {
            (LayerKind::Group(_), LayerKind::Group(_)) => true,
            (a, b) => a == b,
        };
        kinds
            && *id == other.id
            && *name == other.name
            && *visible == other.visible
            && opacity.to_bits() == other.opacity.to_bits()
            && *blend == other.blend
            && *mask == other.mask
            && *geometry_mask == other.geometry_mask
            && *effects == other.effects
            && *asset == other.asset
            && *lock == other.lock
    }
}

/// Which image of a layer an image is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LayerImage {
    /// The paint layer's image of a channel.
    Paint(Channel),
    /// The image of a channel in content effect `effect`.
    Effect { effect: usize, channel: Channel },
    /// The image of paint effect `effect` of the mask.
    Mask { effect: usize },
}

impl Layer {
    /// A visible, fully opaque layer with no channel entries, effects or masks yet.
    pub fn new(id: impl Into<String>, name: impl Into<String>, kind: LayerKind) -> Layer {
        Layer {
            id: id.into(),
            name: name.into(),
            visible: true,
            opacity: 1.0,
            blend: Vec::new(),
            mask: None,
            geometry_mask: None,
            kind,
            effects: Vec::new(),
            asset: None,
            lock: None,
        }
    }

    /// The blend entry for `channel`; `None` means the layer does not affect it.
    pub fn blend_for(&self, channel: Channel) -> Option<&ChannelBlend> {
        self.blend.iter().find(|b| b.channel == channel)
    }

    /// Adds or replaces the blend entry for `channel`.
    pub fn set_blend(&mut self, channel: Channel, mode: BlendMode, opacity: f32) {
        let entry = ChannelBlend { channel, mode, opacity };
        match self.blend.iter_mut().find(|b| b.channel == channel) {
            Some(b) => *b = entry,
            None => self.blend.push(entry),
        }
    }

    /// Builder form of [`Self::set_blend`].
    pub fn with_blend(mut self, channel: Channel, mode: BlendMode, opacity: f32) -> Layer {
        self.set_blend(channel, mode, opacity);
        self
    }

    /// Builder: a painted mask (black with `mask` painted on top, see [`Mask::from_image`]).
    pub fn with_mask(mut self, mask: TiledImage) -> Layer {
        self.mask = Some(Mask::from_image(mask));
        self
    }

    /// Builder: adds `effect` on top of the content.
    pub fn with_effect(mut self, effect: ContentEffect) -> Layer {
        self.effects.push(effect);
        self
    }

    /// The layer this one is an instance of.
    pub fn instance_of(&self) -> Option<&InstanceLink> {
        match &self.kind {
            LayerKind::Instance(link) => Some(link),
            _ => None,
        }
    }

    pub fn children(&self) -> &[Layer] {
        match &self.kind {
            LayerKind::Group(c) => c,
            _ => &[],
        }
    }

    /// Every image the layer owns: paint images, content effect images and mask images, in that order.
    pub fn images(&self) -> Vec<(LayerImage, &TiledImage)> {
        let mut out: Vec<(LayerImage, &TiledImage)> = Vec::new();
        if let LayerKind::Paint(p) = &self.kind {
            out.extend(p.images.iter().map(|(c, i)| (LayerImage::Paint(*c), i)));
        }
        for (effect, e) in self.effects.iter().enumerate() {
            if let EffectKind::Paint(p) = &e.kind {
                out.extend(p.images.iter().map(|(channel, i)| (LayerImage::Effect { effect, channel: *channel }, i)));
            }
        }
        if let Some(m) = &self.mask {
            out.extend(m.images().map(|(effect, i)| (LayerImage::Mask { effect }, i)));
        }
        out
    }

    /// Mutable access to one of [`Self::images`].
    pub fn image_mut(&mut self, which: LayerImage) -> Option<&mut TiledImage> {
        match which {
            LayerImage::Paint(c) => match &mut self.kind {
                LayerKind::Paint(p) => p.image_mut(c),
                _ => None,
            },
            LayerImage::Effect { effect, channel } => match &mut self.effects.get_mut(effect)?.kind {
                EffectKind::Paint(p) => p.image_mut(channel),
                _ => None,
            },
            LayerImage::Mask { effect } => self.mask.as_mut()?.image_mut(effect),
        }
    }
}

/// Depth-first, pre-order walk over a layer tree, yielding `(depth, layer)`; a group comes before
/// its children. Created by [`crate::paint::TextureSet::walk_layers`].
pub struct LayerWalk<'a> {
    stack: Vec<std::slice::Iter<'a, Layer>>,
}

impl<'a> LayerWalk<'a> {
    pub fn new(layers: &'a [Layer]) -> LayerWalk<'a> {
        LayerWalk { stack: vec![layers.iter()] }
    }
}

impl<'a> Iterator for LayerWalk<'a> {
    type Item = (usize, &'a Layer);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let depth = self.stack.len().checked_sub(1)?;
            match self.stack.last_mut()?.next() {
                Some(l) => {
                    if let LayerKind::Group(children) = &l.kind {
                        self.stack.push(children.iter());
                    }
                    return Some((depth, l));
                }
                None => {
                    self.stack.pop();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::PixelFormat;
    use crate::paint::effect::{GreyBlend, MaskEffect, MaskPixels};

    #[test]
    fn blend_entries_and_walk() {
        let mut l = Layer::new("a", "A", LayerKind::Paint(PaintContent::default())).with_blend(Channel::BaseColor, BlendMode::Normal, 1.0);
        l.set_blend(Channel::BaseColor, BlendMode::Multiply, 0.5);
        assert_eq!(l.blend.len(), 1);
        assert_eq!(l.blend_for(Channel::BaseColor).map(|b| b.mode), Some(BlendMode::Multiply));
        assert!(l.blend_for(Channel::Roughness).is_none());
        let g = Layer::new("g", "G", LayerKind::Group(vec![l.clone(), Layer::new("b", "B", LayerKind::Group(vec![]))]));
        let top = vec![g, Layer::new("c", "C", LayerKind::Fill(Fill::uniform(vec![])))];
        let ids: Vec<_> = LayerWalk::new(&top).map(|(d, l)| format!("{d}{}", l.id)).collect();
        assert_eq!(ids, ["0g", "1a", "1b", "0c"]);
        assert_eq!(Fill::uniform(vec![(Channel::Metallic, [1.0; 4])]).uniform_value(Channel::Metallic), Some([1.0; 4]));
    }

    #[test]
    fn images_of_base_effects_and_mask() {
        let img = || TiledImage::new(2, 2, PixelFormat::from_id("graya8").unwrap()).unwrap();
        let mut base = PaintContent::default();
        base.set_image(Channel::Roughness, img());
        let mut fx = PaintContent::default();
        fx.set_image(Channel::Height, img());
        let mask = Mask::new(1.0)
            .with_effect(MaskEffect::new(GreyBlend::default(), EffectKind::Fill(0.5)))
            .with_effect(MaskEffect::new(GreyBlend::default(), EffectKind::Paint(MaskPixels::new(img()))));
        let mut l = Layer::new("l", "L", LayerKind::Paint(base)).with_effect(ContentEffect::new(Vec::new(), EffectKind::Paint(fx)));
        l.mask = Some(mask);
        let roles: Vec<_> = l.images().into_iter().map(|(r, _)| r).collect();
        assert_eq!(roles, [LayerImage::Paint(Channel::Roughness), LayerImage::Effect { effect: 0, channel: Channel::Height }, LayerImage::Mask { effect: 1 }]);
        for r in roles {
            assert!(l.image_mut(r).is_some(), "{r:?}");
        }
        assert!(l.image_mut(LayerImage::Mask { effect: 0 }).is_none());
    }
}
