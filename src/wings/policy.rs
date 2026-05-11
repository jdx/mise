//! Signed mise-wings policy bundles.

use std::{collections::BTreeSet, path::PathBuf};

use eyre::{Context, Result, bail, ensure};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};

use crate::{env, file};

const POLICY_SCHEMA_VERSION: u32 = 1;
const POLICY_RULES_SCHEMA_VERSION: u32 = 3;
const POLICY_PUBLIC_KEY_ENV: &str = "MISE_WINGS_POLICY_PUBLIC_KEY";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct SignedPolicyBundle {
    pub policy: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PolicyBundlePayload {
    iss: String,
    schema_version: u32,
    org: String,
    pub(crate) policy_version: String,
    iat: i64,
    exp: i64,
    issued_at: String,
    valid_until: String,
    pub(crate) mode: PolicyMode,
    pub(crate) rules: PolicyRules,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PolicyMode {
    Observe,
    Prefer,
    Enforce,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PolicyRules {
    rules_schema_version: u32,
    #[serde(default)]
    minimum_artifact_age_seconds: i64,
    #[serde(default)]
    allowed_source_hosts: Vec<String>,
    #[serde(default)]
    require_checksum: bool,
    #[serde(default)]
    require_scan: bool,
    #[serde(default)]
    require_approval: bool,
    #[serde(default)]
    require_managed: bool,
    #[serde(default)]
    overrides: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ArtifactEvidence {
    pub(crate) referrer_artifact_types: BTreeSet<String>,
    pub(crate) source_checksum: Option<String>,
    pub(crate) scanned: bool,
    pub(crate) approved: bool,
    pub(crate) managed: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PolicyDecision {
    pub(crate) policy_version: String,
}

pub(crate) async fn fetch_cached_or_remote(token: &str) -> Result<PolicyBundlePayload> {
    let host = crate::wings::host();
    if let Some(creds) = crate::wings::credentials::cached()
        && creds.host == host
        && let Ok(policy) = load_cached(host, &creds.org)
    {
        return Ok(policy);
    }
    if let Ok(policy) = load_cached_for_host(host) {
        return Ok(policy);
    }

    let url = format!("https://api.{host}/v1/wings/policy");
    let mut headers = HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .wrap_err("wings token contains invalid header characters")?,
    );
    let bundle: SignedPolicyBundle = crate::wings::client::http_client()?
        .get(&url)
        .headers(headers)
        .send()
        .await
        .wrap_err_with(|| format!("GET {url}"))?
        .error_for_status()
        .wrap_err_with(|| format!("wings {url} returned non-2xx"))?
        .json()
        .await
        .wrap_err_with(|| format!("decoding {url} response body"))?;
    let policy = verify_policy_jws(&bundle.policy, host)?;
    store_cached(host, &policy.org, &bundle)?;
    Ok(policy)
}

pub(crate) fn evaluate(
    policy: &PolicyBundlePayload,
    evidence: &ArtifactEvidence,
) -> Result<PolicyDecision> {
    if policy.mode != PolicyMode::Enforce {
        return Ok(PolicyDecision {
            policy_version: policy.policy_version.clone(),
        });
    }

    ensure!(
        !policy.rules.require_checksum
            || evidence
                .source_checksum
                .as_deref()
                .is_some_and(|s| !s.is_empty()),
        "wings policy {} requires source checksum evidence",
        policy.policy_version
    );
    ensure!(
        !policy.rules.require_scan
            || evidence.scanned
            || evidence
                .referrer_artifact_types
                .contains("application/vnd.mise.scan.v1+json"),
        "wings policy {} requires scan evidence",
        policy.policy_version
    );
    ensure!(
        !policy.rules.require_approval || evidence.approved,
        "wings policy {} requires approved catalog trust",
        policy.policy_version
    );
    ensure!(
        !policy.rules.require_managed || evidence.managed,
        "wings policy {} requires managed catalog trust",
        policy.policy_version
    );

    Ok(PolicyDecision {
        policy_version: policy.policy_version.clone(),
    })
}

fn verify_policy_jws(policy: &str, host: &str) -> Result<PolicyBundlePayload> {
    let header = decode_header(policy).wrap_err("decoding wings policy header")?;
    let key = policy_decoding_key(host, header.kid.as_deref())?;
    let mut validation = Validation::new(Algorithm::ES256);
    validation.set_issuer(&[policy_issuer(host)]);
    let data = decode::<PolicyBundlePayload>(policy, &key, &validation)
        .wrap_err("verifying wings policy signature")?;
    validate_policy_payload(&data.claims, host)?;
    Ok(data.claims)
}

fn validate_policy_payload(policy: &PolicyBundlePayload, host: &str) -> Result<()> {
    ensure!(
        policy.schema_version == POLICY_SCHEMA_VERSION,
        "wings policy schemaVersion {} is not supported",
        policy.schema_version
    );
    ensure!(
        policy.rules.rules_schema_version == POLICY_RULES_SCHEMA_VERSION,
        "wings policy rulesSchemaVersion {} is not supported",
        policy.rules.rules_schema_version
    );
    ensure!(
        policy.iss == policy_issuer(host),
        "wings policy issuer {} does not match {}",
        policy.iss,
        policy_issuer(host)
    );
    ensure!(!policy.org.trim().is_empty(), "wings policy org is empty");
    Ok(())
}

fn policy_decoding_key(host: &str, kid: Option<&str>) -> Result<DecodingKey> {
    if let Ok(pem) = std::env::var(POLICY_PUBLIC_KEY_ENV) {
        return DecodingKey::from_ec_pem(pem.as_bytes())
            .wrap_err(format!("decoding {POLICY_PUBLIC_KEY_ENV}"));
    }
    bail!(
        "wings policy trust root is not configured for host {}{}; set {} to an ES256 public key PEM",
        host,
        kid.map(|kid| format!(" kid {kid}")).unwrap_or_default(),
        POLICY_PUBLIC_KEY_ENV
    )
}

fn policy_issuer(host: &str) -> String {
    format!("https://api.{host}/v1/wings/policy")
}

fn cache_dir() -> PathBuf {
    env::MISE_STATE_DIR.join("wings").join("policy")
}

fn cache_path(host: &str, org: &str) -> Result<PathBuf> {
    Ok(cache_dir().join(format!(
        "{}-{}.json",
        safe_cache_key(host),
        safe_cache_key(org)
    )))
}

fn safe_cache_key(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn load_cached(host: &str, org: &str) -> Result<PolicyBundlePayload> {
    let path = cache_path(host, org)?;
    let bundle: SignedPolicyBundle = serde_json::from_slice(&file::read(&path)?)
        .wrap_err_with(|| format!("decoding wings policy cache {}", path.display()))?;
    verify_policy_jws(&bundle.policy, host)
}

fn load_cached_for_host(host: &str) -> Result<PolicyBundlePayload> {
    let prefix = format!("{}-", safe_cache_key(host));
    for entry in std::fs::read_dir(cache_dir()).wrap_err("reading wings policy cache directory")? {
        let entry = entry?;
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !file_name.starts_with(&prefix) || !file_name.ends_with(".json") {
            continue;
        }
        let bundle: SignedPolicyBundle = serde_json::from_slice(&file::read(&path)?)
            .wrap_err_with(|| format!("decoding wings policy cache {}", path.display()))?;
        if let Ok(policy) = verify_policy_jws(&bundle.policy, host) {
            return Ok(policy);
        }
    }
    bail!("no valid wings policy cache entry found for {host}")
}

fn store_cached(host: &str, org: &str, bundle: &SignedPolicyBundle) -> Result<()> {
    let path = cache_path(host, org)?;
    if let Some(parent) = path.parent() {
        file::create_dir_all(parent)?;
    }
    file::write(&path, serde_json::to_string_pretty(bundle)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode};

    const TEST_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQghZ0zjApJqv6BhOi5
B2g+vLvlrB5jogweNzeGwK10aJehRANCAATeMDXqjEwB7nwc7hepBy2PUifpjhkj
99dx2zPl9+oaXU7c/TrEwbbR6b8kHGFpSXI8uMX9CxUAab6to7K90y/O
-----END PRIVATE KEY-----"#;
    const TEST_PUBLIC_KEY: &str = r#"-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE3jA16oxMAe58HO4XqQctj1In6Y4Z
I/fXcdsz5ffqGl1O3P06xMG20em/JBxhaUlyPLjF/QsVAGm+raOyvdMvzg==
-----END PUBLIC KEY-----"#;

    #[test]
    fn verifies_signed_policy_with_env_key() {
        unsafe {
            std::env::set_var(POLICY_PUBLIC_KEY_ENV, TEST_PUBLIC_KEY);
        }
        let policy = signed_policy("mise-wings.en.dev", PolicyMode::Enforce, |rules| {
            rules.require_checksum = true;
        });

        let verified = verify_policy_jws(&policy, "mise-wings.en.dev").unwrap();

        assert_eq!(verified.mode, PolicyMode::Enforce);
        assert!(verified.rules.require_checksum);
        unsafe {
            std::env::remove_var(POLICY_PUBLIC_KEY_ENV);
        }
    }

    #[test]
    fn enforce_policy_requires_configured_evidence() {
        let policy = policy_payload("mise-wings.en.dev", PolicyMode::Enforce, |rules| {
            rules.require_checksum = true;
            rules.require_scan = true;
        });
        let err = evaluate(&policy, &ArtifactEvidence::default()).unwrap_err();
        assert!(err.to_string().contains("source checksum"));

        let mut evidence = ArtifactEvidence {
            source_checksum: Some("sha256:abc".into()),
            ..Default::default()
        };
        let err = evaluate(&policy, &evidence).unwrap_err();
        assert!(err.to_string().contains("scan evidence"));

        evidence
            .referrer_artifact_types
            .insert("application/vnd.mise.scan.v1+json".into());
        evaluate(&policy, &evidence).unwrap();
    }

    #[test]
    fn observe_policy_allows_missing_evidence() {
        let policy = policy_payload("mise-wings.en.dev", PolicyMode::Observe, |rules| {
            rules.require_checksum = true;
            rules.require_scan = true;
        });

        evaluate(&policy, &ArtifactEvidence::default()).unwrap();
    }

    fn signed_policy(host: &str, mode: PolicyMode, f: impl FnOnce(&mut PolicyRules)) -> String {
        let payload = policy_payload(host, mode, f);
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some("test".into());
        encode(
            &header,
            &payload,
            &EncodingKey::from_ec_pem(TEST_PRIVATE_KEY.as_bytes()).unwrap(),
        )
        .unwrap()
    }

    fn policy_payload(
        host: &str,
        mode: PolicyMode,
        f: impl FnOnce(&mut PolicyRules),
    ) -> PolicyBundlePayload {
        let mut rules = PolicyRules {
            rules_schema_version: POLICY_RULES_SCHEMA_VERSION,
            minimum_artifact_age_seconds: 0,
            allowed_source_hosts: vec![],
            require_checksum: false,
            require_scan: false,
            require_approval: false,
            require_managed: false,
            overrides: vec![],
        };
        f(&mut rules);
        PolicyBundlePayload {
            iss: policy_issuer(host),
            schema_version: POLICY_SCHEMA_VERSION,
            org: "acme".into(),
            policy_version: "test:v1".into(),
            iat: 4_102_444_800,
            exp: 4_102_448_400,
            issued_at: "2100-01-01T00:00:00Z".into(),
            valid_until: "2100-01-01T01:00:00Z".into(),
            mode,
            rules,
        }
    }
}
