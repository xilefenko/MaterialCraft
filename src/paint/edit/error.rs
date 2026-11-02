//! [`EditError`]: why an edit could not be applied.

use super::StackId;
use crate::paint::image::ImageError;
use crate::paint::layer::LayerImage;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum EditError {
    NoTextureSet(String),
    NoLayer {
        set: String,
        layer: String,
    },
    /// The parent named in a slot is not a group layer.
    NotAGroup {
        set: String,
        layer: String,
    },
    /// A layer or texture set id that is already taken (or empty).
    DuplicateId {
        set: String,
        id: String,
    },
    /// A group cannot move into itself or one of its descendants.
    IntoItself {
        set: String,
        layer: String,
    },
    /// A position past the end of a list.
    IndexOutOfRange {
        index: usize,
        len: usize,
    },
    /// The layer has no mask.
    NoMask {
        set: String,
        layer: String,
    },
    /// The property does not apply to this layer (a fill for a non-fill layer).
    WrongLayerKind {
        set: String,
        layer: String,
    },
    /// A content effect given for the mask stack or the other way round.
    WrongStack(StackId),
    /// The layer has no such image.
    NoImage {
        set: String,
        layer: String,
        image: LayerImage,
    },
    /// A region outside the image, or pixel data of the wrong length.
    BadRegion,
    Image(ImageError),
    /// An instance's parameters come from its source and cannot be changed on it (IN-02); only its name and
    /// visibility are its own.
    InstanceReadOnly {
        set: String,
        layer: String,
    },
    /// The layer's content is locked (see [`crate::paint::lock`]); `why` is the lock's summary. Unlock it first.
    Locked {
        set: String,
        layer: String,
        why: String,
    },
    /// The change would make an instance depend on itself (an instance inside the folder it instances, IN-11).
    InstanceCycle {
        set: String,
        layer: String,
    },
    /// A panel operation that does not apply to what it was aimed at (e.g. an effect dropped onto a mask
    /// that does not exist, an empty selection); the text says why.
    NotAccepted(&'static str),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EditError::NoTextureSet(s) => write!(f, "no texture set {s:?}"),
            EditError::NoLayer { set, layer } => write!(f, "no layer {layer:?} in texture set {set:?}"),
            EditError::NotAGroup { set, layer } => write!(f, "layer {layer:?} in texture set {set:?} is not a group"),
            EditError::DuplicateId { set, id } => write!(f, "id {id:?} is empty or already used in {set:?}"),
            EditError::IntoItself { set, layer } => write!(f, "group {layer:?} in {set:?} cannot move into itself"),
            EditError::IndexOutOfRange { index, len } => write!(f, "position {index} is past the end ({len})"),
            EditError::NoMask { set, layer } => write!(f, "layer {layer:?} in {set:?} has no mask"),
            EditError::WrongLayerKind { set, layer } => write!(f, "property does not apply to layer {layer:?} in {set:?}"),
            EditError::WrongStack(s) => write!(f, "effect does not belong in the {s:?} stack"),
            EditError::NoImage { set, layer, image } => write!(f, "layer {layer:?} in {set:?} has no image {image:?}"),
            EditError::BadRegion => write!(f, "pixel region outside the image or of the wrong size"),
            EditError::Image(e) => write!(f, "{e}"),
            EditError::InstanceReadOnly { set, layer } => {
                write!(f, "layer {layer:?} in {set:?} is an instance: change its source instead")
            }
            EditError::Locked { set, layer, why } => write!(f, "layer {layer:?} in {set:?} is locked: {why}"),
            EditError::InstanceCycle { set, layer } => write!(f, "instance {layer:?} in {set:?} would depend on itself"),
            EditError::NotAccepted(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for EditError {}

impl From<ImageError> for EditError {
    fn from(e: ImageError) -> EditError {
        EditError::Image(e)
    }
}
