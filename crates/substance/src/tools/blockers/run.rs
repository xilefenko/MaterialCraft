//! Runs the census: imports every `.spp` below a folder (nothing is saved), keeps each one's
//! [`ProjectBlockers`] and drops the document, several projects at once within the pool's memory budget.

use super::census::{Census, Failure};
use super::project::ProjectBlockers;
use crate::inspect::Corpus;
use crate::tools::material_match::ProjectPlanner;
use crate::tools::parallel::CorpusPool;
use crate::translate::layers::replay::ReplayChoice;
use crate::translate::{Cancel, Importer};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// How the projects are imported and which.
pub struct CensusOptions<'a> {
    /// The procedural replacements (and materials) per project, as for `mcsub convert`.
    pub planner: &'a ProjectPlanner,
    pub replay: ReplayChoice,
    /// Only the first this many projects (corpus order).
    pub limit: Option<usize>,
}

/// The census of every `.spp` below `root` (or `root` itself when it is a file). `log` gets `[i/n] path` as
/// each project starts and a line per failure.
pub fn census(root: &Path, options: &CensusOptions, pool: CorpusPool, log: &(dyn Fn(&str) + Sync)) -> Census {
    let start = Instant::now();
    let mut files = if root.is_file() {
        vec![root.to_path_buf()]
    } else {
        Corpus::discover(root).files().to_vec()
    };
    if let Some(limit) = options.limit {
        files.truncate(limit);
    }
    let numbered: Vec<(usize, &PathBuf)> = files.iter().enumerate().collect();
    let shown = |p: &Path| {
        p.strip_prefix(root)
            .ok()
            .filter(|r| !r.as_os_str().is_empty())
            .unwrap_or(p)
            .to_path_buf()
    };
    let results = pool.map(
        &numbered,
        |(_, path)| CorpusPool::estimate(path),
        |&(i, path)| {
            log(&format!("[{}/{}] {}", i + 1, files.len(), path.display()));
            let result = import(path, shown(path), options);
            if let Err(e) = &result {
                log(&format!("FAILED {}: {e}", path.display()));
            }
            result
        },
    );
    let mut census = Census::default();
    for (path, result) in files.iter().zip(results) {
        match result {
            Ok(p) => census.projects.push(p),
            Err(error) => census.failures.push(Failure {
                path: shown(path),
                error,
            }),
        }
    }
    census.seconds = start.elapsed().as_secs_f64();
    census
}

/// One project imported as `mcsub convert` would, reduced to its record.
fn import(path: &Path, shown: PathBuf, options: &CensusOptions) -> Result<ProjectBlockers, String> {
    let start = Instant::now();
    let (plan, _) = options.planner.plan_for(path)?;
    let mut importer = Importer::open(path)
        .map_err(|e| e.to_string())?
        .with_plan(plan)
        .with_replay(options.replay);
    if let Some(library) = options.planner.library() {
        importer = importer.with_library(library);
    }
    let doc = importer.run(&mut |_, _| {}, Cancel::NEVER).map_err(|e| e.to_string())?;
    Ok(ProjectBlockers::of(shown, &doc, start.elapsed().as_secs_f64()))
}
