//! Resolving instances into a texture set at composite time.

use super::InstanceLink;
use super::graph::layer;
use crate::paint::doc::Document;
use crate::paint::effect::{EffectKind, MaskPixels};
use crate::paint::image::TiledImage;
use crate::paint::layer::{Layer, LayerKind, PaintContent};
use crate::paint::texture_set::TextureSet;
use std::borrow::Cow;

/// Separates an instance's id from the ids of the copied layers below it (`instance>child`), so the resolved stack
/// keeps every id unique.
pub const CHILD_ID_SEPARATOR: char = '>';

impl Document {
    /// Texture set `index` as the compositor sees it: every instance replaced by a copy of its source (see
    /// [`crate::paint::instance`]). Borrowed when the set has no instances. Broken instances (missing source, cycle)
    /// stay as they are and composite nothing.
    pub fn resolved_set(&self, index: usize) -> Option<Cow<'_, TextureSet>> {
        let set = self.texture_sets.get(index)?;
        if !set.walk_layers().any(|(_, l)| matches!(l.kind, LayerKind::Instance(_))) {
            return Some(Cow::Borrowed(set));
        }
        let r = Resolver { doc: self, resolution: set.resolution };
        let mut chain = Vec::new();
        let layers = set.layers.iter().map(|l| r.layer(l, &set.id, &mut chain)).collect();
        Some(Cow::Owned(TextureSet { layers, ..set.clone_without_layers() }))
    }
}

impl TextureSet {
    /// A copy of everything but the layer stack.
    fn clone_without_layers(&self) -> TextureSet {
        TextureSet {
            id: self.id.clone(),
            name: self.name.clone(),
            material_slot: self.material_slot,
            uv_set: self.uv_set,
            resolution: self.resolution,
            channels: self.channels.clone(),
            layers: Vec::new(),
            baked: self.baked.clone(),
            bake: self.bake.clone(),
        }
    }
}

struct Resolver<'a> {
    doc: &'a Document,
    /// The resolution of the set being resolved (blank mask paint is made at it).
    resolution: (u32, u32),
}

impl Resolver<'_> {
    /// `l` (a layer of set `home`) with every instance in it resolved. `chain` holds the links being resolved.
    fn layer(&self, l: &Layer, home: &str, chain: &mut Vec<InstanceLink>) -> Layer {
        match &l.kind {
            LayerKind::Instance(link) => self.instance(l, link, home, chain),
            LayerKind::Group(children) => {
                let children = children.iter().map(|c| self.layer(c, home, chain)).collect();
                Layer { kind: LayerKind::Group(children), ..shallow(l) }
            }
            LayerKind::Paint(_) | LayerKind::Fill(_) => l.clone(),
        }
    }

    fn instance(&self, inst: &Layer, link: &InstanceLink, home: &str, chain: &mut Vec<InstanceLink>) -> Layer {
        let Some(source) = layer(self.doc, (&link.set, &link.layer)) else {
            return inst.clone();
        };
        if chain.contains(link) || self.doc.instance_on_cycle(home, &inst.id) {
            return inst.clone();
        }
        chain.push(link.clone());
        let mut out = self.layer(source, &link.set, chain);
        chain.pop();
        strip_paint(&mut out, self.resolution);
        prefix_children(&mut out, &inst.id);
        out.id = inst.id.clone();
        out.name = inst.name.clone();
        out.visible = inst.visible;
        out
    }
}

/// `l` without its children (a group's are filled in by the caller).
fn shallow(l: &Layer) -> Layer {
    Layer {
        id: l.id.clone(),
        name: l.name.clone(),
        visible: l.visible,
        opacity: l.opacity,
        blend: l.blend.clone(),
        mask: l.mask.clone(),
        geometry_mask: l.geometry_mask.clone(),
        kind: LayerKind::Group(Vec::new()),
        effects: l.effects.clone(),
        asset: l.asset.clone(),
        lock: l.lock.clone(),
    }
}

/// Drops the painted pixels of `l` and its subtree (IN-03): paint content and paint effects lose their images,
/// painted mask effects start blank at `resolution`.
fn strip_paint(l: &mut Layer, resolution: (u32, u32)) {
    match &mut l.kind {
        LayerKind::Paint(p) => *p = PaintContent::default(),
        LayerKind::Group(children) => children.iter_mut().for_each(|c| strip_paint(c, resolution)),
        LayerKind::Fill(_) | LayerKind::Instance(_) => {}
    }
    for e in &mut l.effects {
        if let EffectKind::Paint(p) = &mut e.kind {
            *p = PaintContent::default();
        }
    }
    if let Some(m) = &mut l.mask {
        for e in &mut m.effects {
            if let EffectKind::Paint(px) = &mut e.kind {
                match TiledImage::new(resolution.0, resolution.1, px.image.format()) {
                    Ok(blank) => *px = MaskPixels::new(blank),
                    Err(_) => e.enabled = false,
                }
            }
        }
    }
}

/// Gives every layer below `l` the id `prefix>id`.
fn prefix_children(l: &mut Layer, prefix: &str) {
    if let LayerKind::Group(children) = &mut l.kind {
        for c in children {
            c.id = format!("{prefix}{CHILD_ID_SEPARATOR}{}", c.id);
            prefix_children(c, prefix);
        }
    }
}
