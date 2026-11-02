//! Command-line tools built on the importer: map dumps, comparison with Painter exports, batch conversion,
//! and text views of Painter's model.
//! `mcsub` only parses arguments and calls into here.

pub mod accuracy;
pub mod analyze;
pub mod blockers;
pub mod compare;
pub mod convert;
pub mod library;
pub mod maps;
pub mod material_match;
pub mod model_corpus;
pub mod model_text;
pub mod parallel;
pub mod png_out;
pub mod report_text;
pub mod verify_layers;
