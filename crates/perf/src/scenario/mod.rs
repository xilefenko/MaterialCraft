//! [`Scenario`]: one benchmark described as data: where the document comes from ([`Source`]), what is done to it
//! ([`Action`]) and how often. Read from and written to JSON with the core's json module:
//!
//! ```json
//! {"id": "erase-2k", "about": "Eraser over a painted stroke",
//!  "source": {"mesh": "/path/LP.obj", "resolution": 2048},
//!  "action": {"stroke": {"tool": "erase", "primed": true, "path": "wave", "samples": 200, "per_frame": 2}},
//!  "repeat": 1}
//! ```
//!
//! Sources: `{"mesh": <obj or glTF>, "resolution": <px>}` (a new project, File > New project) or
//! `{"document": <.mcpaint or .spp>}`. Actions: `"open"` (open and composite every set, `repeat` more times),
//! `{"stroke": {..}}` ([`StrokeSpec`], every field optional) or `{"edits": ["opacity", ..]}` ([`EditKind`]).

mod json;

use std::path::{Path, PathBuf};

/// One benchmark.
#[derive(Debug, Clone, PartialEq)]
pub struct Scenario {
    /// Short, stable name (`stroke-2k`): the command line and the history use it.
    pub id: String,
    /// One line on what it measures.
    pub about: String,
    pub source: Source,
    pub action: Action,
    /// How often the action is played on the opened document (at least 1).
    pub repeat: u32,
}

/// Where the document comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// A new project made from a mesh with the first built-in template, every set at `resolution`.
    NewProject { mesh: PathBuf, resolution: u32 },
    /// A saved `.mcpaint`, or a Painter `.spp` imported on the fly.
    Document(PathBuf),
}

impl Source {
    /// The file the source reads.
    pub fn path(&self) -> &Path {
        match self {
            Source::NewProject { mesh, .. } => mesh,
            Source::Document(p) => p,
        }
    }
}

/// What is done to the opened document.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Composite every set again (the cost of opening).
    Open,
    Stroke(StrokeSpec),
    /// Layer edits in this order, each one history step, then the recomposite of what it reached.
    Edits(Vec<EditKind>),
}

/// A brush stroke across the view.
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeSpec {
    pub tool: StrokeTool,
    /// A brush preset file (`.mcbrush`) for the tool; `None`: the tool's default brush.
    pub preset: Option<PathBuf>,
    /// Brush diameter (the preset's size units: a fraction of the mesh radius by default); `None`: the preset's.
    pub size: Option<f32>,
    /// Distance between dabs as a share of their size (the preset's units); `None`: the preset's.
    pub spacing: Option<f32>,
    pub target: StrokeTarget,
    pub layer: LayerChoice,
    /// The texture set painted, by name; `None`: the first set of a new project, else the set under the stroke.
    pub set: Option<String>,
    pub symmetry: Symmetry,
    pub path: PathShape,
    /// Pointer samples along the path.
    pub samples: usize,
    /// Samples delivered per frame.
    pub per_frame: usize,
    /// Play in real time with frames this many milliseconds apart: samples arrive at `per_frame` per frame period
    /// and each frame takes every sample that arrived by its start, so a slow frame makes the next one bigger
    /// (as in the window). The recomposites are not waited for. `None`: frame by frame, each recomposite waited for.
    pub frame_ms: Option<f64>,
    /// Paint the same path once before measuring, so the eraser and smudge have something to work on.
    pub primed: bool,
    /// After the strokes, undo the last one and redo it (measured).
    pub undo_redo: bool,
}

impl Default for StrokeSpec {
    fn default() -> StrokeSpec {
        StrokeSpec {
            tool: StrokeTool::Paint,
            preset: None,
            size: None,
            spacing: None,
            target: StrokeTarget::Content,
            layer: LayerChoice::TopPaint,
            set: None,
            symmetry: Symmetry::Off,
            path: PathShape::Wave,
            samples: 200,
            per_frame: 2,
            frame_ms: None,
            primed: false,
            undo_redo: false,
        }
    }
}

/// Declares a fieldless enum with its JSON names.
macro_rules! named {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $v:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($(#[$vm])* $v),+ }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$v),+];

            /// The JSON name.
            pub fn name(self) -> &'static str {
                match self { $($name::$v => $s),+ }
            }

            /// The value named `s`.
            pub fn from_name(s: &str) -> Result<$name, String> {
                match s {
                    $($s => Ok($name::$v),)+
                    _ => Err(format!("unknown {} {s:?} (one of: {})", stringify!($name), [$($s),+].join(", "))),
                }
            }
        }
    };
}

named!(
    /// The brush tool.
    StrokeTool { Paint = "paint", Erase = "erase", Smudge = "smudge", Blur = "blur" }
);

named!(
    /// What the stroke paints into.
    StrokeTarget {
        /// The layer's own channel images.
        Content = "content",
        /// The layer's mask (a white one is added first when it has none).
        Mask = "mask",
    }
);

named!(
    /// Which layer is painted.
    LayerChoice {
        /// The topmost paint layer of the set (a new one when it has none).
        TopPaint = "top-paint",
        /// A new paint layer on top of the stack.
        New = "new",
    }
);

named!(
    /// The pointer's path across the view.
    PathShape {
        /// Straight across the middle.
        Line = "line",
        /// Across the middle, waving up and down.
        Wave = "wave",
        /// Three turns outwards from the centre.
        Spiral = "spiral",
    }
);

named!(
    /// One layer edit, through the layer panel's path (or the history's, for undo and redo). Edits act on the
    /// middle top-level layer of the set with the most layers; mask edits on its first masked layer, effect edits
    /// on its first mask generator or filter.
    EditKind {
        Opacity = "opacity",
        Hide = "hide",
        Show = "show",
        BlendMode = "blend-mode",
        /// Invert a mask.
        Mask = "mask",
        /// Switch a mask off and back on (two steps).
        MaskToggle = "mask-toggle",
        /// Change a numeric parameter of an effect.
        EffectParam = "effect-param",
        /// Switch an effect off and back on (two steps).
        EffectToggle = "effect-toggle",
        Undo = "undo",
        Redo = "redo",
    }
);

/// Symmetry of the stroke (object axes 0 = X, 1 = Y, 2 = Z).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symmetry {
    Off,
    Mirror { axis: usize },
    Radial { axis: usize, count: u32 },
}

#[cfg(test)]
mod tests;
