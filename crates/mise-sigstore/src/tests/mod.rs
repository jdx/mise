use super::*;

/// Tests that load the Sigstore public-good TUF root take this lock, and the
/// test that points the process-global TUF URL at an unreachable host holds
/// it, so the override can't make another test's fetch fail.
pub(crate) static TUF_ROOT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn embedded_sigstore_root() -> TrustedRoot {
    TrustedRoot::from_json(sigstore_verify::trust_root::SIGSTORE_PRODUCTION_TRUSTED_ROOT)
        .expect("embedded production trusted_root.json parses")
}

/// A genuine `*.intoto.jsonl` produced by slsa-github-generator (sops
/// v3.9.0 release). Signed by Sigstore Fulcio. Tests that don't need a
/// matching artifact can run against this fixture alone.
const GENUINE_INTOTO_ENVELOPE: &str = include_str!("../../tests/fixtures/sops_v3_9_0.intoto.jsonl");

mod certificate;
mod cosign;
mod github;
mod retry;
mod slsa;
mod trust;
