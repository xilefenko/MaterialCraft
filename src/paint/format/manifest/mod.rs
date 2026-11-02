//! `document.json`: the JSON mapping of every model type. Writing never fails (the document was
//! validated first); reading checks types and enum values and reports the JSON path of a problem.
//!
//! - [`fields`]: typed access to JSON objects; exact `f32` numbers.
//! - [`extras`]: unknown fields kept across load → save.
//! - [`bake_config`] (with [`bake_enums`]): per-set bake configuration and the document's bake links.
//! - [`texture_set`], [`layer`], [`image_ref`], [`report`]: one file per model type; [`strokes`]: stroke record
//!   references of paint pixels.
//! - [`effect`] (with [`effect_content`], [`effect_mask`], [`effect_kinds`]), [`blend`], [`geometry_mask`]:
//!   effect stacks, blend entries and geometry masks of layers.
//! - [`view`]: the camera list, exposure and environment the document was last looked at with.
//! - [`shading`]: shader instances, the texture sets' instances and the mesh's length unit.
//! - [`project`]: the settings chosen when the project was made (normal map format, tangent frame, fill sizing).
//! - [`import_origin`]: the source file and options of an import, to import it again (since 0.18).

pub mod bake_config;
pub mod bake_enums;
pub mod blend;
mod camera_values;
pub mod effect;
pub mod effect_content;
pub mod effect_kinds;
pub mod effect_mask;
pub mod extras;
pub mod fields;
pub mod geometry_mask;
pub mod image_ref;
pub mod import_origin;
pub mod layer;
pub mod mesh_faces;
pub mod project;
pub mod projection;
pub mod report;
pub mod shading;
pub mod strokes;
pub mod texture_set;
pub mod view;

use super::paths::{self, ImagePlan};
use super::slot::ImageSlot;
use super::strokes::{StrokePlan, StrokeRef};
use super::version::Version;
use super::{FORMAT_VERSION, FormatError};
use crate::json::Value;
use crate::paint::height_normal::HeightFilter;
use crate::paint::{Document, PixelFormat};
use extras::Extras;
use fields::Obj;

/// Value of the `format` field.
pub const FORMAT_NAME: &str = "mcpaint";
/// `generator` of written files.
pub const GENERATOR: &str = concat!("MaterialCraft ", env!("CARGO_PKG_VERSION"));

/// What writing the manifest needs besides the document.
pub struct WriteCtx<'a> {
    pub plan: &'a ImagePlan<'a>,
    pub strokes: &'a StrokePlan<'a>,
    pub extras: &'a Extras,
}

/// An image the manifest refers to, to be loaded into `slot`.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageRef {
    pub slot: ImageSlot,
    pub path: String,
    pub format: PixelFormat,
    pub size: (u32, u32),
}

/// State collected while reading the manifest.
#[derive(Default)]
pub struct ReadCtx {
    pub extras: Extras,
    pub images: Vec<ImageRef>,
    pub strokes: Vec<StrokeRef>,
    /// Each texture set's `shader` (set id, instance id), collected for [`shading::read`].
    pub set_shaders: Vec<(String, String)>,
}

/// A read manifest: the document without mesh, image pixels, foreign bytes and report, plus
/// where to find them.
pub struct Manifest {
    pub version: Version,
    /// Texture sets with correctly sized, still empty images.
    pub doc: Document,
    pub images: Vec<ImageRef>,
    /// Stroke records to load.
    pub strokes: Vec<StrokeRef>,
    pub mesh: String,
    /// Source polygons of the mesh parts, to give the parts once the mesh is read (since 0.9).
    pub mesh_faces: Vec<(usize, crate::mesh::FaceGroups)>,
    pub foreign: Vec<String>,
    /// References of the embedded assets (since 0.6).
    pub assets: Vec<String>,
    pub import_report: Option<String>,
    pub extras: Extras,
}

/// The manifest of `doc`. `thumbnail` says whether a thumbnail entry is written.
pub fn write(doc: &Document, ctx: &WriteCtx, thumbnail: bool) -> Value {
    let mut sets: Value = doc.texture_sets.iter().enumerate().map(|(i, s)| texture_set::write(i, s, ctx)).collect();
    shading::assign_sets(&doc.shading, &mut sets);
    let foreign: Value = doc.foreign.iter().map(|f| f.path.as_str()).collect();
    let mut links = bake_config::write_links(&doc.bake_links);
    ctx.extras.inject("bake_links", &mut links);
    let mut v = Value::object()
        .with("format", FORMAT_NAME)
        .with("format_version", FORMAT_VERSION)
        .with("generator", GENERATOR)
        .with("mesh", paths::MESH)
        .with("height_filter", doc.height_filter.id())
        .with("bake_links", links)
        .with("texture_sets", sets)
        .with("foreign", foreign);
    if let Some(faces) = mesh_faces::write(&doc.mesh) {
        v.insert("mesh_faces", faces);
    }
    if !doc.assets.is_empty() {
        v.insert("assets", super::embedded::manifest(&doc.assets));
    }
    if let Some(view) = &doc.view {
        v.insert("view", view::write(view, ctx.extras));
    }
    shading::write(&doc.shading, &mut v, ctx.extras);
    project::write(&doc.project, &mut v, ctx.extras);
    import_origin::write(doc.import_origin.as_ref(), &mut v, ctx.extras);
    if doc.import_report.is_some() {
        v.insert("import_report", paths::IMPORT_REPORT);
    }
    if thumbnail {
        v.insert("thumbnail", paths::THUMBNAIL);
    }
    ctx.extras.inject("", &mut v);
    v
}

/// Reads a manifest. `format` and `format_version` are checked before anything else.
pub fn read(root: &Value) -> Result<Manifest, FormatError> {
    let mut o = Obj::new(root, "")?;
    if o.opt("format").and_then(Value::as_str) != Some(FORMAT_NAME) {
        return Err(FormatError::NotMcpaint);
    }
    let version = Version::check_readable(o.str("format_version")?)?;
    o.opt("generator");
    o.opt("thumbnail");
    let mut ctx = ReadCtx::default();
    let mut doc = Document { height_filter: height_filter(&mut o)?, ..Document::default() };
    doc.bake_links = bake_config::read_links(o.opt("bake_links"), o.path("bake_links"), &mut ctx)?;
    for (i, s) in o.array("texture_sets")?.iter().enumerate() {
        doc.texture_sets.push(texture_set::read(i, s, &mut ctx)?);
    }
    let foreign = o
        .array("foreign")?
        .iter()
        .map(|p| p.as_str().map(str::to_string))
        .collect::<Option<Vec<String>>>()
        .ok_or_else(|| o.error("foreign", "expected an array of paths"))?;
    let assets = super::embedded::references(o.array("assets")?, &o.path("assets"))?;
    let mesh_faces = mesh_faces::read(&mut o)?;
    doc.view = view::read(&mut o, &mut ctx);
    doc.shading = shading::read(&mut o, &mut ctx)?;
    doc.project = project::read(&mut o, &mut ctx)?;
    doc.import_origin = import_origin::read(&mut o, &mut ctx)?;
    let manifest = Manifest {
        version,
        mesh: o.str("mesh")?.to_string(),
        mesh_faces,
        import_report: o.opt_str("import_report")?.map(str::to_string),
        doc,
        images: Vec::new(),
        strokes: Vec::new(),
        foreign,
        assets,
        extras: Extras::default(),
    };
    ctx.extras.record("", &o);
    Ok(Manifest { images: ctx.images, strokes: ctx.strokes, extras: ctx.extras, ..manifest })
}

/// `height_filter`, optional since 0.2 (default `backward`).
fn height_filter(o: &mut Obj) -> Result<HeightFilter, FormatError> {
    match o.opt_str("height_filter")? {
        None => Ok(HeightFilter::default()),
        Some(id) => HeightFilter::from_id(id).ok_or_else(|| o.error("height_filter", format!("unknown height filter {id:?}"))),
    }
}
