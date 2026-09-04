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
}

impl ProposalInbox {
    pub fn from_env() -> Option<Self> {
        std::env::var_os("LOG_INBOX_PROPOSAL_DIR")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .map(|pending_dir| Self { pending_dir })
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

        Ok(AppliedProposal {
            proposal_id: proposal_id.to_owned(),
            daily_path,
            evidence_event_ids: proposal.frontmatter.evidence_event_ids,
            supersedes_proposal_ids: proposal.frontmatter.supersedes_proposal_ids,
            proposal_removed: false,
            status: "applied",
        })
    }

    pub fn discard(&self, proposal_id: &str) -> Result<(), String> {
        validate_proposal_id(proposal_id)?;
        let proposal_path = self.find_proposal(proposal_id)?;
        fs::remove_file(&proposal_path)
            .map_err(|error| format!("removing consolidated proposal: {error}"))
    }

    pub fn discard_if_present(&self, proposal_id: &str) -> Result<bool, String> {
        validate_proposal_id(proposal_id)?;
        match self.find_proposal(proposal_id) {
            Ok(path) => {
                fs::remove_file(path)
                    .map_err(|error| format!("removing superseded proposal: {error}"))?;
                Ok(true)
            }
            Err(error) if error.contains("no pending proposal matches") => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn list(&self) -> Result<Vec<PendingProposal>, String> {
        let mut paths = fs::read_dir(&self.pending_dir)
            .map_err(|error| format!("reading proposal inbox: {error}"))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("md"))
            .collect::<Vec<_>>();
        paths.sort_by(|left, right| right.file_name().cmp(&left.file_name()));

        paths.into_iter().map(pending_proposal).collect()
    }

    pub fn get(&self, proposal_id: &str) -> Result<PendingProposal, String> {
        validate_proposal_id(proposal_id)?;
        pending_proposal(self.find_proposal(proposal_id)?)
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
    pub evidence_event_ids: Vec<String>,
    pub supersedes_proposal_ids: Vec<String>,
    pub proposal_removed: bool,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct PendingProposal {
    pub filename: String,
    pub proposal_id: String,
    pub created_at: String,
    pub target_note: String,
    pub confidence: String,
    pub provider: String,
    pub evidence_event_ids: Vec<String>,
    pub canonical_links: Vec<String>,
    pub link_candidates: Vec<String>,
    pub supersedes_proposal_ids: Vec<String>,
    pub consolidation_job_id: Option<String>,
    pub markdown: String,
    pub link_context_revision: String,
    pub stale: bool,
}

#[derive(Debug, Deserialize)]
struct ProposalFrontmatter {
    proposal_id: String,
    #[serde(default)]
    created_at: String,
    target_note: String,
    #[serde(default)]
    confidence: String,
    #[serde(default)]
    provider: String,
    #[serde(default)]
    evidence_event_ids: Vec<String>,
    #[serde(default)]
    canonical_links: Vec<String>,
    #[serde(default)]
    link_candidates: Vec<String>,
    #[serde(default)]
    supersedes_proposal_ids: Vec<String>,
    #[serde(default)]
    consolidation_job_id: Option<String>,
    #[serde(default)]
    link_context_revision: String,
}

struct ParsedProposal {
    frontmatter: ProposalFrontmatter,
    markdown: String,
}

fn pending_proposal(path: PathBuf) -> Result<PendingProposal, String> {
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "proposal filename is not valid UTF-8".to_owned())?
        .to_owned();
    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("reading proposal {}: {error}", path.display()))?;
    let parsed = parse_proposal(&contents)?;
    Ok(PendingProposal {
        filename,
        proposal_id: parsed.frontmatter.proposal_id,
        created_at: parsed.frontmatter.created_at,
        target_note: parsed.frontmatter.target_note,
        confidence: parsed.frontmatter.confidence,
        provider: parsed.frontmatter.provider,
        evidence_event_ids: parsed.frontmatter.evidence_event_ids,
        canonical_links: parsed.frontmatter.canonical_links,
        link_candidates: parsed.frontmatter.link_candidates,
        supersedes_proposal_ids: parsed.frontmatter.supersedes_proposal_ids,
        consolidation_job_id: parsed.frontmatter.consolidation_job_id,
        markdown: parsed.markdown,
        link_context_revision: parsed.frontmatter.link_context_revision,
        stale: false,
    })
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
    output.push_str("\n\n## ");
    if proposal.frontmatter.canonical_links.len() == 1 {
        output.push_str(&proposal.frontmatter.canonical_links[0]);
        output.push_str(" activity report\n\n");
    } else {
        output.push_str("Activity report\n\n");
    }
    output.push_str(proposal.markdown.trim());
    if proposal
        .markdown
        .lines()
        .any(|line| line.trim_start().starts_with("Details:"))
    {
        if proposal.frontmatter.canonical_links.len() > 1 {
            output.push_str("\n\nRelated: ");
            output.push_str(&proposal.frontmatter.canonical_links.join(" · "));
        }
    } else {
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
    if proposal.link_candidates.is_empty() {
        output.push_str("link_candidates: []\n");
    } else {
        output.push_str("link_candidates:\n");
        append_yaml_list(&mut output, &proposal.link_candidates);
    }
    if proposal.supersedes_proposal_ids.is_empty() {
        output.push_str("supersedes_proposal_ids: []\n");
    } else {
        output.push_str("supersedes_proposal_ids:\n");
        append_yaml_list(&mut output, &proposal.supersedes_proposal_ids);
    }
    if let Some(job_id) = &proposal.consolidation_job_id {
        output.push_str("consolidation_job_id: ");
        output.push_str(&yaml_string(job_id));
        output.push('\n');
    }
    output.push_str("link_context_revision: ");
    output.push_str(&yaml_string(&proposal.link_context_revision));
    output.push('\n');
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
            target_note: "Configured daily note".to_owned(),
            canonical_links: vec!["[[Log Inbox]]".to_owned()],
            link_candidates: vec!["[[Log Inbox]]".to_owned()],
            markdown: "- Consolidated a bounded log window.".to_owned(),
            evidence_event_ids: vec!["evt_123".to_owned()],
            confidence: "high".to_owned(),
            open_questions: vec!["Confirm the target note.".to_owned()],
            requires_review: true,
            provider: "local".to_owned(),
            supersedes_proposal_ids: vec!["proposal_previous".to_owned()],
            consolidation_job_id: Some("consolidation_test".to_owned()),
            link_context_revision: "catalog-1".to_owned(),
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
        };

        let first = inbox.stage(&proposal()).expect("first proposal stages");
        let second = inbox.stage(&proposal()).expect("second proposal stages");

        assert_ne!(first.path, second.path);
        let listed = inbox.list().expect("proposals list");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].target_note, "Configured daily note");
        assert_eq!(listed[0].evidence_event_ids, ["evt_123"]);
        assert_eq!(listed[0].supersedes_proposal_ids, ["proposal_previous"]);
        assert_eq!(
            listed[0].consolidation_job_id.as_deref(),
            Some("consolidation_test")
        );
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
    fn applies_a_proposal_to_an_empty_daily_note_and_discards_it_after_acknowledgement() {
        let root =
            std::env::temp_dir().join(format!("log-inbox-apply-test-{}", Uuid::new_v4().simple()));
        let inbox = ProposalInbox {
            pending_dir: root.join("pending"),
        };
        let staged = inbox.stage(&proposal()).expect("proposal stages");
        let daily_dir = root.join("daily");
        fs::create_dir_all(&daily_dir).expect("daily directory exists");
        fs::write(daily_dir.join("Configured daily note.md"), "").expect("empty daily note exists");

        let applied = inbox
            .apply(&staged.proposal_id, &daily_dir)
            .expect("proposal applies");
        let daily = fs::read_to_string(&applied.daily_path).expect("daily note is readable");

        assert!(daily.starts_with("# Configured daily note\n\n## [[Log Inbox]] activity report"));
        assert!(daily.contains("[[Log Inbox]]"));
        assert!(daily.contains("evt_123"));
        assert_eq!(applied.supersedes_proposal_ids, ["proposal_previous"]);
        assert!(staged.path.exists());
        inbox
            .discard(&staged.proposal_id)
            .expect("consolidated proposal is discarded");
        assert!(!staged.path.exists());

        fs::remove_dir_all(root).expect("test directory is removable");
    }
}
