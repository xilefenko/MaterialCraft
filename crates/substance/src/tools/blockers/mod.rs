//! `mcsub blockers`: which missing pieces lock the most layers over a corpus of Painter projects, the roadmap to
//! full parity. Every project is imported (nothing saved), its locked layers are grouped by blocker key
//! ([`crate::translate::blocker_summary::BlockerSummary`]) and the keys are added up over the corpus and ranked
//! by how many layers supporting each one would unlock ([`Census::ranked`]).
//!
//! - [`project`]: one imported project's record.
//! - [`census`]: the aggregation, without I/O.
//! - [`render`]: markdown, TSV and JSON.
//! - [`run`]: imports the projects on a [`crate::tools::parallel::CorpusPool`].

pub mod census;
pub mod project;
pub mod render;
pub mod run;

pub use census::{Census, Failure, KeyTotal, Totals, Unlock};
pub use project::{ProjectBlockers, SetCoverage};
pub use render::{CensusText, RenderOptions, census_json};
pub use run::{CensusOptions, census};
