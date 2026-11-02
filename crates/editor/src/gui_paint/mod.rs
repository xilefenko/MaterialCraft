//! Paint mode screen (milestone M4: view an imported Substance Painter project).
//!
//! [`PaintMode`] is the only type the rest of the editor sees. It wires small components together:
//! [`loader::Loader`] reads documents (and imports `.spp` in-process), [`scene_sync::SceneSync`]
//! keeps the GPU scene in step with the document, [`viewport::Viewport`] draws it, the
//! [`shell::Shell`] holds the docking workspace, shortcuts and settings, and the [`panels`] turn
//! clicks and key presses into [`panels::PanelEvent`]s that are applied here ([`documents`]: opening and
//! closing; [`editing`]: stack edits and history moves; [`tray_host`]: the asset tray's drops and settings). The
//! colour picker ([`crate::gui_color::ColorPicker`]) floats above the screen; [`eyedrop`] answers its viewport
//! eyedropper.

mod axis_gizmo;
mod bake_editor;
mod bake_queue;
mod baking;
mod brush_drop;
mod brush_presets;
mod cache;
mod camera_files;
mod channel_display;
mod channel_view;
mod demo;
mod dialogs;
mod display_prefs;
mod document_state;
mod documents;
mod editing;
mod environment;
mod eyedrop;
mod fill_surfaces;
mod geometry;
mod geometry_mask_edit;
mod import_review;
mod input;
mod instance_reach;
mod isolate;
mod job;
mod launch;
mod library_import;
mod loader;
mod maps_patch;
mod maps_queue;
mod maps_worker;
mod navigation;
mod new_project;
mod occlusion_check;
mod panels;
pub mod perf;
mod projection_edit;
mod projection_screen;
mod quick_mask_tint;
mod quick_tools;
mod reimport;
mod reveal;
mod save_point;
mod saving;
mod scene_shading;
mod scene_stats;
mod scene_sync;
mod screen_images;
mod shading_screen;
mod shared_mesh;
pub mod shell;
mod shot;
mod smart_assets;
mod spp_import;
mod stack_editor;
mod stroke_test;
mod strokes;
mod tools;
mod tray_host;
mod unsaved;
mod uv_outlines;
mod uv_screen;
mod uv_view;
mod view2d;
mod view_cameras;
mod view_pick;
mod view_state;
mod view_thumbnail;
mod viewport;
mod window;

pub use input::window_system::WindowChoice;

use crate::gui_assets::AssetTray;
use crate::gui_color::ColorPicker;
use crate::gui_settings::UserSettings;
use baking::Baking;
use channel_view::ChannelView;
use document_state::OpenDocument;
use eframe::egui;
use launch::{Launch, StartDocument};
use loader::{Loaded, Loader};
use materialcraft::diag::{Area, RingBuffer, Sink, SinkExt, Target};
use materialcraft::projection::CpuViewProjector;
use occlusion_check::OcclusionCheck;
use panels::PanelEvent;
use panels::bakers::BakersPanel;
use panels::import_report::ReportPanel;
use panels::layers::LayerPanel;
use panels::layout::Screen;
use panels::log::LogPanel;
use panels::status_bar::Activity;
use reveal::{Destination, Focus};
use scene_sync::SceneSync;
use shell::Shell;
use shell::panels::PaintPanel;
use shot::ShotJob;
use std::sync::Arc;
use view_pick::ViewPicking;
use viewport::Viewport;
use window::WindowTitle;

/// What the editor should do after a Paint-screen frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaintAction {
    Stay,
    /// Go back to the home screen.
    Home,
}

/// Which files a file dialog offers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    /// `.mcpaint` documents and `.spp` projects.
    Any,
    /// Substance Painter projects only.
    Substance,
}

/// All state of the Paint screen.
pub struct PaintMode {
    document: Option<OpenDocument>,
    loader: Option<Loader>,
    /// A `.spp` waiting for the import review.
    review: Option<import_review::ImportReview>,
    scene: SceneSync,
    viewport: Viewport,
    display: ChannelView,
    report: ReportPanel,
    log: LogPanel,
    /// The texture set a log entry led to, highlighted for a moment.
    focus: Option<Focus>,
    layers: LayerPanel,
    /// The asset tray (step 15); it works with or without a document.
    tray: AssetTray,
    bakers: BakersPanel,
    /// The running bake and waiting auto-rebakes.
    baking: Baking,
    /// Where status messages and errors go (the log file and the session history).
    diag: Arc<dyn Sink>,
    /// Docking, shortcuts, tools and the user settings they live in.
    shell: Shell,
    shot: Option<ShotJob>,
    /// Screen ↔ mesh for the tools: the view projector's per-mesh scene, kept in step with the document.
    picking: ViewPicking,
    check: Option<OcclusionCheck>,
    title: WindowTitle,
    /// The floating colour picker and its eyedropper (step 21).
    color: ColorPicker,
    /// The view's frame count when the eyedropper asked the GPU for a displayed colour, and whether it asked the 2D
    /// view.
    color_read: Option<(u64, bool)>,
    /// The brush tools: settings, the stroke in progress, the painted set's surface.
    painting: tools::Painting,
    /// `--stroke-test`: paint one stroke once the document has settled.
    stroke_test: Option<stroke_test::StrokeTest>,
    /// The running save and the "Save changes?" question.
    saving: unsaved::Saving,
    /// Naming a new smart material or mask; asking whether to bake the maps a dropped one needs.
    smart: smart_assets::SmartAssets,
    /// Naming a brush preset saved from the brush panel (AS-06).
    brush_presets: brush_presets::BrushPresets,
    /// The New project dialog and naming a project template (step 50).
    new_projects: new_project::NewProjects,
    /// Writing the project assets the import review ticked into the user's library (step 46).
    library_imports: library_import::LibraryImports,
    /// File > Re-import from source: its question and the locks it compares with.
    reimports: reimport::Reimports,
    /// Editing the selected fill's projection: the view gizmo and the panel's gestures (step 38).
    projection: projection_edit::ProjectionEditing,
    /// The shader settings panel's gesture and where its shader files come from (step 41).
    shader_panel: shading_screen::ShaderPanelState,
    /// The platform's pen reader (step 35), kept alive while the screen exists.
    tablet: input::tablet::TabletInput,
    /// The island outlines of the set the 2D view shows.
    uv_outlines: uv_outlines::UvOutlines,
}

impl PaintMode {
    /// Paint mode configured from the command line (`--paint`, `--paint-demo`, `--shot`, `--channel`);
    /// its messages go to `diag`, its workspace is kept in `settings`.
    pub fn from_args(diag: Arc<dyn Sink>, settings: UserSettings) -> PaintMode {
        let launch = Launch::from_args(std::env::args().skip(1));
        let (tray, first_run) = AssetTray::new(&settings.get().library, settings.dir(), diag.clone());
        let prefs = &settings.get().display;
        let mut state = display_prefs::state_from(prefs);
        state.layout = launch.layout.unwrap_or(state.layout);
        let viewport = Viewport::new(diag.clone(), state, display_prefs::opts_from(prefs));
        if let Some(note) = first_run {
            diag.info(Area::Assets, note);
        }
        let mut pm = PaintMode {
            document: None,
            loader: None,
            review: None,
            scene: SceneSync::default(),
            viewport,
            display: launch.channel.unwrap_or(ChannelView::Material),
            report: ReportPanel::default(),
            log: LogPanel::default(),
            focus: None,
            layers: LayerPanel::default(),
            tray,
            bakers: BakersPanel::default(),
            baking: Baking::default(),
            shell: Shell::new(settings, diag.clone()),
            diag,
            shot: launch.shot.map(ShotJob::new),
            picking: ViewPicking::default(),
            check: launch.occlusion_check.map(OcclusionCheck::new),
            title: WindowTitle::default(),
            color: ColorPicker::default(),
            color_read: None,
            painting: tools::Painting::default(),
            stroke_test: launch.stroke_test.then_some(stroke_test::StrokeTest::Waiting),
            saving: unsaved::Saving::default(),
            smart: smart_assets::SmartAssets::default(),
            brush_presets: brush_presets::BrushPresets::default(),
            new_projects: new_project::NewProjects::default(),
            library_imports: library_import::LibraryImports::default(),
            reimports: reimport::Reimports::default(),
            tablet: input::tablet::TabletInput::none(),
            projection: projection_edit::ProjectionEditing::default(),
            shader_panel: shading_screen::ShaderPanelState::default(),
            uv_outlines: uv_outlines::UvOutlines::default(),
        };
        if launch.stroke_test && launch.stroke_helpers {
            pm.painting.brush.helpers = stroke_test::helpers();
        }
        match launch.open {
            Some(StartDocument::File(p)) => pm.open(p),
            Some(StartDocument::Demo) => pm.show(Loaded { document: demo::document(), source: "demo".into(), summary: None, notes: Vec::new() }),
            Some(StartDocument::NewProject(mesh)) => {
                let settings = materialcraft::paint::new_project::NewProject::new(mesh, Default::default());
                pm.loader = Some(Loader::start_new(settings));
            }
            None => {}
        }
        pm
    }

    /// Starts the platform's pen reader for the window eframe created (once, from the app's creation); `window`
    /// is where the window was asked to open. Pen pressure that isn't available is a warning (the status bar
    /// shows it), not a silent mouse.
    pub fn connect_tablet(&mut self, cc: &eframe::CreationContext<'_>, window: WindowChoice) {
        self.tablet = input::tablet::TabletInput::start(cc, &self.viewport.pen_feed(), window);
        self.diag.info(Area::General, window.describe().to_string());
        let summary = self.tablet.status.summary();
        if self.tablet.status.available() {
            self.diag.info(Area::General, summary);
        } else {
            self.diag.warn(Area::General, summary);
        }
    }

    /// True if a document was given on the command line (the editor then starts on this screen).
    pub fn wants_screen(&self) -> bool {
        self.document.is_some() || self.loader.is_some()
    }

    /// Draws the screen and handles its input; `history` is the session's diagnostics (the status
    /// bar shows the newest entry).
    pub fn ui(&mut self, ui: &mut egui::Ui, history: &RingBuffer) -> PaintAction {
        let ctx = &ui.ctx().clone();
        self.shell.frame(ctx);
        self.viewport.set_pen(&self.shell.settings.get().pen);
        if let Some(p) = window::dropped_document(ctx) {
            self.guard(unsaved::Then::OpenPath(p));
        }
        self.poll_review(ctx);
        self.poll_library_imports(ctx);
        self.poll_loader();
        self.poll_save();
        self.poll_bake();
        self.picking.poll(self.document.as_ref());
        self.sync_geometry_editing();
        if let Some(doc) = &mut self.document {
            self.scene.poll(doc, self.viewport.gpu(), &*self.diag);
            if let Some(e) = self.scene.error.take() {
                self.diag.error(Area::Gpu, e);
            }
        }
        let env_busy = self.viewport.sync_environment(&self.tray.material_folders());
        self.sync_uv_view(ctx);
        let mut events = self.shell.take_shortcuts(ctx);
        self.before_tray_frame();
        events.extend(self.draw(ui, history));
        events.extend(self.geometry_mask_window(ctx));
        events.extend(self.end_tray_frame(ctx));
        self.paint_strokes(ctx);
        let mut action = PaintAction::Stay;
        for ev in events {
            if self.apply(ev) == PaintAction::Home {
                action = PaintAction::Home;
            }
        }
        self.color.frame(ctx, &mut self.shell.settings);
        display_prefs::sync(&mut self.shell.settings, self.viewport.state(), &self.viewport.opts());
        if let Some(doc) = &mut self.document {
            self.scene.set_channel_display(doc, self.display, self.viewport.state().channel);
        }
        self.eyedrop(ctx);
        self.poll_smart_assets(ctx);
        self.poll_brush_presets(ctx);
        self.poll_new_project(ctx);
        self.poll_reimport(ctx);
        self.poll_unsaved(ctx);
        let name = self.document.as_ref().map(|d| d.title());
        self.title.update(ctx, name.as_deref());
        if self.loader.is_some() || self.scene.is_busy() || self.baking.is_busy() || self.is_saving() || env_busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        let settled = self.loader.is_none() && self.document.is_some() && !self.scene.is_busy() && self.picking.is_ready();
        let tested = self.run_stroke_test(settled);
        if let Some(shot) = &mut self.shot {
            let settled = tested && self.loader.is_none() && self.document.is_some() && !self.scene.is_busy() && !self.viewport.refining();
            shot.update(ctx, settled, self.viewport.frames(), self.viewport.rect());
        }
        if let Some(check) = &mut self.check {
            let settled = self.loader.is_none() && !self.scene.is_busy() && self.picking.is_ready();
            let frames = self.viewport.frames();
            let (doc, picking) = (self.document.as_ref().map(|d| d.doc()), &self.picking);
            let (state, gpu) = self.viewport.settings_mut();
            check.update(ctx, settled, frames, gpu, &mut state.view, doc, |camera| doc.and_then(|d| picking.projector(d, camera)));
        }
        action
    }

    /// Screen ↔ mesh through this frame's 3D view, for the tools: screen positions in UI points from the view's
    /// top-left corner (`Viewport::rect`). `None` without a document, before the view was laid out, or while the
    /// scene is being built.
    fn view_projector(&self) -> Option<CpuViewProjector<'_>> {
        let doc = self.document.as_ref()?;
        self.picking.projector(doc.doc(), self.viewport.camera()?)
    }

    fn draw(&mut self, ui: &mut egui::Ui, history: &RingBuffer) -> Vec<PanelEvent> {
        let focus_set = self.focus.as_mut().and_then(Focus::frame);
        if focus_set.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        } else {
            self.focus = None;
        }
        self.layers.set_fill_surfaces(self.scene.fill_surfaces());
        let screen = Screen {
            document: self.document.as_ref(),
            report: &mut self.report,
            layers: &mut self.layers,
            viewport: &mut self.viewport,
            display: self.display,
            history,
            log: &mut self.log,
            tray: &mut self.tray,
            focus_set,
            activity: Activity {
                loading: self.loader.as_ref().map(|l| l.progress()),
                maps: self.scene.progress(),
                bake: self.baking.progress(),
                saving: self.saving.job.as_ref().map(|j| j.progress()),
            },
            bakers: &mut self.bakers,
            shell: &mut self.shell,
            brush: &mut self.painting.brush,
            images: &self.painting.images,
            quick_mask: self.painting.quick_mask.state(),
            projection: &self.projection.settings,
            shaders: self.scene.shader_library(),
            model: self.scene.model_scale(),
            pen_status: &self.tablet.status,
        };
        panels::layout::show(ui, screen)
    }

    fn apply(&mut self, ev: PanelEvent) -> PaintAction {
        self.end_stroke_before(&ev);
        self.end_projection_drag_before(&ev);
        if ev == PanelEvent::PreviousTool {
            self.apply_quick_mask();
        }
        let can_paint = self.document.as_ref().is_some_and(|d| self.layers.paintable(d.doc()));
        let Some(ev) = self.shell.apply(ev, can_paint) else {
            return PaintAction::Stay;
        };
        match ev {
            PanelEvent::NewProject => self.guard(unsaved::Then::NewProject),
            PanelEvent::SaveTemplate => self.save_template(),
            PanelEvent::Open => self.guard(unsaved::Then::Open(Pick::Any)),
            PanelEvent::Import => self.guard(unsaved::Then::Open(Pick::Substance)),
            PanelEvent::Reimport => self.ask_reimport(),
            PanelEvent::Close => self.guard(unsaved::Then::Close),
            PanelEvent::Save => self.save(None),
            PanelEvent::SaveAs => self.save_as(None),
            PanelEvent::Home => {
                self.title.forget(); // the editor sets its own title meanwhile
                return PaintAction::Home;
            }
            PanelEvent::ResetCamera => self.viewport.reset_camera(),
            PanelEvent::ToggleProjection => self.viewport.toggle_projection(),
            PanelEvent::SelectCamera(i) => self.viewport.settings_mut().0.select_camera(i),
            PanelEvent::NextCamera => self.viewport.settings_mut().0.next_camera(),
            PanelEvent::ImportCameras => self.import_cameras(),
            PanelEvent::UseDisplayDefaults => {
                display_prefs::save_project_defaults(&mut self.shell.settings, self.viewport.state());
                self.diag.info(Area::General, "New documents will start with this camera, exposure and environment.");
            }
            PanelEvent::ShowChannel(view) => self.show_channel(view),
            PanelEvent::ViewLayout(layout) => self.viewport.settings_mut().0.layout = layout,
            PanelEvent::CycleChannel(step) => self.show_channel(self.display.cycled(step)),
            PanelEvent::SetVisible(set, visible) => {
                if let Some(doc) = &mut self.document {
                    doc.set_visible(set, visible);
                    self.scene.rebuild(doc, self.display);
                }
            }
            PanelEvent::Undo | PanelEvent::Redo if self.quick_mask_step(ev == PanelEvent::Undo) => {}
            PanelEvent::Tool(ev) => self.tool_event(ev),
            PanelEvent::Undo => self.move_in_history("Undo", |d| d.undo().map(|_| ())),
            PanelEvent::Redo => self.move_in_history("Redo", |d| d.redo().map(|_| ())),
            PanelEvent::JumpHistory(done) => self.move_in_history("Going back in the history", |d| d.jump_to(done)),
            PanelEvent::Layers(ev) => self.edit_layers(ev),
            PanelEvent::LayerAction(action) => {
                if let Some(ev) = self.document.as_ref().and_then(|d| self.layers.action(d.doc(), action)) {
                    self.edit_layers(ev);
                }
            }
            PanelEvent::CancelLoad => {
                self.loader = None;
                self.diag.info(Area::Project, "Cancelled.");
            }
            PanelEvent::Reveal(target) => self.reveal(&target),
            PanelEvent::Assets(ev) => self.apply_tray(ev),
            PanelEvent::Bakers(ev) => self.bakers_event(ev),
            PanelEvent::Projection(ev) => self.projection_event(ev),
            PanelEvent::Shader(ev) => self.shader_event(ev),
            // Workspace events were handled by the shell.
            _ => {}
        }
        PaintAction::Stay
    }

    /// Goes to what a log entry concerns (LG-10).
    fn reveal(&mut self, target: &Target) {
        let set = match reveal::resolve(target, self.document.as_ref()) {
            Destination::TextureSet(set) => set,
            Destination::Layer { set, layer } => {
                self.reveal_layer(set, &layer);
                set
            }
            Destination::BakeSetting { set, field } => {
                self.bakers.reveal(set, &field);
                if !self.shell.workspace.is_open(PaintPanel::Bakers)
                    && let Err(e) = self.shell.workspace.open(PaintPanel::Bakers)
                {
                    self.diag.warn(Area::General, format!("The bakers panel can't be shown: {e}."));
                }
                return;
            }
            Destination::Nowhere(why) => {
                self.diag.info(Area::General, why);
                return;
            }
        };
        if !self.shell.workspace.is_open(PaintPanel::TextureSets)
            && let Err(e) = self.shell.workspace.open(PaintPanel::TextureSets)
        {
            self.diag.warn(Area::General, format!("The texture set list can't be shown: {e}."));
        }
        self.focus = Some(Focus::new(set));
    }

    /// Selects `layer` of texture set `set` in the layer panel, unfolding its folders, and shows the panel.
    fn reveal_layer(&mut self, set: usize, layer: &str) {
        let Some(s) = self.document.as_ref().and_then(|d| d.doc().texture_sets.get(set)) else { return };
        self.layers.reveal(s, layer);
        if !self.shell.workspace.is_open(PaintPanel::Layers)
            && let Err(e) = self.shell.workspace.open(PaintPanel::Layers)
        {
            self.diag.warn(Area::General, format!("The layer panel can't be shown: {e}."));
        }
    }

    fn show_channel(&mut self, view: ChannelView) {
        self.display = view;
        if let Some(doc) = &mut self.document {
            self.scene.remap(doc, view);
        }
    }

    /// The interface scale the user chose (applied to the whole editor at start-up).
    pub fn ui_scale(&self) -> f32 {
        self.shell.ui_scale()
    }

    /// On exit: offers to save unsaved changes, then saves the workspace and settings.
    pub fn flush(&mut self) {
        self.save_on_exit();
        self.shell.flush();
    }
}
