//! PromptStdio authentication.
//!
//! Handles login/logout and token management for connecting stitch
//! to the PromptStdio platform.

/// Log out by clearing the stored API token.
pub async fn logout(config: &mut crate::config::StitchConfig) -> anyhow::Result<()> {
    config.api_token = None;
    config.save()?;
    println!("✅ Logged out. Local credentials cleared.");
    Ok(())
}
