//! The bars above and below the layer list: texture set and view channel (LS-09) with the focused layer's
//! blend mode and opacity for that channel above; create, mask, effect, duplicate and delete buttons below
//! (LS-10, LS-12, MK-14).

use super::events::LayerEvent;
use super::menus;
use crate::gui_widgets::SliderRow;
use eframe::egui::{self, Ui};
use materialcraft::history::CoalesceKey;
use materialcraft::paint::edit::ops::NewLayer;
use materialcraft::paint::edit::{LayerRef, StackId};
use materialcraft::paint::{BlendMode, Channel, Document, Layer, TextureSet};

/// A readable channel name.
pub fn channel_label(c: Channel) -> String {
    let id = c.id().replace('_', " ");
    let mut chars = id.chars();
    chars.next().map(|f| f.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
}

/// The set and channel pickers. Returns the chosen set id and channel when they change.
pub fn pickers(ui: &mut Ui, doc: &Document, set: &TextureSet, channel: Option<Channel>) -> (Option<String>, Option<Channel>) {
    let (mut new_set, mut new_channel) = (None, None);
    ui.horizontal(|ui| {
        if doc.texture_sets.len() > 1 {
            egui::ComboBox::from_id_salt("layers_set").width(110.0).selected_text(&set.name).show_ui(ui, |ui| {
                for s in &doc.texture_sets {
                    if ui.selectable_label(s.id == set.id, &s.name).clicked() {
                        new_set = Some(s.id.clone());
                    }
                }
            });
        }
        let shown = channel.map_or_else(|| "—".to_string(), channel_label);
        egui::ComboBox::from_id_salt("layers_channel")
            .width(110.0)
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for spec in &set.channels {
                    if ui.selectable_label(Some(spec.channel) == channel, channel_label(spec.channel)).clicked() {
                        new_channel = Some(spec.channel);
                    }
                }
            })
            .response
            .on_hover_text("The channel the thumbnails show and whose blending is edited here");
    });
    (new_set, new_channel)
}

/// Blend mode and opacity of `layer`'s entry for `channel`. `drag` holds the key of the running drag.
pub fn blending(ui: &mut Ui, r: &LayerRef, layer: &Layer, channel: Channel, drag: &mut Option<CoalesceKey>, next_key: &mut u64, events: &mut Vec<LayerEvent>) {
    let Some(blend) = layer.blend_for(channel).copied() else {
        ui.weak(format!("{} does not affect {}.", layer.name, channel_label(channel)));
        return;
    };
    ui.horizontal(|ui| {
        ui.label("Blend");
        egui::ComboBox::from_id_salt("layers_blend").width(130.0).selected_text(blend.mode.id().replace('_', " ")).show_ui(ui, |ui| {
            for mode in BlendMode::ALL {
                if ui.selectable_label(mode == blend.mode, mode.id().replace('_', " ")).clicked() && mode != blend.mode {
                    events.push(LayerEvent::SetBlendMode { layer: r.clone(), channel, mode });
                }
            }
        });
    });
    let mut percent = blend.opacity * 100.0;
    let resp = SliderRow::new("Opacity", &mut percent, 0.0..=100.0).step(1.0).default_value(100.0).decimals(0).suffix("%").show(ui);
    if resp.began || drag.is_none() && resp.changed {
        *next_key += 1;
        *drag = Some(CoalesceKey(*next_key));
    }
    if resp.changed
        && let Some(key) = *drag
    {
        events.push(LayerEvent::SetOpacity { layer: r.clone(), channel, opacity: percent / 100.0, key });
    }
    if resp.ended {
        *drag = None;
        events.push(LayerEvent::EndGesture);
    }
}

/// The bottom bar. `focus` is the focused layer, `ids` the selection.
pub fn buttons(ui: &mut Ui, set: &str, focus: Option<(&LayerRef, &Layer)>, ids: &[String], events: &mut Vec<LayerEvent>) {
    ui.horizontal_wrapped(|ui| {
        let above = focus.map(|(r, _)| r.layer.clone());
        for (what, label, tip) in [
            (NewLayer::Paint, "+ Paint", "New paint layer above the selection"),
            (NewLayer::Fill, "+ Fill", "New fill layer above the selection"),
            (NewLayer::Folder, "+ Folder", "New empty folder above the selection"),
            (
                NewLayer::SeeThrough,
                "+ See-through",
                "New see-through layer above the selection: smudge and clone work on the layers below it and follow their changes",
            ),
        ] {
            if ui.small_button(label).on_hover_text(tip).clicked() {
                events.push(LayerEvent::Create { set: set.to_string(), what, above: above.clone() });
            }
        }
        let unlocked = focus.is_some_and(|(_, layer)| layer.lock.is_none());
        ui.add_enabled_ui(unlocked, |ui| {
            if let Some((r, layer)) = focus {
                if layer.mask.is_none() {
                    menus::add_mask(ui, r, events);
                }
                menus::add_effect(ui, "+ Effect", r, StackId::Content, events);
            }
        });
        let any = !ids.is_empty();
        if ui.add_enabled(any, egui::Button::new("Duplicate").small()).clicked() {
            events.push(LayerEvent::Duplicate { set: set.to_string(), ids: ids.to_vec() });
        }
        if ui.add_enabled(any, egui::Button::new("Delete").small()).clicked() {
            events.push(LayerEvent::Delete { set: set.to_string(), ids: ids.to_vec() });
        }
    });
}
