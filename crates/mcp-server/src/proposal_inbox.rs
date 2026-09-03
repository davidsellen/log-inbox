use crate::llm::SummaryProposal;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ProposalInbox {
    pending_dir: PathBuf,
    processed_dir: PathBuf,
}

impl ProposalInbox {
    pub fn from_env() -> Option<Self> {
        std::env::var_os("LOG_INBOX_PROPOSAL_DIR")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .map(|pending_dir| {
                let processed_dir = std::env::var_os("LOG_INBOX_PROCESSED_DIR")
                    .filter(|path| !path.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        pending_dir
                            .parent()
                            .unwrap_or(Path::new("."))
                            .join("processed")
                    });
                Self {
                    pending_dir,
                    processed_dir,
                }
            })
    }

    pub fn stage(&self, proposal: &SummaryProposal) -> io::Result<StagedProposal> {
        fs::create_dir_all(&self.pending_dir)?;

        let created_at = Utc::now();
        let proposal_id = format!("proposal_{}", Uuid::new_v4().simple());
        let filename = format!(
            "{}-{}.md",
            created_at.format("%Y%m%dT%H%M%S%.3fZ"),
            proposal_id
        );
        let final_path = self.pending_dir.join(filename);
        let temporary_path = temporary_path(&self.pending_dir, &proposal_id);
        let markdown = render_proposal(&proposal_id, created_at, proposal);

        let write_result = write_then_rename(&temporary_path, &final_path, markdown.as_bytes());
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        write_result?;

        Ok(StagedProposal {
            proposal_id,
            path: final_path,
            created_at,
            status: "pending",
        })
    }

    pub fn apply(
        &self,
        proposal_id: &str,
        daily_notes_dir: &Path,
    ) -> Result<AppliedProposal, String> {
        validate_proposal_id(proposal_id)?;
        let proposal_path = self.find_proposal(proposal_id)?;
        let contents = fs::read_to_string(&proposal_path)
            .map_err(|error| format!("reading proposal {}: {error}", proposal_path.display()))?;
        let proposal = parse_proposal(&contents)?;
        if proposal.frontmatter.proposal_id != proposal_id {
            return Err("proposal ID does not match its frontmatter".to_owned());
        }
        validate_note_name(&proposal.frontmatter.target_note)?;

        fs::create_dir_all(daily_notes_dir)
            .map_err(|error| format!("creating daily notes directory: {error}"))?;
        let daily_path = daily_notes_dir.join(format!("{}.md", proposal.frontmatter.target_note));
        let marker = format!("<!-- log-inbox:{} -->", proposal_id);
        let current = match fs::read_to_string(&daily_path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => {
                return Err(format!(
                    "reading daily note {}: {error}",
                    daily_path.display()
                ));
            }
        };

        if !current.contains(&marker) {
            let updated = render_daily_note(&current, &proposal, &marker);
            let temporary_path = daily_notes_dir.join(format!(".{}.tmp", proposal_id));
            let write_result = write_then_rename(&temporary_path, &daily_path, updated.as_bytes());
            if write_result.is_err() {
                let _ = fs::remove_file(&temporary_path);
            }
            write_result.map_err(|error| format!("writing daily note: {error}"))?;
        }

        fs::create_dir_all(&self.processed_dir)
            .map_err(|error| format!("creating processed proposal directory: {error}"))?;
        let file_name = proposal_path
            .file_name()
            .ok_or_else(|| "proposal path has no filename".to_owned())?;
        let processed_path = self.processed_dir.join(file_name);
        archive_file(&proposal_path, &processed_path, proposal_id)
            .map_err(|error| format!("archiving applied proposal: {error}"))?;

        Ok(AppliedProposal {
            proposal_id: proposal_id.to_owned(),
            daily_path,
            processed_path,
            evidence_event_ids: proposal.frontmatter.evidence_event_ids,
            status: "applied",
        })
    }

    fn find_proposal(&self, proposal_id: &str) -> Result<PathBuf, String> {
        let suffix = format!("-{proposal_id}.md");
        let matches = fs::read_dir(&self.pending_dir)
            .map_err(|error| format!("reading proposal inbox: {error}"))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(&suffix))
            })
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [path] => Ok(path.clone()),
            [] => Err(format!("pending proposal {proposal_id} not found")),
            _ => Err(format!("multiple pending proposals match {proposal_id}")),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct StagedProposal {
    pub proposal_id: String,
    pub path: PathBuf,
    pub created_at: DateTime<Utc>,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct AppliedProposal {
    pub proposal_id: String,
    pub daily_path: PathBuf,
    pub processed_path: PathBuf,
    pub evidence_event_ids: Vec<String>,
    pub status: &'static str,
}

#[derive(Debug, Deserialize)]
struct ProposalFrontmatter {
    proposal_id: String,
    target_note: String,
    #[serde(default)]
    evidence_event_ids: Vec<String>,
    #[serde(default)]
    canonical_links: Vec<String>,
}

struct ParsedProposal {
    frontmatter: ProposalFrontmatter,
    markdown: String,
}

fn parse_proposal(contents: &str) -> Result<ParsedProposal, String> {
    let rest = contents
        .strip_prefix("---\n")
        .ok_or_else(|| "proposal is missing YAML frontmatter".to_owned())?;
    let (yaml, body) = rest
        .split_once("\n---\n")
        .ok_or_else(|| "proposal frontmatter is not terminated".to_owned())?;
    let frontmatter = serde_yaml::from_str(yaml)
        .map_err(|error| format!("parsing proposal frontmatter: {error}"))?;
    let markdown = body
        .trim()
        .strip_prefix("# Log summary proposal")
        .unwrap_or(body.trim())
        .trim()
        .to_owned();
    Ok(ParsedProposal {
        frontmatter,
        markdown,
    })
}

fn render_daily_note(current: &str, proposal: &ParsedProposal, marker: &str) -> String {
    let mut output = if current.trim().is_empty() {
        format!("# {}", proposal.frontmatter.target_note)
    } else {
        current.trim_end().to_owned()
    };
    output.push_str("\n\n## Activity report\n\n");
    output.push_str(proposal.markdown.trim());
    output.push_str("\n\nDetails:");
    if !proposal.frontmatter.canonical_links.is_empty() {
        output.push(' ');
        output.push_str(&proposal.frontmatter.canonical_links.join(" · "));
    }
    for event_id in &proposal.frontmatter.evidence_event_ids {
        output.push_str(" · `");
        output.push_str(event_id);
        output.push('`');
    }
    output.push('\n');
    output.push_str(marker);
    output.push('\n');
    output
}

fn validate_proposal_id(proposal_id: &str) -> Result<(), String> {
    if proposal_id.starts_with("proposal_")
        && proposal_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        Ok(())
    } else {
        Err("invalid proposal ID".to_owned())
    }
}

fn validate_note_name(note: &str) -> Result<(), String> {
    if note.trim().is_empty()
        || note.contains('/')
        || note.contains('\\')
        || note.contains("..")
        || note.contains('\0')
    {
        Err("proposal target_note must be a plain Markdown filename".to_owned())
    } else {
        Ok(())
    }
}

fn temporary_path(directory: &Path, proposal_id: &str) -> PathBuf {
    directory.join(format!(".{proposal_id}.tmp"))
}

fn write_then_rename(temporary_path: &Path, final_path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary_path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temporary_path, final_path)
}

fn archive_file(source: &Path, destination: &Path, proposal_id: &str) -> io::Result<()> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(18) => {
            let contents = fs::read(source)?;
            let directory = destination.parent().unwrap_or(Path::new("."));
            let temporary_path = directory.join(format!(".{proposal_id}.tmp"));
            let write_result = write_then_rename(&temporary_path, destination, &contents);
            if write_result.is_err() {
                let _ = fs::remove_file(&temporary_path);
            }
            write_result?;
            fs::remove_file(source)
        }
        Err(error) => Err(error),
    }
}

fn render_proposal(
    proposal_id: &str,
    created_at: DateTime<Utc>,
    proposal: &SummaryProposal,
) -> String {
    let mut output = format!(
        "---\ntype: log-inbox-proposal\nproposal_id: {}\nstatus: pending\ncreated_at: {}\ntarget_note: {}\nconfidence: {}\nprovider: {}\nrequires_review: true\nevidence_event_ids:\n",
        yaml_string(proposal_id),
        yaml_string(&created_at.to_rfc3339()),
        yaml_string(&proposal.target_note),
        yaml_string(&proposal.confidence),
        yaml_string(&proposal.provider),
    );

    append_yaml_list(&mut output, &proposal.evidence_event_ids);
    if proposal.canonical_links.is_empty() {
        output.push_str("canonical_links: []\n");
    } else {
        output.push_str("canonical_links:\n");
        append_yaml_list(&mut output, &proposal.canonical_links);
    }
    output.push_str("---\n\n# Log summary proposal\n\n");
    output.push_str(&proposal.markdown);
    output.push('\n');

    if !proposal.open_questions.is_empty() {
        output.push_str("\n## Open questions\n\n");
        for question in &proposal.open_questions {
            output.push_str("- ");
            output.push_str(question);
            output.push('\n');
        }
    }

    output
}

fn append_yaml_list(output: &mut String, values: &[String]) {
    for value in values {
        output.push_str("  - ");
        output.push_str(&yaml_string(value));
        output.push('\n');
    }
}

fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposal() -> SummaryProposal {
        SummaryProposal {
            target_note: "Daily log Sep 3".to_owned(),
            canonical_links: vec!["[[Log Inbox]]".to_owned()],
            markdown: "- Consolidated a bounded log window.".to_owned(),
            evidence_event_ids: vec!["evt_123".to_owned()],
            confidence: "high".to_owned(),
            open_questions: vec!["Confirm the target note.".to_owned()],
            requires_review: true,
            provider: "local".to_owned(),
        }
    }

    #[test]
    fn concurrent_staging_creates_distinct_complete_files() {
        let directory = std::env::temp_dir().join(format!(
            "log-inbox-proposal-test-{}",
            Uuid::new_v4().simple()
        ));
        let inbox = ProposalInbox {
            pending_dir: directory.clone(),
            processed_dir: directory.join("processed"),
        };

        let first = inbox.stage(&proposal()).expect("first proposal stages");
        let second = inbox.stage(&proposal()).expect("second proposal stages");

        assert_ne!(first.path, second.path);
        let contents = fs::read_to_string(first.path).expect("proposal is readable");
        assert!(contents.contains("status: pending"));
        assert!(contents.contains("evt_123"));
        assert!(contents.contains("Consolidated a bounded log window"));
        assert!(
            fs::read_dir(&directory)
                .expect("inbox is readable")
                .all(|entry| !entry
                    .expect("entry is readable")
                    .file_name()
                    .to_string_lossy()
                    .starts_with('.'))
        );

        fs::remove_dir_all(directory).expect("test directory is removable");
    }

    #[test]
    fn applies_a_proposal_to_an_empty_daily_note_and_archives_it() {
        let root =
            std::env::temp_dir().join(format!("log-inbox-apply-test-{}", Uuid::new_v4().simple()));
        let inbox = ProposalInbox {
            pending_dir: root.join("pending"),
            processed_dir: root.join("processed"),
        };
        let staged = inbox.stage(&proposal()).expect("proposal stages");
        let daily_dir = root.join("daily");
        fs::create_dir_all(&daily_dir).expect("daily directory exists");
        fs::write(daily_dir.join("Daily log Sep 3.md"), "").expect("empty daily note exists");

        let applied = inbox
            .apply(&staged.proposal_id, &daily_dir)
            .expect("proposal applies");
        let daily = fs::read_to_string(&applied.daily_path).expect("daily note is readable");

        assert!(daily.starts_with("# Daily log Sep 3\n\n## Activity report"));
        assert!(daily.contains("[[Log Inbox]]"));
        assert!(daily.contains("evt_123"));
        assert!(!staged.path.exists());
        assert!(applied.processed_path.exists());

        fs::remove_dir_all(root).expect("test directory is removable");
    }
}
