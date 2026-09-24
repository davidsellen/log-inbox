pub mod auth;
mod context_store;
pub use context_store::validate_context_snapshot_payload;
pub mod daily;
mod daily_store;
mod generation_store;
pub mod history;
pub mod models;
pub mod redaction;
pub mod settings;
pub mod store;
pub mod workspace;

/// Classify only SQLite lock contention, including errors wrapped with context.
pub fn sqlite_busy_code(error: &anyhow::Error) -> Option<i32> {
    match error.downcast_ref::<rusqlite::Error>()? {
        rusqlite::Error::SqliteFailure(code, _)
            if matches!(
                code.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            ) =>
        {
            Some(code.extended_code)
        }
        _ => None,
    }
}
