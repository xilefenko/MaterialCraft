//! `mcsub analyze`: which Substance procedurals a project uses, where, with which parameter values, which of
//! them MaterialCraft can replace with its own generators, and how accurate those replacements measured
//! (median MAE from mcparity's history, read at run time). The JSON it prints is the input of the import
//! review: a `choices.json` for `mcsub convert --plan` has the same `procedurals[].name` keys.

mod uses;

pub use uses::{ProceduralUse, Role, collect};

use crate::project::SppProject;
use crate::tools::accuracy::{Accuracy, ReplacementAccuracy};
use crate::translate::replacements::{self, Replacement};
use materialcraft::json::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// At most this many distinct values are listed per parameter.
const MAX_VALUES: usize = 8;

/// One procedural (by graph name) over the whole project.
#[derive(Debug, Clone)]
pub struct ProceduralSummary {
    pub name: String,
    pub uses: Vec<ProceduralUse>,
    pub replacement: Option<&'static Replacement>,
    pub accuracy: Option<Accuracy>,
}

impl ProceduralSummary {
    /// Uses that change what Painter shows.
    pub fn visible_uses(&self) -> usize {
        self.uses.iter().filter(|u| u.visible).count()
    }

    /// True if it fills layer content as a Substance material somewhere (the review offers our materials).
    pub fn is_material(&self) -> bool {
        self.uses.iter().any(|u| u.material)
    }

    /// Uses per role, in role order.
    pub fn roles(&self) -> BTreeMap<Role, usize> {
        let mut roles = BTreeMap::new();
        for u in &self.uses {
            *roles.entry(u.role).or_default() += 1;
        }
        roles
    }

    /// Texture sets that use it, in first-use order.
    pub fn sets(&self) -> Vec<&str> {
        let mut sets: Vec<&str> = Vec::new();
        for u in &self.uses {
            if !sets.contains(&u.set.as_str()) {
                sets.push(&u.set);
            }
        }
        sets
    }

    /// Distinct values per parameter, in first-use order (at most `MAX_VALUES` each).
    pub fn params(&self) -> Vec<(String, Vec<String>)> {
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        for (k, v) in self.uses.iter().flat_map(|u| &u.params) {
            let i = match out.iter().position(|(name, _)| name == k) {
                Some(i) => i,
                None => {
                    out.push((k.clone(), Vec::new()));
                    out.len() - 1
                }
            };
            if let Some((_, values)) = out.get_mut(i)
                && values.len() < MAX_VALUES
                && !values.contains(v)
            {
                values.push(v.clone());
            }
        }
        out
    }
}

/// The analysis of one project.
#[derive(Debug, Clone)]
pub struct Analysis {
    pub project: String,
    /// Most-used first.
    pub procedurals: Vec<ProceduralSummary>,
}

impl Analysis {
    /// Reads `path`'s layer document; accuracy comes from `accuracy`.
    pub fn of_project(path: &Path, accuracy: &ReplacementAccuracy) -> Result<Analysis, String> {
        let project = SppProject::open(path).map_err(|e| e.to_string())?;
        let (document, _) = project.model().map_err(|e| e.to_string())?;
        Ok(Analysis::of_uses(
            &path.display().to_string(),
            collect(&document),
            accuracy,
        ))
    }

    pub fn of_uses(project: &str, uses: Vec<ProceduralUse>, accuracy: &ReplacementAccuracy) -> Analysis {
        let mut by_name: Vec<ProceduralSummary> = Vec::new();
        for u in uses {
            match by_name.iter_mut().find(|s| s.name == u.name) {
                Some(s) => s.uses.push(u),
                None => {
                    let replacement = replacements::find(&u.name);
                    by_name.push(ProceduralSummary {
                        name: u.name.clone(),
                        accuracy: replacement.and_then(|r| accuracy.of(r.generator)),
                        replacement,
                        uses: vec![u],
                    });
                }
            }
        }
        by_name.sort_by(|a, b| b.uses.len().cmp(&a.uses.len()).then_with(|| a.name.cmp(&b.name)));
        Analysis {
            project: project.to_string(),
            procedurals: by_name,
        }
    }

    /// The Substance materials filling layer content, most-used first.
    pub fn materials(&self) -> impl Iterator<Item = &ProceduralSummary> {
        self.procedurals.iter().filter(|p| p.is_material())
    }

    pub fn to_json(&self) -> Value {
        let procedurals = self.procedurals.iter().map(summary_json).collect();
        let replaceable = self.procedurals.iter().filter(|p| p.replacement.is_some()).count();
        Value::object()
            .with("project", self.project.as_str())
            .with("procedurals", Value::Array(procedurals))
            .with("replaceable", replaceable)
    }
}

fn summary_json(p: &ProceduralSummary) -> Value {
    let roles = p
        .roles()
        .into_iter()
        .map(|(r, n)| (r.name().to_string(), Value::from(n)))
        .collect();
    let params = p
        .params()
        .into_iter()
        .map(|(k, values)| (k, Value::Array(values.into_iter().map(Value::from).collect())))
        .collect();
    let mut inputs: Vec<(String, String)> = Vec::new();
    for i in p.uses.iter().flat_map(|u| &u.inputs) {
        if !inputs.contains(i) {
            inputs.push(i.clone());
        }
    }
    let inputs = inputs
        .into_iter()
        .map(|(k, v)| Value::object().with("input", k).with("from", v))
        .collect();
    let replacement = match p.replacement {
        Some(r) => Value::object()
            .with("generator", r.generator)
            .with("label", r.label)
            .with("note", r.note),
        None => Value::Null,
    };
    let accuracy = match &p.accuracy {
        Some(a) => Value::object()
            .with("median_mae", a.median_mae)
            .with("cases", a.cases)
            .with("run", a.run.as_str()),
        None => Value::Null,
    };
    Value::object()
        .with("name", p.name.as_str())
        .with("uses", p.uses.len())
        .with("visible_uses", p.visible_uses())
        .with("material", p.is_material())
        .with("roles", Value::Object(roles))
        .with("sets", Value::Array(p.sets().into_iter().map(Value::from).collect()))
        .with("params", Value::Object(params))
        .with("inputs", Value::Array(inputs))
        .with("replacement", replacement)
        .with("accuracy", accuracy)
}
