//! Human-readable reports about one project ([`ProjectReport`]) or a folder of them ([`Corpus`]).

mod cache_summary;
mod corpus;
mod corpus_table;
pub mod document_dump;
mod report;
mod resource_summary;
mod tally;
mod text;
mod units;

pub use cache_summary::CacheSummary;
pub use corpus::{Corpus, CorpusRow, RowFacts};
pub use corpus_table::CorpusTable;
pub use document_dump::DocumentDump;
pub use report::{InspectOptions, Outcome, ProjectReport};
pub use resource_summary::ResourceSummary;
pub use tally::{Tally, Verification};
pub use text::TextReport;
