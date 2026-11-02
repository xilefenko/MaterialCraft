//! [`LayerEvent`]: what the layer panel asks of the document. `PaintMode` hands these to the
//! [`crate::gui_paint::stack_editor`], which makes each one a history step through `paint::edit`.

use materialcraft::history::CoalesceKey;
use materialcraft::paint::edit::ops::{DropPayload, DropTarget, MaskStart, NewEffect, NewLayer};
use materialcraft::paint::edit::{LayerRef, LayerSlot, StackId};
use materialcraft::paint::{BlendMode, Channel, GeometryMask};

#[derive(Clone, Debug, PartialEq)]
pub enum LayerEvent {
    /// A new layer directly above `above` (top of the stack without one).
    Create {
        set: String,
        what: NewLayer,
        above: Option<String>,
    },
    Duplicate {
        set: String,
        ids: Vec<String>,
    },
    Delete {
        set: String,
        ids: Vec<String>,
    },
    Group {
        set: String,
        ids: Vec<String>,
    },
    /// Move the layers to `to` (counted before the move), or with `copy` place copies there.
    Move {
        set: String,
        ids: Vec<String>,
        to: LayerSlot,
        copy: bool,
    },
    Rename(LayerRef, String),
    SetVisible(LayerRef, bool),
    /// Remove the layer's lock so its content can be edited (one history step; Undo locks it again).
    Unlock(LayerRef),
    /// The opacity of the layer's entry for `channel`; events with the same key are one step (a drag).
    SetOpacity {
        layer: LayerRef,
        channel: Channel,
        opacity: f32,
        key: CoalesceKey,
    },
    SetBlendMode {
        layer: LayerRef,
        channel: Channel,
        mode: BlendMode,
    },
    /// The end of a slider drag: the next change starts a new step.
    EndGesture,
    AddEffect {
        layer: LayerRef,
        stack: StackId,
        what: NewEffect,
    },
    RemoveEffect {
        layer: LayerRef,
        stack: StackId,
        index: usize,
    },
    SetEffectEnabled {
        layer: LayerRef,
        stack: StackId,
        index: usize,
        enabled: bool,
    },
    AddMask(LayerRef, MaskStart),
    RemoveMask(LayerRef),
    SetMaskEnabled(LayerRef, bool),
    InvertMask(LayerRef),
    /// Show only this layer's mask in the viewport (`None`: show the material again). View state only.
    Isolate(Option<LayerRef>),
    /// Replace the layer's geometry mask (`None`: the layer includes all geometry), one history step (GM-01).
    SetGeometryMask(LayerRef, Option<GeometryMask>),
    /// Enter the geometry mask edit mode for this layer (`None`: leave it, GM-03). View state only.
    EditGeometryMask(Option<LayerRef>),
    /// Instances of `source` in each of the texture sets `sets` (ids), one history step (IN-07).
    Instantiate {
        source: LayerRef,
        sets: Vec<String>,
    },
    /// An instance of `source` in texture set `set`, directly above layer `above` (the top without one) (IN-05).
    PasteInstance {
        source: LayerRef,
        set: String,
        above: Option<String>,
    },
    /// Save layers or a mask into the user's asset library (MK-15). Not a document change.
    SaveAsset(SaveAsset),
    /// An asset dropped onto the stack; `replace` is Ctrl. Sent by the asset tray (step 15).
    #[allow(dead_code)]
    Drop {
        target: DropTarget,
        payload: DropPayload,
        replace: bool,
    },
}

/// What "Save as smart material" and "Save as smart mask" save.
#[derive(Clone, Debug, PartialEq)]
pub enum SaveAsset {
    /// The selected layers (a single selected folder: its contents under its name).
    SmartMaterial { set: String, ids: Vec<String> },
    /// The layer's mask with its effects.
    SmartMask(LayerRef),
}
