//! MaterialCraft's brush strokes (roadmap step 20): input points → dabs → texture-space stamps, as one history step
//! and one stroke record per stroke.
//!
//! - [`engine`]: [`DabEngine`] turns input points into dabs. It is PhotoCraft's brush engine (`photocraft-paint`
//!   at a pinned revision) behind one adapter file, `engine::photocraft`, the only code naming it (ADR 0007)
//!   besides `engine::photocraft_tips`, which hands out PhotoCraft's procedural tips ([`procedural_tip`]),
//!   `engine::photocraft_curve`, its response curve ([`ResponseCurve`], the pen pressure curve), and
//!   `engine::photocraft_smudge`, its smudge as the stamp stage's local kernel ([`SmudgeKernel`], PT-23), and
//!   `engine::photocraft_patterns`, its procedural patterns ([`pattern`], the starter library's pattern textures).
//!   Model-sized brushes get a per-point dab scale there (ADR 0006 Q-PT-12).
//! - [`session`]: [`StrokeSession`] drives one stroke through the core's stamp stage and records its samples.
//! - [`target`]: [`PaintTarget`], what a tool paints (a paint layer's channels or a mask, ADR 0008), [`mode_for`] how
//!   each tool lands in it (paint, erase, clone, smudge and blur kernels), and with which
//!   values.
//! - [`clone_source`]: [`CloneSource`], the clone tool's source and its aligned or fixed offset (PT-27, PT-28).
//! - [`document`]: [`DocumentStroke`], a stroke into a document with the inverses for one undo step.
//! - [`quick_mask`]: [`QuickMask`], the session mask every stroke honours, and [`QuickMaskStroke`] to paint it.
//! - [`see_through`]: see-through layers (PT-25): their strokes painted again on the composite below whenever it
//!   changes ([`SeeThroughCache`]).
//! - [`replay`]: painting a recorded stroke again, bit for bit; imported strokes with a mapped [`ReplayBrush`].
//! - [`stroke`]: input transforms in front of a session: [`LazyMouse`], straight lines ([`LineAnchor`]).
//! - [`symmetry`]: symmetric copies in a stroke record's form, built the same way live and on replay.
//! - [`worker`]: [`StrokeWorker`], a document stroke stamped on its own thread (the input thread only hands points
//!   over and picks up what was painted).
//! - [`tips`]: alpha-image tips found in the asset libraries ([`FolderAlphas`]); [`preview`]: a brush's sample
//!   stroke for its tray thumbnail ([`stroke_preview`]).
//!
//! Rule: the std-only core never depends on this crate; it sees only plain dabs and coverage grids.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod clone_source;
pub mod document;
pub mod engine;
pub mod preview;
pub mod quick_mask;
pub mod replay;
pub mod see_through;
pub mod session;
pub mod stroke;
pub mod symmetry;
pub mod target;
pub mod tips;
pub mod worker;

pub use clone_source::{CloneSource, SourceMode, texel_under};
pub use document::{Base, DocumentStroke, Ended, Prepared};
pub use engine::{
    DabEngine, EngineExtras, InputPoint, PATTERN_GENERATOR, PATTERN_PERIOD, PATTERN_PX, PROCEDURAL_TIP_GENERATOR, PROCEDURAL_TIP_PX, PROCEDURAL_TIPS,
    PatternKind, ResponseCurve, SmudgeKernel, TipAlpha, TipInfo, pattern, procedural_tip, procedural_tip_alpha,
};
pub use preview::stroke_preview;
pub use quick_mask::{QuickMask, QuickMaskState, QuickMaskStroke};
pub use replay::{ReplayBrush, ReplayScene, recorded_brush, recorded_values, replay, replay_imported, replay_with};
pub use see_through::{BelowSource, Refreshed, SeeThroughCache, repaint};
pub use session::{Finished, SessionScene, StrokeSession, StrokeSetup};
pub use stroke::{LazyMouse, LineAnchor};
pub use target::{PaintTarget, PaintValues, Resolved, TargetPart, mode_for};
pub use tips::{AlphaSource, FolderAlphas, NoAlphas, alpha_for, decode_alpha};
pub use worker::{Done, Progress, StrokeWorker, WorkerScene};

use materialcraft::paint::edit::EditError;
use materialcraft::stamp::StampError;
use std::fmt;

/// Why a stroke could not be painted.
#[derive(Debug, Clone, PartialEq)]
pub enum BrushError {
    /// The target can't be painted (the text says why).
    NotPaintable(&'static str),
    /// A 3D-view stroke without a view.
    NoView,
    /// A recorded stroke can't be replayed (the text says why).
    NotReplayable(&'static str),
    Stamp(StampError),
    Edit(EditError),
}

impl fmt::Display for BrushError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            BrushError::NotPaintable(why) | BrushError::NotReplayable(why) => f.write_str(why),
            BrushError::NoView => f.write_str("the 3D view is not ready"),
            BrushError::Stamp(e) => write!(f, "{e}"),
            BrushError::Edit(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BrushError {}

impl From<StampError> for BrushError {
    fn from(e: StampError) -> BrushError {
        BrushError::Stamp(e)
    }
}
