//! [`StampStroke`]: one stroke's stamps applied to texture-space images by pulling texels into each dab's frame.
//!
//! Per dab: find the cells that may lie under it ([`TexelIndex`]), map each of their texels into the dab's frame
//! and sample the coverage there, scale it by the texel's visibility and the stroke's factors, and build up the
//! stroke's coverage with the dab's flow towards its opacity ceiling (`c ← c + v · (ceiling − c)`, the rule of
//! PhotoCraft's stroke buffer). Each changed tile of each target image is then rewritten from its pixels before the
//! stroke and the stroke coverage, so the opacity ceiling holds over the whole stroke (PT-04).
//!
//! The camera does not move during a stroke, so each texel's screen position and visibility are computed once per
//! stroke (lazily, per cell) and reused by every later dab. Tiles are processed in parallel; each texel's result
//! depends only on the dabs in order, so a stroke gives the same bits however it is split up.
//!
//! Clone strokes ([`StampMode::Clone`]) build coverage the same way and lay the image as it was before the stroke,
//! shifted by the clone offset, through it. Local strokes ([`StampMode::Local`]) instead run their kernels dab by
//! dab on the values under each dab (see `local`), still building coverage so [`StampStroke::touched`] holds.

use super::dab::DabSample;
use super::factors::TexelFactor;
use super::index::TexelIndex;
use super::local::{CellMap, LocalKernel, PickCache, delta, gather, sample};
use super::occlusion::DepthMap;
use super::plan::{DabPlan, Hit, Pull, T, TexelFrame, TileState, Visit, uv_frame, view_max_angle};
use super::write::{Blend, ChannelTarget, StampMode};
use crate::camera::Camera;
use crate::mesh::SurfaceMap;
use crate::paint::brush::Placement;
use crate::paint::{Rect, TiledImage};
use crate::projection::ViewProjector;
use crate::symmetry::{CopyTransform, SymmetrySpec};
use std::collections::BTreeMap;
use std::fmt;

/// The view a 3D-view stroke is painted through.
#[derive(Clone, Copy)]
pub struct ViewSpace<'a> {
    pub projector: &'a dyn ViewProjector,
    /// The view's depth for the occlusion test; `None` paints hidden surfaces too.
    pub depth: Option<&'a DepthMap>,
    pub placement: Placement,
    /// Surfaces turned away from the view by more than this many degrees are not painted (PT-09).
    pub max_angle: f32,
}

/// The frame dabs are made in.
#[derive(Clone, Copy)]
pub enum StrokeSpace<'a> {
    /// 2D-view strokes: dab frames are texel coordinates of the texture set.
    Texture,
    /// 3D-view strokes: dab frames are screen positions of the view.
    View(ViewSpace<'a>),
}

/// Why a stamp could not be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StampError {
    /// The targets differ from the ones the stroke started with (count or size).
    TargetsChanged,
    /// The surface map, its index and the images are not all the same size.
    SizeMismatch,
    /// A [`StampMode::Local`] target has no kernel ([`StampStroke::set_kernels`]).
    NoKernel,
}

impl fmt::Display for StampError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StampError::TargetsChanged => f.write_str("the images being painted changed during the stroke"),
            StampError::SizeMismatch => f.write_str("the surface map and the images being painted differ in size"),
            StampError::NoKernel => f.write_str("the brush mode has nothing to run on the paint"),
        }
    }
}

impl std::error::Error for StampError {}

/// What a stroke's stamps are applied through, borrowed per call so a stroke can live across frames: the surface
/// map and its index, the frame dabs are made in, and the factors.
#[derive(Clone, Copy)]
pub struct StampContext<'a> {
    pub surface: &'a SurfaceMap,
    pub index: &'a TexelIndex,
    pub space: StrokeSpace<'a>,
    pub factors: &'a [&'a dyn TexelFactor],
}

/// What the per-stroke view caches were computed with; a call with anything else drops them.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ViewKey {
    camera: Camera,
    max_angle: f32,
    occlusion: bool,
}

/// One stroke into a fixed set of target images. See the module docs.
pub struct StampStroke {
    size: (usize, usize),
    copies: Vec<CopyTransform>,
    /// The targets' pixels before the stroke (sharing their tiles) and how each blends.
    before: Vec<(TiledImage, Blend)>,
    tiles: BTreeMap<usize, TileState>,
    tiles_x: usize,
    view_key: Option<ViewKey>,
    /// Per target, the kernel of a [`StampMode::Local`] stroke.
    kernels: Vec<Option<Box<dyn LocalKernel>>>,
    /// Local strokes in the 3D view: frame pixel → texel.
    picks: PickCache,
}

impl StampStroke {
    /// A stroke into `targets` (all of one texture set's size) with the copies `symmetry` asks for. 3D-view dabs
    /// repeat on the mesh through the view; texture-space dabs repeat through the mesh (see `through_mesh`).
    pub fn new(symmetry: SymmetrySpec, targets: &[ChannelTarget]) -> Result<StampStroke, StampError> {
        StampStroke::with_copies(symmetry.copies(), targets)
    }

    /// A stroke into `targets` with the given symmetric copies (the direct copy among them; a recorded stroke's
    /// copies come from [`crate::symmetry::repeat`]). Without any copy the direct one is used.
    pub fn with_copies(mut copies: Vec<CopyTransform>, targets: &[ChannelTarget]) -> Result<StampStroke, StampError> {
        if copies.is_empty() {
            copies.push(CopyTransform::IDENTITY);
        }
        let size = targets.first().map_or((0, 0), |t| t.image.size());
        if targets.iter().any(|t| t.image.size() != size) {
            return Err(StampError::SizeMismatch);
        }
        let (w, h) = (size.0 as usize, size.1 as usize);
        Ok(StampStroke {
            size: (w, h),
            copies,
            before: targets.iter().map(|t| (t.image.clone(), Blend::new(t))).collect(),
            tiles: BTreeMap::new(),
            tiles_x: w.div_ceil(T),
            view_key: None,
            kernels: Vec::new(),
            picks: PickCache::default(),
        })
    }

    /// The kernels of a [`StampMode::Local`] stroke, one per target in the targets' order (`None` for targets of
    /// another mode).
    pub fn set_kernels(&mut self, kernels: Vec<Option<Box<dyn LocalKernel>>>) {
        self.kernels = kernels;
    }

    fn local(&self) -> bool {
        self.before.iter().any(|(_, b)| b.mode() == StampMode::Local)
    }

    /// Every image tile the stroke has changed so far, as `(tx, ty)`.
    pub fn touched(&self) -> Vec<(u32, u32)> {
        let painted = self.tiles.iter().filter(|(_, s)| s.acc.iter().any(|&c| c > 0.0));
        painted.map(|(&t, _)| ((t % self.tiles_x) as u32, (t / self.tiles_x) as u32)).collect()
    }

    /// The stroke's coverage at texel `(x, y)` so far.
    pub fn coverage(&self, x: usize, y: usize) -> f32 {
        let t = (y / T) * self.tiles_x + x / T;
        self.tiles.get(&t).and_then(|s| s.acc.get((y % T) * T + x % T)).copied().unwrap_or(0.0)
    }

    /// Applies one dab through `ctx` to `targets` (the stroke's targets, in the same order); returns the image
    /// tiles it changed.
    pub fn apply(&mut self, ctx: &StampContext, dab: &DabSample, targets: &mut [ChannelTarget]) -> Result<Vec<(u32, u32)>, StampError> {
        self.apply_all(ctx, std::slice::from_ref(dab), targets)
    }

    /// Applies `dabs` in order through `ctx` to `targets` (the stroke's targets, in the same order); returns the
    /// image tiles they changed, sorted. The same pixels as applying them one by one: each tile takes the dabs in
    /// order, and each changed tile of the targets is rewritten once from the coverage after the last dab (it
    /// depends only on the pixels before the stroke and the coverage). Local strokes read the targets under each
    /// dab, so they still go dab by dab.
    pub fn apply_all(&mut self, ctx: &StampContext, dabs: &[DabSample], targets: &mut [ChannelTarget]) -> Result<Vec<(u32, u32)>, StampError> {
        if targets.len() != self.before.len() || targets.iter().zip(&self.before).any(|(t, (b, _))| t.image.size() != b.size()) {
            return Err(StampError::TargetsChanged);
        }
        if (ctx.surface.width(), ctx.surface.height()) != self.size || !ctx.index.fits(ctx.surface) {
            return Err(StampError::SizeMismatch);
        }
        let local = self.local();
        if local && (self.kernels.len() != targets.len() || targets.iter().zip(&self.kernels).any(|(t, k)| t.mode == StampMode::Local && k.is_none())) {
            return Err(StampError::NoKernel);
        }
        if let StrokeSpace::View(view) = ctx.space {
            self.see_from(view);
        }
        if local {
            let mut changed = Vec::new();
            for dab in dabs {
                changed.extend(self.apply_local_dab(ctx, dab, targets));
            }
            changed.sort_unstable();
            changed.dedup();
            return Ok(changed);
        }
        let plans: Vec<DabPlan> = dabs.iter().filter_map(|d| self.plan(ctx, d, Visit::Build).map(|(p, _)| p)).collect();
        let changed = self.pull(ctx, &plans);
        let coords: Vec<(u32, u32)> = changed.iter().map(|&t| ((t % self.tiles_x) as u32, (t / self.tiles_x) as u32)).collect();
        self.write(&coords, targets);
        Ok(coords)
    }

    /// Drops the per-stroke view caches if `view` differs from the one they were computed with.
    fn see_from(&mut self, view: ViewSpace) {
        let key = ViewKey { camera: *view.projector.camera(), max_angle: view_max_angle(view), occlusion: view.depth.is_some() };
        if self.view_key != Some(key) {
            // another view than the cached texels were seen from: see them again
            self.tiles.values_mut().for_each(|t| t.views.clear());
            self.picks.clear();
            self.view_key = Some(key);
        }
    }

    /// What dabs are pulled through: `ctx` and the stroke's copies.
    fn pull_of<'a>(&'a self, ctx: &StampContext<'a>) -> Pull<'a> {
        Pull { surface: ctx.surface, index: ctx.index, factors: ctx.factors, copies: &self.copies, tiles_x: self.tiles_x }
    }

    /// Plans `dab` (`visit` given its opacity ceiling) with the cell map a local kernel reads it through; `None`
    /// if it paints nothing.
    fn plan<'a>(&self, ctx: &StampContext<'a>, dab: &'a DabSample, visit: fn(f32) -> Visit) -> Option<(DabPlan<'a>, CellMap<'a>)> {
        let ceiling = if dab.opacity.is_finite() { dab.opacity.clamp(0.0, 1.0) } else { 0.0 };
        if ceiling <= 0.0 || dab.footprint.values.is_empty() {
            return None;
        }
        let visit = visit(ceiling);
        let pull = self.pull_of(ctx);
        let limit = [self.size.0 as f32, self.size.1 as f32];
        let texel_map = |f: TexelFrame| CellMap::Texel { center: f.center, anchor: f.anchor, scale: f.scale };
        match ctx.space {
            StrokeSpace::Texture => {
                let frame = TexelFrame { center: dab.center, anchor: dab.center, scale: 1.0, limit };
                DabPlan::rect(&pull, dab, visit, frame).map(|p| (p, texel_map(frame)))
            }
            StrokeSpace::View(view) if view.placement == Placement::UvAligned => {
                let frame = uv_frame(ctx.surface, view.projector, dab)?;
                DabPlan::rect(&pull, dab, visit, frame).map(|p| (p, texel_map(frame)))
            }
            StrokeSpace::View(view) => DabPlan::view(&pull, view, dab, visit).map(|p| (p, CellMap::View { projector: view.projector })),
        }
    }

    /// Runs `plans` on the tiles they reach, each tile taking its dabs in order, tiles in parallel; returns the
    /// tiles that changed (sorted). Tile states are created as needed; ones left with nothing in them are dropped.
    fn pull(&mut self, ctx: &StampContext, plans: &[DabPlan]) -> Vec<usize> {
        let mut by_tile: BTreeMap<usize, Vec<&DabPlan>> = BTreeMap::new();
        for p in plans {
            for &t in p.tiles() {
                by_tile.entry(t).or_default().push(p);
            }
        }
        if by_tile.is_empty() {
            return Vec::new();
        }
        for &t in by_tile.keys() {
            self.tiles.entry(t).or_insert_with(TileState::new);
        }
        let pull = Pull { surface: ctx.surface, index: ctx.index, factors: ctx.factors, copies: &self.copies, tiles_x: self.tiles_x };
        let mut jobs: Vec<(usize, &mut TileState, bool)> =
            self.tiles.iter_mut().filter(|(k, _)| by_tile.contains_key(k)).map(|(k, s)| (*k, s, false)).collect();
        crate::paint::par::for_each_mut(&mut jobs, |_, (t, s, changed)| {
            for p in by_tile.get(t).into_iter().flatten() {
                *changed |= p.run(&pull, *t, s);
            }
        });
        let changed: Vec<usize> = jobs.iter().filter(|j| j.2).map(|j| j.0).collect();
        // keep states that hold coverage or view data; others were only touched by the cull
        self.tiles.retain(|_, s| s.worth_keeping());
        changed
    }

    /// Rewrites every target where the coverage changed in tiles `coords` (each tile's dirty rectangle) from its
    /// pixels before the stroke and the stroke's coverage. Texels elsewhere already hold that (or were never
    /// reached, and blending nothing gives a texel back unchanged).
    fn write(&mut self, coords: &[(u32, u32)], targets: &mut [ChannelTarget]) {
        let tiles_x = self.tiles_x;
        let regions: Vec<Rect> = coords
            .iter()
            .filter_map(|&(tx, ty)| {
                let d = std::mem::take(&mut self.tiles.get_mut(&(ty as usize * tiles_x + tx as usize))?.dirty);
                let (ox, oy) = (tx as usize * T, ty as usize * T);
                (!d.is_empty()).then(|| Rect { x: (ox + d.x0) as u32, y: (oy + d.y0) as u32, w: (d.x1 - d.x0) as u32, h: (d.y1 - d.y0) as u32 })
            })
            .collect();
        if regions.is_empty() {
            return;
        }
        let tiles = &self.tiles;
        // the targets side by side (each writes its own image)
        let mut jobs: Vec<(&mut ChannelTarget, &(TiledImage, Blend))> = targets.iter_mut().zip(&self.before).collect();
        crate::paint::par::for_each_mut(&mut jobs, |_, (target, (before, blend))| {
            let source = target.source.filter(|_| blend.paints());
            target.image.update_regions(&regions, |rect, px| {
                let ti = (rect.y as usize / T) * tiles_x + rect.x as usize / T;
                let Some(acc) = tiles.get(&ti).map(|s| &s.acc) else { return };
                let mut orig = vec![[0.0f32; 4]; rect.area()];
                before.read_region(rect, &mut orig);
                let w = rect.w as usize;
                let (ax, ay) = (rect.x as usize % T, rect.y as usize % T);
                let offset = match blend.mode() {
                    StampMode::Clone { offset } => Some(offset),
                    _ => None,
                };
                for (i, (p, o)) in px.iter_mut().zip(&orig).enumerate() {
                    let c = acc.get((ay + i / w) * T + ax + i % w).copied().unwrap_or(0.0);
                    *p = match (offset, source) {
                        (Some(d), _) => {
                            let (sx, sy) = (i64::from(rect.x) + (i % w) as i64 + i64::from(d[0]), i64::from(rect.y) + (i / w) as i64 + i64::from(d[1]));
                            let inside = sx >= 0 && sy >= 0 && sx < i64::from(before.width()) && sy < i64::from(before.height());
                            if inside { blend.copy(*o, before.get(sx as u32, sy as u32), c) } else { *o }
                        }
                        // a value per texel, looked up only where the stroke reached
                        (None, Some(src)) if c > 0.0 => match src.value(rect.x + (i % w) as u32, rect.y + (i / w) as u32) {
                            Some(v) => blend.with_value(v).apply(*o, c),
                            None => *o,
                        },
                        _ => blend.apply(*o, c),
                    };
                }
            });
        });
    }

    /// One dab of a local stroke: its coverage and the texels under it, then the kernels on the targets.
    fn apply_local_dab(&mut self, ctx: &StampContext, dab: &DabSample, targets: &mut [ChannelTarget]) -> Vec<(u32, u32)> {
        let Some((plan, cells)) = self.plan(ctx, dab, Visit::Collect) else { return Vec::new() };
        let ceiling = plan.visit.ceiling();
        let changed = self.pull(ctx, std::slice::from_ref(&plan));
        let coords: Vec<(u32, u32)> = changed.iter().map(|&t| ((t % self.tiles_x) as u32, (t / self.tiles_x) as u32)).collect();
        let hits: BTreeMap<usize, Vec<Hit>> = self.tiles.iter_mut().map(|(t, s)| (*t, std::mem::take(&mut s.hits))).filter(|(_, h)| !h.is_empty()).collect();
        self.apply_local(ctx.surface, &cells, dab, ceiling, &hits, targets);
        coords
    }

    /// Local strokes: each target's kernel runs on the values under the dab, and the texels under it (`hits`, per
    /// tile) take the change at their frame position, scaled by their weight and the dab's ceiling.
    fn apply_local(
        &mut self,
        surface: &SurfaceMap,
        cells: &CellMap,
        dab: &DabSample,
        ceiling: f32,
        hits: &BTreeMap<usize, Vec<Hit>>,
        targets: &mut [ChannelTarget],
    ) {
        if hits.is_empty() {
            return;
        }
        let tiles_x = self.tiles_x;
        let coords: Vec<(u32, u32)> = hits.keys().map(|&t| ((t % tiles_x) as u32, (t / tiles_x) as u32)).collect();
        let StampStroke { kernels, picks, before, .. } = self;
        for ((target, (_, blend)), kernel) in targets.iter_mut().zip(before.iter()).zip(kernels.iter_mut()) {
            let (Some(kernel), StampMode::Local) = (kernel, target.mode) else { continue };
            let (grid, _) = gather(target.image, blend, surface, cells, picks, dab);
            let mut after = grid.clone();
            kernel.dab(&mut after, dab);
            let change = delta(&grid, &after);
            let has_alpha = grid.has_alpha;
            target.image.update_tiles(&coords, |rect, px| {
                let ti = (rect.y as usize / T) * tiles_x + rect.x as usize / T;
                let Some(list) = hits.get(&ti) else { return };
                let w = rect.w as usize;
                for h in list {
                    let i = h.i as usize;
                    let Some(p) = px.get_mut((i / T) * w + i % T) else { continue };
                    let d = sample(&change, h.p);
                    let k = h.w * ceiling;
                    let cur = blend.decode(*p);
                    let mut next = [cur[0] + d[0] * k, cur[1] + d[1] * k, cur[2] + d[2] * k, cur[3] + d[3] * k];
                    if has_alpha {
                        next[3] = next[3].clamp(0.0, 1.0);
                    }
                    if next.iter().all(|v| v.is_finite()) {
                        *p = blend.encode(next);
                    }
                }
            });
        }
    }
}
