//! Imports Substance 3D Painter projects (`.spp`) into MaterialCraft's paint documents.
//!
//! Reading the file is [`spp_reader`]'s job (re-exported here as [`container`], [`decode`], [`model`], [`project`],
//! [`inspect`] and [`blockers`]); this crate translates what it reads into MaterialCraft's model:
//! - [`translate`]: a project → MaterialCraft's paint document (the importer), with the support catalog that says
//!   what can be unlocked ([`translate::catalog`]).
//! - [`tools`]: what `mcsub` runs on top of the importer (map dumps, export comparison, batch conversion, the blocker
//!   census, re-import).

pub use spp_reader::{blockers, container, decode, inspect, model, project};

pub mod tools;
pub mod translate;
