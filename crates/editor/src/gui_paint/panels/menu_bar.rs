//! The Paint screen's menu bar. Entries show their current shortcut from the table.

use super::super::channel_view::ChannelView;
use super::super::reimport;
use super::super::shell::Shell;
use super::super::shell::actions;
use super::super::shell::panels::PaintPanel;
use super::PanelEvent;
use crate::gui_workspace::PanelKind;
use eframe::egui::{self, Color32, Id, Ui};
use materialcraft::history::PaintHistory;
use materialcraft::paint::ImportOrigin;

/// Draws the menu bar; `has_document` enables the document entries, `origin` (the open document's import origin)
/// Re-import from source, `history` is the open document's.
pub fn show(
    ui: &mut Ui,
    title: &str,
    has_document: bool,
    origin: Option<&ImportOrigin>,
    history: Option<&PaintHistory>,
    shell: &Shell,
    events: &mut Vec<PanelEvent>,
) {
    let frame = egui::Frame::NONE.fill(Color32::from_rgb(34, 34, 34)).inner_margin(egui::Margin::symmetric(6, 2));
    egui::Panel::top("paint_menu").frame(frame).show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                item(ui, shell, events, "New project...", "paint.new", true, PanelEvent::NewProject);
                item(ui, shell, events, "Open in Paint mode...", "paint.open", true, PanelEvent::Open);
                item(ui, shell, events, "Import Substance Painter project...", "paint.import", true, PanelEvent::Import);
                reimport_item(ui, origin, events);
                ui.separator();
                item(ui, shell, events, "Save", "paint.save", has_document, PanelEvent::Save);
                item(ui, shell, events, "Save as...", "paint.save_as", has_document, PanelEvent::SaveAs);
                item(ui, shell, events, "Save as project template...", "", has_document, PanelEvent::SaveTemplate);
                ui.separator();
                item(ui, shell, events, "Close document", "paint.close", has_document, PanelEvent::Close);
                ui.separator();
                item(ui, shell, events, "Home screen", "paint.home", true, PanelEvent::Home);
            });
            ui.menu_button("Edit", |ui| {
                let undo = history.and_then(|h| h.undo_label()).map_or("Undo".to_owned(), |l| format!("Undo {l}"));
                let redo = history.and_then(|h| h.redo_label()).map_or("Redo".to_owned(), |l| format!("Redo {l}"));
                item(ui, shell, events, &undo, "paint.edit.undo", history.is_some_and(|h| h.can_undo()), PanelEvent::Undo);
                item(ui, shell, events, &redo, "paint.edit.redo", history.is_some_and(|h| h.can_redo()), PanelEvent::Redo);
            });
            ui.menu_button("View", |ui| {
                item(ui, shell, events, "Reset camera", "paint.view.reset_camera", has_document, PanelEvent::ResetCamera);
                item(ui, shell, events, "Next camera", "paint.view.next_camera", has_document, PanelEvent::NextCamera);
                item(ui, shell, events, "Import cameras…", "", has_document, PanelEvent::ImportCameras);
                item(ui, shell, events, "Show the material", "paint.view.material", has_document, PanelEvent::ShowChannel(ChannelView::Material));
                item(ui, shell, events, "Next channel", "paint.view.next_channel", has_document, PanelEvent::CycleChannel(1));
                item(ui, shell, events, "Previous channel", "paint.view.previous_channel", has_document, PanelEvent::CycleChannel(-1));
                ui.separator();
                if ui.button("Colour picker").clicked() {
                    crate::gui_color::open_unbound(ui.ctx());
                    ui.close();
                }
            });
            ui.menu_button("Window", |ui| {
                for p in PaintPanel::ALL.iter().copied().filter(|p| !p.is_canvas()) {
                    let label = format!("{} {}", if shell.workspace.is_open(p) { "✔" } else { "    " }, p.title());
                    item(ui, shell, events, &label, actions::toggle_action(p), true, PanelEvent::TogglePanel(p));
                }
                ui.separator();
                layouts_menu(ui, shell, events);
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(title).color(Color32::from_gray(170)));
                ui.label(egui::RichText::new("PAINT").strong().color(crate::gui_theme::ACCENT));
            });
        });
    });
}

/// A menu entry with its shortcut.
fn item(ui: &mut Ui, shell: &Shell, events: &mut Vec<PanelEvent>, label: &str, action: &str, enabled: bool, ev: PanelEvent) {
    if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(shell.shortcuts.label(action))).clicked() {
        events.push(ev);
        ui.close();
    }
}

/// File > Re-import from source: enabled for a document imported from a `.spp`, otherwise says why not.
fn reimport_item(ui: &mut Ui, origin: Option<&ImportOrigin>, events: &mut Vec<PanelEvent>) {
    let available = reimport::availability(origin);
    let response = ui.add_enabled(available.is_ok(), egui::Button::new("Re-import from source..."));
    let response = match available {
        Ok(()) => response.on_hover_text("Import the Substance Painter project again with the same choices, as a new document"),
        Err(why) => response.on_disabled_hover_text(why),
    };
    if response.clicked() {
        events.push(PanelEvent::Reimport);
        ui.close();
    }
}

/// Choosing, saving, resetting and deleting named layouts (WS-05).
fn layouts_menu(ui: &mut Ui, shell: &Shell, events: &mut Vec<PanelEvent>) {
    ui.menu_button("Layouts", |ui| {
        for (name, _) in shell.layout_names() {
            let action = format!("paint.layout.{}", name.to_lowercase());
            let key = shell.shortcuts.label(&action);
            if ui.add(egui::Button::selectable(name == shell.layout_name, name.as_str()).shortcut_text(key)).clicked() {
                events.push(PanelEvent::SwitchLayout(name.clone()));
                ui.close();
            }
        }
        ui.separator();
        if ui
            .add(egui::Button::new("Reset layout").shortcut_text(shell.shortcuts.label("paint.layout.reset")))
            .on_hover_text("Back to the layout as it was saved")
            .clicked()
        {
            events.push(PanelEvent::ResetLayout);
            ui.close();
        }
        let is_saved = shell.layout_names().iter().any(|(n, saved)| *saved && *n == shell.layout_name);
        if is_saved && ui.button(format!("Delete \"{}\"", shell.layout_name)).on_hover_text("Built-in layouts come back as they were").clicked() {
            events.push(PanelEvent::DeleteLayout(shell.layout_name.clone()));
            ui.close();
        }
        ui.separator();
        let id = Id::new("paint_layout_name");
        let mut name: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
        ui.horizontal(|ui| {
            let edit = ui.add(egui::TextEdit::singleline(&mut name).hint_text("New layout name").desired_width(140.0));
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Save")).clicked() || enter) && !name.trim().is_empty() {
                events.push(PanelEvent::SaveLayout(std::mem::take(&mut name)));
                ui.close();
            }
        });
        ui.data_mut(|d| d.insert_temp(id, name));
    });
}
