use crate::llm::SummaryProposal;
use chrono::{DateTime, Utc};
use serde::Serialize;
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
}

#[derive(Debug, Serialize)]
pub struct StagedProposal {
    pub proposal_id: String,
    pub path: PathBuf,
    pub created_at: DateTime<Utc>,
    pub status: &'static str,
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
}
