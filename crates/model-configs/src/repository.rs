use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::manifest::CompatibilityManifest;
use crate::normalize::normalize;
use crate::validation::validate_repository;
use crate::{ConfigError, ModelRepositoryConfig, NormalizationError, SourceDocument};

/// Maximum number of regular files and directories inventoried in one snapshot.
pub const MAX_REPOSITORY_ENTRIES: usize = 250_000;

/// Maximum number of recognized source documents retained in one repository.
pub const MAX_REPOSITORY_DOCUMENTS: usize = 16_384;

/// Maximum exact source bytes retained across all recognized documents.
pub const MAX_REPOSITORY_SOURCE_BYTES: u64 = 256 * 1024 * 1024;

/// Exact logical file and directory inventory for an in-memory snapshot.
///
/// This is the integration seam for content-addressed stores such as
/// `hf-store-rs`: configuration bytes can be parsed independently while shard
/// and component existence is validated from path metadata alone.
#[derive(Clone, Debug, Default)]
pub struct RepositoryInventory {
    files: BTreeMap<String, PathBuf>,
    directories: BTreeMap<String, PathBuf>,
}

impl RepositoryInventory {
    /// Creates an empty logical inventory whose repository root exists implicitly.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            files: BTreeMap::new(),
            directories: BTreeMap::new(),
        }
    }

    /// Adds one regular file path and its parent directories, returning whether
    /// the file itself was newly inserted.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is not a portable repository-relative path
    /// or the bounded inventory limit would be exceeded.
    pub fn insert_file(&mut self, path: impl AsRef<Path>) -> Result<bool, ConfigError> {
        self.insert(path.as_ref(), true)
    }

    /// Adds one directory path and its parents, returning whether the directory
    /// itself was newly inserted.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is not a portable repository-relative path
    /// or the bounded inventory limit would be exceeded.
    pub fn insert_directory(&mut self, path: impl AsRef<Path>) -> Result<bool, ConfigError> {
        self.insert(path.as_ref(), false)
    }

    /// Returns regular files in ascending logical-path order.
    #[must_use]
    pub fn files(&self) -> impl ExactSizeIterator<Item = &Path> {
        self.files.values().map(PathBuf::as_path)
    }

    /// Returns directories in ascending logical-path order.
    #[must_use]
    pub fn directories(&self) -> impl ExactSizeIterator<Item = &Path> {
        self.directories.values().map(PathBuf::as_path)
    }

    fn insert(&mut self, path: &Path, file: bool) -> Result<bool, ConfigError> {
        crate::path_serde::validate(path)?;
        let portable = crate::path_serde::portable(path);
        let already_present = if file {
            self.files.contains_key(&portable)
        } else {
            self.directories.contains_key(&portable)
        };
        let mut parents = Vec::new();
        let mut parent = path.parent();
        while let Some(directory) = parent.filter(|directory| !directory.as_os_str().is_empty()) {
            let portable = crate::path_serde::portable(directory);
            if !self.directories.contains_key(&portable) {
                parents.push((portable, directory.to_path_buf()));
            }
            parent = directory.parent();
        }
        let additions = usize::from(!already_present).saturating_add(parents.len());
        check_entry_limit(
            Path::new(""),
            self.files
                .len()
                .saturating_add(self.directories.len())
                .saturating_add(additions),
        )?;
        let target = if file {
            &mut self.files
        } else {
            &mut self.directories
        };
        let inserted = target.insert(portable, path.to_path_buf()).is_none();
        self.directories.extend(parents);
        Ok(inserted)
    }
}

/// Parsed configuration documents from one local model repository snapshot.
pub struct ModelRepository {
    root: PathBuf,
    documents: Vec<SourceDocument>,
    document_keys: Vec<String>,
    files: Vec<String>,
    directories: Vec<String>,
    scan_diagnostics: Vec<Diagnostic>,
}

impl fmt::Debug for ModelRepository {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ModelRepository")
            .field("document_count", &self.documents.len())
            .field("inventoried_file_count", &self.files.len())
            .field("inventoried_directory_count", &self.directories.len())
            .field("scan_diagnostic_count", &self.scan_diagnostics.len())
            .field("has_filesystem_root", &!self.root.as_os_str().is_empty())
            .finish_non_exhaustive()
    }
}

impl ModelRepository {
    /// Reads every supported configuration document beneath `root`.
    ///
    /// Directory symbolic links are not followed, so a repository cannot make
    /// the scan escape its root through a link.
    ///
    /// # Errors
    ///
    /// Returns an error if the root cannot be enumerated, a recognized document
    /// cannot be read within resource limits, or a path is not portable.
    pub fn read(root: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let root = root.as_ref().to_path_buf();
        let mut paths = Vec::new();
        let mut files = Vec::new();
        let mut directories = Vec::new();
        let mut scan_diagnostics = Vec::new();
        let mut entries_seen = 0;
        collect_paths(
            &root,
            &root,
            &mut paths,
            &mut files,
            &mut directories,
            &mut scan_diagnostics,
            &mut entries_seen,
        )?;
        sort_paths_portably(&mut paths);
        sort_paths_portably(&mut files);
        sort_paths_portably(&mut directories);
        validate_no_portable_collisions(&files, &directories)?;
        let source_sizes = paths
            .iter()
            .map(|path| {
                let filesystem_path = root.join(path);
                let size = std::fs::metadata(&filesystem_path)
                    .map_err(|source| ConfigError::Read {
                        path: filesystem_path,
                        source,
                    })?
                    .len();
                Ok((path.as_path(), size))
            })
            .collect::<Result<Vec<_>, ConfigError>>()?;
        check_document_resource_limits(source_sizes)?;
        let mut documents = Vec::with_capacity(paths.len());
        let mut document_count = 0;
        let mut source_bytes = 0;
        for path in paths {
            let document = SourceDocument::read(&root, path)?;
            check_next_document_resource(
                document.relative_path(),
                document.original().len() as u64,
                &mut document_count,
                &mut source_bytes,
            )?;
            documents.push(document);
        }
        let document_keys = portable_document_keys(&documents);
        Ok(Self {
            root,
            documents,
            document_keys,
            files: portable_paths(files),
            directories: portable_paths(directories),
            scan_diagnostics,
        })
    }

    /// Builds a repository from already-parsed source documents.
    ///
    /// Document paths and their parent directories form the initial logical
    /// inventory. Use [`Self::from_documents_with_inventory`] when validation
    /// must also see weight shards, empty component directories, or other
    /// unsupported files.
    ///
    /// # Errors
    ///
    /// Returns an error for duplicate or colliding document paths or an oversized
    /// inventory.
    pub fn from_documents(documents: Vec<SourceDocument>) -> Result<Self, ConfigError> {
        Self::from_documents_with_inventory(documents, RepositoryInventory::new())
    }

    /// Builds a repository from parsed documents and an exact logical inventory.
    ///
    /// No filesystem or network access occurs. Paths compare case-sensitively and
    /// use portable slash-separated repository identity.
    ///
    /// # Errors
    ///
    /// Returns an error for unsafe, duplicate, or non-portably colliding paths,
    /// an oversized inventory, or repository-wide source resource limits.
    pub fn from_documents_with_inventory(
        mut documents: Vec<SourceDocument>,
        mut inventory: RepositoryInventory,
    ) -> Result<Self, ConfigError> {
        if let Some(document) = documents.get(MAX_REPOSITORY_DOCUMENTS) {
            return Err(ConfigError::RepositoryDocumentLimit {
                path: document.relative_path().to_path_buf(),
                count: documents.len(),
                limit: MAX_REPOSITORY_DOCUMENTS,
            });
        }
        documents
            .sort_by_cached_key(|document| crate::path_serde::portable(document.relative_path()));
        let document_keys = portable_document_keys(&documents);
        check_document_resource_limits(
            documents
                .iter()
                .map(|document| (document.relative_path(), document.original().len() as u64)),
        )?;
        for (index, pair) in document_keys.windows(2).enumerate() {
            if pair[0] == pair[1] {
                return Err(ConfigError::DuplicateDocumentPath(
                    documents[index].relative_path().to_path_buf(),
                ));
            }
        }
        for document in &documents {
            inventory.insert_file(document.relative_path())?;
        }
        let (files, directories) = inventory.into_vectors();
        validate_no_portable_collisions(&files, &directories)?;
        Ok(Self {
            root: PathBuf::new(),
            documents,
            document_keys,
            files: portable_paths(files),
            directories: portable_paths(directories),
            scan_diagnostics: Vec::new(),
        })
    }

    /// Returns the filesystem root used for reads.
    ///
    /// In-memory repositories return an empty path. Reference validation uses
    /// logical inventory paths and never joins against this value.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns source documents in stable path order.
    #[must_use]
    pub fn documents(&self) -> &[SourceDocument] {
        &self.documents
    }

    /// Finds one document by its repository-relative path.
    #[must_use]
    pub fn document(&self, relative_path: impl AsRef<Path>) -> Option<&SourceDocument> {
        let relative_path = relative_path.as_ref();
        crate::path_serde::validate(relative_path).ok()?;
        let portable = crate::path_serde::portable(relative_path);
        self.document_keys
            .binary_search(&portable)
            .ok()
            .map(|index| &self.documents[index])
    }

    /// Builds a normalized view from the root pipeline or model configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when no root configuration exists, its JSON root is not
    /// an object, or it does not provide a data-only architecture identity.
    pub fn normalized(&self) -> Result<ModelRepositoryConfig, NormalizationError> {
        normalize(&self.documents)
    }

    /// Validates paths, component configs, checkpoint shards, and processor links.
    #[must_use]
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        merge_diagnostics(validate_repository(self), &self.scan_diagnostics)
    }

    /// Builds a deterministic compatibility manifest for this snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if a source path cannot be represented as a portable
    /// UTF-8 Hub path.
    pub fn manifest(&self) -> Result<CompatibilityManifest, ConfigError> {
        CompatibilityManifest::from_repository(self)
    }

    pub(crate) fn has_file(&self, relative_path: &Path) -> bool {
        crate::path_serde::validate(relative_path).is_ok()
            && self
                .files
                .binary_search(&crate::path_serde::portable(relative_path))
                .is_ok()
    }

    pub(crate) fn has_directory(&self, relative_path: &Path) -> bool {
        relative_path.as_os_str().is_empty()
            || (crate::path_serde::validate(relative_path).is_ok()
                && self
                    .directories
                    .binary_search(&crate::path_serde::portable(relative_path))
                    .is_ok())
    }

    pub(crate) fn has_entry(&self, relative_path: &Path) -> bool {
        self.has_file(relative_path) || self.has_directory(relative_path)
    }
}

fn merge_diagnostics(content: Vec<Diagnostic>, scan: &[Diagnostic]) -> Vec<Diagnostic> {
    const SCAN_RESERVE: usize = crate::MAX_REPOSITORY_DIAGNOSTICS / 4;
    let content_had_limit = content
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::DiagnosticLimitReached);
    let scan_had_limit = scan
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::DiagnosticLimitReached);
    let content = content
        .into_iter()
        .filter(|diagnostic| diagnostic.code != DiagnosticCode::DiagnosticLimitReached)
        .collect::<Vec<_>>();
    let scan = scan
        .iter()
        .filter(|diagnostic| diagnostic.code != DiagnosticCode::DiagnosticLimitReached)
        .cloned()
        .collect::<Vec<_>>();
    let finding_limit = crate::MAX_REPOSITORY_DIAGNOSTICS - 1;
    let scan_reserve = scan.len().min(SCAN_RESERVE);
    let content_take = content.len().min(finding_limit - scan_reserve);
    let scan_take = scan.len().min(finding_limit - content_take);
    let truncated = content_had_limit
        || scan_had_limit
        || content_take < content.len()
        || scan_take < scan.len();

    let mut diagnostics = Vec::with_capacity(content_take + scan_take + usize::from(truncated));
    diagnostics.extend(content.into_iter().take(content_take));
    diagnostics.extend(scan.into_iter().take(scan_take));
    if truncated {
        diagnostics.push(crate::diagnostic::limit_diagnostic());
    }
    crate::diagnostic::sort_diagnostics(&mut diagnostics);
    diagnostics
}

impl RepositoryInventory {
    fn into_vectors(self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        (
            self.files.into_values().collect(),
            self.directories.into_values().collect(),
        )
    }
}

fn sort_paths_portably(paths: &mut [PathBuf]) {
    paths.sort_by_cached_key(|path| crate::path_serde::portable(path));
}

fn portable_paths(paths: Vec<PathBuf>) -> Vec<String> {
    paths
        .into_iter()
        .map(|path| crate::path_serde::portable(&path))
        .collect()
}

fn portable_document_keys(documents: &[SourceDocument]) -> Vec<String> {
    documents
        .iter()
        .map(|document| crate::path_serde::portable(document.relative_path()))
        .collect()
}

pub(crate) fn validate_no_portable_collisions(
    files: &[PathBuf],
    directories: &[PathBuf],
) -> Result<(), ConfigError> {
    let mut materialized = BTreeMap::<String, (&Path, bool)>::new();
    for (path, is_file) in files
        .iter()
        .map(|path| (path.as_path(), true))
        .chain(directories.iter().map(|path| (path.as_path(), false)))
    {
        let portable = crate::path_serde::portable(path);
        let normalized = portable.nfc().collect::<String>();
        let key = normalized.as_str().case_fold().nfc().collect::<String>();
        if let Some((previous, previous_is_file)) = materialized.get(&key) {
            if previous != &path || previous_is_file != &is_file {
                return Err(ConfigError::NonPortablePathCollision {
                    first: (*previous).to_path_buf(),
                    second: path.to_path_buf(),
                });
            }
        } else {
            materialized.insert(key, (path, is_file));
        }
    }
    Ok(())
}

fn collect_paths(
    root: &Path,
    directory: &Path,
    paths: &mut Vec<PathBuf>,
    files: &mut Vec<PathBuf>,
    directories: &mut Vec<PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
    entries_seen: &mut usize,
) -> Result<(), ConfigError> {
    let entries = std::fs::read_dir(directory).map_err(|source| ConfigError::Read {
        path: directory.to_path_buf(),
        source,
    })?;
    let mut entries_by_name = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| ConfigError::Read {
            path: directory.to_path_buf(),
            source,
        })?;
        if is_excluded_metadata_name(&entry.file_name()) {
            continue;
        }
        let path = entry.path();
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| ConfigError::NonUtf8Path(path))?;
        check_entry_limit(
            root,
            entries_seen
                .saturating_add(entries_by_name.len())
                .saturating_add(1),
        )?;
        entries_by_name.push((name, entry));
    }
    entries_by_name.sort_by(|left, right| left.0.cmp(&right.0));
    for (_name, entry) in entries_by_name {
        let path = entry.path();
        *entries_seen = entries_seen.saturating_add(1);
        check_entry_limit(root, *entries_seen)?;
        let file_type = entry.file_type().map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?;
        if is_link_like(&path, file_type)? {
            let related_path = portable_relative_path(root, &path)?;
            let mut diagnostic = Diagnostic::warning(
                DiagnosticCode::SymlinkSkipped,
                format!(
                    "filesystem link was not followed: {}",
                    related_path.display()
                ),
            );
            diagnostic.related_path = Some(related_path);
            crate::diagnostic::push_bounded(diagnostics, diagnostic);
        } else if file_type.is_dir() {
            directories.push(portable_relative_path(root, &path)?);
            collect_paths(
                root,
                &path,
                paths,
                files,
                directories,
                diagnostics,
                entries_seen,
            )?;
        } else if file_type.is_file() {
            let relative = portable_relative_path(root, &path)?;
            files.push(relative.clone());
            if crate::DocumentKind::from_path(&path).is_some() {
                paths.push(relative);
            }
        }
    }
    Ok(())
}

fn portable_relative_path(root: &Path, path: &Path) -> Result<PathBuf, ConfigError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| ConfigError::UnsafePath(path.to_path_buf()))?;
    let logical = crate::manifest::portable_path(relative).map(PathBuf::from)?;
    crate::path_serde::validate(&logical)?;
    Ok(logical)
}

fn check_entry_limit(root: &Path, entries: usize) -> Result<(), ConfigError> {
    if entries > MAX_REPOSITORY_ENTRIES {
        return Err(ConfigError::RepositoryEntryLimit {
            root: root.to_path_buf(),
            limit: MAX_REPOSITORY_ENTRIES,
        });
    }
    Ok(())
}

fn check_document_resource_limits<I, P>(documents: I) -> Result<(), ConfigError>
where
    I: IntoIterator<Item = (P, u64)>,
    P: AsRef<Path>,
{
    let mut count = 0_usize;
    let mut total_bytes = 0_u64;
    for (path, size) in documents {
        check_next_document_resource(path.as_ref(), size, &mut count, &mut total_bytes)?;
    }
    Ok(())
}

fn check_next_document_resource(
    path: &Path,
    size: u64,
    count: &mut usize,
    total_bytes: &mut u64,
) -> Result<(), ConfigError> {
    *count = count.saturating_add(1);
    if *count > MAX_REPOSITORY_DOCUMENTS {
        return Err(ConfigError::RepositoryDocumentLimit {
            path: path.to_path_buf(),
            count: *count,
            limit: MAX_REPOSITORY_DOCUMENTS,
        });
    }
    *total_bytes = total_bytes.saturating_add(size);
    if *total_bytes > MAX_REPOSITORY_SOURCE_BYTES {
        return Err(ConfigError::RepositorySourceBytesLimit {
            path: path.to_path_buf(),
            size: *total_bytes,
            limit: MAX_REPOSITORY_SOURCE_BYTES,
        });
    }
    Ok(())
}

fn is_excluded_metadata_name(name: &std::ffi::OsStr) -> bool {
    [".git", ".hg", ".svn", ".cache"]
        .iter()
        .any(|excluded| name == *excluded)
}

#[cfg_attr(
    not(windows),
    expect(
        clippy::unnecessary_wraps,
        reason = "the Windows implementation must propagate reparse-point metadata errors"
    )
)]
fn is_link_like(path: &Path, file_type: std::fs::FileType) -> Result<bool, ConfigError> {
    if file_type.is_symlink() {
        return Ok(true);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        let metadata = std::fs::symlink_metadata(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_document_count_is_bounded_without_allocating_sources() {
        let documents =
            std::iter::repeat_n((Path::new("config.json"), 0), MAX_REPOSITORY_DOCUMENTS + 1);

        assert!(matches!(
            check_document_resource_limits(documents),
            Err(ConfigError::RepositoryDocumentLimit { .. })
        ));
    }

    #[test]
    fn scan_warning_saturation_reserves_content_error_capacity() {
        let content = vec![Diagnostic::error(
            DiagnosticCode::MissingArchitecture,
            "missing architecture",
        )];
        let scan = std::iter::repeat_with(|| {
            Diagnostic::warning(DiagnosticCode::SymlinkSkipped, "skipped link")
        })
        .take(crate::MAX_REPOSITORY_DIAGNOSTICS)
        .collect::<Vec<_>>();

        let diagnostics = merge_diagnostics(content, &scan);

        assert_eq!(diagnostics.len(), crate::MAX_REPOSITORY_DIAGNOSTICS);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::MissingArchitecture)
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::SymlinkSkipped)
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::DiagnosticLimitReached)
        );
    }

    #[test]
    fn aggregate_source_bytes_are_bounded_without_allocating_sources() {
        let documents = [
            (Path::new("config.json"), MAX_REPOSITORY_SOURCE_BYTES),
            (Path::new("nested/config.json"), 1),
        ];

        assert!(matches!(
            check_document_resource_limits(documents),
            Err(ConfigError::RepositorySourceBytesLimit { .. })
        ));
    }
}
