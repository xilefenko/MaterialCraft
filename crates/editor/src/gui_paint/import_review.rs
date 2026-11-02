//! [`ImportReview`]: the step between choosing a `.spp` and importing it. The project is analysed in the
//! background (`mcsub analyze`'s [`Analysis`]: which Substance procedurals it uses, which of them MaterialCraft
//! can replace, their measured accuracy; and its Substance material fills with the tray materials suggested
//! for them, [`MaterialMatches`]; and what it brings for the user's own library, [`LibraryCandidates`]); when
//! something can be replaced or added the user picks in the [`ReviewDialog`], otherwise the import starts right away.

use super::job::Job;
use super::panels::import_review::{ReviewDialog, ReviewResult};
use eframe::egui;
use materialcraft_substance::project::SppProject;
use materialcraft_substance::tools::accuracy::ReplacementAccuracy;
use materialcraft_substance::tools::analyze::Analysis;
use materialcraft_substance::tools::material_match::{MaterialMatches, TrayMaterials};
use materialcraft_substance::translate::layers::replay::ReplayChoice;
use materialcraft_substance::translate::library::LibraryCandidates;
use materialcraft_substance::translate::replacements::ProceduralPlan;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What the background analysis found.
pub struct Findings {
    pub analysis: Analysis,
    pub materials: MaterialMatches,
    pub tray: Arc<TrayMaterials>,
    /// What the project brings for the user's own library (empty when that could not be read).
    pub library: LibraryCandidates,
}

impl Findings {
    /// True when the review has something to ask.
    fn has_choices(&self) -> bool {
        self.analysis.procedurals.iter().any(|p| p.replacement.is_some()) || !self.materials.materials.is_empty() || !self.library.is_empty()
    }
}

enum State {
    Analysing(Job<Result<Findings, String>>),
    Choosing(Box<ReviewDialog>),
}

/// What the review decided this frame.
pub enum ReviewOutcome {
    /// Still analysing or waiting for the user.
    Pending,
    /// Import with these choices (procedurals, stroke replay); the library items, if any were ticked, are written into
    /// the user's library.
    Import(ProceduralPlan, Option<LibraryRequest>, ReplayChoice),
    Cancelled,
}

/// Library items the review ticked, to be written into `library` (the user's own library folder).
pub struct LibraryRequest {
    pub candidates: Arc<LibraryCandidates>,
    pub keys: Vec<String>,
    pub library: PathBuf,
}

pub struct ImportReview {
    source: PathBuf,
    /// The asset library folders whose materials are offered (and embedded on import).
    library: Vec<PathBuf>,
    /// The user's own library folder, where ticked library items go (`None`: there is none, nothing can be added).
    user_library: Option<PathBuf>,
    /// The choices the review starts with (and imports with when there is nothing to ask).
    initial: ProceduralPlan,
    /// The stroke replay choice to start from.
    initial_replay: ReplayChoice,
    state: State,
}

impl ImportReview {
    /// Starts analysing `source` in the background, offering the materials of the `library` folders and adding
    /// ticked project assets to `user_library`.
    pub fn start(source: PathBuf, library: Vec<PathBuf>, user_library: Option<PathBuf>) -> ImportReview {
        ImportReview::start_with(source, library, user_library, ProceduralPlan::default(), ReplayChoice::default())
    }

    /// [`Self::start`] with the choices starting at `initial` (a re-import repeats the earlier import's choices).
    pub fn start_with(
        source: PathBuf,
        library: Vec<PathBuf>,
        user_library: Option<PathBuf>,
        initial: ProceduralPlan,
        initial_replay: ReplayChoice,
    ) -> ImportReview {
        let (path, folders) = (source.clone(), library.clone());
        let job = Job::spawn(move |_, tx| {
            let _ = tx.send(analyse(&path, &folders));
        });
        ImportReview { source, library, user_library, initial, initial_replay, state: State::Analysing(job) }
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn library(&self) -> &[PathBuf] {
        &self.library
    }

    /// Polls the analysis and shows the dialog once it is there.
    pub fn ui(&mut self, ctx: &egui::Context) -> ReviewOutcome {
        match &mut self.state {
            State::Analysing(job) => {
                let Some(result) = job.poll().into_iter().next() else {
                    if job.is_finished() {
                        // The analysis thread ended without an answer: import with the initial choices.
                        return ReviewOutcome::Import(self.initial.clone(), None, self.initial_replay);
                    }
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                    return ReviewOutcome::Pending;
                };
                match result {
                    Ok(f) if f.has_choices() => {
                        self.state = State::Choosing(Box::new(ReviewDialog::new(f, self.user_library.is_some()).prefilled(&self.initial, self.initial_replay)));
                        ReviewOutcome::Pending
                    }
                    // Nothing to choose (or the layer document is unreadable, which the importer reports).
                    _ => ReviewOutcome::Import(self.initial.clone(), None, self.initial_replay),
                }
            }
            State::Choosing(dialog) => match dialog.show(ctx, &self.source) {
                Some(ReviewResult::Import { plan, library, replay }) => {
                    let request = match (&self.user_library, library.is_empty()) {
                        (Some(folder), false) => {
                            Some(LibraryRequest { candidates: dialog.library_candidates().clone(), keys: library, library: folder.clone() })
                        }
                        _ => None,
                    };
                    ReviewOutcome::Import(plan, request, replay)
                }
                Some(ReviewResult::Cancel) => ReviewOutcome::Cancelled,
                None => ReviewOutcome::Pending,
            },
        }
    }
}

/// The analysis, the tray materials ranked for each material fill (Painter's look measured in the project) and the
/// project's library candidates.
fn analyse(path: &Path, library: &[PathBuf]) -> Result<Findings, String> {
    let analysis = Analysis::of_project(path, &ReplacementAccuracy::load())?;
    let tray = Arc::new(match analysis.materials().next() {
        Some(_) => TrayMaterials::load(library),
        None => TrayMaterials::default(),
    });
    let materials = MaterialMatches::measure(path, &analysis, &tray)?;
    let library = SppProject::open(path).ok().and_then(|p| LibraryCandidates::of_project(&p).ok()).unwrap_or_default();
    Ok(Findings { analysis, materials, tray, library })
}
