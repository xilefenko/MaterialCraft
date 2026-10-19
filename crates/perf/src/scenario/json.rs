//! The JSON form of a [`Scenario`] (see the module docs of [`super`]): written and read with the core's json module;
//! unknown fields are errors.

use super::{Action, EditKind, LayerChoice, PathShape, Scenario, Source, StrokeSpec, StrokeTarget, StrokeTool, Symmetry};
use materialcraft::json::Value;
use std::path::PathBuf;

const AXES: [&str; 3] = ["x", "y", "z"];

fn axis_name(axis: usize) -> &'static str {
    AXES.get(axis).copied().unwrap_or("x")
}

fn axis_of(v: &Value) -> Result<usize, String> {
    let s = v.as_str().ok_or("a symmetry axis is \"x\", \"y\" or \"z\"")?;
    AXES.iter().position(|a| *a == s).ok_or_else(|| format!("unknown axis {s:?}"))
}

impl Symmetry {
    pub fn to_json(self) -> Value {
        match self {
            Symmetry::Off => "off".into(),
            Symmetry::Mirror { axis } => Value::object().with("mirror", axis_name(axis)),
            Symmetry::Radial { axis, count } => Value::object().with("radial", axis_name(axis)).with("count", count),
        }
    }

    pub fn from_json(v: &Value) -> Result<Symmetry, String> {
        if v.as_str() == Some("off") || v.is_null() {
            return Ok(Symmetry::Off);
        }
        if let Some(a) = v.get("mirror") {
            return Ok(Symmetry::Mirror { axis: axis_of(a)? });
        }
        if let Some(a) = v.get("radial") {
            let count = v.get("count").map_or(Some(4), |c| c.as_u64()).and_then(|c| u32::try_from(c).ok()).ok_or("radial count is a whole number")?;
            return Ok(Symmetry::Radial { axis: axis_of(a)?, count });
        }
        Err(format!("symmetry is \"off\", {{\"mirror\": axis}} or {{\"radial\": axis, \"count\": n}}, not {v}"))
    }
}

impl Scenario {
    pub fn to_json(&self) -> Value {
        let source = match &self.source {
            Source::NewProject { mesh, resolution } => Value::object().with("mesh", mesh.display().to_string()).with("resolution", *resolution),
            Source::Document(p) => Value::object().with("document", p.display().to_string()),
        };
        let action = match &self.action {
            Action::Open => Value::from("open"),
            Action::Stroke(s) => Value::object().with("stroke", s.to_json()),
            Action::Edits(list) => Value::object().with("edits", list.iter().map(|e| e.name()).collect::<Vec<_>>()),
        };
        Value::object()
            .with("id", self.id.as_str())
            .with("about", self.about.as_str())
            .with("source", source)
            .with("action", action)
            .with("repeat", self.repeat)
    }

    pub fn from_json(v: &Value) -> Result<Scenario, String> {
        known_keys(v, &["id", "about", "source", "action", "repeat"], "scenario")?;
        let id = v["id"].as_str().filter(|s| !s.is_empty()).ok_or("a scenario needs an \"id\"")?.to_string();
        let within = |e: String| format!("scenario {id}: {e}");
        let about = v["about"].as_str().unwrap_or_default().to_string();
        let source = source_from(&v["source"]).map_err(within)?;
        let action = action_from(&v["action"]).map_err(within)?;
        let repeat = match v.get("repeat") {
            None => 1,
            Some(r) => r.as_u64().and_then(|r| u32::try_from(r).ok()).filter(|r| *r > 0).ok_or_else(|| within("repeat is a positive whole number".into()))?,
        };
        Ok(Scenario { id, about, source, action, repeat })
    }
}

fn source_from(v: &Value) -> Result<Source, String> {
    if let Some(mesh) = v.get("mesh") {
        known_keys(v, &["mesh", "resolution"], "source")?;
        let mesh = mesh.as_str().ok_or("\"mesh\" is a path")?;
        let resolution = match v.get("resolution") {
            None => 2048,
            Some(r) => r.as_u64().and_then(|r| u32::try_from(r).ok()).filter(|r| r.is_power_of_two() && *r >= 16).ok_or("resolution is a power of two")?,
        };
        return Ok(Source::NewProject { mesh: PathBuf::from(mesh), resolution });
    }
    if let Some(doc) = v.get("document") {
        known_keys(v, &["document"], "source")?;
        return Ok(Source::Document(PathBuf::from(doc.as_str().ok_or("\"document\" is a path")?)));
    }
    Err("the source is {\"mesh\": path, \"resolution\": px} or {\"document\": path}".into())
}

fn action_from(v: &Value) -> Result<Action, String> {
    if v.as_str() == Some("open") {
        return Ok(Action::Open);
    }
    if let Some(s) = v.get("stroke") {
        return Ok(Action::Stroke(StrokeSpec::from_json(s)?));
    }
    if let Some(list) = v.get("edits") {
        let items = list.as_array().ok_or("\"edits\" is a list of names")?;
        let edits =
            items.iter().map(|e| e.as_str().ok_or_else(|| format!("{e} is not an edit name")).and_then(EditKind::from_name)).collect::<Result<Vec<_>, _>>()?;
        if edits.is_empty() {
            return Err("\"edits\" is empty".into());
        }
        return Ok(Action::Edits(edits));
    }
    Err("the action is \"open\", {\"stroke\": {..}} or {\"edits\": [..]}".into())
}

impl StrokeSpec {
    pub fn to_json(&self) -> Value {
        let mut v = Value::object().with("tool", self.tool.name());
        if let Some(p) = &self.preset {
            v.insert("preset", p.display().to_string());
        }
        if let Some(s) = self.size {
            v.insert("size", s);
        }
        if let Some(s) = self.spacing {
            v.insert("spacing", s);
        }
        if let Some(f) = self.frame_ms {
            v.insert("frame_ms", f);
        }
        if let Some(s) = &self.set {
            v.insert("set", s.as_str());
        }
        v.with("target", self.target.name())
            .with("layer", self.layer.name())
            .with("symmetry", self.symmetry.to_json())
            .with("path", self.path.name())
            .with("samples", self.samples)
            .with("per_frame", self.per_frame)
            .with("primed", self.primed)
            .with("undo_redo", self.undo_redo)
    }

    /// Missing fields keep their defaults; unknown ones are an error (a typo would otherwise be ignored).
    pub fn from_json(v: &Value) -> Result<StrokeSpec, String> {
        known_keys(
            v,
            &["tool", "preset", "size", "spacing", "target", "layer", "set", "symmetry", "path", "samples", "per_frame", "frame_ms", "primed", "undo_redo"],
            "stroke",
        )?;
        let mut s = StrokeSpec::default();
        let text = |key: &str| v.get(key).map(|x| x.as_str().ok_or_else(|| format!("\"{key}\" is a string")));
        if let Some(t) = text("tool") {
            s.tool = StrokeTool::from_name(t?)?;
        }
        if let Some(t) = text("preset") {
            s.preset = Some(PathBuf::from(t?));
        }
        if let Some(t) = text("target") {
            s.target = StrokeTarget::from_name(t?)?;
        }
        if let Some(t) = text("layer") {
            s.layer = LayerChoice::from_name(t?)?;
        }
        if let Some(t) = text("set") {
            s.set = Some(t?.to_string());
        }
        if let Some(t) = text("path") {
            s.path = PathShape::from_name(t?)?;
        }
        if let Some(x) = v.get("size") {
            s.size = Some(x.as_f64().filter(|x| *x > 0.0).ok_or("\"size\" is a positive number")? as f32);
        }
        if let Some(x) = v.get("spacing") {
            s.spacing = Some(x.as_f64().filter(|x| *x > 0.0).ok_or("\"spacing\" is a positive number")? as f32);
        }
        if let Some(x) = v.get("frame_ms") {
            s.frame_ms = Some(x.as_f64().filter(|x| *x > 0.0 && x.is_finite()).ok_or("\"frame_ms\" is a positive number")?);
        }
        if let Some(x) = v.get("symmetry") {
            s.symmetry = Symmetry::from_json(x)?;
        }
        let count = |key: &str, default: usize| -> Result<usize, String> {
            v.get(key).map_or(Ok(default), |x| x.as_usize().filter(|n| *n > 0).ok_or_else(|| format!("\"{key}\" is a positive whole number")))
        };
        s.samples = count("samples", s.samples)?.max(2);
        s.per_frame = count("per_frame", s.per_frame)?;
        let flag = |key: &str| v.get(key).map_or(Ok(false), |x| x.as_bool().ok_or_else(|| format!("\"{key}\" is true or false")));
        s.primed = flag("primed")?;
        s.undo_redo = flag("undo_redo")?;
        Ok(s)
    }
}

/// An error naming the first key of object `v` not in `known`.
fn known_keys(v: &Value, known: &[&str], what: &str) -> Result<(), String> {
    let entries = v.as_object().ok_or_else(|| format!("the {what} is an object"))?;
    match entries.iter().find(|(k, _)| !known.contains(&k.as_str())) {
        Some((k, _)) => Err(format!("unknown {what} field {k:?} (known: {})", known.join(", "))),
        None => Ok(()),
    }
}
