use std::path::{Component, Path, PathBuf};

use serde::de::Error as _;
use serde::ser::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Maximum UTF-8 bytes in one portable repository-relative path.
pub const MAX_REPOSITORY_PATH_BYTES: usize = 1_024;

/// Maximum UTF-8 bytes in one portable repository path segment.
pub const MAX_REPOSITORY_PATH_SEGMENT_BYTES: usize = 255;

pub(crate) fn serialize<S>(path: &Path, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    validate(path).map_err(S::Error::custom)?;
    portable(path).serialize(serializer)
}

#[expect(
    clippy::ref_option,
    reason = "serde serialize_with passes a reference to the field type"
)]
pub(crate) fn serialize_option<S>(path: &Option<PathBuf>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let portable = path
        .as_ref()
        .map(|value| {
            validate(value).map_err(S::Error::custom)?;
            Ok(portable(value))
        })
        .transpose()?;
    portable.serialize(serializer)
}

pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    let path = PathBuf::from(value);
    validate(&path).map_err(D::Error::custom)?;
    Ok(path)
}

pub(crate) fn deserialize_option<'de, D>(deserializer: D) -> Result<Option<PathBuf>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    value
        .map(|value| {
            let path = PathBuf::from(value);
            validate(&path).map(|()| path).map_err(D::Error::custom)
        })
        .transpose()
}

pub(crate) fn portable(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub(crate) fn validate(path: &Path) -> Result<(), crate::ConfigError> {
    let value = path
        .to_str()
        .ok_or_else(|| crate::ConfigError::NonUtf8Path(path.to_path_buf()))?;
    let invalid_segment = value.split('/').any(|segment| {
        segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.len() > MAX_REPOSITORY_PATH_SEGMENT_BYTES
            || !is_portable_segment(segment)
    });
    if value.is_empty()
        || value.len() > MAX_REPOSITORY_PATH_BYTES
        || path.is_absolute()
        || value.contains(['\\', '\0', ':'])
        || invalid_segment
        || path.components().any(|component| {
            matches!(
                component,
                Component::CurDir
                    | Component::ParentDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
    {
        return Err(crate::ConfigError::UnsafePath(path.to_path_buf()));
    }
    Ok(())
}

fn is_portable_segment(segment: &str) -> bool {
    if segment.ends_with(['.', ' '])
        || segment
            .chars()
            .any(|character| character <= '\u{1f}' || "<>\"|?*".contains(character))
    {
        return false;
    }
    let stem = segment
        .split_once('.')
        .map_or(segment, |(stem, _extension)| stem);
    if stem.ends_with(['.', ' ']) {
        return false;
    }
    let stem = stem.to_ascii_uppercase();
    !matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    ) && !matches_reserved_numbered_name(&stem, "COM")
        && !matches_reserved_numbered_name(&stem, "LPT")
}

fn matches_reserved_numbered_name(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|suffix| {
        matches!(
            suffix,
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
    })
}
