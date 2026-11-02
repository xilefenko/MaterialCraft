//! Background loading of a document: `.mcpaint` directly, a new project from a mesh file, `.spp` through the in-process importer
//! ([`SppImport`], which keeps a conversion cache).

use super::job::{CancelToken, Job};
use super::spp_import::SppImport;
use materialcraft::paint::new_project::{self, NewProject};
use materialcraft::paint::{self, Document};
use materialcraft_substance::translate::layers::replay::ReplayChoice;
use materialcraft_substance::translate::replacements::ProceduralPlan;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

/// A document read from disk.
pub struct Loaded {
    pub document: Document,
    /// The file the user opened (the `.spp` for imports).
    pub source: PathBuf,
    /// Importer summary line, for imports.
    pub summary: Option<String>,
    /// What the user should know about how a new project was made (files left out, the unit, cameras).
    pub notes: Vec<String>,
}

enum Event {
    Progress(f32, String),
    Done(Result<Loaded, String>),
}

/// One load in progress. Dropping it cancels the load.
pub struct Loader {
    job: Job<Event>,
    source: PathBuf,
    progress: (f32, String),
}

impl Loader {
    pub fn start(source: PathBuf) -> Loader {
        Loader::start_with(source, ProceduralPlan::default(), Vec::new(), ReplayChoice::default())
    }

    /// Like [`Loader::start`], importing a `.spp` with the procedural replacements of `plan` and the `replay`
    /// choice for brush strokes; the materials it picks are embedded from the asset library `folders`.
    pub fn start_with(source: PathBuf, plan: ProceduralPlan, library: Vec<PathBuf>, replay: ReplayChoice) -> Loader {
        let path = source.clone();
        let job = Job::spawn(move |cancel, tx| {
            let result = load(&path, &plan, &library, replay, cancel, tx);
            let _ = tx.send(Event::Done(result));
        });
        Loader { job, source, progress: (0.0, "Opening".into()) }
    }

    /// Makes a new project from a mesh file (File > New project); the mesh file is its source until it is saved.
    pub fn start_new(project: NewProject) -> Loader {
        let source = project.mesh.clone();
        let job = Job::spawn(move |_, tx| {
            let mut progress = |f: f32, m: &str| {
                let _ = tx.send(Event::Progress(f, m.to_string()));
            };
            let result = new_project::create(&project, &mut progress)
                .map(|c| Loaded { document: c.document, source: project.mesh.clone(), summary: None, notes: c.notes })
                .map_err(|e| format!("No project was made: {e}."));
            let _ = tx.send(Event::Done(result));
        });
        Loader { job, source, progress: (0.0, "Reading the mesh".into()) }
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Fraction done and the current step.
    pub fn progress(&self) -> (f32, &str) {
        (self.progress.0, &self.progress.1)
    }

    /// The result once the load has finished.
    pub fn poll(&mut self) -> Option<Result<Loaded, String>> {
        let mut done = None;
        for e in self.job.poll() {
            match e {
                Event::Progress(f, m) => self.progress = (f, m),
                Event::Done(r) => done = Some(r),
            }
        }
        if done.is_none() && self.job.is_finished() {
            done = Some(Err("The loader stopped unexpectedly.".into()));
        }
        done
    }
}

fn load(path: &Path, plan: &ProceduralPlan, library: &[PathBuf], replay: ReplayChoice, cancel: &CancelToken, tx: &Sender<Event>) -> Result<Loaded, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "mcpaint" => {
            let _ = tx.send(Event::Progress(0.95, "Reading document".into()));
            Ok(Loaded { document: read_document(path)?, source: path.to_path_buf(), summary: None, notes: Vec::new() })
        }
        "spp" => {
            let mut progress = |f: f32, m: &str| {
                let _ = tx.send(Event::Progress(f, m.to_string()));
            };
            let imported = SppImport::new().with_plan(plan.clone()).with_library(library.to_vec()).with_replay(replay).open(path, cancel, &mut progress)?;
            Ok(Loaded { document: imported.document, source: path.to_path_buf(), summary: Some(imported.summary), notes: Vec::new() })
        }
        _ => Err(format!("{} is neither a .mcpaint document nor a .spp project.", path.display())),
    }
}

fn read_document(file: &Path) -> Result<Document, String> {
    paint::format::load_path(file).map_err(|e| format!("Cannot read {}: {e}", file.display()))
}
