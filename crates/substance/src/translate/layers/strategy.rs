//! [`StackStrategy`]: the one decision per texture set about what the imported document shows.
//!
//! - **Real stack**: every channel of the imported layers reproduces Painter exactly (only visible,
//!   contributing layers count). The layers are the result; the flattened cache layer stays as a hidden
//!   reference at the top ("Painter result (reference)").
//! - **Painter result on top**: something is not computable (procedurals, strokes, a guessed blend mode …). The
//!   layers are imported as they are (those that can't be computed are locked), and a correction layer
//!   ([`super::correction`]) on top of them holds Painter's flattened result on exactly the texels the layers don't
//!   reproduce, so the visible result is Painter's while every layer can still be switched on and off underneath.
//!   The full flattened result stays as the hidden reference above it.
//! - **Cache disagrees**: every layer is computable, but the composite differs from Painter's cache beyond
//!   rounding ([`CacheAgreement`], e.g. a stale cache). Arranged like "Painter result on top", so an import
//!   never looks worse than the flattened one.
//! - **Replacements**: every channel is exact apart from Substance procedurals the user chose to replace with
//!   MaterialCraft's generators and brush strokes replayed by our brush pipeline. Arranged like the real stack (the layers are the result, editable; Painter's
//!   flattened result is the hidden reference), but not compared with the cache: replacements are measured
//!   approximations.
//! - **Best effort**: not computable and no texture cache to fall back on; the layers are all there is.
//!
//! Layers Painter shows whose mask is not computable would cover the whole set without it: they are hidden (and
//! locked); the correction shows what they contribute in Painter.
//!
//! "Painter defaults" (Painter's values for uncovered texels) sits at the bottom unless the layers already
//! cover every channel with an unmasked, full-strength uniform fill.

use super::cache_agreement::CacheAgreement;
use super::channels::SetChannels;
use super::verdict::StackVerdict;
use materialcraft::paint::{BlendMode, Layer, LayerKind};

pub const PAINTER_RESULT_NAME: &str = "Painter result";
pub const REFERENCE_NAME: &str = "Painter result (reference)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackStrategy {
    RealStack,
    Replacements,
    PainterResultOnTop,
    CacheDisagrees,
    BestEffort,
}

impl StackStrategy {
    /// `agreement` is the measurement of a fully computable stack against the cache (empty when not measured).
    pub fn choose(verdict: &StackVerdict, has_painter_result: bool, agreement: &CacheAgreement) -> StackStrategy {
        if !verdict.all_exact() && verdict.all_exact_or_replaced() {
            return StackStrategy::Replacements;
        }
        match (verdict.all_exact(), has_painter_result) {
            (true, true) if !agreement.agrees() => StackStrategy::CacheDisagrees,
            (true, _) => StackStrategy::RealStack,
            (false, true) => StackStrategy::PainterResultOnTop,
            (false, false) => StackStrategy::BestEffort,
        }
    }

    /// True when a correction layer ([`super::correction`]) goes between the imported layers and the reference.
    pub fn corrects(self) -> bool {
        matches!(self, StackStrategy::PainterResultOnTop | StackStrategy::CacheDisagrees)
    }

    pub fn describe(self) -> &'static str {
        match self {
            StackStrategy::RealStack => {
                "layers reproduce Painter exactly and are the result; Painter's flattened result kept hidden as a reference"
            }
            StackStrategy::Replacements => {
                "layers reproduce Painter with MaterialCraft's replacements for Substance procedurals or replayed brush strokes and are the result; Painter's flattened result kept hidden as a reference"
            }
            StackStrategy::PainterResultOnTop => {
                "not every layer is computable: a correction layer shows Painter's flattened result where the imported layers differ"
            }
            StackStrategy::CacheDisagrees => {
                "every layer is computable, but Painter's cache disagrees with the stored layers: a correction layer shows Painter's flattened result where they differ"
            }
            StackStrategy::BestEffort => {
                "not every layer is computable and there is no texture cache: the imported layers show what can be computed"
            }
        }
    }
}

/// What a set's final stack is made of.
pub struct StackParts {
    /// The imported layers, bottom → top.
    pub imported: Vec<Layer>,
    /// The flattened cache layer, if the set has a texture cache.
    pub painter_result: Option<Layer>,
    /// Painter's channel defaults as a uniform fill.
    pub defaults: Layer,
    /// Ids of layers to hide: Painter shows them, but their masks are not computable.
    pub uncomputable_masks: Vec<String>,
}

impl StackStrategy {
    /// The final layers, bottom → top. For a strategy that [`Self::corrects`], the correction is inserted below
    /// the reference (the last layer) once the rest is composited.
    pub fn assemble(self, parts: StackParts, channels: &SetChannels) -> Vec<Layer> {
        let StackParts {
            imported,
            painter_result,
            defaults,
            uncomputable_masks,
        } = parts;
        let mut imported = imported;
        hide(&mut imported, &uncomputable_masks);
        let mut out = Vec::new();
        if !provides_base(&imported, channels) {
            out.push(defaults);
        }
        out.extend(imported);
        if self != StackStrategy::BestEffort
            && let Some(mut result) = painter_result
        {
            result.name = REFERENCE_NAME.into();
            result.visible = false;
            out.push(result);
        }
        out
    }
}

/// Hides the layers with these ids, at any depth.
fn hide(layers: &mut [Layer], ids: &[String]) {
    for l in layers {
        if ids.contains(&l.id) {
            l.visible = false;
        }
        if let LayerKind::Group(children) = &mut l.kind {
            hide(children, ids);
        }
    }
}

/// True if every declared channel is fully covered by a visible, unmasked top-level uniform fill at
/// normal or replace blending and full opacity.
pub fn provides_base(layers: &[Layer], channels: &SetChannels) -> bool {
    channels.all().all(|channel| {
        layers.iter().any(|l| {
            let full = l
                .blend_for(channel)
                .is_some_and(|b| matches!(b.mode, BlendMode::Normal | BlendMode::Replace) && b.opacity >= 1.0);
            let value = matches!(&l.kind, LayerKind::Fill(f) if f.uniform_value(channel).is_some());
            let unmasked = l.mask.as_ref().is_none_or(|m| m.is_identity());
            l.visible && unmasked && l.opacity >= 1.0 && full && value
        })
    })
}
