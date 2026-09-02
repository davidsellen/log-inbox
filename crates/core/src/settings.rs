use std::{collections::HashSet, env, path::PathBuf};

#[derive(Debug, Clone)]
pub struct Settings {
    pub data_dir: PathBuf,
    pub api_keys: HashSet<String>,
    pub retention_days: u64,
}

impl Settings {
    pub fn from_env() -> Self {
        let data_dir = env::var("LOG_INBOX_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./data"));

        let api_keys = env::var("LOG_INBOX_API_KEYS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(ToOwned::to_owned)
            .collect();

        let retention_days = env::var("LOG_INBOX_RETENTION_DAYS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(14);

        Self {
            data_dir,
            api_keys,
            retention_days,
        }
    }

    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join("log-inbox.sqlite3")
    }
}
