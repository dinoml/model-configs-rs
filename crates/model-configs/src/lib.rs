//! Lossless source documents and normalized views for model repositories.
//!
//! Parsing is deliberately data-only. This crate never imports Python or
//! instantiates classes named by configuration files.

mod document;
mod error;
mod normalize;
mod repository;

pub use document::{DocumentKind, SourceDocument};
pub use error::ConfigError;
pub use normalize::{
    AppliedDefault, ArchitectureId, ComponentReference, ModelRepositoryConfig, TaskKind,
};
pub use repository::{Diagnostic, DiagnosticLevel, ModelRepository};
