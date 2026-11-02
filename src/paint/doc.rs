//! [`Document`]: a mesh with its texture sets, plus what an importer could not translate.

use super::bake_config::BakeLinks;
use super::embedded::EmbeddedAsset;
use super::height_normal::HeightFilter;
use super::import_origin::ImportOrigin;
use super::project_settings::ProjectSettings;
use super::report::ImportReport;
use super::saved_view::SavedView;
use super::texture_set::TextureSet;
use super::validate::{self, DocError};
use crate::mesh::Mesh;
use crate::shading::{ShaderInstance, ShaderSettings};

/// A file kept untouched from an import (e.g. `foreign/substance/document.bin`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignFile {
    /// Path inside the document, `/`-separated.
    pub path: String,
    pub bytes: Vec<u8>,
}

/// A paint document. Plain data: no I/O, no GUI state.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    pub mesh: Mesh,
    /// One per painted material slot, in display order.
    pub texture_sets: Vec<TextureSet>,
    /// How height becomes normal detail in the final normal map, for every texture set (see
    /// [`crate::paint::MaterialMaps`]). Importers set the filter of the program the work came from.
    pub height_filter: HeightFilter,
    /// Which parts of the texture sets' bake configurations are linked across sets.
    pub bake_links: BakeLinks,
    pub foreign: Vec<ForeignFile>,
    /// Library assets the document uses, embedded so it opens without the library (AS-21 project scope).
    pub assets: Vec<EmbeddedAsset>,
    /// Present if the document came from an import.
    pub import_report: Option<ImportReport>,
    /// The source file and options of the import, to import it again; present if an importer set it.
    pub import_origin: Option<ImportOrigin>,
    /// The cameras, exposure and environment it was last looked at with (display state; ADR 0008 mc2-vq15).
    pub view: Option<SavedView>,
    /// Shader instances, which texture set uses which, and the mesh's length unit (VS-03, VS-12). Changed only
    /// through the shader settings' own history, never the project timeline.
    pub shading: ShaderSettings,
    /// Normal map convention, tangent frame and fill sizing chosen when the project was made.
    pub project: ProjectSettings,
}

impl Document {
    pub fn new(mesh: Mesh) -> Document {
        Document { mesh, ..Document::default() }
    }

    pub fn texture_set(&self, id: &str) -> Option<&TextureSet> {
        self.texture_sets.iter().find(|s| s.id == id)
    }

    pub fn texture_set_mut(&mut self, id: &str) -> Option<&mut TextureSet> {
        self.texture_sets.iter_mut().find(|s| s.id == id)
    }

    /// The texture set painted on material slot `slot`.
    pub fn texture_set_for_slot(&self, slot: usize) -> Option<&TextureSet> {
        self.texture_sets.iter().find(|s| s.material_slot == slot)
    }

    /// The shader instance texture set `set` (id) is shaded with; `None` for the built-in defaults.
    pub fn shader_instance(&self, set: &str) -> Option<&ShaderInstance> {
        self.shading.instance_for(set)
    }

    pub fn foreign_file(&self, path: &str) -> Option<&ForeignFile> {
        self.foreign.iter().find(|f| f.path == path)
    }

    /// Bytes used by all layer, mask and baked images.
    pub fn image_memory_bytes(&self) -> usize {
        let layers: usize = self.texture_sets.iter().flat_map(|s| s.walk_layers()).flat_map(|(_, l)| l.images()).map(|(_, i)| i.memory_bytes()).sum();
        let baked: usize = self.texture_sets.iter().flat_map(|s| &s.baked).map(|b| b.image.memory_bytes()).sum();
        layers + baked
    }

    /// Checks the invariants the rest of Paint mode relies on; returns the first problem.
    /// See [`DocError`] for the list.
    pub fn validate(&self) -> Result<(), DocError> {
        match self.problems().into_iter().next() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Every problem [`Self::validate`] would report.
    pub fn problems(&self) -> Vec<DocError> {
        validate::problems(self)
    }
}
