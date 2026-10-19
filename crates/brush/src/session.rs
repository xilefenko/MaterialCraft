//! [`StrokeSession`]: one stroke from pen down to pen up: input points become dabs (the engine adapter), dabs land
//! in the target images (the core's stamp stage), and the samples become the stroke's record.
//!
//! The session owns only its own state; what it paints through ([`SessionScene`]: the surface map, the view, the
//! factors) and into (the target images) is lent to each call, so a session can live across editor frames while
//! the document changes hands in between.

use crate::BrushError;
use crate::engine::{DabEngine, EngineExtras, InputPoint, SmudgeKernel};
use crate::symmetry::copies;
use crate::target::PaintValues;
use materialcraft::camera::Camera;
use materialcraft::mesh::{Mesh, Sphere, SurfaceMap};
use materialcraft::paint::brush::{BrushPreset, RecordedBrush, Retouch, SizeSpace};
use materialcraft::paint::stroke::{self, Stroke, StrokeSample, StrokeTool, Symmetry, ViewKind};
use materialcraft::projection::ViewProjector;
use materialcraft::stamp::{
    BlurKernel, ChannelTarget, DabSample, DepthMap, LocalKernel, ProjectedPaint, StampContext, StampMode, StampStroke, StrokeSpace, TexelFactor, TexelIndex,
    ViewSpace, screen_scale_at,
};

/// What a session paints through, lent per call.
#[derive(Clone, Copy)]
pub struct SessionScene<'a> {
    pub surface: &'a SurfaceMap,
    pub index: &'a TexelIndex,
    /// The 3D view (for [`ViewKind::Mesh`] strokes; ignored for 2D-view strokes). The camera must stay the same
    /// during a stroke.
    pub projector: Option<&'a dyn ViewProjector>,
    pub factors: &'a [&'a dyn TexelFactor],
    /// Projection painting (PJ-26): what the stroke lays down per texel, and that it stays inside the image (added
    /// to `factors`). The targets must be built with it ([`crate::PaintTarget::projected_targets`]).
    pub projected: Option<&'a ProjectedPaint<'a>>,
}

/// How a stroke is painted.
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeSetup {
    pub preset: BrushPreset,
    /// [`StrokeTool::Paint`], [`StrokeTool::Eraser`] or a retouch tool ([`StrokeTool::Smudge`], [`StrokeTool::Blur`],
    /// [`StrokeTool::Clone`]); the targets' mode must match ([`crate::target::mode_for`]).
    pub tool: StrokeTool,
    /// Seed of the brush's random variation.
    pub seed: u32,
    /// Symmetric copies in the record's form ([`crate::symmetry::to_record`] of the settings' spec), for 3D-view
    /// and 2D-view strokes alike; recorded with the stroke as it is.
    pub symmetry: Symmetry,
    /// 3D view (screen positions) or 2D view (texel positions).
    pub view: ViewKind,
    /// The values painted (recorded with the stroke).
    pub values: PaintValues,
    /// The mesh's bounding sphere in object space: model-sized brushes are a fraction of its radius.
    pub model: Sphere,
    /// Test occlusion (3D view).
    pub occlusion: bool,
    /// Smudge and blur strength, clone offset (recorded with the stroke).
    pub retouch: Retouch,
}

/// A finished stroke.
#[derive(Debug, Clone, PartialEq)]
pub struct Finished {
    /// The stroke as recorded (to append to the target's stroke record).
    pub stroke: Stroke,
    /// Every image tile the stroke changed, as `(tx, ty)`.
    pub touched: Vec<(u32, u32)>,
    /// Dabs placed.
    pub dabs: usize,
}

/// One stroke in progress. See the module docs.
pub struct StrokeSession {
    setup: StrokeSetup,
    preset: BrushPreset,
    /// What the engine takes beyond the preset (imported brushes).
    extras: EngineExtras,
    engine: Option<DabEngine>,
    stamp: StampStroke,
    depth: Option<DepthMap>,
    /// The camera of a 3D-view stroke (recorded; samples are stored in its normalised device coordinates).
    camera: Option<Camera>,
    /// Screen units per object unit where the stroke started (model-sized brushes).
    start_scale: Option<f32>,
    last_scale: f32,
    samples: Vec<StrokeSample>,
    dabs: Vec<DabSample>,
    placed: usize,
    /// Per target, whether it runs a local kernel (smudge, blur).
    local: Vec<bool>,
}

impl StrokeSession {
    /// Starts a stroke into `targets` (one texture set's images). For a 3D-view stroke `scene.projector` must be
    /// set; with occlusion on, the view's depth is drawn here from the parts of `mesh` that `shown` accepts.
    pub fn begin(
        setup: StrokeSetup,
        scene: &SessionScene,
        mesh: &Mesh,
        shown: &dyn Fn(usize) -> bool,
        targets: &[ChannelTarget],
    ) -> Result<StrokeSession, BrushError> {
        // 2D-view strokes are mirrored through the mesh as well (ADR 0008 mc2-q-pt-04c)
        let stamp = StampStroke::with_copies(copies(&setup.symmetry), targets)?;
        let local = targets.iter().map(|t| t.mode == StampMode::Local).collect();
        let (camera, depth) = match setup.view {
            ViewKind::Mesh => {
                let projector = scene.projector.ok_or(BrushError::NoView)?;
                let depth = setup.occlusion.then(|| DepthMap::build(mesh, projector, shown));
                (Some(*projector.camera()), depth)
            }
            ViewKind::Uv => (None, None),
        };
        let preset = setup.preset.clamped();
        Ok(StrokeSession {
            setup,
            preset,
            extras: EngineExtras::default(),
            engine: None,
            stamp,
            depth,
            camera,
            start_scale: None,
            last_scale: 1.0,
            samples: Vec::new(),
            dabs: Vec::new(),
            placed: 0,
            local,
        })
    }

    /// The kernels of a smudge or blur stroke, one per target that runs one, for dabs about `diameter` across.
    fn kernels(&self, diameter: f32) -> Vec<Option<Box<dyn LocalKernel>>> {
        let strength = self.setup.retouch.strength;
        let tool = self.setup.tool;
        let kernel = || -> Option<Box<dyn LocalKernel>> {
            match tool {
                // room for dabs that grow along the stroke (model-sized brushes, pressure)
                StrokeTool::Smudge => Some(Box::new(SmudgeKernel::new(strength, diameter * 2.0))),
                StrokeTool::Blur => Some(Box::new(BlurKernel { strength })),
                _ => None,
            }
        };
        self.local.iter().map(|&l| if l { kernel() } else { None }).collect()
    }

    /// Builder, before the first [`StrokeSession::add`]: paint with `extras` (a sampled tip, pressure minimums).
    pub fn with_extras(mut self, extras: EngineExtras) -> StrokeSession {
        self.extras = extras;
        self
    }

    /// The stroke's camera (3D-view strokes).
    pub fn camera(&self) -> Option<Camera> {
        self.camera
    }

    /// Every image tile changed so far.
    pub fn touched(&self) -> Vec<(u32, u32)> {
        self.stamp.touched()
    }

    /// Adds input points (positions in the stroke's frame, times in seconds since the stroke started); paints the
    /// dabs they complete. Returns the image tiles changed.
    pub fn add(&mut self, scene: &SessionScene, points: &[InputPoint], targets: &mut [ChannelTarget]) -> Result<Vec<(u32, u32)>, BrushError> {
        let mut scaled = Vec::with_capacity(points.len());
        for p in points {
            let scale = self.scale_at(scene, p.pos);
            if self.engine.is_none() {
                let diameter = self.diameter(scene);
                self.engine = Some(DabEngine::with_extras(&self.preset, &self.extras, diameter, u64::from(self.setup.seed)));
                if self.local.iter().any(|l| *l) {
                    let kernels = self.kernels(diameter);
                    self.stamp.set_kernels(kernels);
                }
            }
            self.samples.push(self.sample(scene, p));
            scaled.push(InputPoint { scale, ..*p });
        }
        let mut dabs = std::mem::take(&mut self.dabs);
        if let Some(engine) = &mut self.engine {
            engine.push(&scaled, &mut dabs);
        }
        let out = self.paint(scene, &dabs, targets);
        dabs.clear();
        self.dabs = dabs;
        out
    }

    /// Dabs placed so far.
    pub fn dabs(&self) -> usize {
        self.placed
    }

    /// Ends the stroke: paints its last dabs and returns it as recorded.
    pub fn finish(mut self, scene: &SessionScene, targets: &mut [ChannelTarget]) -> Result<Finished, BrushError> {
        let mut dabs = Vec::new();
        if let Some(engine) = &mut self.engine {
            engine.finish(&mut dabs);
        }
        self.paint(scene, &dabs, targets)?;
        let recorded = RecordedBrush {
            preset: self.setup.preset.clone(),
            values: self.setup.values.channels.iter().filter(|(_, _, on)| *on).map(|(c, v, _)| (*c, *v)).collect(),
            mask: self.setup.values.mask,
            viewport: match self.camera {
                Some(c) => c.viewport,
                None => [scene.surface.width() as f32, scene.surface.height() as f32],
            },
            retouch: self.setup.retouch,
        };
        let stroke = Stroke {
            tool: self.setup.tool,
            preset: recorded.to_reference(),
            view: self.setup.view,
            camera: self.camera.map(stroke::Camera::from).unwrap_or_default(),
            seed: self.setup.seed,
            symmetry: self.setup.symmetry,
            samples: self.samples,
        };
        Ok(Finished { stroke, touched: self.stamp.touched(), dabs: self.placed })
    }

    fn paint(&mut self, scene: &SessionScene, dabs: &[DabSample], targets: &mut [ChannelTarget]) -> Result<Vec<(u32, u32)>, BrushError> {
        let space = match (self.setup.view, scene.projector) {
            (ViewKind::Uv, _) => StrokeSpace::Texture,
            (ViewKind::Mesh, Some(projector)) => {
                StrokeSpace::View(ViewSpace { projector, depth: self.depth.as_ref(), placement: self.preset.placement, max_angle: self.preset.max_angle })
            }
            (ViewKind::Mesh, None) => return Err(BrushError::NoView),
        };
        let mut factors: Vec<&dyn TexelFactor> = scene.factors.to_vec();
        if let Some(p) = scene.projected {
            factors.push(p);
        }
        let ctx = StampContext { surface: scene.surface, index: scene.index, space, factors: &factors };
        // all of them at once: the same pixels as one by one, each changed tile written once
        let changed = self.stamp.apply_all(&ctx, dabs, targets)?;
        self.placed += dabs.len();
        Ok(changed)
    }

    /// The recorded sample of an input point.
    fn sample(&self, scene: &SessionScene, p: &InputPoint) -> StrokeSample {
        let position = match self.camera {
            Some(c) => c.screen_to_ndc(p.pos),
            None => [p.pos[0] / scene.surface.width().max(1) as f32, p.pos[1] / scene.surface.height().max(1) as f32],
        };
        StrokeSample { position, pressure: p.pressure, tilt: p.tilt, rotation: p.rotation, time: p.time as f32 }
    }

    /// Dab diameter in frame units at the stroke's start.
    fn diameter(&self, scene: &SessionScene) -> f32 {
        let size = self.preset.size;
        let radius = self.setup.model.radius;
        match (self.setup.view, self.preset.size_space) {
            (ViewKind::Uv, SizeSpace::Model) => size * scene.surface.width() as f32,
            (ViewKind::Uv, _) | (ViewKind::Mesh, SizeSpace::Screen | SizeSpace::Texture) => size,
            (ViewKind::Mesh, SizeSpace::Model) => size * radius * self.start_scale.unwrap_or(1.0),
        }
    }

    /// The dab scale at screen position `pos`: for model-sized 3D-view brushes the model's screen size there
    /// relative to the stroke's start (the last known scale where the pointer is off the model), else 1.
    fn scale_at(&mut self, scene: &SessionScene, pos: [f32; 2]) -> f32 {
        if self.setup.view != ViewKind::Mesh || self.preset.size_space != SizeSpace::Model {
            return 1.0;
        }
        let Some(projector) = scene.projector else { return self.last_scale };
        let here = projector.pick(pos).and_then(|h| screen_scale_at(projector, h.position));
        let start = *self.start_scale.get_or_insert_with(|| here.or_else(|| screen_scale_at(projector, self.setup.model.centre)).unwrap_or(1.0));
        if let Some(k) = here {
            self.last_scale = k / start;
        }
        self.last_scale
    }
}
