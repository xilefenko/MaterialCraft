//! [`BlockerSummary`]: an imported document's locked layers grouped by what blocks them, with the catalog's
//! support for each blocker. It reads only the locks the import left on the layers (their reason keys), so it
//! works on any saved `.mcpaint`, not just on a fresh import. The blockers panel and `mcsub blockers` show it.

use super::blockers::BlockerKey;
use super::catalog::{Support, support};
use super::layers::correction::CORRECTION_KEY;
use materialcraft::paint::{Document, Layer, LayerKind, TextureSet};
use std::collections::BTreeMap;

/// One locked layer.
#[derive(Debug, Clone, PartialEq)]
pub struct LockedLayer {
    /// Texture set id and name.
    pub set: String,
    pub set_name: String,
    /// Layer id, and its folder path and name (`Folder / Layer`).
    pub layer: String,
    pub path: String,
    pub visible: bool,
    /// Its blocker keys, distinct, in order.
    pub keys: Vec<String>,
}

/// One blocker over the document.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockerUse {
    /// `kind:name`.
    pub key: String,
    /// The text it was first met with.
    pub text: String,
    /// `None` when the key isn't a blocker key the catalog knows.
    pub support: Option<Support>,
    /// The locked layers it blocks (indexes into [`BlockerSummary::locked`]).
    pub layers: Vec<usize>,
    /// How many of them it blocks alone: supporting it would unlock them.
    pub only: usize,
}

impl BlockerUse {
    /// The parsed key, if it is one.
    pub fn parsed(&self) -> Option<BlockerKey> {
        BlockerKey::parse(&self.key)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BlockerSummary {
    pub locked: Vec<LockedLayer>,
    /// Most layers first, then by key.
    pub blockers: Vec<BlockerUse>,
}

impl BlockerSummary {
    /// The locks of every layer of `doc` (the correction layer left out: it is Painter's pixels, not a Painter layer).
    pub fn of(doc: &Document) -> BlockerSummary {
        let mut locked = Vec::new();
        for set in &doc.texture_sets {
            collect(&set.layers, &set.id, &set.name, "", &mut locked);
        }
        BlockerSummary::from_locked(locked)
    }

    /// The locks of one texture set's layers.
    pub fn of_set(set: &TextureSet) -> BlockerSummary {
        let mut locked = Vec::new();
        collect(&set.layers, &set.id, &set.name, "", &mut locked);
        BlockerSummary::from_locked(locked)
    }

    /// Groups `locked` by key.
    pub fn from_locked(locked: Vec<LockedLayer>) -> BlockerSummary {
        let mut by_key: BTreeMap<String, BlockerUse> = BTreeMap::new();
        for (i, l) in locked.iter().enumerate() {
            for key in &l.keys {
                let entry = by_key.entry(key.clone()).or_insert_with(|| BlockerUse {
                    key: key.clone(),
                    text: String::new(),
                    support: BlockerKey::parse(key).map(|k| support(&k)),
                    layers: Vec::new(),
                    only: 0,
                });
                entry.layers.push(i);
                if l.keys.len() == 1 {
                    entry.only += 1;
                }
            }
        }
        let mut blockers: Vec<BlockerUse> = by_key.into_values().collect();
        blockers.sort_by(|a, b| b.layers.len().cmp(&a.layers.len()).then(a.key.cmp(&b.key)));
        BlockerSummary { locked, blockers }
    }

    /// Fills in each blocker's text from the layers' lock reasons in `doc` (the first reason with its key).
    pub fn with_texts(mut self, doc: &Document) -> BlockerSummary {
        let mut texts: BTreeMap<&str, &str> = BTreeMap::new();
        for set in &doc.texture_sets {
            for (_, l) in set.walk_layers() {
                for r in l.lock.iter().flat_map(|k| &k.reasons) {
                    texts.entry(r.key.as_str()).or_insert(r.text.as_str());
                }
            }
        }
        for b in &mut self.blockers {
            if let Some(t) = texts.get(b.key.as_str()) {
                b.text = (*t).to_string();
            }
        }
        self
    }

    /// True when nothing is locked.
    pub fn is_empty(&self) -> bool {
        self.locked.is_empty()
    }
}

fn collect(layers: &[Layer], set: &str, set_name: &str, folder: &str, out: &mut Vec<LockedLayer>) {
    for l in layers {
        let path = if folder.is_empty() {
            l.name.clone()
        } else {
            format!("{folder} / {}", l.name)
        };
        if let Some(lock) = &l.lock {
            let keys: Vec<String> = lock
                .keys()
                .into_iter()
                .filter(|k| *k != CORRECTION_KEY)
                .map(str::to_string)
                .collect();
            if !keys.is_empty() {
                out.push(LockedLayer {
                    set: set.to_string(),
                    set_name: set_name.to_string(),
                    layer: l.id.clone(),
                    path: path.clone(),
                    visible: l.visible,
                    keys,
                });
            }
        }
        if let LayerKind::Group(children) = &l.kind {
            collect(children, set, set_name, &path, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use materialcraft::paint::{Fill, LayerLock, LockReason, PaintContent};

    fn locked(id: &str, keys: &[&str]) -> Layer {
        let mut l = Layer::new(id, id.to_uppercase(), LayerKind::Fill(Fill::uniform(vec![])));
        let reasons = keys
            .iter()
            .map(|k| LockReason::new(*k, format!("text of {k}")))
            .collect();
        l.lock = Some(LayerLock::new("test", reasons));
        l
    }

    #[test]
    fn locks_group_by_key_with_sole_blockers_counted() {
        let mut set = TextureSet::new("s", "Body", (4, 4));
        let mut correction = Layer::new("c", "Correction", LayerKind::Paint(PaintContent::default()));
        correction.lock = Some(LayerLock::new("test", vec![LockReason::new(CORRECTION_KEY, "pixels")]));
        let folder = Layer::new(
            "f",
            "Rust",
            LayerKind::Group(vec![locked("b", &["substance:mg_dirt", "strokes:brush"])]),
        );
        set.layers = vec![
            locked("a", &["substance:mg_dirt"]),
            folder,
            Layer::new("free", "Free", LayerKind::Fill(Fill::uniform(vec![]))),
            correction,
        ];
        let doc = Document {
            texture_sets: vec![set],
            ..Document::default()
        };
        let s = BlockerSummary::of(&doc).with_texts(&doc);
        assert_eq!(s.locked.len(), 2, "the correction and unlocked layers are left out");
        assert_eq!(s.locked[1].path, "Rust / B");
        assert_eq!(s.blockers[0].key, "substance:mg_dirt");
        assert_eq!((s.blockers[0].layers.len(), s.blockers[0].only), (2, 1));
        assert_eq!(s.blockers[0].text, "text of substance:mg_dirt");
        assert_eq!(s.blockers[1].support, Some(Support::Replayable));
        assert!(!s.is_empty());
    }
}
