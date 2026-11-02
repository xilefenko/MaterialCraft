//! Reading and writing `.mcpaint` files, the native Paint-mode document format.
//!
//! A `.mcpaint` file is a ZIP archive holding a JSON manifest (`document.json`), the mesh as glTF 2.0
//! binary (`mesh/model.glb`), every layer, mask and baked image as PNG (integer formats) or OpenEXR
//! (float formats), untranslated import originals under `foreign/`, the import report and a
//! thumbnail. The specification is `formats/mcpaint.md` in the MaterialCraft docs.
//!
//! - [`images`]: pixel format → PNG/EXR encoding and back.
//! - [`manifest`]: the JSON mapping of every model type.
//! - [`paths`] and [`slot`]: entry names derived from ids; which image an entry holds.
//! - [`strokes`]: stroke record entries (binary, one per recorded paint layer or effect).
//! - [`writer`] and [`reader`]: assembling and taking apart the archive; [`embedded`]: embedded library assets.
//! - [`fragment`]: smart materials and smart masks (`.mcfrag`), built from the same pieces.
//! - [`template`]: project templates (`.mctemplate`), defaults for new projects, built from the same pieces.
//!
//! ```
//! use materialcraft::paint::{format, Document};
//! let mut bytes = Vec::new();
//! format::save(&Document::default(), &mut bytes).unwrap();
//! let doc = format::load(std::io::Cursor::new(bytes)).unwrap();
//! assert_eq!(doc, Document::default());
//! ```

mod batch;
pub mod embedded;
mod error;
pub mod fragment;
pub mod images;
pub mod manifest;
pub mod paths;
pub mod reader;
pub mod slot;
pub mod strokes;
pub mod template;
mod thumbnail;
pub mod version;
pub mod writer;

#[cfg(test)]
mod tests;

pub use error::FormatError;
pub use reader::LoadOptions;
pub use thumbnail::Thumbnail;
pub use writer::SaveOptions;

use crate::paint::Document;
use std::io::{Read, Seek, Write};
use std::path::Path;

/// Version written by [`save`]: `major.minor`. Readers reject a newer major version.
pub const FORMAT_VERSION: &str = "0.18";

/// Writes `doc` as a `.mcpaint` archive with default options (including a thumbnail).
/// Same document, same bytes.
pub fn save(doc: &Document, out: impl Write) -> Result<(), FormatError> {
    save_with(doc, out, &SaveOptions::default())
}

/// [`save`] with explicit options.
pub fn save_with(doc: &Document, out: impl Write, options: &SaveOptions) -> Result<(), FormatError> {
    writer::DocumentWriter::new(options).write(doc, out)
}

/// Saves to `path` through a temporary file in the same directory, renamed over `path` once
/// complete, so a failed save never leaves a half-written document behind.
pub fn save_path(doc: &Document, path: &Path) -> Result<(), FormatError> {
    writer::save_atomically(path, |out| save(doc, out))
}

/// [`save_path`] that tells `written` the number of bytes written so far after each write (a progress display for
/// large documents; the total is not known in advance).
pub fn save_path_observed(doc: &Document, path: &Path, written: &mut dyn FnMut(u64)) -> Result<(), FormatError> {
    writer::save_atomically(path, |out| save(doc, Counting { out, bytes: 0, written }))
}

/// A writer that reports how much went through it.
struct Counting<'a, W: Write> {
    out: W,
    bytes: u64,
    written: &'a mut dyn FnMut(u64),
}

impl<W: Write> Write for Counting<'_, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.out.write(buf)?;
        self.bytes += n as u64;
        (self.written)(self.bytes);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

/// Reads a `.mcpaint` archive with default limits.
pub fn load(input: impl Read + Seek) -> Result<Document, FormatError> {
    load_with(input, &LoadOptions::default())
}

/// [`load`] with explicit limits.
pub fn load_with(input: impl Read + Seek, options: &LoadOptions) -> Result<Document, FormatError> {
    reader::DocumentReader::open(input, options)?.read()
}

/// Reads the `.mcpaint` file at `path`.
pub fn load_path(path: &Path) -> Result<Document, FormatError> {
    let file = std::fs::File::open(path)?;
    load(std::io::BufReader::new(file))
}
