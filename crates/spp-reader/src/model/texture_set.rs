//! [`PainterTextureSet`]: one texture set (`DataMaterial`) with its declared channels and layer stack.

use super::channel::PainterChannel;
use super::layer::PainterLayer;

#[derive(Debug, Clone, PartialEq)]
pub struct PainterTextureSet {
    /// The `DataMaterial`'s own uid.
    pub uid: Option<u64>,
    /// `sceneMaterialUID`: links to the mesh material and the texture-cache folder.
    pub material_uid: Option<u64>,
    /// `sceneMaterialName`: the set name Painter shows.
    pub name: String,
    /// `resolutionLog2` (11 = 2048).
    pub resolution_log2: Option<[i32; 2]>,
    /// `stacks[0].channels`.
    pub channels: Vec<ChannelDecl>,
    /// `stacks[0].stack.items`, **bottom → top**.
    pub layers: Vec<PainterLayer>,
}

/// A channel the set declares (`DataChannel`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelDecl {
    pub channel: PainterChannel,
    /// Storage format enum: 0 sRGB8, 1 L8, 5 L16F, 6 RGB16F (Verified for the channels in use).
    pub format: u64,
    /// Name of a user channel; empty for standard ones.
    pub user_name: String,
}

impl PainterTextureSet {
    /// Edge length in texels, if the resolution is known and sane.
    pub fn resolution(&self) -> Option<[u32; 2]> {
        let [w, h] = self.resolution_log2?;
        let side = |l: i32| u32::try_from(l).ok().filter(|l| *l < 32).map(|l| 1u32 << l);
        Some([side(w)?, side(h)?])
    }

    /// Every layer including those inside folders, depth first.
    pub fn all_layers(&self) -> Vec<&PainterLayer> {
        self.layers.iter().flat_map(|l| l.walk()).collect()
    }
}
