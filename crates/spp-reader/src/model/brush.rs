//! [`PainterBrush`]: the brush a paint effect's strokes were painted with (`DataBrush`) and how it lays its stamps
//! along a stroke (`stampsEmitter`: `DataEmitterStroke`, `DataEmitterPhysics` for particle brushes,
//! `DataEmitterStroke3D` for path strokes). Field meanings and counts: `interop/painter-document-catalogue.md`
//! (`DataBrush`, `DataEmitterStroke`); the meanings in the comments are Inferred unless the import's replay
//! calibration says otherwise (`translate::layers::replay`).

/// `DataBrush`.
#[derive(Debug, Clone, PartialEq)]
pub struct PainterBrush {
    pub uid: Option<u64>,
    /// `brushSize`: stamp size (0.001 … 1.5; 0.1 most common).
    pub size: f32,
    /// `brushSizeSpace` (1 in every corpus brush).
    pub size_space: u64,
    /// `brushSizeJitter` 0 … 1.
    pub size_jitter: f32,
    /// `flow` 0 … 1.
    pub flow: f32,
    /// `flowJitter` 0 … 1.
    pub flow_jitter: f32,
    /// `alignment` (3 ×12254, 1 ×844, 2 ×496): how a stamp lies on the surface.
    pub alignment: u64,
    /// `cullingEnabled` and `cullingAngle` (0.5 everywhere): surfaces turned away are not painted.
    pub culling: Option<f32>,
    /// `paintThrough` (0, 1; absent in old files).
    pub paint_through: u64,
    /// `filtering` (0, 1).
    pub filtering: u64,
    pub emitter: PainterEmitter,
}

impl Default for PainterBrush {
    fn default() -> PainterBrush {
        PainterBrush {
            uid: None,
            size: 0.1,
            size_space: 1,
            size_jitter: 0.0,
            flow: 1.0,
            flow_jitter: 0.0,
            alignment: 3,
            culling: Some(0.5),
            paint_through: 0,
            filtering: 0,
            emitter: PainterEmitter::Stroke(StrokeEmitter::default()),
        }
    }
}

/// `stampsEmitter`.
#[derive(Debug, Clone, PartialEq)]
pub enum PainterEmitter {
    /// `DataEmitterStroke`: stamps along the pen's path.
    Stroke(StrokeEmitter),
    /// `DataEmitterPhysics`: a particle brush (`urlEmitter`, `urlReceiver`); stamps come from a simulation.
    Physics { emitter: String, receiver: String },
    /// `DataEmitterStroke3D`: stamps along a 3D path stroke.
    Stroke3D(StrokeEmitter),
    /// No emitter, or a class the model does not know.
    Other(String),
}

/// One pressure response (`pressureSizeEnabled` / `pressureSizeMinimum`, the same for flow).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PressureResponse {
    pub enabled: bool,
    /// The value at zero pressure, as a fraction of the full value.
    pub minimum: f32,
}

/// `DataEmitterStroke` (and the shared fields of `DataEmitterStroke3D`).
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeEmitter {
    /// `spacing`: distance between stamps, a fraction of the stamp size (0.2 most common).
    pub spacing: f32,
    /// `positionJitter` (0 … 1.1), `positionJitterBothAxes`, `positionJitterDistribution` (0, 1).
    pub position_jitter: f32,
    pub position_jitter_both_axes: bool,
    pub position_jitter_distribution: u64,
    /// `rotation` in radians and `rotationJitter` in radians (0 … π).
    pub rotation: f32,
    pub rotation_jitter: f32,
    /// `followPath`: stamps turn along the stroke.
    pub follow_path: bool,
    pub pressure_size: PressureResponse,
    pub pressure_flow: PressureResponse,
    /// `aspectRatio` (1 in the corpus; old files only).
    pub aspect_ratio: f32,
}

impl Default for StrokeEmitter {
    fn default() -> StrokeEmitter {
        StrokeEmitter {
            spacing: 0.2,
            position_jitter: 0.0,
            position_jitter_both_axes: true,
            position_jitter_distribution: 1,
            rotation: 0.0,
            rotation_jitter: 0.0,
            follow_path: false,
            pressure_size: PressureResponse {
                enabled: true,
                minimum: 0.0,
            },
            pressure_flow: PressureResponse {
                enabled: false,
                minimum: 0.0,
            },
            aspect_ratio: 1.0,
        }
    }
}
