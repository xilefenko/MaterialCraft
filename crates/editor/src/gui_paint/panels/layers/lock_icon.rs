//! The lock badge of a layer row: a padlock on the content thumbnail's bottom-left corner of a locked layer (its
//! content can't be edited, see `materialcraft::paint::lock`), with the reasons on hover. Unlocked layers have none.

use super::lock_text;
use crate::gui_theme;
use eframe::egui::{Color32, CornerRadius, Id, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use materialcraft::paint::LayerLock;

const SIZE: f32 = 13.0;
const LOCKED: Color32 = Color32::from_rgb(230, 180, 70);
/// Blockers listed in the tooltip; the blockers panel has them all.
const MAX_REASONS: usize = 12;

/// Draws the badge for `lock` in the bottom-left corner of `thumb`; `None` when the layer is not locked.
pub fn show(ui: &mut Ui, id: Id, thumb: Rect, lock: Option<&LayerLock>) -> Option<Response> {
    let lock = lock?;
    let rect = Rect::from_min_size(pos2(thumb.min.x - 2.0, thumb.max.y - SIZE + 2.0), vec2(SIZE, SIZE));
    let painter = ui.painter_at(rect.expand(1.0));
    painter.circle_filled(rect.center(), SIZE / 2.0, gui_theme::PANEL_FILL);
    let c = rect.center();
    let body = Rect::from_center_size(pos2(c.x, c.y + 1.5), vec2(7.0, 5.0));
    painter.rect_filled(body, CornerRadius::same(1), LOCKED);
    let shackle = Rect::from_center_size(pos2(c.x, body.min.y - 1.0), vec2(4.6, 5.0));
    painter.rect_stroke(shackle, CornerRadius::same(2), Stroke::new(1.3, LOCKED), StrokeKind::Middle);
    let mut tip = lock_text::heading(lock);
    for line in lock_text::reasons(lock, MAX_REASONS) {
        tip.push_str(&format!("\n• {line}"));
    }
    tip.push_str("\n\nVisibility, opacity, blending and the mask switch still work. Right-click: Unlock for editing.");
    Some(ui.interact(rect, id, Sense::hover()).on_hover_text(tip))
}
