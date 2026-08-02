//! HTTP transport for PromptStdio REST API.
//!
//! Uses `Authorization: Bearer <token>` for authentication against
//! the PromptStdio API v1 endpoints.

use reqwest::header::{AUTHORIZATION, HeaderValue};
use serde::de::DeserializeOwned;

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("not authenticated — run `stitch login` first")]
    NotAuthenticated,
    #[error("API error {status}: {body}")]
    ApiError { status: u16, body: String },
    #[error("request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),
}

/// HTTP transport for PromptStdio REST API calls.
pub struct ApiTransport {
    client: reqwest::Client,
    api_base: String,
    /// The user's PromptStdio API token (None if not logged in).
    pub api_token: Option<String>,
}

impl ApiTransport {
    pub fn new(api_base: String, api_token: Option<String>) -> Self {
        let api_base = crate::config::normalize_promptstdio_api_base(&api_base)
            .map(|s| s.to_string())
            .unwrap_or_else(|| api_base.trim().trim_end_matches('/').to_string());
        Self {
            client: reqwest::Client::new(),
            api_base,
            api_token,
        }
    }

    /// Check whether this transport has valid credentials.
    pub fn is_authenticated(&self) -> bool {
        self.api_token
            .as_ref()
            .map(|t| !t.is_empty())
            .unwrap_or(false)
    }

    /// Send a GET request to the PromptStdio API and deserialize the response.
    ///
    /// Expects the standard API response envelope: `{ "success": true, "data": ... }`
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let token = self
            .api_token
            .as_deref()
            .ok_or(TransportError::NotAuthenticated)?;

        let url = format!("{}{}", self.api_base, path);
        let resp = self
            .client
            .get(&url)
            .header(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|e| anyhow::anyhow!("invalid token: {e}"))?,
            )
            .send()
            .await?;

        let status = resp.status();
        let body_text = resp.text().await?;

        if !status.is_success() {
            return Err(TransportError::ApiError {
                status: status.as_u16(),
                body: body_text,
            }
            .into());
        }

        let envelope: serde_json::Value = serde_json::from_str(&body_text).map_err(|e| {
            anyhow::anyhow!("failed to parse API response: {e} — body: {body_text}")
        })?;

        let data = envelope
            .get("data")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("API response missing 'data' field: {body_text}"))?;

        let result: T = serde_json::from_value(data)
            .map_err(|e| anyhow::anyhow!("failed to deserialize API response: {e}"))?;

        Ok(result)
    }

    /// Send a POST request with a JSON body and deserialize the `data` envelope.
    pub async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> anyhow::Result<T> {
        let token = self
            .api_token
            .as_deref()
            .ok_or(TransportError::NotAuthenticated)?;

        let url = format!("{}{}", self.api_base, path);
        let resp = self
            .client
            .post(&url)
            .header(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|e| anyhow::anyhow!("invalid token: {e}"))?,
            )
            .json(body)
            .send()
            .await?;

        let status = resp.status();
        let body_text = resp.text().await?;

        if !status.is_success() {
            return Err(TransportError::ApiError {
                status: status.as_u16(),
                body: body_text,
            }
            .into());
        }

        let envelope: serde_json::Value = serde_json::from_str(&body_text).map_err(|e| {
            anyhow::anyhow!("failed to parse API response: {e} — body: {body_text}")
        })?;

        let data = envelope
            .get("data")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("API response missing 'data' field: {body_text}"))?;

        let result: T = serde_json::from_value(data)
            .map_err(|e| anyhow::anyhow!("failed to deserialize API response: {e}"))?;

        Ok(result)
    }

    /// POST that only requires a 2xx status (e.g. usage-logs/track returns bare 201).
    pub async fn post_ok(&self, path: &str, body: &serde_json::Value) -> anyhow::Result<()> {
        let token = self
            .api_token
            .as_deref()
            .ok_or(TransportError::NotAuthenticated)?;

        let url = format!("{}{}", self.api_base, path);
        let resp = self
            .client
            .post(&url)
            .header(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|e| anyhow::anyhow!("invalid token: {e}"))?,
            )
            .json(body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(TransportError::ApiError {
                status: status.as_u16(),
                body: body_text,
            }
            .into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_new_creates_client() {
        let t = ApiTransport::new("https://example.com".into(), Some("tok".into()));
        assert!(t.is_authenticated());
    }

    #[test]
    fn transport_not_authenticated_when_token_none() {
        let t = ApiTransport::new("https://example.com".into(), None);
        assert!(!t.is_authenticated());
    }

    #[test]
    fn transport_not_authenticated_when_token_empty() {
        let t = ApiTransport::new("https://example.com".into(), Some("".into()));
        assert!(!t.is_authenticated());
    }

    #[tokio::test]
    async fn get_without_token_returns_error() {
        let t = ApiTransport::new("https://example.com".into(), None);
        let err = t
            .get::<serde_json::Value>("/api/v1/test")
            .await
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("not authenticated"),
            "expected auth error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn get_with_invalid_base_url_yields_connection_error() {
        let t = ApiTransport::new("http://127.0.0.1:1".into(), Some("bogus-token".into()));
        let err = t
            .get::<serde_json::Value>("/api/v1/test")
            .await
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("request failed") || msg.contains("error") || msg.contains("connect"),
            "expected connection error, got: {msg}"
        );
    }

    #[test]
    fn transport_error_display_not_authenticated() {
        let e = TransportError::NotAuthenticated;
        assert_eq!(
            e.to_string(),
            "not authenticated — run `stitch login` first"
        );
    }

    #[test]
    fn transport_error_display_api_error() {
        let e = TransportError::ApiError {
            status: 404,
            body: "not found".into(),
        };
        assert_eq!(e.to_string(), "API error 404: not found");
    }
}
