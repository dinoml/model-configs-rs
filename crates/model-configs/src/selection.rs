use std::path::Path;

use serde_json::Value;

use crate::{ChatTemplateError, ConfigError, ModelRepository, SelectionError, SourceDocument};

/// Source selected by a versioned repository-level precedence rule.
#[derive(Clone, Copy, Debug)]
pub struct SourceSelection<'a> {
    document: &'a SourceDocument,
    json_pointer: Option<&'static str>,
}

impl<'a> SourceSelection<'a> {
    /// Returns the selected source document.
    #[must_use]
    pub const fn document(self) -> &'a SourceDocument {
        self.document
    }

    /// Returns a JSON Pointer when the selected value is inline.
    #[must_use]
    pub const fn json_pointer(self) -> Option<&'static str> {
        self.json_pointer
    }
}

/// Effective chat-template value selected without executing Jinja.
#[derive(Clone, Copy, Debug)]
pub enum ChatTemplateValue<'a> {
    /// Standalone UTF-8 Jinja source.
    Text(&'a str),
    /// Inline JSON value, including named template collections.
    Inline(&'a Value),
}

/// Effective chat template and its exact source location.
#[derive(Clone, Copy, Debug)]
pub struct ChatTemplateSelection<'a> {
    /// Selected inert template value.
    pub value: ChatTemplateValue<'a>,
    /// Exact document and optional inline pointer.
    pub source: SourceSelection<'a>,
}

impl ModelRepository {
    /// Selects the root generation source using the `dinoml-v1` profile.
    ///
    /// A root `generation_config.json` always wins, even when malformed. When
    /// absent, root `config.json` is selected only if it contains a legacy
    /// generation field. No values are merged across the two documents.
    #[must_use]
    pub fn generation_source(&self) -> Option<SourceSelection<'_>> {
        self.generation_source_at(Path::new(""))
    }

    /// Selects the generation source inside a component-relative scope.
    ///
    /// The scope must be a non-empty portable repository-relative directory.
    /// Use [`Self::generation_source`] for the repository root.
    ///
    /// # Errors
    ///
    /// Returns an error when `scope` is not a safe portable relative path.
    pub fn generation_source_in(
        &self,
        scope: impl AsRef<Path>,
    ) -> Result<Option<SourceSelection<'_>>, ConfigError> {
        let scope = scope.as_ref();
        crate::path_serde::validate(scope)?;
        Ok(self.generation_source_at(scope))
    }

    fn generation_source_at(&self, scope: &Path) -> Option<SourceSelection<'_>> {
        if let Some(document) = self.document(scope.join("generation_config.json")) {
            return Some(SourceSelection {
                document,
                json_pointer: None,
            });
        }
        let config = self.document(scope.join("config.json"))?;
        if config.has_duplicate_keys() {
            return None;
        }
        let object = config.json()?.as_object()?;
        GENERATION_FIELDS
            .iter()
            .any(|field| object.contains_key(*field))
            .then_some(SourceSelection {
                document: config,
                json_pointer: None,
            })
    }

    /// Selects the tokenizer chat template using the `dinoml-v1` profile.
    ///
    /// # Errors
    ///
    /// Returns an error when a standalone `chat_template.jinja` exists but is
    /// not valid UTF-8. Its presence blocks inline fallback.
    pub fn tokenizer_chat_template(
        &self,
    ) -> Result<Option<ChatTemplateSelection<'_>>, ChatTemplateError> {
        self.chat_template_from_at(Path::new(""), "tokenizer_config.json")
    }

    /// Selects the tokenizer chat template inside a component-relative scope.
    ///
    /// The scope must be a non-empty portable repository-relative directory.
    /// Use [`Self::tokenizer_chat_template`] for the repository root.
    ///
    /// # Errors
    ///
    /// Returns an error when `scope` is unsafe or when the selected standalone
    /// `chat_template.jinja` is not valid UTF-8.
    pub fn tokenizer_chat_template_in(
        &self,
        scope: impl AsRef<Path>,
    ) -> Result<Option<ChatTemplateSelection<'_>>, SelectionError> {
        let scope = scope.as_ref();
        crate::path_serde::validate(scope)?;
        self.chat_template_from_at(scope, "tokenizer_config.json")
            .map_err(SelectionError::from)
    }

    /// Selects the processor chat template using the `dinoml-v1` profile.
    ///
    /// # Errors
    ///
    /// Returns an error when a standalone `chat_template.jinja` exists but is
    /// not valid UTF-8. Its presence blocks inline fallback.
    pub fn processor_chat_template(
        &self,
    ) -> Result<Option<ChatTemplateSelection<'_>>, ChatTemplateError> {
        self.chat_template_from_at(Path::new(""), "processor_config.json")
    }

    /// Selects the processor chat template inside a component-relative scope.
    ///
    /// The scope must be a non-empty portable repository-relative directory.
    /// Use [`Self::processor_chat_template`] for the repository root.
    ///
    /// # Errors
    ///
    /// Returns an error when `scope` is unsafe or when the selected standalone
    /// `chat_template.jinja` is not valid UTF-8.
    pub fn processor_chat_template_in(
        &self,
        scope: impl AsRef<Path>,
    ) -> Result<Option<ChatTemplateSelection<'_>>, SelectionError> {
        let scope = scope.as_ref();
        crate::path_serde::validate(scope)?;
        self.chat_template_from_at(scope, "processor_config.json")
            .map_err(SelectionError::from)
    }

    fn chat_template_from_at(
        &self,
        scope: &Path,
        inline_document: &str,
    ) -> Result<Option<ChatTemplateSelection<'_>>, ChatTemplateError> {
        if let Some(document) = self.document(scope.join("chat_template.jinja")) {
            let content = document
                .text()
                .map_err(|source| ChatTemplateError::InvalidUtf8 {
                    path: document.relative_path().to_path_buf(),
                    source,
                })?;
            let Some(content) = content else {
                return Ok(None);
            };
            return Ok(Some(ChatTemplateSelection {
                value: ChatTemplateValue::Text(content),
                source: SourceSelection {
                    document,
                    json_pointer: None,
                },
            }));
        }
        let Some(document) = self.document(scope.join(inline_document)) else {
            return Ok(None);
        };
        if document.has_duplicate_keys() {
            return Ok(None);
        }
        let value = document
            .json()
            .and_then(Value::as_object)
            .and_then(|object| object.get("chat_template"))
            .filter(|value| matches!(value, Value::String(_) | Value::Object(_) | Value::Array(_)));
        Ok(value.map(|value| ChatTemplateSelection {
            value: ChatTemplateValue::Inline(value),
            source: SourceSelection {
                document,
                json_pointer: Some("/chat_template"),
            },
        }))
    }
}

const GENERATION_FIELDS: &[&str] = &[
    "bad_words_ids",
    "begin_suppress_tokens",
    "bos_token_id",
    "constraints",
    "decoder_start_token_id",
    "diversity_penalty",
    "do_sample",
    "early_stopping",
    "eos_token_id",
    "eta_cutoff",
    "epsilon_cutoff",
    "exponential_decay_length_penalty",
    "force_words_ids",
    "forced_bos_token_id",
    "forced_decoder_ids",
    "forced_eos_token_id",
    "length_penalty",
    "max_length",
    "max_new_tokens",
    "min_length",
    "min_new_tokens",
    "no_repeat_ngram_size",
    "num_beams",
    "num_beam_groups",
    "num_return_sequences",
    "pad_token_id",
    "penalty_alpha",
    "repetition_penalty",
    "remove_invalid_values",
    "renormalize_logits",
    "sequence_bias",
    "suppress_tokens",
    "temperature",
    "top_k",
    "top_p",
    "typical_p",
];
