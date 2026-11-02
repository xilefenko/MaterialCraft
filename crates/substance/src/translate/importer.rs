//! [`import_spp`]: the translate stage's façade. Opens a project, binds texture sets, builds each one and keeps
//! the untranslated parts; the report goes into [`Document::import_report`], the source and the options into
//! [`Document::import_origin`] (see [`super::origin`]).

use super::bake_settings::ProjectBaking;
use super::baked_maps::BakedMaps;
use super::cache_channels::CacheChannels;
use super::cameras::ProjectCameras;
use super::cancel::Cancel;
use super::error::ImportError;
use super::foreign::ForeignFiles;
use super::layers::replay::ReplayChoice;
use super::layers::strokes::{StrokeCount, report_unimported_strokes};
use super::layers::{BlendTable, LayerImport, ProjectImages, ProjectionTable};
use super::normal_conventions::NormalConventions;
use super::origin::{self, ImportOptions};
use super::replacements::ProceduralPlan;
use super::replacements::materials::MaterialCount;
use super::report::Reporter;
use super::set_builder::SetBuilder;
use super::set_layers::SetLayers;
use super::texture_sets::SetBinding;
use crate::decode::assbin::{AssbinGeometry, AssbinScene};
use crate::model::PainterDocument;
use crate::project::{SppProject, paths};
use materialcraft::material_fill::MaterialResolver;
use materialcraft::mesh::Mesh;
use materialcraft::paint::saved_view::SavedView;
use materialcraft::paint::{Document, Fidelity, embedded};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

/// Share of the progress bar spent on texture sets (the rest: mesh, document, foreign files).
const SETS_START: f32 = 0.1;
const SETS_END: f32 = 0.9;

/// Imports a `.spp` as a flattened document. `progress` gets a fraction 0..1 and a short message.
pub fn import_spp(path: &Path, progress: &mut dyn FnMut(f32, &str)) -> Result<Document, ImportError> {
    import_spp_cancellable(path, progress, Cancel::NEVER)
}

/// [`import_spp`] that stops with [`ImportError::Cancelled`] soon after `cancel` asks for it.
pub fn import_spp_cancellable(
    path: &Path,
    progress: &mut dyn FnMut(f32, &str),
    cancel: Cancel<'_>,
) -> Result<Document, ImportError> {
    cancel.check()?;
    Importer::open(path)?.run(progress, cancel)
}

/// [`import_spp_cancellable`] with the user's choice of procedural replacements (see [`ProceduralPlan`]).
pub fn import_spp_with(
    path: &Path,
    plan: &ProceduralPlan,
    progress: &mut dyn FnMut(f32, &str),
    cancel: Cancel<'_>,
) -> Result<Document, ImportError> {
    cancel.check()?;
    Importer::open(path)?.with_plan(plan.clone()).run(progress, cancel)
}

/// [`import_spp_with`] that also embeds the materials `plan` picks, read through `library` (the asset library
/// folders), so the document opens anywhere.
pub fn import_spp_planned(
    path: &Path,
    plan: &ProceduralPlan,
    library: Arc<dyn MaterialResolver>,
    progress: &mut dyn FnMut(f32, &str),
    cancel: Cancel<'_>,
) -> Result<Document, ImportError> {
    cancel.check()?;
    Importer::open(path)?
        .with_plan(plan.clone())
        .with_library(library)
        .run(progress, cancel)
}

/// One import in progress.
pub struct Importer {
    project: SppProject,
    /// Which Substance procedurals become MaterialCraft generators (none by default).
    plan: ProceduralPlan,
    /// Where the materials the plan picks are read from to embed them (not embedded without one).
    library: Option<Arc<dyn MaterialResolver>>,
    /// Whether painted masks are replayed (see [`ReplayChoice`] for the default).
    replay: ReplayChoice,
}

impl Importer {
    pub fn open(path: &Path) -> Result<Importer, ImportError> {
        Ok(Importer {
            project: SppProject::open(path)?,
            plan: ProceduralPlan::default(),
            library: None,
            replay: ReplayChoice::default(),
        })
    }

    /// Builder: the procedural replacements to make.
    pub fn with_plan(mut self, plan: ProceduralPlan) -> Importer {
        self.plan = plan;
        self
    }

    /// Builder: embed the picked materials, read through `library`.
    pub fn with_library(mut self, library: Arc<dyn MaterialResolver>) -> Importer {
        self.library = Some(library);
        self
    }

    /// The material fills the plan replaces and keeps, in the report; the picked materials embedded.
    fn materials(&self, model: Option<&PainterDocument>, doc: &mut Document, report: &mut Reporter) {
        let count = model.map(|m| MaterialCount::of(m, &self.plan)).unwrap_or_default();
        if count.total() > 0 {
            report.add("material fills", Fidelity::Approximated, count.describe());
        }
        let Some(library) = &self.library else { return };
        let embedded = embedded::embed_used(doc, |r| library.resolve(r).ok().map(String::into_bytes));
        for missing in embedded.missing {
            let note = "not found in the asset library: the material fill shows nothing until the material is there";
            report.add(format!("material \"{missing}\""), Fidelity::Approximated, note);
        }
    }

    /// Builder: whether painted masks are replayed.
    pub fn with_replay(mut self, replay: ReplayChoice) -> Importer {
        self.replay = replay;
        self
    }

    /// Builder: the plan and replay choice of `options` (e.g. a document's stored origin).
    pub fn with_options(self, options: ImportOptions) -> Importer {
        self.with_plan(options.plan).with_replay(options.replay)
    }

    /// The options this import runs with.
    pub fn options(&self) -> ImportOptions {
        ImportOptions {
            plan: self.plan.clone(),
            replay: self.replay,
        }
    }

    /// Builds the document; `cancel` is asked between stages, per texture set and per image.
    pub fn run(&self, progress: &mut dyn FnMut(f32, &str), cancel: Cancel<'_>) -> Result<Document, ImportError> {
        let mut report = Reporter::new(self.source_label());
        progress(0.0, "reading mesh");
        let scene = self.project.read(paths::SCENE)?;
        cancel.check()?;
        let decode = |part| move |source| ImportError::Decode { part, source };
        let (mut mesh, faces) = AssbinGeometry::parse_with_faces(&scene).map_err(decode("mesh"))?;
        let materials = AssbinScene::parse(&scene).map_err(decode("mesh"))?.materials;
        report.mesh(mesh.parts.len(), mesh.parts.iter().map(|p| p.triangles.len()).sum());
        cancel.check()?;
        progress(0.05, "reading layer document");
        let records = self.project.document_materials()?;
        let normals = NormalConventions::from_settings(self.project.document_settings().ok().as_ref());
        report.normals(&normals);
        let model = match self.project.model() {
            Ok((document, _)) => Some(document),
            Err(e) => {
                let note = format!("layer document could not be read ({e}); layer stacks stay flattened");
                report.add("layer document", Fidelity::Approximated, note);
                None
            }
        };
        cancel.check()?;
        let table = BlendTable::painter();
        let images = ProjectImages::new(&self.project).with_cancel(cancel);
        let import = LayerImport {
            table: &table,
            projections: &ProjectionTable::painter(),
            images: &images,
            plan: &self.plan,
            replay: self.replay,
            replay_cache: None,
        };
        let layers = SetLayers {
            import: &import,
            document: model.as_ref(),
            faces: &faces,
        };
        let cache = CacheChannels::new(&self.project);
        let baked = BakedMaps::new(&self.project);
        let bake_settings = ProjectBaking::read(&self.project, &scene);
        let (cameras, camera_notes) = ProjectCameras::read(&self.project, &scene);
        let binding = SetBinding::bind(&materials, &records, &cache.sets());
        let builder = SetBuilder {
            cache: &cache,
            baked: &baked,
            normals: &normals,
            layers: &layers,
            cancel,
        };
        let mut translated = BTreeSet::new();
        let mut sets = Vec::with_capacity(binding.sets.len());
        let slot_names = mesh.materials.clone();
        for (i, source) in binding.sets.iter().enumerate() {
            let at = SETS_START + (SETS_END - SETS_START) * i as f32 / binding.sets.len() as f32;
            cancel.check()?;
            progress(at, &format!("texture set {}", source.record.name));
            merge_slots(
                &mut mesh,
                source.slot,
                &source.merged_slots,
                &slot_names,
                &mut report,
                &source.record.name,
            );
            let mut set = builder.build(source, &mesh, &mut report, &mut translated)?;
            let (config, unmapped) = bake_settings.config(&source.record, set.resolution);
            set.bake = config;
            if !unmapped.is_empty() {
                report.set_problem(
                    &source.record.name,
                    Fidelity::Approximated,
                    format!("bake settings not taken over: {}", unmapped.join("; ")),
                );
            }
            sets.push(set);
        }
        for r in &binding.unused {
            report.unused_material(&r.name);
            let painter = r.scene_uid.and_then(|uid| model.as_ref()?.texture_set(uid));
            if let Some(set) = painter {
                report_unimported_strokes(&mut report, &r.name, &StrokeCount::of_set(set));
            }
        }
        cancel.check()?;
        progress(SETS_END, "keeping original files");
        let kept = ForeignFiles::new(&self.project).collect(&translated, cancel)?;
        report.foreign(&kept);
        let mut doc = Document::new(mesh);
        doc.texture_sets = sets;
        doc.height_filter = normals.height_filter;
        doc.foreign = kept.files;
        doc.view = cameras.view(&doc.mesh);
        report_cameras(&mut report, doc.view.as_ref(), camera_notes);
        self.materials(model.as_ref(), &mut doc, &mut report);
        for problem in doc.problems() {
            report.add("document check", Fidelity::Approximated, problem.to_string());
        }
        doc.import_report = Some(report.finish());
        doc.import_origin = Some(origin::origin(self.project.path(), &self.options()));
        progress(1.0, "imported");
        Ok(doc)
    }

    /// `substance-painter <version>: <file name>`.
    fn source_label(&self) -> String {
        let version = self
            .project
            .settings()
            .ok()
            .flatten()
            .and_then(|s| s.last_saved_with.or(s.created_with))
            .map_or("(version unknown)".to_string(), |v| v.to_string());
        let file = self
            .project
            .path()
            .file_name()
            .map_or(String::new(), |n| n.to_string_lossy().into_owned());
        format!("substance-painter {version}: {file}")
    }
}

/// What became of the project's cameras.
fn report_cameras(report: &mut Reporter, view: Option<&SavedView>, notes: Vec<String>) {
    let imported = view.map_or(0, |v| v.cameras.iter().filter(|c| c.original().is_some()).count());
    let mut note = match imported {
        0 => "none found".to_string(),
        n => format!(
            "{n} imported (the saved view opens as the default camera; depth of field and display settings not taken over)"
        ),
    };
    for n in notes {
        note.push_str("; ");
        note.push_str(&n);
    }
    report.add("cameras", Fidelity::Approximated, note);
}

/// Points mesh parts of collapsed slots at the set's slot, so they show the shared texture set.
fn merge_slots(mesh: &mut Mesh, slot: usize, merged: &[usize], names: &[String], report: &mut Reporter, set: &str) {
    for &m in merged {
        mesh.parts
            .iter_mut()
            .filter(|p| p.material == m)
            .for_each(|p| p.material = slot);
        let from = names.get(m).map_or("?", |s| s.as_str());
        report.add(
            format!("mesh material \"{from}\""),
            Fidelity::Exact,
            format!("shares texture set \"{set}\" (collapsed in Painter); its parts now use that set's slot"),
        );
    }
}
