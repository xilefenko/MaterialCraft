//! Reads Substance 3D Painter projects (`.spp`): the file container, Painter's records and its layer model, as plain
//! data any application can translate into its own document. Nothing here knows an importer; MaterialCraft's lives in
//! `materialcraft-substance`.
//!
//! The crate is layered; each layer only uses the ones above it in this list:
//! - [`container`]: generic file formats (HDF5, Qt QDataStream, byte cursors). Knows nothing about Painter.
//! - [`decode`]: Painter's individual records (settings, resource images, texture cache, mesh scene, document).
//! - [`model`]: Painter's own concepts (texture sets, layers, effects, sources, strokes) built from `document.bin`.
//! - [`project`]: one opened `.spp` file, giving typed access to its parts.
//! - [`inspect`]: human-readable reports about one project or a whole folder of them.
//! - [`blockers`]: the shared vocabulary for what an importer can't evaluate yet (`substance:<graph>`,
//!   `brush-alpha:<name>`, …), so importers, reports and corpus censuses can name the same things the same way.
//!
//! Everything is read from the user's own files at run time; the crate contains no Painter code, data or resources.

pub mod blockers;
pub mod container;
pub mod decode;
pub mod inspect;
pub mod model;
pub mod project;

mod inflate;
