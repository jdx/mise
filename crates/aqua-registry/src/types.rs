use deepmerge::prelude::*;
use indexmap::IndexMap;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize, Default, Clone, PartialEq, strum::Display, DeepMerge)]
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

#[derive(Debug, Deserialize, Clone, DeepMerge)]
#[serde(default)]
pub struct AquaPackage {
    pub r#type: AquaPackageType,
    pub repo_owner: String,
    pub repo_name: String,
    pub name: Option<String>,
    pub asset: String,
    pub url: String,
    pub description: Option<String>,
    pub format: String,
    pub rosetta2: bool,
    pub windows_arm_emulation: bool,
    pub complete_windows_ext: bool,
    pub supported_envs: Vec<String>,
    pub files: Vec<AquaFile>,
    pub replacements: HashMap<String, String>,
    pub version_prefix: Option<String>,
    pub version_filter: Option<String>,
    pub version_source: Option<String>,
    pub checksum: Option<AquaChecksum>,
    pub slsa_provenance: Option<AquaSlsaProvenance>,
    pub minisign: Option<AquaMinisign>,
    #[merge(skip)]
    pub overrides: Vec<AquaOverride>,
    pub version_constraint: String,
    #[merge(skip)]
    pub version_overrides: Vec<AquaPackage>,
    pub no_asset: bool,
    pub error_message: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, Clone, DeepMerge)]
pub struct AquaOverride {
    #[serde(flatten)]
    #[merge(skip)]
    pub pkg: AquaPackage,
    pub goos: Option<String>,
    pub goarch: Option<String>,
}

#[derive(Debug, Deserialize, Clone, DeepMerge)]
pub struct AquaFile {
    pub name: String,
    pub src: Option<String>,
}

#[derive(Debug, Deserialize, Clone, strum::AsRefStr, strum::Display, DeepMerge)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum AquaChecksumAlgorithm {
    Blake3,
    Sha1,
    Sha256,
    Sha512,
    Md5,
}

#[derive(Debug, Deserialize, Clone, DeepMerge)]
#[serde(rename_all = "snake_case")]
pub enum AquaChecksumType {
    GithubRelease,
    Http,
}

#[derive(Debug, Deserialize, Clone, DeepMerge)]
#[serde(rename_all = "snake_case")]
pub enum AquaMinisignType {
    GithubRelease,
    Http,
}

#[derive(Debug, Deserialize, Clone, DeepMerge)]
pub struct AquaCosignSignature {
    pub r#type: Option<String>,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub url: Option<String>,
    pub asset: Option<String>,
}

#[derive(Debug, Deserialize, Clone, DeepMerge)]
pub struct AquaCosign {
    pub enabled: Option<bool>,
    pub experimental: Option<bool>,
    pub signature: Option<AquaCosignSignature>,
    pub key: Option<AquaCosignSignature>,
    pub certificate: Option<AquaCosignSignature>,
    pub bundle: Option<AquaCosignSignature>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub opts: Vec<String>,
}

#[derive(Debug, Deserialize, Clone, DeepMerge)]
pub struct AquaSlsaProvenance {
    pub enabled: Option<bool>,
    pub r#type: Option<String>,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub url: Option<String>,
    pub asset: Option<String>,
    pub source_uri: Option<String>,
    pub source_tag: Option<String>,
}

#[derive(Debug, Deserialize, Clone, DeepMerge)]
pub struct AquaMinisign {
    pub enabled: Option<bool>,
    pub r#type: Option<AquaMinisignType>,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub url: Option<String>,
    pub asset: Option<String>,
    pub public_key: Option<String>,
}

#[derive(Debug, Deserialize, Clone, DeepMerge)]
pub struct AquaChecksum {
    pub r#type: Option<AquaChecksumType>,
    pub algorithm: Option<AquaChecksumAlgorithm>,
    pub pattern: Option<AquaChecksumPattern>,
    pub cosign: Option<AquaCosign>,
    pub file_format: Option<String>,
    pub enabled: Option<bool>,
    pub asset: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Deserialize, Clone, DeepMerge)]
pub struct AquaChecksumPattern {
    pub checksum: String,
    pub file: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RegistryYaml {
    pub packages: Vec<AquaPackage>,
    pub aliases: Option<Vec<AquaAlias>>,
}

#[derive(Debug, Deserialize, Clone, DeepMerge)]
pub struct AquaAlias {
    pub name: String,
    pub package: String,
}

impl Default for AquaPackage {
    fn default() -> Self {
        Self {
            r#type: AquaPackageType::GithubRelease,
            repo_owner: "".to_string(),
            repo_name: "".to_string(),
            name: None,
            asset: "".to_string(),
            url: "".to_string(),
            description: None,
            format: "".to_string(),
            rosetta2: false,
            windows_arm_emulation: false,
            complete_windows_ext: true,
            supported_envs: vec![],
            files: vec![],
            replacements: HashMap::new(),
            version_prefix: None,
            version_filter: None,
            version_source: None,
            checksum: None,
            slsa_provenance: None,
            minisign: None,
            overrides: vec![],
            version_constraint: "".to_string(),
            version_overrides: vec![],
            no_asset: false,
            error_message: None,
            path: None,
        }
    }
}

#[derive(Clone)]
pub struct RegistryIndex {
    pub packages_by_name: IndexMap<String, AquaPackage>,
    pub aliases: IndexMap<String, String>,
}

impl RegistryIndex {
    pub fn get(&self, id_or_alias: &str) -> Option<&AquaPackage> {
        // First check if it's a direct package name
        if let Some(pkg) = self.packages_by_name.get(id_or_alias) {
            return Some(pkg);
        }

        // Then check aliases
        if let Some(canonical_name) = self.aliases.get(id_or_alias) {
            return self.packages_by_name.get(canonical_name);
        }

        None
    }

    pub fn contains(&self, id_or_alias: &str) -> bool {
        self.get(id_or_alias).is_some()
    }
}
