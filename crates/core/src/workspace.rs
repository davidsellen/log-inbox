use anyhow::{Context, Result};
use cap_std::fs::Dir;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::ErrorKind,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

const PROTECTED_COMPONENTS: &[&str] = &[".git", ".obsidian", ".trash", ".log-inbox"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownPathMode {
    ExistingFile,
    MayCreate,
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
}

impl PartialEq for InspectedWorkspace {
    fn eq(&self, other: &Self) -> bool {
        self.canonical_root == other.canonical_root && self.root_binding == other.root_binding
    }
}

impl Eq for InspectedWorkspace {}

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
        let workspace = InspectedWorkspace::inspect(&root).unwrap();

        for path in ["InsideLink/note.md", "OutsideLink/note.md"] {
            assert!(
                workspace
                    .resolve_markdown_path(Path::new(path), MarkdownPathMode::ExistingFile)
                    .is_err()
            );
        }
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
