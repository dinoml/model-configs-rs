//! Lossless source documents and normalized views for model repositories.
//!
//! Parsing is deliberately data-only. This crate never imports Python or
//! instantiates classes named by configuration files.

mod diagnostic;
mod document;
mod error;
mod json_scan;
mod manifest;
mod normalize;
mod path_serde;
mod repository;
mod selection;
mod validation;
mod views;

pub use diagnostic::{
    Diagnostic, DiagnosticCode, DiagnosticLevel, MAX_DIAGNOSTIC_TEXT_BYTES,
    MAX_REPOSITORY_DIAGNOSTICS,
};
pub use document::{
    DocumentKind, JsonError, JsonErrorCategory, MAX_DUPLICATE_KEY_LOCATION_BYTES,
    MAX_DUPLICATE_KEY_LOCATIONS, MAX_SOURCE_DOCUMENT_BYTES, SourceDocument,
};
pub use error::{ChatTemplateError, ConfigError, NormalizationError, SelectionError};
pub use manifest::{
    CompatibilityManifest, MANIFEST_SCHEMA_VERSION, ManifestDocument, ManifestReadError,
};
pub use normalize::{
    AppliedDefault, ArchitectureId, ArchitectureSource, ComponentReference, ModelRepositoryConfig,
    NORMALIZATION_PROFILE, TaskKind,
};
pub use path_serde::{MAX_REPOSITORY_PATH_BYTES, MAX_REPOSITORY_PATH_SEGMENT_BYTES};
pub use repository::{
    MAX_REPOSITORY_DOCUMENTS, MAX_REPOSITORY_ENTRIES, MAX_REPOSITORY_SOURCE_BYTES, ModelRepository,
    RepositoryInventory,
};
pub use selection::{ChatTemplateSelection, ChatTemplateValue, SourceSelection};
pub use views::{
    AdapterConfigView, AddedTokenDecoderEntries, AddedTokenView, ChatTemplateView, ConfigView,
    DiffusersComponent, DiffusersComponentValue, DiffusersComponents, ExtraFields,
    GenerationConfigView, ModelIndexExtraFields, ModelIndexView, PreprocessorConfigView,
    ProcessorConfigView, QuantizationConfigView, SafetensorsIndexView, SafetensorsMetadataView,
    SafetensorsWeightMapEntries, SafetensorsWeightMapView, SchedulerConfigView, SourceField,
    SpecialTokenValue, SpecialTokenValues, SpecialTokensMapView, TokenizerConfigView,
    TypedDocumentView, ViewError,
};
