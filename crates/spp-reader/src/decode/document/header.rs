//! The 12-byte header of Painter's object serialisation (`document.bin`, `default_material.bin`,
//! `layersstackstate.bin`): magic, a `u32` that is always 0, and a `u32` format version.

use super::RECORD;
use crate::container::bytes::ByteReader;
use crate::decode::DecodeError;

const MAGIC: [u8; 4] = [0xdd, 0x2f, 0x7c, 0x1b];
/// Offset of the root value, right after the header.
pub const ROOT_OFFSET: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentHeader {
    /// Grows with Painter releases: 51 (2019) … 78 (Painter 9, 2023). `layersstackstate.bin` uses 2.
    pub format_version: u32,
}

impl DocumentHeader {
    pub fn parse(data: &[u8]) -> Result<Self, DecodeError> {
        let mut r = ByteReader::new(data);
        let magic = r.bytes(MAGIC.len()).map_err(DecodeError::truncated(RECORD))?;
        if magic != MAGIC {
            return Err(DecodeError::BadMagic { record: RECORD });
        }
        let _reserved = r.u32_le().map_err(DecodeError::truncated(RECORD))?;
        let format_version = r.u32_le().map_err(DecodeError::truncated(RECORD))?;
        Ok(DocumentHeader { format_version })
    }
}
