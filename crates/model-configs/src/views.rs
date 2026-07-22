//! Borrowed, format-specific projections over lossless source documents.
//!
//! Views interpret only values explicitly present in a source document. They
//! retain access to the original JSON object and never apply Python or library
//! defaults. [`SourceField`] distinguishes absence, explicit `null`, valid
//! typed values, and values with an unexpected JSON shape.

use std::fmt;
use std::path::PathBuf;
use std::str::Utf8Error;

use serde_json::{Map, Value};

use crate::{DocumentKind, SourceDocument};

/// A source field interpreted without discarding its original state.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum SourceField<'a, T> {
    /// The source object does not contain the field.
    Missing,
    /// The source field is explicitly JSON `null`.
    Null,
    /// The source field contains a value of the expected shape.
    Value(T),
    /// The source field exists but has an unexpected JSON shape.
    Invalid(&'a Value),
}

impl<T> SourceField<'_, T> {
    /// Returns whether the field was absent from the source object.
    #[must_use]
    pub const fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }

    /// Returns whether the field was explicitly JSON `null`.
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Returns whether the field had an unexpected JSON shape.
    #[must_use]
    pub const fn is_invalid(&self) -> bool {
        matches!(self, Self::Invalid(_))
    }

    /// Returns the interpreted value when the field has the expected shape.
    #[must_use]
    pub fn value(self) -> Option<T> {
        if let Self::Value(value) = self {
            Some(value)
        } else {
            None
        }
    }
}

/// An error constructing a format-specific document view.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ViewError {
    /// A recognized JSON document did not contain an object at its root.
    #[error("{kind:?} document at {path} must contain a JSON object")]
    ExpectedObject {
        /// Recognized source document kind.
        kind: DocumentKind,
        /// Repository-relative source path.
        path: PathBuf,
    },
    /// Duplicate object members make the generic JSON projection ambiguous.
    #[error("{kind:?} document at {path} contains duplicate JSON object keys")]
    DuplicateKeys {
        /// Recognized source document kind.
        kind: DocumentKind,
        /// Repository-relative source path.
        path: PathBuf,
    },
}

#[derive(Clone, Copy, Debug)]
struct ObjectView<'a> {
    source: &'a SourceDocument,
    object: &'a Map<String, Value>,
}

impl<'a> ObjectView<'a> {
    fn new(source: &'a SourceDocument) -> Result<Self, ViewError> {
        if source.has_duplicate_keys() {
            return Err(ViewError::DuplicateKeys {
                kind: *source.kind(),
                path: source.relative_path().to_path_buf(),
            });
        }
        let Some(object) = source.json().and_then(Value::as_object) else {
            return Err(ViewError::ExpectedObject {
                kind: *source.kind(),
                path: source.relative_path().to_path_buf(),
            });
        };
        Ok(Self { source, object })
    }
}

/// Iterator over fields not interpreted by a particular typed view.
pub struct ExtraFields<'a> {
    fields: serde_json::map::Iter<'a>,
    known: &'static [&'static str],
}

impl fmt::Debug for ExtraFields<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtraFields")
            .finish_non_exhaustive()
    }
}

impl<'a> Iterator for ExtraFields<'a> {
    type Item = (&'a str, &'a Value);

    fn next(&mut self) -> Option<Self::Item> {
        for (key, value) in self.fields.by_ref() {
            if !self.known.contains(&key.as_str()) {
                return Some((key.as_str(), value));
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, self.fields.size_hint().1)
    }
}

fn raw_field<'a>(object: &'a Map<String, Value>, key: &str) -> SourceField<'a, &'a Value> {
    match object.get(key) {
        None => SourceField::Missing,
        Some(Value::Null) => SourceField::Null,
        Some(value) => SourceField::Value(value),
    }
}

/// A source special-token value in its string or structured added-token form.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum SpecialTokenValue<'a> {
    /// Plain token text.
    String(&'a str),
    /// Structured Transformers added-token metadata.
    AddedToken(AddedTokenView<'a>),
    /// An element inside a typed collection has an unsupported shape.
    Invalid(&'a Value),
}

/// Borrowed structured added-token metadata.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AddedTokenView<'a> {
    object: &'a Map<String, Value>,
}

impl<'a> AddedTokenView<'a> {
    /// Returns the unmodified added-token object.
    #[must_use]
    pub const fn raw(self) -> &'a Map<String, Value> {
        self.object
    }

    /// Returns the token content.
    #[must_use]
    pub fn content(self) -> SourceField<'a, &'a str> {
        string_field(self.object, "content")
    }

    /// Returns whether matching is restricted to a complete word.
    #[must_use]
    pub fn single_word(self) -> SourceField<'a, bool> {
        bool_field(self.object, "single_word")
    }

    /// Returns whether left-side whitespace is consumed.
    #[must_use]
    pub fn lstrip(self) -> SourceField<'a, bool> {
        bool_field(self.object, "lstrip")
    }

    /// Returns whether right-side whitespace is consumed.
    #[must_use]
    pub fn rstrip(self) -> SourceField<'a, bool> {
        bool_field(self.object, "rstrip")
    }

    /// Returns whether the token participates in normalization.
    #[must_use]
    pub fn normalized(self) -> SourceField<'a, bool> {
        bool_field(self.object, "normalized")
    }

    /// Returns whether this is explicitly a special token.
    #[must_use]
    pub fn special(self) -> SourceField<'a, bool> {
        bool_field(self.object, "special")
    }
}

/// Iterator over a source array of special-token values.
#[derive(Clone, Debug)]
pub struct SpecialTokenValues<'a> {
    values: std::slice::Iter<'a, Value>,
}

impl<'a> Iterator for SpecialTokenValues<'a> {
    type Item = SpecialTokenValue<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.values.next().map(classify_special_token)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.values.size_hint()
    }
}

impl ExactSizeIterator for SpecialTokenValues<'_> {}

/// Iterator over `added_tokens_decoder` source entries.
#[derive(Clone, Debug)]
pub struct AddedTokenDecoderEntries<'a> {
    entries: serde_json::map::Iter<'a>,
}

impl<'a> Iterator for AddedTokenDecoderEntries<'a> {
    type Item = (&'a str, SpecialTokenValue<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        self.entries
            .next()
            .map(|(id, value)| (id.as_str(), classify_special_token(value)))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.entries.size_hint()
    }
}

impl ExactSizeIterator for AddedTokenDecoderEntries<'_> {}

fn classify_special_token(value: &Value) -> SpecialTokenValue<'_> {
    match value {
        Value::String(value) => SpecialTokenValue::String(value),
        Value::Object(object) => SpecialTokenValue::AddedToken(AddedTokenView { object }),
        value => SpecialTokenValue::Invalid(value),
    }
}

fn special_token_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> SourceField<'a, SpecialTokenValue<'a>> {
    match object.get(key) {
        None => SourceField::Missing,
        Some(Value::Null) => SourceField::Null,
        Some(value @ (Value::String(_) | Value::Object(_))) => {
            SourceField::Value(classify_special_token(value))
        }
        Some(value) => SourceField::Invalid(value),
    }
}

fn special_token_values_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> SourceField<'a, SpecialTokenValues<'a>> {
    match object.get(key) {
        None => SourceField::Missing,
        Some(Value::Null) => SourceField::Null,
        Some(Value::Array(values)) => SourceField::Value(SpecialTokenValues {
            values: values.iter(),
        }),
        Some(value) => SourceField::Invalid(value),
    }
}

fn added_token_decoder_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> SourceField<'a, AddedTokenDecoderEntries<'a>> {
    match object.get(key) {
        None => SourceField::Missing,
        Some(Value::Null) => SourceField::Null,
        Some(Value::Object(entries)) => SourceField::Value(AddedTokenDecoderEntries {
            entries: entries.iter(),
        }),
        Some(value) => SourceField::Invalid(value),
    }
}

fn string_field<'a>(object: &'a Map<String, Value>, key: &str) -> SourceField<'a, &'a str> {
    match object.get(key) {
        None => SourceField::Missing,
        Some(Value::Null) => SourceField::Null,
        Some(Value::String(value)) => SourceField::Value(value),
        Some(value) => SourceField::Invalid(value),
    }
}

fn bool_field<'a>(object: &'a Map<String, Value>, key: &str) -> SourceField<'a, bool> {
    match object.get(key) {
        None => SourceField::Missing,
        Some(Value::Null) => SourceField::Null,
        Some(Value::Bool(value)) => SourceField::Value(*value),
        Some(value) => SourceField::Invalid(value),
    }
}

fn u64_field<'a>(object: &'a Map<String, Value>, key: &str) -> SourceField<'a, u64> {
    match object.get(key) {
        None => SourceField::Missing,
        Some(Value::Null) => SourceField::Null,
        Some(value) => value
            .as_u64()
            .map_or(SourceField::Invalid(value), SourceField::Value),
    }
}

fn i64_field<'a>(object: &'a Map<String, Value>, key: &str) -> SourceField<'a, i64> {
    match object.get(key) {
        None => SourceField::Missing,
        Some(Value::Null) => SourceField::Null,
        Some(value) => value
            .as_i64()
            .map_or(SourceField::Invalid(value), SourceField::Value),
    }
}

fn f64_field<'a>(object: &'a Map<String, Value>, key: &str) -> SourceField<'a, f64> {
    match object.get(key) {
        None => SourceField::Missing,
        Some(Value::Null) => SourceField::Null,
        Some(value) => value
            .as_f64()
            .map_or(SourceField::Invalid(value), SourceField::Value),
    }
}

macro_rules! json_view {
    ($(#[$meta:meta])* $name:ident, [$($known:literal),* $(,)?]) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug)]
        pub struct $name<'a> {
            inner: ObjectView<'a>,
        }

        impl<'a> $name<'a> {
            const KNOWN_FIELDS: &'static [&'static str] = &[$($known),*];

            fn new(source: &'a SourceDocument) -> Result<Self, ViewError> {
                ObjectView::new(source).map(|inner| Self { inner })
            }

            /// Returns the lossless source document backing this view.
            #[must_use]
            pub const fn source(&self) -> &'a SourceDocument {
                self.inner.source
            }

            /// Returns the unmodified source JSON object.
            #[must_use]
            pub const fn raw(&self) -> &'a Map<String, Value> {
                self.inner.object
            }

            /// Returns an unmodified source field by name.
            #[must_use]
            pub fn get(&self, key: &str) -> Option<&'a Value> {
                self.inner.object.get(key)
            }

            /// Iterates over source fields not interpreted by this view.
            #[must_use]
            pub fn extra(&self) -> ExtraFields<'a> {
                ExtraFields {
                    fields: self.inner.object.iter(),
                    known: Self::KNOWN_FIELDS,
                }
            }
        }
    };
}

/// A borrowed typed projection for any recognized source document.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum TypedDocumentView<'a> {
    /// Main Transformers or component configuration.
    Config(ConfigView<'a>),
    /// Generation configuration.
    GenerationConfig(GenerationConfigView<'a>),
    /// Tokenizer configuration.
    TokenizerConfig(TokenizerConfigView<'a>),
    /// Special-token mapping.
    SpecialTokensMap(SpecialTokensMapView<'a>),
    /// Feature extractor or image/audio preprocessor configuration.
    PreprocessorConfig(PreprocessorConfigView<'a>),
    /// Multimodal processor configuration.
    ProcessorConfig(ProcessorConfigView<'a>),
    /// Diffusers scheduler configuration.
    SchedulerConfig(SchedulerConfigView<'a>),
    /// Diffusers pipeline index.
    ModelIndex(ModelIndexView<'a>),
    /// Adapter or PEFT configuration.
    AdapterConfig(AdapterConfigView<'a>),
    /// Quantization configuration.
    QuantizationConfig(QuantizationConfigView<'a>),
    /// Jinja chat template.
    ChatTemplate(ChatTemplateView<'a>),
    /// Safetensors sharded checkpoint index.
    SafetensorsIndex(SafetensorsIndexView<'a>),
}

impl<'a> TypedDocumentView<'a> {
    /// Returns the source document backing this typed view.
    #[must_use]
    pub const fn source(&self) -> &'a SourceDocument {
        match self {
            Self::Config(view) => view.source(),
            Self::GenerationConfig(view) => view.source(),
            Self::TokenizerConfig(view) => view.source(),
            Self::SpecialTokensMap(view) => view.source(),
            Self::PreprocessorConfig(view) => view.source(),
            Self::ProcessorConfig(view) => view.source(),
            Self::SchedulerConfig(view) => view.source(),
            Self::ModelIndex(view) => view.source(),
            Self::AdapterConfig(view) => view.source(),
            Self::QuantizationConfig(view) => view.source(),
            Self::ChatTemplate(view) => view.source(),
            Self::SafetensorsIndex(view) => view.source(),
        }
    }

    /// Returns the recognized kind of the source document.
    #[must_use]
    pub fn kind(&self) -> &'a DocumentKind {
        self.source().kind()
    }
}

impl<'a> TryFrom<&'a SourceDocument> for TypedDocumentView<'a> {
    type Error = ViewError;

    fn try_from(source: &'a SourceDocument) -> Result<Self, Self::Error> {
        match source.kind() {
            DocumentKind::Config => ConfigView::new(source).map(Self::Config),
            DocumentKind::GenerationConfig => {
                GenerationConfigView::new(source).map(Self::GenerationConfig)
            }
            DocumentKind::TokenizerConfig => {
                TokenizerConfigView::new(source).map(Self::TokenizerConfig)
            }
            DocumentKind::SpecialTokensMap => {
                SpecialTokensMapView::new(source).map(Self::SpecialTokensMap)
            }
            DocumentKind::PreprocessorConfig => {
                PreprocessorConfigView::new(source).map(Self::PreprocessorConfig)
            }
            DocumentKind::ProcessorConfig => {
                ProcessorConfigView::new(source).map(Self::ProcessorConfig)
            }
            DocumentKind::SchedulerConfig => {
                SchedulerConfigView::new(source).map(Self::SchedulerConfig)
            }
            DocumentKind::ModelIndex => ModelIndexView::new(source).map(Self::ModelIndex),
            DocumentKind::AdapterConfig => AdapterConfigView::new(source).map(Self::AdapterConfig),
            DocumentKind::QuantizationConfig => {
                QuantizationConfigView::new(source).map(Self::QuantizationConfig)
            }
            DocumentKind::ChatTemplate => Ok(Self::ChatTemplate(ChatTemplateView { source })),
            DocumentKind::SafetensorsIndex => {
                SafetensorsIndexView::new(source).map(Self::SafetensorsIndex)
            }
        }
    }
}

json_view!(
    /// Typed fields from a Transformers `config.json` document.
    ConfigView,
    [
        "_name_or_path",
        "_class_name",
        "architectures",
        "model_type",
        "transformers_version",
        "pipeline_tag",
        "task",
        "auto_map",
        "torch_dtype",
        "tie_word_embeddings",
        "vocab_size",
        "bos_token_id",
        "eos_token_id",
        "pad_token_id",
        "decoder_start_token_id",
        "is_encoder_decoder",
        "quantization_config",
    ]
);

impl<'a> ConfigView<'a> {
    /// Returns the source-provided model path or identifier.
    #[must_use]
    pub fn name_or_path(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "_name_or_path")
    }

    /// Returns the source-provided configuration class name.
    #[must_use]
    pub fn class_name(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "_class_name")
    }

    /// Returns the unmodified `architectures` value.
    #[must_use]
    pub fn architectures(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "architectures")
    }

    /// Returns the first non-empty string in `architectures`.
    #[must_use]
    pub fn first_architecture(&self) -> SourceField<'a, &'a str> {
        match self.raw().get("architectures") {
            None => SourceField::Missing,
            Some(Value::Null) => SourceField::Null,
            Some(value @ Value::Array(values)) => values
                .iter()
                .filter_map(Value::as_str)
                .find(|value| !value.is_empty())
                .map_or_else(|| SourceField::Invalid(value), SourceField::Value),
            Some(value) => SourceField::Invalid(value),
        }
    }

    /// Returns the Transformers model type.
    #[must_use]
    pub fn model_type(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "model_type")
    }

    /// Returns the Transformers version recorded by the source.
    #[must_use]
    pub fn transformers_version(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "transformers_version")
    }

    /// Returns the explicit pipeline tag.
    #[must_use]
    pub fn pipeline_tag(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "pipeline_tag")
    }

    /// Returns the source-provided legacy task alias.
    #[must_use]
    pub fn task(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "task")
    }

    /// Returns the unmodified automatic-class mapping.
    #[must_use]
    pub fn auto_map(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "auto_map")
    }

    /// Returns the source-provided tensor data type.
    #[must_use]
    pub fn torch_dtype(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "torch_dtype")
    }

    /// Returns whether input and output embeddings are tied.
    #[must_use]
    pub fn tie_word_embeddings(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "tie_word_embeddings")
    }

    /// Returns the source-provided vocabulary size.
    #[must_use]
    pub fn vocab_size(&self) -> SourceField<'a, u64> {
        u64_field(self.raw(), "vocab_size")
    }

    /// Returns the unmodified beginning-of-sequence token identifier.
    #[must_use]
    pub fn bos_token_id(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "bos_token_id")
    }

    /// Returns the unmodified end-of-sequence token identifier or identifiers.
    #[must_use]
    pub fn eos_token_id(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "eos_token_id")
    }

    /// Returns the unmodified padding token identifier.
    #[must_use]
    pub fn pad_token_id(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "pad_token_id")
    }

    /// Returns the unmodified decoder-start token identifier.
    #[must_use]
    pub fn decoder_start_token_id(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "decoder_start_token_id")
    }

    /// Returns whether the model declares an encoder-decoder architecture.
    #[must_use]
    pub fn is_encoder_decoder(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "is_encoder_decoder")
    }

    /// Returns the unmodified nested quantization configuration.
    #[must_use]
    pub fn quantization_config(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "quantization_config")
    }
}

json_view!(
    /// Typed fields from a Transformers `generation_config.json` document.
    GenerationConfigView,
    [
        "_from_model_config",
        "transformers_version",
        "max_length",
        "max_new_tokens",
        "min_length",
        "min_new_tokens",
        "do_sample",
        "num_beams",
        "temperature",
        "top_k",
        "top_p",
        "typical_p",
        "repetition_penalty",
        "length_penalty",
        "no_repeat_ngram_size",
        "early_stopping",
        "use_cache",
        "bos_token_id",
        "eos_token_id",
        "pad_token_id",
        "decoder_start_token_id",
        "bad_words_ids",
        "forced_bos_token_id",
        "forced_eos_token_id",
        "stop_strings",
        "cache_implementation",
    ]
);

impl<'a> GenerationConfigView<'a> {
    /// Returns whether the file originated from a model configuration.
    #[must_use]
    pub fn from_model_config(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "_from_model_config")
    }

    /// Returns the Transformers version recorded by the source.
    #[must_use]
    pub fn transformers_version(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "transformers_version")
    }

    /// Returns the maximum total sequence length.
    #[must_use]
    pub fn max_length(&self) -> SourceField<'a, u64> {
        u64_field(self.raw(), "max_length")
    }

    /// Returns the maximum number of newly generated tokens.
    #[must_use]
    pub fn max_new_tokens(&self) -> SourceField<'a, u64> {
        u64_field(self.raw(), "max_new_tokens")
    }

    /// Returns the minimum total sequence length.
    #[must_use]
    pub fn min_length(&self) -> SourceField<'a, u64> {
        u64_field(self.raw(), "min_length")
    }

    /// Returns the minimum number of newly generated tokens.
    #[must_use]
    pub fn min_new_tokens(&self) -> SourceField<'a, u64> {
        u64_field(self.raw(), "min_new_tokens")
    }

    /// Returns whether source generation enables sampling.
    #[must_use]
    pub fn do_sample(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "do_sample")
    }

    /// Returns the beam count.
    #[must_use]
    pub fn num_beams(&self) -> SourceField<'a, u64> {
        u64_field(self.raw(), "num_beams")
    }

    /// Returns the sampling temperature.
    #[must_use]
    pub fn temperature(&self) -> SourceField<'a, f64> {
        f64_field(self.raw(), "temperature")
    }

    /// Returns the top-k sampling limit.
    #[must_use]
    pub fn top_k(&self) -> SourceField<'a, u64> {
        u64_field(self.raw(), "top_k")
    }

    /// Returns the top-p sampling threshold.
    #[must_use]
    pub fn top_p(&self) -> SourceField<'a, f64> {
        f64_field(self.raw(), "top_p")
    }

    /// Returns the typical-p sampling threshold.
    #[must_use]
    pub fn typical_p(&self) -> SourceField<'a, f64> {
        f64_field(self.raw(), "typical_p")
    }

    /// Returns the repetition penalty.
    #[must_use]
    pub fn repetition_penalty(&self) -> SourceField<'a, f64> {
        f64_field(self.raw(), "repetition_penalty")
    }

    /// Returns the beam-search length penalty.
    #[must_use]
    pub fn length_penalty(&self) -> SourceField<'a, f64> {
        f64_field(self.raw(), "length_penalty")
    }

    /// Returns the no-repeat n-gram size.
    #[must_use]
    pub fn no_repeat_ngram_size(&self) -> SourceField<'a, u64> {
        u64_field(self.raw(), "no_repeat_ngram_size")
    }

    /// Returns the unmodified polymorphic early-stopping setting.
    #[must_use]
    pub fn early_stopping(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "early_stopping")
    }

    /// Returns whether generation requests a key/value cache.
    #[must_use]
    pub fn use_cache(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "use_cache")
    }

    /// Returns the unmodified beginning-of-sequence token identifier.
    #[must_use]
    pub fn bos_token_id(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "bos_token_id")
    }

    /// Returns the unmodified end-of-sequence token identifier or identifiers.
    #[must_use]
    pub fn eos_token_id(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "eos_token_id")
    }

    /// Returns the unmodified padding token identifier.
    #[must_use]
    pub fn pad_token_id(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "pad_token_id")
    }

    /// Returns the unmodified decoder-start token identifier.
    #[must_use]
    pub fn decoder_start_token_id(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "decoder_start_token_id")
    }

    /// Returns the unmodified bad-words token sequences.
    #[must_use]
    pub fn bad_words_ids(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "bad_words_ids")
    }

    /// Returns the unmodified forced beginning-of-sequence identifier.
    #[must_use]
    pub fn forced_bos_token_id(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "forced_bos_token_id")
    }

    /// Returns the unmodified forced end-of-sequence identifier or identifiers.
    #[must_use]
    pub fn forced_eos_token_id(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "forced_eos_token_id")
    }

    /// Returns the unmodified stop string or string list.
    #[must_use]
    pub fn stop_strings(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "stop_strings")
    }

    /// Returns the requested generation cache implementation.
    #[must_use]
    pub fn cache_implementation(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "cache_implementation")
    }
}

json_view!(
    /// Typed fields from a Transformers `tokenizer_config.json` document.
    TokenizerConfigView,
    [
        "tokenizer_class",
        "auto_map",
        "model_max_length",
        "padding_side",
        "truncation_side",
        "clean_up_tokenization_spaces",
        "add_prefix_space",
        "bos_token",
        "eos_token",
        "unk_token",
        "sep_token",
        "pad_token",
        "cls_token",
        "mask_token",
        "additional_special_tokens",
        "added_tokens_decoder",
        "chat_template",
    ]
);

impl<'a> TokenizerConfigView<'a> {
    /// Returns the tokenizer class named by the source.
    #[must_use]
    pub fn tokenizer_class(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "tokenizer_class")
    }

    /// Returns the unmodified automatic-class mapping.
    #[must_use]
    pub fn auto_map(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "auto_map")
    }

    /// Returns the unmodified model maximum length.
    #[must_use]
    pub fn model_max_length(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "model_max_length")
    }

    /// Returns the tokenizer padding side.
    #[must_use]
    pub fn padding_side(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "padding_side")
    }

    /// Returns the tokenizer truncation side.
    #[must_use]
    pub fn truncation_side(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "truncation_side")
    }

    /// Returns whether decoded text cleanup was explicitly requested.
    #[must_use]
    pub fn clean_up_tokenization_spaces(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "clean_up_tokenization_spaces")
    }

    /// Returns whether the tokenizer explicitly adds a prefix space.
    #[must_use]
    pub fn add_prefix_space(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "add_prefix_space")
    }

    /// Returns the beginning-of-sequence token value.
    #[must_use]
    pub fn bos_token(&self) -> SourceField<'a, SpecialTokenValue<'a>> {
        special_token_field(self.raw(), "bos_token")
    }

    /// Returns the unmodified end-of-sequence token value.
    #[must_use]
    pub fn eos_token(&self) -> SourceField<'a, SpecialTokenValue<'a>> {
        special_token_field(self.raw(), "eos_token")
    }

    /// Returns the unmodified unknown token value.
    #[must_use]
    pub fn unk_token(&self) -> SourceField<'a, SpecialTokenValue<'a>> {
        special_token_field(self.raw(), "unk_token")
    }

    /// Returns the unmodified separator token value.
    #[must_use]
    pub fn sep_token(&self) -> SourceField<'a, SpecialTokenValue<'a>> {
        special_token_field(self.raw(), "sep_token")
    }

    /// Returns the unmodified padding token value.
    #[must_use]
    pub fn pad_token(&self) -> SourceField<'a, SpecialTokenValue<'a>> {
        special_token_field(self.raw(), "pad_token")
    }

    /// Returns the unmodified classification token value.
    #[must_use]
    pub fn cls_token(&self) -> SourceField<'a, SpecialTokenValue<'a>> {
        special_token_field(self.raw(), "cls_token")
    }

    /// Returns the unmodified mask token value.
    #[must_use]
    pub fn mask_token(&self) -> SourceField<'a, SpecialTokenValue<'a>> {
        special_token_field(self.raw(), "mask_token")
    }

    /// Returns the unmodified additional special tokens.
    #[must_use]
    pub fn additional_special_tokens(&self) -> SourceField<'a, SpecialTokenValues<'a>> {
        special_token_values_field(self.raw(), "additional_special_tokens")
    }

    /// Returns the unmodified added-token decoder mapping.
    #[must_use]
    pub fn added_tokens_decoder(&self) -> SourceField<'a, AddedTokenDecoderEntries<'a>> {
        added_token_decoder_field(self.raw(), "added_tokens_decoder")
    }

    /// Returns the unmodified inline chat template value.
    #[must_use]
    pub fn chat_template(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "chat_template")
    }
}

json_view!(
    /// Typed fields from a `special_tokens_map.json` document.
    SpecialTokensMapView,
    [
        "bos_token",
        "eos_token",
        "unk_token",
        "sep_token",
        "pad_token",
        "cls_token",
        "mask_token",
        "additional_special_tokens",
    ]
);

impl<'a> SpecialTokensMapView<'a> {
    /// Returns the unmodified beginning-of-sequence token value.
    #[must_use]
    pub fn bos_token(&self) -> SourceField<'a, SpecialTokenValue<'a>> {
        special_token_field(self.raw(), "bos_token")
    }

    /// Returns the unmodified end-of-sequence token value.
    #[must_use]
    pub fn eos_token(&self) -> SourceField<'a, SpecialTokenValue<'a>> {
        special_token_field(self.raw(), "eos_token")
    }

    /// Returns the unmodified unknown token value.
    #[must_use]
    pub fn unk_token(&self) -> SourceField<'a, SpecialTokenValue<'a>> {
        special_token_field(self.raw(), "unk_token")
    }

    /// Returns the unmodified separator token value.
    #[must_use]
    pub fn sep_token(&self) -> SourceField<'a, SpecialTokenValue<'a>> {
        special_token_field(self.raw(), "sep_token")
    }

    /// Returns the unmodified padding token value.
    #[must_use]
    pub fn pad_token(&self) -> SourceField<'a, SpecialTokenValue<'a>> {
        special_token_field(self.raw(), "pad_token")
    }

    /// Returns the unmodified classification token value.
    #[must_use]
    pub fn cls_token(&self) -> SourceField<'a, SpecialTokenValue<'a>> {
        special_token_field(self.raw(), "cls_token")
    }

    /// Returns the unmodified mask token value.
    #[must_use]
    pub fn mask_token(&self) -> SourceField<'a, SpecialTokenValue<'a>> {
        special_token_field(self.raw(), "mask_token")
    }

    /// Returns the unmodified additional special tokens.
    #[must_use]
    pub fn additional_special_tokens(&self) -> SourceField<'a, SpecialTokenValues<'a>> {
        special_token_values_field(self.raw(), "additional_special_tokens")
    }
}

json_view!(
    /// Typed fields from a `preprocessor_config.json` document.
    PreprocessorConfigView,
    [
        "feature_extractor_type",
        "image_processor_type",
        "processor_class",
        "do_resize",
        "size",
        "do_center_crop",
        "crop_size",
        "resample",
        "do_rescale",
        "rescale_factor",
        "do_normalize",
        "image_mean",
        "image_std",
        "return_attention_mask",
        "sampling_rate",
    ]
);

impl<'a> PreprocessorConfigView<'a> {
    /// Returns the feature-extractor class named by the source.
    #[must_use]
    pub fn feature_extractor_type(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "feature_extractor_type")
    }

    /// Returns the image-processor class named by the source.
    #[must_use]
    pub fn image_processor_type(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "image_processor_type")
    }

    /// Returns the composite processor class named by the source.
    #[must_use]
    pub fn processor_class(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "processor_class")
    }

    /// Returns whether resizing is explicitly enabled.
    #[must_use]
    pub fn do_resize(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "do_resize")
    }

    /// Returns the unmodified scalar or structured resize dimensions.
    #[must_use]
    pub fn size(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "size")
    }

    /// Returns whether center cropping is explicitly enabled.
    #[must_use]
    pub fn do_center_crop(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "do_center_crop")
    }

    /// Returns the unmodified scalar or structured crop dimensions.
    #[must_use]
    pub fn crop_size(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "crop_size")
    }

    /// Returns the unmodified resampling algorithm value.
    #[must_use]
    pub fn resample(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "resample")
    }

    /// Returns whether pixel rescaling is explicitly enabled.
    #[must_use]
    pub fn do_rescale(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "do_rescale")
    }

    /// Returns the pixel rescaling factor.
    #[must_use]
    pub fn rescale_factor(&self) -> SourceField<'a, f64> {
        f64_field(self.raw(), "rescale_factor")
    }

    /// Returns whether normalization is explicitly enabled.
    #[must_use]
    pub fn do_normalize(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "do_normalize")
    }

    /// Returns the unmodified per-channel normalization means.
    #[must_use]
    pub fn image_mean(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "image_mean")
    }

    /// Returns the unmodified per-channel normalization standard deviations.
    #[must_use]
    pub fn image_std(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "image_std")
    }

    /// Returns whether an attention mask is explicitly requested.
    #[must_use]
    pub fn return_attention_mask(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "return_attention_mask")
    }

    /// Returns the source-provided audio sampling rate.
    #[must_use]
    pub fn sampling_rate(&self) -> SourceField<'a, u64> {
        u64_field(self.raw(), "sampling_rate")
    }
}

json_view!(
    /// Typed fields from a `processor_config.json` document.
    ProcessorConfigView,
    [
        "processor_class",
        "auto_map",
        "chat_template",
        "image_processor_type",
        "feature_extractor_type",
        "tokenizer_class",
        "num_additional_image_tokens",
        "patch_size",
        "vision_feature_select_strategy",
    ]
);

impl<'a> ProcessorConfigView<'a> {
    /// Returns the composite processor class named by the source.
    #[must_use]
    pub fn processor_class(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "processor_class")
    }

    /// Returns the unmodified automatic-class mapping.
    #[must_use]
    pub fn auto_map(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "auto_map")
    }

    /// Returns the unmodified inline chat template value.
    #[must_use]
    pub fn chat_template(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "chat_template")
    }

    /// Returns the image-processor class named by the source.
    #[must_use]
    pub fn image_processor_type(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "image_processor_type")
    }

    /// Returns the feature-extractor class named by the source.
    #[must_use]
    pub fn feature_extractor_type(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "feature_extractor_type")
    }

    /// Returns the tokenizer class named by the source.
    #[must_use]
    pub fn tokenizer_class(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "tokenizer_class")
    }

    /// Returns the number of additional image tokens.
    #[must_use]
    pub fn num_additional_image_tokens(&self) -> SourceField<'a, u64> {
        u64_field(self.raw(), "num_additional_image_tokens")
    }

    /// Returns the source-provided vision patch size.
    #[must_use]
    pub fn patch_size(&self) -> SourceField<'a, u64> {
        u64_field(self.raw(), "patch_size")
    }

    /// Returns the vision feature selection strategy.
    #[must_use]
    pub fn vision_feature_select_strategy(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "vision_feature_select_strategy")
    }
}

json_view!(
    /// Typed fields from a Diffusers `scheduler_config.json` document.
    SchedulerConfigView,
    [
        "_class_name",
        "_diffusers_version",
        "beta_start",
        "beta_end",
        "beta_schedule",
        "trained_betas",
        "num_train_timesteps",
        "prediction_type",
        "timestep_spacing",
        "steps_offset",
        "clip_sample",
        "set_alpha_to_one",
        "skip_prk_steps",
        "variance_type",
    ]
);

impl<'a> SchedulerConfigView<'a> {
    /// Returns the scheduler class named by the source.
    #[must_use]
    pub fn class_name(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "_class_name")
    }

    /// Returns the Diffusers version recorded by the source.
    #[must_use]
    pub fn diffusers_version(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "_diffusers_version")
    }

    /// Returns the starting beta value.
    #[must_use]
    pub fn beta_start(&self) -> SourceField<'a, f64> {
        f64_field(self.raw(), "beta_start")
    }

    /// Returns the ending beta value.
    #[must_use]
    pub fn beta_end(&self) -> SourceField<'a, f64> {
        f64_field(self.raw(), "beta_end")
    }

    /// Returns the beta schedule name.
    #[must_use]
    pub fn beta_schedule(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "beta_schedule")
    }

    /// Returns unmodified source-provided trained beta values.
    #[must_use]
    pub fn trained_betas(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "trained_betas")
    }

    /// Returns the number of training timesteps.
    #[must_use]
    pub fn num_train_timesteps(&self) -> SourceField<'a, u64> {
        u64_field(self.raw(), "num_train_timesteps")
    }

    /// Returns the scheduler prediction type.
    #[must_use]
    pub fn prediction_type(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "prediction_type")
    }

    /// Returns the scheduler timestep spacing strategy.
    #[must_use]
    pub fn timestep_spacing(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "timestep_spacing")
    }

    /// Returns the scheduler timestep offset.
    #[must_use]
    pub fn steps_offset(&self) -> SourceField<'a, i64> {
        i64_field(self.raw(), "steps_offset")
    }

    /// Returns whether source samples are clipped.
    #[must_use]
    pub fn clip_sample(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "clip_sample")
    }

    /// Returns whether the final cumulative alpha is set to one.
    #[must_use]
    pub fn set_alpha_to_one(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "set_alpha_to_one")
    }

    /// Returns whether pseudo Runge-Kutta steps are skipped.
    #[must_use]
    pub fn skip_prk_steps(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "skip_prk_steps")
    }

    /// Returns the scheduler variance type.
    #[must_use]
    pub fn variance_type(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "variance_type")
    }
}

json_view!(
    /// Typed fields from an `adapter_config.json` document.
    AdapterConfigView,
    [
        "peft_type",
        "task_type",
        "base_model_name_or_path",
        "revision",
        "inference_mode",
        "r",
        "lora_alpha",
        "lora_dropout",
        "fan_in_fan_out",
        "bias",
        "target_modules",
        "modules_to_save",
        "auto_mapping",
        "use_dora",
        "use_rslora",
    ]
);

impl<'a> AdapterConfigView<'a> {
    /// Returns the PEFT adapter type.
    #[must_use]
    pub fn peft_type(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "peft_type")
    }

    /// Returns the source-provided PEFT task type.
    #[must_use]
    pub fn task_type(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "task_type")
    }

    /// Returns the base model repository identifier or path.
    #[must_use]
    pub fn base_model_name_or_path(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "base_model_name_or_path")
    }

    /// Returns the base model revision.
    #[must_use]
    pub fn revision(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "revision")
    }

    /// Returns whether the adapter is marked for inference.
    #[must_use]
    pub fn inference_mode(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "inference_mode")
    }

    /// Returns the low-rank adapter rank.
    #[must_use]
    pub fn rank(&self) -> SourceField<'a, u64> {
        u64_field(self.raw(), "r")
    }

    /// Returns the low-rank adapter alpha value.
    #[must_use]
    pub fn lora_alpha(&self) -> SourceField<'a, f64> {
        f64_field(self.raw(), "lora_alpha")
    }

    /// Returns the low-rank adapter dropout probability.
    #[must_use]
    pub fn lora_dropout(&self) -> SourceField<'a, f64> {
        f64_field(self.raw(), "lora_dropout")
    }

    /// Returns the source-provided fan-in/fan-out setting.
    #[must_use]
    pub fn fan_in_fan_out(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "fan_in_fan_out")
    }

    /// Returns the `LoRA` bias mode.
    #[must_use]
    pub fn bias(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "bias")
    }

    /// Returns the unmodified target-module selector or list.
    #[must_use]
    pub fn target_modules(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "target_modules")
    }

    /// Returns the unmodified list of additional modules to save.
    #[must_use]
    pub fn modules_to_save(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "modules_to_save")
    }

    /// Returns the unmodified automatic class mapping.
    #[must_use]
    pub fn auto_mapping(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "auto_mapping")
    }

    /// Returns whether weight-decomposed `LoRA` is enabled.
    #[must_use]
    pub fn use_dora(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "use_dora")
    }

    /// Returns whether rank-stabilized `LoRA` is enabled.
    #[must_use]
    pub fn use_rslora(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "use_rslora")
    }
}

json_view!(
    /// Typed fields from a `quantization_config.json` document.
    QuantizationConfigView,
    [
        "quant_method",
        "bits",
        "load_in_4bit",
        "load_in_8bit",
        "bnb_4bit_compute_dtype",
        "bnb_4bit_quant_type",
        "bnb_4bit_use_double_quant",
        "llm_int8_threshold",
        "llm_int8_has_fp16_weight",
        "llm_int8_enable_fp32_cpu_offload",
        "llm_int8_skip_modules",
        "modules_to_not_convert",
        "config_groups",
    ]
);

impl<'a> QuantizationConfigView<'a> {
    /// Returns the quantization method identifier.
    #[must_use]
    pub fn quant_method(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "quant_method")
    }

    /// Returns the target bit width.
    #[must_use]
    pub fn bits(&self) -> SourceField<'a, u64> {
        u64_field(self.raw(), "bits")
    }

    /// Returns whether four-bit loading is explicitly enabled.
    #[must_use]
    pub fn load_in_4bit(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "load_in_4bit")
    }

    /// Returns whether eight-bit loading is explicitly enabled.
    #[must_use]
    pub fn load_in_8bit(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "load_in_8bit")
    }

    /// Returns the bitsandbytes four-bit compute data type.
    #[must_use]
    pub fn bnb_4bit_compute_dtype(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "bnb_4bit_compute_dtype")
    }

    /// Returns the bitsandbytes four-bit quantization type.
    #[must_use]
    pub fn bnb_4bit_quant_type(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "bnb_4bit_quant_type")
    }

    /// Returns whether bitsandbytes double quantization is enabled.
    #[must_use]
    pub fn bnb_4bit_use_double_quant(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "bnb_4bit_use_double_quant")
    }

    /// Returns the bitsandbytes eight-bit outlier threshold.
    #[must_use]
    pub fn llm_int8_threshold(&self) -> SourceField<'a, f64> {
        f64_field(self.raw(), "llm_int8_threshold")
    }

    /// Returns whether eight-bit weights retain FP16 copies.
    #[must_use]
    pub fn llm_int8_has_fp16_weight(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "llm_int8_has_fp16_weight")
    }

    /// Returns whether FP32 CPU offload is enabled.
    #[must_use]
    pub fn llm_int8_enable_fp32_cpu_offload(&self) -> SourceField<'a, bool> {
        bool_field(self.raw(), "llm_int8_enable_fp32_cpu_offload")
    }

    /// Returns the unmodified modules skipped by eight-bit conversion.
    #[must_use]
    pub fn llm_int8_skip_modules(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "llm_int8_skip_modules")
    }

    /// Returns the unmodified modules excluded from conversion.
    #[must_use]
    pub fn modules_to_not_convert(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "modules_to_not_convert")
    }

    /// Returns the unmodified provider-specific configuration groups.
    #[must_use]
    pub fn config_groups(&self) -> SourceField<'a, &'a Value> {
        raw_field(self.raw(), "config_groups")
    }
}

const MODEL_INDEX_FIELDS: &[&str] = &[
    "_class_name",
    "_diffusers_version",
    "_name_or_path",
    "_ignore_files",
    "pipeline_tag",
    "task",
    "auto_map",
    "auto_mapping",
    "custom_pipeline",
    "trust_remote_code",
];

pub(crate) fn is_model_index_metadata(name: &str) -> bool {
    name.starts_with('_') || MODEL_INDEX_FIELDS.contains(&name)
}

fn is_component_tuple(name: &str, value: &Value) -> bool {
    if is_model_index_metadata(name) {
        return false;
    }
    let Value::Array(values) = value else {
        return false;
    };
    values.len() == 2 && values.iter().all(|item| item.is_null() || item.is_string())
}

/// A Diffusers component tuple interpreted without importing its library.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum DiffusersComponentValue<'a> {
    /// A component class with an optional source library name.
    Reference {
        /// Source library, or `None` for a custom/local component.
        library: Option<&'a str>,
        /// Component class name retained as inert data.
        class_name: &'a str,
    },
    /// An optional component represented by `[null, null]`.
    Optional,
    /// A tuple-shaped value with an unsupported null placement.
    Invalid(&'a Value),
}

/// One component entry from a Diffusers pipeline index.
#[derive(Clone, Copy, Debug)]
pub struct DiffusersComponent<'a> {
    name: &'a str,
    raw: &'a Value,
}

impl<'a> DiffusersComponent<'a> {
    /// Returns the component field name and relative-directory candidate.
    #[must_use]
    pub const fn name(&self) -> &'a str {
        self.name
    }

    /// Returns the unmodified source tuple.
    #[must_use]
    pub const fn raw(&self) -> &'a Value {
        self.raw
    }

    /// Interprets the source tuple without loading its library or class.
    #[must_use]
    pub fn value(&self) -> DiffusersComponentValue<'a> {
        let Value::Array(values) = self.raw else {
            return DiffusersComponentValue::Invalid(self.raw);
        };
        match values.as_slice() {
            [Value::String(library), Value::String(class_name)] => {
                DiffusersComponentValue::Reference {
                    library: Some(library),
                    class_name,
                }
            }
            [Value::Null, Value::String(class_name)] => DiffusersComponentValue::Reference {
                library: None,
                class_name,
            },
            [Value::Null, Value::Null] => DiffusersComponentValue::Optional,
            _ => DiffusersComponentValue::Invalid(self.raw),
        }
    }
}

/// Iterator over component tuples in a Diffusers pipeline index.
pub struct DiffusersComponents<'a> {
    fields: serde_json::map::Iter<'a>,
}

impl fmt::Debug for DiffusersComponents<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DiffusersComponents")
            .finish_non_exhaustive()
    }
}

impl<'a> Iterator for DiffusersComponents<'a> {
    type Item = DiffusersComponent<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        for (name, value) in self.fields.by_ref() {
            if is_component_tuple(name, value) {
                return Some(DiffusersComponent {
                    name: name.as_str(),
                    raw: value,
                });
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, self.fields.size_hint().1)
    }
}

/// Iterator over non-component fields not interpreted by a model-index view.
pub struct ModelIndexExtraFields<'a> {
    fields: serde_json::map::Iter<'a>,
}

impl fmt::Debug for ModelIndexExtraFields<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ModelIndexExtraFields")
            .finish_non_exhaustive()
    }
}

impl<'a> Iterator for ModelIndexExtraFields<'a> {
    type Item = (&'a str, &'a Value);

    fn next(&mut self) -> Option<Self::Item> {
        for (name, value) in self.fields.by_ref() {
            if !is_model_index_metadata(name) && !is_component_tuple(name, value) {
                return Some((name.as_str(), value));
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, self.fields.size_hint().1)
    }
}

/// Typed fields and components from a Diffusers `model_index.json` document.
#[derive(Clone, Copy, Debug)]
pub struct ModelIndexView<'a> {
    inner: ObjectView<'a>,
}

impl<'a> ModelIndexView<'a> {
    fn new(source: &'a SourceDocument) -> Result<Self, ViewError> {
        ObjectView::new(source).map(|inner| Self { inner })
    }

    /// Returns the lossless source document backing this view.
    #[must_use]
    pub const fn source(&self) -> &'a SourceDocument {
        self.inner.source
    }

    /// Returns the unmodified source JSON object.
    #[must_use]
    pub const fn raw(&self) -> &'a Map<String, Value> {
        self.inner.object
    }

    /// Returns an unmodified source field by name.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&'a Value> {
        self.raw().get(key)
    }

    /// Returns the pipeline class named by the source.
    #[must_use]
    pub fn class_name(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "_class_name")
    }

    /// Returns the Diffusers version recorded by the source.
    #[must_use]
    pub fn diffusers_version(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "_diffusers_version")
    }

    /// Returns the source-provided repository path or identifier.
    #[must_use]
    pub fn name_or_path(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "_name_or_path")
    }

    /// Returns the explicit pipeline tag.
    #[must_use]
    pub fn pipeline_tag(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "pipeline_tag")
    }

    /// Returns the source-provided legacy task alias.
    #[must_use]
    pub fn task(&self) -> SourceField<'a, &'a str> {
        string_field(self.raw(), "task")
    }

    /// Iterates over inert component tuples in source order.
    #[must_use]
    pub fn components(&self) -> DiffusersComponents<'a> {
        DiffusersComponents {
            fields: self.raw().iter(),
        }
    }

    /// Iterates over fields that are neither metadata nor component tuples.
    #[must_use]
    pub fn extra(&self) -> ModelIndexExtraFields<'a> {
        ModelIndexExtraFields {
            fields: self.raw().iter(),
        }
    }
}

/// A borrowed view of a `chat_template.jinja` source document.
#[derive(Clone, Copy, Debug)]
pub struct ChatTemplateView<'a> {
    source: &'a SourceDocument,
}

impl<'a> ChatTemplateView<'a> {
    /// Returns the lossless source document backing this view.
    #[must_use]
    pub const fn source(&self) -> &'a SourceDocument {
        self.source
    }

    /// Returns the exact template bytes.
    #[must_use]
    pub fn raw(&self) -> &'a [u8] {
        self.source.original()
    }

    /// Returns the template as UTF-8 text.
    ///
    /// # Errors
    ///
    /// Returns an error if the exact source bytes are not valid UTF-8.
    pub fn text(&self) -> Result<&'a str, Utf8Error> {
        std::str::from_utf8(self.raw())
    }
}

/// A typed view of safetensors index metadata.
#[derive(Clone, Copy, Debug)]
pub struct SafetensorsMetadataView<'a> {
    object: &'a Map<String, Value>,
}

impl<'a> SafetensorsMetadataView<'a> {
    /// Returns the unmodified metadata object.
    #[must_use]
    pub const fn raw(&self) -> &'a Map<String, Value> {
        self.object
    }

    /// Returns the declared total tensor byte size.
    #[must_use]
    pub fn total_size(&self) -> SourceField<'a, u64> {
        u64_field(self.raw(), "total_size")
    }

    /// Iterates over metadata fields other than `total_size`.
    #[must_use]
    pub fn extra(&self) -> ExtraFields<'a> {
        ExtraFields {
            fields: self.raw().iter(),
            known: &["total_size"],
        }
    }
}

/// A typed view of tensor-to-shard entries in a safetensors index.
#[derive(Clone, Copy, Debug)]
pub struct SafetensorsWeightMapView<'a> {
    object: &'a Map<String, Value>,
}

impl<'a> SafetensorsWeightMapView<'a> {
    /// Returns the unmodified tensor-to-shard object.
    #[must_use]
    pub const fn raw(&self) -> &'a Map<String, Value> {
        self.object
    }

    /// Returns an unmodified shard value for a tensor name.
    #[must_use]
    pub fn get(&self, tensor_name: &str) -> Option<&'a Value> {
        self.raw().get(tensor_name)
    }

    /// Iterates over tensor names and typed shard path strings.
    #[must_use]
    pub fn entries(&self) -> SafetensorsWeightMapEntries<'a> {
        SafetensorsWeightMapEntries {
            fields: self.raw().iter(),
        }
    }
}

/// Iterator over tensor-to-shard entries in source order.
pub struct SafetensorsWeightMapEntries<'a> {
    fields: serde_json::map::Iter<'a>,
}

impl fmt::Debug for SafetensorsWeightMapEntries<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafetensorsWeightMapEntries")
            .finish_non_exhaustive()
    }
}

impl<'a> Iterator for SafetensorsWeightMapEntries<'a> {
    type Item = (&'a str, SourceField<'a, &'a str>);

    fn next(&mut self) -> Option<Self::Item> {
        self.fields.next().map(|(tensor_name, value)| {
            let shard = match value {
                Value::Null => SourceField::Null,
                Value::String(path) => SourceField::Value(path.as_str()),
                _ => SourceField::Invalid(value),
            };
            (tensor_name.as_str(), shard)
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.fields.size_hint()
    }
}

impl ExactSizeIterator for SafetensorsWeightMapEntries<'_> {}

json_view!(
    /// Typed fields from a `*.safetensors.index.json` document.
    SafetensorsIndexView,
    ["metadata", "weight_map"]
);

impl<'a> SafetensorsIndexView<'a> {
    /// Returns the typed metadata object while preserving invalid source values.
    #[must_use]
    pub fn metadata(&self) -> SourceField<'a, SafetensorsMetadataView<'a>> {
        match self.raw().get("metadata") {
            None => SourceField::Missing,
            Some(Value::Null) => SourceField::Null,
            Some(Value::Object(object)) => SourceField::Value(SafetensorsMetadataView { object }),
            Some(value) => SourceField::Invalid(value),
        }
    }

    /// Returns the typed tensor-to-shard map while preserving invalid values.
    #[must_use]
    pub fn weight_map(&self) -> SourceField<'a, SafetensorsWeightMapView<'a>> {
        match self.raw().get("weight_map") {
            None => SourceField::Missing,
            Some(Value::Null) => SourceField::Null,
            Some(Value::Object(object)) => SourceField::Value(SafetensorsWeightMapView { object }),
            Some(value) => SourceField::Invalid(value),
        }
    }
}
