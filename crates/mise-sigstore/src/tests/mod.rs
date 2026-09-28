use super::*;

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
mod retry;
mod slsa;
mod trust;
