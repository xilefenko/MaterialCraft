//! [`ProceduralPlan`]: what the user chose per Substance procedural before an import: MaterialCraft's
//! replacement (editable, approximate) or Painter's own result (exact look, flattened). Read from the import
//! review or from a `choices.json`:
//!
//! ```json
//! {"default": "keep", "procedurals": {"mg_dirt": "replace", "mg_mask_editor": "keep"}}
//! ```
//!
//! `procedurals` may also be the `procedurals` array `mcsub analyze` prints, each entry with a `"choice"`.
//!
//! Substance material fills (roadmap step 27) are chosen apart: `"materials"` maps a Painter material's graph
//! name to a material of ours (a library reference such as `materials/iron-rusted.mca`) or to `"keep"`
//! (Painter's flattened result, also what every material not listed gets). It may also be the `materials`
//! array `mcsub analyze --library` prints, each entry with a `"choice"`:
//!
//! ```json
//! {"materials": {"rust_fine": "materials/iron-rusted.mca", "moss": "keep"}}
//! ```

use materialcraft::json::{self, Value};

/// One choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Painter's own result: the procedural stays as it is (not computable; Painter's flattened result shows).
    Keep,
    /// MaterialCraft's replacement, where one exists.
    Replace,
}

impl Choice {
    pub fn id(self) -> &'static str {
        match self {
            Choice::Keep => "keep",
            Choice::Replace => "replace",
        }
    }

    pub fn from_id(id: &str) -> Option<Choice> {
        match id {
            "keep" => Some(Choice::Keep),
            "replace" => Some(Choice::Replace),
            _ => None,
        }
    }
}

/// Choices by procedural (graph) name, with a default for the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProceduralPlan {
    pub default: Choice,
    pub choices: Vec<(String, Choice)>,
    /// Material fills replaced by a material of ours: Painter graph name → library reference. Every other
    /// material fill keeps Painter's result.
    pub materials: Vec<(String, String)>,
}

impl Default for ProceduralPlan {
    /// Keep everything: the import is what it was before replacements existed.
    fn default() -> ProceduralPlan {
        ProceduralPlan::all(Choice::Keep)
    }
}

impl ProceduralPlan {
    /// The same choice for every procedural.
    pub fn all(choice: Choice) -> ProceduralPlan {
        ProceduralPlan {
            default: choice,
            choices: Vec::new(),
            materials: Vec::new(),
        }
    }

    /// The choice for graph `name`.
    pub fn choice(&self, name: &str) -> Choice {
        self.choices
            .iter()
            .find(|(n, _)| n == name)
            .map_or(self.default, |(_, c)| *c)
    }

    /// Builder: sets the choice for `name`.
    pub fn with(mut self, name: &str, choice: Choice) -> ProceduralPlan {
        self.choices.retain(|(n, _)| n != name);
        self.choices.push((name.to_string(), choice));
        self
    }

    /// The material of ours replacing Painter material `name`, or `None` to keep Painter's result.
    pub fn material(&self, name: &str) -> Option<&str> {
        self.materials.iter().find(|(n, _)| n == name).map(|(_, r)| r.as_str())
    }

    /// Builder: Painter material `name` is replaced by our material `reference` (`None`: Painter's result).
    pub fn with_material(mut self, name: &str, reference: Option<&str>) -> ProceduralPlan {
        self.materials.retain(|(n, _)| n != name);
        if let Some(r) = reference {
            self.materials.push((name.to_string(), r.to_string()));
        }
        self
    }

    /// A short stable text for cache keys (`keep;mg_dirt=replace;material:rust_fine=materials/rust.mca`).
    pub fn key(&self) -> String {
        let mut parts: Vec<String> = self.choices.iter().map(|(n, c)| format!("{n}={}", c.id())).collect();
        parts.extend(self.materials.iter().map(|(n, r)| format!("material:{n}={r}")));
        parts.sort();
        parts.insert(0, self.default.id().to_string());
        parts.join(";")
    }

    pub fn parse(text: &str) -> Result<ProceduralPlan, String> {
        let v = json::parse(text).map_err(|e| format!("choices: {e}"))?;
        ProceduralPlan::from_json(&v)
    }

    /// The plan of a parsed `choices.json` (the form [`Self::to_json`] writes, or the `mcsub analyze` arrays).
    pub fn from_json(v: &Value) -> Result<ProceduralPlan, String> {
        let mut plan = ProceduralPlan::default();
        if let Some(d) = v.get("default") {
            plan.default = choice_of(d, "default")?;
        }
        match v.get("procedurals") {
            Some(Value::Object(entries)) => {
                for (name, c) in entries {
                    plan = plan.with(name, choice_of(c, name)?);
                }
            }
            Some(Value::Array(entries)) => {
                for e in entries {
                    let name = e["name"].as_str().ok_or("choices: an entry without \"name\"")?;
                    if let Some(c) = e.get("choice") {
                        plan = plan.with(name, choice_of(c, name)?);
                    }
                }
            }
            Some(_) => return Err("choices: \"procedurals\" must be an object or an array".into()),
            None => {}
        }
        match v.get("materials") {
            Some(Value::Object(entries)) => {
                for (name, c) in entries {
                    plan = plan.with_material(name, material_of(c, name)?);
                }
            }
            Some(Value::Array(entries)) => {
                for e in entries {
                    let name = e["name"].as_str().ok_or("choices: a material without \"name\"")?;
                    if let Some(c) = e.get("choice") {
                        plan = plan.with_material(name, material_of(c, name)?);
                    }
                }
            }
            Some(_) => return Err("choices: \"materials\" must be an object or an array".into()),
            None => {}
        }
        Ok(plan)
    }

    pub fn to_json(&self) -> Value {
        let choices = self
            .choices
            .iter()
            .map(|(n, c)| (n.clone(), Value::from(c.id())))
            .collect();
        let mut v = Value::object()
            .with("default", self.default.id())
            .with("procedurals", Value::Object(choices));
        if !self.materials.is_empty() {
            let materials = self
                .materials
                .iter()
                .map(|(n, r)| (n.clone(), Value::from(r.as_str())))
                .collect();
            v.insert("materials", Value::Object(materials));
        }
        v
    }
}

/// A material choice: `"keep"` (Painter's result) or a reference to a material of ours.
fn material_of<'v>(v: &'v Value, what: &str) -> Result<Option<&'v str>, String> {
    match v.as_str() {
        Some("keep") => Ok(None),
        Some(r) if !r.trim().is_empty() => Ok(Some(r)),
        _ => Err(format!(
            "choices: material {what}: expected \"keep\" or a material reference"
        )),
    }
}

fn choice_of(v: &Value, what: &str) -> Result<Choice, String> {
    v.as_str()
        .and_then(Choice::from_id)
        .ok_or_else(|| format!("choices: {what}: expected \"keep\" or \"replace\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_objects_and_analyze_arrays() {
        let plan = ProceduralPlan::parse(r#"{"default": "replace", "procedurals": {"mg_dirt": "keep"}}"#).unwrap();
        assert_eq!(plan.choice("mg_dirt"), Choice::Keep);
        assert_eq!(plan.choice("mg_curvature"), Choice::Replace);
        assert_eq!(plan.key(), "replace;mg_dirt=keep");
        let again = ProceduralPlan::parse(&plan.to_json().to_string()).unwrap();
        assert_eq!(again, plan);
        let array = r#"{"procedurals": [{"name": "mg_dirt", "choice": "replace"}, {"name": "blur"}]}"#;
        let plan = ProceduralPlan::parse(array).unwrap();
        assert_eq!(
            (plan.choice("mg_dirt"), plan.choice("blur")),
            (Choice::Replace, Choice::Keep)
        );
        assert!(ProceduralPlan::parse(r#"{"default": "maybe"}"#).is_err());
        assert!(ProceduralPlan::parse("[").is_err());
    }

    #[test]
    fn material_choices_parse_round_trip_and_key() {
        let text = r#"{"materials": {"rust_fine": "materials/iron-rusted.mca", "moss": "keep"}}"#;
        let plan = ProceduralPlan::parse(text).unwrap();
        assert_eq!(plan.material("rust_fine"), Some("materials/iron-rusted.mca"));
        assert_eq!((plan.material("moss"), plan.material("gold")), (None, None));
        assert_eq!(plan.key(), "keep;material:rust_fine=materials/iron-rusted.mca");
        assert_eq!(ProceduralPlan::parse(&plan.to_json().to_string()).unwrap(), plan);
        // the analyze array: entries without a choice keep Painter's result
        let array = r#"{"materials": [{"name": "moss", "choice": "materials/moss.mca"}, {"name": "gold"}]}"#;
        let plan = ProceduralPlan::parse(array).unwrap();
        assert_eq!(
            (plan.material("moss"), plan.material("gold")),
            (Some("materials/moss.mca"), None)
        );
        assert_eq!(plan.clone().with_material("moss", None), ProceduralPlan::default());
        assert!(ProceduralPlan::parse(r#"{"materials": {"moss": 3}}"#).is_err());
        // a plan without materials keeps its old key and JSON (conversion caches stay valid)
        assert_eq!(ProceduralPlan::default().key(), "keep");
        assert!(ProceduralPlan::default().to_json().get("materials").is_none());
    }
}
