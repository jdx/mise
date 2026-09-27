use crate::file_ext::{append_str_ext, file_ext, file_ext_is_empty};
use expr::{Context, Environment, Program, Value};
use eyre::{Result, eyre};
use indexmap::IndexSet;
use itertools::Itertools;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Deserializer};
use std::cmp::PartialEq;
use std::collections::{BTreeSet, HashMap};
use versions::Versioning;

/// Type of Aqua package
#[derive(
    Debug,
    Deserialize,
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
    Default,
    Copy,
    Clone,
    PartialEq,
    strum::Display,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum AquaPackageType {
    GithubArchive,
    GithubContent,
    #[default]
    GithubRelease,
    Http,
    GoInstall,
    GoBuild,
    Cargo,
}

/// Main Aqua package definition
///
/// rkyv archives parsed package data only. Runtime-only fields mirror serde's
/// skipped behavior with `rkyv::with::Skip`.
#[derive(Debug, Default, Deserialize, Archive, RkyvDeserialize, RkyvSerialize, Clone)]
#[rkyv(serialize_bounds(
    __S: rkyv::ser::Writer + rkyv::ser::Allocator,
    __S::Error: rkyv::rancor::Source,
))]
#[rkyv(deserialize_bounds(__D::Error: rkyv::rancor::Source))]
#[rkyv(bytecheck(
    bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source,
    )
))]
#[serde(default)]
pub struct AquaPackage {
    pub r#type: Option<AquaPackageType>,
    pub repo_owner: String,
    pub repo_name: String,
    pub name: Option<String>,
    #[serde(rename = "crate")]
    pub crate_name: Option<String>,
    pub asset: String,
    pub url: String,
    pub description: Option<String>,
    pub format: String,
    pub rosetta2: Option<bool>,
    pub windows_arm_emulation: Option<bool>,
    pub complete_windows_ext: Option<bool>,
    pub windows_ext: String,
    pub append_ext: Option<bool>,
    pub supported_envs: Vec<String>,
    pub files: Vec<AquaFile>,
    pub vars: Vec<AquaVar>,
    #[serde(default, deserialize_with = "deserialize_string_map")]
    pub replacements: HashMap<String, String>,
    pub version_prefix: Option<String>,
    version_filter: Option<String>,
    #[serde(skip)]
    #[rkyv(with = rkyv::with::Skip)]
    version_filter_expr: Option<Program>,
    pub version_source: Option<String>,
    pub cosign: Option<AquaCosign>,
    pub checksum: Option<AquaChecksum>,
    pub slsa_provenance: Option<AquaSlsaProvenance>,
    pub minisign: Option<AquaMinisign>,
    pub github_artifact_attestations: Option<AquaGithubArtifactAttestations>,
    format_overrides: Vec<AquaFormatOverride>,
    #[rkyv(omit_bounds)]
    overrides: Vec<AquaOverride>,
    version_constraint: String,
    #[rkyv(omit_bounds)]
    pub version_overrides: Vec<AquaPackage>,
    pub no_asset: Option<bool>,
    pub private: bool,
    pub error_message: Option<String>,
    pub path: Option<String>,
    #[serde(skip)]
    #[rkyv(with = rkyv::with::Skip)]
    var_values: HashMap<String, String>,
}

/// Override configuration for specific OS/architecture combinations
#[derive(Debug, Deserialize, Archive, RkyvDeserialize, RkyvSerialize, Clone)]
struct AquaOverride {
    #[serde(flatten)]
    pkg: AquaPackage,
    goos: Option<String>,
    goarch: Option<String>,
    #[serde(default)]
    envs: Vec<String>,
    #[serde(default)]
    variants: Vec<AquaVariant>,
}

/// Format override for a specific GOOS.
#[derive(Debug, Deserialize, Archive, RkyvDeserialize, RkyvSerialize, Clone)]
struct AquaFormatOverride {
    goos: String,
    format: String,
}

/// Runtime variant selector for an override.
#[derive(Debug, Deserialize, Archive, RkyvDeserialize, RkyvSerialize, Clone)]
struct AquaVariant {
    key: String,
    value: String,
}

#[derive(Debug, Clone, Copy, Default)]
struct AquaRuntime<'a> {
    libc: Option<&'a str>,
}

/// A platform-specific package override with selectors mise can evaluate at runtime.
#[derive(Debug, Clone)]
pub struct AquaPackagePlatformOverride {
    pub package: AquaPackage,
    pub goos: Option<String>,
    pub goarch: Option<String>,
    pub envs: Vec<String>,
    pub libc: Option<String>,
}

/// Variable definition for Aqua templates
#[derive(Debug, Deserialize, Archive, RkyvDeserialize, RkyvSerialize, Clone, Default)]
pub struct AquaVar {
    pub name: String,
    /// Aqua's schema allows arbitrary YAML defaults, but mise intentionally
    /// supports only string defaults to keep variable resolution simple.
    #[serde(default, deserialize_with = "deserialize_optional_scalar_string")]
    pub default: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// File definition within a package
#[derive(Debug, Deserialize, Archive, RkyvDeserialize, RkyvSerialize, Clone, Default)]
pub struct AquaFile {
    pub name: String,
    pub src: Option<String>,
    pub link: Option<String>,
    #[serde(default)]
    pub hard: bool,
}

/// Checksum algorithm options
#[derive(
    Debug,
    Deserialize,
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
    Clone,
    strum::AsRefStr,
    strum::Display,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum AquaChecksumAlgorithm {
    Sha1,
    Sha256,
    Sha512,
    Md5,
}

/// Type of checksum source
#[derive(Debug, Deserialize, Archive, RkyvDeserialize, RkyvSerialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AquaChecksumType {
    GithubRelease,
    Http,
}

/// Type of minisign source
#[derive(Debug, Deserialize, Archive, RkyvDeserialize, RkyvSerialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AquaMinisignType {
    GithubRelease,
    Http,
}

/// Cosign signature configuration
#[derive(Debug, Deserialize, Archive, RkyvDeserialize, RkyvSerialize, Clone)]
pub struct AquaCosignSignature {
    pub r#type: Option<String>,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub url: Option<String>,
    pub asset: Option<String>,
}

/// Cosign verification configuration
#[derive(Debug, Deserialize, Archive, RkyvDeserialize, RkyvSerialize, Clone)]
pub struct AquaCosign {
    pub enabled: Option<bool>,
    pub signature: Option<AquaCosignSignature>,
    pub key: Option<AquaCosignSignature>,
    pub certificate: Option<AquaCosignSignature>,
    pub bundle: Option<AquaCosignSignature>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    opts: Vec<String>,
}

/// SLSA provenance configuration
#[derive(Debug, Deserialize, Archive, RkyvDeserialize, RkyvSerialize, Clone)]
pub struct AquaSlsaProvenance {
    pub enabled: Option<bool>,
    pub r#type: Option<String>,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub url: Option<String>,
    pub asset: Option<String>,
    pub source_uri: Option<String>,
    pub source_tag: Option<String>,
    /// Exact URI SAN expected in the Fulcio signer certificate.
    pub signer_identity: Option<String>,
    /// Exact OIDC issuer expected in the Fulcio signer certificate.
    pub signer_issuer: Option<String>,
}

/// Minisign verification configuration
#[derive(Debug, Deserialize, Archive, RkyvDeserialize, RkyvSerialize, Clone)]
pub struct AquaMinisign {
    pub enabled: Option<bool>,
    pub r#type: Option<AquaMinisignType>,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub url: Option<String>,
    pub asset: Option<String>,
    pub public_key: Option<String>,
}

/// GitHub artifact attestations configuration
#[derive(Debug, Deserialize, Archive, RkyvDeserialize, RkyvSerialize, Clone)]
pub struct AquaGithubArtifactAttestations {
    pub enabled: Option<bool>,
    pub predicate_type: Option<String>,
    pub signer_workflow: Option<String>,
}

/// Checksum verification configuration
#[derive(Debug, Deserialize, Archive, RkyvDeserialize, RkyvSerialize, Clone)]
pub struct AquaChecksum {
    pub r#type: Option<AquaChecksumType>,
    pub algorithm: Option<AquaChecksumAlgorithm>,
    pub pattern: Option<AquaChecksumPattern>,
    pub cosign: Option<AquaCosign>,
    pub minisign: Option<AquaMinisign>,
    pub github_artifact_attestations: Option<AquaGithubArtifactAttestations>,
    #[serde(default, deserialize_with = "deserialize_optional_string_map")]
    replacements: Option<HashMap<String, String>>,
    file_format: Option<String>,
    enabled: Option<bool>,
    asset: Option<String>,
    url: Option<String>,
}

/// Checksum pattern configuration
#[derive(Debug, Deserialize, Archive, RkyvDeserialize, RkyvSerialize, Clone)]
pub struct AquaChecksumPattern {
    pub checksum: String,
    pub file: Option<String>,
}

/// Registry YAML file structure
#[derive(Debug, Deserialize)]
pub struct RegistryYaml {
    pub packages: Vec<RegistryPackageRow>,
}

/// Top-level package row in a merged aqua registry YAML file.
#[derive(Debug, Deserialize)]
pub struct RegistryPackageRow {
    #[serde(flatten)]
    pub package: AquaPackage,
    #[serde(default, deserialize_with = "deserialize_registry_aliases")]
    pub aliases: Vec<String>,
}

fn deserialize_registry_aliases<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let aliases = Option::<serde_yaml::Value>::deserialize(deserializer)?;
    Ok(aliases
        .and_then(|aliases| {
            aliases
                .as_sequence()
                .map(|aliases| aliases.iter().filter_map(registry_alias_name).collect())
        })
        .unwrap_or_default())
}

fn registry_alias_name(alias: &serde_yaml::Value) -> Option<String> {
    alias.get("name")?.as_str().map(str::to_string)
}

fn deserialize_optional_scalar_string<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_yaml::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_yaml::Value::Null) => Ok(None),
        Some(value) => yaml_scalar_to_string(value).map(Some).ok_or_else(|| {
            <D::Error as serde::de::Error>::custom("invalid type: expected a scalar string default")
        }),
    }
}

fn yaml_mapping_to_string_map<D: serde::de::Error>(
    value: serde_yaml::Value,
) -> std::result::Result<HashMap<String, String>, D> {
    let serde_yaml::Value::Mapping(mapping) = value else {
        return Err(D::custom("invalid type: expected a scalar string map"));
    };

    mapping
        .into_iter()
        .map(|(key, value)| {
            let key = yaml_scalar_to_string(key)
                .ok_or_else(|| D::custom("invalid type: expected a scalar string map key"))?;
            let value = yaml_scalar_to_string(value)
                .ok_or_else(|| D::custom("invalid type: expected a scalar string map value"))?;
            Ok((key, value))
        })
        .collect()
}

fn deserialize_string_map<'de, D>(
    deserializer: D,
) -> std::result::Result<HashMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_yaml::Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(HashMap::new());
    };
    yaml_mapping_to_string_map(value)
}

fn deserialize_optional_string_map<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<HashMap<String, String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let Some(value) = Option::<serde_yaml::Value>::deserialize(deserializer)? else {
        return Ok(None);
    };
    if matches!(value, serde_yaml::Value::Null) {
        return Ok(None);
    }
    yaml_mapping_to_string_map(value).map(Some)
}

fn yaml_scalar_to_string(value: serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(value) => Some(value),
        serde_yaml::Value::Bool(value) => Some(value.to_string()),
        serde_yaml::Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

mod file;
mod matchers;
mod merge;
mod package;
mod package_assets;
mod package_filter;
mod verification;

use matchers::{
    AQUA_ASSET_FORMATS, AquaRequirement, asset_without_ext, normalize_libc, split_version_prefix,
};
use merge::apply_override;

#[cfg(test)]
mod tests;
