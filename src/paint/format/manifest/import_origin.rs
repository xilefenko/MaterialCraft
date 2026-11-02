//! Import origin (since 0.18): the root's `import_origin`, written only when the document has one
//! ([`crate::paint::ImportOrigin`]).
//!
//! ```json
//! "import_origin": {
//!   "kind": "substance-painter", "importer": "materialcraft-substance 0.1.0", "path": "/projects/robot.spp",
//!   "size": 52428800, "modified_ms": 1730000000000, "options": {"plan": {"default": "keep", "procedurals": {}}}
//! }
//! ```
//!
//! `importer`, `size`, `modified_ms` and `options` are optional (`options` is any JSON value, kept as it is).
//! Unknown fields are kept at `import_origin`.

use super::extras::Extras;
use super::fields::Obj;
use super::{FormatError, ReadCtx};
use crate::json::Value;
use crate::paint::ImportOrigin;

const HERE: &str = "import_origin";

/// Adds `import_origin` to `root` if the document has one.
pub fn write(origin: Option<&ImportOrigin>, root: &mut Value, extras: &Extras) {
    let Some(o) = origin else { return };
    let mut v = Value::object().with("kind", o.kind.as_str());
    if !o.importer.is_empty() {
        v.insert("importer", o.importer.as_str());
    }
    v.insert("path", o.path.as_str());
    if let Some(size) = o.size {
        v.insert("size", size);
    }
    if let Some(ms) = o.modified_ms {
        v.insert("modified_ms", ms);
    }
    if !o.options.is_null() {
        v.insert("options", o.options.clone());
    }
    extras.inject(HERE, &mut v);
    root.insert(HERE, v);
}

/// Reads the root's `import_origin`; `None` when it is absent.
pub fn read(root: &mut Obj, ctx: &mut ReadCtx) -> Result<Option<ImportOrigin>, FormatError> {
    let Some(v) = root.opt(HERE) else { return Ok(None) };
    let mut o = Obj::new(v, root.path(HERE))?;
    let origin = ImportOrigin {
        kind: o.str("kind")?.to_string(),
        importer: o.opt_str("importer")?.unwrap_or_default().to_string(),
        path: o.str("path")?.to_string(),
        size: opt_u64(&mut o, "size")?,
        modified_ms: opt_u64(&mut o, "modified_ms")?,
        options: o.opt("options").cloned().unwrap_or_default(),
    };
    ctx.extras.record(HERE, &o);
    Ok(Some(origin))
}

fn opt_u64(o: &mut Obj, key: &str) -> Result<Option<u64>, FormatError> {
    match o.opt(key) {
        None => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| o.error(key, "expected a non-negative integer")),
    }
}
