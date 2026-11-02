//! [`Procedural`]: one use of a `.sbsar` (`DataProcedural`): the resource, its parameter values
//! (tweaks), the outputs wired to channels and the inputs fed from other sources.

use super::channel::{ChannelMask, PainterChannel};
use super::source::PainterSource;

#[derive(Debug, Clone, PartialEq)]
pub struct Procedural {
    pub uid: Option<u64>,
    /// `urlToSbsRes`: `resource://<shelf path>?version=<hash>.sbsar` or an embedded `/<name>?version=<id>`.
    pub url: String,
    pub tweaks: Vec<Tweak>,
    pub outputs: Vec<ProceduralOutput>,
    pub inputs: Vec<ProceduralInput>,
}

impl Procedural {
    /// The resource name without shelf path and version (`concrete_cast` for
    /// `resource://starter_assets/concrete_cast/concrete_cast?version=….sbsar`).
    pub fn name(&self) -> &str {
        let path = self.url.split('?').next().unwrap_or("");
        path.rsplit('/').next().unwrap_or(path)
    }
}

/// One parameter value (`DataTweak*`), keyed by the `.sbsar` input identifier.
#[derive(Debug, Clone, PartialEq)]
pub struct Tweak {
    pub identifier: String,
    pub value: TweakValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TweakValue {
    Int(i32),
    /// `DataTweakInt2`, e.g. `$outputsize` as log2 sizes.
    Int2([i32; 2]),
    /// `DataTweakFloat` … `DataTweakFloat4`: 1 to 4 components.
    Floats(Vec<f32>),
    /// `DataTweakByteArray` (text inputs).
    Text(String),
    /// A tweak class the model does not know, by class name.
    Other(String),
}

/// An output of the `.sbsar` and the channels it feeds (`DataProceduralOutput`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProceduralOutput {
    pub identifier: String,
    pub channels: ChannelMask,
}

/// An image input of the `.sbsar` and what feeds it (`DataProceduralInputsSource`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProceduralInput {
    /// Input identifiers (`DataProceduralInput.identifier`) with the channel each one reads.
    pub identifiers: Vec<(String, PainterChannel)>,
    pub source: Option<Box<PainterSource>>,
}
