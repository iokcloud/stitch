//! API Token 存库哈希：本地 SHA-256 · 生产 HMAC-SHA256(`APP_KEY`)。

use sha2::{Digest, Sha256};

use crate::AppConfig;

#[derive(Clone, Debug)]
pub struct TokenHasher {
    app_key: Option<String>,
}

impl TokenHasher {
    pub fn from_config(config: &AppConfig) -> Self {
        Self::from_app_key(config.app_key.clone())
    }

    pub fn from_app_key(app_key: Option<String>) -> Self {
        let app_key = app_key.filter(|k| !k.trim().is_empty());
        Self { app_key }
    }

    /// 新 Token 入库哈希（有 `APP_KEY` 时用 HMAC）。
    pub fn hash_for_store(&self, token: &str) -> String {
        match &self.app_key {
            Some(key) => hmac_sha256_hex(key, token),
            None => legacy_sha256_hex(token),
        }
    }

    /// 鉴权时按序尝试（HMAC 优先，兼容旧 SHA-256 行）。
    pub fn candidate_hashes(&self, token: &str) -> Vec<String> {
        let legacy = legacy_sha256_hex(token);
        match &self.app_key {
            Some(key) => {
                let hmac = hmac_sha256_hex(key, token);
                if hmac == legacy {
                    vec![legacy]
                } else {
                    vec![hmac, legacy]
                }
            }
            None => vec![legacy],
        }
    }

    pub fn uses_hmac(&self) -> bool {
        self.app_key.is_some()
    }

    pub fn legacy_hash(token: &str) -> String {
        legacy_sha256_hex(token)
    }
}

pub fn legacy_sha256_hex(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

fn hmac_sha256_hex(key: &str, token: &str) -> String {
    use hmac::{Hmac, Mac};
    type HmacSha256 = Hmac<Sha256>;
    let key_bytes = if key.is_empty() {
        b"promptstdio-fallback"
    } else {
        key.as_bytes()
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(key_bytes) else {
        // 仅空密钥会触发此分支，已在上方处理；此处为防御性兜底
        return crate::token_hash::legacy_sha256_hex(token);
    };
    mac.update(token.as_bytes());
    format!("{:x}", mac.finalize().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_differs_from_legacy_when_key_set() {
        let hasher = TokenHasher::from_app_key(Some("test-pepper-key".into()));
        let token = "ps_abc123";
        assert_ne!(hasher.hash_for_store(token), legacy_sha256_hex(token));
    }

    #[test]
    fn candidate_hashes_include_legacy_for_migration() {
        let hasher = TokenHasher::from_app_key(Some("k".into()));
        let hashes = hasher.candidate_hashes("ps_x");
        assert_eq!(hashes.len(), 2);
        assert_eq!(hashes[1], legacy_sha256_hex("ps_x"));
    }

    #[test]
    fn no_key_uses_legacy_only() {
        let hasher = TokenHasher::from_app_key(None);
        let token = "dev-token-change-me";
        assert_eq!(hasher.hash_for_store(token), legacy_sha256_hex(token));
        assert_eq!(hasher.candidate_hashes(token).len(), 1);
    }
}
