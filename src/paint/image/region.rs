//! Bulk pixel access to a [`TiledImage`] by region and by tile, including the parallel tile update.

use super::{Rect, T, TILE_SIZE, Tile, TileBuf, TiledImage, tile_rect};
use crate::paint::par::for_each_mut;
use crate::paint::pixel::PixelFormat;

impl TiledImage {
    /// Decodes `r` row by row into `out` (`r.w * r.h` pixels); pixels outside the image read as
    /// zero samples. If `out` is shorter, only the rows that fit are written.
    pub fn read_region(&self, r: Rect, out: &mut [[f32; 4]]) {
        let zero = self.zero_pixel();
        let bpp = self.format.bytes_per_pixel();
        for (row, line) in out.chunks_exact_mut(r.w as usize).take(r.h as usize).enumerate() {
            let y = r.y as usize + row;
            if y >= self.height as usize {
                line.fill(zero);
                continue;
            }
            for (col, p) in line.iter_mut().enumerate() {
                let x = r.x as usize + col;
                *p = if x >= self.width as usize {
                    zero
                } else {
                    match &self.tiles[(y / T) * self.tiles_x() as usize + x / T] {
                        None => zero,
                        Some(t) => {
                            let o = ((y % T) * T + x % T) * bpp;
                            self.format.decode(&t.bytes()[o..o + bpp])
                        }
                    }
                };
            }
        }
    }

    /// Decodes tile (tx, ty) into `out` (its [`Rect::area`] pixels). Returns its rect, or `None`
    /// outside the tile grid.
    pub fn read_tile(&self, tx: u32, ty: u32, out: &mut [[f32; 4]]) -> Option<Rect> {
        let r = self.tile_rect(tx, ty)?;
        self.read_region(r, out);
        Some(r)
    }

    /// Encodes `pixels` (row by row, the tile rect's size) into tile (tx, ty) and marks it dirty.
    pub fn write_tile(&mut self, tx: u32, ty: u32, pixels: &[[f32; 4]]) -> bool {
        let (Some(i), Some(r)) = (self.tile_index(tx, ty), self.tile_rect(tx, ty)) else {
            return false;
        };
        let fmt = self.format;
        let t = self.tiles[i].get_or_insert_with(|| Tile::zeroed(T * T * fmt.bytes_per_pixel())).make_mut();
        encode_tile(fmt, r, pixels, t);
        self.dirty[i] = true;
        true
    }

    /// Rewrites the listed tiles in parallel: `f` gets the tile rect and its current pixels, and
    /// edits them in place. Tiles outside the grid and repeated entries are ignored. Marks them dirty.
    pub fn update_tiles<F: Fn(Rect, &mut [[f32; 4]]) + Sync>(&mut self, tiles: &[(u32, u32)], f: F) {
        let (w, h, fmt) = (self.width, self.height, self.format);
        let mut wanted = vec![false; self.tiles.len()];
        for &(tx, ty) in tiles {
            if let Some(i) = self.tile_index(tx, ty) {
                wanted[i] = true;
            }
        }
        let tiles_x = self.tiles_x();
        let mut jobs: Vec<(u32, &mut TileBuf)> = self.tiles.iter_mut().enumerate().filter(|(i, _)| wanted[*i]).map(|(i, t)| (i as u32, t)).collect();
        for_each_mut(&mut jobs, |_, (i, tile)| {
            let Some(r) = tile_rect(w, h, *i % tiles_x, *i / tiles_x) else {
                return;
            };
            let mut px = vec![[0.0f32; 4]; r.area()];
            let buf = tile.get_or_insert_with(|| Tile::zeroed(T * T * fmt.bytes_per_pixel())).make_mut();
            decode_tile(fmt, r, buf, &mut px);
            f(r, &mut px);
            encode_tile(fmt, r, &px, buf);
        });
        for (d, wnt) in self.dirty.iter_mut().zip(wanted) {
            *d |= wnt;
        }
    }

    /// Like [`Self::update_tiles`] for parts of tiles: each of `regions` lies inside one tile (it is cut to the
    /// tile holding its top-left corner; at most one region per tile counts). Only its pixels are decoded, handed to
    /// `f` with the region and written back; the rest of the tile stays as it is. Tiles are allocated as needed and
    /// marked dirty.
    pub fn update_regions<F: Fn(Rect, &mut [[f32; 4]]) + Sync>(&mut self, regions: &[Rect], f: F) {
        let (w, h, fmt) = (self.width, self.height, self.format);
        let tiles_x = self.tiles_x();
        let mut wanted: Vec<Option<Rect>> = vec![None; self.tiles.len()];
        for r in regions {
            let Some(i) = self.tile_index(r.x / TILE_SIZE, r.y / TILE_SIZE) else { continue };
            let Some(t) = tile_rect(w, h, r.x / TILE_SIZE, r.y / TILE_SIZE) else { continue };
            let (x1, y1) = ((r.x + r.w).min(t.x + t.w), (r.y + r.h).min(t.y + t.h));
            if let (Some(slot), true) = (wanted.get_mut(i), r.x < x1 && r.y < y1) {
                slot.get_or_insert(Rect { x: r.x, y: r.y, w: x1 - r.x, h: y1 - r.y });
            }
        }
        let mut jobs: Vec<(Rect, u32, &mut TileBuf)> =
            self.tiles.iter_mut().zip(&wanted).enumerate().filter_map(|(i, (t, r))| r.map(|r| (r, i as u32, t))).collect();
        for_each_mut(&mut jobs, |_, (r, i, tile)| {
            let (ox, oy) = ((*i % tiles_x * TILE_SIZE) as usize, (*i / tiles_x * TILE_SIZE) as usize);
            let (x0, y0) = (r.x as usize - ox, r.y as usize - oy);
            let mut px = vec![[0.0f32; 4]; r.area()];
            let buf = tile.get_or_insert_with(|| Tile::zeroed(T * T * fmt.bytes_per_pixel())).make_mut();
            let bpp = fmt.bytes_per_pixel();
            for (row, line) in px.chunks_exact_mut(r.w as usize).enumerate() {
                let o = ((y0 + row) * T + x0) * bpp;
                let Some(src) = buf.get(o..o + line.len() * bpp) else { continue };
                for (p, b) in line.iter_mut().zip(src.chunks_exact(bpp)) {
                    *p = fmt.decode(b);
                }
            }
            f(*r, &mut px);
            for (row, line) in px.chunks_exact(r.w as usize).enumerate() {
                let o = ((y0 + row) * T + x0) * bpp;
                let Some(dst) = buf.get_mut(o..o + line.len() * bpp) else { continue };
                for (p, b) in line.iter().zip(dst.chunks_exact_mut(bpp)) {
                    fmt.encode(*p, b);
                }
            }
        });
        for (d, r) in self.dirty.iter_mut().zip(wanted) {
            *d |= r.is_some();
        }
    }

    /// Sets every pixel to `p` (allocating all tiles).
    pub fn fill(&mut self, p: [f32; 4]) {
        let all = self.all_tiles();
        self.update_tiles(&all, |_, px| px.fill(p));
    }

    /// Frees every tile (all pixels read as zero again) and marks them dirty.
    pub fn clear(&mut self) {
        self.tiles.iter_mut().for_each(|t| *t = None);
        self.dirty.iter_mut().for_each(|d| *d = true);
    }
}

pub(super) fn decode_tile(fmt: PixelFormat, r: Rect, buf: &[u8], out: &mut [[f32; 4]]) {
    let bpp = fmt.bytes_per_pixel();
    for (row, line) in out.chunks_exact_mut(r.w as usize).take(r.h as usize).enumerate() {
        for (col, p) in line.iter_mut().enumerate() {
            let o = (row * T + col) * bpp;
            *p = fmt.decode(&buf[o..o + bpp]);
        }
    }
}

pub(super) fn encode_tile(fmt: PixelFormat, r: Rect, px: &[[f32; 4]], buf: &mut [u8]) {
    let bpp = fmt.bytes_per_pixel();
    for (row, line) in px.chunks_exact(r.w as usize).take(r.h as usize).enumerate() {
        for (col, p) in line.iter().enumerate() {
            let o = (row * T + col) * bpp;
            fmt.encode(*p, &mut buf[o..o + bpp]);
        }
    }
}
