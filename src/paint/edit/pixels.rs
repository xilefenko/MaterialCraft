//! Pixel edits as tile swaps: putting shared tiles into an image, writing a region, and capturing what a
//! direct paint operation changed.

use super::tree::content_layer_mut;
use super::{Edit, EditError, ImageRef};
use crate::paint::doc::Document;
use crate::paint::image::{ImageError, Rect, TILE_SIZE, Tile, TiledImage};
use std::collections::HashSet;

/// One tile of an [`Edit::SwapTiles`]: tile (tx, ty) becomes `tile` (`None`: unallocated, reads as zero).
#[derive(Debug, Clone, PartialEq)]
pub struct TileSwap {
    pub tx: u32,
    pub ty: u32,
    pub tile: Option<Tile>,
}

fn image_mut<'a>(doc: &'a mut Document, r: &ImageRef) -> Result<&'a mut TiledImage, EditError> {
    let no_image = || EditError::NoImage { set: r.layer.set.clone(), layer: r.layer.layer.clone(), image: r.image };
    content_layer_mut(doc, &r.layer)?.image_mut(r.image).ok_or_else(no_image)
}

fn image<'a>(doc: &'a Document, r: &ImageRef) -> Result<&'a TiledImage, EditError> {
    let layer = doc.texture_set(&r.layer.set).and_then(|s| s.layer(&r.layer.layer));
    let layer = layer.ok_or_else(|| EditError::NoLayer { set: r.layer.set.clone(), layer: r.layer.layer.clone() })?;
    let found = layer.images().into_iter().find(|(which, _)| *which == r.image).map(|(_, i)| i);
    found.ok_or_else(|| EditError::NoImage { set: r.layer.set.clone(), layer: r.layer.layer.clone(), image: r.image })
}

/// Swaps of every tile of `now` that is not the same allocation as in `before`, back to `before`'s tiles.
fn restore(now: &TiledImage, before: &TiledImage) -> Vec<TileSwap> {
    now.changed_tiles(before).into_iter().map(|(tx, ty)| TileSwap { tx, ty, tile: before.tile(tx, ty).cloned() }).collect()
}

pub(super) fn swap(doc: &mut Document, r: ImageRef, tiles: Vec<TileSwap>) -> Result<Edit, EditError> {
    let img = image_mut(doc, &r)?;
    let expected = (TILE_SIZE * TILE_SIZE) as usize * img.format().bytes_per_pixel();
    for s in &tiles {
        if img.tile_rect(s.tx, s.ty).is_none() {
            return Err(ImageError::NoSuchTile { tx: s.tx, ty: s.ty }.into());
        }
        if let Some(t) = &s.tile
            && t.len() != expected
        {
            return Err(ImageError::WrongDataSize { expected, found: t.len() }.into());
        }
    }
    let mut undo = Vec::with_capacity(tiles.len());
    for s in tiles {
        match img.replace_tile(s.tx, s.ty, s.tile) {
            Ok(old) => undo.push(TileSwap { tx: s.tx, ty: s.ty, tile: old }),
            Err(e) => {
                // Not reached after the checks above; put back what was swapped so the image stays as it was.
                for u in undo.into_iter().rev() {
                    let _ = img.replace_tile(u.tx, u.ty, u.tile);
                }
                return Err(e.into());
            }
        }
    }
    // Applied in reverse, the old tiles also come back right when a tile was listed twice.
    undo.reverse();
    Ok(Edit::SwapTiles { image: r, tiles: undo })
}

pub(super) fn write(doc: &mut Document, r: ImageRef, rect: Rect, pixels: &[[f32; 4]]) -> Result<Edit, EditError> {
    let img = image_mut(doc, &r)?;
    let (x1, y1) = (rect.x.checked_add(rect.w).ok_or(EditError::BadRegion)?, rect.y.checked_add(rect.h).ok_or(EditError::BadRegion)?);
    if x1 > img.width() || y1 > img.height() || pixels.len() != rect.area() {
        return Err(EditError::BadRegion);
    }
    if rect.area() == 0 {
        return Ok(Edit::SwapTiles { image: r, tiles: Vec::new() });
    }
    let before = img.clone();
    let touched: Vec<(u32, u32)> =
        (rect.y / TILE_SIZE..=(y1 - 1) / TILE_SIZE).flat_map(|ty| (rect.x / TILE_SIZE..=(x1 - 1) / TILE_SIZE).map(move |tx| (tx, ty))).collect();
    img.update_tiles(&touched, |t, px| {
        for y in t.y.max(rect.y)..(t.y + t.h).min(y1) {
            for x in t.x.max(rect.x)..(t.x + t.w).min(x1) {
                let src = (y - rect.y) as usize * rect.w as usize + (x - rect.x) as usize;
                let dst = (y - t.y) as usize * t.w as usize + (x - t.x) as usize;
                if let (Some(d), Some(s)) = (px.get_mut(dst), pixels.get(src)) {
                    *d = *s;
                }
            }
        }
    });
    Ok(Edit::SwapTiles { tiles: restore(img, &before), image: r })
}

/// Folds the tile swaps of a later inverse into an older one on the same image: the older swap's tiles win
/// (they hold the state before both); tiles only the later one touched are added.
pub(super) fn merge_older(older: &mut Vec<TileSwap>, later: Vec<TileSwap>) {
    let seen: HashSet<(u32, u32)> = older.iter().map(|s| (s.tx, s.ty)).collect();
    older.extend(later.into_iter().filter(|s| !seen.contains(&(s.tx, s.ty))));
}

/// Records what painting straight into an image changes: [`Self::begin`] before a stroke (or a dab), paint
/// into the document's image as usual, then [`Self::finish`] returns the inverse edit for the history.
///
/// Holding the capture keeps every tile of the image shared, so the first write to a tile copies it and
/// the old tile stays here; the capture costs one pointer per tile, not a copy of the image.
#[derive(Debug, Clone)]
pub struct TileCapture {
    image: ImageRef,
    before: TiledImage,
}

impl TileCapture {
    pub fn begin(doc: &Document, image_ref: ImageRef) -> Result<TileCapture, EditError> {
        let before = image(doc, &image_ref)?.clone();
        Ok(TileCapture { image: image_ref, before })
    }

    /// The image being captured.
    pub fn image(&self) -> &ImageRef {
        &self.image
    }

    /// The edit that puts back every tile changed since [`Self::begin`] (an empty swap if nothing changed).
    pub fn finish(self, doc: &Document) -> Result<Edit, EditError> {
        let now = image(doc, &self.image)?;
        if now.size() != self.before.size() || now.format() != self.before.format() {
            return Err(EditError::BadRegion);
        }
        Ok(Edit::SwapTiles { tiles: restore(now, &self.before), image: self.image })
    }
}
