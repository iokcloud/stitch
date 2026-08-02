//! Interactive dialog prompts (confirmations, selections).

/// Ask the user for yes/no confirmation.
pub fn confirm(message: &str) -> bool {
    dialoguer::Confirm::new()
        .with_prompt(message)
        .default(false)
        .interact()
        .unwrap_or(false)
}
