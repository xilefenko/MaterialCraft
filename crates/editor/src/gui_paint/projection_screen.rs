//! The Paint screen's side of projection editing (roadmap step 38): the projection panel's requests, and each
//! frame the projection gizmo of the selected fill: the viewport's pointer becomes drags (one history step each),
//! what is under the pointer decides whether the next press grabs the gizmo, and the gizmo is drawn over the view.
//! The state lives in [`super::projection_edit::ProjectionEditing`].

use super::PaintMode;
use super::instance_reach::Reach;
use super::panels::PanelEvent;
use super::panels::layers::content_lock::content_editable;
use super::panels::projection::ProjectionEvent;
use super::projection_edit::view::GizmoView;
use super::projection_edit::{GizmoFrame, draw};
use eframe::egui;
use materialcraft::diag::{Area, SinkExt};
use materialcraft::paint::edit::ops::FillRef;
use materialcraft::projection::ProjectionMode;
use std::time::{Duration, Instant};

/// How often the set is recomposited during a gizmo drag.
const REFRESH: Duration = Duration::from_millis(100);

impl PaintMode {
    /// A projection panel request.
    pub(super) fn projection_event(&mut self, ev: ProjectionEvent) {
        match ev {
            ProjectionEvent::Set { target, projection, gesture } => {
                let Some(doc) = &mut self.document else { return };
                match self.projection.set_from_panel(doc, &target, *projection, gesture) {
                    Ok(()) => self.refresh_fill(&target),
                    Err(e) => self.diag.warn(Area::Project, format!("That projection change was not made: {e}.")),
                }
            }
            ProjectionEvent::Source { target, source, gesture } => {
                let Some(doc) = &mut self.document else { return };
                match self.projection.set_source_from_panel(doc, &target, *source, gesture) {
                    Ok(()) => self.refresh_fill(&target),
                    Err(e) => self.diag.warn(Area::Project, format!("That fill change was not made: {e}.")),
                }
            }
            ProjectionEvent::EndGesture => self.projection.end_panel_gesture(self.document.as_mut()),
            ProjectionEvent::Gizmo(settings) => self.projection.settings = settings,
        }
    }

    /// Ends a gizmo drag or panel gesture before `ev` moves in the history or changes the stack under it.
    pub(super) fn end_projection_drag_before(&mut self, ev: &PanelEvent) {
        if matches!(
            ev,
            PanelEvent::Undo | PanelEvent::Redo | PanelEvent::JumpHistory(_) | PanelEvent::Layers(_) | PanelEvent::LayerAction(_) | PanelEvent::Close
        ) {
            self.projection.cancel(self.document.as_mut());
            self.projection.end_panel_gesture(self.document.as_mut());
        }
    }

    /// Recomposites the texture set of `target`, and the sets with instances of its layer (IN-13).
    fn refresh_fill(&mut self, target: &FillRef) {
        if let Some(doc) = &mut self.document
            && let Some(set) = doc.set_index(&target.layer.set)
        {
            let also = Reach::of_layer(doc.doc(), &target.layer).also(doc.doc(), Some(set));
            self.scene.refresh(doc, self.display, std::iter::once(set).chain(also).collect());
        }
    }

    /// This frame's projection gizmo: the viewport's pointer event, what the pointer is over (for the next
    /// press), and drawing. Call after the viewport was drawn.
    pub(super) fn projection_gizmo(&mut self, ctx: &egui::Context) {
        let pointer = self.viewport.gizmo_pointer();
        self.viewport.set_gizmo_hover(false);
        let (Some(doc), Some(camera), Some(rect)) = (self.document.as_mut(), self.viewport.camera(), self.viewport.rect()) else {
            self.projection.cancel(None);
            return;
        };
        // a locked layer's projection can't be edited: no gizmo for it
        let selected = self.layers.selected_fill(doc.doc()).filter(|t| content_editable(doc.doc(), &t.layer));
        let target = self.projection.dragged().cloned().or(selected);
        let Some(target) = target else {
            self.projection.cancel(Some(doc));
            return;
        };
        let mesh = doc.shared_mesh();
        let frame = self.projection.scene_frame(&mesh);
        let uv_mode = target.fill(doc.doc()).is_some_and(|f| matches!(f.projection.mode, ProjectionMode::Uv));
        let surface = match (uv_mode && self.projection.settings.shown, doc.set_index(&target.layer.set)) {
            (true, Some(set)) => self.painting.surface_of(doc, set),
            _ => None,
        };
        let Some(view) = self.picking.projector_on(&mesh.mesh, camera) else { return };
        let f = GizmoFrame { view: &view, frame: frame.as_ref(), surface: surface.as_ref().map(|s| &s.map) };
        let mut refresh = false;
        if let Some(p) = pointer {
            match self.projection.pointer(doc, &target, &f, p) {
                Ok(out) => {
                    let due = self.projection.refreshed.is_none_or(|t: Instant| t.elapsed() >= REFRESH);
                    refresh = (out.changed && due) || out.ended;
                }
                Err(e) => self.diag.warn(Area::Project, format!("The projection was not moved: {e}.")),
            }
            ctx.request_repaint();
        }
        let hover = self.viewport.hover().and_then(|h| self.projection.hover(doc, &target, &f, h.pos));
        let gizmo = GizmoView::build(doc.doc(), &target, &view, frame.as_ref(), f.surface, &self.projection.settings);
        if let (Some(gizmo), Some(painter)) = (gizmo, self.viewport.overlay_painter(ctx)) {
            draw::draw(&gizmo, &painter, rect.min, if self.projection.is_dragging() { None } else { hover });
        }
        self.viewport.set_gizmo_hover(hover.is_some());
        if refresh {
            self.projection.refreshed = Some(Instant::now());
            self.refresh_fill(&target);
        }
    }
}
