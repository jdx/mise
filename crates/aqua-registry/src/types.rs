use expr::{Context, Environment, Program, Value};
use eyre::{Result, eyre};
use indexmap::IndexSet;
use itertools::Itertools;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Deserializer};
use std::cmp::PartialEq;
use std::collections::HashMap;
use versions::Versioning;

/// Type of Aqua package
#[derive(
    Debug,
    Deserialize,
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
    Default,
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
#[derive(Debug, Deserialize, Archive, RkyvDeserialize, RkyvSerialize, Clone)]
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
    #[rkyv(omit_bounds)]
    overrides: Vec<AquaOverride>,
    version_constraint: String,
    #[rkyv(omit_bounds)]
    pub version_overrides: Vec<AquaPackage>,
    pub no_asset: bool,
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
    variants: Vec<AquaVariant>,
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
    pub signer_workflow: Option<String>,
}

/// Checksum verification configuration
#[derive(Debug, Deserialize, Archive, RkyvDeserialize, RkyvSerialize, Clone)]
pub struct AquaChecksum {
    pub r#type: Option<AquaChecksumType>,
    pub algorithm: Option<AquaChecksumAlgorithm>,
    pub pattern: Option<AquaChecksumPattern>,
    pub cosign: Option<AquaCosign>,
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
    let serde_yaml::Value::Mapping(mapping) = value else {
        return Err(<D::Error as serde::de::Error>::custom(
            "invalid type: expected a string map",
        ));
    };

    mapping
        .into_iter()
        .map(|(key, value)| {
            let key = yaml_scalar_to_string(key).ok_or_else(|| {
                <D::Error as serde::de::Error>::custom(
                    "invalid type: expected a scalar string map key",
                )
            })?;
            let value = yaml_scalar_to_string(value).ok_or_else(|| {
                <D::Error as serde::de::Error>::custom(
                    "invalid type: expected a scalar string map value",
                )
            })?;
            Ok((key, value))
        })
        .collect()
}

fn yaml_scalar_to_string(value: serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(value) => Some(value),
        serde_yaml::Value::Bool(value) => Some(value.to_string()),
        serde_yaml::Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

impl Default for AquaPackage {
    fn default() -> Self {
        Self {
            r#type: AquaPackageType::GithubRelease,
            repo_owner: String::new(),
            repo_name: String::new(),
            name: None,
            asset: String::new(),
            url: String::new(),
            description: None,
            format: String::new(),
            rosetta2: false,
            windows_arm_emulation: false,
            complete_windows_ext: true,
            supported_envs: Vec::new(),
            files: Vec::new(),
            vars: Vec::new(),
            replacements: HashMap::new(),
            version_prefix: None,
            version_filter: None,
            version_filter_expr: None,
            version_source: None,
            cosign: None,
            checksum: None,
            slsa_provenance: None,
            minisign: None,
            github_artifact_attestations: None,
            overrides: Vec::new(),
            version_constraint: String::new(),
            version_overrides: Vec::new(),
            no_asset: false,
            error_message: None,
            path: None,
            var_values: HashMap::new(),
        }
    }
}

impl AquaPackage {
    /// Apply version-specific configurations and overrides
    pub fn with_version(self, versions: &[&str], os: &str, arch: &str) -> AquaPackage {
        self.with_version_runtime(versions, os, arch, AquaRuntime::default())
    }

    /// Apply version-specific configurations and overrides for a libc runtime variant.
    pub fn with_version_libc(
        self,
        versions: &[&str],
        os: &str,
        arch: &str,
        libc: Option<&str>,
    ) -> AquaPackage {
        self.with_version_runtime(versions, os, arch, AquaRuntime { libc })
    }

    fn with_version_runtime(
        mut self,
        versions: &[&str],
        os: &str,
        arch: &str,
        runtime: AquaRuntime<'_>,
    ) -> AquaPackage {
        if let Some(version_override) = self
            .version_override(versions)
            .filter(|version_override| !std::ptr::eq(*version_override, &self))
            .cloned()
        {
            self = apply_override(self, &version_override);
        }
        if let Some(pkg) = self
            .overrides
            .iter()
            .find(|o| o.matches(os, arch, runtime))
            .map(|o| o.pkg.clone())
        {
            self = apply_override(self, &pkg)
        }
        self
    }

    /// Apply user-provided variable values used by aqua `vars` templates.
    pub fn with_var_values(mut self, var_values: HashMap<String, String>) -> Result<AquaPackage> {
        self.var_values = var_values;
        self.validate_vars()?;
        Ok(self)
    }

    pub fn version_constraint_ok(&self, versions: &[&str]) -> bool {
        self.version_override(versions).is_some()
    }

    fn version_override(&self, versions: &[&str]) -> Option<&AquaPackage> {
        let expressions = versions
            .iter()
            .map(|v| (self.expr_parser(v), self.expr_ctx(v)))
            .collect_vec();
        vec![self]
            .into_iter()
            .chain(self.version_overrides.iter())
            .find(|vo| {
                if vo.version_constraint.is_empty() {
                    true
                } else {
                    expressions.iter().any(|(expr, ctx)| {
                        expr.eval(&vo.version_constraint, ctx)
                            .map_err(|e| {
                                log::debug!("error parsing {}: {e}", vo.version_constraint)
                            })
                            .unwrap_or(false.into())
                            .as_bool()
                            .unwrap()
                    })
                }
            })
    }

    /// Detect the format of an archive based on its filename
    fn detect_format(&self, asset_name: &str) -> &'static str {
        let formats = [
            "tar.br", "tar.bz2", "tar.gz", "tar.lz4", "tar.sz", "tar.xz", "tbr", "tbz", "tbz2",
            "tgz", "tlz4", "tsz", "txz", "tar.zst", "zip", "gz", "bz2", "lz4", "sz", "xz", "zst",
            "dmg", "pkg", "rar", "tar",
        ];

        for format in formats {
            if asset_name.ends_with(&format!(".{format}")) {
                return match format {
                    "tgz" => "tar.gz",
                    "txz" => "tar.xz",
                    "tbz2" | "tbz" => "tar.bz2",
                    _ => format,
                };
            }
        }
        "raw"
    }

    /// Get the format for this package and version
    pub fn format(&self, v: &str, os: &str, arch: &str) -> Result<&str> {
        if self.r#type == AquaPackageType::GithubArchive {
            return Ok("tar.gz");
        }
        let format = if self.format.is_empty() {
            let asset = if !self.asset.is_empty() {
                self.asset(v, os, arch)?
            } else if !self.url.is_empty() {
                self.url.to_string()
            } else {
                log::debug!("no asset or url for {}/{}", self.repo_owner, self.repo_name);
                String::new()
            };
            self.detect_format(&asset)
        } else {
            match self.format.as_str() {
                "tgz" => "tar.gz",
                "txz" => "tar.xz",
                "tbz2" | "tbz" => "tar.bz2",
                format => format,
            }
        };
        Ok(format)
    }

    /// Get the asset name for this package and version
    pub fn asset(&self, v: &str, os: &str, arch: &str) -> Result<String> {
        if self.asset.is_empty() && self.url.split("/").count() > "//".len() {
            let asset = self.url.rsplit("/").next().unwrap_or("");
            self.parse_aqua_str(asset, v, &Default::default(), os, arch)
        } else {
            self.parse_aqua_str(&self.asset, v, &Default::default(), os, arch)
        }
    }

    /// Get all possible asset strings for this package, version and platform
    pub fn asset_strs(&self, v: &str, os: &str, arch: &str) -> Result<IndexSet<String>> {
        let mut strs =
            IndexSet::from([self.parse_aqua_str(&self.asset, v, &Default::default(), os, arch)?]);
        if os == "darwin" {
            let mut ctx = HashMap::default();
            ctx.insert("Arch".to_string(), "universal".to_string());
            strs.insert(self.parse_aqua_str(&self.asset, v, &ctx, os, arch)?);
        } else if os == "windows" {
            let mut ctx = HashMap::default();
            let asset = self.parse_aqua_str(&self.asset, v, &ctx, os, arch)?;
            strs.insert(self.complete_windows_ext_to_asset(&asset, v, os, arch)?);
            if arch == "arm64" {
                ctx.insert("Arch".to_string(), "amd64".to_string());
                strs.insert(self.parse_aqua_str(&self.asset, v, &ctx, os, arch)?);
                let asset = self.parse_aqua_str(&self.asset, v, &ctx, os, arch)?;
                strs.insert(self.complete_windows_ext_to_asset(&asset, v, os, arch)?);
            }
        }
        Ok(strs)
    }

    /// Apply Windows .exe extension to an asset or URL string if appropriate.
    /// Mirrors upstream aqua's `completeWindowsExtToAsset` decision tree.
    fn complete_windows_ext_to_asset(
        &self,
        s: &str,
        v: &str,
        os: &str,
        arch: &str,
    ) -> Result<String> {
        if os != "windows" || s.ends_with(".exe") {
            return Ok(s.to_string());
        }
        if self.complete_windows_ext && self.format(v, os, arch)? == "raw" {
            return Ok(format!("{s}.exe"));
        }
        Ok(s.to_string())
    }

    /// Get the URL for this package and version
    pub fn url(&self, v: &str, os: &str, arch: &str) -> Result<String> {
        let url = self.parse_aqua_str(&self.url, v, &Default::default(), os, arch)?;
        self.complete_windows_ext_to_asset(&url, v, os, arch)
    }

    /// Parse an Aqua template string with variable substitution and platform info
    pub fn parse_aqua_str(
        &self,
        s: &str,
        v: &str,
        overrides: &HashMap<String, String>,
        os: &str,
        arch: &str,
    ) -> Result<String> {
        let mut actual_arch = arch;
        if os == "darwin" && arch == "arm64" && self.rosetta2 {
            actual_arch = "amd64";
        }
        if os == "windows" && arch == "arm64" && self.windows_arm_emulation {
            actual_arch = "amd64";
        }

        let replace = |s: &str| {
            self.replacements
                .get(s)
                .map(|s| s.to_string())
                .unwrap_or_else(|| s.to_string())
        };

        let semver = if let Some(prefix) = &self.version_prefix {
            v.strip_prefix(prefix).unwrap_or(v)
        } else {
            v
        };

        let mut ctx = HashMap::new();
        ctx.insert("Version".to_string(), replace(v));
        ctx.insert("SemVer".to_string(), replace(semver));
        ctx.insert("OS".to_string(), replace(os));
        ctx.insert("GOOS".to_string(), replace(os));
        ctx.insert("GOARCH".to_string(), replace(actual_arch));
        ctx.insert("Arch".to_string(), replace(actual_arch));
        ctx.insert("Format".to_string(), replace(&self.format));
        ctx.extend(self.vars_ctx()?);
        ctx.extend(overrides.clone());

        crate::template::render(s, &ctx)
    }

    fn vars_ctx(&self) -> Result<HashMap<String, String>> {
        self.validate_vars()?;
        let mut ctx = HashMap::new();
        for var in &self.vars {
            if let Some(value) = self.var_value(var)? {
                ctx.insert(format!("Vars.{}", var.name), value);
            }
        }
        Ok(ctx)
    }

    fn validate_vars(&self) -> Result<()> {
        for var in &self.vars {
            if var.name.is_empty() {
                return Err(eyre!("aqua var name is empty"));
            }
            if var.required && self.var_value(var)?.is_none() {
                return Err(eyre!("required aqua var not set: {}", var.name));
            }
        }
        Ok(())
    }

    fn var_value(&self, var: &AquaVar) -> Result<Option<String>> {
        if let Some(value) = self.var_values.get(&var.name) {
            return Ok(Some(value.clone()));
        }
        Ok(var.default.clone())
    }

    /// Set up version filter expression if configured
    pub fn setup_version_filter(&mut self) -> Result<()> {
        if let Some(version_filter) = &self.version_filter {
            self.version_filter_expr = Some(expr::compile(version_filter)?);
        }
        Ok(())
    }

    /// Check if a version passes the version filter
    pub fn version_filter_ok(&self, v: &str) -> Result<bool> {
        if let Some(filter) = self.version_filter_expr.clone() {
            if let Value::Bool(expr) = self.expr(v, filter)? {
                Ok(expr)
            } else {
                log::warn!(
                    "invalid response from version filter: {}",
                    self.version_filter.as_ref().unwrap()
                );
                Ok(true)
            }
        } else {
            Ok(true)
        }
    }

    fn expr(&self, v: &str, program: Program) -> Result<Value> {
        let expr = self.expr_parser(v);
        expr.run(program, &self.expr_ctx(v)).map_err(|e| eyre!(e))
    }

    fn expr_parser(&self, v: &str) -> Environment<'_> {
        let (_, v) = split_version_prefix(v);
        let ver = Versioning::new(v);
        let mut env = Environment::new();
        env.add_function("semver", move |c| {
            if c.args.len() != 1 {
                return Err("semver() takes exactly one argument".to_string().into());
            }
            let requirements = c.args[0]
                .as_string()
                .unwrap()
                .replace(' ', "")
                .split(',')
                .map(versions::Requirement::new)
                .collect::<Vec<_>>();
            if requirements.iter().any(|r| r.is_none()) {
                return Err("invalid semver requirement".to_string().into());
            }
            if let Some(ver) = &ver {
                Ok(requirements
                    .iter()
                    .all(|r| r.clone().is_some_and(|r| r.matches(ver)))
                    .into())
            } else {
                Err("invalid version".to_string().into())
            }
        });
        env
    }

    fn expr_ctx(&self, v: &str) -> Context {
        let mut ctx = Context::default();
        ctx.insert("Version", v);
        ctx
    }
}

impl AquaOverride {
    fn matches(&self, os: &str, arch: &str, runtime: AquaRuntime<'_>) -> bool {
        let platform_matches = if let (Some(goos), Some(goarch)) = (&self.goos, &self.goarch) {
            goos == os && goarch == arch
        } else if let Some(goos) = &self.goos {
            goos == os
        } else if let Some(goarch) = &self.goarch {
            goarch == arch
        } else {
            false
        };

        platform_matches
            && self
                .variants
                .iter()
                .all(|variant| variant.matches(os, runtime))
    }
}

impl AquaVariant {
    fn matches(&self, os: &str, runtime: AquaRuntime<'_>) -> bool {
        match self.key.as_str() {
            "libc" => match (
                normalize_libc(runtime.libc),
                normalize_libc(Some(&self.value)),
            ) {
                (Some(actual), Some(expected)) => os == "linux" && actual == expected,
                _ => false,
            },
            key => {
                log::debug!("unsupported aqua override variant key: {key}");
                false
            }
        }
    }
}

fn normalize_libc(libc: Option<&str>) -> Option<&str> {
    match libc? {
        "glibc" | "gnu" => Some("gnu"),
        "musl" => Some("musl"),
        _ => None,
    }
}
/// splits a version number into an optional prefix and the remaining version string
fn split_version_prefix(version: &str) -> (String, String) {
    version
        .char_indices()
        .find_map(|(i, c)| {
            if c.is_ascii_digit() {
                if i == 0 {
                    return Some(i);
                }
                // If the previous char is a delimiter or 'v', we found a split point.
                let prev_char = version.chars().nth(i - 1).unwrap();
                if ['-', '_', '/', '.', 'v', 'V'].contains(&prev_char) {
                    return Some(i);
                }
            }
            None
        })
        .map_or_else(
            || ("".into(), version.into()),
            |i| {
                let (prefix, version) = version.split_at(i);
                (prefix.into(), version.into())
            },
        )
}

impl AquaFile {
    fn template_ctx(
        &self,
        pkg: &AquaPackage,
        v: &str,
        os: &str,
        arch: &str,
    ) -> Result<HashMap<String, String>> {
        let asset = pkg.asset(v, os, arch)?;
        let asset = asset.strip_suffix(".tar.gz").unwrap_or(&asset);
        let asset = asset.strip_suffix(".tar.xz").unwrap_or(asset);
        let asset = asset.strip_suffix(".tar.bz2").unwrap_or(asset);
        let asset = asset.strip_suffix(".gz").unwrap_or(asset);
        let asset = asset.strip_suffix(".xz").unwrap_or(asset);
        let asset = asset.strip_suffix(".bz2").unwrap_or(asset);
        let asset = asset.strip_suffix(".zip").unwrap_or(asset);
        let asset = asset.strip_suffix(".tar").unwrap_or(asset);
        let asset = asset.strip_suffix(".tgz").unwrap_or(asset);
        let asset = asset.strip_suffix(".txz").unwrap_or(asset);
        let asset = asset.strip_suffix(".tbz2").unwrap_or(asset);
        let asset = asset.strip_suffix(".tbz").unwrap_or(asset);

        let mut ctx = HashMap::new();
        ctx.insert("AssetWithoutExt".to_string(), asset.to_string());
        ctx.insert("FileName".to_string(), self.name.to_string());
        Ok(ctx)
    }

    /// Get the source path for this file within the package
    pub fn src(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<Option<String>> {
        let ctx = self.template_ctx(pkg, v, os, arch)?;
        self.src
            .as_ref()
            .map(|src| pkg.parse_aqua_str(src, v, &ctx, os, arch))
            .transpose()
    }

    /// Get the link path for this file.
    pub fn link(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<Option<String>> {
        let ctx = self.template_ctx(pkg, v, os, arch)?;
        self.link
            .as_ref()
            .map(|link| pkg.parse_aqua_str(link, v, &ctx, os, arch))
            .transpose()
    }
}

fn apply_override(mut orig: AquaPackage, avo: &AquaPackage) -> AquaPackage {
    if avo.r#type != AquaPackageType::GithubRelease {
        orig.r#type = avo.r#type.clone();
    }
    if !avo.repo_owner.is_empty() {
        orig.repo_owner = avo.repo_owner.clone();
    }
    if !avo.repo_name.is_empty() {
        orig.repo_name = avo.repo_name.clone();
    }
    if !avo.asset.is_empty() {
        orig.asset = avo.asset.clone();
    }
    if !avo.url.is_empty() {
        orig.url = avo.url.clone();
    }
    if !avo.format.is_empty() {
        orig.format = avo.format.clone();
    }
    if avo.rosetta2 {
        orig.rosetta2 = true;
    }
    if avo.windows_arm_emulation {
        orig.windows_arm_emulation = true;
    }
    if !avo.complete_windows_ext {
        orig.complete_windows_ext = false;
    }
    if !avo.supported_envs.is_empty() {
        orig.supported_envs = avo.supported_envs.clone();
    }
    if !avo.files.is_empty() {
        orig.files = avo.files.clone();
    }
    if !avo.vars.is_empty() {
        orig.vars = avo.vars.clone();
    }
    orig.replacements.extend(avo.replacements.clone());
    if let Some(avo_version_prefix) = avo.version_prefix.clone() {
        orig.version_prefix = Some(avo_version_prefix);
    }
    if !avo.overrides.is_empty() {
        orig.overrides = avo.overrides.clone();
    }

    if let Some(avo_checksum) = avo.checksum.clone() {
        match &mut orig.checksum {
            Some(checksum) => {
                checksum.merge(avo_checksum.clone());
            }
            None => {
                orig.checksum = Some(avo_checksum.clone());
            }
        }
    }

    if let Some(avo_cosign) = &avo.cosign {
        match &mut orig.cosign {
            Some(cosign) => {
                cosign.merge(avo_cosign.clone());
            }
            None => {
                orig.cosign = Some(avo_cosign.clone());
            }
        }
    }

    if let Some(avo_slsa_provenance) = avo.slsa_provenance.clone() {
        match &mut orig.slsa_provenance {
            Some(slsa_provenance) => {
                slsa_provenance.merge(avo_slsa_provenance.clone());
            }
            None => {
                orig.slsa_provenance = Some(avo_slsa_provenance.clone());
            }
        }
    }

    if let Some(avo_minisign) = avo.minisign.clone() {
        match &mut orig.minisign {
            Some(minisign) => {
                minisign.merge(avo_minisign.clone());
            }
            None => {
                orig.minisign = Some(avo_minisign.clone());
            }
        }
    }

    if let Some(avo_attestations) = avo.github_artifact_attestations.clone() {
        match &mut orig.github_artifact_attestations {
            Some(orig_attestations) => {
                orig_attestations.merge(avo_attestations.clone());
            }
            None => {
                orig.github_artifact_attestations = Some(avo_attestations.clone());
            }
        }
    }

    if avo.no_asset {
        orig.no_asset = true;
    }
    if let Some(error_message) = avo.error_message.clone() {
        orig.error_message = Some(error_message);
    }
    if let Some(path) = avo.path.clone() {
        orig.path = Some(path);
    }
    orig
}

// Implementation of merge methods for various types
impl AquaChecksum {
    pub fn _type(&self) -> &AquaChecksumType {
        self.r#type.as_ref().unwrap()
    }

    pub fn algorithm(&self) -> &AquaChecksumAlgorithm {
        self.algorithm.as_ref().unwrap()
    }

    pub fn asset_strs(
        &self,
        pkg: &AquaPackage,
        v: &str,
        os: &str,
        arch: &str,
    ) -> Result<IndexSet<String>> {
        let mut asset_strs = IndexSet::new();
        for asset in pkg.asset_strs(v, os, arch)? {
            let checksum_asset = self.asset.as_ref().unwrap();
            let mut ctx = HashMap::new();
            ctx.insert("Asset".to_string(), asset.to_string());
            asset_strs.insert(pkg.parse_aqua_str(checksum_asset, v, &ctx, os, arch)?);
        }
        Ok(asset_strs)
    }

    pub fn pattern(&self) -> &AquaChecksumPattern {
        self.pattern.as_ref().unwrap()
    }

    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    pub fn file_format(&self) -> &str {
        self.file_format.as_deref().unwrap_or("raw")
    }

    pub fn url(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<String> {
        pkg.parse_aqua_str(self.url.as_ref().unwrap(), v, &Default::default(), os, arch)
    }

    fn merge(&mut self, other: Self) {
        if let Some(r#type) = other.r#type {
            self.r#type = Some(r#type);
        }
        if let Some(algorithm) = other.algorithm {
            self.algorithm = Some(algorithm);
        }
        if let Some(pattern) = other.pattern {
            self.pattern = Some(pattern);
        }
        if let Some(enabled) = other.enabled {
            self.enabled = Some(enabled);
        }
        if let Some(asset) = other.asset {
            self.asset = Some(asset);
        }
        if let Some(url) = other.url {
            self.url = Some(url);
        }
        if let Some(file_format) = other.file_format {
            self.file_format = Some(file_format);
        }
        if let Some(cosign) = other.cosign {
            if self.cosign.is_none() {
                self.cosign = Some(cosign.clone());
            }
            self.cosign.as_mut().unwrap().merge(cosign);
        }
    }
}

impl AquaCosign {
    // TODO: This does not support `{{.Asset}}`.
    pub fn opts(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<Vec<String>> {
        self.opts
            .iter()
            .map(|opt| pkg.parse_aqua_str(opt, v, &Default::default(), os, arch))
            .collect()
    }

    fn merge(&mut self, other: Self) {
        if let Some(enabled) = other.enabled {
            self.enabled = Some(enabled);
        }
        if let Some(signature) = other.signature.clone() {
            if self.signature.is_none() {
                self.signature = Some(signature.clone());
            }
            self.signature.as_mut().unwrap().merge(signature);
        }
        if let Some(key) = other.key.clone() {
            if self.key.is_none() {
                self.key = Some(key.clone());
            }
            self.key.as_mut().unwrap().merge(key);
        }
        if let Some(certificate) = other.certificate.clone() {
            if self.certificate.is_none() {
                self.certificate = Some(certificate.clone());
            }
            self.certificate.as_mut().unwrap().merge(certificate);
        }
        if let Some(bundle) = other.bundle.clone() {
            if self.bundle.is_none() {
                self.bundle = Some(bundle.clone());
            }
            self.bundle.as_mut().unwrap().merge(bundle);
        }
        if !other.opts.is_empty() {
            self.opts = other.opts.clone();
        }
    }
}

impl AquaCosignSignature {
    pub fn url(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<String> {
        pkg.parse_aqua_str(self.url.as_ref().unwrap(), v, &Default::default(), os, arch)
    }

    pub fn asset_strs(
        &self,
        pkg: &AquaPackage,
        v: &str,
        os: &str,
        arch: &str,
    ) -> Result<IndexSet<String>> {
        let mut asset_strs = IndexSet::new();
        if let Some(cosign_asset_template) = &self.asset {
            for asset in pkg.asset_strs(v, os, arch)? {
                let mut ctx = HashMap::new();
                ctx.insert("Asset".to_string(), asset.to_string());
                asset_strs.insert(pkg.parse_aqua_str(cosign_asset_template, v, &ctx, os, arch)?);
            }
        }
        Ok(asset_strs)
    }

    fn merge(&mut self, other: Self) {
        if let Some(r#type) = other.r#type {
            self.r#type = Some(r#type);
        }
        if let Some(repo_owner) = other.repo_owner {
            self.repo_owner = Some(repo_owner);
        }
        if let Some(repo_name) = other.repo_name {
            self.repo_name = Some(repo_name);
        }
        if let Some(url) = other.url {
            self.url = Some(url);
        }
        if let Some(asset) = other.asset {
            self.asset = Some(asset);
        }
    }
}

impl AquaSlsaProvenance {
    pub fn asset_strs(
        &self,
        pkg: &AquaPackage,
        v: &str,
        os: &str,
        arch: &str,
    ) -> Result<IndexSet<String>> {
        let mut asset_strs = IndexSet::new();
        if let Some(slsa_asset_template) = &self.asset {
            for asset in pkg.asset_strs(v, os, arch)? {
                let mut ctx = HashMap::new();
                ctx.insert("Asset".to_string(), asset.to_string());
                asset_strs.insert(pkg.parse_aqua_str(slsa_asset_template, v, &ctx, os, arch)?);
            }
        }
        Ok(asset_strs)
    }

    pub fn url(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<String> {
        pkg.parse_aqua_str(self.url.as_ref().unwrap(), v, &Default::default(), os, arch)
    }

    fn merge(&mut self, other: Self) {
        if let Some(enabled) = other.enabled {
            self.enabled = Some(enabled);
        }
        if let Some(r#type) = other.r#type {
            self.r#type = Some(r#type);
        }
        if let Some(repo_owner) = other.repo_owner {
            self.repo_owner = Some(repo_owner);
        }
        if let Some(repo_name) = other.repo_name {
            self.repo_name = Some(repo_name);
        }
        if let Some(url) = other.url {
            self.url = Some(url);
        }
        if let Some(asset) = other.asset {
            self.asset = Some(asset);
        }
        if let Some(source_uri) = other.source_uri {
            self.source_uri = Some(source_uri);
        }
        if let Some(source_tag) = other.source_tag {
            self.source_tag = Some(source_tag);
        }
    }
}

impl AquaMinisign {
    pub fn _type(&self) -> &AquaMinisignType {
        self.r#type.as_ref().unwrap()
    }

    pub fn url(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<String> {
        pkg.parse_aqua_str(self.url.as_ref().unwrap(), v, &Default::default(), os, arch)
    }

    pub fn asset(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<String> {
        pkg.parse_aqua_str(
            self.asset.as_ref().unwrap(),
            v,
            &Default::default(),
            os,
            arch,
        )
    }

    pub fn public_key(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<String> {
        pkg.parse_aqua_str(
            self.public_key.as_ref().unwrap(),
            v,
            &Default::default(),
            os,
            arch,
        )
    }

    fn merge(&mut self, other: Self) {
        if let Some(enabled) = other.enabled {
            self.enabled = Some(enabled);
        }
        if let Some(r#type) = other.r#type {
            self.r#type = Some(r#type);
        }
        if let Some(repo_owner) = other.repo_owner {
            self.repo_owner = Some(repo_owner);
        }
        if let Some(repo_name) = other.repo_name {
            self.repo_name = Some(repo_name);
        }
        if let Some(url) = other.url {
            self.url = Some(url);
        }
        if let Some(asset) = other.asset {
            self.asset = Some(asset);
        }
        if let Some(public_key) = other.public_key {
            self.public_key = Some(public_key);
        }
    }
}

impl AquaGithubArtifactAttestations {
    fn merge(&mut self, other: Self) {
        if let Some(enabled) = other.enabled {
            self.enabled = Some(enabled);
        }
        if let Some(signer_workflow) = other.signer_workflow {
            self.signer_workflow = Some(signer_workflow);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_str(value: &str) -> Option<String> {
        Some(value.to_string())
    }

    fn first_registry_package(yml: &str) -> AquaPackage {
        serde_yaml::from_str::<RegistryYaml>(yml)
            .unwrap()
            .packages
            .into_iter()
            .next()
            .unwrap()
            .package
    }

    #[test]
    fn test_registry_package_row_aliases_are_top_level_only() {
        let yml = r#"
packages:
  - name: example/canonical
    aliases:
      - name: example/alias
      - name: 123
      - other: ignored
    unsupported_field: ignored
    version_overrides:
      - aliases:
          - name: example/nested-alias
"#;
        let registry = serde_yaml::from_str::<RegistryYaml>(yml).unwrap();
        let row = registry.packages.into_iter().next().unwrap();

        assert_eq!(row.package.name.as_deref(), Some("example/canonical"));
        assert_eq!(row.aliases, vec!["example/alias"]);
        assert_eq!(row.package.version_overrides.len(), 1);
    }

    #[test]
    fn test_registry_package_row_preserves_yaml_scalar_coercions() {
        let yml = r#"
packages:
  - replacements:
      386: i686
    vars:
      - name: enabled
        default: true
"#;
        let pkg = first_registry_package(yml);

        assert_eq!(pkg.replacements.get("386"), Some(&"i686".to_string()));
        assert_eq!(pkg.vars[0].default.as_deref(), Some("true"));
    }

    #[test]
    fn test_aqua_file_src_gradle() {
        // Test the gradle package src template: {{.AssetWithoutExt | trimSuffix "-bin"}}/bin/gradle
        let pkg = AquaPackage {
            repo_owner: "gradle".to_string(),
            repo_name: "gradle-distributions".to_string(),
            asset: "gradle-{{trimV .Version}}-bin.zip".to_string(),
            ..Default::default()
        };
        let file = AquaFile {
            name: "gradle".to_string(),
            src: Some("{{.AssetWithoutExt | trimSuffix \"-bin\"}}/bin/gradle".to_string()),
            ..Default::default()
        };

        let result = file.src(&pkg, "8.14.3", "darwin", "arm64").unwrap();
        assert_eq!(result, Some("gradle-8.14.3/bin/gradle".to_string()));
    }

    #[test]
    fn test_aqua_file_src_empty_asset_produces_absolute_path() {
        // When a linked version name like "brew" matches a wrong version_override
        // that has no asset field, the package ends up with an empty asset.
        // The src template "{{.AssetWithoutExt}}/name" then renders to "/name"
        // which is an absolute path — this caused a StripPrefixError panic
        // in the aqua backend's list_bin_paths.
        let pkg = AquaPackage {
            repo_owner: "mozilla".to_string(),
            repo_name: "sccache".to_string(),
            asset: String::new(),
            ..Default::default()
        };
        let file = AquaFile {
            name: "sccache".to_string(),
            src: Some("{{.AssetWithoutExt}}/sccache".to_string()),
            ..Default::default()
        };

        let result = file.src(&pkg, "brew", "darwin", "arm64").unwrap();
        assert_eq!(result, Some("/sccache".to_string()));
    }

    #[test]
    fn test_version_override_non_version_string_matches_semver() {
        // Non-version strings like "brew" are parsed as valid General versions
        // by the versions crate, and can match semver constraints unexpectedly.
        // This documents the root cause of the linked-version panic.
        let pkg = AquaPackage {
            version_constraint: "false".to_string(),
            version_overrides: vec![
                AquaPackage {
                    version_constraint: "semver(\"<= 0.2.13\")".to_string(),
                    error_message: Some("too old".to_string()),
                    ..Default::default()
                },
                AquaPackage {
                    version_constraint: "true".to_string(),
                    asset: "tool-{{.Version}}.tar.gz".to_string(),
                    format: "tar.gz".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let result = pkg.version_override(&["brew"]).unwrap();
        // "brew" matches semver("<= 0.2.13") instead of "true",
        // because Versioning::new("brew") parses as General(Alphanum("brew"))
        // which sorts before numeric versions.
        assert!(result.error_message.is_some());
        assert!(result.asset.is_empty());
    }

    #[test]
    fn test_url_no_double_exe_extension() {
        // When a Windows override URL already ends in .exe, complete_windows_ext
        // should not append another .exe (which would produce .exe.exe).
        let pkg = AquaPackage {
            url: "https://example.com/tool/{{.Version}}/tool.exe".to_string(),
            format: "raw".to_string(),
            complete_windows_ext: true,
            ..Default::default()
        };

        let url = pkg.url("1.0.0", "windows", "amd64").unwrap();
        assert!(
            !url.ends_with(".exe.exe"),
            "URL should not have double .exe extension, got: {url}"
        );
        assert!(url.ends_with(".exe"));
    }

    #[test]
    fn test_url_adds_exe_when_missing() {
        // When a Windows URL does not end in .exe and format is raw,
        // complete_windows_ext should append .exe.
        let pkg = AquaPackage {
            url: "https://example.com/tool/{{.Version}}/tool".to_string(),
            format: "raw".to_string(),
            complete_windows_ext: true,
            ..Default::default()
        };

        let url = pkg.url("1.0.0", "windows", "amd64").unwrap();
        assert!(
            url.ends_with(".exe"),
            "URL should end with .exe, got: {url}"
        );
    }

    #[test]
    fn test_asset_strs_no_double_exe_extension() {
        // asset_strs should also not double .exe when asset already ends in .exe.
        let pkg = AquaPackage {
            asset: "tool.exe".to_string(),
            format: "raw".to_string(),
            complete_windows_ext: true,
            ..Default::default()
        };

        let strs = pkg.asset_strs("1.0.0", "windows", "amd64").unwrap();
        for s in &strs {
            assert!(
                !s.ends_with(".exe.exe"),
                "Asset string should not have double .exe, got: {s}"
            );
        }
    }

    #[test]
    fn test_aqua_file_link_template() {
        let pkg = AquaPackage {
            repo_owner: "example".to_string(),
            repo_name: "tool".to_string(),
            asset: "tool-{{.Version}}.tar.gz".to_string(),
            ..Default::default()
        };
        let file = AquaFile {
            name: "tool".to_string(),
            link: Some("{{.FileName}}-alias".to_string()),
            ..Default::default()
        };

        let result = file.link(&pkg, "1.0.0", "linux", "amd64").unwrap();
        assert_eq!(result, Some("tool-alias".to_string()));
    }

    #[test]
    fn test_vars_default_value() {
        let pkg = AquaPackage {
            asset: "tool-{{.Vars.channel}}-{{.Version}}.tar.gz".to_string(),
            vars: vec![AquaVar {
                name: "channel".to_string(),
                default: default_str("stable"),
                required: false,
            }],
            ..Default::default()
        };
        let asset = pkg.asset("1.0.0", "linux", "amd64").unwrap();
        assert_eq!(asset, "tool-stable-1.0.0.tar.gz");
    }

    #[test]
    fn test_vars_override_value() {
        let mut var_values = HashMap::new();
        var_values.insert("channel".to_string(), "beta".to_string());
        let pkg = AquaPackage {
            asset: "tool-{{.Vars.channel}}-{{.Version}}.tar.gz".to_string(),
            vars: vec![AquaVar {
                name: "channel".to_string(),
                default: default_str("stable"),
                required: false,
            }],
            ..Default::default()
        }
        .with_var_values(var_values)
        .unwrap();
        let asset = pkg.asset("1.0.0", "linux", "amd64").unwrap();
        assert_eq!(asset, "tool-beta-1.0.0.tar.gz");
    }

    #[test]
    fn test_vars_default_unquoted_yaml_string() {
        let yml = r#"
packages:
  - asset: tool-{{.Vars.channel}}-{{.Version}}.tar.gz
    vars:
      - name: channel
        default: stable
"#;
        let pkg = first_registry_package(yml);
        let asset = pkg.asset("1.0.0", "linux", "amd64").unwrap();
        assert_eq!(asset, "tool-stable-1.0.0.tar.gz");
    }

    #[test]
    fn test_vars_scalar_defaults_deserialize_as_strings() {
        for (yaml_default, expected) in [("true", "true"), ("123", "123")] {
            let yml = format!(
                r#"
packages:
  - vars:
      - name: channel
        default: {yaml_default}
"#
            );
            let pkg = first_registry_package(&yml);
            assert_eq!(pkg.vars[0].default.as_deref(), Some(expected));
        }
    }

    #[test]
    fn test_vars_null_default_deserializes_as_none() {
        let yml = r#"
packages:
  - vars:
      - name: channel
        default: null
"#;
        let pkg = first_registry_package(yml);
        assert_eq!(pkg.vars[0].default, None);
    }

    #[test]
    fn test_vars_sequence_and_mapping_defaults_fail_yaml_parse() {
        for yaml_default in ["[stable, beta]", "{channel: stable}"] {
            let yml = format!(
                r#"
packages:
  - vars:
      - name: channel
        default: {yaml_default}
"#
            );
            let err = serde_yaml::from_str::<RegistryYaml>(&yml).unwrap_err();
            assert!(
                err.to_string().contains("invalid type"),
                "unexpected error for {yaml_default}: {err}"
            );
        }
    }

    #[test]
    fn test_vars_required_missing() {
        let pkg = AquaPackage {
            asset: "tool-{{.Vars.channel}}-{{.Version}}.tar.gz".to_string(),
            vars: vec![AquaVar {
                name: "channel".to_string(),
                default: None,
                required: true,
            }],
            ..Default::default()
        };
        let err = pkg.asset("1.0.0", "linux", "amd64").unwrap_err();
        assert!(
            err.to_string()
                .contains("required aqua var not set: channel"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_vars_required_missing_with_var_values() {
        let pkg = AquaPackage {
            vars: vec![AquaVar {
                name: "go_version".to_string(),
                default: None,
                required: true,
            }],
            ..Default::default()
        };
        let err = pkg.with_var_values(HashMap::new()).unwrap_err();
        assert!(
            err.to_string()
                .contains("required aqua var not set: go_version"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_vars_empty_name() {
        let pkg = AquaPackage {
            vars: vec![AquaVar {
                name: String::new(),
                default: None,
                required: false,
            }],
            ..Default::default()
        };
        let err = pkg.asset("1.0.0", "linux", "amd64").unwrap_err();
        assert!(
            err.to_string().contains("aqua var name is empty"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_top_level_cosign_is_deserialized() {
        let yml = r#"
packages:
  - cosign:
      bundle:
        type: github_release
        asset: "{{.Asset}}.sigstore.json"
"#;
        let pkg = first_registry_package(yml);
        assert!(pkg.cosign.is_some());
        assert!(pkg.checksum.is_none());
    }

    #[test]
    fn test_top_level_cosign_is_merged_from_version_override() {
        let yml = r#"
packages:
  - asset: tool-{{.Version}}-{{.OS}}-{{.Arch}}
    format: raw
    cosign:
      bundle:
        type: github_release
        asset: "{{.Asset}}.sigstore.json"
    version_constraint: "false"
    version_overrides:
      - version_constraint: "true"
        cosign:
          key:
            type: github_release
            asset: cosign.pub
"#;
        let pkg = first_registry_package(yml).with_version(&["v1.0.0"], "linux", "amd64");
        let cosign = pkg.cosign.unwrap();
        assert!(cosign.bundle.is_some());
        assert!(cosign.key.is_some());
    }

    #[test]
    fn test_override_variants_match_linux_libc() {
        let yml = r#"
packages:
  - url: https://example.com/tool-{{.Version}}-{{.OS}}-{{.Arch}}-gnu
    format: raw
    overrides:
      - goos: linux
        variants:
          - key: libc
            value: gnu
      - goos: linux
        url: https://example.com/tool-{{.Version}}-{{.OS}}-{{.Arch}}-musl
        variants:
          - key: libc
            value: musl
"#;
        let pkg = first_registry_package(yml);

        let gnu = pkg
            .clone()
            .with_version_libc(&["1.0.0"], "linux", "amd64", Some("gnu"));
        let musl = pkg.with_version_libc(&["1.0.0"], "linux", "amd64", Some("musl"));

        assert_eq!(
            gnu.url("1.0.0", "linux", "amd64").unwrap(),
            "https://example.com/tool-1.0.0-linux-amd64-gnu"
        );
        assert_eq!(
            musl.url("1.0.0", "linux", "amd64").unwrap(),
            "https://example.com/tool-1.0.0-linux-amd64-musl"
        );
    }

    #[test]
    fn test_override_variants_skip_unknown_keys() {
        let yml = r#"
packages:
  - url: https://example.com/tool-default
    format: raw
    overrides:
      - goos: linux
        url: https://example.com/tool-avx2
        variants:
          - key: cpu
            value: avx2
      - goos: linux
        url: https://example.com/tool-musl
        variants:
          - key: libc
            value: musl
"#;
        let pkg = first_registry_package(yml).with_version_libc(
            &["1.0.0"],
            "linux",
            "amd64",
            Some("musl"),
        );

        assert_eq!(
            pkg.url("1.0.0", "linux", "amd64").unwrap(),
            "https://example.com/tool-musl"
        );
    }

    #[test]
    fn test_override_variants_do_not_match_without_runtime_libc() {
        let yml = r#"
packages:
  - url: https://example.com/tool-default
    format: raw
    overrides:
      - goos: linux
        url: https://example.com/tool-musl
        variants:
          - key: libc
            value: musl
"#;
        let pkg = first_registry_package(yml).with_version(&["1.0.0"], "linux", "amd64");

        assert_eq!(
            pkg.url("1.0.0", "linux", "amd64").unwrap(),
            "https://example.com/tool-default"
        );
    }
}
