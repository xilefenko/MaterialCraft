//! The scrolling layer list: draws only the rows in view, turns clicks into selection changes and row
//! actions, and runs drags: layers dragged inside the list (Ctrl at the drop: copies, LS-13) and assets
//! dragged in from outside (LS-19) show where they would land.

use super::events::LayerEvent;
use super::menus::{self, Local, MenuTarget};
use super::row_ui::{self, EffectAt, LayerLook, mask_span};
use super::rows::{Folding, Landing, RowKind, Rows};
use super::selection::{ClickMods, Selection};
use super::thumbnails::Thumbnails;
use crate::gui_theme;
use eframe::egui::{self, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use materialcraft::paint::edit::ops::{DropPayload, DropTarget, slot_of};
use materialcraft::paint::edit::{LayerRef, LayerSlot, StackId};
use materialcraft::paint::instance::InstanceIndex;
use materialcraft::paint::{Channel, GeometryMask, LayerKind, TextureSet};

/// What one frame of the list works on.
pub struct ListCtx<'a> {
    pub set: &'a TextureSet,
    pub rows: &'a Rows,
    pub channel: Option<Channel>,
    pub height: f32,
    pub external: Option<&'a DropPayload>,
    /// The names of the set's mesh parts (what a geometry mask chooses from).
    pub parts: &'a [String],
    /// The layer whose geometry mask is being edited.
    pub geometry_editing: Option<&'a LayerRef>,
    /// The set with its instances resolved: instance rows show their source's thumbnails.
    pub resolved: &'a TextureSet,
    /// The document's instances (row badges).
    pub instances: &'a InstanceIndex,
    /// The layer copied for "Paste as instance".
    pub copied: Option<&'a LayerRef>,
}

/// Layers being dragged inside the list.
#[derive(Clone, Debug)]
struct Drag {
    ids: Vec<String>,
}

#[derive(Default)]
pub struct ListState {
    renaming: Option<(String, String)>,
    drag: Option<Drag>,
    /// Screen position of the list's first row last frame, and the visible part of the list.
    origin: Option<(Pos2, Rect)>,
    /// A context menu action for the panel to carry out, with the layer it was asked for.
    request: Option<(Local, LayerRef)>,
}

impl ListState {
    pub fn start_rename(&mut self, id: String, name: String) {
        self.renaming = Some((id, name));
    }

    /// The context menu action asked for this frame that the panel carries out (copy, instantiate, go to source).
    pub fn take_request(&mut self) -> Option<(Local, LayerRef)> {
        self.request.take()
    }

    /// The landing under screen position `pos` (for the asset tray).
    pub fn landing_at(&self, rows: &Rows, pos: Pos2) -> Option<Landing> {
        let (origin, clip) = self.origin?;
        clip.contains(pos).then(|| landing(rows, origin, pos)).flatten()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut Ui,
        c: ListCtx,
        selection: &mut Selection,
        folding: &mut Folding,
        thumbs: &mut Thumbnails,
        isolated: Option<&LayerRef>,
        out: &mut Vec<LayerEvent>,
    ) {
        let set = c.set;
        let order = c.rows.layer_order();
        egui::ScrollArea::vertical().id_salt("layers_list").max_height(c.height).auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
            let width = ui.available_width();
            let (all, background) = ui.allocate_exact_size(vec2(width, c.rows.height.max(viewport.height())), Sense::click());
            let origin = all.min;
            self.origin = Some((origin, ui.clip_rect()));
            if background.clicked() {
                selection.clear();
            }
            for i in c.rows.visible(viewport.min.y, viewport.max.y) {
                let Some(row) = c.rows.rows.get(i) else { continue };
                let rect = Rect::from_min_size(origin + vec2(0.0, row.top), vec2(width, row.height));
                match &row.kind {
                    RowKind::Layer { id, open_folder, .. } => {
                        let Some(layer) = set.layer(id) else { continue };
                        let r = LayerRef::new(set.id.clone(), id.clone());
                        let is_isolated = isolated == Some(&r);
                        let geometry_editing = c.geometry_editing == Some(&r);
                        let look = LayerLook {
                            layer,
                            depth: row.depth,
                            open_folder: *open_folder,
                            selected: selection.contains(id),
                            focus: (selection.focus() == Some(id.as_str())).then_some(selection.part),
                            isolated: is_isolated,
                            channel: c.channel,
                            thumbs: thumbs.get(c.resolved.layer(id).unwrap_or(layer)),
                            effects_open: folding.effects_open.contains(id),
                            geometry: GeometryMask::inclusion(layer.geometry_mask.as_ref(), c.parts),
                            geometry_editing,
                            instance: c.instances.role(&set.id, id),
                        };
                        let o = row_ui::layer_row(ui, rect, look);
                        let mods = ui.input(|i| ClickMods { toggle: i.modifiers.command, range: i.modifiers.shift });
                        if let Some(part) = o.clicked {
                            selection.click(id, mods, part, &order);
                        }
                        if o.isolate {
                            out.push(LayerEvent::Isolate(if is_isolated { None } else { Some(r.clone()) }));
                        }
                        if o.edit_geometry {
                            out.push(LayerEvent::EditGeometryMask(if geometry_editing { None } else { Some(r.clone()) }));
                        }
                        if o.toggle_visible {
                            out.push(LayerEvent::SetVisible(r.clone(), !layer.visible));
                        }
                        if o.toggle_fold && !folding.folded.remove(id) {
                            folding.folded.insert(id.clone());
                        }
                        if o.toggle_effects && !folding.effects_open.remove(id) {
                            folding.effects_open.insert(id.clone());
                        }
                        if o.rename {
                            self.renaming = Some((id.clone(), layer.name.clone()));
                        }
                        if let Some(resp) = o.response {
                            if resp.drag_started() {
                                if !selection.contains(id) {
                                    selection.click(id, ClickMods::default(), selection.part, &order);
                                }
                                self.drag = Some(Drag { ids: selection.ids().to_vec() });
                            }
                            let ids = if selection.contains(id) { selection.ids().to_vec() } else { vec![id.clone()] };
                            let role = c.instances.role(&set.id, id);
                            let copied = c.copied.cloned();
                            let target = MenuTarget { layer, r: r.clone(), ids, isolated: is_isolated, geometry_editing, role, copied };
                            resp.context_menu(|ui| match menus::context(ui, &target, out) {
                                Some(Local::Rename) => self.renaming = Some((id.clone(), layer.name.clone())),
                                Some(other) => self.request = Some((other, r.clone())),
                                None => {}
                            });
                        }
                        self.rename_field(ui, rect, row.depth, &r, out);
                    }
                    RowKind::Effect { layer, stack, index } => {
                        let Some(owner) = set.layer(layer) else { continue };
                        let r = LayerRef::new(set.id.clone(), layer.clone());
                        let at = EffectAt { owner: layer, stack: *stack, index: *index, editable: owner.lock.is_none() };
                        let picked = *stack == StackId::Content && selection.focus() == Some(layer.as_str()) && selection.effect() == Some(*index);
                        let shown = match stack {
                            StackId::Content => owner.effects.get(*index).map(|e| row_ui::effect_row(ui, rect, row.depth, at, e, picked)),
                            StackId::Mask => {
                                owner.mask.as_ref().and_then(|m| m.effects.get(*index)).map(|e| row_ui::effect_row(ui, rect, row.depth, at, e, false))
                            }
                        };
                        let enabled = match stack {
                            StackId::Content => owner.effects.get(*index).is_some_and(|e| e.enabled),
                            StackId::Mask => owner.mask.as_ref().and_then(|m| m.effects.get(*index)).is_some_and(|e| e.enabled),
                        };
                        if let Some((resp, toggled, deleted)) = shown {
                            // a content effect clicked is picked for editing (a fill effect's projection)
                            if resp.clicked() && !toggled && !deleted && *stack == StackId::Content {
                                selection.pick_effect(layer, *index);
                            }
                            if toggled {
                                out.push(LayerEvent::SetEffectEnabled { layer: r.clone(), stack: *stack, index: *index, enabled: !enabled });
                            }
                            if deleted {
                                out.push(LayerEvent::RemoveEffect { layer: r, stack: *stack, index: *index });
                            }
                        }
                    }
                }
            }
            self.drags(ui, &c, origin, width, out);
        });
    }

    /// The text field over a row being renamed (LS-21): Enter or leaving the field renames, Escape cancels.
    fn rename_field(&mut self, ui: &mut Ui, rect: Rect, depth: usize, r: &LayerRef, out: &mut Vec<LayerEvent>) {
        let Some((id, buffer)) = &mut self.renaming else { return };
        if *id != r.layer {
            return;
        }
        let x = rect.min.x + mask_span(depth).1 + 4.0;
        let field = Rect::from_min_max(pos2(x, rect.min.y + 3.0), pos2(rect.max.x - 60.0, rect.min.y + 23.0));
        let resp = ui.put(field, egui::TextEdit::singleline(buffer).id_salt("layer_rename"));
        if !resp.has_focus() && !resp.lost_focus() {
            resp.request_focus();
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.renaming = None;
        } else if resp.lost_focus() {
            let name = buffer.trim().to_string();
            if !name.is_empty() {
                out.push(LayerEvent::Rename(r.clone(), name));
            }
            self.renaming = None;
        }
    }

    /// The landing marker of a running drag, and the drop of layers dragged inside the list.
    fn drags(&mut self, ui: &mut Ui, c: &ListCtx, origin: Pos2, width: f32, out: &mut Vec<LayerEvent>) {
        if self.drag.is_none() && c.external.is_none() {
            return;
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.drag = None;
            return;
        }
        let pointer = ui.input(|i| i.pointer.latest_pos());
        let landing = pointer.and_then(|p| landing(c.rows, origin, p));
        let landing = if self.drag.is_some() { landing.map(|l| layer_drop(c.set, c.rows, l)) } else { landing };
        if let Some(l) = &landing {
            let painter = ui.painter();
            match (l.line, l.row.and_then(|i| c.rows.rows.get(i))) {
                (Some(y), _) => {
                    let indent = match &l.target {
                        DropTarget::Between { at, .. } => at.parent.as_ref().map_or(0.0, |_| 14.0),
                        _ => 0.0,
                    };
                    painter.line_segment([pos2(origin.x + indent, origin.y + y), pos2(origin.x + width, origin.y + y)], Stroke::new(2.0, gui_theme::ACCENT));
                }
                (None, Some(row)) => {
                    let r = Rect::from_min_size(origin + vec2(0.0, row.top), vec2(width, row.height));
                    painter.rect_stroke(r.shrink(1.0), 2.0, Stroke::new(2.0, gui_theme::ACCENT), StrokeKind::Inside);
                }
                _ => {}
            }
        }
        let released = ui.input(|i| i.pointer.any_released());
        if released && let Some(drag) = self.drag.take() {
            let copy = ui.input(|i| i.modifiers.command);
            if let Some(to) = landing.and_then(|l| move_slot(c.set, l.target)) {
                out.push(LayerEvent::Move { set: c.set.id.clone(), ids: drag.ids, to, copy });
            }
        }
        if self.drag.is_some() {
            ui.ctx().request_repaint();
        }
    }
}

/// The landing under screen position `pos` of a list whose first row is at `origin`.
fn landing(rows: &Rows, origin: Pos2, pos: Pos2) -> Option<Landing> {
    let y = pos.y - origin.y;
    let on_mask = rows.row_at(y).and_then(|i| rows.rows.get(i)).is_some_and(|r| {
        let (m0, m1) = mask_span(r.depth);
        let x = pos.x - origin.x;
        matches!(r.kind, RowKind::Layer { .. }) && x >= m0 && x < m1
    });
    rows.landing(y, on_mask)
}

/// Layers dragged inside the list onto another layer (not a folder) or its mask land directly above it, and the
/// marker shows that line: a drop (or a Ctrl+drop copy) always does something instead of silently nothing.
pub(super) fn layer_drop(set: &TextureSet, rows: &Rows, l: Landing) -> Landing {
    let (DropTarget::OntoLayer(r) | DropTarget::OntoMask(r)) = &l.target else { return l };
    if matches!(set.layer(&r.layer).map(|x| &x.kind), Some(LayerKind::Group(_))) && matches!(l.target, DropTarget::OntoLayer(_)) {
        return l;
    }
    let (Some(slot), Some(row)) = (slot_of(set, &r.layer), l.row.and_then(|i| rows.rows.get(i))) else { return l };
    let at = LayerSlot { parent: slot.parent, index: slot.index + 1 };
    Landing { target: DropTarget::Between { set: set.id.clone(), at }, line: Some(row.top), row: None }
}

/// Where dropped layers go: between rows as marked, into a folder (on top of its children) when dropped onto
/// it; a drop onto another layer or a mask has no slot (the list makes those a drop above, see [`layer_drop`]).
pub fn move_slot(set: &TextureSet, target: DropTarget) -> Option<LayerSlot> {
    match target {
        DropTarget::Between { at, .. } => Some(at),
        DropTarget::OntoLayer(r) => match &set.layer(&r.layer)?.kind {
            LayerKind::Group(children) => Some(LayerSlot::in_group(r.layer, children.len())),
            _ => None,
        },
        DropTarget::OntoMask(_) => None,
    }
}
