//! `mcsub`: inspect and import Substance Painter projects.
//!
//!     mcsub inspect <file.spp> [--verify]          one project, all sections
//!     mcsub tree <file.spp>                        every group and dataset in the file
//!     mcsub doc <file.spp> [dataset] [--full]      document.bin (or another object file) as indented text
//!     mcsub model <file.spp>                       Painter's layer stacks as a tree, with the model report
//!     mcsub model --all <folder>                   build the model of every .spp below the folder
//!     mcsub corpus <folder> [--verify]             one table row per .spp below the folder
//!     mcsub analyze <file.spp> [--library <dir>]   Substance procedurals used, replacements, accuracy (JSON);
//!                                                  with a library also the material fills and our suggestions
//!     mcsub match-materials <file.spp | folder> --library <dir> [--jobs N]
//!                                                  suggestions per material fill and how close their looks are
//!     mcsub library <file.spp> [<library dir>]     what the import review offers for the user's library; with a
//!                                                  folder every candidate written into it
//!     mcsub convert <in.spp> <out.mcpaint> [--plan choices.json | --procedurals keep|replace] [--strokes keep|replay]
//!                                                  import and save as .mcpaint
//!     mcsub convert --all <folder> <out dir> [--discard] [--jobs N]   convert every .spp below the folder
//!     mcsub reimport <in.mcpaint> <out.mcpaint> [--source <file.spp>] [--plan choices.json | --procedurals keep|replace]
//!                    [--strokes keep|replay] [--library <dir>]
//!                                                  import a converted document's .spp again, from the source and with
//!                                                  the options its import origin recorded (each flag overrides the
//!                                                  stored value; `--source` when the .spp moved), save it as <out>
//!                                                  and print what changed in the locks: layers newly unlocked, newly
//!                                                  locked and still locked (matched by layer id, which comes from
//!                                                  Painter's layer uid), and the locked layers per blocker key before
//!                                                  and after (`tools::lock_diff`)
//!     mcsub report <in>                            import and print the import report
//!     mcsub maps <in> <out dir>                    final maps per texture set as PNGs
//!     mcsub compare <in> <export dir> [--diff <dir>] [--height-normal <factor>]
//!                                                  compare with Painter's exported textures
//!     mcsub verify-layers <file.spp> [--fit] [--strokes keep|replay]
//!                                                  imported layer stacks against Painter's texture cache
//!     mcsub verify-layers --all <folder> [--fit] [--strokes keep|replay] [--jobs N]
//!                                                  the same for every .spp below the folder
//!     mcsub isolate <file.spp|folder|list.txt> [--measure] [--cases] [--joint] [--jobs N]
//!                                                  isolated effect cases per effect id (procedural fills and
//!                                                  filters measurable against the texture cache), see
//!                                                  `tools::verify_layers::isolate`; `--joint`: candidate masks
//!                                                  solved together with the other unknown masks across channels
//!
//! `convert`, `report`, `maps`, `compare` and `verify-layers` take `--plan <choices.json>` or
//! `--procedurals keep|replace`: which Substance procedurals are replaced by MaterialCraft's generators
//! (default: keep them all; `choices.json` as written by the import review, see `replacements::plan`).
//! They also take `--library <dir>` (repeatable; asset library folders, e.g. the starter library) to embed the
//! materials a plan picks for Substance material fills, and `--materials auto` to give every material fill the
//! plan does not choose its best-scored material from those folders (`keep`, the default, keeps Painter's result).
//!
//! The `--all` commands work on several projects at once: `--jobs N` at most (default a quarter of the cores),
//! and only as many as fit in 60 % of the free memory (a project needs about twice its file size). Their
//! output is the same as one project after another.
//! `<in>` is a `.spp` (imported on the fly) or an already converted `.mcpaint`. `--height-normal` multiplies
//! the height → normal scale (default 1 = Painter's; 0 leaves height out of the normal map).
//! `convert`, `maps` and `compare` print progress on stderr as `progress <0..1> <message>`.

use materialcraft::paint::{Document, format};
use materialcraft_substance::container::hdf5::EntryKind;
use materialcraft_substance::inspect::document_dump::LIST_PREVIEW;
use materialcraft_substance::inspect::{Corpus, CorpusTable, DocumentDump, InspectOptions, ProjectReport, TextReport};
use materialcraft_substance::project::{SppProject, paths};
use materialcraft_substance::tools::accuracy::ReplacementAccuracy;
use materialcraft_substance::tools::analyze::Analysis;
use materialcraft_substance::tools::material_match::report::MatchTable;
use materialcraft_substance::tools::material_match::{MaterialMatches, ProjectPlanner, TrayMaterials};
use materialcraft_substance::tools::model_text::ModelText;
use materialcraft_substance::tools::parallel::CorpusPool;
use materialcraft_substance::tools::reimport::{self, ReimportRequest};
use materialcraft_substance::tools::report_text::ReportText;
use materialcraft_substance::tools::verify_layers::isolate::IsolateOptions;
use materialcraft_substance::tools::verify_layers::isolate::corpus::{CASE_HEADER, census_files};
use materialcraft_substance::tools::verify_layers::isolate::joint_cases::JointOptions;
use materialcraft_substance::tools::verify_layers::isolate::joint_census::{JOINT_CASE_HEADER, joint_census_files};
use materialcraft_substance::tools::verify_layers::{VerifyOptions, verify_path};
use materialcraft_substance::tools::{compare, convert, library, maps, model_corpus};
use materialcraft_substance::translate::layers::replay::ReplayChoice;
use materialcraft_substance::translate::replacements::{Choice, ProceduralPlan};
use materialcraft_substance::translate::{Cancel, import_spp_with};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

const USAGE: &str = "usage: mcsub inspect|tree|doc|model|corpus|analyze|library|convert|reimport|report|maps|compare|verify-layers|isolate … (see the source header for details)";

/// Parsed command line: positional words, `--flag`s and `--diff <dir>`.
struct Args {
    words: Vec<String>,
    flags: Vec<String>,
    diff: Option<PathBuf>,
    height_normal: Option<f32>,
    jobs: Option<usize>,
    /// `--plan <choices.json>`.
    plan: Option<PathBuf>,
    /// `--procedurals keep|replace`.
    procedurals: Option<String>,
    /// `--library <folder>` (repeatable): asset library folders whose materials replace material fills.
    library: Vec<PathBuf>,
    /// `--materials keep|auto`.
    materials: Option<String>,
    /// `--strokes keep|replay`.
    strokes: Option<String>,
    /// `--source <file.spp>` (reimport).
    source: Option<PathBuf>,
}

impl Args {
    fn parse(raw: Vec<String>) -> Args {
        let mut args = Args {
            words: Vec::new(),
            flags: Vec::new(),
            diff: None,
            height_normal: None,
            jobs: None,
            plan: None,
            procedurals: None,
            library: Vec::new(),
            materials: None,
            strokes: None,
            source: None,
        };
        let mut it = raw.into_iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--diff" => args.diff = it.next().map(PathBuf::from),
                "--height-normal" => args.height_normal = it.next().and_then(|v| v.parse().ok()),
                "--jobs" => args.jobs = it.next().and_then(|v| v.parse().ok()),
                "--plan" => args.plan = it.next().map(PathBuf::from),
                "--procedurals" => args.procedurals = it.next(),
                "--library" => args.library.extend(it.next().map(PathBuf::from)),
                "--materials" => args.materials = it.next(),
                "--strokes" => args.strokes = it.next(),
                "--source" => args.source = it.next().map(PathBuf::from),
                f if f.starts_with("--") => args.flags.push(a),
                _ => args.words.push(a),
            }
        }
        args
    }

    /// The pool for the `--all` commands.
    fn pool(&self) -> CorpusPool {
        CorpusPool::new(self.jobs.unwrap_or_else(CorpusPool::default_jobs).clamp(1, 64))
    }

    fn has(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }

    /// The procedural replacements asked for: `--plan <choices.json>`, else `--procedurals keep|replace`,
    /// else keep everything.
    fn plan(&self) -> Result<ProceduralPlan, String> {
        if let Some(path) = &self.plan {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            return ProceduralPlan::parse(&text);
        }
        match self.procedurals.as_deref() {
            None => Ok(ProceduralPlan::default()),
            Some(id) => Choice::from_id(id)
                .map(ProceduralPlan::all)
                .ok_or_else(|| format!("--procedurals {id}: expected keep or replace")),
        }
    }
}

impl Args {
    /// The plan per project: [`Args::plan`]; with `--library`, picked materials are embedded, and with
    /// `--materials auto` every material fill the plan does not choose gets its best-scored tray material.
    fn planner(&self) -> Result<ProjectPlanner, String> {
        let auto = match self.materials.as_deref() {
            None | Some("keep") => false,
            Some("auto") => true,
            Some(other) => return Err(format!("--materials {other}: expected keep or auto")),
        };
        let planner = ProjectPlanner::fixed(self.plan()?);
        match (self.library.is_empty(), auto) {
            (true, true) => Err("--materials auto needs --library <folder>".into()),
            (true, false) => Ok(planner),
            (false, _) => Ok(planner.with_tray(Arc::new(self.tray()), auto)),
        }
    }

    /// The materials of the `--library` folders.
    fn tray(&self) -> TrayMaterials {
        TrayMaterials::load(&self.library)
    }
    /// `--strokes keep|replay`: whether painted masks are replayed (default: [`ReplayChoice::default`]).
    fn replay(&self) -> Result<ReplayChoice, String> {
        match self.strokes.as_deref() {
            None => Ok(ReplayChoice::default()),
            Some("keep") => Ok(ReplayChoice::Keep),
            Some("replay") => Ok(ReplayChoice::Replay),
            Some(other) => Err(format!("--strokes {other}: expected keep or replay")),
        }
    }
}

fn progress_line(fraction: f32, message: &str) {
    eprintln!("progress {fraction:.3} {message}");
}

fn is_mcpaint(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mcpaint"))
}

/// Loads a `.mcpaint`; imports anything else as a `.spp`.
fn open_document(path: &Path, plan: &ProceduralPlan) -> Result<Document, String> {
    if is_mcpaint(path) {
        let doc = format::load_path(path).map_err(|e| e.to_string())?;
        for problem in materialcraft::paint::stroke::problems(&doc) {
            eprintln!("warning: {problem}");
        }
        Ok(doc)
    } else {
        import_spp_with(path, plan, &mut progress_line, Cancel::NEVER).map_err(|e| e.to_string())
    }
}

fn run(args: Args) -> Result<(), String> {
    let options = InspectOptions {
        verify_pixels: args.has("--verify"),
    };
    let words: Vec<&str> = args.words.iter().map(String::as_str).collect();
    let err = |e: &dyn std::fmt::Display| e.to_string();
    match words.as_slice() {
        ["inspect", path] => {
            let report = ProjectReport::build(Path::new(path), options).map_err(|e| err(&e))?;
            print!("{}", TextReport(&report));
        }
        ["library", path, rest @ ..] if rest.len() <= 1 => {
            print!("{}", library::run(Path::new(path), rest.first().map(Path::new))?);
        }
        ["tree", path] => {
            let project = SppProject::open(Path::new(path)).map_err(|e| err(&e))?;
            for e in project.layout().entries() {
                match e.kind {
                    EntryKind::Group => println!("{}/", e.path),
                    EntryKind::Dataset { len } => println!("{:<90} {len:>12}", e.path),
                }
            }
        }
        ["doc", path, rest @ ..] if rest.len() <= 1 => {
            let project = SppProject::open(Path::new(path)).map_err(|e| err(&e))?;
            let data = project
                .read(rest.first().copied().unwrap_or(paths::DOCUMENT))
                .map_err(|e| err(&e))?;
            let preview = if args.has("--full") { usize::MAX } else { LIST_PREVIEW };
            print!("{}", DocumentDump::render_with(&data, preview).map_err(|e| err(&e))?);
        }
        ["model", root] if args.has("--all") => {
            print!(
                "{}",
                model_corpus::model_all(Path::new(root), &mut |p| eprintln!("{}", p.display()))
            );
        }
        ["model", path] => {
            let project = SppProject::open(Path::new(path)).map_err(|e| err(&e))?;
            let (document, report) = project.model().map_err(|e| err(&e))?;
            print!(
                "{}",
                ModelText {
                    document: &document,
                    report: &report
                }
            );
        }
        ["corpus", root] => {
            let corpus = Corpus::discover(Path::new(root));
            let rows = corpus.run(options, |i, n, p| eprintln!("[{}/{n}] {}", i + 1, p.display()));
            let root = Path::new(root);
            print!("{}", CorpusTable { rows: &rows, root });
        }
        ["analyze", path] => {
            let analysis = Analysis::of_project(Path::new(path), &ReplacementAccuracy::load())?;
            let mut json = analysis.to_json();
            if !args.library.is_empty() {
                let planner = args.planner()?;
                let tray = planner.tray().cloned().unwrap_or_default();
                let matches = MaterialMatches::measure(Path::new(path), &analysis, &tray)?;
                let plan = match args.materials.as_deref() {
                    Some("auto") => matches.best_plan(planner.base().clone()),
                    _ => planner.base().clone(),
                };
                json.insert("materials", matches.to_json(&plan));
            }
            println!("{}", json.to_string_pretty());
        }
        ["match-materials", root] => {
            if args.library.is_empty() {
                return Err("match-materials needs --library <folder>".into());
            }
            let table = MatchTable::of_path(Path::new(root), &args.tray(), args.pool(), &|p| {
                eprintln!("{}", p.display())
            });
            print!("{table}");
        }
        ["convert", root, out] if args.has("--all") => {
            let table = convert::convert_all(
                Path::new(root),
                Path::new(out),
                &args.planner()?,
                args.has("--discard"),
                args.pool(),
                &|line| eprintln!("{line}"),
            );
            print!("{table}");
        }
        ["convert", input, out] => {
            let summary = convert::convert_one(
                Path::new(input),
                Path::new(out),
                &args.planner()?,
                args.replay()?,
                &mut progress_line,
            )
            .map_err(|e| err(&e))?;
            println!("{summary}");
        }
        ["reimport", input, out] => {
            let request = ReimportRequest {
                source: args.source.clone(),
                plan: (args.plan.is_some() || args.procedurals.is_some())
                    .then(|| args.plan())
                    .transpose()?,
                replay: args.strokes.is_some().then(|| args.replay()).transpose()?,
                library: args.library.clone(),
            };
            let summary = reimport::reimport(Path::new(input), Path::new(out), &request, &mut progress_line)?;
            print!("{summary}");
        }
        ["report", input] => {
            let doc = open_document(Path::new(input), &args.plan()?)?;
            if let Some(report) = &doc.import_report {
                print!("{}", ReportText(report));
            }
        }
        ["maps", input, dir] => {
            let doc = open_document(Path::new(input), &args.plan()?)?;
            let written = maps::write_maps(&doc, Path::new(dir), &mut |set| {
                progress_line(1.0, &format!("maps of {set}"))
            })
            .map_err(|e| err(&e))?;
            println!("{} maps written to {dir}", written.len());
        }
        ["compare", input, exports] => {
            let input = Path::new(input);
            let doc = open_document(input, &args.plan()?)?;
            let mut options = compare::CompareOptions::for_project(input, args.diff.clone());
            if is_mcpaint(input) {
                options.project_time = None;
            }
            if let Some(factor) = args.height_normal {
                options.height_normal_strength = factor;
            }
            let table = compare::compare_document(&doc, Path::new(exports), &options, &mut |m| progress_line(1.0, m))
                .map_err(|e| err(&e))?;
            print!("{table}");
        }
        ["verify-layers", path] => {
            let options = VerifyOptions {
                fit: args.has("--fit"),
                planner: args.planner()?,
                replay: args.replay()?,
            };
            let result = verify_path(Path::new(path), options, args.pool(), &|p| eprintln!("{}", p.display()));
            print!("{result}");
        }
        ["isolate", root] if args.has("--joint") => {
            let result = joint_census_files(
                &project_files(Path::new(root)),
                args.has("--measure"),
                &JointOptions::default(),
                args.pool(),
                &|p| eprintln!("{}", p.display()),
            );
            print!("{result}");
            if args.has("--cases") {
                println!("\n{JOINT_CASE_HEADER}");
                result.cases.iter().for_each(|line| println!("{line}"));
            }
        }
        ["isolate", root] => {
            let result = census_files(
                &project_files(Path::new(root)),
                args.has("--measure"),
                &IsolateOptions::default(),
                args.pool(),
                &|p| eprintln!("{}", p.display()),
            );
            print!("{result}");
            if args.has("--cases") {
                println!("\n{CASE_HEADER}");
                result.cases.iter().for_each(|line| println!("{line}"));
            }
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

/// A `.spp`, every `.spp` below a folder, or the paths listed one per line in a text file.
fn project_files(root: &Path) -> Vec<PathBuf> {
    match root.extension().and_then(|e| e.to_str()) {
        Some("txt") => std::fs::read_to_string(root)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(PathBuf::from)
            .collect(),
        _ if root.is_file() => vec![root.to_path_buf()],
        _ => Corpus::discover(root).files().to_vec(),
    }
}

fn main() -> ExitCode {
    match run(Args::parse(std::env::args().skip(1).collect())) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
