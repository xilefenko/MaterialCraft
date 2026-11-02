//! [`PainterDocument`]: the whole `document.bin` as Painter's own concepts, and its entry point.

use super::builder::ModelBuilder;
use super::raw::RawTree;
use super::report::ModelReport;
use super::texture_set::PainterTextureSet;
use crate::decode::DecodeError;

#[derive(Debug, Clone, PartialEq)]
pub struct PainterDocument {
    /// Header format version (51 in 2019 … 78 in Painter 9).
    pub format_version: u32,
    /// `DataDocument.materials`, in document order (includes stale sets no mesh uses any more).
    pub texture_sets: Vec<PainterTextureSet>,
}

impl PainterDocument {
    /// Builds the model from the bytes of `document.bin`. Fails only where the object serialisation
    /// itself is broken; unknown classes are kept raw and counted in the report.
    pub fn from_bytes(data: &[u8]) -> Result<(PainterDocument, ModelReport), DecodeError> {
        let tree = RawTree::parse(data)?;
        Ok(ModelBuilder::build(&tree))
    }

    /// The set with this `sceneMaterialUID`.
    pub fn texture_set(&self, material_uid: u64) -> Option<&PainterTextureSet> {
        self.texture_sets.iter().find(|s| s.material_uid == Some(material_uid))
    }
}
