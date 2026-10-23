//! Original-event Knowledge capture. Owner review is separate from producer claims.
use crate::{
    models::{LogQueryResult, StoredLogEvent},
    store::{Store, stored_event_from_row},
};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSubmission {
    pub schema_version: u32,
    pub submission_key: String,
    pub plan_id: String,
    pub plan_revision: String,
    pub product: String,
    pub modules: Vec<String>,
    pub title: String,
    pub summary: String,
    pub claims: Vec<KnowledgeClaim>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub questions: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<KnowledgeArtifact>,
    #[serde(default)]
    pub commits: Vec<String>,
    #[serde(default)]
    pub pull_requests: Vec<String>,
    #[serde(default)]
    pub supersedes: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeClaim {
    pub kind: KnowledgeClaimKind,
    pub text: String,
    #[serde(default)]
    pub verification_scope: Option<String>,
    #[serde(default)]
    pub reported_acceptance: Option<ReportedAcceptance>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeClaimKind {
    Reported,
    Proposed,
    Accepted,
    Implemented,
    Verified,
    Merged,
    Deployed,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReportedAcceptance {
    pub source: String,
    pub actor: String,
    pub at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeArtifact {
    pub path: String,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeDispositionKind {
    Accept,
    Reject,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnowledgeDisposition {
    pub candidate_id: String,
    pub workspace_id: String,
    pub candidate_hash: String,
    pub disposition: KnowledgeDispositionKind,
    pub reason: String,
    pub actor: String,
    pub created_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCandidate {
    pub id: String,
    pub workspace_id: String,
    pub producer_source: String,
    pub source_event_id: String,
    pub source_event_digest: String,
    pub payload_digest: String,
    pub candidate_hash: String,
    pub submission: KnowledgeSubmission,
    pub created_at: DateTime<Utc>,
    pub review: Option<KnowledgeDisposition>,
    pub evidence_available: bool,
    pub superseded: bool,
}
fn text(value: &str, max: usize, label: &str) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control),
        "Knowledge {label} must be nonempty, at most {max} bytes, without control characters"
    );
    Ok(())
}
fn strings(values: &[String], max_count: usize, max_bytes: usize, label: &str) -> Result<()> {
    ensure!(values.len() <= max_count, "too many Knowledge {label}");
    for value in values {
        text(value, max_bytes, label)?;
    }
    Ok(())
}
impl KnowledgeSubmission {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "unsupported Knowledge submission schema_version"
        );
        for (v, label) in [
            (&self.submission_key, "submission_key"),
            (&self.plan_id, "plan_id"),
            (&self.plan_revision, "plan_revision"),
            (&self.product, "product"),
        ] {
            text(v, 200, label)?;
        }
        text(&self.title, 300, "title")?;
        if self
            .claims
            .iter()
            .all(|c| c.kind == KnowledgeClaimKind::Reported)
        {
            ensure!(
                !self.summary.trim().is_empty()
                    && self.summary.len() <= 4000
                    && !self
                        .summary
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')),
                "Knowledge reported summary must be nonempty and at most 4000 bytes"
            );
        } else {
            text(&self.summary, 4000, "summary")?;
        }
        ensure!(!self.modules.is_empty(), "Knowledge modules are required");
        strings(&self.modules, 20, 200, "modules")?;
        ensure!(
            !self.claims.is_empty() && self.claims.len() <= 32,
            "Knowledge requires 1-32 claims"
        );
        for c in &self.claims {
            if c.kind == KnowledgeClaimKind::Reported {
                ensure!(
                    !c.text.trim().is_empty()
                        && c.text.len() <= 2000
                        && !c
                            .text
                            .chars()
                            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')),
                    "Knowledge reported claim must be nonempty and at most 2000 bytes"
                );
            } else {
                text(&c.text, 2000, "claim text")?;
            }
            if let Some(scope) = &c.verification_scope {
                text(scope, 1000, "verification_scope")?;
            }
            if let Some(a) = &c.reported_acceptance {
                ensure!(
                    c.kind == KnowledgeClaimKind::Accepted,
                    "reported_acceptance requires accepted claim"
                );
                text(&a.source, 300, "acceptance source")?;
                text(&a.actor, 200, "acceptance actor")?;
            }
            ensure!(
                c.kind != KnowledgeClaimKind::Verified || c.verification_scope.is_some(),
                "verified claim requires verification_scope"
            );
        }
        for (v, label) in [
            (&self.decisions, "decisions"),
            (&self.constraints, "constraints"),
            (&self.questions, "questions"),
        ] {
            strings(v, 32, 2000, label)?;
        }
        strings(&self.commits, 32, 300, "commits")?;
        strings(&self.pull_requests, 32, 1000, "pull_requests")?;
        ensure!(self.artifacts.len() <= 32, "too many Knowledge artifacts");
        for a in &self.artifacts {
            text(&a.path, 1000, "artifact path")?;
            ensure!(
                a.sha256.len() == 64
                    && a.sha256
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
                "artifact sha256 must be 64 lowercase hex characters"
            );
        }
        if let Some(id) = &self.supersedes {
            text(id, 100, "supersedes")?;
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= 65536,
            "Knowledge submission exceeds 65536 bytes"
        );
        Ok(())
    }
}
fn digest(value: &impl Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}
fn event_digest(e: &StoredLogEvent) -> Result<String> {
    digest(
        &serde_json::json!({"id":e.id,"received_at":e.received_at,"timestamp":e.timestamp,"source":e.source,"level":e.level,"message":e.message,"metadata":e.metadata,"fingerprint":e.fingerprint,"truncated":e.truncated}),
    )
}
fn event(conn: &Connection, id: &str) -> Result<Option<StoredLogEvent>> {
    conn.query_row("SELECT e.id,e.received_at,e.timestamp,e.source,e.level,e.message,e.metadata_json,e.fingerprint,e.truncated,EXISTS(SELECT 1 FROM review_state r WHERE r.event_id=e.id) FROM log_events e WHERE e.id=?1",[id],stored_event_from_row).optional().map_err(Into::into)
}
fn active(conn: &Connection, workspace: &str) -> Result<()> {
    ensure!(
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM workspace_profiles WHERE id=?1 AND status='active')",
            [workspace],
            |r| r.get::<_, bool>(0)
        )?,
        "Knowledge mutation requires active workspace"
    );
    Ok(())
}
fn load(conn: &Connection, workspace: &str, id: &str) -> Result<Option<KnowledgeCandidate>> {
    let raw=conn.query_row("SELECT id,workspace_id,producer_source,source_event_id,source_event_digest,payload_digest,candidate_hash,submission_json,created_at FROM knowledge_candidates WHERE workspace_id=?1 AND id=?2",params![workspace,id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?,r.get::<_,String>(8)?))).optional()?;
    let Some((
        id,
        workspace_id,
        producer_source,
        source_event_id,
        source_event_digest,
        payload_digest,
        candidate_hash,
        json,
        created_at,
    )) = raw
    else {
        return Ok(None);
    };
    let review=conn.query_row("SELECT disposition,reason,actor,created_at FROM knowledge_dispositions WHERE candidate_id=?1 AND workspace_id=?2",params![id,workspace],|r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?))).optional()?.map(|(d,reason,actor,at)|->Result<_>{Ok(KnowledgeDisposition{candidate_id:id.clone(),workspace_id:workspace.to_owned(),candidate_hash:candidate_hash.clone(),disposition:serde_json::from_value(serde_json::Value::String(d))?,reason,actor,created_at:at.parse()?})}).transpose()?;
    let evidence_available = event(conn, &source_event_id)?
        .map(|e| event_digest(&e))
        .transpose()?
        .is_some_and(|d| d == source_event_digest);
    let superseded = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM knowledge_candidates WHERE supersedes=?1)",
        [&id],
        |r| r.get(0),
    )?;
    Ok(Some(KnowledgeCandidate {
        id,
        workspace_id,
        producer_source,
        source_event_id,
        source_event_digest,
        payload_digest,
        candidate_hash,
        submission: serde_json::from_str(&json)?,
        created_at: created_at.parse()?,
        review,
        evidence_available,
        superseded,
    }))
}
/// A deterministic projection of one original report, never an inferred work status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeSuggestion {
    pub event: StoredLogEvent,
    pub suggestion_hash: String,
    pub submission: KnowledgeSubmission,
    pub capture_issue: Option<String>,
    pub captured_candidate_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeSuggestionList {
    pub suggestions: Vec<KnowledgeSuggestion>,
    pub limit: usize,
    pub truncated: bool,
}
fn suggestion_eligible(e: &StoredLogEvent, workspace: &str) -> bool {
    !e.metadata.contains_key("knowledge_submission")
        && e.metadata
            .get("workspace_id")
            .is_none_or(|v| v.as_str() == Some(workspace))
        && matches!(
            e.metadata.get("event_type").and_then(|v| v.as_str()),
            Some("complete" | "completed" | "blocked" | "failed")
        )
        && !e.message.trim().is_empty()
}
fn suggestion(
    conn: &Connection,
    workspace: &str,
    e: StoredLogEvent,
) -> Result<KnowledgeSuggestion> {
    ensure!(
        suggestion_eligible(&e, workspace),
        "Knowledge source event does not belong to workspace or is not eligible"
    );
    let mut claims = vec![KnowledgeClaim {
        kind: KnowledgeClaimKind::Reported,
        text: e.message.clone(),
        verification_scope: None,
        reported_acceptance: None,
    }];
    // Retain complete values, including qualifications and structured checks. No summary model.
    for field in [
        "outcome",
        "decision",
        "decisions",
        "constraints",
        "questions",
        "tests",
        "validation",
        "blocker",
        "follow_up",
        "canonical_note_candidates",
        "commit",
        "commits",
        "pull_request",
        "pull_requests",
        "artifacts",
    ] {
        if let Some(value) = e.metadata.get(field) {
            let value = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            if !value.trim().is_empty() {
                claims.push(KnowledgeClaim {
                    kind: KnowledgeClaimKind::Reported,
                    text: format!("{field}: {value}"),
                    verification_scope: None,
                    reported_acceptance: None,
                });
            }
        }
    }
    let context = ["repo", "work_item", "task_id"]
        .into_iter()
        .filter_map(|key| {
            e.metadata.get(key).filter(|v| !v.is_null()).map(|v| {
                let value = v
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string());
                format!("{key}: {value}")
            })
        })
        .collect::<Vec<_>>()
        .join(" · ");
    let title = if context.is_empty() {
        "Reported activity".into()
    } else {
        context
    };
    let summary = e
        .metadata
        .get("outcome")
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(&e.message)
        .to_owned();
    let submission = KnowledgeSubmission {
        schema_version: 1,
        submission_key: format!("activity-{}", e.id),
        plan_id: format!("activity-{}", e.id),
        plan_revision: "1".into(),
        product: e
            .metadata
            .get("product")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned(),
        modules: e
            .metadata
            .get("modules")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        title,
        summary,
        claims,
        decisions: vec![],
        constraints: vec![],
        questions: vec![],
        artifacts: vec![],
        commits: vec![],
        pull_requests: vec![],
        supersedes: None,
    };
    let suggestion_hash = digest(&serde_json::json!({"workspace_id":workspace,
        "source_event_digest":event_digest(&e)?, "submission":submission}))?;
    // Missing attribution is supplied by the owner. Other validation failures stay visible.
    let mut checked = submission.clone();
    checked.product = "Owner attribution".into();
    checked.modules = vec!["Owner attribution".into()];
    let capture_issue = (e.truncated || checked.validate().is_err()).then(|| "This exact report exceeds candidate limits or contains unsupported characters. Submit a bounded knowledge_submission retaining all relevant qualifications; the original evidence remains available here.".into());
    let captured_candidate_id = conn
        .query_row(
            "SELECT id FROM knowledge_candidates WHERE source_event_id=?1 AND workspace_id=?2",
            params![e.id, workspace],
            |r| r.get(0),
        )
        .optional()?;
    Ok(KnowledgeSuggestion {
        event: e,
        suggestion_hash,
        submission,
        capture_issue,
        captured_candidate_id,
    })
}

fn persist_candidate(
    conn: &Connection,
    workspace: &str,
    e: &StoredLogEvent,
    submission: &KnowledgeSubmission,
    payload_digest: &str,
) -> Result<KnowledgeCandidate> {
    let existing:Option<(String,String)>=conn.query_row("SELECT id,payload_digest FROM knowledge_candidates WHERE workspace_id=?1 AND producer_source=?2 AND submission_key=?3",params![workspace,e.source,submission.submission_key],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    if let Some((id, previous)) = existing {
        ensure!(
            previous == payload_digest,
            "Knowledge submission key conflicts with existing payload"
        );
        let result = load(conn, workspace, &id)?.context("Knowledge candidate missing")?;
        return Ok(result);
    }
    let bound: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM knowledge_candidates WHERE source_event_id=?1)",
        [&e.id],
        |r| r.get(0),
    )?;
    ensure!(
        !bound,
        "Knowledge source event is already bound to a workspace"
    );
    if let Some(previous) = &submission.supersedes {
        let predecessor = load(conn, workspace, previous)?
            .context("Knowledge predecessor does not belong to workspace")?;
        ensure!(
            predecessor.submission.plan_id == submission.plan_id,
            "Knowledge predecessor belongs to different plan"
        );
        ensure!(
            predecessor.producer_source == e.source,
            "Knowledge predecessor belongs to different producer"
        );
        ensure!(
            predecessor.submission.product == submission.product,
            "Knowledge predecessor belongs to different product"
        );
        ensure!(!predecessor.superseded, "Knowledge predecessor is stale");
    }
    if submission.supersedes.is_none() {
        let plan_exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM knowledge_candidates WHERE workspace_id=?1 AND producer_source=?2 AND plan_id=?3)",
                params![workspace,e.source,submission.plan_id], |r|r.get(0))?;
        ensure!(
            !plan_exists,
            "Knowledge plan already exists; correction requires explicit supersedes"
        );
    }
    let id = format!("knowledge_{}", Uuid::new_v4().simple());
    let source_event_digest = event_digest(e)?;
    let candidate_hash = digest(
        &serde_json::json!({"workspace_id":workspace,"producer_source":e.source,"source_event_id":e.id,"source_event_digest":source_event_digest,"payload_digest":payload_digest}),
    )?;
    conn.execute("INSERT INTO knowledge_candidates(id,workspace_id,producer_source,submission_key,plan_id,source_event_id,source_event_digest,payload_digest,candidate_hash,submission_json,supersedes,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![id,workspace,e.source,submission.submission_key,submission.plan_id,e.id,source_event_digest,payload_digest,candidate_hash,serde_json::to_string(&submission)?,submission.supersedes,Utc::now().to_rfc3339()])?;
    let result = load(conn, workspace, &id)?.context("Knowledge candidate missing")?;
    Ok(result)
}

impl Store {
    /// Materialize only original, already-redacted metadata; never Daily prose or overlays.
    pub fn materialize_knowledge_submission(&self, event_id: &str) -> Result<KnowledgeCandidate> {
        text(event_id, 100, "event ID")?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let workspace: String = tx
            .query_row(
                "SELECT id FROM workspace_profiles WHERE status='active'",
                [],
                |r| r.get(0),
            )
            .optional()?
            .context("Knowledge capture requires active workspace")?;
        let e = event(&tx, event_id)?.context("Knowledge source event is missing or expired")?;
        if let Some(reported_workspace) = e.metadata.get("workspace_id") {
            ensure!(
                reported_workspace.as_str() == Some(workspace.as_str()),
                "Knowledge source event does not belong to workspace"
            );
        }
        let payload = e
            .metadata
            .get("knowledge_submission")
            .context("event has no knowledge_submission")?;
        ensure!(
            serde_json::to_vec(payload)?.len() <= 65536,
            "Knowledge submission exceeds 65536 bytes"
        );
        let submission: KnowledgeSubmission =
            serde_json::from_value(payload.clone()).context("invalid knowledge_submission")?;
        submission.validate()?;
        // Hash the redacted JSON, including explicit optional/default field presence.
        let payload_digest = digest(payload)?;
        let result = persist_candidate(&tx, &workspace, &e, &submission, &payload_digest)?;
        tx.commit()?;
        Ok(result)
    }
    /// SQL applies workspace and terminal-event eligibility before the result bound.
    pub fn list_knowledge_suggestions(
        &self,
        workspace: &str,
        limit: usize,
    ) -> Result<KnowledgeSuggestionList> {
        text(workspace, 100, "workspace ID")?;
        ensure!(
            (1..=100).contains(&limit),
            "Knowledge list limit must be 1-100"
        );
        let conn = self.connect()?;
        let mut statement = conn.prepare("SELECT e.id,e.received_at,e.timestamp,e.source,e.level,e.message,e.metadata_json,e.fingerprint,e.truncated,EXISTS(SELECT 1 FROM review_state r WHERE r.event_id=e.id)
            FROM log_events e WHERE CASE WHEN json_valid(e.metadata_json) THEN
            json_type(e.metadata_json,'$.knowledge_submission') IS NULL
            AND json_extract(e.metadata_json,'$.event_type') IN ('complete','completed','blocked','failed')
            AND (json_type(e.metadata_json,'$.workspace_id') IS NULL OR (json_type(e.metadata_json,'$.workspace_id')='text' AND json_extract(e.metadata_json,'$.workspace_id')=?1))
            AND length(trim(e.message)) > 0 ELSE 0 END
            AND NOT EXISTS(SELECT 1 FROM knowledge_candidates k WHERE k.source_event_id=e.id)
            ORDER BY e.received_at DESC,e.id DESC LIMIT ?2")?;
        let events = statement
            .query_map(params![workspace, limit + 1], stored_event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = events.len() > limit;
        let suggestions = events
            .into_iter()
            .take(limit)
            .map(|e| suggestion(&conn, workspace, e))
            .collect::<Result<Vec<_>>>()?;
        Ok(KnowledgeSuggestionList {
            suggestions,
            limit,
            truncated,
        })
    }

    /// Freeze the exact original projection and owner attribution; never mutate source evidence.
    pub fn capture_knowledge_suggestion(
        &self,
        workspace: &str,
        event_id: &str,
        expected_suggestion_hash: &str,
        product: &str,
        modules: &[String],
    ) -> Result<KnowledgeCandidate> {
        text(event_id, 100, "event ID")?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        active(&tx, workspace)?;
        let e = event(&tx, event_id)?.context("Knowledge source event is missing or expired")?;
        let projected = suggestion(&tx, workspace, e.clone())?;
        ensure!(
            projected.suggestion_hash == expected_suggestion_hash,
            "Knowledge suggestion hash changed"
        );
        ensure!(
            projected.capture_issue.is_none(),
            "Knowledge suggestion exceeds capture limits"
        );
        let mut submission = projected.submission;
        submission.product = product.to_owned();
        submission.modules = modules.to_vec();
        submission.validate()?;
        let payload_digest = digest(&submission)?;
        let result = persist_candidate(&tx, workspace, &e, &submission, &payload_digest)?;
        tx.commit()?;
        Ok(result)
    }

    /// Filter original redacted submission events before bounding the result.
    /// Malformed submission payloads remain visible for validation; malformed or
    /// mismatched workspace claims are excluded rather than treated as unscoped.
    pub fn list_knowledge_submission_events(
        &self,
        workspace_id: &str,
        limit: usize,
    ) -> Result<LogQueryResult> {
        text(workspace_id, 100, "workspace ID")?;
        ensure!(
            (1..=100).contains(&limit),
            "Knowledge list limit must be 1-100"
        );
        let conn = self.connect()?;
        let mut statement = conn.prepare(
            "SELECT e.id,e.received_at,e.timestamp,e.source,e.level,e.message,
                    e.metadata_json,e.fingerprint,e.truncated,
                    EXISTS(SELECT 1 FROM review_state r WHERE r.event_id=e.id)
             FROM log_events e
             WHERE CASE WHEN json_valid(e.metadata_json) THEN
                 json_type(e.metadata_json,'$.knowledge_submission') IS NOT NULL
                 AND (json_type(e.metadata_json,'$.workspace_id') IS NULL
                     OR (json_type(e.metadata_json,'$.workspace_id')='text'
                         AND json_extract(e.metadata_json,'$.workspace_id')=?1))
                 ELSE 0 END
             ORDER BY e.received_at DESC,e.id DESC LIMIT ?2",
        )?;
        let events = statement
            .query_map(params![workspace_id, limit + 1], stored_event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = events.len() > limit;
        Ok(LogQueryResult {
            events: events.into_iter().take(limit).collect(),
            truncated,
            limit,
        })
    }

    pub fn knowledge_candidate(
        &self,
        workspace_id: &str,
        id: &str,
    ) -> Result<Option<KnowledgeCandidate>> {
        load(&self.connect()?, workspace_id, id)
    }
    pub fn list_knowledge_candidates(
        &self,
        workspace_id: &str,
        limit: usize,
    ) -> Result<Vec<KnowledgeCandidate>> {
        ensure!(
            (1..=100).contains(&limit),
            "Knowledge list limit must be 1-100"
        );
        let conn = self.connect()?;
        let mut stmt=conn.prepare("SELECT id FROM knowledge_candidates WHERE workspace_id=?1 ORDER BY created_at DESC,id DESC LIMIT ?2")?;
        let ids = stmt
            .query_map(params![workspace_id, limit], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.iter()
            .map(|id| load(&conn, workspace_id, id)?.context("Knowledge candidate missing"))
            .collect()
    }
    /// Recheck exact owner approval and live provenance before promotion.
    /// The caller must recheck at confirmation and coordinate its own write journal.
    pub fn require_promotable_knowledge_candidate(
        &self,
        workspace_id: &str,
        id: &str,
        expected_hash: &str,
    ) -> Result<KnowledgeCandidate> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        active(&tx, workspace_id)?;
        let candidate = load(&tx, workspace_id, id)?
            .context("Knowledge candidate does not belong to workspace")?;
        ensure!(
            candidate.candidate_hash == expected_hash,
            "Knowledge candidate hash changed"
        );
        ensure!(
            candidate.evidence_available,
            "Knowledge evidence is expired or changed"
        );
        ensure!(!candidate.superseded, "Knowledge candidate is superseded");
        let review = candidate
            .review
            .as_ref()
            .context("Knowledge candidate requires owner acceptance")?;
        ensure!(
            review.disposition == KnowledgeDispositionKind::Accept
                && review.candidate_hash == expected_hash,
            "Knowledge candidate requires exact owner acceptance"
        );
        tx.commit()?;
        Ok(candidate)
    }

    /// Caller must enforce dashboard owner authentication, KB review scope and CSRF.
    pub fn review_knowledge_candidate(
        &self,
        workspace_id: &str,
        id: &str,
        expected_candidate_hash: &str,
        disposition: KnowledgeDispositionKind,
        reason: &str,
    ) -> Result<KnowledgeDisposition> {
        text(reason, 2000, "review reason")?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        active(&tx, workspace_id)?;
        let candidate = load(&tx, workspace_id, id)?
            .context("Knowledge candidate does not belong to workspace")?;
        ensure!(
            candidate.candidate_hash == expected_candidate_hash,
            "Knowledge candidate hash changed"
        );
        ensure!(
            candidate.evidence_available,
            "Knowledge evidence is expired or changed"
        );
        ensure!(!candidate.superseded, "Knowledge candidate is superseded");
        if let Some(review) = candidate.review {
            ensure!(
                review.disposition == disposition && review.reason == reason,
                "Knowledge candidate already has immutable owner disposition"
            );
            tx.commit()?;
            return Ok(review);
        }
        let review = KnowledgeDisposition {
            candidate_id: id.to_owned(),
            workspace_id: workspace_id.to_owned(),
            candidate_hash: expected_candidate_hash.to_owned(),
            disposition,
            reason: reason.to_owned(),
            actor: "owner".into(),
            created_at: Utc::now(),
        };
        let d = serde_json::to_value(&review.disposition)?
            .as_str()
            .context("invalid disposition")?
            .to_owned();
        tx.execute("INSERT INTO knowledge_dispositions(candidate_id,workspace_id,candidate_hash,disposition,reason,actor,created_at) VALUES(?1,?2,?3,?4,?5,'owner',?6)",params![id,workspace_id,expected_candidate_hash,d,reason,review.created_at.to_rfc3339()])?;
        tx.commit()?;
        Ok(review)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::LogEventInput;
    use serde_json::{Value, json};
    fn setup() -> (Store, String) {
        let store =
            Store::open(std::env::temp_dir().join(format!("knowledge-{}.sqlite", Uuid::new_v4())))
                .unwrap();
        let workspace = store
            .save_active_workspace_profile(
                "/workspace",
                "UTC",
                "Daily",
                "{date}.md",
                None,
                "wikilink",
                None,
            )
            .unwrap();
        (store, workspace.id)
    }
    fn payload(key: &str) -> Value {
        json!({"schema_version":1,"submission_key":key,"plan_id":"plan-1","plan_revision":"r1","product":"Log Inbox","modules":["knowledge"],"title":"Capture plans","summary":"Plan accepted; implementation remains pending.","claims":[{"kind":"accepted","text":"Owner accepted the capture plan","reported_acceptance":{"source":"owner conversation","actor":"owner","at":"2026-10-19T10:00:00Z"}}]})
    }
    fn insert(store: &Store, payload: Value) -> StoredLogEvent {
        store.insert_event(LogEventInput{source:"codex/host".into(),level:None,timestamp:None,message:"Task complete".into(),metadata:Some(json!({"event_type":"complete","status":"succeeded","knowledge_submission":payload}).as_object().unwrap().clone()),fingerprint:None}).unwrap()
    }
    #[test]
    fn knowledge_submission_listing_filters_before_limit_and_rejects_invalid_workspace_claims() {
        let (store, workspace) = setup();
        let old = insert(&store, payload("old"));
        for _ in 0..501 {
            store
                .insert_event(LogEventInput {
                    source: "codex/ordinary".into(),
                    level: None,
                    timestamp: None,
                    message: "ordinary activity".into(),
                    metadata: None,
                    fingerprint: None,
                })
                .unwrap();
        }
        let listed = store
            .list_knowledge_submission_events(&workspace, 100)
            .unwrap();
        assert_eq!(listed.events.len(), 1);
        assert_eq!(listed.events[0].id, old.id);
        assert!(!listed.truncated);
        for value in [
            json!("other"),
            json!(null),
            json!(42),
            json!([workspace]),
            json!({"id":workspace}),
        ] {
            let e = insert(&store, payload("excluded"));
            store
                .connect()
                .unwrap()
                .execute(
                    "UPDATE log_events SET metadata_json=?1 WHERE id=?2",
                    params![
                        json!({"workspace_id":value,"knowledge_submission":payload("excluded")})
                            .to_string(),
                        e.id
                    ],
                )
                .unwrap();
        }
        let invalid_json = insert(&store, payload("invalid-json"));
        store
            .connect()
            .unwrap()
            .execute(
                "UPDATE log_events SET metadata_json='{' WHERE id=?1",
                [invalid_json.id],
            )
            .unwrap();
        let listed = store
            .list_knowledge_submission_events(&workspace, 100)
            .unwrap();
        assert_eq!(listed.events.len(), 1);
        let scoped = insert(&store, payload("scoped"));
        store
            .connect()
            .unwrap()
            .execute(
                "UPDATE log_events SET metadata_json=?1 WHERE id=?2",
                params![
                    json!({"workspace_id":workspace,"knowledge_submission":payload("scoped")})
                        .to_string(),
                    scoped.id
                ],
            )
            .unwrap();
        let listed = store
            .list_knowledge_submission_events(&workspace, 1)
            .unwrap();
        assert_eq!(listed.events.len(), 1);
        assert_eq!(listed.events[0].id, scoped.id);
        assert!(listed.truncated);
        assert_eq!(listed.limit, 1);
        assert!(
            store
                .list_knowledge_submission_events(&workspace, 0)
                .is_err()
        );
        assert!(
            store
                .list_knowledge_submission_events(&workspace, 101)
                .is_err()
        );
    }

    fn activity(store: &Store, metadata: Value, message: &str) -> StoredLogEvent {
        store
            .insert_event(LogEventInput {
                source: "codex/activity".into(),
                level: None,
                timestamp: None,
                message: message.into(),
                metadata: Some(metadata.as_object().unwrap().clone()),
                fingerprint: None,
            })
            .unwrap()
    }
    #[test]
    fn knowledge_suggestions_freeze_reported_claims_attribution_and_source() {
        let (store, workspace) = setup();
        let e = activity(
            &store,
            json!({"event_type":"complete", "status":"succeeded", "outcome":"Probable cause found; fix unverified.", "validation":["not run"], "decisions":["Defer deployment"], "repo":"not-a-product"}),
            "Investigation completed.\nImplementation remains pending.",
        );
        let listed = store.list_knowledge_suggestions(&workspace, 100).unwrap();
        let suggestion = &listed.suggestions[0];
        assert!(suggestion.submission.product.is_empty());
        assert!(suggestion.submission.modules.is_empty());
        assert!(suggestion.capture_issue.is_none());
        assert_eq!(suggestion.submission.claims[0].text, e.message);
        assert!(
            suggestion
                .submission
                .claims
                .iter()
                .all(|c| c.kind == KnowledgeClaimKind::Reported)
        );
        assert!(
            suggestion
                .submission
                .claims
                .iter()
                .any(|c| c.text.contains("fix unverified"))
        );
        assert!(
            store
                .capture_knowledge_suggestion(
                    &workspace,
                    &e.id,
                    &suggestion.suggestion_hash,
                    "",
                    &[]
                )
                .is_err()
        );
        assert!(
            store
                .capture_knowledge_suggestion(
                    &workspace,
                    &e.id,
                    "stale",
                    "Log Inbox",
                    &["knowledge".into()]
                )
                .is_err()
        );
        let capture = || {
            store.capture_knowledge_suggestion(
                &workspace,
                &e.id,
                &suggestion.suggestion_hash,
                "Log Inbox",
                &["knowledge".into()],
            )
        };
        let c = capture().unwrap();
        assert_eq!(capture().unwrap().id, c.id);
        assert!(c.evidence_available && c.review.is_none());
        assert!(
            store
                .list_knowledge_suggestions(&workspace, 100)
                .unwrap()
                .suggestions
                .is_empty()
        );
        assert!(
            store
                .capture_knowledge_suggestion(
                    &workspace,
                    &e.id,
                    &suggestion.suggestion_hash,
                    "Other",
                    &["knowledge".into()]
                )
                .is_err()
        );
        assert!(
            store
                .require_promotable_knowledge_candidate(&workspace, &c.id, &c.candidate_hash)
                .is_err()
        );
        store
            .connect()
            .unwrap()
            .execute(
                "UPDATE log_events SET message='Changed evidence' WHERE id=?1",
                [&e.id],
            )
            .unwrap();
        assert!(
            !store
                .knowledge_candidate(&workspace, &c.id)
                .unwrap()
                .unwrap()
                .evidence_available
        );
        assert!(capture().is_err());
        store
            .connect()
            .unwrap()
            .execute("DELETE FROM log_events WHERE id=?1", [&e.id])
            .unwrap();
        assert!(capture().is_err());
    }
    #[test]
    fn knowledge_suggestions_filter_before_bound_and_keep_oversized_reports_visible() {
        let (store, workspace) = setup();
        let large = activity(
            &store,
            json!({"event_type":"blocked", "outcome":"Pending external verification"}),
            &"x".repeat(4001),
        );
        for _ in 0..110 {
            activity(
                &store,
                json!({"event_type":"complete", "workspace_id":"other"}),
                "Excluded workspace",
            );
            activity(&store, json!({"event_type":"start"}), "Started work");
        }
        let listed = store.list_knowledge_suggestions(&workspace, 1).unwrap();
        assert_eq!(listed.suggestions[0].event.id, large.id);
        assert!(!listed.truncated);
        assert_eq!(listed.suggestions[0].submission.claims[0].text.len(), 4001);
        assert!(listed.suggestions[0].capture_issue.is_some());
        assert!(
            store
                .capture_knowledge_suggestion(
                    &workspace,
                    &large.id,
                    &listed.suggestions[0].suggestion_hash,
                    "Log Inbox",
                    &["knowledge".into()]
                )
                .is_err()
        );
        let wrong = activity(
            &store,
            json!({"event_type":"complete", "workspace_id":"other"}),
            "Wrong workspace",
        );
        assert!(
            store
                .capture_knowledge_suggestion(
                    &workspace,
                    &wrong.id,
                    "hash",
                    "Log Inbox",
                    &["knowledge".into()]
                )
                .is_err()
        );
        assert!(
            store
                .list_knowledge_candidates(&workspace, 100)
                .unwrap()
                .is_empty()
        );
        assert!(store.list_knowledge_suggestions(&workspace, 0).is_err());
        assert!(store.list_knowledge_suggestions(&workspace, 101).is_err());
        activity(
            &store,
            json!({"event_type":"complete"}),
            "Another eligible report",
        );
        assert!(
            store
                .list_knowledge_suggestions(&workspace, 1)
                .unwrap()
                .truncated
        );
    }

    #[test]
    fn knowledge_capture_retries_conflicts_and_concurrency() {
        let (store, workspace) = setup();
        let e = insert(&store, payload("key"));
        let handles = (0..4)
            .map(|_| {
                let store = store.clone();
                let id = e.id.clone();
                std::thread::spawn(move || store.materialize_knowledge_submission(&id).unwrap().id)
            })
            .collect::<Vec<_>>();
        let ids = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>();
        assert!(ids.iter().all(|id| id == &ids[0]));
        let duplicate = insert(&store, payload("key"));
        let c = store
            .materialize_knowledge_submission(&duplicate.id)
            .unwrap();
        assert_eq!(c.id, ids[0]);
        assert_eq!(c.source_event_id, e.id);
        assert_eq!(c.submission.claims.len(), 1);
        assert_eq!(c.submission.claims[0].kind, KnowledgeClaimKind::Accepted);
        assert!(c.review.is_none());
        assert!(
            store
                .require_promotable_knowledge_candidate(&workspace, &c.id, &c.candidate_hash)
                .is_err()
        );
        let mut conflict = payload("key");
        conflict["summary"] = json!("Different");
        let conflict = insert(&store, conflict);
        assert!(
            store
                .materialize_knowledge_submission(&conflict.id)
                .unwrap_err()
                .to_string()
                .contains("conflicts")
        );
        assert_eq!(
            store
                .list_knowledge_candidates(&workspace, 100)
                .unwrap()
                .len(),
            1
        );
        assert!(store.list_knowledge_candidates(&workspace, 101).is_err());
        assert!(store.knowledge_candidate("other", &c.id).unwrap().is_none());
    }
    #[test]
    fn knowledge_capture_rejects_malformed_without_partial_materialization() {
        let (store, workspace) = setup();
        let mut cases = Vec::new();
        let mut p = payload("bad");
        p["approval"] = json!(true);
        cases.push(p);
        let mut p = payload("bad");
        p["modules"] = json!([]);
        cases.push(p);
        let mut p = payload("bad");
        p["title"] = json!("bad\ncontrol");
        cases.push(p);
        let mut p = payload("bad");
        p["claims"][0]["kind"] = json!("verified");
        cases.push(p);
        let mut p = payload("bad");
        p["claims"][0]["kind"] = json!("complete");
        cases.push(p);
        let mut p = payload("bad");
        p["artifacts"] = json!([{"path":"plan.md","sha256":"bad"}]);
        cases.push(p);
        let mut p = payload("bad");
        p["questions"] = json!(["x".repeat(2001)]);
        cases.push(p);
        let scoped = store
            .insert_event(LogEventInput {
                source: "codex/host".into(),
                level: None,
                timestamp: None,
                message: "wrong workspace".into(),
                metadata: Some(
                    json!({"workspace_id":"other", "knowledge_submission":payload("scoped")})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
                fingerprint: None,
            })
            .unwrap();
        assert!(store.materialize_knowledge_submission(&scoped.id).is_err());
        for p in cases {
            let e = insert(&store, p);
            assert!(store.materialize_knowledge_submission(&e.id).is_err());
        }
        assert!(
            store
                .list_knowledge_candidates(&workspace, 10)
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn knowledge_capture_review_is_exact_immutable_and_supersession_revokes_currency() {
        let (store, workspace) = setup();
        let e = insert(&store, payload("first"));
        let first = store.materialize_knowledge_submission(&e.id).unwrap();
        assert!(
            store
                .review_knowledge_candidate(
                    &workspace,
                    &first.id,
                    "wrong",
                    KnowledgeDispositionKind::Accept,
                    "reviewed"
                )
                .is_err()
        );
        let review = store
            .review_knowledge_candidate(
                &workspace,
                &first.id,
                &first.candidate_hash,
                KnowledgeDispositionKind::Accept,
                "reviewed",
            )
            .unwrap();
        assert!(
            store
                .require_promotable_knowledge_candidate(
                    &workspace,
                    &first.id,
                    &first.candidate_hash
                )
                .is_ok()
        );
        assert_eq!(
            review,
            store
                .review_knowledge_candidate(
                    &workspace,
                    &first.id,
                    &first.candidate_hash,
                    KnowledgeDispositionKind::Accept,
                    "reviewed"
                )
                .unwrap()
        );
        assert!(
            store
                .review_knowledge_candidate(
                    &workspace,
                    &first.id,
                    &first.candidate_hash,
                    KnowledgeDispositionKind::Reject,
                    "changed"
                )
                .is_err()
        );
        let mut p = payload("second");
        p["supersedes"] = json!(first.id);
        p["plan_revision"] = json!("r2");
        let e = insert(&store, p);
        let second = store.materialize_knowledge_submission(&e.id).unwrap();
        assert!(second.review.is_none());
        let old = store
            .knowledge_candidate(&workspace, &first.id)
            .unwrap()
            .unwrap();
        assert!(old.superseded);
        assert!(old.review.is_some());
        assert!(
            store
                .require_promotable_knowledge_candidate(
                    &workspace,
                    &first.id,
                    &first.candidate_hash
                )
                .is_err()
        );
        assert!(
            store
                .review_knowledge_candidate(
                    &workspace,
                    &first.id,
                    &first.candidate_hash,
                    KnowledgeDispositionKind::Accept,
                    "reviewed"
                )
                .is_err()
        );
        store
            .review_knowledge_candidate(
                &workspace,
                &second.id,
                &second.candidate_hash,
                KnowledgeDispositionKind::Reject,
                "needs correction",
            )
            .unwrap();
        assert!(
            store
                .review_knowledge_candidate(
                    &workspace,
                    &second.id,
                    &second.candidate_hash,
                    KnowledgeDispositionKind::Accept,
                    "reviewed"
                )
                .is_err()
        );
        let mut p = payload("third");
        p["supersedes"] = json!(first.id);
        let e = insert(&store, p);
        assert!(store.materialize_knowledge_submission(&e.id).is_err());
        let conn = store.connect().unwrap();
        assert!(
            conn.execute(
                "UPDATE knowledge_candidates SET submission_json='{}' WHERE id=?1",
                [&second.id]
            )
            .is_err()
        );
        assert!(
            conn.execute("DELETE FROM knowledge_dispositions", [])
                .is_err()
        );
    }
    #[test]
    fn knowledge_capture_corrections_require_same_producer_product_and_explicit_predecessor() {
        let (store, workspace) = setup();
        let e = insert(&store, payload("first"));
        let first = store.materialize_knowledge_submission(&e.id).unwrap();
        let fork = insert(&store, payload("fork"));
        assert!(
            store
                .materialize_knowledge_submission(&fork.id)
                .unwrap_err()
                .to_string()
                .contains("explicit supersedes")
        );
        let mut p = payload("wrong-producer");
        p["supersedes"] = json!(first.id);
        let other = insert(&store, p);
        store
            .connect()
            .unwrap()
            .execute(
                "UPDATE log_events SET source='codex/other' WHERE id=?1",
                [&other.id],
            )
            .unwrap();
        assert!(
            store
                .materialize_knowledge_submission(&other.id)
                .unwrap_err()
                .to_string()
                .contains("different producer")
        );
        let mut p = payload("wrong-product");
        p["supersedes"] = json!(first.id);
        p["product"] = json!("Other product");
        let other = insert(&store, p);
        assert!(
            store
                .materialize_knowledge_submission(&other.id)
                .unwrap_err()
                .to_string()
                .contains("different product")
        );
        assert!(
            !store
                .knowledge_candidate(&workspace, &first.id)
                .unwrap()
                .unwrap()
                .superseded
        );
        assert_eq!(
            store
                .list_knowledge_candidates(&workspace, 10)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn knowledge_capture_expiry_preserves_binding_but_blocks_review_and_detects_changes() {
        let (store, workspace) = setup();
        let e = insert(&store, payload("expiry"));
        let c = store.materialize_knowledge_submission(&e.id).unwrap();
        store
            .connect()
            .unwrap()
            .execute(
                "UPDATE log_events SET message='changed' WHERE id=?1",
                [&e.id],
            )
            .unwrap();
        assert!(
            !store
                .knowledge_candidate(&workspace, &c.id)
                .unwrap()
                .unwrap()
                .evidence_available
        );
        assert!(
            store
                .review_knowledge_candidate(
                    &workspace,
                    &c.id,
                    &c.candidate_hash,
                    KnowledgeDispositionKind::Accept,
                    "reviewed"
                )
                .is_err()
        );
        store
            .prune_old_events_at(1, Utc::now() + chrono::Duration::days(2))
            .unwrap();
        assert!(event(&store.connect().unwrap(), &e.id).unwrap().is_none());
        let expired = store
            .knowledge_candidate(&workspace, &c.id)
            .unwrap()
            .unwrap();
        assert!(!expired.evidence_available);
        assert_eq!(expired.source_event_digest, c.source_event_digest);
        assert_eq!(expired.submission, c.submission);
        assert!(
            store
                .review_knowledge_candidate(
                    &workspace,
                    &c.id,
                    &c.candidate_hash,
                    KnowledgeDispositionKind::Reject,
                    "expired"
                )
                .is_err()
        );
    }
    #[test]
    fn knowledge_capture_cross_workspace_and_wrong_plan_fail() {
        let (store, workspace) = setup();
        let e = insert(&store, payload("first"));
        let c = store.materialize_knowledge_submission(&e.id).unwrap();
        let other = store
            .create_pending_workspace_profile(
                "/other",
                "UTC",
                "Daily",
                "{date}.md",
                None,
                "wikilink",
            )
            .unwrap();
        store.activate_workspace_profile(&other.id).unwrap();
        assert!(store.materialize_knowledge_submission(&e.id).is_err());
        assert!(
            store
                .review_knowledge_candidate(
                    &workspace,
                    &c.id,
                    &c.candidate_hash,
                    KnowledgeDispositionKind::Accept,
                    "reviewed"
                )
                .is_err()
        );
        assert!(
            store
                .review_knowledge_candidate(
                    &other.id,
                    &c.id,
                    &c.candidate_hash,
                    KnowledgeDispositionKind::Accept,
                    "reviewed"
                )
                .is_err()
        );
        let mut p = payload("other");
        p["supersedes"] = json!(c.id);
        let event = insert(&store, p);
        assert!(store.materialize_knowledge_submission(&event.id).is_err());
        store.activate_workspace_profile(&workspace).unwrap();
        let mut p = payload("other-plan");
        p["supersedes"] = json!(c.id);
        p["plan_id"] = json!("different");
        let event = insert(&store, p);
        assert!(store.materialize_knowledge_submission(&event.id).is_err());
    }
    #[test]
    fn knowledge_capture_upgrade_from_real_v27_preserves_existing_data_and_reopens() {
        // Migration 28 is strictly additive. Remove its objects to construct the
        // released v27 schema, retain v27 data, then exercise the actual upgrader.
        let (store, workspace) = setup();
        let e = insert(&store, payload("preserve"));
        let conn = store.connect().unwrap();
        conn.execute_batch("DROP TRIGGER knowledge_promotion_immutable; DROP TABLE knowledge_promotion_operations; DROP TRIGGER knowledge_candidates_immutable_update; DROP TRIGGER knowledge_candidates_immutable_delete; DROP TRIGGER knowledge_dispositions_immutable_update; DROP TRIGGER knowledge_dispositions_immutable_delete; DROP TABLE knowledge_dispositions; DROP TABLE knowledge_candidates; DELETE FROM schema_migrations WHERE version>=28;").unwrap();
        drop(conn);
        assert_eq!(store.schema_version().unwrap(), 27);
        let upgraded = Store::open(store.database_path().to_owned()).unwrap();
        assert_eq!(upgraded.schema_version().unwrap(), 29);
        assert_eq!(
            event(&upgraded.connect().unwrap(), &e.id)
                .unwrap()
                .unwrap()
                .message,
            e.message
        );
        assert_eq!(
            upgraded.active_workspace_profile().unwrap().unwrap().id,
            workspace
        );
        let c = upgraded.materialize_knowledge_submission(&e.id).unwrap();
        let reopened = Store::open(upgraded.database_path().to_owned()).unwrap();
        assert_eq!(
            reopened
                .knowledge_candidate(&workspace, &c.id)
                .unwrap()
                .unwrap()
                .candidate_hash,
            c.candidate_hash
        );
        let count: i64 = reopened
            .connect()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version=28",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
