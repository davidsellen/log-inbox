use anyhow::Result;
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const DASHBOARD_SCOPES: &[&str] = &[
    "logs:read",
    "knowledge:read",
    "draft:generate",
    "review:write",
    "settings:write",
    "vault:write",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCredentials {
    pub session_token: String,
    pub csrf_token: String,
}

pub fn hash_owner_secret(secret: &str) -> Result<String> {
    anyhow::ensure!(secret.len() >= 20, "owner secret must be at least 20 bytes");
    let salt = SaltString::encode_b64(Uuid::new_v4().as_bytes())
        .map_err(|error| anyhow::anyhow!("encoding owner-secret salt: {error}"))?;
    Argon2::default()
        .hash_password(secret.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| anyhow::anyhow!("hashing owner secret: {error}"))
}

pub fn verify_owner_secret(secret: &str, encoded_hash: &str) -> bool {
    let Ok(hash) = PasswordHash::new(encoded_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(secret.as_bytes(), &hash)
        .is_ok()
}

pub fn generate_session_credentials() -> SessionCredentials {
    SessionCredentials {
        session_token: format!("session_{}", Uuid::new_v4().simple()),
        csrf_token: format!("csrf_{}", Uuid::new_v4().simple()),
    }
}

pub fn token_digest(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

pub fn normalize_scopes(scopes: &[String]) -> Result<Vec<String>> {
    let mut normalized = scopes
        .iter()
        .map(|scope| scope.trim())
        .filter(|scope| !scope.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    anyhow::ensure!(
        normalized
            .iter()
            .all(|scope| DASHBOARD_SCOPES.contains(&scope.as_str())),
        "session contains an unsupported scope"
    );
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_verifies_owner_secrets() {
        let encoded = hash_owner_secret("a-long-owner-secret-value").expect("secret hashes");
        assert!(verify_owner_secret("a-long-owner-secret-value", &encoded));
        assert!(!verify_owner_secret("a-different-secret-value", &encoded));
        assert!(!encoded.contains("a-long-owner-secret-value"));
        assert!(hash_owner_secret("short").is_err());
    }

    #[test]
    fn generates_independent_opaque_session_credentials() {
        let first = generate_session_credentials();
        let second = generate_session_credentials();
        assert_ne!(first.session_token, second.session_token);
        assert_ne!(first.csrf_token, first.session_token);
        assert_eq!(token_digest(&first.session_token).len(), 64);
        assert!(!token_digest(&first.session_token).contains(&first.session_token));
    }

    #[test]
    fn validates_and_normalizes_scopes() {
        let scopes = normalize_scopes(&[
            "review:write".to_owned(),
            "logs:read".to_owned(),
            "review:write".to_owned(),
        ])
        .expect("scopes validate");
        assert_eq!(scopes, ["logs:read", "review:write"]);
        assert!(normalize_scopes(&["admin:everything".to_owned()]).is_err());
    }
}
