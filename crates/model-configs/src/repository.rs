use std::path::{Path, PathBuf};

use crate::normalize::normalize;
use crate::{ConfigError, ModelRepositoryConfig, SourceDocument};

/// Severity of a repository diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticLevel {
    /// Informational compatibility note.
    Info,
    /// A likely repository configuration problem.
    Warning,
    /// A reference that prevents reliable normalization.
    Error,
}

/// A validation message tied to an optional source path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    /// Severity of the finding.
    pub level: DiagnosticLevel,
    /// Stable machine-readable diagnostic code.
    pub code: &'static str,
    /// Human-readable explanation.
    pub message: String,
    /// Related repository-relative path.
    pub path: Option<PathBuf>,
}

/// Parsed configuration documents from one local model repository snapshot.
#[derive(Debug)]
pub struct ModelRepository {
    root: PathBuf,
    documents: Vec<SourceDocument>,
}

impl ModelRepository {
    /// Reads every supported configuration document beneath `root`.
    ///
    /// # Errors
    /// Returns an error if a recognized document cannot be read or parsed.
    pub fn read(root: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let root = root.as_ref().to_path_buf();
        let mut paths = Vec::new();
        collect_paths(&root, &root, &mut paths)?;
        paths.sort();
        let documents = paths
            .into_iter()
            .map(|path| SourceDocument::read(&root, path))
            .collect::<Result<_, _>>()?;
        Ok(Self { root, documents })
    }

    /// Returns the repository root used for reads and validation.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns source documents in stable path order.
    #[must_use]
    pub fn documents(&self) -> &[SourceDocument] {
        &self.documents
    }

    /// Builds a normalized view from the root pipeline or model configuration.
    #[must_use]
    pub fn normalized(&self) -> Option<ModelRepositoryConfig> {
        normalize(&self.documents)
    }

    /// Validates component-relative directory references.
    #[must_use]
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        let Some(config) = self.normalized() else {
            return vec![Diagnostic {
                level: DiagnosticLevel::Error,
                code: "missing_root_config",
                message: "repository has no normalizable root config.json or model_index.json"
                    .into(),
                path: None,
            }];
        };
        config
            .components
            .iter()
            .filter_map(|component| {
                let path = component.path.as_ref()?;
                (!self.root.join(path).is_dir()).then(|| Diagnostic {
                    level: DiagnosticLevel::Error,
                    code: "missing_component_directory",
                    message: format!(
                        "component '{}' refers to missing directory {}",
                        component.name,
                        path.display()
                    ),
                    path: Some(path.clone()),
                })
            })
            .collect()
    }
}

fn collect_paths(
    root: &Path,
    directory: &Path,
    paths: &mut Vec<PathBuf>,
) -> Result<(), ConfigError> {
    let entries = std::fs::read_dir(directory).map_err(|source| ConfigError::Read {
        path: directory.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| ConfigError::Read {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_paths(root, &path, paths)?;
        } else if crate::DocumentKind::from_path(&path).is_some() {
            let relative = path
                .strip_prefix(root)
                .map_or_else(|_| path.clone(), Path::to_path_buf);
            paths.push(relative);
        }
    }
    Ok(())
}
