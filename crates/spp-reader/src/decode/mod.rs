//! Painter's individual records, each decoded from the bytes of one HDF5 dataset.
//! Decoders take `&[u8]` and never touch files, so each can be tested on its own.

pub mod assbin;
pub mod document;
pub mod error;
pub mod image;
mod lz4_payload;
pub mod resource_toc;
pub mod settings;
pub mod texture_cache;

pub use error::DecodeError;
pub use lz4_payload::Decompressed;
