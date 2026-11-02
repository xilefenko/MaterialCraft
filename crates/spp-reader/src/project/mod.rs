//! One opened `.spp` file: [`SppProject`] and the listings it offers.

mod cache;
mod error;
mod layout;
pub mod paths;
mod resources;
mod spp;

pub use cache::CacheEntryRef;
pub use error::ProjectError;
pub use layout::{DatasetRef, Layout};
pub use resources::{ResourceKind, ResourceRef};
pub use spp::SppProject;
