use expr::{Context, Environment, Program, Value};
use eyre::{Result, eyre};
use indexmap::IndexSet;
use itertools::Itertools;
use serde_derive::Deserialize;
use std::cmp::PartialEq;
use std::collections::HashMap;
use versions::Versioning;

/// Type of Aqua package
#[derive(Debug, Deserialize, Default, Clone, PartialEq, strum::Display)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum AquaPackageType {
    GithubArchive,
    GithubContent,
    #[default]
    GithubRelease,
    Http,
    GoInstall,
    Cargo,
}

/// Main Aqua package definition
#[derive(Debug, Deserialize, Clone)]
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
    version_filter: Option<String>,
    #[serde(skip)]
    version_filter_expr: Option<Program>,
    pub version_source: Option<String>,
    pub checksum: Option<AquaChecksum>,
    pub slsa_provenance: Option<AquaSlsaProvenance>,
    pub minisign: Option<AquaMinisign>,
    pub github_artifact_attestations: Option<AquaGithubArtifactAttestations>,
    overrides: Vec<AquaOverride>,
    version_constraint: String,
    version_overrides: Vec<AquaPackage>,
    pub no_asset: bool,
    pub error_message: Option<String>,
    pub path: Option<String>,
}

/// Override configuration for specific OS/architecture combinations
#[derive(Debug, Deserialize, Clone)]
struct AquaOverride {
    #[serde(flatten)]
    pkg: AquaPackage,
    goos: Option<String>,
    goarch: Option<String>,
}

/// File definition within a package
#[derive(Debug, Deserialize, Clone)]
pub struct AquaFile {
    pub name: String,
    pub src: Option<String>,
}

/// Checksum algorithm options
#[derive(Debug, Deserialize, Clone, strum::AsRefStr, strum::Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum AquaChecksumAlgorithm {
    Sha1,
    Sha256,
    Sha512,
    Md5,
}

/// Type of checksum source
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AquaChecksumType {
    GithubRelease,
    Http,
}

/// Type of minisign source
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AquaMinisignType {
    GithubRelease,
    Http,
}

/// Cosign signature configuration
#[derive(Debug, Deserialize, Clone)]
pub struct AquaCosignSignature {
    pub r#type: Option<String>,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub url: Option<String>,
    pub asset: Option<String>,
}

/// Cosign verification configuration
#[derive(Debug, Deserialize, Clone)]
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
#[derive(Debug, Deserialize, Clone)]
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
#[derive(Debug, Deserialize, Clone)]
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
#[derive(Debug, Deserialize, Clone)]
pub struct AquaGithubArtifactAttestations {
    pub enabled: Option<bool>,
    pub signer_workflow: Option<String>,
}

/// Checksum verification configuration
#[derive(Debug, Deserialize, Clone)]
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
#[derive(Debug, Deserialize, Clone)]
pub struct AquaChecksumPattern {
    pub checksum: String,
    pub file: Option<String>,
}

/// Registry YAML file structure
#[derive(Debug, Deserialize)]
pub struct RegistryYaml {
    pub packages: Vec<AquaPackage>,
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
            replacements: HashMap::new(),
            version_prefix: None,
            version_filter: None,
            version_filter_expr: None,
            version_source: None,
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
        }
    }
}

impl AquaPackage {
    /// Apply version-specific configurations and overrides
    pub fn with_version(mut self, versions: &[&str], os: &str, arch: &str) -> AquaPackage {
        self = apply_override(self.clone(), self.version_override(versions));
        if let Some(avo) = self.overrides.clone().into_iter().find(|o| {
            if let (Some(goos), Some(goarch)) = (&o.goos, &o.goarch) {
                goos == os && goarch == arch
            } else if let Some(goos) = &o.goos {
                goos == os
            } else if let Some(goarch) = &o.goarch {
                goarch == arch
            } else {
                false
            }
        }) {
            self = apply_override(self, &avo.pkg)
        }
        self
    }

    fn version_override(&self, versions: &[&str]) -> &AquaPackage {
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
            .unwrap_or(self)
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
            if self.complete_windows_ext && self.format(v, os, arch)? == "raw" {
                strs.insert(format!("{asset}.exe"));
            } else {
                strs.insert(asset);
            }
            if arch == "arm64" {
                ctx.insert("Arch".to_string(), "amd64".to_string());
                strs.insert(self.parse_aqua_str(&self.asset, v, &ctx, os, arch)?);
                let asset = self.parse_aqua_str(&self.asset, v, &ctx, os, arch)?;
                if self.complete_windows_ext && self.format(v, os, arch)? == "raw" {
                    strs.insert(format!("{asset}.exe"));
                } else {
                    strs.insert(asset);
                }
            }
        }
        Ok(strs)
    }

    /// Get the URL for this package and version
    pub fn url(&self, v: &str, os: &str, arch: &str) -> Result<String> {
        let mut url = self.url.clone();
        if os == "windows" && self.complete_windows_ext && self.format(v, os, arch)? == "raw" {
            url.push_str(".exe");
        }
        self.parse_aqua_str(&url, v, &Default::default(), os, arch)
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
        ctx.extend(overrides.clone());

        crate::template::render(s, &ctx)
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
    /// Get the source path for this file within the package
    pub fn src(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<Option<String>> {
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

        self.src
            .as_ref()
            .map(|src| pkg.parse_aqua_str(src, v, &ctx, os, arch))
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

    pub fn asset(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<String> {
        pkg.parse_aqua_str(
            self.asset.as_ref().unwrap(),
            v,
            &Default::default(),
            os,
            arch,
        )
    }

    pub fn arg(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<String> {
        match self.r#type.as_deref().unwrap_or_default() {
            "github_release" => {
                let asset = self.asset(pkg, v, os, arch)?;
                let repo_owner = self
                    .repo_owner
                    .clone()
                    .unwrap_or_else(|| pkg.repo_owner.clone());
                let repo_name = self
                    .repo_name
                    .clone()
                    .unwrap_or_else(|| pkg.repo_name.clone());
                let repo = format!("{repo_owner}/{repo_name}");
                Ok(format!(
                    "https://github.com/{repo}/releases/download/{v}/{asset}"
                ))
            }
            "http" => self.url(pkg, v, os, arch),
            t => {
                log::warn!(
                    "unsupported cosign signature type for {}/{}: {t}",
                    pkg.repo_owner,
                    pkg.repo_name
                );
                Ok(String::new())
            }
        }
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
    pub fn asset(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<String> {
        pkg.parse_aqua_str(
            self.asset.as_ref().unwrap(),
            v,
            &Default::default(),
            os,
            arch,
        )
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
