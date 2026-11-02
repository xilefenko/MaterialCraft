//! Drawing one row of the layer list and reporting what was clicked in it. A layer row is: eye, indent,
//! fold arrow (folders), content thumbnail (with the instance and lock badges), mask thumbnail (if any), name, kind, effects and geometry mask icon,
//! and on the right the view channel's blend mode and opacity (LS-08). An effect row is its kind, label and an on/off box.

use super::geometry_icon;
use super::instance_icon;
use super::lock_icon;
use super::rows::{EFFECT_ROW, INDENT, LAYER_ROW};
use super::selection::Part;
use super::thumbnails::Pair;
use crate::gui_theme;
use eframe::egui::{self, Align2, Color32, FontId, Id, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use materialcraft::paint::edit::StackId;
use materialcraft::paint::effect::{EffectDomain, EffectKind};
use materialcraft::paint::instance::InstanceRole;
use materialcraft::paint::{Channel, Inclusion, Layer, LayerKind};

const EYE: f32 = 22.0;
const FOLD: f32 = 12.0;
const THUMB: f32 = 36.0;
const GAP: f32 = 4.0;

/// Horizontal span of a layer row's mask thumbnail, relative to the row's left edge.
pub fn mask_span(depth: usize) -> (f32, f32) {
    let x = EYE + depth as f32 * INDENT + FOLD + THUMB + GAP;
    (x, x + THUMB)
}

/// What a layer row reported this frame.
#[derive(Default)]
pub struct LayerRowOut {
    /// Clicked on the content or the mask.
    pub clicked: Option<Part>,
    /// Alt+click on the mask thumbnail (MK-05).
    pub isolate: bool,
    pub toggle_visible: bool,
    pub toggle_fold: bool,
    pub rename: bool,
    pub toggle_effects: bool,
    /// The geometry mask icon was clicked (GM-03).
    pub edit_geometry: bool,
    /// The whole row with every part in it (eye, thumbnails, name, effects toggle), for drags and the
    /// context menu: a right-click anywhere on the row opens the menu, not only on the bare background
    /// the parts leave free (the parts sit on top and take the click).
    pub response: Option<Response>,
}

/// How a layer row looks.
pub struct LayerLook<'a> {
    pub layer: &'a Layer,
    pub depth: usize,
    pub open_folder: bool,
    pub selected: bool,
    /// The selected part when this is the focused layer.
    pub focus: Option<Part>,
    pub isolated: bool,
    pub channel: Option<Channel>,
    pub thumbs: Pair<'a>,
    pub effects_open: bool,
    /// How much of the set's geometry the layer's geometry mask includes.
    pub geometry: Inclusion,
    /// The geometry mask edit mode is on for this layer.
    pub geometry_editing: bool,
    /// Source, instance or neither (IN-09).
    pub instance: InstanceRole,
}

pub fn layer_row(ui: &mut Ui, rect: Rect, look: LayerLook) -> LayerRowOut {
    let l = look.layer;
    let mut out = LayerRowOut::default();
    let id = Id::new(("layer_row", &l.id));
    let row = ui.interact(rect, id, Sense::click_and_drag());
    let mut whole = row.clone();
    let painter = ui.painter_at(rect);
    let bg = if look.selected {
        gui_theme::STRIP_SELECTED
    } else if row.hovered() {
        gui_theme::STRIP_HOVER
    } else {
        gui_theme::PANEL_FILL
    };
    painter.rect_filled(rect.shrink2(vec2(0.0, 0.5)), 0.0, bg);
    // Eye.
    let eye = Rect::from_min_size(rect.min, vec2(EYE, LAYER_ROW));
    let eye_r = ui.interact(eye, id.with("eye"), Sense::click());
    let eye_col = if l.visible { gui_theme::ICON } else { gui_theme::ICON_DISABLED };
    painter.circle_stroke(eye.center(), 5.0, Stroke::new(1.2, eye_col));
    if l.visible {
        painter.circle_filled(eye.center(), 2.0, eye_col);
    }
    let eye_r = eye_r.on_hover_text(if l.visible { "Hide" } else { "Show" });
    out.toggle_visible = eye_r.clicked();
    whole |= eye_r;
    let mut x = rect.min.x + EYE + look.depth as f32 * INDENT;
    // Fold arrow for folders.
    if let LayerKind::Group(children) = &l.kind {
        let fold = Rect::from_min_size(pos2(x, rect.min.y), vec2(FOLD, LAYER_ROW));
        if !children.is_empty() {
            let arrow = if look.open_folder { "⏷" } else { "⏵" };
            painter.text(fold.center(), Align2::CENTER_CENTER, arrow, FontId::proportional(10.0), gui_theme::ICON);
            let fold_r = ui.interact(fold, id.with("fold"), Sense::click());
            out.toggle_fold = fold_r.clicked();
            whole |= fold_r;
        }
    }
    x += FOLD;
    // Thumbnails.
    let content = Rect::from_min_size(pos2(x, rect.min.y + (LAYER_ROW - THUMB) / 2.0), vec2(THUMB, THUMB));
    thumb(ui, content, look.thumbs.content, look.focus == Some(Part::Content), kind_mark(l));
    let content_r = ui.interact(content, id.with("content"), Sense::click());
    if content_r.clicked() {
        out.clicked = Some(Part::Content);
    }
    whole |= content_r;
    if let Some(badge) = instance_icon::show(ui, id.with("instance"), content, look.instance) {
        whole |= badge;
    }
    if let Some(badge) = lock_icon::show(ui, id.with("lock"), content, l.lock.as_ref()) {
        whole |= badge;
    }
    x += THUMB + GAP;
    if let Some(mask) = &l.mask {
        let (m0, _) = mask_span(look.depth);
        let mrect = Rect::from_min_size(pos2(rect.min.x + m0, content.min.y), vec2(THUMB, THUMB));
        thumb(ui, mrect, look.thumbs.mask, look.focus == Some(Part::Mask), if mask.effects.is_empty() { "" } else { "fx" });
        if !mask.enabled {
            painter.line_segment([mrect.left_top(), mrect.right_bottom()], Stroke::new(2.0, Color32::from_rgb(220, 70, 60)));
        }
        if look.isolated {
            painter.rect_stroke(mrect.expand(1.5), 1.0, Stroke::new(1.5, Color32::from_rgb(90, 170, 255)), StrokeKind::Outside);
        }
        let m = ui.interact(mrect, id.with("mask"), Sense::click()).on_hover_text("Click: paint into the mask. Alt+click: show only this mask.");
        if m.clicked() {
            if ui.input(|i| i.modifiers.alt) {
                out.isolate = true;
            } else {
                out.clicked = Some(Part::Mask);
            }
        }
        whole |= m;
        x += THUMB + GAP;
    }
    // Name, kind, effects toggle.
    let name_rect = Rect::from_min_max(pos2(x, rect.min.y + 4.0), pos2(rect.max.x - 64.0, rect.min.y + 22.0));
    let name_col = if l.visible { gui_theme::HEADER_TEXT } else { gui_theme::ICON_DISABLED };
    painter.text(name_rect.left_center(), Align2::LEFT_CENTER, &l.name, FontId::proportional(13.0), name_col);
    let name_r = ui.interact(name_rect, id.with("name"), Sense::click()).on_hover_text("Double-click to rename");
    out.rename = name_r.double_clicked();
    if name_r.clicked() {
        out.clicked = Some(Part::Content);
    }
    whole |= name_r;
    let n_effects = l.effects.len() + l.mask.as_ref().map_or(0, |m| m.effects.len());
    let sub = pos2(x, rect.min.y + 32.0);
    painter.text(sub, Align2::LEFT_CENTER, kind_name(l), FontId::proportional(10.5), gui_theme::LABEL_TEXT);
    if n_effects > 0 {
        let fx = Rect::from_min_size(pos2(x + 52.0, rect.min.y + 25.0), vec2(56.0, 14.0));
        let label = format!("{} {} fx", if look.effects_open { "⏷" } else { "⏵" }, n_effects);
        painter.text(fx.left_center(), Align2::LEFT_CENTER, label, FontId::proportional(10.5), gui_theme::ACCENT);
        let fx_r = ui.interact(fx, id.with("fx"), Sense::click()).on_hover_text("Show or hide the effects (MK-13)");
        out.toggle_effects = fx_r.clicked();
        whole |= fx_r;
    }
    let geo_x = x + 52.0 + if n_effects > 0 { 56.0 } else { 0.0 };
    let geo = geometry_icon::show(ui, id.with("geometry"), geo_x, rect.min.y + 32.0, look.geometry, look.geometry_editing);
    out.edit_geometry = geo.clicked();
    whole |= geo;
    // The view channel's blend and opacity.
    let right = pos2(rect.max.x - 6.0, rect.center().y);
    let blend = look.channel.and_then(|c| l.blend_for(c));
    let text = match blend {
        Some(b) => format!("{}\n{:.0}%", short_mode(b.mode.id()), b.opacity * 100.0),
        None => "—".to_string(),
    };
    painter.text(right, Align2::RIGHT_CENTER, text, FontId::proportional(10.5), gui_theme::LABEL_TEXT);
    if row.clicked() && out.clicked.is_none() {
        out.clicked = Some(Part::Content);
    }
    out.response = Some(whole);
    out
}

/// Which effect a row shows: its layer, stack and index, and whether it can be switched or deleted (not on a locked
/// layer).
pub struct EffectAt<'a> {
    pub owner: &'a str,
    pub stack: StackId,
    pub index: usize,
    pub editable: bool,
}

/// One effect row (`picked`: its properties are being edited); returns (row response, on/off toggled, delete clicked).
pub fn effect_row<D: EffectDomain>(
    ui: &mut Ui,
    rect: Rect,
    depth: usize,
    at: EffectAt,
    e: &materialcraft::paint::Effect<D>,
    picked: bool,
) -> (Response, bool, bool) {
    let EffectAt { owner, stack, index, editable } = at;
    let id = Id::new(("effect_row", owner, stack == StackId::Mask, index));
    let row = ui.interact(rect, id, Sense::click());
    let painter = ui.painter_at(rect);
    let bg = if picked {
        gui_theme::STRIP_SELECTED
    } else if row.hovered() {
        gui_theme::STRIP_HOVER
    } else {
        Color32::from_gray(36)
    };
    painter.rect_filled(rect.shrink2(vec2(0.0, 0.5)), 0.0, bg);
    let x = rect.min.x + EYE + depth as f32 * INDENT;
    let tick = Rect::from_min_size(pos2(x, rect.min.y + 4.0), vec2(14.0, 14.0));
    painter.rect_stroke(tick, 2.0, Stroke::new(1.0, gui_theme::ICON), StrokeKind::Inside);
    if e.enabled {
        painter.rect_filled(tick.shrink(3.5), 1.0, gui_theme::ACCENT);
    }
    let toggled = editable && ui.interact(tick, id.with("on"), Sense::click()).on_hover_text("Switch the effect on or off").clicked();
    let place = if stack == StackId::Mask { "mask" } else { "content" };
    let label = if e.label.is_empty() { effect_name(&e.kind).to_string() } else { format!("{} ({})", e.label, effect_name(&e.kind)) };
    let col = if e.enabled { gui_theme::HEADER_TEXT } else { gui_theme::ICON_DISABLED };
    painter.text(pos2(tick.max.x + 6.0, rect.center().y), Align2::LEFT_CENTER, format!("{label} · {place}"), FontId::proportional(11.5), col);
    if !editable {
        return (row.on_hover_text("The layer is locked: its effects can't be changed"), false, false);
    }
    let del = Rect::from_min_size(pos2(rect.max.x - 20.0, rect.min.y + 3.0), vec2(16.0, EFFECT_ROW - 6.0));
    painter.text(del.center(), Align2::CENTER_CENTER, "×", FontId::proportional(13.0), gui_theme::ICON);
    let deleted = ui.interact(del, id.with("del"), Sense::click()).on_hover_text("Delete the effect").clicked();
    (row, toggled, deleted)
}

fn thumb(ui: &Ui, rect: Rect, texture: Option<&egui::TextureHandle>, focused: bool, mark: &str) {
    let painter = ui.painter_at(rect.expand(2.0));
    checker(&painter, rect);
    if let Some(t) = texture {
        painter.image(t.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    if !mark.is_empty() {
        painter.text(rect.right_bottom() - vec2(2.0, 1.0), Align2::RIGHT_BOTTOM, mark, FontId::proportional(9.0), gui_theme::ACCENT);
    }
    let stroke = if focused { Stroke::new(1.5, gui_theme::ACCENT) } else { Stroke::new(1.0, gui_theme::BORDER) };
    painter.rect_stroke(rect, 1.0, stroke, StrokeKind::Outside);
}

fn checker(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(rect, 0.0, Color32::from_gray(90));
    let s = rect.width() / 4.0;
    for i in 0..4 {
        for j in 0..4 {
            if (i + j) % 2 == 0 {
                let r = Rect::from_min_size(rect.min + vec2(i as f32 * s, j as f32 * s), vec2(s, s));
                painter.rect_filled(r, 0.0, Color32::from_gray(120));
            }
        }
    }
}

/// "fx" on the content thumbnail when its stack has effects (MK-13).
fn kind_mark(l: &Layer) -> &'static str {
    if l.effects.is_empty() { "" } else { "fx" }
}

fn kind_name(l: &Layer) -> &'static str {
    match &l.kind {
        LayerKind::Paint(_) if l.blend.iter().all(|b| b.mode.id() == "passthrough") && !l.blend.is_empty() => "Effect layer",
        LayerKind::Paint(_) => "Paint",
        LayerKind::Fill(_) => "Fill",
        LayerKind::Group(_) => "Folder",
        LayerKind::Instance(_) => "Instance",
    }
}

fn short_mode(id: &str) -> String {
    let mut s: String = id.replace('_', " ");
    if let Some(first) = s.get(0..1) {
        s = first.to_uppercase() + s.get(1..).unwrap_or("");
    }
    s
}

pub fn effect_name<D: EffectDomain>(k: &EffectKind<D>) -> &'static str {
    match k {
        EffectKind::Paint(_) => "Paint",
        EffectKind::Fill(_) => "Fill",
        EffectKind::Generator(_) => "Generator",
        EffectKind::Filter(_) => "Filter",
        EffectKind::Levels(_) => "Levels",
        EffectKind::ColorSelection(_) => "Colour selection",
        EffectKind::PolygonFill(_) => "Polygon fill",
        EffectKind::Anchor(_) => "Anchor",
        EffectKind::Reference(_) => "Reference",
        EffectKind::Unknown(_) => "Unknown",
    }
}
