//! Client for the formulae.brew.sh JSON API (static JSON, no auth).

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use eyre::{WrapErr, bail, eyre};
use ring::signature::{RSA_PSS_2048_8192_SHA512, UnparsedPublicKey};
use serde::Deserialize;
use serde_json::Value;

use crate::http::HTTP_FETCH;
use crate::result::Result;

const API_BASE: &str = "https://formulae.brew.sh/api";
const HOMEBREW_1_SPKI_B64: &str = "MIICIjANBgkqhkiG9w0BAQEFAAOCAg8AMIICCgKCAgEAyKoOYzp1rhwXISRi61BYXBEr2PalSK8lEVOL2USy7mpy0OubOlFyujawyQcBcCn+uPOJ/WaK+POhNWcLLoiKL2m8GViaQm7SMwdLKUXFgKSPHcG/1m6Vu+TNBKTfQqT60PjEYIrn5NW9ZrM0cUhKREmsbeAMBevdSaW9UwY9iIhprrgovvT8SzKhF8ZOIZKXfJX4VNk0y/7VJYNuGGqH3npxV7OKd4yTGRGqFcC9kJ84me3thiu0yqlOjASmfWIwIwcfp4j6BEM2LuqKd7yXh51/O+MTthkuxV36moDKfdgdOFsvlCFkziaYLScCX9lOlmZHtOfJTAOXxTmM7qGrwTGK0vhvTi8k9dBmH/dccredQBtPOfM/FEdeyakGLoTcDguiBS/4El3I2KtF6B2hOGoBumR915/cI4drr5yPMduZ7gjs7ZEZnVkeVzic24TfUHpnOYzrhucNJtHMBDj96d1Gk82AhtuF9KlusLmCb6qXCWQSp/A4RZpN37E/p9q8rLp/7B/zp8X2TVvecPNyBdMagdktdEqK7WPlYMcUp56JaOph8vqYoU+oGyCpWoLvcXFb75o4eefuu6Rs5SyMc9JCCJ0DDFPjCRFnGPkvsKxFCzMFqH1jpWH0RQIrgmNVM5PO84iRH9YJsSPQzpMjKvK/ZH4YgR9wNkBNagFo7lsCAwEAAQ==";
const BASE64_URL_NO_PAD: GeneralPurpose = GeneralPurpose::new(
    &alphabet::URL_SAFE,
    GeneralPurposeConfig::new()
        .with_encode_padding(false)
        .with_decode_padding_mode(DecodePaddingMode::RequireNone),
);
const BASE64_URL_SIGNATURE: GeneralPurpose = GeneralPurpose::new(
    &alphabet::URL_SAFE,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FetchMode {
    Cached,
    Fresh,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Formula {
    pub name: String,
    #[serde(default)]
    pub tap: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub versions: Versions,
    #[serde(default)]
    pub revision: u32,
    #[serde(default)]
    pub keg_only: bool,
    /// runtime dependencies (formula names)
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// build-time-only dependencies — needed for source builds, not pours
    #[serde(default)]
    pub build_dependencies: Vec<String>,
    #[serde(default)]
    pub bottle: HashMap<String, BottleSpec>,
    /// per-bottle-tag overrides (e.g. different dependencies on some platforms)
    #[serde(default)]
    pub variations: HashMap<String, Variation>,
    /// source download specs keyed by spec name ("stable")
    #[serde(default)]
    pub urls: HashMap<String, SourceUrl>,
    /// formula .rb location in homebrew/core (e.g. "Formula/h/hello.rb")
    #[serde(default)]
    pub ruby_source_path: Option<String>,
    #[serde(default)]
    pub ruby_source_checksum: Option<RubySourceChecksum>,
    /// homebrew/core commit this API snapshot was generated from
    #[serde(default)]
    pub tap_git_head: Option<String>,
    #[serde(default)]
    pub post_install_steps: Vec<Value>,
    #[serde(default)]
    pub post_install_defined: bool,
    /// Homebrew install policy that must be checked before any keg mutation.
    /// Kept grouped so callers cannot accidentally deserialize a policy field
    /// without including it in the common validation boundary.
    #[serde(flatten)]
    pub(super) install_policy: FormulaInstallPolicy,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct FormulaInstallPolicy {
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    disable_date: Option<String>,
    #[serde(default)]
    disable_reason: Option<String>,
    #[serde(default)]
    disable_replacement_formula: Option<String>,
    #[serde(default)]
    disable_replacement_cask: Option<String>,
    #[serde(default)]
    deprecated: bool,
    #[serde(default)]
    deprecation_date: Option<String>,
    #[serde(default)]
    deprecation_reason: Option<String>,
    #[serde(default)]
    deprecation_replacement_formula: Option<String>,
    #[serde(default)]
    deprecation_replacement_cask: Option<String>,
    #[serde(default)]
    requirements: Vec<FormulaRequirement>,
    #[serde(default)]
    pour_bottle_only_if: Option<String>,
    #[serde(default)]
    conflicts_with: Vec<String>,
    #[serde(default)]
    conflicts_with_reasons: Vec<Option<String>>,
    #[serde(default)]
    link_overwrite: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct FormulaRequirement {
    name: String,
    #[serde(default)]
    cask: Option<String>,
    #[serde(default)]
    download: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    contexts: Vec<String>,
    #[serde(default)]
    specs: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceUrl {
    pub url: String,
    /// sha256 of the source archive; absent for VCS sources
    #[serde(default)]
    pub checksum: Option<String>,
    /// non-default download strategy (":git", ":svn", ...) — unsupported
    #[serde(default)]
    pub using: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RubySourceChecksum {
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Versions {
    pub stable: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BottleSpec {
    #[serde(default)]
    pub rebuild: u32,
    #[serde(default)]
    pub files: HashMap<String, BottleFile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BottleFile {
    /// ":any", ":any_skip_relocation", or a pinned cellar path
    pub cellar: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Variation {
    #[serde(default)]
    pub dependencies: Option<Vec<String>>,
    #[serde(default)]
    pub build_dependencies: Option<Vec<String>>,
}

impl Formula {
    /// Validate Homebrew policy that mise must understand before installing.
    ///
    /// `link_overwrite` is intentionally represented but does not grant
    /// overwrite authority: mise's topology preflight remains conservative
    /// and rejects an occupied destination. This preserves user/Homebrew state
    /// until exact typed overwrite ownership exists.
    pub fn validate_install_policy(&self) -> Result<()> {
        let policy = &self.install_policy;
        if policy.disabled {
            let detail = policy_detail(
                policy.disable_date.as_deref(),
                policy.disable_reason.as_deref(),
                policy.disable_replacement_formula.as_deref(),
                policy.disable_replacement_cask.as_deref(),
            );
            bail!(
                "brew:{} is disabled by Homebrew{detail}; mise will not install it",
                self.name
            );
        }

        let unsupported_requirements = policy
            .requirements
            .iter()
            .filter(|requirement| !requirement.is_satisfied())
            .collect::<Vec<_>>();
        if !unsupported_requirements.is_empty() {
            let requirements = unsupported_requirements
                .into_iter()
                .map(FormulaRequirement::describe)
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "brew:{} declares unsupported Homebrew requirements ({requirements}); refusing to install without exact requirement enforcement",
                self.name
            );
        }

        if let Some(condition) = policy.pour_bottle_only_if.as_deref() {
            bail!(
                "brew:{} declares unsupported pour_bottle_only_if policy {condition:?}; refusing to install without exact predicate enforcement",
                self.name
            );
        }

        if self
            .link_overwrite()
            .iter()
            .any(|pattern| pattern.trim().is_empty())
        {
            bail!(
                "brew:{} declares an empty link_overwrite pattern; refusing ambiguous overwrite policy",
                self.name
            );
        }

        if policy.deprecated {
            let detail = policy_detail(
                policy.deprecation_date.as_deref(),
                policy.deprecation_reason.as_deref(),
                policy.deprecation_replacement_formula.as_deref(),
                policy.deprecation_replacement_cask.as_deref(),
            );
            warn!("brew:{} is deprecated by Homebrew{detail}", self.name);
        }
        Ok(())
    }

    pub fn conflicts_with(&self) -> &[String] {
        &self.install_policy.conflicts_with
    }

    pub fn conflict_reason(&self, name: &str) -> Option<&str> {
        let index = self
            .install_policy
            .conflicts_with
            .iter()
            .position(|conflict| conflict == name)?;
        self.install_policy
            .conflicts_with_reasons
            .get(index)
            .and_then(|reason| reason.as_deref())
    }

    /// Patterns are informational until an exact ownership-aware overwrite
    /// implementation exists. Callers must not treat them as mutation authority.
    pub fn link_overwrite(&self) -> &[String] {
        &self.install_policy.link_overwrite
    }

    /// keg directory name: version plus brew's bottle revision suffix
    pub fn pkg_version(&self) -> Result<String> {
        let stable = self
            .versions
            .stable
            .as_ref()
            .ok_or_else(|| eyre!("formula {} has no stable version", self.name))?;
        Ok(if self.revision > 0 {
            format!("{stable}_{}", self.revision)
        } else {
            stable.clone()
        })
    }

    /// runtime dependencies for the given bottle tag, applying `variations`
    pub fn dependencies_for(&self, tag: &str) -> &[String] {
        if let Some(v) = self.variations.get(tag)
            && let Some(deps) = &v.dependencies
        {
            return deps;
        }
        &self.dependencies
    }

    /// build-time dependencies for the given bottle tag, applying `variations`
    pub fn build_dependencies_for(&self, tag: &str) -> &[String] {
        if let Some(v) = self.variations.get(tag)
            && let Some(deps) = &v.build_dependencies
        {
            return deps;
        }
        &self.build_dependencies
    }

    pub fn bottle_files(&self) -> Option<&HashMap<String, BottleFile>> {
        self.bottle.get("stable").map(|b| &b.files)
    }

    /// the stable source archive spec, when present
    pub fn stable_url(&self) -> Option<&SourceUrl> {
        self.urls.get("stable")
    }
}

impl FormulaRequirement {
    fn is_satisfied(&self) -> bool {
        if cfg!(target_os = "macos")
            && self.name == "macos"
            && self.version.is_none()
            && self.cask.is_none()
            && self.download.is_none()
            && self.contexts.is_empty()
            && !self.specs.is_empty()
            && self
                .specs
                .iter()
                .all(|spec| matches!(spec.as_str(), "stable" | "head"))
        {
            return true;
        }
        if !cfg!(target_os = "macos")
            || self.name != "xcode"
            || self.version.is_some()
            || self.cask.is_some()
            || self.download.is_some()
            || self.contexts.as_slice() != ["build"]
            || self.specs.as_slice() != ["stable"]
        {
            return false;
        }
        let selected = std::process::Command::new("/usr/bin/xcode-select")
            .arg("-p")
            .output()
            .is_ok_and(|output| output.status.success() && !output.stdout.is_empty());
        let compiler = std::process::Command::new("/usr/bin/xcrun")
            .args(["--find", "clang"])
            .output()
            .is_ok_and(|output| output.status.success() && !output.stdout.is_empty());
        selected && compiler
    }

    fn describe(&self) -> String {
        let mut attributes = Vec::new();
        if let Some(version) = &self.version {
            attributes.push(format!("version={version}"));
        }
        if let Some(cask) = &self.cask {
            attributes.push(format!("cask={cask}"));
        }
        if let Some(download) = &self.download {
            attributes.push(format!("download={download}"));
        }
        if !self.contexts.is_empty() {
            attributes.push(format!("contexts={}", self.contexts.join("|")));
        }
        if !self.specs.is_empty() {
            attributes.push(format!("specs={}", self.specs.join("|")));
        }
        if attributes.is_empty() {
            self.name.clone()
        } else {
            format!("{}[{}]", self.name, attributes.join(","))
        }
    }
}

fn policy_detail(
    date: Option<&str>,
    reason: Option<&str>,
    replacement_formula: Option<&str>,
    replacement_cask: Option<&str>,
) -> String {
    let mut details = Vec::new();
    if let Some(date) = date {
        details.push(format!("date {date}"));
    }
    if let Some(reason) = reason {
        details.push(format!("reason {reason}"));
    }
    if let Some(replacement) = replacement_formula {
        details.push(format!("replacement formula {replacement}"));
    }
    if let Some(replacement) = replacement_cask {
        details.push(format!("replacement cask {replacement}"));
    }
    if details.is_empty() {
        String::new()
    } else {
        format!(" ({})", details.join(", "))
    }
}

/// Fetch formula metadata by name (or alias — brew's API redirects aliases
/// to the canonical formula).
pub(super) async fn formula_with_mode(name: &str, mode: FetchMode) -> Result<Formula> {
    let _ = mode;
    internal_formula(name).await
}

#[derive(Deserialize)]
struct InternalApiEnvelope {
    payload: String,
    signatures: Vec<InternalApiSignature>,
}

#[derive(Deserialize)]
struct InternalApiSignature {
    protected: String,
    signature: String,
    header: InternalApiSignatureHeader,
}

#[derive(Deserialize)]
struct InternalApiSignatureHeader {
    kid: String,
}

#[derive(Deserialize)]
struct InternalApiProtectedHeader {
    alg: String,
    b64: bool,
    crit: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct InternalFormulaIndex {
    formulae: HashMap<String, InternalFormula>,
    #[serde(default)]
    formula_aliases: HashMap<String, String>,
    #[serde(default)]
    formula_renames: HashMap<String, String>,
    formula_tap_git_head: String,
    metadata: InternalMetadata,
}

#[derive(Debug, Deserialize)]
struct InternalMetadata {
    bottle_tag: String,
}

/// Homebrew's signed, host-projected install record. Keep this exhaustive so
/// a new upstream field cannot silently acquire installation semantics.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InternalFormula {
    stable_version: Option<String>,
    #[serde(default)]
    revision: u32,
    #[serde(default)]
    #[serde(rename = "version_scheme")]
    _version_scheme: u64,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    oldnames: Vec<String>,
    #[serde(default)]
    stable_url_args: Vec<Value>,
    #[serde(default)]
    stable_checksum: Option<String>,
    #[serde(default)]
    stable_dependencies: Vec<Value>,
    #[serde(default)]
    stable_uses_from_macos: Vec<Value>,
    #[serde(default)]
    stable_patches: Vec<Value>,
    #[serde(default)]
    bottle_checksum: Option<String>,
    #[serde(default)]
    bottle_cellar: Option<String>,
    #[serde(default)]
    bottle_rebuild: u32,
    #[serde(default)]
    bottle_tag: Option<String>,
    #[serde(default)]
    ruby_source_checksum: Option<String>,
    #[serde(default)]
    keg_only_args: Vec<Value>,
    #[serde(default)]
    disable_args: Option<InternalLifecyclePolicy>,
    #[serde(default)]
    deprecate_args: Option<InternalLifecyclePolicy>,
    #[serde(default)]
    pour_bottle_args: Option<InternalPourPolicy>,
    #[serde(default)]
    conflicts: Vec<(String, InternalConflictPolicy)>,
    #[serde(default)]
    link_overwrite_paths: Vec<String>,
    #[serde(default)]
    post_install_steps: Vec<Value>,
    #[serde(default)]
    #[serde(rename = "desc")]
    _desc: Option<String>,
    #[serde(default)]
    #[serde(rename = "executables")]
    _executables: Vec<String>,
    #[serde(default)]
    #[serde(rename = "homepage")]
    _homepage: Option<String>,
    #[serde(default)]
    #[serde(rename = "license")]
    _license: Option<Value>,
    #[serde(default)]
    #[serde(rename = "caveats")]
    _caveats: Option<Value>,
    #[serde(default)]
    #[serde(rename = "head_dependencies")]
    _head_dependencies: Vec<Value>,
    #[serde(default)]
    #[serde(rename = "head_url_args")]
    _head_url_args: Vec<Value>,
    #[serde(default)]
    #[serde(rename = "head_uses_from_macos")]
    _head_uses_from_macos: Vec<Value>,
    #[serde(default)]
    #[serde(rename = "no_autobump_args")]
    _no_autobump_args: Option<Value>,
    #[serde(default)]
    #[serde(rename = "service_args")]
    _service_args: Option<Value>,
    #[serde(default)]
    #[serde(rename = "service_name_args")]
    _service_name_args: Option<Value>,
    #[serde(default)]
    #[serde(rename = "service_run_args")]
    _service_run_args: Option<Value>,
    #[serde(default)]
    #[serde(rename = "service_run_kwargs")]
    _service_run_kwargs: Option<Value>,
    #[serde(default)]
    #[serde(rename = "versioned_formulae")]
    _versioned_formulae: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct InternalLifecyclePolicy {
    #[serde(default, rename = ":date")]
    date: Option<String>,
    #[serde(default, rename = ":because")]
    reason: Option<String>,
    #[serde(default, rename = ":replacement_formula")]
    replacement_formula: Option<String>,
    #[serde(default, rename = ":replacement_cask")]
    replacement_cask: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InternalPourPolicy {
    #[serde(rename = ":only_if")]
    only_if: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct InternalConflictPolicy {
    #[serde(default, rename = ":because")]
    reason: Option<String>,
}

static INTERNAL_FORMULAE: tokio::sync::OnceCell<
    std::result::Result<Arc<InternalFormulaIndex>, String>,
> = tokio::sync::OnceCell::const_new();

pub(super) fn verify_internal_api_envelope(raw: &str) -> Result<String> {
    let spki = base64::engine::general_purpose::STANDARD
        .decode(HOMEBREW_1_SPKI_B64)
        .wrap_err("invalid embedded Homebrew API trust anchor")?;
    verify_internal_api_envelope_with_key(raw, &spki)
}

fn verify_internal_api_envelope_with_key(raw: &str, spki: &[u8]) -> Result<String> {
    let envelope: InternalApiEnvelope =
        serde_json::from_str(raw).wrap_err("invalid Homebrew internal API envelope")?;
    let signature = envelope
        .signatures
        .iter()
        .find(|signature| signature.header.kid == "homebrew-1")
        .ok_or_else(|| eyre!("Homebrew internal API envelope has no trusted signature"))?;
    let protected = BASE64_URL_NO_PAD
        .decode(&signature.protected)
        .wrap_err("invalid Homebrew internal API protected header encoding")?;
    let protected: InternalApiProtectedHeader = serde_json::from_slice(&protected)
        .wrap_err("invalid Homebrew internal API protected header")?;
    if protected.alg != "PS512" || protected.b64 || protected.crit.as_slice() != ["b64"] {
        bail!("unsupported Homebrew internal API signature parameters");
    }
    let signature_bytes = BASE64_URL_SIGNATURE
        .decode(&signature.signature)
        .wrap_err("invalid Homebrew internal API signature encoding")?;
    let mut signing_input =
        Vec::with_capacity(signature.protected.len() + 1 + envelope.payload.len());
    signing_input.extend_from_slice(signature.protected.as_bytes());
    signing_input.push(b'.');
    signing_input.extend_from_slice(envelope.payload.as_bytes());
    let rsa_public_key = rsa_public_key_from_spki(spki)?;
    UnparsedPublicKey::new(&RSA_PSS_2048_8192_SHA512, rsa_public_key)
        .verify(&signing_input, &signature_bytes)
        .map_err(|_| eyre!("Homebrew internal API signature verification failed"))?;
    Ok(envelope.payload)
}

fn rsa_public_key_from_spki(spki: &[u8]) -> Result<&[u8]> {
    const RSA_ENCRYPTION_ALGORITHM: &[u8] = &[
        0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00,
    ];
    let mut input = spki;
    let sequence = der_value(&mut input, 0x30)?;
    if !input.is_empty() {
        bail!("Homebrew API trust anchor has trailing DER data");
    }
    let mut sequence = sequence;
    let algorithm = der_value(&mut sequence, 0x30)?;
    if algorithm != RSA_ENCRYPTION_ALGORITHM {
        bail!("Homebrew API trust anchor uses an unsupported key algorithm");
    }
    let bit_string = der_value(&mut sequence, 0x03)?;
    if !sequence.is_empty() || bit_string.first() != Some(&0) || bit_string.len() == 1 {
        bail!("Homebrew API trust anchor has an invalid public-key bit string");
    }
    Ok(&bit_string[1..])
}

fn der_value<'a>(input: &mut &'a [u8], expected_tag: u8) -> Result<&'a [u8]> {
    let (&tag, rest) = input
        .split_first()
        .ok_or_else(|| eyre!("truncated Homebrew API trust anchor"))?;
    if tag != expected_tag {
        bail!("invalid Homebrew API trust anchor DER tag");
    }
    let (&first_length, mut rest) = rest
        .split_first()
        .ok_or_else(|| eyre!("truncated Homebrew API trust anchor"))?;
    let length = if first_length & 0x80 == 0 {
        usize::from(first_length)
    } else {
        let octets = usize::from(first_length & 0x7f);
        if octets == 0 || octets > std::mem::size_of::<usize>() || rest.len() < octets {
            bail!("invalid Homebrew API trust anchor DER length");
        }
        if rest[0] == 0 {
            bail!("non-minimal Homebrew API trust anchor DER length");
        }
        let mut length = 0usize;
        for &octet in &rest[..octets] {
            length = length
                .checked_mul(256)
                .and_then(|length| length.checked_add(usize::from(octet)))
                .ok_or_else(|| eyre!("oversized Homebrew API trust anchor DER length"))?;
        }
        if length < 128 {
            bail!("non-minimal Homebrew API trust anchor DER length");
        }
        rest = &rest[octets..];
        length
    };
    if rest.len() < length {
        bail!("truncated Homebrew API trust anchor DER value");
    }
    let (value, trailing) = rest.split_at(length);
    *input = trailing;
    Ok(value)
}

async fn internal_formula(name: &str) -> Result<Formula> {
    let url = format!(
        "{API_BASE}/internal/packages.{}.jws.json",
        super::tag::host_tag()
    );
    let result = INTERNAL_FORMULAE
        .get_or_init(|| async {
            let raw = HTTP_FETCH
                .get_text(&url)
                .await
                .map_err(|err| err.to_string())?;
            let payload = verify_internal_api_envelope(&raw).map_err(|err| err.to_string())?;
            serde_json::from_str(&payload)
                .map(Arc::new)
                .map_err(|err| err.to_string())
        })
        .await;
    let index = result
        .as_ref()
        .map_err(|err| eyre!("failed to load Homebrew internal formula API: {err}"))?;
    let canonical = canonical_internal_name(index, name)?;
    let signed = index
        .formulae
        .get(&canonical)
        .ok_or_else(|| eyre!("Homebrew internal API has no formula '{name}'"))?;
    formula_from_internal(index, &canonical, signed, &url)
}

fn canonical_internal_name(index: &InternalFormulaIndex, requested: &str) -> Result<String> {
    let mut current = requested;
    for _ in 0..3 {
        if index.formulae.contains_key(current) {
            return Ok(current.to_string());
        }
        let Some(next) = index
            .formula_aliases
            .get(current)
            .or_else(|| index.formula_renames.get(current))
        else {
            bail!("Homebrew internal API has no formula '{requested}'");
        };
        current = next;
    }
    bail!("Homebrew internal API has a cyclic alias or rename for '{requested}'")
}

fn formula_from_internal(
    index: &InternalFormulaIndex,
    name: &str,
    signed: &InternalFormula,
    _source: &str,
) -> Result<Formula> {
    validate_formula_name(name)?;
    if !signed.stable_patches.is_empty() {
        bail!("brew:{name} has signed source patches that mise cannot apply safely");
    }
    let mut dependencies = Vec::new();
    let mut build_dependencies = Vec::new();
    for dependency in &signed.stable_dependencies {
        parse_internal_dependency(dependency, &mut dependencies, &mut build_dependencies)?;
    }
    if cfg!(target_os = "linux") {
        for dependency in &signed.stable_uses_from_macos {
            parse_uses_from_macos_dependency(
                dependency,
                &mut dependencies,
                &mut build_dependencies,
            )?;
        }
    }
    dependencies.sort();
    dependencies.dedup();
    build_dependencies.sort();
    build_dependencies.dedup();

    let stable_url = parse_internal_source(name, signed)?;
    let bottle_tag = signed
        .bottle_tag
        .as_deref()
        .unwrap_or(&index.metadata.bottle_tag);
    if bottle_tag != index.metadata.bottle_tag {
        bail!("brew:{name} signed bottle tag disagrees with the signed index metadata");
    }
    let mut bottle = HashMap::new();
    if let Some(checksum) = signed.bottle_checksum.as_deref() {
        validate_sha256("bottle", checksum)?;
        let file = BottleFile {
            cellar: signed
                .bottle_cellar
                .clone()
                .unwrap_or_else(|| ":any_skip_relocation".to_string()),
            url: format!("https://ghcr.io/v2/homebrew/core/{name}/blobs/sha256:{checksum}"),
            sha256: checksum.to_string(),
        };
        bottle.insert(
            "stable".to_string(),
            BottleSpec {
                rebuild: signed.bottle_rebuild,
                files: HashMap::from([(bottle_tag.to_string(), file)]),
            },
        );
    }

    let ruby_checksum = signed
        .ruby_source_checksum
        .as_deref()
        .map(|checksum| {
            validate_sha256("formula source", checksum)?;
            Ok::<_, eyre::Report>(RubySourceChecksum {
                sha256: Some(checksum.to_string()),
            })
        })
        .transpose()?;
    let first = name
        .chars()
        .next()
        .ok_or_else(|| eyre!("empty signed formula name"))?;
    let aliases = index
        .formula_aliases
        .iter()
        .filter_map(|(alias, canonical)| (canonical == name).then_some(alias.clone()))
        .chain(signed.aliases.iter().cloned())
        .chain(signed.oldnames.iter().cloned())
        .collect();
    let disable = signed.disable_args.as_ref();
    let deprecate = signed.deprecate_args.as_ref();
    Ok(Formula {
        name: name.to_string(),
        tap: Some("homebrew/core".to_string()),
        aliases,
        versions: Versions {
            stable: signed.stable_version.clone(),
        },
        revision: signed.revision,
        keg_only: !signed.keg_only_args.is_empty(),
        dependencies,
        build_dependencies,
        bottle,
        variations: HashMap::new(),
        urls: stable_url
            .map(|url| HashMap::from([("stable".to_string(), url)]))
            .unwrap_or_default(),
        ruby_source_path: Some(format!("Formula/{first}/{name}.rb")),
        ruby_source_checksum: ruby_checksum,
        tap_git_head: Some(index.formula_tap_git_head.clone()),
        post_install_defined: !signed.post_install_steps.is_empty(),
        post_install_steps: signed.post_install_steps.clone(),
        install_policy: FormulaInstallPolicy {
            disabled: disable.is_some(),
            disable_date: disable.and_then(|p| p.date.clone()),
            disable_reason: disable.and_then(|p| p.reason.clone()),
            disable_replacement_formula: disable.and_then(|p| p.replacement_formula.clone()),
            disable_replacement_cask: disable.and_then(|p| p.replacement_cask.clone()),
            deprecated: deprecate.is_some(),
            deprecation_date: deprecate.and_then(|p| p.date.clone()),
            deprecation_reason: deprecate.and_then(|p| p.reason.clone()),
            deprecation_replacement_formula: deprecate.and_then(|p| p.replacement_formula.clone()),
            deprecation_replacement_cask: deprecate.and_then(|p| p.replacement_cask.clone()),
            requirements: Vec::new(),
            pour_bottle_only_if: signed.pour_bottle_args.as_ref().map(|p| p.only_if.clone()),
            conflicts_with: signed
                .conflicts
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
            conflicts_with_reasons: signed
                .conflicts
                .iter()
                .map(|(_, policy)| policy.reason.clone())
                .collect(),
            link_overwrite: signed.link_overwrite_paths.clone(),
        },
    })
}

fn validate_formula_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"@+._-".contains(&byte))
    {
        bail!("Homebrew internal API has an unsafe formula name {name:?}");
    }
    Ok(())
}

fn validate_sha256(kind: &str, checksum: &str) -> Result<()> {
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("signed Homebrew {kind} checksum is not a sha256");
    }
    Ok(())
}

fn parse_internal_source(name: &str, signed: &InternalFormula) -> Result<Option<SourceUrl>> {
    if signed.stable_url_args.is_empty() {
        if signed.stable_checksum.is_some() {
            bail!("brew:{name} has a signed source checksum without a source URL");
        }
        return Ok(None);
    }
    let url = signed.stable_url_args[0]
        .as_str()
        .ok_or_else(|| eyre!("brew:{name} signed source URL is not a string"))?;
    let checksum = signed.stable_checksum.as_deref();
    if let Some(checksum) = checksum {
        validate_sha256("source", checksum)?;
    }
    // Additional URL arguments describe VCS strategies, tags, revisions, or
    // other download behavior. Preserve the URL for diagnostics but mark the
    // strategy unsupported instead of guessing at Homebrew DSL semantics.
    let using = (signed.stable_url_args.len() > 1).then(|| "signed URL options".to_string());
    Ok(Some(SourceUrl {
        url: url.to_string(),
        checksum: checksum.map(str::to_string),
        using,
    }))
}

fn parse_internal_dependency(
    value: &Value,
    runtime: &mut Vec<String>,
    build: &mut Vec<String>,
) -> Result<()> {
    if let Some(name) = value.as_str() {
        validate_formula_name(name)?;
        runtime.push(name.to_string());
        return Ok(());
    }
    let object = value
        .as_object()
        .filter(|object| object.len() == 1)
        .ok_or_else(|| eyre!("unsupported signed Homebrew dependency shape: {value}"))?;
    let (name, qualifier) = object.iter().next().unwrap();
    validate_formula_name(name)?;
    let qualifiers = match qualifier {
        Value::String(value) => vec![value.as_str()],
        Value::Array(values) => values
            .iter()
            .map(|value| {
                value.as_str().ok_or_else(|| {
                    eyre!("unsupported signed Homebrew dependency qualifier: {qualifier}")
                })
            })
            .collect::<Result<Vec<_>>>()?,
        _ => bail!("unsupported signed Homebrew dependency qualifier: {qualifier}"),
    };
    if qualifiers.contains(&":build") {
        build.push(name.clone());
    } else if qualifiers
        .iter()
        .all(|q| *q == ":test" || *q == ":optional")
    {
        // Test and opt-in dependencies are not part of a default installation.
    } else if qualifiers.iter().all(|q| *q == ":recommended") {
        runtime.push(name.clone());
    } else {
        bail!("unsupported signed Homebrew dependency qualifier: {qualifier}");
    }
    Ok(())
}

fn parse_uses_from_macos_dependency(
    value: &Value,
    runtime: &mut Vec<String>,
    build: &mut Vec<String>,
) -> Result<()> {
    let dependency = value
        .as_array()
        .and_then(|values| values.first())
        .ok_or_else(|| eyre!("unsupported signed uses_from_macos shape: {value}"))?;
    parse_internal_dependency(dependency, runtime, build)
}

pub(super) async fn formula_with_tap_name_mode(
    name: &str,
    tap_name: Option<&str>,
    tap_url: Option<&str>,
    mode: FetchMode,
) -> Result<Formula> {
    let Some((owner, tap, formula_name)) = split_tap_name(name).or_else(|| {
        let (owner, tap) = split_tap(tap_name?)?;
        Some((owner, tap, name))
    }) else {
        return formula_with_mode(name, mode).await;
    };
    if owner == "homebrew" && tap == "core" {
        return formula_with_mode(formula_name, mode).await;
    }
    let Some(url) = tap_formula_api_url(owner, tap, formula_name, tap_url) else {
        bail!(
            "brew: tapped formula '{name}' needs a GitHub tap URL in [bootstrap.brew.taps] \
             so mise can fetch metadata directly without the brew CLI"
        );
    };
    let result = match mode {
        FetchMode::Cached => HTTP_FETCH.json_cached::<Formula, _>(url).await,
        FetchMode::Fresh => HTTP_FETCH.json::<Formula, _>(url).await,
    };
    result.wrap_err_with(|| {
        format!(
            "failed to fetch Homebrew tap formula '{name}' directly. \
                 The tap must publish API metadata at api/formula/{formula_name}.json; \
                 mise will not proxy to the brew CLI"
        )
    })
}

pub(super) fn tap_name(name: &str) -> Option<String> {
    let (owner, tap, _) = split_tap_name(name)?;
    if owner == "homebrew" && tap == "core" {
        None
    } else {
        Some(format!("{owner}/{tap}"))
    }
}

fn split_tap(name: &str) -> Option<(&str, &str)> {
    let mut parts = name.split('/');
    let owner = parts.next()?;
    let tap = parts.next()?;
    if parts.next().is_some() || owner.is_empty() || tap.is_empty() {
        None
    } else {
        Some((owner, tap))
    }
}

pub(super) fn split_tap_name(name: &str) -> Option<(&str, &str, &str)> {
    let mut parts = name.split('/');
    let owner = parts.next()?;
    let tap = parts.next()?;
    let formula = parts.next()?;
    if parts.next().is_some() || owner.is_empty() || tap.is_empty() || formula.is_empty() {
        None
    } else {
        Some((owner, tap, formula))
    }
}

fn tap_formula_api_url(
    owner: &str,
    tap: &str,
    formula: &str,
    tap_url: Option<&str>,
) -> Option<String> {
    let repo = tap_raw_base(owner, tap, tap_url)?;
    Some(format!("{repo}/api/formula/{formula}.json"))
}

pub(super) fn tap_raw_base(owner: &str, tap: &str, tap_url: Option<&str>) -> Option<String> {
    match tap_url {
        Some(url) => github_raw_base(url),
        None => Some(format!(
            "https://raw.githubusercontent.com/{owner}/homebrew-{tap}/HEAD"
        )),
    }
}

pub(super) fn github_raw_base(url: &str) -> Option<String> {
    let url = url.trim_end_matches(".git").trim_end_matches('/');
    let rest = url.strip_prefix("https://github.com/")?;
    let mut parts = rest.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if parts.next().is_some() || owner.is_empty() || repo.is_empty() {
        None
    } else {
        Some(format!(
            "https://raw.githubusercontent.com/{owner}/{repo}/HEAD"
        ))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    const TEST_SPKI_B64: &str = "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAwIWspWJVE51stHOwJaaOdZnrECZuI3LH+tzYwTrFaAkcPvC9XEZuV/HX/fV2pFXR6HTjwl1UyIihJ1zGpfaMmy+SYfImuDincSaNQGpJsuLb5xesPilOsP46eu5JwJ7zbzRpMyIntlqZEe6zoNRYnJXxna2hZnauDGrldtYDtj/MSaES0gNIVQemuzSG14L7JFuhK9Pkuh+xW9x5xcIBvk/38B4QC8yMOGB1WWryV+QREC/Zzm3jcSs4GoYVbpakGhVYYHvQvN+HRjDA1CcwGCgJPhNZ76RkOP47da0pSXh9ibrKUVQpLgyN7mC/1ypwELrL7FPmYwjsJqoViC4LYwIDAQAB";
    const TEST_PROTECTED: &str = "eyJhbGciOiJQUzUxMiIsImI2NCI6ZmFsc2UsImNyaXQiOlsiYjY0Il19";
    const TEST_SIGNATURE: &str = "jYKZ5fGzdy7jgMu4IlrlGJdGLb9bfcAVlqS0Bmfy1_6Ov0GMLTPgixYlIERMFa5OHptV8eeR3Fvd13e2_72ScxGpV_41cx83LQ-vFbZ19pT-_v6sMqb6oBOWeJTYomC5Tq5CKK7nchqTd6faEPTWl5qelL1yoFIwrq4fdTVL0KG5jvO0iomMkgLI6PhWtDKbLks1nsOwC_S0Pf0XttgDFcelqWe29IYs4GxbTfMBEsbhIKMyPO3V_lhJyGM9trqtMXR4Ks5io6LjbKSJxIT3Gy34CeTjVdrO42D1_hOQbf_E_8rVTIu_ht3oRrocjHkmXT7eA5TS7CPHikNOc0FE6A";
    const TEST_PAYLOAD: &str = r#"{"formulae":{"hello":{}}}"#;

    fn test_envelope(protected: &str, kid: &str, payload: &str) -> String {
        serde_json::json!({
            "payload": payload,
            "signatures": [{
                "protected": protected,
                "header": { "kid": kid },
                "signature": TEST_SIGNATURE
            }]
        })
        .to_string()
    }

    fn test_spki() -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode(TEST_SPKI_B64)
            .unwrap()
    }

    fn encoded_protected(value: Value) -> String {
        BASE64_URL_NO_PAD.encode(serde_json::to_vec(&value).unwrap())
    }

    #[test]
    fn internal_api_envelope_verifies_detached_payload() {
        let raw = test_envelope(TEST_PROTECTED, "homebrew-1", TEST_PAYLOAD);
        assert_eq!(
            verify_internal_api_envelope_with_key(&raw, &test_spki()).unwrap(),
            TEST_PAYLOAD
        );
    }

    #[test]
    fn internal_api_envelope_rejects_tampering_and_unknown_keys() {
        let tampered = test_envelope(TEST_PROTECTED, "homebrew-1", "{}");
        assert!(verify_internal_api_envelope_with_key(&tampered, &test_spki()).is_err());
        let unknown = test_envelope(TEST_PROTECTED, "other", TEST_PAYLOAD);
        assert!(verify_internal_api_envelope_with_key(&unknown, &test_spki()).is_err());
    }

    #[test]
    fn internal_api_envelope_rejects_unsupported_protected_headers() {
        for header in [
            json!({"alg":"RS512", "b64":false, "crit":["b64"]}),
            json!({"alg":"PS512", "b64":true, "crit":["b64"]}),
            json!({"alg":"PS512", "b64":false, "crit":["b64", "other"]}),
            json!({"alg":"PS512", "b64":false, "crit":[]}),
        ] {
            let raw = test_envelope(&encoded_protected(header), "homebrew-1", TEST_PAYLOAD);
            assert!(verify_internal_api_envelope_with_key(&raw, &test_spki()).is_err());
        }
    }

    #[test]
    fn internal_api_envelope_rejects_malformed_input() {
        assert!(verify_internal_api_envelope_with_key("not-json", &test_spki()).is_err());
        let malformed = test_envelope("***", "homebrew-1", TEST_PAYLOAD);
        assert!(verify_internal_api_envelope_with_key(&malformed, &test_spki()).is_err());
    }

    fn formula_with_policy(policy: Value) -> Formula {
        let mut value = json!({
            "name": "policy-test",
            "versions": { "stable": "1.0" }
        });
        value
            .as_object_mut()
            .unwrap()
            .extend(policy.as_object().unwrap().clone());
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn install_policy_defaults_are_safe() {
        let formula = formula_with_policy(json!({}));
        formula.validate_install_policy().unwrap();
        assert!(formula.conflicts_with().is_empty());
        assert!(formula.link_overwrite().is_empty());
    }

    #[test]
    fn disabled_formula_is_rejected_with_typed_context() {
        let formula = formula_with_policy(json!({
            "disabled": true,
            "disable_date": "2026-08-01",
            "disable_reason": "does_not_build",
            "disable_replacement_formula": "replacement"
        }));
        let error = formula.validate_install_policy().unwrap_err().to_string();
        assert!(error.contains("disabled by Homebrew"));
        assert!(error.contains("does_not_build"));
        assert!(error.contains("replacement formula replacement"));
    }

    #[test]
    fn unsupported_requirements_and_pour_predicates_are_rejected() {
        let requirement = formula_with_policy(json!({
            "requirements": [{
                "name": "xcode",
                "version": "26.0",
                "contexts": ["build"],
                "specs": ["stable"]
            }]
        }));
        let error = requirement
            .validate_install_policy()
            .unwrap_err()
            .to_string();
        assert!(error.contains("xcode[version=26.0,contexts=build,specs=stable]"));

        let predicate = formula_with_policy(json!({
            "pour_bottle_only_if": "default_prefix"
        }));
        assert!(
            predicate
                .validate_install_policy()
                .unwrap_err()
                .to_string()
                .contains("default_prefix")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unversioned_macos_requirement_accepts_current_stable_or_head_platform() {
        let formula = formula_with_policy(json!({
            "requirements": [{
                "name": "macos",
                "specs": ["stable", "head"]
            }]
        }));
        formula.validate_install_policy().unwrap();
    }

    #[test]
    fn conflict_and_link_policy_is_typed_but_link_overwrite_grants_no_authority() {
        let formula = formula_with_policy(json!({
            "conflicts_with": ["other", "third"],
            "conflicts_with_reasons": ["same binary", null],
            "link_overwrite": ["bin/tool", "share/tool/*"],
            "deprecated": true,
            "deprecation_reason": "versioned_formula"
        }));
        formula.validate_install_policy().unwrap();
        assert_eq!(formula.conflicts_with(), ["other", "third"]);
        assert_eq!(formula.conflict_reason("other"), Some("same binary"));
        assert_eq!(formula.conflict_reason("third"), None);
        assert_eq!(formula.link_overwrite(), ["bin/tool", "share/tool/*"]);
    }

    fn signed_index(value: Value) -> InternalFormulaIndex {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn signed_core_entry_is_the_authoritative_install_record() {
        let bottle_sha = "a".repeat(64);
        let source_sha = "b".repeat(64);
        let ruby_sha = "c".repeat(64);
        let index = signed_index(json!({
            "formulae": {
                "hello": {
                    "stable_version": "2.12.3",
                    "revision": 1,
                    "stable_url_args": ["https://trusted.example/hello.tar.gz"],
                    "stable_checksum": source_sha,
                    "stable_dependencies": ["runtime", {"builder": ":build"}],
                    "bottle_checksum": bottle_sha,
                    "bottle_cellar": ":any_skip_relocation",
                    "ruby_source_checksum": ruby_sha,
                    "keg_only_args": [":versioned_formula"],
                    "conflicts": [["other", {":because": "same binary"}]],
                    "link_overwrite_paths": ["bin/hello"],
                    "deprecate_args": {":because": ":unmaintained"}
                }
            },
            "formula_aliases": {"hi": "hello"},
            "formula_renames": {},
            "formula_tap_git_head": "signed-core-head",
            "metadata": {"bottle_tag": "x86_64_linux"}
        }));
        let canonical = canonical_internal_name(&index, "hi").unwrap();
        let formula = formula_from_internal(
            &index,
            &canonical,
            index.formulae.get(&canonical).unwrap(),
            "signed-index-url",
        )
        .unwrap();

        assert_eq!(formula.name, "hello");
        assert_eq!(formula.pkg_version().unwrap(), "2.12.3_1");
        assert!(formula.keg_only);
        assert_eq!(formula.dependencies, ["runtime"]);
        assert_eq!(formula.build_dependencies, ["builder"]);
        assert_eq!(formula.aliases, ["hi"]);
        assert_eq!(formula.tap_git_head.as_deref(), Some("signed-core-head"));
        assert_eq!(
            formula.ruby_source_path.as_deref(),
            Some("Formula/h/hello.rb")
        );
        let bottle = &formula.bottle["stable"].files["x86_64_linux"];
        assert_eq!(
            bottle.url,
            format!("https://ghcr.io/v2/homebrew/core/hello/blobs/sha256:{bottle_sha}")
        );
        assert_eq!(bottle.sha256, bottle_sha);
        assert_eq!(
            formula.stable_url().unwrap().url,
            "https://trusted.example/hello.tar.gz"
        );
        assert_eq!(
            formula.stable_url().unwrap().checksum.as_deref(),
            Some(source_sha.as_str())
        );
        assert_eq!(formula.conflicts_with(), ["other"]);
        assert_eq!(formula.conflict_reason("other"), Some("same binary"));
        assert_eq!(formula.link_overwrite(), ["bin/hello"]);
    }

    #[test]
    fn signed_core_schema_rejects_unknown_or_unsupported_install_shapes() {
        let unknown = json!({
            "stable_version": "1",
            "attacker_artifact_url": "https://evil.example/tool.tar.gz"
        });
        assert!(serde_json::from_value::<InternalFormula>(unknown).is_err());

        let index = signed_index(json!({
            "formulae": {
                "patched": {
                    "stable_version": "1",
                    "stable_patches": [{"url": "https://example.test/patch"}]
                }
            },
            "formula_tap_git_head": "signed-core-head",
            "metadata": {"bottle_tag": "all"}
        }));
        let err = formula_from_internal(
            &index,
            "patched",
            &index.formulae["patched"],
            "signed-index-url",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("signed source patches"));
    }

    #[test]
    fn signed_core_rejects_malformed_digest_and_non_archive_source_options() {
        let malformed: InternalFormula = serde_json::from_value(json!({
            "stable_version": "1",
            "bottle_checksum": "not-a-digest"
        }))
        .unwrap();
        let index = signed_index(json!({
            "formulae": {},
            "formula_tap_git_head": "signed-core-head",
            "metadata": {"bottle_tag": "all"}
        }));
        assert!(formula_from_internal(&index, "tool", &malformed, "signed-index-url").is_err());

        let vcs: InternalFormula = serde_json::from_value(json!({
            "stable_version": "1",
            "stable_url_args": ["https://example.test/tool.git", {":tag": "v1"}]
        }))
        .unwrap();
        let formula = formula_from_internal(&index, "tool", &vcs, "signed-index-url").unwrap();
        assert_eq!(
            formula.stable_url().unwrap().using.as_deref(),
            Some("signed URL options")
        );
    }
}
