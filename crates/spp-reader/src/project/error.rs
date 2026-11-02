use crate::container::hdf5::Hdf5Error;
use crate::decode::DecodeError;
use std::fmt;

#[derive(Debug)]
pub enum ProjectError {
    Hdf5(Hdf5Error),
    /// A dataset was read but its content could not be decoded.
    Decode {
        dataset: String,
        source: DecodeError,
    },
    /// A valid HDF5 file, but not a Painter project (the named dataset is missing).
    NotAPainterProject {
        missing: &'static str,
    },
}

impl ProjectError {
    pub fn decode(dataset: &str) -> impl FnOnce(DecodeError) -> ProjectError + '_ {
        move |source| ProjectError::Decode {
            dataset: dataset.to_string(),
            source,
        }
    }
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectError::Hdf5(e) => write!(f, "{e}"),
            ProjectError::Decode { dataset, source } => write!(f, "{dataset}: {source}"),
            ProjectError::NotAPainterProject { missing } => write!(f, "not a Substance Painter project (no {missing})"),
        }
    }
}

impl std::error::Error for ProjectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ProjectError::Hdf5(e) => Some(e),
            ProjectError::Decode { source, .. } => Some(source),
            ProjectError::NotAPainterProject { .. } => None,
        }
    }
}

impl From<Hdf5Error> for ProjectError {
    fn from(e: Hdf5Error) -> Self {
        ProjectError::Hdf5(e)
    }
}
