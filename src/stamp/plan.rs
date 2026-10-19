//! [`DabPlan`]: what one dab of a stroke does, worked out before any tile is visited: the frame its footprint is
//! sampled in and, per tile, the texels or cells it may reach. A batch of plans is then run tile by tile
//! ([`DabPlan::run`]): each tile takes its dabs in order and nothing else, so a texel's coverage depends only on
//! the dabs in order, the same bits however the dabs are batched (one dab at a time or a frame's dabs at once).
//!
//! Also here: the per-tile state the stroke keeps ([`TileState`]) and the texel helpers the pull shares.

use super::dab::DabSample;
use super::factors::TexelFactor;
use super::frame::{DabFrame, screen_scale_at};
use super::index::{Aabb, CELL, CELLS_PER_TILE, TexelIndex};
use super::stroke::ViewSpace;
use super::through_mesh::TexelLookup;
use crate::mesh::{SurfaceMap, Vec3};
use crate::paint::TILE_SIZE;
use crate::projection::{ScreenPoint, ViewProjector};
use crate::symmetry::CopyTransform;
use crate::v3::{len, sub};
use std::collections::BTreeMap;

pub(super) const T: usize = TILE_SIZE as usize;

/// A texel as a 3D-view stroke sees it: computed once per stroke.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ViewTexel {
    pos: Vec3,
    screen: [f32; 2],
    /// Visibility × facing fade, 0 where the texel can't be painted from this view.
    weight: f32,
}

/// A stroke's state in one image tile.
pub(super) struct TileState {
    /// Stroke coverage per texel of the tile (row stride [`T`]).
    pub acc: Vec<f32>,
    /// Per symmetry copy and cell of the tile, the cell's texels as the view sees them (filled when first needed).
    pub views: Vec<Option<Box<[ViewTexel]>>>,
    /// Local strokes: the texels under the current dab.
    pub hits: Vec<Hit>,
    /// Where the coverage changed since the targets were last written.
    pub dirty: Dirty,
}

/// A rectangle of a tile's texels (`x0..x1`, `y0..y1`), empty when `x0 >= x1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Dirty {
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
}

impl Default for Dirty {
    fn default() -> Dirty {
        Dirty { x0: T, y0: T, x1: 0, y1: 0 }
    }
}

impl Dirty {
    /// Grows the rectangle over texel `i` of the tile.
    #[inline]
    fn include(&mut self, i: usize) {
        let (x, y) = (i % T, i / T);
        self.x0 = self.x0.min(x);
        self.y0 = self.y0.min(y);
        self.x1 = self.x1.max(x + 1);
        self.y1 = self.y1.max(y + 1);
    }

    pub fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
}

impl TileState {
    pub fn new() -> TileState {
        TileState { acc: vec![0.0; T * T], views: Vec::new(), hits: Vec::new(), dirty: Dirty::default() }
    }

    /// True if the state holds coverage or view data worth keeping.
    pub fn worth_keeping(&self) -> bool {
        !self.views.is_empty() || self.acc.iter().any(|&c| c > 0.0)
    }
}

/// A texel under a local stroke's dab: its index in the tile, its frame position and its weight (visibility ×
/// factors).
#[derive(Debug, Clone, Copy)]
pub(super) struct Hit {
    pub i: u32,
    pub p: [f32; 2],
    pub w: f32,
}

/// What the texels under a dab are visited for, with the dab's opacity ceiling.
#[derive(Debug, Clone, Copy)]
pub(super) enum Visit {
    /// Build the stroke coverage up.
    Build(f32),
    /// Build it up and collect the texels for a local kernel (direct copy only).
    Collect(f32),
}

impl Visit {
    fn mirrors(self) -> bool {
        matches!(self, Visit::Build(_))
    }

    /// The dab's opacity ceiling.
    pub fn ceiling(self) -> f32 {
        match self {
            Visit::Build(c) | Visit::Collect(c) => c,
        }
    }
}

/// Maps texel positions into a texture-space dab frame: `frame = center + (texel − anchor) · scale`.
#[derive(Debug, Clone, Copy)]
pub(super) struct TexelFrame {
    pub center: [f32; 2],
    pub anchor: [f32; 2],
    pub scale: f32,
    /// The texture's size in texels.
    pub limit: [f32; 2],
}

impl TexelFrame {
    fn to_frame(self, t: [f32; 2]) -> [f32; 2] {
        [self.center[0] + (t[0] - self.anchor[0]) * self.scale, self.center[1] + (t[1] - self.anchor[1]) * self.scale]
    }

    fn to_texel(self, f: [f32; 2]) -> [f32; 2] {
        [self.anchor[0] + (f[0] - self.center[0]) / self.scale, self.anchor[1] + (f[1] - self.center[1]) / self.scale]
    }
}

/// What a batch of plans is pulled through: the same for every dab of the batch.
#[derive(Clone, Copy)]
pub(super) struct Pull<'a> {
    pub surface: &'a SurfaceMap,
    pub index: &'a TexelIndex,
    pub factors: &'a [&'a dyn TexelFactor],
    pub copies: &'a [CopyTransform],
    pub tiles_x: usize,
}

/// How a dab's footprint is laid on the texels.
enum Shape<'a> {
    /// Texture-space frames: the texels of a rectangle (and, with symmetry, cells mirrored onto it).
    Rect { frame: TexelFrame, rect: [usize; 4], lookup: Option<TexelLookup> },
    /// 3D-view frames: cells found through the index.
    View { view: ViewSpace<'a>, frame: DabFrame, min_facing: f32 },
}

/// One dab, planned. See the module docs.
pub(super) struct DabPlan<'a> {
    dab: &'a DabSample,
    pub visit: Visit,
    shape: Shape<'a>,
    /// Per tile, the `(copy, cell)` pairs to visit.
    work: BTreeMap<usize, Vec<(usize, usize)>>,
    /// Every tile the dab may change (sorted, once each).
    tiles: Vec<usize>,
}

impl<'a> DabPlan<'a> {
    /// A texture-space dab (2D-view strokes, UV-aligned placement) in `frame`; `None` if it lies off the texture.
    pub fn rect(pull: &Pull, dab: &'a DabSample, visit: Visit, frame: TexelFrame) -> Option<DabPlan<'a>> {
        let b = dab.footprint.bounds();
        // the texel rectangle the footprint covers
        let lo = frame.to_texel([b[0], b[1]]);
        let hi = frame.to_texel([b[2], b[3]]);
        let (x0, y0) = (lo[0].floor().max(0.0), lo[1].floor().max(0.0));
        let (x1, y1) = (hi[0].ceil().min(frame.limit[0]), hi[1].ceil().min(frame.limit[1]));
        if !(x0 < x1 && y0 < y1) {
            return None;
        }
        let (x0, y0, x1, y1) = (x0 as usize, y0 as usize, x1 as usize, y1 as usize);
        let tiles_x = pull.tiles_x;
        let mut tiles: Vec<usize> = (y0 / T..=(y1 - 1) / T).flat_map(|ty| (x0 / T..=(x1 - 1) / T).map(move |tx| ty * tiles_x + tx)).collect();
        // symmetric copies: cells whose texels may land on the surface under the dab once transformed
        let mirrored = visit.mirrors() && pull.copies.iter().any(|c| !c.is_identity());
        let lookup = if mirrored { TexelLookup::build(pull.surface, x0, y0, x1, y1) } else { None };
        let mut work: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
        if let Some(lookup) = &lookup {
            for (ci, copy) in pull.copies.iter().enumerate().filter(|(_, c)| !c.is_identity()) {
                for (tile, cell) in pull.index.cull(|aabb| transform_box(*copy, aabb).overlaps(lookup.bounds())) {
                    work.entry(tile).or_default().push((ci, cell));
                }
            }
            tiles.extend(work.keys());
            tiles.sort_unstable();
            tiles.dedup();
        }
        Some(DabPlan { dab, visit, shape: Shape::Rect { frame, rect: [x0, y0, x1, y1], lookup }, work, tiles })
    }

    /// A 3D-view dab; `None` if no cell may lie under it.
    pub fn view(pull: &Pull, view: ViewSpace<'a>, dab: &'a DabSample, visit: Visit) -> Option<DabPlan<'a>> {
        let max_angle = view_max_angle(view);
        let min_facing = max_angle.to_radians().cos();
        let b = dab.footprint.bounds();
        let reach = ((b[2] - b[0]).max(b[3] - b[1])) * 0.5 * std::f32::consts::SQRT_2;
        let frame = DabFrame::new(view.projector, view.placement, dab.center, reach);
        // a flipped copy samples the footprint mirrored about the dab's centre: its bounds mirrored as well
        let c = dab.center;
        let flipped = [b[0].min(2.0 * c[0] - b[2]), b[1].min(2.0 * c[1] - b[3]), b[2].max(2.0 * c[0] - b[0]), b[3].max(2.0 * c[1] - b[1])];
        // cells per tile, per copy
        let mut work: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
        for (ci, copy) in pull.copies.iter().enumerate().filter(|(_, c)| visit.mirrors() || c.is_identity()) {
            let bounds = if copy.flips() { flipped } else { b };
            let cells = pull.index.cull(|aabb| {
                let moved = transform_box(*copy, aabb);
                frame.may_touch(view.projector, &moved, bounds)
            });
            for (tile, cell) in cells {
                work.entry(tile).or_default().push((ci, cell));
            }
        }
        if work.is_empty() {
            return None;
        }
        let tiles = work.keys().copied().collect();
        Some(DabPlan { dab, visit, shape: Shape::View { view, frame, min_facing }, work, tiles })
    }

    /// The tiles the dab may change (sorted).
    pub fn tiles(&self) -> &[usize] {
        &self.tiles
    }

    /// Runs the dab on tile `ti`; true if the tile changed.
    pub fn run(&self, pull: &Pull, ti: usize, state: &mut TileState) -> bool {
        match &self.shape {
            Shape::Rect { frame, rect, lookup } => self.run_rect(pull, ti, state, *frame, *rect, lookup.as_ref()),
            Shape::View { view, frame, min_facing } => self.run_view(pull, ti, state, *view, frame, *min_facing),
        }
    }

    fn run_rect(&self, pull: &Pull, ti: usize, state: &mut TileState, frame: TexelFrame, rect: [usize; 4], lookup: Option<&TexelLookup>) -> bool {
        let (dab, visit, factors, surface) = (self.dab, self.visit, pull.factors, pull.surface);
        let [x0, y0, x1, y1] = rect;
        let (w, cells_x) = (surface.width(), pull.index.cells_x());
        let (tx, ty) = ((ti % pull.tiles_x) * T, (ti / pull.tiles_x) * T);
        let mut changed = false;
        for y in y0.max(ty)..y1.min(ty + T) {
            for x in x0.max(tx)..x1.min(tx + T) {
                let p = frame.to_frame([x as f32 + 0.5, y as f32 + 0.5]);
                let v = dab.footprint.sample(p);
                if v <= 0.0 {
                    continue;
                }
                let w = factor(factors, x, y, None);
                changed |= visit_at(state, (y - ty) * T + (x - tx), p, v, w, visit);
            }
        }
        let (Some(lookup), Some(cells)) = (lookup, self.work.get(&ti)) else { return changed };
        for &(copy_i, cell) in cells {
            let Some(copy) = pull.copies.get(copy_i) else { continue };
            let (cx, cy) = ((cell % cells_x) * CELL, (cell / cells_x) * CELL);
            for k in 0..CELL * CELL {
                let (x, y) = (cx + k % CELL, cy + k / CELL);
                if x >= w || y >= surface.height() {
                    continue;
                }
                let Some((p, n, _)) = surface.point_index(y * w + x) else { continue };
                let Some(s) = lookup.nearest(copy.point(p), copy.normal(n)) else { continue };
                let f = copy.frame(frame.to_frame([s[0] as f32 + 0.5, s[1] as f32 + 0.5]), frame.center);
                let v = dab.footprint.sample(f);
                if v <= 0.0 {
                    continue;
                }
                let w = factor(factors, x, y, None);
                changed |= visit_at(state, (y - ty) * T + (x - tx), f, v, w, visit);
            }
        }
        changed
    }

    fn run_view(&self, pull: &Pull, ti: usize, state: &mut TileState, view: ViewSpace, frame: &DabFrame, min_facing: f32) -> bool {
        let Some(cells) = self.work.get(&ti) else { return false };
        let (dab, visit, surface, copies, factors) = (self.dab, self.visit, pull.surface, pull.copies, pull.factors);
        let cells_x = pull.index.cells_x();
        if state.views.is_empty() {
            state.views = vec![None; copies.len() * CELLS_PER_TILE * CELLS_PER_TILE];
        }
        let (tx, ty) = ((ti % pull.tiles_x) * T, (ti / pull.tiles_x) * T);
        let mut changed = false;
        for &(copy_i, cell) in cells {
            let (cx, cy) = ((cell % cells_x) * CELL, (cell / cells_x) * CELL);
            let slot = copy_i * CELLS_PER_TILE * CELLS_PER_TILE + ((cy - ty) / CELL) * CELLS_PER_TILE + (cx - tx) / CELL;
            let Some(copy) = copies.get(copy_i) else { continue };
            if state.views.get(slot).is_some_and(Option::is_none) {
                let texels = view_cell(surface, view, *copy, cx, cy, min_facing);
                if let Some(s) = state.views.get_mut(slot) {
                    *s = Some(texels);
                }
            }
            // the cell's texels, taken out while the tile's coverage is built up
            let Some(Some(texels)) = state.views.get_mut(slot).map(Option::take) else { continue };
            for (k, t) in texels.iter().enumerate() {
                if t.weight <= 0.0 {
                    continue;
                }
                let Some((p, fade)) = frame.map(t.pos, t.screen) else { continue };
                let f = copy.frame(p, dab.center);
                let v = dab.footprint.sample(f);
                if v <= 0.0 {
                    continue;
                }
                let (x, y) = (cx + k % CELL, cy + k / CELL);
                let w = t.weight * fade * factor(factors, x, y, Some(t.screen));
                changed |= visit_at(state, (y - ty) * T + (x - tx), f, v, w, visit);
            }
            if let Some(s) = state.views.get_mut(slot) {
                *s = Some(texels);
            }
        }
        changed
    }
}

/// The facing limit of a view, in degrees (0 … 180).
pub(super) fn view_max_angle(view: ViewSpace) -> f32 {
    if view.max_angle.is_finite() { view.max_angle.clamp(0.0, 180.0) } else { 180.0 }
}

/// The UV-aligned frame of a dab in the 3D view: the texel under its centre and the screen size of a texel there.
pub(super) fn uv_frame(surface: &SurfaceMap, projector: &dyn ViewProjector, dab: &DabSample) -> Option<TexelFrame> {
    let hit = projector.pick(dab.center)?;
    let (w, h) = (surface.width(), surface.height());
    let anchor = [hit.uv[0] * w as f32, hit.uv[1] * h as f32];
    let (ax, ay) = (anchor[0].floor().max(0.0) as usize, anchor[1].floor().max(0.0) as usize);
    let p0 = surface.point_index(ay * w + ax)?.0;
    let p1 = surface.point_index(ay * w + ax + 1).or_else(|| surface.point_index((ay + 1) * w + ax))?.0;
    let per_texel = len(sub(p1, p0));
    let scale = screen_scale_at(projector, hit.position)? * per_texel;
    (scale.is_finite() && scale > 0.0).then_some(TexelFrame { center: dab.center, anchor, scale, limit: [w as f32, h as f32] })
}

/// The texels of the cell at `(cx, cy)` as the view sees them through symmetry copy `copy`.
fn view_cell(surface: &SurfaceMap, view: ViewSpace, copy: CopyTransform, cx: usize, cy: usize, min_facing: f32) -> Box<[ViewTexel]> {
    let (w, h) = (surface.width(), surface.height());
    let mut out = vec![ViewTexel::default(); CELL * CELL];
    let camera = view.projector.camera();
    for (k, t) in out.iter_mut().enumerate() {
        let (x, y) = (cx + k % CELL, cy + k / CELL);
        if x >= w || y >= h {
            continue;
        }
        let Some((p, n, _)) = surface.point_index(y * w + x) else { continue };
        let (p, n) = (copy.point(p), copy.normal(n));
        let Some(s) = view.projector.to_screen(p) else { continue };
        let facing = view.projector.facing(p, n);
        if facing < min_facing {
            continue;
        }
        // a short fade above the limit avoids a hard rim
        let fade = ((facing - min_facing) / 0.05).clamp(0.0, 1.0);
        if let Some(depth) = view.depth {
            // two screen units of depth, more on steep surfaces, whose depth changes fast within one unit
            let visible = |s: ScreenPoint| {
                let unit = 1.0 / camera.screen_scale(s.view_depth).max(1e-12);
                depth.visible(s.pos, s.view_depth, 2.0 * unit / facing.max(0.15))
            };
            // an edge or gutter texel's centre extends its triangle's plane, which can dip behind the neighbouring
            // surface at a concave crease: the point on the triangle itself counts as well
            let on_triangle = || surface.point_on_triangle_index(y * w + x).and_then(|q| view.projector.to_screen(copy.point(q)));
            if !visible(s) && !on_triangle().is_some_and(visible) {
                continue;
            }
        }
        *t = ViewTexel { pos: p, screen: s.pos, weight: fade };
    }
    out.into_boxed_slice()
}

/// The box around `b`'s corners moved by `copy`.
fn transform_box(copy: CopyTransform, b: &Aabb) -> Aabb {
    if copy.is_identity() {
        return *b;
    }
    let [first, rest @ ..] = b.corners().map(|p| copy.point(p));
    let mut out = Aabb::point(first);
    rest.into_iter().for_each(|p| out.include(p));
    out
}

fn factor(factors: &[&dyn TexelFactor], x: usize, y: usize, screen: Option<[f32; 2]>) -> f32 {
    factors.iter().fold(1.0, |f, t| f * t.factor(x as u32, y as u32, screen).clamp(0.0, 1.0))
}

/// Visits texel `i` of a tile at frame position `p` with footprint coverage `v` and weight `w`: builds the stroke
/// coverage up by `v · w` and, for local strokes, collects the texel. True if the tile changed.
#[inline]
fn visit_at(state: &mut TileState, i: usize, p: [f32; 2], v: f32, w: f32, visit: Visit) -> bool {
    match visit {
        Visit::Build(ceiling) => {
            let changed = build_up_at(&mut state.acc, i, v * w, ceiling);
            if changed {
                state.dirty.include(i);
            }
            changed
        }
        Visit::Collect(ceiling) => {
            if build_up_at(&mut state.acc, i, v * w, ceiling) {
                state.dirty.include(i);
            }
            let w = if w.is_finite() { w.clamp(0.0, 1.0) } else { 0.0 };
            if v * w <= 0.0 {
                return false;
            }
            state.hits.push(Hit { i: i as u32, p, w });
            true
        }
    }
}

/// Builds the stroke coverage at `i` up by `v` towards `ceiling`; true if it changed.
#[inline]
fn build_up_at(acc: &mut [f32], i: usize, v: f32, ceiling: f32) -> bool {
    let Some(c) = acc.get_mut(i) else { return false };
    let v = v.min(1.0);
    if ceiling <= *c || v.is_nan() || v <= 0.0 {
        return false;
    }
    let next = *c + v * (ceiling - *c);
    if next > *c {
        *c = next;
        true
    } else {
        false
    }
}
