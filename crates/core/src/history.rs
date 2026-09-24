//! Read-only recall over retained data. No index extends the retention of content.
use crate::{models::DailyRevisionContent, store::Store};
use anyhow::{Result, ensure};
use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashSet},
};

type Key = (Reverse<NaiveDate>, String, String, String);

#[derive(Debug, Serialize, Deserialize)]
pub struct HistoryMatch {
    pub local_date: NaiveDate,
    pub kind: String,
    pub id: String,
    pub target: String,
    pub excerpt: String,
}

impl HistoryMatch {
    fn key(&self) -> Key {
        (
            Reverse(self.local_date),
            self.kind.clone(),
            self.id.clone(),
            self.target.clone(),
        )
    }
}

#[derive(Debug, Serialize)]
pub struct HistoryPage {
    pub matches: Vec<HistoryMatch>,
    pub next_cursor: Option<String>,
}

// Unicode lowercasing, including expanding characters, while preserving original
// character boundaries in the returned excerpt.
fn excerpt(text: &str, query: &str) -> Option<String> {
    let folded = text.to_lowercase();
    let start = folded.find(query)?;
    let mut offset = 0;
    let mut first = 0;
    for (i, c) in text.chars().enumerate() {
        if offset > start {
            break;
        }
        first = i;
        offset += c.to_lowercase().map(char::len_utf8).sum::<usize>();
    }
    let from = first.saturating_sub(60);
    let length = query.chars().count() + 160;
    let body: String = text.chars().skip(from).take(length).collect();
    Some(format!(
        "{}{}{}",
        if from > 0 { "…" } else { "" },
        body,
        if text.chars().count() > from + length {
            "…"
        } else {
            ""
        }
    ))
}

impl Store {
    pub fn recent_days_preference(&self, workspace: &str) -> Result<usize> {
        Ok(self
            .connect()?
            .query_row(
                "SELECT recent_days FROM dashboard_preferences WHERE workspace_id = ?1",
                [workspace],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(10))
    }

    pub fn save_recent_days_preference(&self, workspace: &str, days: usize) -> Result<()> {
        ensure!(
            [7, 10, 14, 30].contains(&days),
            "recent_days must be 7, 10, 14, or 30"
        );
        self.connect()?.execute(
            "INSERT INTO dashboard_preferences(workspace_id, recent_days) VALUES (?1, ?2)
             ON CONFLICT(workspace_id) DO UPDATE SET recent_days = excluded.recent_days",
            params![workspace, days],
        )?;
        Ok(())
    }

    pub fn search_history(
        &self,
        workspace: &str,
        timezone: &str,
        query: &str,
        cursor: Option<&str>,
    ) -> Result<HistoryPage> {
        let query = query.trim();
        ensure!(
            !query.is_empty() && query.chars().count() <= 200,
            "Search must contain 1–200 characters"
        );
        ensure!(
            cursor.is_none_or(|value| value.len() <= 2048),
            "Invalid search cursor"
        );
        let after: Option<Key> = cursor.map(serde_json::from_str).transpose()?;
        let needle = query.to_lowercase();
        let timezone: chrono_tz::Tz = timezone.parse()?;
        let conn = self.connect()?;
        // Bound retained matches, not the searchable date range. Reading live tables
        // avoids an index with separate deletion/expiry semantics.
        let mut found = BTreeMap::<Key, HistoryMatch>::new();
        let mut consider = |date: NaiveDate, kind: &str, id: &str, target: &str, text: &str| {
            let hit = HistoryMatch {
                local_date: date,
                kind: kind.into(),
                id: id.into(),
                target: target.into(),
                excerpt: String::new(),
            };
            let key = hit.key();
            if after.as_ref().is_some_and(|after| &key <= after)
                || (found.len() >= 201
                    && found.last_key_value().is_some_and(|(last, _)| &key >= last))
            {
                return;
            }
            if let Some(excerpt) = excerpt(text, &needle) {
                found.insert(key, HistoryMatch { excerpt, ..hit });
                if found.len() > 201 {
                    found.pop_last();
                }
            }
        };
        let mut statement = conn.prepare(
            "SELECT local_date, id, text FROM manual_daily_entries WHERE workspace_id = ?1 AND deleted_at IS NULL"
        )?;
        let mut rows = statement.query([workspace])?;
        while let Some(row) = rows.next()? {
            consider(
                row.get::<_, String>(0)?.parse()?,
                "note",
                &row.get::<_, String>(1)?,
                "",
                &row.get::<_, String>(2)?,
            );
        }
        drop(rows);
        drop(statement);
        // Load frozen intervals once, rather than a correlated scan for every log.
        let mut statement = conn.prepare("SELECT local_date, start_utc, end_utc FROM daily_days WHERE workspace_id = ?1 ORDER BY local_date DESC")?;
        let windows = statement
            .query_map([workspace], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .map(|row| {
                let (date, start, end) = row?;
                Ok((
                    date.parse::<NaiveDate>()?,
                    start.parse::<DateTime<Utc>>()?,
                    end.parse::<DateTime<Utc>>()?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        drop(statement);
        let mut statement =
            conn.prepare("SELECT timestamp, id, message, source FROM log_events")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let text = format!("{}\n{}", row.get::<_, String>(2)?, row.get::<_, String>(3)?);
            if !text.to_lowercase().contains(&needle) {
                continue;
            }
            let timestamp: DateTime<Utc> = row.get::<_, String>(0)?.parse()?;
            let date = if let Some((date, _, _)) = windows
                .iter()
                .find(|(_, start, end)| timestamp >= *start && timestamp < *end)
            {
                *date
            } else {
                let date = timestamp.with_timezone(&timezone).date_naive();
                // A frozen date cannot be reopened using today's timezone rules.
                if windows.iter().any(|(frozen, _, _)| *frozen == date) {
                    continue;
                }
                date
            };
            consider(date, "activity", &row.get::<_, String>(1)?, "", &text);
        }
        drop(rows);
        drop(statement);
        let mut statement = conn.prepare(
            "SELECT d.local_date, r.id, r.content_json, r.snapshot_id FROM daily_days d
             JOIN proposal_revisions r ON r.id = d.current_revision_id WHERE d.workspace_id = ?1",
        )?;
        let mut rows = statement.query([workspace])?;
        while let Some(row) = rows.next()? {
            let date: NaiveDate = row.get::<_, String>(0)?.parse()?;
            let id: String = row.get(1)?;
            let content: DailyRevisionContent = serde_json::from_str(&row.get::<_, String>(2)?)?;
            let snapshot: Option<String> = row.get(3)?;
            let mut excluded = conn.prepare("SELECT event_id FROM evidence_snapshot_events WHERE snapshot_id = ?1 AND disposition IN ('omit', 'duplicate_of', 'superseded_by')")?;
            let excluded = excluded
                .query_map([snapshot], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<HashSet<_>>>()?;
            for group in content.workstreams {
                let mut text = vec![group.title];
                text.extend(group.canonical_links);
                let mut visible_facts = 0;
                for facts in [
                    group.outcome,
                    group.decision,
                    group.trade_off,
                    group.validation,
                    group.blocker,
                    group.follow_up,
                    group.activity,
                ] {
                    for fact in facts.into_iter().filter(|fact| {
                        fact.evidence_event_ids
                            .iter()
                            .any(|id| !excluded.contains(id))
                    }) {
                        text.push(fact.text);
                        visible_facts += 1;
                    }
                }
                if visible_facts > 0 {
                    consider(date, "draft", &id, &group.id, &text.join("\n"));
                }
            }
            consider(date, "draft", &id, "", &content.open_questions.join("\n"));
        }
        let mut matches = Vec::new();
        let mut days = 0;
        let mut previous = None;
        let mut more = false;
        for hit in found.into_values() {
            if previous != Some(hit.local_date) {
                days += 1;
            }
            if days > 20 || matches.len() == 200 {
                more = true;
                break;
            }
            previous = Some(hit.local_date);
            matches.push(hit);
        }
        let next_cursor = if more {
            matches
                .last()
                .map(|hit| serde_json::to_string(&hit.key()))
                .transpose()?
        } else {
            None
        };
        Ok(HistoryPage {
            matches,
            next_cursor,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::LogEventInput;
    use serde_json::json;

    fn fixture() -> (Store, String, NaiveDate) {
        let store = Store::open(
            std::env::temp_dir().join(format!("history-{}.sqlite3", uuid::Uuid::new_v4())),
        )
        .unwrap();
        let profile = store
            .create_pending_workspace_profile(
                "history-test",
                "UTC",
                "Daily",
                "{date}.md",
                None,
                "markdown",
            )
            .unwrap();
        store.activate_workspace_profile(&profile.id).unwrap();
        let date = "2025-01-01".parse().unwrap();
        store
            .ensure_daily_day(date, "Daily/2025-01-01.md", None)
            .unwrap();
        (store, profile.id, date)
    }

    fn event(store: &Store, text: &str) -> String {
        store
            .insert_event(LogEventInput {
                source: "codex/test".into(),
                level: None,
                timestamp: Some("2025-01-01T23:30:00Z".parse().unwrap()),
                message: text.into(),
                metadata: Some(
                    serde_json::from_value(json!({"private_search_marker":"not searchable"}))
                        .unwrap(),
                ),
                fingerprint: None,
            })
            .unwrap()
            .id
    }

    #[test]
    fn shared_preferences_default_validate_and_survive_reopen() {
        let (store, workspace, _) = fixture();
        assert_eq!(store.recent_days_preference(&workspace).unwrap(), 10);
        assert!(store.save_recent_days_preference(&workspace, 9).is_err());
        store.save_recent_days_preference(&workspace, 30).unwrap();
        let reopened = Store::open(store.database_path().to_owned()).unwrap();
        assert_eq!(reopened.recent_days_preference(&workspace).unwrap(), 30);
        assert_eq!(
            reopened
                .recent_days_preference("another workspace")
                .unwrap(),
            10
        );
    }

    #[test]
    fn history_searches_old_content_and_respects_deletion_metadata_and_frozen_dates() {
        let (store, workspace, date) = fixture();
        let note = store
            .create_manual_daily_entry(&workspace, date, "PR 9417 Återställ 100%_safe", &[])
            .unwrap();
        event(&store, "PR 9417 finished");
        let page = store
            .search_history(&workspace, "Europe/Stockholm", "pr 9417", None)
            .unwrap();
        assert_eq!(page.matches.len(), 2);
        assert!(page.matches.iter().all(|hit| hit.local_date == date));
        assert_eq!(
            store
                .search_history(&workspace, "UTC", "återställ", None)
                .unwrap()
                .matches
                .len(),
            1
        );
        assert!(
            store
                .search_history(&workspace, "UTC", "private_search_marker", None)
                .unwrap()
                .matches
                .is_empty()
        );
        assert!(store.search_history(&workspace, "UTC", "", None).is_err());
        store
            .delete_manual_daily_entry(&workspace, date, &note.id)
            .unwrap();
        assert_eq!(
            store
                .search_history(&workspace, "UTC", "PR 9417", None)
                .unwrap()
                .matches
                .len(),
            1
        );
        store
            .connect()
            .unwrap()
            .execute("DELETE FROM log_events", [])
            .unwrap();
        assert!(
            store
                .search_history(&workspace, "UTC", "PR 9417", None)
                .unwrap()
                .matches
                .is_empty()
        );
    }

    #[test]
    fn history_only_searches_current_readable_draft_fields() {
        let (store, workspace, date) = fixture();
        let event_id = event(&store, "source event");
        let snapshot = store
            .create_evidence_snapshot(&workspace, date, std::slice::from_ref(&event_id))
            .unwrap();
        for text in ["Old draft marker", "Current draft marker"] {
            let content = json!({"schema_version":1,"manual_entry_ids":[],"open_questions":["Who owns the rollout?"],"workstreams":[{
                "id":"internal_identity", "title":"Draft subject", "evidence_event_ids":[event_id], "canonical_links":[],
                "outcome":[{"text":text,"evidence_event_ids":[event_id]}], "decision":[], "trade_off":[], "validation":[], "blocker":[], "follow_up":[]
            }]});
            store
                .create_proposal_revision(
                    &workspace,
                    date,
                    Some(&snapshot.id),
                    "generated",
                    &content,
                )
                .unwrap();
        }
        for absent in [
            "Old draft marker",
            "internal_identity",
            "evidence_event_ids",
        ] {
            assert!(
                store
                    .search_history(&workspace, "UTC", absent, None)
                    .unwrap()
                    .matches
                    .is_empty()
            );
        }
        assert_eq!(
            store
                .search_history(&workspace, "UTC", "Current draft", None)
                .unwrap()
                .matches[0]
                .kind,
            "draft"
        );
        assert_eq!(
            store
                .search_history(&workspace, "UTC", "Who owns", None)
                .unwrap()
                .matches[0]
                .target,
            ""
        );
        store
            .connect()
            .unwrap()
            .execute(
                "UPDATE evidence_snapshot_events SET disposition = 'omit' WHERE snapshot_id = ?1",
                [&snapshot.id],
            )
            .unwrap();
        assert!(
            store
                .search_history(&workspace, "UTC", "Current draft", None)
                .unwrap()
                .matches
                .is_empty()
        );
    }

    #[test]
    fn history_paginates_large_days_without_losing_matches() {
        let (store, workspace, _) = fixture();
        let conn = store.connect().unwrap();
        conn.execute_batch("WITH RECURSIVE numbers(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM numbers WHERE n < 205)
            INSERT INTO log_events(id, received_at, timestamp, source, level, message, metadata_json)
            SELECT printf('event_%04d',n), '2025-01-01T23:30:00+00:00', '2025-01-01T23:30:00+00:00', 'test', 'info', 'same-day match', '{}' FROM numbers;").unwrap();
        let first = store
            .search_history(&workspace, "UTC", "match", None)
            .unwrap();
        assert_eq!(first.matches.len(), 200);
        let second = store
            .search_history(&workspace, "UTC", "match", first.next_cursor.as_deref())
            .unwrap();
        assert_eq!(second.matches.len(), 5);
        assert!(second.next_cursor.is_none());
        let ids: HashSet<_> = first
            .matches
            .into_iter()
            .chain(second.matches)
            .map(|hit| hit.id)
            .collect();
        assert_eq!(ids.len(), 205);
    }

    #[test]
    fn history_pages_twenty_days_independently_of_the_rail() {
        let (store, workspace, date) = fixture();
        for offset in 0..25 {
            let day = date + chrono::Duration::days(offset);
            store
                .ensure_daily_day(day, &format!("Daily/{day}.md"), None)
                .unwrap();
            store
                .create_manual_daily_entry(&workspace, day, "Retained history", &[])
                .unwrap();
        }
        store.save_recent_days_preference(&workspace, 7).unwrap();
        let page = store
            .search_history(&workspace, "UTC", "Retained", None)
            .unwrap();
        assert_eq!(page.matches.len(), 20);
        let next = store
            .search_history(&workspace, "UTC", "Retained", page.next_cursor.as_deref())
            .unwrap();
        assert_eq!(next.matches.len(), 5);
    }

    #[test]
    #[ignore = "Opt-in synthetic search timing with concurrent ingestion; not a model-quality test"]
    fn history_search_timing_with_concurrent_writer() {
        let (store, workspace, date) = fixture();
        for offset in 0..180 {
            let day = date + chrono::Duration::days(offset);
            store
                .ensure_daily_day(day, &format!("Daily/{day}.md"), None)
                .unwrap();
        }
        store.connect().unwrap().execute_batch("WITH RECURSIVE numbers(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM numbers WHERE n < 50000)
            INSERT INTO log_events(id, received_at, timestamp, source, level, message, metadata_json)
            SELECT printf('event_%06d',n), '2025-01-01T23:30:00+00:00', '2025-01-01T23:30:00+00:00', 'test', 'info', printf('PR %d validated deployment and regression coverage', n), '{}' FROM numbers;").unwrap();
        let writer = store.clone();
        let concurrent = std::thread::spawn(move || {
            for _ in 0..30 {
                event(&writer, "Concurrent intake");
            }
        });
        for query in ["PR 9417", "not present", "PR"] {
            let start = std::time::Instant::now();
            let result = store
                .search_history(&workspace, "UTC", query, None)
                .unwrap();
            eprintln!(
                "{query:?}: {}ms, {} matches, 50,000 events / 180 frozen days",
                start.elapsed().as_millis(),
                result.matches.len()
            );
            assert!(
                start.elapsed().as_secs_f64() < 1.0,
                "Search exceeded one-second target"
            );
        }
        concurrent.join().unwrap();
    }
    #[test]
    fn literal_unicode_excerpts() {
        assert!(excerpt("PR 9417: ÅTERSTÄLL 100%_safe", "återställ").is_some());
        assert!(excerpt("no wildcard", "%").is_none());
        assert!(
            excerpt("a İdentifier", "i\u{307}dentifier")
                .unwrap()
                .contains("İdentifier")
        );
    }
}
