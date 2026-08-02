use std::env;

/// GitHub OAuth App（`PROMPTSTDIO_GITHUB_CLIENT_ID` / `SECRET` + `PROMPTSTDIO_APP_URL`）。
#[derive(Debug, Clone)]
pub struct GitHubOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl GitHubOAuthConfig {
    pub fn from_env(app_public_url: &str) -> Option<Self> {
        let client_id = env::var("PROMPTSTDIO_GITHUB_CLIENT_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let client_secret = env::var("PROMPTSTDIO_GITHUB_CLIENT_SECRET")
            .ok()
            .filter(|s| !s.is_empty())?;
        let base = app_public_url.trim().trim_end_matches('/');
        Some(Self {
            client_id: client_id.trim().to_string(),
            client_secret,
            redirect_uri: format!("{base}/auth/github/callback"),
        })
    }

    pub fn authorize_url(&self, state: &str) -> String {
        format!(
            "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope={}&state={}",
            urlencoding::encode(&self.client_id),
            urlencoding::encode(&self.redirect_uri),
            urlencoding::encode("read:user user:email"),
            urlencoding::encode(state),
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.client_id.is_empty() {
            return Err("PROMPTSTDIO_GITHUB_CLIENT_ID 未配置".into());
        }
        if self.client_secret.is_empty() {
            return Err("PROMPTSTDIO_GITHUB_CLIENT_SECRET 未配置".into());
        }
        if self.redirect_uri.is_empty() {
            return Err("PROMPTSTDIO_APP_URL 未配置".into());
        }
        Ok(())
    }
}
