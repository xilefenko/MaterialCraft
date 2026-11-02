//! Suggesting our materials for a project's Substance material fills: the import review offers, per Painter material, the tray materials ranked by
//! [`Suggestion::score`], from
//!
//! - the name: the Painter material's name against our material's name and tags ([`names`]), and
//! - the look: Painter's saved result where the material shows ([`PainterLooks`]) against our material rendered
//!   small ([`TrayMaterials`]), per channel by mean and histogram ([`look`]).
//!
//! The score is `(NAME_WEIGHT × name + w × look) / (NAME_WEIGHT + w)` with `w = (1 − NAME_WEIGHT) ×` the
//! confidence of Painter's look ([`PainterLook::confidence`]); the name alone when there is nothing to compare
//! the look with. A picked material is a replacement with its own look, not a reproduction of Painter's:
//! the similarities say how far apart they are.
//!
//! ```text
//! .spp ──Analysis (uses, material flag)──┐
//!      ──PainterLooks (cache × masks)────┼──MaterialMatches::rank──> suggestions ──> ProceduralPlan.materials
//! library folders ──TrayMaterials (looks)┘
//! ```

pub mod look;
pub mod names;
mod painter_look;
mod planner;
pub mod report;
mod tray;

pub use look::{ChannelLook, Look, LookSimilarity};
pub use painter_look::{PainterLook, PainterLooks};
pub use planner::ProjectPlanner;
pub use tray::{LOOK_RENDER, TrayMaterial, TrayMaterials};

use crate::project::SppProject;
use crate::tools::analyze::{Analysis, ProceduralSummary};
use crate::translate::replacements::ProceduralPlan;
use materialcraft::json::Value;
use materialcraft::paint::Channel;
use std::path::Path;

/// Weight of the name in the score.
pub const NAME_WEIGHT: f32 = 0.6;

/// Suggestions listed per material.
pub const SUGGESTIONS: usize = 5;

/// One of our materials for one Painter material.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub reference: String,
    pub name: String,
    /// 0..1, what the ranking uses.
    pub score: f32,
    pub name_score: f32,
    /// How close the looks are, when both could be measured.
    pub look: Option<LookSimilarity>,
}

/// One Painter material of a project and every tray material ranked for it, best first.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialMatch {
    /// Painter's graph name.
    pub name: String,
    pub uses: usize,
    pub visible_uses: usize,
    pub sets: Vec<String>,
    /// Painter's saved result where the material shows, if it could be measured.
    pub painter: Option<PainterLook>,
    pub ranked: Vec<Suggestion>,
}

impl MaterialMatch {
    /// Ranks `tray` for Painter material `name`.
    pub fn rank(name: &str, painter: Option<&PainterLook>, tray: &TrayMaterials) -> Vec<Suggestion> {
        let mut ranked: Vec<Suggestion> = tray
            .materials()
            .iter()
            .map(|m| {
                let name_score = names::name_score(name, &m.name, &m.tags);
                let look = painter.and_then(|p| m.look.as_ref().and_then(|l| p.look.similarity(l)));
                // the look counts less where Painter's was sampled under masks we could not compute
                let look_weight = (1.0 - NAME_WEIGHT) * painter.map_or(0.0, PainterLook::confidence);
                let total = NAME_WEIGHT + look_weight;
                let score = match (&look, painter) {
                    (Some(l), _) => (NAME_WEIGHT * name_score + look_weight * l.overall) / total,
                    // Painter's look is known but ours is not: ours cannot score on it
                    (None, Some(_)) => NAME_WEIGHT * name_score / total,
                    (None, None) => name_score,
                };
                Suggestion {
                    reference: m.reference.clone(),
                    name: m.name.clone(),
                    score,
                    name_score,
                    look,
                }
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then(b.name_score.total_cmp(&a.name_score))
                .then(a.reference.cmp(&b.reference))
        });
        ranked
    }

    fn of(p: &ProceduralSummary, looks: &PainterLooks, tray: &TrayMaterials) -> MaterialMatch {
        let painter = looks.get(&p.name).cloned();
        MaterialMatch {
            name: p.name.clone(),
            uses: p.uses.iter().filter(|u| u.material).count(),
            visible_uses: p.uses.iter().filter(|u| u.material && u.visible).count(),
            sets: p.sets().into_iter().map(str::to_string).collect(),
            ranked: MaterialMatch::rank(&p.name, painter.as_ref(), tray),
            painter,
        }
    }

    /// The best suggestion.
    pub fn best(&self) -> Option<&Suggestion> {
        self.ranked.first()
    }

    /// The first [`SUGGESTIONS`].
    pub fn suggestions(&self) -> &[Suggestion] {
        &self.ranked[..self.ranked.len().min(SUGGESTIONS)]
    }
}

/// The material fills of one project with their suggestions.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MaterialMatches {
    pub materials: Vec<MaterialMatch>,
}

impl MaterialMatches {
    /// Ranks `tray` for every material of `analysis`, measuring Painter's looks in the project at `path`.
    pub fn measure(path: &Path, analysis: &Analysis, tray: &TrayMaterials) -> Result<MaterialMatches, String> {
        let names: Vec<String> = analysis.materials().map(|p| p.name.clone()).collect();
        if names.is_empty() {
            return Ok(MaterialMatches::default());
        }
        let looks = match tray.is_empty() {
            true => PainterLooks::default(),
            false => {
                let project = SppProject::open(path).map_err(|e| e.to_string())?;
                let (document, _) = project.model().map_err(|e| e.to_string())?;
                PainterLooks::measure(&project, &document, &names)
            }
        };
        Ok(MaterialMatches::from_looks(analysis, &looks, tray))
    }

    /// Ranks `tray` for every material of `analysis` with Painter's looks already measured.
    pub fn from_looks(analysis: &Analysis, looks: &PainterLooks, tray: &TrayMaterials) -> MaterialMatches {
        MaterialMatches {
            materials: analysis
                .materials()
                .map(|p| MaterialMatch::of(p, looks, tray))
                .collect(),
        }
    }

    /// `base` with every material not chosen there replaced by its best suggestion.
    pub fn best_plan(&self, base: ProceduralPlan) -> ProceduralPlan {
        self.materials
            .iter()
            .fold(base, |plan, m| match (plan.material(&m.name), m.best()) {
                (None, Some(best)) => plan.with_material(&m.name, Some(&best.reference)),
                _ => plan,
            })
    }

    /// The `materials` array of `mcsub analyze --library`; each entry's `"choice"` is `plan`'s (a plan file
    /// may be this array with the choices edited).
    pub fn to_json(&self, plan: &ProceduralPlan) -> Value {
        Value::Array(self.materials.iter().map(|m| match_json(m, plan)).collect())
    }
}

fn match_json(m: &MaterialMatch, plan: &ProceduralPlan) -> Value {
    let painter = match &m.painter {
        Some(p) => look_json(&p.look)
            .with("layers", p.layers)
            .with("open_masks", p.open_masks)
            .with("coverage", p.coverage),
        None => Value::Null,
    };
    let suggestions = m
        .suggestions()
        .iter()
        .map(|s| {
            let look = match &s.look {
                Some(l) => {
                    let channels = l
                        .channels
                        .iter()
                        .map(|(c, v)| (c.id().to_string(), Value::from(*v)))
                        .collect();
                    Value::object()
                        .with("overall", l.overall)
                        .with("channels", Value::Object(channels))
                }
                None => Value::Null,
            };
            Value::object()
                .with("reference", s.reference.as_str())
                .with("name", s.name.as_str())
                .with("score", s.score)
                .with("name_score", s.name_score)
                .with("look", look)
        })
        .collect();
    Value::object()
        .with("name", m.name.as_str())
        .with("uses", m.uses)
        .with("visible_uses", m.visible_uses)
        .with(
            "sets",
            Value::Array(m.sets.iter().map(|s| Value::from(s.as_str())).collect()),
        )
        .with("painter", painter)
        .with("suggestions", Value::Array(suggestions))
        .with("choice", plan.material(&m.name).unwrap_or("keep"))
}

/// Means of a look, by channel id (base colour as RGB, the others as one number).
fn look_json(look: &Look) -> Value {
    let mut v = Value::object();
    for c in &look.channels {
        let mean = match c.channel {
            Channel::BaseColor => Value::Array(c.mean.iter().map(|x| Value::from(*x)).collect()),
            _ => Value::from(c.mean[0]),
        };
        v.insert(c.channel.id(), mean);
    }
    v
}

#[cfg(test)]
mod tests;
