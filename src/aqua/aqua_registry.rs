use crate::aqua::aqua_template;
use crate::backend::aqua;
use crate::backend::aqua::{arch, os};
use crate::config::SETTINGS;
use crate::duration::DAILY;
use crate::git::Git;
use crate::{dirs, file, hashmap, http};
use eyre::{ContextCompat, Result};
use indexmap::IndexSet;
use itertools::Itertools;
use once_cell::sync::Lazy;
use serde_derive::Deserialize;
use std::cmp::PartialEq;
use std::collections::HashMap;
use std::path::PathBuf;
use url::Url;
use xx::regex;

#[allow(clippy::invisible_characters)]
pub static AQUA_STANDARD_REGISTRY_FILES: Lazy<HashMap<&'static str, &'static str>> =
    Lazy::new(|| include!(concat!(env!("OUT_DIR"), "/aqua_standard_registry.rs")));

pub static AQUA_REGISTRY: Lazy<AquaRegistry> = Lazy::new(|| {
    AquaRegistry::standard().unwrap_or_else(|err| {
        warn!("failed to initialize aqua registry: {err:?}");
        AquaRegistry::default()
    })
});
static AQUA_REGISTRY_PATH: Lazy<PathBuf> = Lazy::new(|| dirs::CACHE.join("aqua-registry"));

#[derive(Default)]
pub struct AquaRegistry {
    path: PathBuf,
    repo_exists: bool,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AquaPackageType {
    GithubArchive,
    GithubContent,
    #[default]
    GithubRelease,
    Http,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
pub struct AquaPackage {
    pub r#type: AquaPackageType,
    pub repo_owner: String,
    pub repo_name: String,
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
    pub version_source: Option<String>,
    pub version_filter: Option<String>,
    pub checksum: Option<AquaChecksum>,
    pub slsa_provenance: Option<AquaSlsaProvenance>,
    overrides: Vec<AquaOverride>,
    version_constraint: String,
    version_overrides: Vec<AquaPackage>,
}

#[derive(Debug, Deserialize, Clone)]
struct AquaOverride {
    #[serde(flatten)]
    pkg: AquaPackage,
    goos: Option<String>,
    goarch: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AquaFile {
    pub name: String,
    pub src: Option<String>,
}

#[derive(Debug, Deserialize, Clone, strum::AsRefStr, strum::Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum AquaChecksumAlgorithm {
    Sha1,
    Sha256,
    Sha512,
    Md5,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AquaChecksumType {
    GithubRelease,
    Http,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AquaCosignSignature {
    pub r#type: Option<String>,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub url: Option<String>,
    pub asset: Option<String>,
}
#[derive(Debug, Deserialize, Clone)]
pub struct AquaCosign {
    pub enabled: Option<bool>,
    pub experimental: Option<bool>,
    pub signature: Option<AquaCosignSignature>,
    pub key: Option<AquaCosignSignature>,
    pub certificate: Option<AquaCosignSignature>,
    opts: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AquaSlsaProvenance {
    pub enabled: Option<bool>,
    pub r#type: Option<String>,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub url: Option<String>,
    pub asset: Option<String>,
}

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

#[derive(Debug, Deserialize, Clone)]
pub struct AquaChecksumPattern {
    pub checksum: String,
    pub file: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RegistryYaml {
    packages: Vec<AquaPackage>,
}

impl AquaRegistry {
    pub fn standard() -> Result<Self> {
        let path = AQUA_REGISTRY_PATH.clone();
        let repo = Git::new(&path);
        let mut repo_exists = repo.exists();
        if repo_exists {
            fetch_latest_repo(&repo)?;
        } else if let Some(aqua_registry_url) = &SETTINGS.aqua.registry_url {
            info!("cloning aqua registry to {path:?}");
            repo.clone(aqua_registry_url, None)?;
            repo_exists = true;
        }
        Ok(Self { path, repo_exists })
    }

    pub fn package(&self, id: &str) -> Result<AquaPackage> {
        let path_id = id.split('/').join(std::path::MAIN_SEPARATOR_STR);
        let path = self.path.join("pkgs").join(&path_id).join("registry.yaml");
        let registry: RegistryYaml = if !self.repo_exists {
            if let Some(registry) = AQUA_STANDARD_REGISTRY_FILES.get(id) {
                serde_yaml::from_str(registry)?
            } else if !path.exists() || file::modified_duration(&path)? > DAILY {
                let url: Url =
                    format!("https://mise-versions.jdx.dev/aqua-registry/{path_id}/registry.yaml")
                        .parse()?;
                http::HTTP_FETCH.download_file(url, &path, None)?;
                serde_yaml::from_reader(file::open(&path)?)?
            } else {
                serde_yaml::from_reader(file::open(&path)?)?
            }
        } else {
            serde_yaml::from_reader(file::open(&path)?)?
        };
        let mut pkg = registry
            .packages
            .into_iter()
            .next()
            .wrap_err(format!("no package found for {id} in {path:?}"))?;
        if let Some(filter) = &pkg.version_filter {
            if let Some(filter) = filter.strip_prefix("Version startsWith") {
                pkg.version_prefix = Some(
                    pkg.version_prefix
                        .unwrap_or(filter.trim().trim_matches('"').to_string()),
                );
            } else {
                warn!("unsupported version filter: {filter}");
            }
        }
        Ok(pkg)
    }

    pub fn package_with_version(&self, id: &str, v: &str) -> Result<AquaPackage> {
        Ok(self.package(id)?.with_version(v))
    }
}

fn fetch_latest_repo(repo: &Git) -> Result<()> {
    if file::modified_duration(&repo.dir)? < DAILY {
        return Ok(());
    }
    info!("updating aqua registry repo");
    repo.update(None)?;
    Ok(())
}

impl AquaPackage {
    fn with_version(mut self, v: &str) -> AquaPackage {
        if let Some(avo) = self.version_override(v).cloned() {
            self = apply_override(self, &avo)
        }
        if let Some(avo) = self.overrides.clone().into_iter().find(|o| {
            if let (Some(goos), Some(goarch)) = (&o.goos, &o.goarch) {
                goos == aqua::os() && goarch == aqua::arch()
            } else if let Some(goos) = &o.goos {
                goos == aqua::os()
            } else if let Some(goarch) = &o.goarch {
                goarch == aqua::arch()
            } else {
                false
            }
        }) {
            self = apply_override(self, &avo.pkg)
        }
        self
    }

    fn version_override(&self, v: &str) -> Option<&AquaPackage> {
        let re = regex!(r#"semver\("(.*)"\)"#);
        let re_exact = regex!(r#"Version == "(.*)""#);
        let v = versions::Versioning::new(v.strip_prefix('v').unwrap_or(v)).unwrap();
        let semver_match = |vc| {
            if let Some(caps) = re.captures(vc) {
                let vc = caps.get(1).unwrap().as_str().replace(' ', "");
                if let Some(req) = versions::Requirement::new(&vc) {
                    req.matches(&v)
                } else {
                    debug!("invalid semver constraint: {vc}");
                    false
                }
            } else if let Some(caps) = re_exact.captures(vc) {
                let vc = caps.get(1).unwrap().as_str();
                v.to_string() == vc
            } else {
                false
            }
        };
        vec![self]
            .into_iter()
            .chain(self.version_overrides.iter())
            .find(|vo| vo.version_constraint == "true" || semver_match(&vo.version_constraint))
    }

    pub fn format(&self, v: &str) -> Result<&str> {
        if self.r#type == AquaPackageType::GithubArchive {
            return Ok("tar.gz");
        }
        let format = if self.format.is_empty() {
            let asset = if !self.asset.is_empty() {
                self.asset(v)?
            } else if !self.url.is_empty() {
                self.url.to_string()
            } else {
                debug!("no asset or url for {}/{}", self.repo_owner, self.repo_name);
                "".to_string()
            };
            if asset.ends_with(".tar.gz") || asset.ends_with(".tgz") {
                "tar.gz"
            } else if asset.ends_with(".tar.xz") || asset.ends_with(".txz") {
                "tar.xz"
            } else if asset.ends_with(".tar.bz2") || asset.ends_with(".tbz2") {
                "tar.bz2"
            } else if asset.ends_with(".gz") {
                "gz"
            } else if asset.ends_with(".xz") {
                "xz"
            } else if asset.ends_with(".bz2") {
                "bz2"
            } else if asset.ends_with(".zip") {
                "zip"
            } else {
                "raw"
            }
        } else {
            match self.format.as_str() {
                "tgz" => "tar.gz",
                "txz" => "tar.xz",
                "tbz2" => "tar.bz2",
                format => format,
            }
        };
        Ok(format)
    }

    pub fn asset(&self, v: &str) -> Result<String> {
        self.parse_aqua_str(&self.asset, v, &Default::default())
    }

    pub fn asset_strs(&self, v: &str) -> Result<IndexSet<String>> {
        let mut strs = IndexSet::from([self.asset(v)?]);
        if cfg!(macos) {
            let mut ctx = HashMap::default();
            ctx.insert("Arch".to_string(), "universal".to_string());
            strs.insert(self.parse_aqua_str(&self.asset, v, &ctx)?);
        } else if cfg!(windows) {
            let mut ctx = HashMap::default();
            let with_exe = format!("{}.exe", self.parse_aqua_str(&self.asset, v, &ctx)?);
            strs.insert(with_exe);
            if cfg!(target_arch = "aarch64") {
                // assume windows arm64 emulation is supported
                ctx.insert("Arch".to_string(), "amd64".to_string());
                strs.insert(self.parse_aqua_str(&self.asset, v, &ctx)?);
                strs.insert(format!(
                    "{}.exe",
                    self.parse_aqua_str(&self.asset, v, &ctx)?
                ));
            }
        }
        Ok(strs)
    }

    pub fn url(&self, v: &str) -> Result<String> {
        self.parse_aqua_str(&self.url, v, &Default::default())
    }

    fn parse_aqua_str(
        &self,
        s: &str,
        v: &str,
        overrides: &HashMap<String, String>,
    ) -> Result<String> {
        let os = os();
        let mut arch = arch();
        if os == "darwin" && arch == "arm64" && self.rosetta2 {
            arch = "amd64";
        }
        if os == "windows" && arch == "arm64" && self.windows_arm_emulation {
            arch = "amd64";
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
        let mut ctx = hashmap! {
            "Version".to_string() => replace(v),
            "SemVer".to_string() => replace(semver),
            "OS".to_string() => replace(os),
            "GOOS".to_string() => replace(os),
            "GOARCH".to_string() => replace(arch),
            "Arch".to_string() => replace(arch),
            "Format".to_string() => replace(&self.format),
        };
        ctx.extend(overrides.clone());
        aqua_template::render(s, &ctx)
    }
}

impl AquaFile {
    pub fn src(&self, pkg: &AquaPackage, v: &str) -> Result<Option<String>> {
        let asset = pkg.asset(v)?;
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
        let ctx = hashmap! {
            "AssetWithoutExt".to_string() => asset.to_string(),
        };
        self.src
            .as_ref()
            .map(|src| pkg.parse_aqua_str(src, v, &ctx))
            .transpose()
    }
}

fn apply_override(mut orig: AquaPackage, avo: &AquaPackage) -> AquaPackage {
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
    if avo.complete_windows_ext {
        orig.complete_windows_ext = true;
    }
    if !avo.supported_envs.is_empty() {
        orig.supported_envs = avo.supported_envs.clone();
    }
    if !avo.files.is_empty() {
        orig.files = avo.files.clone();
    }
    orig.replacements.extend(avo.replacements.clone());
    if !avo.overrides.is_empty() {
        orig.overrides = avo.overrides.clone();
    }

    if let Some(avo_checksum) = avo.checksum.clone() {
        let mut checksum = orig.checksum.unwrap_or_else(|| avo_checksum.clone());
        checksum.merge(avo_checksum);
        orig.checksum = Some(checksum);
    }

    if let Some(avo_slsa_provenance) = avo.slsa_provenance.clone() {
        let mut slsa_provenance = orig
            .slsa_provenance
            .unwrap_or_else(|| avo_slsa_provenance.clone());
        slsa_provenance.merge(avo_slsa_provenance);
        orig.slsa_provenance = Some(slsa_provenance);
    }
    orig
}

impl AquaChecksum {
    pub fn _type(&self) -> &AquaChecksumType {
        self.r#type.as_ref().unwrap()
    }
    pub fn algorithm(&self) -> &AquaChecksumAlgorithm {
        self.algorithm.as_ref().unwrap()
    }
    pub fn asset_strs(&self, pkg: &AquaPackage, v: &str) -> Result<IndexSet<String>> {
        let mut asset_strs = IndexSet::new();
        for asset in pkg.asset_strs(v)? {
            let checksum_asset = self.asset.as_ref().unwrap();
            let ctx = hashmap! {
                "Asset".to_string() => asset.to_string(),
            };
            asset_strs.insert(pkg.parse_aqua_str(checksum_asset, v, &ctx)?);
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
    pub fn url(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        pkg.parse_aqua_str(self.url.as_ref().unwrap(), v, &Default::default())
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
    pub fn opts(&self, pkg: &AquaPackage, v: &str) -> Result<Vec<String>> {
        self.opts
            .iter()
            .map(|opt| pkg.parse_aqua_str(opt, v, &Default::default()))
            .collect()
    }

    fn merge(&mut self, other: Self) {
        if let Some(enabled) = other.enabled {
            self.enabled = Some(enabled);
        }
        if let Some(experimental) = other.experimental {
            self.experimental = Some(experimental);
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
        if !other.opts.is_empty() {
            self.opts = other.opts.clone();
        }
    }
}

impl AquaCosignSignature {
    pub fn url(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        pkg.parse_aqua_str(self.url.as_ref().unwrap(), v, &Default::default())
    }
    pub fn asset(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        pkg.parse_aqua_str(self.asset.as_ref().unwrap(), v, &Default::default())
    }
    pub fn arg(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        match self.r#type.as_deref().unwrap_or_default() {
            "github_release" => {
                let asset = self.asset(pkg, v)?;
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
            "http" => self.url(pkg, v),
            t => {
                warn!(
                    "unsupported cosign signature type for {}/{}: {t}",
                    pkg.repo_owner, pkg.repo_name
                );
                Ok("".to_string())
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
    pub fn asset(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        pkg.parse_aqua_str(self.asset.as_ref().unwrap(), v, &Default::default())
    }
    pub fn url(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        pkg.parse_aqua_str(self.url.as_ref().unwrap(), v, &Default::default())
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
    }
}
