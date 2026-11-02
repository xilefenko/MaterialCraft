//! What an imported project brings for the user's own asset library: the import review lists these
//! [`LibraryCandidates`] with an "Add to my library" checkbox each, and [`LibraryExport`] writes the ticked ones into
//! the user's library folder. The imported document is not touched: it keeps embedding its own copies.
//!
//! - [`origin`]: where a candidate came from and the rule for vendor shelf content ("from this project, not
//!   shareable").
//! - [`brush`]: a paint effect's brush as our brush preset (through the replay's mapping table).
//! - [`collect`]: the walk over the layer stacks and viewer settings.
//! - [`export`]: candidates → assets written by `materialcraft::assets`.
//!
//! Everything offered comes from the user's own file and only ever lands in the user's library; MaterialCraft ships
//! none of it and the starter library refuses imported assets. Smart materials and smart masks are not offered yet:
//! they need the translated layer stacks with the review's replacements, i.e. a whole import.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod brush;
pub mod candidate;
pub mod collect;
pub mod export;
pub mod origin;
#[cfg(test)]
mod tests;

pub use candidate::{CandidateKind, CandidateSource, LibraryCandidate, LibraryCandidates};
pub use collect::{Collector, ProjectFacts};
pub use export::{IMPORTED_TAG, LibraryExport, NOT_SHAREABLE_TAG, Written};
pub use origin::Origin;

use super::baked_maps::is_baked_name;
use super::resources::{resource_dataset, shelf_version, split_resource_url};
use crate::model::viewer::{PainterViewer, VIEWER_INI};
use crate::project::SppProject;
use std::collections::HashSet;

impl LibraryCandidates {
    /// The candidates of an opened project: its layer document, the baked maps its sets name (left out) and the
    /// viewer's environment map. Fails only when the layer document cannot be read.
    pub fn of_project(project: &SppProject) -> Result<LibraryCandidates, String> {
        let (document, _) = project.model().map_err(|e| e.to_string())?;
        let baked: HashSet<String> = project
            .document_materials()
            .unwrap_or_default()
            .into_iter()
            .flat_map(|m| m.baked)
            .map(|b| b.url)
            .collect();
        let environment = project
            .read(VIEWER_INI)
            .ok()
            .and_then(|data| PainterViewer::parse(&data).ok())
            .and_then(|v| v.environment_url);
        let stored = |url: &str| {
            let id = split_resource_url(url).map(|(_, id)| id).or_else(|| shelf_version(url));
            id.is_some_and(|id| resource_dataset(project, id).is_ok())
        };
        let facts = ProjectFacts {
            project: project.path().display().to_string(),
            baked,
            environment,
            stored: &stored,
            baked_name: &is_baked_name,
        };
        Ok(Collector::new(&facts).collect(&document))
    }
}
