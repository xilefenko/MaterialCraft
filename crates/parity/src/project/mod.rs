//! [`OpenedProject`]: one `.spp` opened for parity cases, with everything its sets share loaded once: the
//! mesh (in Painter's normalised space), the normalisation, the baking settings and the texture sets.

mod sets;

pub use sets::{SetCase, discover};

use crate::inputs::{HighCache, HighSource, Inputs};
use crate::settings::{BAKING_INI, HighPolyRequest, MaterialBaking, PainterBaking, Params};
use crate::subject::CaseError;
use materialcraft::bake::TextureSetRef;
use materialcraft::mesh::Mesh;
use materialcraft_substance::decode::assbin::Normalization;
use materialcraft_substance::decode::document::NormalMapSyscoord;
use materialcraft_substance::project::{SppProject, paths};
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub struct OpenedProject {
    pub path: PathBuf,
    pub spp: SppProject,
    /// The whole mesh (also the high mesh for "low as high").
    pub mesh: Rc<Mesh>,
    pub normalization: Option<Normalization>,
    pub baking: Option<PainterBaking>,
    /// Baked normal maps are stored DirectX (`normalMapSyscoord` = 1).
    pub directx_normals: bool,
    pub sets: Vec<SetCase>,
    /// What could not be read (the cases still run where possible).
    pub notes: Vec<String>,
    high_meshes: HighCache,
}

impl OpenedProject {
    pub fn open(path: &Path) -> Result<OpenedProject, String> {
        let spp = SppProject::open(path).map_err(|e| e.to_string())?;
        let scene = spp.read(paths::SCENE).map_err(|e| e.to_string())?;
        let (mesh, _) =
            materialcraft_substance::translate::mesh::parse_mesh(&scene).map_err(|e| format!("mesh: {e}"))?;
        let mut notes = Vec::new();
        let normalization = spp.mesh_normalization(&scene).unwrap_or_else(|e| {
            notes.push(format!("mesh normalisation unreadable: {e}"));
            None
        });
        let baking = match spp.read(BAKING_INI) {
            Ok(data) => PainterBaking::parse(&data)
                .map_err(|e| notes.push(format!("baking settings unreadable: {e}")))
                .ok(),
            Err(_) => {
                notes.push("project has no baking settings".into());
                None
            }
        };
        let directx_normals = spp
            .document_settings()
            .ok()
            .and_then(|s| s.normal_map_syscoord)
            .is_some_and(|s| s == NormalMapSyscoord::DirectX);
        let sets = discover(&spp, &scene)?;
        Ok(OpenedProject {
            path: path.to_path_buf(),
            spp,
            mesh: Rc::new(mesh),
            normalization,
            baking,
            directx_normals,
            sets,
            notes,
            high_meshes: HighCache::default(),
        })
    }

    /// The project's file name, for tables.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map_or_else(|| self.path.display().to_string(), |n| n.to_string_lossy().into_owned())
    }

    /// Painter's baking settings for a set (its own, else the template).
    pub fn baking_of(&self, set: &SetCase) -> Option<&MaterialBaking> {
        self.baking.as_ref()?.for_material(set.material_uid)
    }

    /// Where the set's high mesh comes from, without loading it.
    pub fn high_source(&self, set: &SetCase) -> Result<HighSource, CaseError> {
        let baking = self
            .baking_of(set)
            .ok_or_else(|| CaseError::skipped("no baking settings, so the high-poly source is unknown"))?;
        let dir = self.path.parent().unwrap_or(Path::new("."));
        HighSource::resolve(&HighPolyRequest::of(baking), dir)
    }

    /// Everything a case of `set` needs; the high mesh is loaded here.
    pub fn inputs(&self, set: &SetCase, source: &HighSource) -> Result<Inputs, CaseError> {
        let triangles = self
            .mesh
            .parts
            .iter()
            .filter(|p| set.slots.contains(&p.material))
            .map(|p| p.triangles.len())
            .sum::<usize>();
        if triangles == 0 {
            return Err(CaseError::skipped("the set has no triangles"));
        }
        let high = self.high_meshes.get_or_load(source, &self.mesh, self.normalization)?;
        Ok(Inputs::new(
            Rc::clone(&self.mesh),
            TextureSetRef {
                materials: set.slots.clone(),
                uv_set: 0,
            },
            high,
        ))
    }

    /// Our parameters for a set compared at `size`².
    pub fn params(&self, set: &SetCase, size: usize) -> Params {
        Params::from_painter(self.baking_of(set), size)
    }
}
