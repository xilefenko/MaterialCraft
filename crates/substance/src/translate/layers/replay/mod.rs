//! Replaying Painter's brush strokes at import (roadmap step 32): painted masks and the brush work of paint layers
//! become real paint pixels, made by our brush pipeline (`materialcraft_brush`) from the stroke records the
//! import keeps anyway, with Painter's brush mapped by [`brush_map`] and its alpha by [`tip`].
//!
//! [`StrokeReplay`] serves one texture set: it builds the set's [`scene::SetScene`] on first use (3D-view strokes
//! go through their stored camera, 2D-view strokes through UV space) and keeps every replayed effect, since the
//! stack import walks the layers more than once (anchor passes). Painter's random sequence differs from ours, so
//! jittered strokes come out close, not identical; a replay's status is "replaced" (computable and editable,
//! measured but not exact) and its accuracy is `mcparity brush.replay`.
//!
//! - [`effect`]: one paint effect's strokes into target images.
//! - [`content`]: the values a paint layer's brush work paints per channel.
//! - [`jobs`]: every replay a set's import will ask for, painted up front in parallel ([`StrokeReplay::prefetch`]).

pub mod brush_map;
pub mod content;
pub mod effect;
pub mod jobs;
pub mod scene;
pub mod tip;

use super::channels::SetChannels;
use super::computability::Computability;
use super::decal::geometry::SetGeometry;
use super::images::ImageSource;
use crate::model::{PaintEffect, PaintTool, PainterSource, PainterTextureSet};
use crate::translate::blockers::{Blocker, BlockerKey, BlockerKind};
use jobs::ReplayJobs;
use materialcraft::paint::{Channel, PixelFormat, PixelLayout, SampleType, TiledImage};
use materialcraft::stamp::{ChannelTarget, StampMode};
use scene::SetScene;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

/// Most threads one set's replays use at once (several projects may be imported side by side).
const MAX_WORKERS: usize = 4;

/// Stored format of replayed mask paint (16 bits, so many soft strokes build up without banding).
const MASK_FORMAT: PixelFormat = PixelFormat::new(PixelLayout::GrayAlpha, SampleType::U16);

/// The words every replay reason starts with; `mcparity brush.replay` finds its cases by them.
pub const REPLAYED: &str = "strokes replayed";

/// Whether the import replays strokes. Opt-in: on the user's projects the replay is not yet shown to match
/// Painter (one measurable case, far off; see the step-32 report), and it multiplies import times.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplayChoice {
    /// Painted masks and paint layers become real paint.
    Replay,
    /// They stay empty with their stroke records; Painter's flattened result shows them (the default).
    #[default]
    Keep,
}

impl ReplayChoice {
    /// The word `mcsub --strokes` and a document's import origin use.
    pub fn id(self) -> &'static str {
        match self {
            ReplayChoice::Replay => "replay",
            ReplayChoice::Keep => "keep",
        }
    }

    pub fn from_id(id: &str) -> Option<ReplayChoice> {
        match id {
            "replay" => Some(ReplayChoice::Replay),
            "keep" => Some(ReplayChoice::Keep),
            _ => None,
        }
    }
}

/// Replayed paint: its pixels and what did not map.
#[derive(Clone)]
pub struct Replayed<T> {
    pub pixels: T,
    pub strokes: usize,
    /// Brush values without a mapping (reported, painted with our nearest).
    pub unknown: Vec<String>,
}

impl<T> Replayed<T> {
    /// The status the replayed part gets (`what`: "painted mask", "paint layer").
    pub fn status(&self, what: &str) -> Computability {
        let mut why = format!("{REPLAYED}: {what} ({} strokes)", self.strokes);
        if !self.unknown.is_empty() {
            why.push_str(&format!("; unmapped: {}", self.unknown.join(", ")));
        }
        Computability::replaced(why)
    }
}

/// What a set's replay did, for the import report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplayTally {
    /// Paint effects (masks) and brush-work groups (layers) replayed, and their strokes.
    pub replayed: usize,
    pub strokes: usize,
    /// Why the others were not, with how often.
    pub refused: Vec<(String, usize)>,
}

impl ReplayTally {
    fn refuse(&mut self, why: &str) {
        match self.refused.iter_mut().find(|(r, _)| r == why) {
            Some((_, n)) => *n += 1,
            None => self.refused.push((why.to_string(), 1)),
        }
    }
}

/// A paint layer's brush work replayed: one image per channel it paints (the channel's layer format).
pub type ContentImages = Vec<(Channel, TiledImage)>;

/// Which paint effects a cached replay is of: their uids (the same effects in any copy of the set's model), or
/// their addresses where an effect has no uid (valid during one import only).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    Uids(Vec<u64>),
    Addresses(Vec<usize>),
}

impl Key {
    fn of(effects: &[&PaintEffect]) -> Key {
        match effects.iter().map(|e| e.header.uid).collect::<Option<Vec<u64>>>() {
            Some(uids) => Key::Uids(uids),
            None => Key::Addresses(effects.iter().map(|e| *e as *const PaintEffect as usize).collect()),
        }
    }
}

/// Replays kept across imports of one texture set at one resolution on one mesh (tools that import a set many
/// times with parts swapped, `mcsub verify-layers`' isolation). Only use it for the same set, mesh and size.
#[derive(Default)]
pub struct ReplayCache {
    masks: Mutex<HashMap<Key, Result<Replayed<TiledImage>, Blocker>>>,
    contents: Mutex<HashMap<Key, Result<Replayed<ContentImages>, Blocker>>>,
}

/// Replays the paint effects of one texture set. See the module docs.
pub struct StrokeReplay<'a> {
    geometry: &'a SetGeometry<'a>,
    images: &'a dyn ImageSource,
    resolution: (u32, u32),
    scene: OnceLock<Option<SetScene>>,
    own: ReplayCache,
    shared: Option<&'a ReplayCache>,
}

impl<'a> StrokeReplay<'a> {
    /// A replay for one import; with `shared`, replays done before are reused and new ones kept there.
    pub fn new(
        geometry: &'a SetGeometry<'a>,
        images: &'a dyn ImageSource,
        resolution: (u32, u32),
        shared: Option<&'a ReplayCache>,
    ) -> StrokeReplay<'a> {
        StrokeReplay {
            geometry,
            images,
            resolution,
            scene: OnceLock::new(),
            own: ReplayCache::default(),
            shared,
        }
    }

    fn cache(&self) -> &ReplayCache {
        self.shared.unwrap_or(&self.own)
    }

    /// The pixels of a mask paint effect (grey × alpha), or why its strokes cannot be replayed.
    pub fn mask(&self, effect: &PaintEffect) -> Result<Replayed<TiledImage>, Blocker> {
        cached(&self.cache().masks, Key::of(&[effect]), || self.paint_mask(effect))
    }

    /// The pixels `effects` (a paint layer's brush work, in order) paint into the declared `channels`, or why they
    /// cannot be replayed.
    pub fn content(
        &self,
        effects: &[&PaintEffect],
        channels: &SetChannels,
    ) -> Result<Replayed<ContentImages>, Blocker> {
        cached(&self.cache().contents, Key::of(effects), || {
            self.paint_content(effects, channels)
        })
    }

    fn paint_mask(&self, effect: &PaintEffect) -> Result<Replayed<TiledImage>, Blocker> {
        let value = match effect.tool {
            PaintTool::Brush => mask_value(effect)?,
            // erasing a mask paints black over what the effect's strokes reach (PT-20)
            _ => 0.0,
        };
        effect::check(effect)?;
        let scene = self.scene()?;
        let (w, h) = scene.size();
        let mut image = TiledImage::new(w, h, MASK_FORMAT).map_err(|e| failed(e.to_string()))?;
        let mut targets = [ChannelTarget {
            image: &mut image,
            value: [value, value, value, 1.0],
            mode: StampMode::Paint,
            srgb: false,
            source: None,
        }];
        let (strokes, unknown) = effect::paint_strokes(effect, scene, self.images, &mut targets)?;
        Ok(Replayed {
            pixels: image,
            strokes,
            unknown,
        })
    }

    fn paint_content(
        &self,
        effects: &[&PaintEffect],
        channels: &SetChannels,
    ) -> Result<Replayed<ContentImages>, Blocker> {
        let plans = effects
            .iter()
            .map(|e| effect::check(e).and_then(|_| content::values(e, channels)))
            .collect::<Result<Vec<_>, _>>()?;
        let scene = self.scene()?;
        let (w, h) = scene.size();
        let mut images: ContentImages = Vec::new();
        for spec in channels.specs() {
            if plans.iter().any(|p| p.paints(spec.channel)) {
                images.push((
                    spec.channel,
                    TiledImage::new(w, h, spec.layer_format()).map_err(|e| failed(e.to_string()))?,
                ));
            }
        }
        let (mut strokes, mut unknown) = (0, Vec::new());
        for (effect, plan) in effects.iter().zip(&plans) {
            let mut targets: Vec<ChannelTarget> = images
                .iter_mut()
                .filter_map(|(c, image)| plan.target(*c, image))
                .collect();
            if targets.is_empty() {
                continue;
            }
            let (n, u) = effect::paint_strokes(effect, scene, self.images, &mut targets)?;
            strokes += n;
            unknown.extend(u.into_iter().filter(|x| !unknown.contains(x)).collect::<Vec<_>>());
        }
        Ok(Replayed {
            pixels: images,
            strokes,
            unknown,
        })
    }

    /// Paints every replay the import of `set` will ask for, several at once; the import then finds them done.
    /// The biggest jobs start first.
    pub fn prefetch(&self, set: &PainterTextureSet, channels: &SetChannels) {
        let jobs = ReplayJobs::of(set);
        let size = |effects: &[&PaintEffect]| {
            effects
                .iter()
                .map(|e| e.strokes.iter().map(|s| s.points.len()).sum::<usize>())
                .sum::<usize>()
        };
        let mut all: Vec<(usize, Option<&PaintEffect>, Vec<&PaintEffect>)> =
            jobs.masks.iter().map(|m| (size(&[*m]), Some(*m), Vec::new())).collect();
        all.extend(jobs.contents.into_iter().map(|c| (size(&c), None, c)));
        all.sort_by_key(|j| std::cmp::Reverse(j.0));
        let workers = std::thread::available_parallelism()
            .map_or(1, |n| n.get() / 2)
            .clamp(1, MAX_WORKERS)
            .min(all.len());
        let next = AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..workers {
                s.spawn(|| {
                    while let Some((_, mask, content)) = all.get(next.fetch_add(1, Ordering::Relaxed)) {
                        // the results stay in the caches; errors are reported where the import asks
                        let _ = match mask {
                            Some(m) => self.mask(m).map(|_| ()),
                            None => self.content(content, channels).map(|_| ()),
                        };
                    }
                });
            }
        });
    }

    /// What was replayed so far, and what was not with why.
    pub fn tally(&self) -> ReplayTally {
        let mut t = ReplayTally::default();
        let masks = self.cache().masks.lock().map(|m| {
            m.values()
                .map(|r| r.as_ref().map(|r| r.strokes).map_err(Clone::clone))
                .collect::<Vec<_>>()
        });
        let contents = self.cache().contents.lock().map(|m| {
            m.values()
                .map(|r| r.as_ref().map(|r| r.strokes).map_err(Clone::clone))
                .collect::<Vec<_>>()
        });
        for r in masks
            .unwrap_or_default()
            .into_iter()
            .chain(contents.unwrap_or_default())
        {
            match r {
                Ok(n) => {
                    t.replayed += 1;
                    t.strokes += n;
                }
                Err(why) => t.refuse(&why.text),
            }
        }
        t.refused.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        t
    }

    fn scene(&self) -> Result<&SetScene, Blocker> {
        self.scene
            .get_or_init(|| SetScene::build(self.geometry, self.resolution))
            .as_ref()
            .ok_or_else(|| refused("no-uvs", "the set's mesh has no usable UVs"))
    }
}

/// The cached result for `key`, made with `make` the first time.
fn cached<K: std::hash::Hash + Eq, T: Clone>(
    cache: &Mutex<HashMap<K, Result<T, Blocker>>>,
    key: K,
    make: impl FnOnce() -> Result<T, Blocker>,
) -> Result<T, Blocker> {
    if let Some(r) = cache.lock().ok().and_then(|d| d.get(&key).cloned()) {
        return r;
    }
    let result = make();
    if let Ok(mut d) = cache.lock() {
        d.insert(key, result.clone());
    }
    result
}

/// The grey value a brush effect paints into a mask (`sourceColor`'s uniform; 1 without one).
fn mask_value(effect: &PaintEffect) -> Result<f32, Blocker> {
    match effect.colors.first() {
        None => Ok(1.0),
        Some(PainterSource::Uniform(u)) => Ok((u.color[0] * u.opacity).clamp(0.0, 1.0)),
        Some(other) => Err(refused(
            &format!("mask-source-{}", other.kind()),
            format!("mask painted with a {} source", other.kind()),
        )),
    }
}

/// A brush replay refusal: `what` names it in the key (`brush:<what>`), `text` says it.
pub fn refused(what: &str, text: impl Into<String>) -> Blocker {
    Blocker::new(BlockerKey::new(BlockerKind::Brush, what), text)
}

/// A replay that failed on its way (an image that could not be made, a stroke the engine refused).
pub fn failed(text: impl Into<String>) -> Blocker {
    refused("replay-failed", text)
}
