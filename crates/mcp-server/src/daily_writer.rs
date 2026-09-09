use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read, Write},
    path::{Component, Path},
};

const MAX_MARKDOWN_FILE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ManagedBlockPlan {
    pub previous_block: Option<String>,
    pub next_block: String,
    #[serde(skip_serializing)]
    pub updated_content: Vec<u8>,
    pub expected_old_block_hash: Option<String>,
    pub intended_new_block_hash: String,
}

pub fn plan_managed_block(
    current: Option<&[u8]>,
    block_id: &str,
    markdown: &str,
    new_note_title: &str,
) -> Result<ManagedBlockPlan, String> {
    if block_id.is_empty()
        || !block_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("managed block ID is invalid".to_owned());
    }
    let current = current.unwrap_or_default();
    if current.len() > MAX_MARKDOWN_FILE_BYTES {
        return Err(format!(
            "Markdown note exceeds the {MAX_MARKDOWN_FILE_BYTES}-byte Apply limit"
        ));
    }
    let text =
        std::str::from_utf8(current).map_err(|_| "Markdown note must be valid UTF-8".to_owned())?;
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let begin = format!("<!-- log-inbox:daily:{block_id}:begin -->");
    let end = format!("<!-- log-inbox:daily:{block_id}:end -->");
    let markers = marker_ranges(text, &begin, &end)?;
    let body = normalize_line_endings(markdown.trim(), eol);
    let next_block = format!("{begin}{eol}{body}{eol}{end}");
    let intended_new_block_hash = digest(next_block.as_bytes());

    let (previous_block, expected_old_block_hash, updated) = if let Some((start, finish)) = markers
    {
        let previous = text[start..finish].to_owned();
        let trailing_eol = if text[start..finish].ends_with('\n') {
            eol
        } else {
            ""
        };
        let mut updated = String::with_capacity(text.len() - (finish - start) + next_block.len());
        updated.push_str(&text[..start]);
        updated.push_str(&next_block);
        updated.push_str(trailing_eol);
        updated.push_str(&text[finish..]);
        (
            Some(previous.clone()),
            Some(digest(previous.trim_end_matches(['\r', '\n']).as_bytes())),
            updated,
        )
    } else {
        let mut updated = text.to_owned();
        if updated.is_empty() {
            if !new_note_title.trim().is_empty() {
                updated.push_str("# ");
                updated.push_str(new_note_title.trim());
                updated.push_str(eol);
                updated.push_str(eol);
            }
        } else if !updated.ends_with(&format!("{eol}{eol}")) {
            if updated.ends_with(eol) {
                updated.push_str(eol);
            } else {
                updated.push_str(eol);
                updated.push_str(eol);
            }
        }
        updated.push_str(&next_block);
        updated.push_str(eol);
        (None, None, updated)
    };

    Ok(ManagedBlockPlan {
        previous_block,
        next_block,
        updated_content: updated.into_bytes(),
        expected_old_block_hash,
        intended_new_block_hash,
    })
}

fn marker_ranges(text: &str, begin: &str, end: &str) -> Result<Option<(usize, usize)>, String> {
    let mut begin_ranges = Vec::new();
    let mut end_ranges = Vec::new();
    let mut fence: Option<char> = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        let trimmed = content.trim();
        let fence_char = trimmed
            .strip_prefix("```")
            .map(|_| '`')
            .or_else(|| trimmed.strip_prefix("~~~").map(|_| '~'));
        if let Some(character) = fence_char {
            match fence {
                None => fence = Some(character),
                Some(open) if open == character => fence = None,
                Some(_) => {}
            }
        } else if fence.is_none() {
            if trimmed == begin {
                begin_ranges.push((offset, offset + line.len()));
            } else if trimmed == end {
                end_ranges.push((offset, offset + line.len()));
            }
        }
        offset += line.len();
    }
    if fence.is_some() {
        return Err("unterminated fenced code prevents safe Daily marker placement".to_owned());
    }
    match (begin_ranges.as_slice(), end_ranges.as_slice()) {
        ([], []) => Ok(None),
        ([(begin_start, _)], [(_, end_finish)]) if begin_start < end_finish => {
            Ok(Some((*begin_start, *end_finish)))
        }
        _ => Err("managed Daily markers are missing, duplicated, or out of order".to_owned()),
    }
}

fn normalize_line_endings(value: &str, eol: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', eol)
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn managed_block_hash(current: &[u8], block_id: &str) -> Result<Option<String>, String> {
    let text =
        std::str::from_utf8(current).map_err(|_| "Markdown note must be valid UTF-8".to_owned())?;
    let begin = format!("<!-- log-inbox:daily:{block_id}:begin -->");
    let end = format!("<!-- log-inbox:daily:{block_id}:end -->");
    marker_ranges(text, &begin, &end).map(|range| {
        range.map(|(start, finish)| {
            digest(
                text[start..finish]
                    .trim_end_matches(['\r', '\n'])
                    .as_bytes(),
            )
        })
    })
}

pub fn read_file(workspace: &Dir, target: &Path) -> io::Result<Option<Vec<u8>>> {
    let (parent, file_name) = match open_parent(workspace, target, false) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = match parent.open_with(file_name, &options) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut content = Vec::new();
    Read::by_ref(&mut file)
        .take((MAX_MARKDOWN_FILE_BYTES + 1) as u64)
        .read_to_end(&mut content)?;
    if content.len() > MAX_MARKDOWN_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Markdown note exceeds the {MAX_MARKDOWN_FILE_BYTES}-byte Apply limit"),
        ));
    }
    Ok(Some(content))
}

pub fn write_atomically(
    workspace: &Dir,
    target: &Path,
    contents: &[u8],
    operation_id: &str,
) -> io::Result<()> {
    let (parent, file_name) = open_parent(workspace, target, true)?;
    let file_name = file_name
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid target filename"))?;
    let temporary = format!(".{file_name}.log-inbox-{operation_id}.tmp");
    let permissions = parent
        .symlink_metadata(file_name)
        .ok()
        .map(|metadata| metadata.permissions());
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    let mut file = parent.open_with(&temporary, &options)?;
    let result = (|| {
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        parent.rename(&temporary, &parent, file_name)?;
        parent.open(".")?.sync_all()
    })();
    if result.is_err() {
        let _ = parent.remove_file(&temporary);
    }
    result
}

fn open_parent<'a>(workspace: &Dir, target: &'a Path, create: bool) -> io::Result<(Dir, &'a Path)> {
    let file_name = target
        .file_name()
        .map(Path::new)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no filename"))?;
    let mut directory = workspace.try_clone()?;
    if let Some(parent) = target.parent() {
        for component in parent.components() {
            let Component::Normal(name) = component else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "target must be a relative normalized path",
                ));
            };
            match directory.open_dir_nofollow(name) {
                Ok(next) => directory = next,
                Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                    match directory.create_dir(name) {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(error) => return Err(error),
                    }
                    directory = directory.open_dir_nofollow(name)?;
                }
                Err(error) => return Err(error),
            }
        }
    }
    Ok((directory, file_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cap_std::ambient_authority;
    use std::fs;

    #[test]
    fn creates_a_minimal_note_without_touching_the_reviewed_markdown() {
        let plan = plan_managed_block(None, "day_test", "### My notes\n\n- Done.", "Daily log")
            .expect("plan builds");
        let result = String::from_utf8(plan.updated_content).unwrap();
        assert!(result.starts_with("# Daily log\n\n<!-- log-inbox:daily:day_test:begin -->"));
        assert!(result.contains("### My notes\n\n- Done."));
        assert!(plan.previous_block.is_none());
    }

    #[test]
    fn replaces_only_the_owned_block_and_preserves_frontmatter_bom_and_crlf() {
        let current = "\u{feff}---\r\ntags: [work-log]\r\n---\r\n\r\nUser text.\r\n\r\n<!-- log-inbox:daily:day_test:begin -->\r\nOld.\r\n<!-- log-inbox:daily:day_test:end -->\r\n\r\nTail.\r\n";
        let plan = plan_managed_block(
            Some(current.as_bytes()),
            "day_test",
            "### Automated activity\n\n- New.",
            "ignored",
        )
        .expect("plan builds");
        let result = String::from_utf8(plan.updated_content).unwrap();
        assert!(result.starts_with("\u{feff}---\r\ntags: [work-log]\r\n---"));
        assert!(result.contains("User text.\r\n\r\n<!-- log-inbox"));
        assert!(result.contains("### Automated activity\r\n\r\n- New."));
        assert!(result.ends_with("\r\n\r\nTail.\r\n"));
        assert!(!result.contains("Old."));
        assert!(plan.expected_old_block_hash.is_some());
    }

    #[test]
    fn ignores_marker_examples_inside_fenced_code() {
        let current = "# Note\n\n```md\n<!-- log-inbox:daily:day_test:begin -->\nexample\n<!-- log-inbox:daily:day_test:end -->\n```\n";
        let plan = plan_managed_block(Some(current.as_bytes()), "day_test", "Real.", "ignored")
            .expect("fenced examples are user content");
        let result = String::from_utf8(plan.updated_content).unwrap();
        assert!(result.contains("example"));
        assert_eq!(result.matches("log-inbox:daily:day_test:begin").count(), 2);
    }

    #[test]
    fn rejects_duplicate_missing_and_reversed_markers() {
        let begin = "<!-- log-inbox:daily:day_test:begin -->";
        let end = "<!-- log-inbox:daily:day_test:end -->";
        for current in [
            format!("{begin}\nfirst\n{begin}\nsecond\n{end}\n"),
            format!("{begin}\nmissing end\n"),
            format!("{end}\nwrong order\n{begin}\n"),
        ] {
            assert!(
                plan_managed_block(Some(current.as_bytes()), "day_test", "new", "ignored").is_err()
            );
        }
    }

    #[test]
    fn rejects_an_unterminated_fence_instead_of_appending_inside_it() {
        let current = "# Note\n\n```md\nexample\n";
        let error = plan_managed_block(Some(current.as_bytes()), "day_test", "new", "ignored")
            .expect_err("an open fence makes safe placement ambiguous");
        assert!(error.contains("unterminated fenced code"));
    }

    #[test]
    fn rejects_invalid_input_without_rewriting_it() {
        assert!(plan_managed_block(Some(&[0xff]), "day", "new", "title").is_err());
        assert!(plan_managed_block(None, "bad:id", "new", "title").is_err());
        let oversized = vec![b'x'; MAX_MARKDOWN_FILE_BYTES + 1];
        assert!(plan_managed_block(Some(&oversized), "day", "new", "title").is_err());
    }

    #[test]
    fn reports_the_exact_owned_block_hash() {
        let plan = plan_managed_block(None, "day_test", "Done.", "Daily").unwrap();
        assert_eq!(
            managed_block_hash(&plan.updated_content, "day_test").unwrap(),
            Some(plan.intended_new_block_hash)
        );
    }

    #[test]
    fn writes_via_same_directory_replacement_and_preserves_permissions() {
        let root =
            std::env::temp_dir().join(format!("log-inbox-daily-writer-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let workspace = Dir::open_ambient_dir(&root, ambient_authority()).unwrap();
        let target = Path::new("nested/Daily.md");
        write_atomically(&workspace, target, b"first", "apply_test").unwrap();
        assert_eq!(read_file(&workspace, target).unwrap().unwrap(), b"first");
        let permissions = fs::metadata(root.join(target)).unwrap().permissions();
        write_atomically(&workspace, target, b"second", "apply_test_2").unwrap();
        assert_eq!(read_file(&workspace, target).unwrap().unwrap(), b"second");
        assert_eq!(
            fs::metadata(root.join(target)).unwrap().permissions(),
            permissions
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_parents_and_targets_at_the_capability_boundary() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "log-inbox-daily-capability-{}",
            uuid::Uuid::new_v4()
        ));
        let outside = root.with_extension("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        symlink(&outside, root.join("linked")).unwrap();
        symlink(outside.join("note.md"), root.join("target.md")).unwrap();
        let workspace = Dir::open_ambient_dir(&root, ambient_authority()).unwrap();

        assert!(
            write_atomically(
                &workspace,
                Path::new("linked/Daily.md"),
                b"no",
                "apply_test"
            )
            .is_err()
        );
        assert!(read_file(&workspace, Path::new("target.md")).is_err());
        write_atomically(
            &workspace,
            Path::new("target.md"),
            b"safe replacement",
            "apply_replace_symlink",
        )
        .unwrap();
        assert_eq!(
            read_file(&workspace, Path::new("target.md"))
                .unwrap()
                .unwrap(),
            b"safe replacement"
        );
        assert!(
            !fs::symlink_metadata(root.join("target.md"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(!outside.join("Daily.md").exists());
        assert!(!outside.join("note.md").exists());

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
