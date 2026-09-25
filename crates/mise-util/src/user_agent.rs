//! The User-Agent mise sends with HTTP requests, registered by mise at startup
//! (it names mise's version and the shell it was invoked from).

use std::sync::OnceLock;

static USER_AGENT: OnceLock<String> = OnceLock::new();

/// Register the User-Agent, e.g. `mise/2026.9.14 macos-arm64 (2026-09-25) zsh`.
pub fn set(user_agent: String) {
    let _ = USER_AGENT.set(user_agent);
}

/// The registered User-Agent.
pub fn get() -> &'static str {
    USER_AGENT
        .get()
        .expect("mise_util::user_agent::set must be called first")
}
