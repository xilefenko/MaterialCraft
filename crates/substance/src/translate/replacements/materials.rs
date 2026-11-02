//! Substance material fills replaced by a material of ours.
//!
//! Painter marks a fill source that is a whole material (several outputs feeding the channels) with `mode` 1.
//! The user picks one of our materials per Painter material in the import review ([`ProceduralPlan::material`]);
//! the fill then becomes a [`FillSource::Material`] naming it, laid on with the fill's projection and keeping
//! the Painter procedural as its [`Fill::original`]. Nothing of the Substance material is evaluated: ours is a
//! replacement with its own look, not a reproduction.

use super::ProceduralPlan;
use crate::model::{PainterDocument, PainterEffect, PainterSource, ProceduralSource};
use crate::translate::layers::effect_data;
use materialcraft::paint::{Fill, FillSource};
use materialcraft::projection::Projection;

/// `DataSourceProcedural.mode` of a material (see [`ProceduralSource::mode`]).
pub const MATERIAL_MODE: u64 = 1;

/// True if the fill source is a Substance material.
pub fn is_material(source: &ProceduralSource) -> bool {
    source.mode == MATERIAL_MODE && source.procedural.is_some()
}

/// The reference of our material replacing `source` under `plan`, if it is a material and one was picked.
pub fn chosen<'p>(source: &ProceduralSource, plan: &'p ProceduralPlan) -> Option<&'p str> {
    let p = source.procedural.as_ref().filter(|_| is_material(source))?;
    plan.material(p.name())
}

/// Our material fill for `source` (laid on by `projection`), standing in for the Painter procedural.
pub fn material_fill(source: &ProceduralSource, reference: &str, projection: Projection) -> Fill {
    let original = effect_data::procedural(source.procedural.as_ref());
    Fill::new(FillSource::Material {
        path: reference.to_string(),
        params: Vec::new(),
    })
    .with_projection(projection)
    .replacing(original)
}

/// How many material fills of a document `plan` replaces and keeps (every layer, hidden ones too).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaterialCount {
    /// Replaced fills, and the distinct Painter materials they use.
    pub replaced: usize,
    pub replaced_names: Vec<String>,
    /// Fills keeping Painter's result, and their distinct materials.
    pub kept: usize,
    pub kept_names: Vec<String>,
}

impl MaterialCount {
    pub fn of(document: &PainterDocument, plan: &ProceduralPlan) -> MaterialCount {
        let mut count = MaterialCount::default();
        for set in &document.texture_sets {
            for top in &set.layers {
                for l in top.walk() {
                    count.effects(&l.content, plan);
                }
            }
        }
        count
    }

    fn effects(&mut self, effects: &[PainterEffect], plan: &ProceduralPlan) {
        for e in effects {
            match e {
                PainterEffect::Fill(f) => {
                    for s in &f.sources {
                        if let PainterSource::Procedural(p) = s
                            && let Some(procedural) = p.procedural.as_ref().filter(|_| is_material(p))
                        {
                            let name = procedural.name().to_string();
                            let (n, names) = match chosen(p, plan) {
                                Some(_) => (&mut self.replaced, &mut self.replaced_names),
                                None => (&mut self.kept, &mut self.kept_names),
                            };
                            *n += 1;
                            if !names.contains(&name) {
                                names.push(name);
                            }
                        }
                    }
                }
                PainterEffect::Group(g) => self.effects(&g.effects, plan),
                _ => {}
            }
        }
    }

    pub fn total(&self) -> usize {
        self.replaced + self.kept
    }

    /// One line for the import report.
    pub fn describe(&self) -> String {
        let list = |names: &[String]| names.join(", ");
        let mut parts = Vec::new();
        if self.replaced > 0 {
            parts.push(format!(
                "{} replaced by MaterialCraft materials ({})",
                self.replaced,
                list(&self.replaced_names)
            ));
        }
        if self.kept > 0 {
            parts.push(format!(
                "{} kept as Painter's result ({})",
                self.kept,
                list(&self.kept_names)
            ));
        }
        format!("{} Substance material fills: {}", self.total(), parts.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ChannelMask, Procedural};

    fn source(name: &str, mode: u64) -> ProceduralSource {
        ProceduralSource {
            uid: None,
            channels: ChannelMask::default(),
            mode,
            procedural: Some(Procedural {
                uid: None,
                url: format!("resource://shelf/{name}/{name}?version=1.sbsar"),
                tweaks: vec![],
                outputs: vec![],
                inputs: vec![],
            }),
        }
    }

    #[test]
    fn only_picked_materials_become_material_fills() {
        let plan = ProceduralPlan::default().with_material("rust_fine", Some("materials/iron-rusted.mca"));
        assert_eq!(
            chosen(&source("rust_fine", MATERIAL_MODE), &plan),
            Some("materials/iron-rusted.mca")
        );
        assert_eq!(
            chosen(&source("rust_fine", 0), &plan),
            None,
            "a single-output texture is no material"
        );
        assert_eq!(
            chosen(&source("moss", MATERIAL_MODE), &plan),
            None,
            "not picked: Painter's result"
        );
        let fill = material_fill(
            &source("rust_fine", MATERIAL_MODE),
            "materials/iron-rusted.mca",
            Projection::default(),
        );
        assert_eq!(
            fill.source,
            FillSource::Material {
                path: "materials/iron-rusted.mca".into(),
                params: vec![]
            }
        );
        assert_eq!(fill.original.map(|o| o.id), Some("substance:rust_fine".to_string()));
    }
}
