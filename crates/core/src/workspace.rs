use anyhow::{Context, Result};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{ErrorKind, Read},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

const PROTECTED_COMPONENTS: &[&str] = &[".git", ".obsidian", ".trash", ".log-inbox"];
const MAX_SCAN_ENTRIES: usize = 10_000;
const MAX_SCAN_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownPathMode {
    ExistingFile,
    MayCreate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFolder {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMarkdownSource {
    pub path: String,
    pub byte_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMarkdownDocument {
    pub source: WorkspaceMarkdownSource,
    pub content: String,
    pub content_digest: String,
}

#[derive(Debug, Clone)]
pub struct InspectedWorkspace {
    canonical_root: PathBuf,
    root_binding: String,
    directory: Arc<Dir>,
}

impl InspectedWorkspace {
    pub fn inspect(configured_root: &Path) -> Result<Self> {
        let canonical_root = fs::canonicalize(configured_root).with_context(|| {
            format!(
                "workspace root does not exist: {}",
                configured_root.display()
            )
        })?;
        let directory_file = fs::File::open(&canonical_root)
            .with_context(|| format!("opening workspace root: {}", canonical_root.display()))?;
        let metadata = directory_file
            .metadata()
            .with_context(|| format!("reading workspace root: {}", canonical_root.display()))?;
        anyhow::ensure!(metadata.is_dir(), "workspace root is not a directory");
        let root_binding = root_binding(&canonical_root, &metadata);
        Ok(Self {
            canonical_root,
            root_binding,
            directory: Arc::new(Dir::from_std_file(directory_file)),
        })
    }

    pub fn canonical_root(&self) -> &Path {
        &self.canonical_root
    }

    pub fn root_binding(&self) -> &str {
        &self.root_binding
    }

    pub fn directory(&self) -> &Dir {
        &self.directory
    }

    pub fn resolve_markdown_path(
        &self,
        relative_path: &Path,
        mode: MarkdownPathMode,
    ) -> Result<PathBuf> {
        anyhow::ensure!(
            !relative_path.as_os_str().is_empty() && !relative_path.is_absolute(),
            "Markdown path must be a non-empty relative path"
        );
        anyhow::ensure!(
            relative_path.extension().and_then(|value| value.to_str()) == Some("md"),
            "Markdown path must end in .md"
        );

        let components = relative_path.components().collect::<Vec<_>>();
        anyhow::ensure!(
            components
                .iter()
                .all(|component| matches!(component, Component::Normal(_))),
            "Markdown path cannot contain traversal or platform prefixes"
        );
        for component in &components {
            let value = component
                .as_os_str()
                .to_str()
                .context("Markdown path must be valid UTF-8")?;
            anyhow::ensure!(
                !PROTECTED_COMPONENTS
                    .iter()
                    .any(|protected| value.eq_ignore_ascii_case(protected)),
                "Markdown path enters protected workspace metadata: {value}"
            );
        }

        let mut candidate = self.canonical_root.clone();
        let mut missing = false;
        for (index, component) in components.iter().enumerate() {
            candidate.push(component.as_os_str());
            if missing {
                continue;
            }
            match fs::symlink_metadata(&candidate) {
                Ok(metadata) => {
                    anyhow::ensure!(
                        !metadata.file_type().is_symlink(),
                        "Markdown path cannot pass through a symbolic link: {}",
                        candidate.display()
                    );
                    let is_last = index + 1 == components.len();
                    if is_last {
                        anyhow::ensure!(
                            metadata.is_file(),
                            "Markdown target is not a regular file: {}",
                            candidate.display()
                        );
                    } else {
                        anyhow::ensure!(
                            metadata.is_dir(),
                            "Markdown parent is not a directory: {}",
                            candidate.display()
                        );
                    }
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    anyhow::ensure!(
                        mode == MarkdownPathMode::MayCreate,
                        "Markdown target does not exist: {}",
                        candidate.display()
                    );
                    missing = true;
                }
                Err(error) => return Err(error).context("inspecting Markdown path"),
            }
        }

        if !missing {
            let canonical_target = fs::canonicalize(&candidate)?;
            anyhow::ensure!(
                canonical_target.starts_with(&self.canonical_root),
                "Markdown target escapes the workspace"
            );
            Ok(canonical_target)
        } else {
            Ok(candidate)
        }
    }

    pub fn list_markdown_folders(&self, maximum: usize) -> Result<Vec<WorkspaceFolder>> {
        anyhow::ensure!(
            (1..=10_000).contains(&maximum),
            "folder catalog limit must be between 1 and 10000"
        );
        let mut folders = vec![WorkspaceFolder {
            path: ".".to_owned(),
        }];
        let root = self.directory.try_clone()?;
        let mut visited = 0;
        collect_folders(root, Path::new(""), 0, maximum, &mut visited, &mut folders)?;
        folders.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(folders)
    }

    pub fn list_markdown_sources(
        &self,
        roots: &[String],
        exclusions: &[String],
        maximum: usize,
    ) -> Result<Vec<WorkspaceMarkdownSource>> {
        anyhow::ensure!(
            (1..=2_000).contains(&maximum),
            "Markdown source limit must be between 1 and 2000"
        );
        let (roots, exclusions) = normalize_knowledge_collection_paths(roots, exclusions)?;
        let mut scan = MarkdownSourceScan {
            exclusions: &exclusions,
            maximum,
            visited: 0,
            seen: BTreeSet::new(),
            sources: Vec::new(),
        };
        for root in roots {
            if path_is_excluded(&root, &exclusions) {
                continue;
            }
            let directory = self.open_relative_directory(&root)?;
            let prefix = if root == "." {
                PathBuf::new()
            } else {
                PathBuf::from(&root)
            };
            collect_markdown_sources(directory, &prefix, prefix.components().count(), &mut scan)?;
        }
        scan.sources
            .sort_by(|left, right| left.path.cmp(&right.path));
        Ok(scan.sources)
    }

    pub fn read_markdown_source(
        &self,
        relative_path: &Path,
        maximum_bytes: u64,
    ) -> Result<WorkspaceMarkdownDocument> {
        anyhow::ensure!(
            (1..=1024 * 1024).contains(&maximum_bytes),
            "Markdown source byte limit must be between 1 and 1048576"
        );
        validate_relative_workspace_path(relative_path, "Markdown source")?;
        anyhow::ensure!(
            relative_path.extension().and_then(|value| value.to_str()) == Some("md"),
            "Markdown source must end in .md"
        );
        let parent = relative_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let directory = self.open_relative_directory(&parent.to_string_lossy())?;
        let file_name = relative_path
            .file_name()
            .context("Markdown source must name a file")?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let mut file = directory
            .open_with(file_name, &options)
            .with_context(|| format!("opening Markdown source: {}", relative_path.display()))?;
        let metadata = file.metadata()?;
        anyhow::ensure!(
            metadata.is_file(),
            "Markdown source is not a regular file: {}",
            relative_path.display()
        );
        anyhow::ensure!(
            metadata.len() <= maximum_bytes,
            "Markdown source exceeds the configured byte limit: {}",
            relative_path.display()
        );
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.by_ref()
            .take(maximum_bytes + 1)
            .read_to_end(&mut bytes)?;
        anyhow::ensure!(
            bytes.len() as u64 <= maximum_bytes,
            "Markdown source changed beyond the configured byte limit: {}",
            relative_path.display()
        );
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let content = String::from_utf8(bytes).with_context(|| {
            format!("Markdown source is not UTF-8: {}", relative_path.display())
        })?;
        Ok(WorkspaceMarkdownDocument {
            source: WorkspaceMarkdownSource {
                path: relative_path.to_string_lossy().into_owned(),
                byte_len: metadata.len(),
            },
            content,
            content_digest: digest,
        })
    }

    fn open_relative_directory(&self, relative_path: &str) -> Result<Dir> {
        if relative_path == "." {
            return self.directory.try_clone().map_err(Into::into);
        }
        let path = Path::new(relative_path);
        validate_relative_workspace_path(path, "collection root")?;
        let mut directory = self.directory.try_clone()?;
        for component in path.components() {
            let name = component.as_os_str();
            let metadata = directory.symlink_metadata(name)?;
            anyhow::ensure!(
                metadata.is_dir() && !metadata.is_symlink(),
                "collection root is not a safe directory: {relative_path}"
            );
            directory = directory.open_dir_nofollow(name)?;
        }
        Ok(directory)
    }
}

impl PartialEq for InspectedWorkspace {
    fn eq(&self, other: &Self) -> bool {
        self.canonical_root == other.canonical_root && self.root_binding == other.root_binding
    }
}

impl Eq for InspectedWorkspace {}

fn collect_folders(
    directory: Dir,
    prefix: &Path,
    depth: usize,
    maximum: usize,
    visited: &mut usize,
    folders: &mut Vec<WorkspaceFolder>,
) -> Result<()> {
    anyhow::ensure!(
        depth <= MAX_SCAN_DEPTH,
        "workspace folder depth exceeds {MAX_SCAN_DEPTH}"
    );
    let mut entries = directory.entries()?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        *visited += 1;
        anyhow::ensure!(
            *visited <= MAX_SCAN_ENTRIES,
            "workspace catalog exceeds {MAX_SCAN_ENTRIES} entries"
        );
        let name = entry.file_name();
        let Some(name_text) = name.to_str() else {
            continue;
        };
        let file_type = entry.file_type()?;
        if !file_type.is_dir() || file_type.is_symlink() || is_protected_component(name_text) {
            continue;
        }
        anyhow::ensure!(
            folders.len() < maximum,
            "workspace contains more than {maximum} safe folders"
        );
        let path = prefix.join(&name);
        folders.push(WorkspaceFolder {
            path: path.to_string_lossy().into_owned(),
        });
        collect_folders(
            directory.open_dir_nofollow(&name)?,
            &path,
            depth + 1,
            maximum,
            visited,
            folders,
        )?;
    }
    Ok(())
}

struct MarkdownSourceScan<'a> {
    exclusions: &'a [String],
    maximum: usize,
    visited: usize,
    seen: BTreeSet<String>,
    sources: Vec<WorkspaceMarkdownSource>,
}

fn collect_markdown_sources(
    directory: Dir,
    prefix: &Path,
    depth: usize,
    scan: &mut MarkdownSourceScan<'_>,
) -> Result<()> {
    anyhow::ensure!(
        depth <= MAX_SCAN_DEPTH,
        "Knowledge collection depth exceeds {MAX_SCAN_DEPTH}"
    );
    let mut entries = directory.entries()?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        scan.visited += 1;
        anyhow::ensure!(
            scan.visited <= MAX_SCAN_ENTRIES,
            "Knowledge collection scan exceeds {MAX_SCAN_ENTRIES} entries"
        );
        let name = entry.file_name();
        let Some(name_text) = name.to_str() else {
            continue;
        };
        let path = prefix.join(&name);
        let path_text = path.to_string_lossy().into_owned();
        if path_is_excluded(&path_text, scan.exclusions) || is_protected_component(name_text) {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_markdown_sources(directory.open_dir_nofollow(&name)?, &path, depth + 1, scan)?;
        } else if file_type.is_file()
            && Path::new(name_text)
                .extension()
                .and_then(|value| value.to_str())
                == Some("md")
            && scan.seen.insert(path_text.clone())
        {
            anyhow::ensure!(
                scan.sources.len() < scan.maximum,
                "Knowledge collection contains more than {} Markdown files",
                scan.maximum
            );
            scan.sources.push(WorkspaceMarkdownSource {
                path: path_text,
                byte_len: entry.metadata()?.len(),
            });
        }
    }
    Ok(())
}

pub fn normalize_knowledge_collection_paths(
    roots: &[String],
    exclusions: &[String],
) -> Result<(Vec<String>, Vec<String>)> {
    let roots = normalized_relative_paths(roots, 1, 8, "roots")?;
    let exclusions = normalized_relative_paths(exclusions, 0, 32, "exclusions")?;
    ensure_exclusions_within_roots(&roots, &exclusions)?;
    Ok((roots, exclusions))
}

fn normalized_relative_paths(
    paths: &[String],
    minimum: usize,
    maximum: usize,
    label: &str,
) -> Result<Vec<String>> {
    anyhow::ensure!(
        (minimum..=maximum).contains(&paths.len()),
        "collection {label} must contain {minimum}-{maximum} paths"
    );
    let mut normalized = Vec::with_capacity(paths.len());
    for value in paths {
        let value = value.trim();
        if value == "." {
            normalized.push(value.to_owned());
            continue;
        }
        let path = Path::new(value);
        validate_relative_workspace_path(path, label)?;
        normalized.push(
            path.components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
        );
    }
    normalized.sort();
    normalized.dedup();
    anyhow::ensure!(
        normalized.len() == paths.len(),
        "collection {label} must be unique"
    );
    Ok(normalized)
}

fn validate_relative_workspace_path(path: &Path, label: &str) -> Result<()> {
    anyhow::ensure!(
        !path.as_os_str().is_empty()
            && !path.is_absolute()
            && path.as_os_str().len() <= 1024
            && !path.to_string_lossy().contains(['\\', '\0']),
        "{label} must be a non-empty relative path"
    );
    let components = path.components().collect::<Vec<_>>();
    anyhow::ensure!(
        components
            .iter()
            .all(|component| matches!(component, Component::Normal(_))),
        "{label} cannot contain traversal or platform prefixes"
    );
    for component in components {
        let value = component
            .as_os_str()
            .to_str()
            .context("workspace paths must be valid UTF-8")?;
        anyhow::ensure!(
            !is_protected_component(value),
            "{label} enters protected workspace metadata: {value}"
        );
    }
    Ok(())
}

fn is_protected_component(value: &str) -> bool {
    PROTECTED_COMPONENTS
        .iter()
        .any(|protected| value.eq_ignore_ascii_case(protected))
}

fn path_is_excluded(path: &str, exclusions: &[String]) -> bool {
    exclusions.iter().any(|excluded| {
        excluded == "."
            || path == excluded
            || path
                .strip_prefix(excluded)
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

fn ensure_exclusions_within_roots(roots: &[String], exclusions: &[String]) -> Result<()> {
    anyhow::ensure!(
        exclusions
            .iter()
            .all(|excluded| roots.iter().any(|root| path_contains(root, excluded))),
        "each collection exclusion must be inside an included root"
    );
    Ok(())
}

fn path_contains(root: &str, candidate: &str) -> bool {
    root == "."
        || root == candidate
        || candidate
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn root_binding(canonical_root: &Path, metadata: &fs::Metadata) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"log-inbox-workspace-root-v1\0");
    hasher.update(canonical_root.to_string_lossy().as_bytes());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        hasher.update(metadata.dev().to_le_bytes());
        hasher.update(metadata.ino().to_le_bytes());
    }
    #[cfg(not(unix))]
    {
        hasher.update(metadata.len().to_le_bytes());
        if let Ok(created) = metadata.created()
            && let Ok(duration) = created.duration_since(std::time::UNIX_EPOCH)
        {
            hasher.update(duration.as_nanos().to_le_bytes());
        }
    }
    format!("workspace-root-v1:{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn temp_root(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("log-inbox-{label}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("temporary workspace creates");
        path
    }

    #[test]
    fn inspects_existing_directories_and_detects_replacement() {
        let root = temp_root("workspace-binding");
        let original = InspectedWorkspace::inspect(&root).expect("workspace inspects");
        assert_eq!(original.canonical_root(), fs::canonicalize(&root).unwrap());

        let previous = root.with_extension("previous");
        fs::rename(&root, &previous).expect("old root remains allocated");
        fs::create_dir(&root).expect("replacement root creates");
        let replacement = InspectedWorkspace::inspect(&root).expect("replacement inspects");
        assert_ne!(original.root_binding(), replacement.root_binding());

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(previous).unwrap();
    }

    #[test]
    fn rejects_missing_roots_and_regular_files() {
        let root = temp_root("workspace-invalid");
        let file = root.join("note.md");
        fs::write(&file, "note").unwrap();
        assert!(InspectedWorkspace::inspect(&file).is_err());
        assert!(InspectedWorkspace::inspect(&root.join("missing")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolves_existing_and_reviewed_missing_markdown_paths() {
        let root = temp_root("workspace-paths");
        fs::create_dir(root.join("Daily")).unwrap();
        fs::write(root.join("Daily/Today.md"), "today").unwrap();
        let workspace = InspectedWorkspace::inspect(&root).unwrap();

        assert_eq!(
            workspace
                .resolve_markdown_path(Path::new("Daily/Today.md"), MarkdownPathMode::ExistingFile)
                .unwrap(),
            fs::canonicalize(root.join("Daily/Today.md")).unwrap()
        );
        assert_eq!(
            workspace
                .resolve_markdown_path(
                    Path::new("Daily/2026/Tomorrow.md"),
                    MarkdownPathMode::MayCreate
                )
                .unwrap(),
            fs::canonicalize(&root)
                .unwrap()
                .join("Daily/2026/Tomorrow.md")
        );
        assert!(
            workspace
                .resolve_markdown_path(
                    Path::new("Daily/Missing.md"),
                    MarkdownPathMode::ExistingFile
                )
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn catalogs_only_safe_folders_and_reads_bounded_collection_sources() {
        let root = temp_root("workspace-knowledge");
        for folder in [
            "Products/Alpha/Decisions",
            "Products/Alpha/Archive",
            "Products/Alphabet",
            ".hidden",
            ".obsidian/Private",
        ] {
            fs::create_dir_all(root.join(folder)).unwrap();
        }
        fs::write(root.join("Products/Alpha/Overview.md"), "# Alpha\n").unwrap();
        fs::write(
            root.join("Products/Alpha/Decisions/Choice.md"),
            "Keep it small.",
        )
        .unwrap();
        fs::write(root.join("Products/Alpha/Archive/Old.md"), "old").unwrap();
        fs::write(root.join("Products/Alphabet/Other.md"), "other").unwrap();
        fs::write(root.join("Products/Alpha/ignore.txt"), "not Markdown").unwrap();
        fs::write(root.join(".hidden/Included.md"), "hidden but allowed").unwrap();
        fs::write(root.join(".obsidian/Private/Secret.md"), "protected").unwrap();
        let workspace = InspectedWorkspace::inspect(&root).unwrap();

        let folders = workspace
            .list_markdown_folders(20)
            .unwrap()
            .into_iter()
            .map(|folder| folder.path)
            .collect::<Vec<_>>();
        assert!(folders.contains(&".".to_owned()));
        assert!(folders.contains(&".hidden".to_owned()));
        assert!(folders.contains(&"Products/Alpha/Decisions".to_owned()));
        assert!(!folders.iter().any(|path| path.contains(".obsidian")));
        assert!(workspace.list_markdown_folders(1).is_err());

        let sources = workspace
            .list_markdown_sources(
                &[
                    "Products/Alpha".to_owned(),
                    "Products//Alpha/Decisions".to_owned(),
                ],
                &["Products/Alpha/Archive".to_owned()],
                10,
            )
            .unwrap();
        assert_eq!(
            sources
                .iter()
                .map(|source| source.path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "Products/Alpha/Decisions/Choice.md",
                "Products/Alpha/Overview.md"
            ]
        );
        assert!(
            workspace
                .list_markdown_sources(
                    &["Products/Alpha".to_owned()],
                    &["Products/Alphabet".to_owned()],
                    10,
                )
                .is_err()
        );
        assert!(
            workspace
                .list_markdown_sources(&[".obsidian".to_owned()], &[], 10)
                .is_err()
        );

        let document = workspace
            .read_markdown_source(Path::new("Products/Alpha/Overview.md"), 100)
            .unwrap();
        assert_eq!(document.content, "# Alpha\n");
        assert_eq!(document.content_digest.len(), 64);
        assert!(
            workspace
                .read_markdown_source(Path::new("Products/Alpha/Overview.md"), 4)
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_traversal_non_markdown_and_protected_paths() {
        let root = temp_root("workspace-rejections");
        let workspace = InspectedWorkspace::inspect(&root).unwrap();
        for path in [
            "../escape.md",
            "/tmp/escape.md",
            "Daily/note.txt",
            ".obsidian/state.md",
            "Docs/.git/config.md",
        ] {
            assert!(
                workspace
                    .resolve_markdown_path(Path::new(path), MarkdownPathMode::MayCreate)
                    .is_err(),
                "{path} must be rejected"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_even_when_their_target_is_inside_the_workspace() {
        use std::os::unix::fs::symlink;

        let root = temp_root("workspace-symlink");
        let outside = temp_root("workspace-outside");
        fs::create_dir(root.join("Real")).unwrap();
        fs::write(root.join("Real/note.md"), "inside").unwrap();
        fs::write(outside.join("note.md"), "outside").unwrap();
        symlink(root.join("Real"), root.join("InsideLink")).unwrap();
        symlink(&outside, root.join("OutsideLink")).unwrap();
        symlink(root.join("Real/note.md"), root.join("LinkedNote.md")).unwrap();
        let workspace = InspectedWorkspace::inspect(&root).unwrap();

        for path in ["InsideLink/note.md", "OutsideLink/note.md"] {
            assert!(
                workspace
                    .resolve_markdown_path(Path::new(path), MarkdownPathMode::ExistingFile)
                    .is_err()
            );
        }
        assert!(
            workspace
                .read_markdown_source(Path::new("LinkedNote.md"), 1024)
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
