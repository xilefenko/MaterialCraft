//! [`SppImport`]: imports a Substance Painter project in-process through `materialcraft-substance`
//! (ADR 0003) and keeps the result in the conversion cache, so reopening an unchanged project is instant.
//!
//! The import asks the caller's [`CancelToken`] between stages, per texture set and per image, so Cancel
//! stops it within about one image decode; a cancelled import caches nothing.

use super::cache;
use super::job::CancelToken;
use materialcraft::material_fill::{FolderResolver, MaterialResolver};
use materialcraft::paint::Document;
use materialcraft::paint::format::save_path;
use materialcraft_substance::translate::layers::replay::ReplayChoice;
use materialcraft_substance::translate::replacements::ProceduralPlan;
use materialcraft_substance::translate::{Cancel, ImportError, Importer};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// Share of the progress bar for the import; the rest is saving to the cache.
const IMPORT_SHARE: f32 = 0.9;

/// The error text of a cancelled import.
pub const CANCELLED: &str = "Import cancelled.";

/// An imported (or cached) document and a one-line summary for the import report's tooltip.
pub struct Imported {
    pub document: Document,
    pub summary: String,
}

pub struct SppImport {
    /// The running editor, which contains the importer: part of the cache key, so a new build imports again.
    build: PathBuf,
    /// Where conversions are cached.
    cache_dir: PathBuf,
    /// Which Substance procedurals become MaterialCraft generators (part of the cache key).
    plan: ProceduralPlan,
    /// Asset library folders the materials the plan picks are embedded from.
    library: Vec<PathBuf>,
    /// Whether painted masks and paint layers are replayed (part of the cache key).
    replay: ReplayChoice,
}

impl SppImport {
    pub fn new() -> SppImport {
        SppImport::with_cache(cache::dir())
    }

    /// An importer caching its conversions in `cache_dir`.
    pub fn with_cache(cache_dir: PathBuf) -> SppImport {
        SppImport {
            build: std::env::current_exe().unwrap_or_default(),
            cache_dir,
            plan: ProceduralPlan::default(),
            library: Vec::new(),
            replay: ReplayChoice::default(),
        }
    }

    /// Builder: import with the procedural replacements of `plan`.
    pub fn with_plan(mut self, plan: ProceduralPlan) -> SppImport {
        self.plan = plan;
        self
    }

    /// Builder: replay brush strokes (or not).
    pub fn with_replay(mut self, replay: ReplayChoice) -> SppImport {
        self.replay = replay;
        self
    }

    /// Builder: embed the materials the plan picks from these asset library folders.
    pub fn with_library(mut self, folders: Vec<PathBuf>) -> SppImport {
        self.library = folders;
        self
    }

    /// Opens the cached conversion of `spp`, or imports it and caches the result. Fails with [`CANCELLED`]
    /// soon after `cancel` is set.
    pub fn open(&self, spp: &Path, cancel: &CancelToken, progress: &mut dyn FnMut(f32, &str)) -> Result<Imported, String> {
        let cached = with_replay_key(with_plan_key(cache::converted_path(&self.cache_dir, spp, &self.build), &self.plan), self.replay);
        if cached.is_file() {
            progress(IMPORT_SHARE, "Reading the cached conversion");
            let document = materialcraft::paint::format::load_path(&cached).map_err(|e| format!("Cannot read {}: {e}", cached.display()))?;
            return Ok(Imported { document, summary: "Opened the cached conversion.".into() });
        }
        let start = Instant::now();
        let asked = || cancel.is_cancelled();
        let mut on = |f: f32, m: &str| progress(f * IMPORT_SHARE, m);
        let imported = self.import(spp, &mut on, Cancel::new(&asked));
        let document = match imported {
            Ok(d) => d,
            Err(ImportError::Cancelled) => return Err(CANCELLED.into()),
            Err(e) => return Err(format!("Import failed: {e}")),
        };
        let imported = start.elapsed().as_secs_f64();
        progress(IMPORT_SHARE, "Saving to the conversion cache");
        // Saving writes a temporary file and renames it, so the cache never holds a partial conversion.
        let stored =
            std::fs::create_dir_all(&self.cache_dir).map_err(|e| e.to_string()).and_then(|()| save_path(&document, &cached).map_err(|e| e.to_string()));
        let summary = summary(&document, imported, stored.err());
        Ok(Imported { document, summary })
    }
}

impl SppImport {
    /// Runs the importer with the plan, the replay choice and (when the plan picks materials) the library.
    fn import(&self, spp: &Path, progress: &mut dyn FnMut(f32, &str), cancel: Cancel<'_>) -> Result<Document, ImportError> {
        cancel.check()?;
        let mut importer = Importer::open(spp)?.with_plan(self.plan.clone()).with_replay(self.replay);
        if !self.plan.materials.is_empty() {
            let library: Arc<dyn MaterialResolver> = Arc::new(FolderResolver::new(self.library.iter().cloned()));
            importer = importer.with_library(library);
        }
        importer.run(progress, cancel)
    }
}

/// The cache file of a conversion with replayed strokes: `-replay` added to the name (without replay the name stays,
/// so earlier conversions stay valid).
fn with_replay_key(path: PathBuf, replay: ReplayChoice) -> PathBuf {
    if replay != ReplayChoice::Replay {
        return path;
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!("{stem}-replay.mcpaint"))
}

/// The cache file of a conversion with replacements: the plan's choices hashed into the name (the default
/// plan keeps the plain name, so earlier conversions stay valid).
fn with_plan_key(path: PathBuf, plan: &ProceduralPlan) -> PathBuf {
    if *plan == ProceduralPlan::default() {
        return path;
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    plan.key().hash(&mut h);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!("{stem}-plan{:08x}.mcpaint", h.finish() as u32))
}

fn summary(doc: &Document, seconds: f64, cache_error: Option<String>) -> String {
    let channels: usize = doc.texture_sets.iter().map(|s| s.channels.len()).sum();
    let baked: usize = doc.texture_sets.iter().map(|s| s.baked.len()).sum();
    let mut line = format!("imported in {seconds:.1} s: {} texture sets, {channels} channels, {baked} baked maps", doc.texture_sets.len());
    if let Some(e) = cache_error {
        line.push_str(&format!("; not cached: {e}"));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn temp_cache(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mc-spp-import-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn cache_files(dir: &Path) -> usize {
        std::fs::read_dir(dir).map_or(0, |d| d.count())
    }

    #[test]
    fn replayed_conversions_are_cached_apart() {
        let plain = PathBuf::from("/cache/abc.mcpaint");
        assert_eq!(with_replay_key(plain.clone(), ReplayChoice::Keep), plain);
        assert_eq!(with_replay_key(plain, ReplayChoice::Replay), PathBuf::from("/cache/abc-replay.mcpaint"));
    }

    #[test]
    fn a_cancelled_import_stops_at_once_and_caches_nothing() {
        // Any file will do: the import asks before opening it.
        let spp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../substance/tests/fixtures/small.h5");
        let dir = temp_cache("pre");
        let token = CancelToken::default();
        token.cancel();
        let result = SppImport::with_cache(dir.clone()).open(&spp, &token, &mut |_, _| {});
        assert_eq!(result.err().as_deref(), Some(CANCELLED));
        assert_eq!(cache_files(&dir), 0);
    }

    /// Cancels a real import while its first texture set is being built. Runs only when
    /// `MC_SUBSTANCE_CORPUS` names a folder of `.spp` projects; uses the largest one up to 400 MB.
    #[test]
    fn cancel_stops_a_running_import_promptly() {
        let Some(root) = std::env::var_os("MC_SUBSTANCE_CORPUS") else { return };
        let files = materialcraft_substance::inspect::Corpus::discover(Path::new(&root)).files().to_vec();
        let size = |p: &PathBuf| std::fs::metadata(p).map_or(u64::MAX, |m| m.len());
        let Some(spp) = files.into_iter().filter(|p| size(p) <= 400 << 20).max_by_key(size) else { return };
        let dir = temp_cache("running");
        let token = CancelToken::default();
        let (tx, rx) = mpsc::channel();
        let worker = {
            let (token, dir, spp) = (token.clone(), dir.clone(), spp.clone());
            std::thread::spawn(move || {
                let result = SppImport::with_cache(dir).open(&spp, &token, &mut |_, m| {
                    let _ = tx.send(m.to_string());
                });
                result.err()
            })
        };
        while let Ok(m) = rx.recv() {
            if m.starts_with("texture set") {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(300));
        let cancelled_at = Instant::now();
        token.cancel();
        let error = worker.join().expect("import thread");
        let latency = cancelled_at.elapsed();
        eprintln!("{}: stopped {latency:?} after cancel", spp.display());
        assert_eq!(error.as_deref(), Some(CANCELLED), "{}", spp.display());
        assert!(latency < Duration::from_secs(5), "{}: {latency:?}", spp.display());
        assert_eq!(cache_files(&dir), 0);
    }
}
