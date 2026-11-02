//! Where things live inside a `.spp` file.

pub const PROJECT_SETTINGS: &str = "/projectsettings.ini";
pub const RESOURCE_TOC: &str = "/resources.toc";
pub const RESOURCES: &str = "/resources";
/// Per-resource metadata records; not resources themselves.
pub const RESOURCE_META: &str = "/resources/.alg_meta";
pub const SCENE: &str = "/paint/scene.assbin";
/// Painter's metadata record of the mesh (normalisation, imported file).
pub const SCENE_META: &str = "/paint/.alg_meta/scene.assbin";
pub const DOCUMENT: &str = "/paint/document.bin";
pub const TEXTURE_CACHE: &str = "/texture-cache";
