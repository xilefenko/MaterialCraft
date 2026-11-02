//! The layer panel's menus: the row context menu (LS-13, LS-17, LS-21, MK-05/06/08/09) and the add-mask
//! and add-effect menus shared with the bottom bar (MK-02, MK-14).

use super::events::{LayerEvent, SaveAsset};
use super::lock_text;
use eframe::egui::{self, Ui};
use materialcraft::generators;
use materialcraft::paint::edit::ops::{MaskStart, NewEffect};
use materialcraft::paint::edit::{LayerRef, StackId};
use materialcraft::paint::instance::InstanceRole;
use materialcraft::paint::{GeometryMask, Layer, LayerLock};

/// Blockers listed in a locked layer's menu; the blockers panel has them all.
const MAX_REASONS: usize = 8;

/// What the context menu acts on.
pub struct MenuTarget<'a> {
    pub layer: &'a Layer,
    pub r: LayerRef,
    /// The selection the panel operations use (contains the row's layer).
    pub ids: Vec<String>,
    pub isolated: bool,
    /// The geometry mask edit mode is on for this layer.
    pub geometry_editing: bool,
    /// Source, instance or neither.
    pub role: InstanceRole,
    /// The layer copied for "Paste as instance".
    pub copied: Option<LayerRef>,
}

/// Panel actions the menu asks for that the panel itself carries out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Local {
    Rename,
    /// Remember the layer for "Paste as instance" (IN-05).
    Copy,
    /// Open the "Instantiate across texture sets" dialog for the layer (IN-07).
    Instantiate,
    /// Select the instance's source, in its texture set (IN-10).
    GoToSource,
}

pub fn context(ui: &mut Ui, t: &MenuTarget, events: &mut Vec<LayerEvent>) -> Option<Local> {
    let set = t.r.set.clone();
    let mut local = None;
    if ui.button("Rename").clicked() {
        local = Some(Local::Rename);
        ui.close();
    }
    if ui.button("Duplicate").clicked() {
        events.push(LayerEvent::Duplicate { set: set.clone(), ids: t.ids.clone() });
        ui.close();
    }
    if ui.button("Group into folder").clicked() {
        events.push(LayerEvent::Group { set: set.clone(), ids: t.ids.clone() });
        ui.close();
    }
    if ui.button("Save as smart material").on_hover_text("Into your asset library; a single folder is saved with its contents").clicked() {
        events.push(LayerEvent::SaveAsset(SaveAsset::SmartMaterial { set: set.clone(), ids: t.ids.clone() }));
        ui.close();
    }
    if ui.button("Delete").clicked() {
        events.push(LayerEvent::Delete { set: set.clone(), ids: t.ids.clone() });
        ui.close();
    }
    ui.separator();
    if let Some(chosen) = instancing(ui, t, &set, events) {
        local = Some(chosen);
    }
    if matches!(t.role, InstanceRole::Instance | InstanceRole::Broken) {
        return local; // its mask, geometry mask and effects are its source's (IN-02)
    }
    if let Some(lock) = &t.layer.lock {
        locked(ui, t, lock, events);
        return local; // its content, mask and effects can't be edited until it is unlocked
    }
    ui.separator();
    match &t.layer.mask {
        None => add_mask(ui, &t.r, events),
        Some(mask) => {
            ui.menu_button("Mask", |ui| {
                if ui.button(if mask.enabled { "Switch the mask off" } else { "Switch the mask on" }).clicked() {
                    events.push(LayerEvent::SetMaskEnabled(t.r.clone(), !mask.enabled));
                    ui.close();
                }
                if ui.button("Invert the starting value").on_hover_text("Black ↔ white; the mask's effects stay as they are").clicked() {
                    events.push(LayerEvent::InvertMask(t.r.clone()));
                    ui.close();
                }
                let isolate = if t.isolated { "Show the material again" } else { "Show only this mask (Alt+click)" };
                if ui.button(isolate).clicked() {
                    events.push(LayerEvent::Isolate(if t.isolated { None } else { Some(t.r.clone()) }));
                    ui.close();
                }
                add_effect(ui, "Add mask effect", &t.r, StackId::Mask, events);
                if ui.button("Save as smart mask").on_hover_text("The mask with its effects, into your asset library").clicked() {
                    events.push(LayerEvent::SaveAsset(SaveAsset::SmartMask(t.r.clone())));
                    ui.close();
                }
                ui.separator();
                add_mask_items(ui, "Replace with", &t.r, events);
                if ui.button("Remove the mask").on_hover_text("Its effects go with it; Undo brings them back").clicked() {
                    events.push(LayerEvent::RemoveMask(t.r.clone()));
                    ui.close();
                }
            });
        }
    }
    geometry_mask(ui, t, events);
    add_effect(ui, "Add effect", &t.r, StackId::Content, events);
    local
}

/// The items of a locked layer: why it is locked, the mask switch and view, and unlocking.
fn locked(ui: &mut Ui, t: &MenuTarget, lock: &LayerLock, events: &mut Vec<LayerEvent>) {
    ui.separator();
    ui.weak(lock_text::heading(lock));
    for line in lock_text::reasons(lock, MAX_REASONS) {
        ui.label(egui::RichText::new(format!("• {line}")).small());
    }
    if let Some(mask) = &t.layer.mask {
        if ui.button(if mask.enabled { "Switch the mask off" } else { "Switch the mask on" }).clicked() {
            events.push(LayerEvent::SetMaskEnabled(t.r.clone(), !mask.enabled));
            ui.close();
        }
        let isolate = if t.isolated { "Show the material again" } else { "Show only this mask (Alt+click)" };
        if ui.button(isolate).clicked() {
            events.push(LayerEvent::Isolate(if t.isolated { None } else { Some(t.r.clone()) }));
            ui.close();
        }
    }
    let tip = "Edit the layer anyway. Its content was kept as imported and may not match the source after edits; Undo locks it again";
    if ui.button("Unlock for editing").on_hover_text(tip).clicked() {
        events.push(LayerEvent::Unlock(t.r.clone()));
        ui.close();
    }
}

/// Copy, paste as instance, instantiate across texture sets and go to source (IN-05, IN-07, IN-10).
fn instancing(ui: &mut Ui, t: &MenuTarget, set: &str, events: &mut Vec<LayerEvent>) -> Option<Local> {
    let mut local = None;
    if ui.button("Copy").on_hover_text("To paste as an instance (Ctrl+Shift+V)").clicked() {
        local = Some(Local::Copy);
        ui.close();
    }
    let paste = ui.add_enabled(t.copied.is_some(), egui::Button::new("Paste as instance")).on_hover_text("An instance of the copied layer above this one");
    if paste.clicked()
        && let Some(source) = &t.copied
    {
        events.push(LayerEvent::PasteInstance { source: source.clone(), set: set.to_string(), above: Some(t.r.layer.clone()) });
        ui.close();
    }
    if t.role != InstanceRole::Broken && ui.button("Instantiate across texture sets…").clicked() {
        local = Some(Local::Instantiate);
        ui.close();
    }
    if t.role == InstanceRole::Instance && ui.button("Go to source").clicked() {
        local = Some(Local::GoToSource);
        ui.close();
    }
    local
}

/// The geometry mask submenu (GM-03, GM-04): edit mode, include all, exclude all.
fn geometry_mask(ui: &mut Ui, t: &MenuTarget, events: &mut Vec<LayerEvent>) {
    ui.menu_button("Geometry mask", |ui| {
        let edit = if t.geometry_editing { "Finish editing" } else { "Choose parts…" };
        if ui.button(edit).on_hover_text("Pick parts in the viewport or from a list").clicked() {
            events.push(LayerEvent::EditGeometryMask(if t.geometry_editing { None } else { Some(t.r.clone()) }));
            ui.close();
        }
        if ui.add_enabled(t.layer.geometry_mask.is_some(), egui::Button::new("Include all")).clicked() {
            events.push(LayerEvent::SetGeometryMask(t.r.clone(), None));
            ui.close();
        }
        if ui.add_enabled(t.layer.geometry_mask != Some(GeometryMask::nothing()), egui::Button::new("Exclude all")).clicked() {
            events.push(LayerEvent::SetGeometryMask(t.r.clone(), Some(GeometryMask::nothing())));
            ui.close();
        }
    });
}

/// "Add mask" with its starting choices.
pub fn add_mask(ui: &mut Ui, r: &LayerRef, events: &mut Vec<LayerEvent>) {
    add_mask_items(ui, "Add mask", r, events);
}

fn add_mask_items(ui: &mut Ui, title: &str, r: &LayerRef, events: &mut Vec<LayerEvent>) {
    ui.menu_button(title, |ui| {
        for start in MaskStart::ALL {
            if ui.button(start.label()).clicked() {
                events.push(LayerEvent::AddMask(r.clone(), start));
                ui.close();
            }
        }
        for (id, label) in generators::MASK_STARTS {
            if ui.button(*label).clicked() {
                events.push(LayerEvent::AddMask(r.clone(), MaskStart::Generator(id)));
                ui.close();
            }
        }
    });
}

/// An add-effect submenu for one stack.
pub fn add_effect(ui: &mut Ui, title: &str, r: &LayerRef, stack: StackId, events: &mut Vec<LayerEvent>) {
    ui.menu_button(title, |ui| {
        for what in NewEffect::ALL {
            if ui.button(what.label()).clicked() {
                events.push(LayerEvent::AddEffect { layer: r.clone(), stack, what });
                ui.close();
            }
        }
        ui.weak("Generators and filters: drag them from the assets.");
    });
}
