//! Keeps the GitHub OAuth refresh token behind Touch ID on macOS.
//!
//! The helper is a small Swift app embedded in mise (see `build_enclave_helper`
//! in `build.rs`) and installed on first use, like the notification helper. It
//! seals a secret to the Secure Enclave on `seal` and needs a biometric check
//! on `unseal`. Only Developer ID signed release builds ship a usable helper;
//! other builds report it as unavailable and mise keeps the token in its cache
//! file.

#[cfg(target_os = "macos")]
mod macos;

/// Register the helper with the OAuth cache, which lives in a lower crate.
pub fn register() {
    #[cfg(target_os = "macos")]
    mise_util::github::oauth::set_enclave_helper(macos::helper);
}
