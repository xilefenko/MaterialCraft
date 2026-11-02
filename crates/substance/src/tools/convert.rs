//! `mcsub convert`: import a `.spp` and save it as `.mcpaint`, for one file or a whole folder.

use crate::inspect::Corpus;
use crate::tools::material_match::ProjectPlanner;
use crate::tools::parallel::CorpusPool;
use crate::translate::layers::replay::ReplayChoice;
use crate::translate::{Cancel, ImportError, Importer};
use materialcraft::paint::format::{FormatError, save_path};
use materialcraft::paint::layer::LayerWalk;
use materialcraft::paint::{Document, EffectKind, Fidelity, FillSource, LayerKind};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Share of the progress bar for the import; saving takes the rest.
const IMPORT_SHARE: f32 = 0.85;

#[derive(Debug)]
pub enum ConvertError {
    /// The plan for the project could not be made (`--materials auto` reads the project first).
    Plan(String),
    Import(ImportError),
    Save(FormatError),
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConvertError::Plan(e) => write!(f, "plan failed: {e}"),
            ConvertError::Import(e) => write!(f, "import failed: {e}"),
            ConvertError::Save(e) => write!(f, "save failed: {e}"),
        }
    }
}

impl std::error::Error for ConvertError {}

/// What one conversion produced.
#[derive(Debug, Clone)]
pub struct ConvertSummary {
    pub output: PathBuf,
    pub sets: usize,
    pub channels: usize,
    pub baked: usize,
    /// Strokes and samples kept as stroke records.
    pub strokes: (usize, usize),
    /// Report entries per fidelity: exact, approximated, baked, kept raw.
    pub fidelity: [usize; 4],
    /// Material fills replaced by a material of ours.
    pub materials: usize,
    pub bytes: u64,
    pub import_seconds: f64,
    pub save_seconds: f64,
}

impl ConvertSummary {
    fn new(doc: &Document, output: &Path, import_seconds: f64, save_seconds: f64) -> Self {
        let count = |f| doc.import_report.as_ref().map_or(0, |r| r.count(f));
        ConvertSummary {
            output: output.to_path_buf(),
            sets: doc.texture_sets.len(),
            channels: doc.texture_sets.iter().map(|s| s.channels.len()).sum(),
            baked: doc.texture_sets.iter().map(|s| s.baked.len()).sum(),
            strokes: materialcraft::paint::stroke::totals(doc),
            fidelity: [
                Fidelity::Exact,
                Fidelity::Approximated,
                Fidelity::Baked,
                Fidelity::KeptRaw,
            ]
            .map(count),
            materials: replaced_materials(doc),
            bytes: std::fs::metadata(output).map_or(0, |m| m.len()),
            import_seconds,
            save_seconds,
        }
    }
}

impl fmt::Display for ConvertSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [exact, approx, baked, raw] = self.fidelity;
        write!(
            f,
            "ok {}: {} texture sets, {} channels, {} baked maps, {} strokes ({} samples) recorded; report {exact} \
             exact, {approx} approximated, {baked} baked, {raw} kept raw; {} material fills replaced; {:.1} MB; import {:.1} s, \
             save {:.1} s",
            self.output.display(),
            self.sets,
            self.channels,
            self.baked,
            self.strokes.0,
            self.strokes.1,
            self.materials,
            self.bytes as f64 / 1048576.0,
            self.import_seconds,
            self.save_seconds
        )
    }
}

/// Imports `input` with the replacements `planner` chooses for it and saves it as `output`; picked materials
/// are embedded when the planner has a library, and painted masks are replayed when `replay` says so.
pub fn convert_one(
    input: &Path,
    output: &Path,
    planner: &ProjectPlanner,
    replay: ReplayChoice,
    progress: &mut dyn FnMut(f32, &str),
) -> Result<ConvertSummary, ConvertError> {
    let start = Instant::now();
    let (plan, _) = planner.plan_for(input).map_err(ConvertError::Plan)?;
    let mut on = |f: f32, m: &str| progress(f * IMPORT_SHARE, m);
    let mut importer = Importer::open(input)
        .map_err(ConvertError::Import)?
        .with_plan(plan)
        .with_replay(replay);
    if let Some(library) = planner.library() {
        importer = importer.with_library(library);
    }
    let doc = importer.run(&mut on, Cancel::NEVER).map_err(ConvertError::Import)?;
    let imported = start.elapsed().as_secs_f64();
    progress(IMPORT_SHARE, "saving");
    save_path(&doc, output).map_err(ConvertError::Save)?;
    let saved = start.elapsed().as_secs_f64() - imported;
    progress(1.0, "done");
    Ok(ConvertSummary::new(&doc, output, imported, saved))
}

/// Results of [`convert_all`], printable as a table.
pub struct ConvertTable {
    pub rows: Vec<(PathBuf, Result<ConvertSummary, String>)>,
    pub seconds: f64,
}

/// Converts every `.spp` below `root` into `out_dir` (`<n>.mcpaint`), several projects at once on `pool`;
/// `discard` deletes each output after measuring it. `log` gets one line per file as it starts; the table
/// lists the files in corpus order.
pub fn convert_all(
    root: &Path,
    out_dir: &Path,
    planner: &ProjectPlanner,
    discard: bool,
    pool: CorpusPool,
    log: &(dyn Fn(&str) + Sync),
) -> ConvertTable {
    let start = Instant::now();
    let files = Corpus::discover(root).files().to_vec();
    if let Err(e) = std::fs::create_dir_all(out_dir) {
        log(&format!("cannot create {}: {e}", out_dir.display()));
    }
    let numbered: Vec<(usize, &PathBuf)> = files.iter().enumerate().collect();
    let results = pool.map(
        &numbered,
        |(_, input)| CorpusPool::estimate(input),
        |&(i, input)| {
            log(&format!("[{}/{}] {}", i + 1, files.len(), input.display()));
            let output = out_dir.join(format!("{i:03}.mcpaint"));
            let result = convert_one(input, &output, planner, ReplayChoice::default(), &mut |_, _| {})
                .map_err(|e| e.to_string());
            if discard {
                let _ = std::fs::remove_file(&output);
            }
            result
        },
    );
    let rows = files
        .iter()
        .zip(results)
        .map(|(input, result)| (input.strip_prefix(root).unwrap_or(input).to_path_buf(), result))
        .collect();
    ConvertTable {
        rows,
        seconds: start.elapsed().as_secs_f64(),
    }
}

impl fmt::Display for ConvertTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ok: Vec<&ConvertSummary> = self.rows.iter().filter_map(|(_, r)| r.as_ref().ok()).collect();
        for (path, result) in &self.rows {
            match result {
                Ok(s) => writeln!(
                    f,
                    "ok    {:>6.1}s {:>6.1}s {:>8.1} MB  {:>2} sets {:>6} strokes  {}",
                    s.import_seconds,
                    s.save_seconds,
                    s.bytes as f64 / 1048576.0,
                    s.sets,
                    s.strokes.0,
                    path.display()
                )?,
                Err(e) => writeln!(f, "FAIL  {}: {e}", path.display())?,
            }
        }
        let sum = |g: fn(&ConvertSummary) -> f64| ok.iter().map(|s| g(s)).sum::<f64>();
        let strokes: usize = ok.iter().map(|s| s.strokes.0).sum();
        let materials: usize = ok.iter().map(|s| s.materials).sum();
        let samples: usize = ok.iter().map(|s| s.strokes.1).sum();
        writeln!(
            f,
            "{} converted, {} failed; {strokes} strokes ({samples} samples) recorded; {materials} material fills replaced; \
             import {:.0} s, save {:.0} s, output {:.0} MB; wall {:.0} s",
            ok.len(),
            self.rows.len() - ok.len(),
            sum(|s| s.import_seconds),
            sum(|s| s.save_seconds),
            sum(|s| s.bytes as f64) / 1048576.0,
            self.seconds
        )
    }
}

/// Fills of layers and content effects standing in for a Substance material.
fn replaced_materials(doc: &Document) -> usize {
    let is_replaced =
        |f: &materialcraft::paint::Fill| matches!(f.source, FillSource::Material { .. }) && f.original.is_some();
    let mut n = 0;
    for set in &doc.texture_sets {
        for (_, l) in LayerWalk::new(&set.layers) {
            n += usize::from(matches!(&l.kind, LayerKind::Fill(f) if is_replaced(f)));
            n += l
                .effects
                .iter()
                .filter(|e| matches!(&e.kind, EffectKind::Fill(f) if is_replaced(f)))
                .count();
        }
    }
    n
}
