//! Frozen owner-reviewed KB writes. This journal never changes Daily review state.
use crate::store::Store;
use anyhow::{Context, Result, ensure};
use chrono::Utc;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgePromotion {
    pub operation_id: String,
    pub workspace_id: String,
    pub candidate_id: String,
    pub candidate_hash: String,
    pub approval_digest: String,
    pub destination_path: String,
    pub configuration_digest: String,
    pub block_id: String,
    pub original_hash: String,
    pub updated_hash: String,
    pub block_hash: String,
    pub previous_block: Option<String>,
    pub next_block: String,
    pub preview_hash: String,
    pub created_at: String,
    pub state: String,
    pub failure_reason: Option<String>,
    #[serde(skip)]
    pub original_content: Vec<u8>,
    #[serde(skip)]
    pub updated_content: Vec<u8>,
}

pub fn content_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl Store {
    pub fn ensure_no_pending_knowledge_write(&self, workspace: &str, target: &str) -> Result<()> {
        let pending:bool=self.connect()?.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_promotion_operations WHERE workspace_id=?1 AND json_extract(record_json,'$.destination_path')=?2 AND state='writing')",params![workspace,target],|r|r.get(0))?;
        ensure!(
            !pending,
            "A Knowledge write on this note needs recovery first"
        );
        Ok(())
    }
    pub fn ensure_no_pending_daily_write(&self, workspace: &str, target: &str) -> Result<()> {
        let pending:bool=self.connect()?.query_row("SELECT EXISTS(SELECT 1 FROM apply_operations WHERE workspace_id=?1 AND destination_path=?2 AND state IN ('prepared','writing','written','failed','reconciliation_required'))",params![workspace,target],|r|r.get(0))?;
        ensure!(!pending, "A Daily write on this note needs recovery first");
        Ok(())
    }
    pub fn prepare_knowledge_promotion(
        &self,
        mut op: KnowledgePromotion,
    ) -> Result<KnowledgePromotion> {
        let candidate = self.require_promotable_knowledge_candidate(
            &op.workspace_id,
            &op.candidate_id,
            &op.candidate_hash,
        )?;
        ensure!(
            content_digest(&serde_json::to_vec(&candidate.review)?) == op.approval_digest,
            "Knowledge approval changed"
        );
        ensure!(
            op.original_content.len() <= 4 * 1024 * 1024
                && op.updated_content.len() <= 4 * 1024 * 1024,
            "Knowledge promotion exceeds file limits"
        );
        ensure!(
            content_digest(&op.original_content) == op.original_hash
                && content_digest(&op.updated_content) == op.updated_hash,
            "Knowledge promotion bytes do not match hashes"
        );
        // Receipt times and mutable operation state do not participate in approval identity.
        op.operation_id.clear();
        op.preview_hash.clear();
        op.created_at.clear();
        op.state = "prepared".into();
        op.failure_reason = None;
        op.preview_hash = content_digest(&serde_json::to_vec(&op)?);
        op.operation_id = format!("kb_{}", op.preview_hash);
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM knowledge_promotion_operations WHERE id=?1",
                [&op.operation_id],
                |r| r.get(0),
            )
            .optional()?;
        if existing.is_none() {
            let moved: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_promotion_operations WHERE workspace_id=?1 AND json_extract(record_json,'$.block_id')=?2 AND json_extract(record_json,'$.destination_path')!=?3 AND state IN ('writing','finalized'))",params![op.workspace_id,op.block_id,op.destination_path],|r|r.get(0))?;
            ensure!(
                !moved,
                "This plan already targets another canonical note; reconcile that note before moving it"
            );
            let count: i64 = tx.query_row("SELECT COUNT(*) FROM knowledge_promotion_operations WHERE workspace_id=?1 AND state NOT IN ('finalized','cancelled')", [&op.workspace_id], |r| r.get(0))?;
            ensure!(
                count < 100,
                "too many unfinished Knowledge previews; finish or discard an existing preview"
            );
            op.created_at = Utc::now().to_rfc3339();
            tx.execute("INSERT INTO knowledge_promotion_operations(id,workspace_id,candidate_id,record_json,original_content,updated_content,state) VALUES(?1,?2,?3,?4,?5,?6,'prepared')",
                params![op.operation_id,op.workspace_id,op.candidate_id,serde_json::to_string(&op)?,op.original_content,op.updated_content])?;
        } else {
            // A new explicit preview can reuse an identical safely cancelled operation.
            tx.execute("UPDATE knowledge_promotion_operations SET state='prepared',failure_reason=NULL WHERE id=?1 AND state='cancelled'",[&op.operation_id])?;
        }
        tx.commit()?;
        self.knowledge_promotion(&op.workspace_id, &op.operation_id)?
            .context("Knowledge preview disappeared")
    }

    pub fn knowledge_promotion(
        &self,
        workspace: &str,
        id: &str,
    ) -> Result<Option<KnowledgePromotion>> {
        let conn = self.connect()?;
        let row = conn.query_row("SELECT record_json,original_content,updated_content,state,failure_reason FROM knowledge_promotion_operations WHERE workspace_id=?1 AND id=?2",
            params![workspace,id], |r| Ok((r.get::<_,String>(0)?, r.get::<_,Vec<u8>>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<String>>(4)?))).optional()?;
        row.map(|(json, original, updated, state, failure)| {
            let mut op: KnowledgePromotion = serde_json::from_str(&json)?;
            op.original_content = original;
            op.updated_content = updated;
            op.state = state;
            op.failure_reason = failure;
            Ok(op)
        })
        .transpose()
    }

    pub fn list_knowledge_promotions(&self, workspace: &str) -> Result<Vec<KnowledgePromotion>> {
        let conn = self.connect()?;
        let mut stmt=conn.prepare("SELECT json_set(record_json,'$.previous_block',NULL,'$.next_block',''),state,failure_reason FROM knowledge_promotion_operations WHERE workspace_id=?1 ORDER BY CASE WHEN state IN ('prepared','writing','conflict') THEN 0 ELSE 1 END,rowid DESC LIMIT 100")?;
        let rows = stmt
            .query_map([workspace], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(record, state, failure)| {
                let mut op: KnowledgePromotion = serde_json::from_str(&record)?;
                op.state = state;
                op.failure_reason = failure;
                Ok(op)
            })
            .collect()
    }

    pub fn set_knowledge_promotion_state(
        &self,
        op: &KnowledgePromotion,
        state: &str,
        failure: Option<&str>,
    ) -> Result<()> {
        ensure!(
            matches!(state, "writing" | "finalized" | "conflict" | "cancelled"),
            "invalid Knowledge promotion state"
        );
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (record,current):(String,String)=tx.query_row("SELECT record_json,state FROM knowledge_promotion_operations WHERE workspace_id=?1 AND id=?2",params![op.workspace_id,op.operation_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
        let stored: KnowledgePromotion = serde_json::from_str(&record)?;
        ensure!(
            current == op.state && stored.preview_hash == op.preview_hash,
            "Knowledge operation changed; reload it"
        );
        if state == "writing" {
            let moved:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_promotion_operations WHERE workspace_id=?1 AND id!=?2 AND json_extract(record_json,'$.block_id')=?3 AND json_extract(record_json,'$.destination_path')!=?4 AND state IN ('writing','finalized'))",params![op.workspace_id,op.operation_id,stored.block_id,stored.destination_path],|r|r.get(0))?;
            ensure!(
                !moved,
                "This plan already targets another canonical note; reconcile that note before moving it"
            );
            let pending:i64=tx.query_row("SELECT COUNT(*) FROM knowledge_promotion_operations WHERE workspace_id=?1 AND id!=?2 AND state='writing' AND json_extract(record_json,'$.destination_path')=?3",params![op.workspace_id,op.operation_id,stored.destination_path],|r|r.get(0))?;
            ensure!(
                pending == 0,
                "another Knowledge write needs recovery on this note"
            );
        }
        let valid = match (op.state.as_str(), state) {
            ("prepared", "writing" | "cancelled")
            | ("writing", "writing" | "finalized" | "conflict" | "cancelled")
            | ("conflict", "cancelled") => true,
            (a, b) => a == b,
        };
        ensure!(valid, "invalid Knowledge write transition");
        let changed=tx.execute("UPDATE knowledge_promotion_operations SET state=?1,failure_reason=?2 WHERE workspace_id=?3 AND id=?4 AND state=?5",params![state,failure,op.workspace_id,op.operation_id,op.state])?;
        ensure!(changed == 1, "Knowledge operation changed; reload it");
        tx.commit()?;
        Ok(())
    }
}
