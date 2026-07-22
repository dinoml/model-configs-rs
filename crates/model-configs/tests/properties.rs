//! Property tests for lossless parsing and forward-compatible normalization.

use std::collections::BTreeMap;

use model_configs::{ModelRepository, SourceDocument};
use proptest::prelude::*;

proptest! {
    #[test]
    fn source_document_retains_every_input_byte(
        leading in 0_usize..8,
        trailing in 0_usize..8,
        value in any::<i64>(),
    ) {
        let source = format!(
            "{}{{\"future\":{value}}}{}",
            " ".repeat(leading),
            "\n".repeat(trailing),
        );
        let document = SourceDocument::parse("config.json", source.as_bytes())?;

        prop_assert_eq!(document.original(), source.as_bytes());
    }

    #[test]
    fn normalization_preserves_arbitrary_unknown_fields(
        unknown in proptest::collection::btree_map("future_[a-z]{1,8}", any::<i64>(), 0..24),
    ) {
        let temp = tempfile::tempdir()?;
        let mut source = serde_json::Map::new();
        source.insert("model_type".into(), "property-model".into());
        for (key, value) in &unknown {
            source.insert(key.clone(), (*value).into());
        }
        std::fs::write(
            temp.path().join("config.json"),
            serde_json::to_vec(&source)?,
        )?;
        let normalized = ModelRepository::read(temp.path())?.normalized()?;
        let actual: BTreeMap<String, i64> = normalized
            .extra
            .iter()
            .filter_map(|(key, value)| value.as_i64().map(|value| (key.clone(), value)))
            .collect();

        prop_assert_eq!(actual, unknown);
    }

    #[test]
    fn every_parent_traversal_document_path_is_rejected(depth in 1_usize..8) {
        let path = format!("{}config.json", "../".repeat(depth));
        let empty_object = [b'{', b'}'];

        prop_assert!(SourceDocument::parse(path, empty_object).is_err());
    }
}
