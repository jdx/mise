use crate::backend::aqua;
use crate::backend::aqua::{arch, os};
use crate::duration::{DAILY, WEEKLY};
use crate::git::{CloneOptions, Git};
use crate::{aqua::aqua_template, config::Settings};
use crate::{dirs, file, hashmap, http};
use expr::{Context, Program, Value};
use eyre::{ContextCompat, Result, eyre};
use indexmap::IndexSet;
use itertools::Itertools;
use regex::Regex;
use serde_derive::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock as Lazy;
use std::sync::LazyLock;
use std::{
    cmp::PartialEq,
    sync::atomic::{AtomicBool, Ordering},
};
use tokio::sync::Mutex;
use url::Url;

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
#[serde(rename_all = "snake_case")]
pub enum AquaMinisignType {
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
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
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
    pub source_uri: Option<String>,
    pub source_tag: Option<String>,
}

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
        } else if let Some(aqua_registry_url) = &Settings::get().aqua.registry_url {
            info!("cloning aqua registry to {path:?}");
            repo.clone(aqua_registry_url, CloneOptions::default())?;
            repo_exists = true;
        }
        Ok(Self { path, repo_exists })
    }

    pub async fn package(&self, id: &str) -> Result<AquaPackage> {
        static CACHE: LazyLock<Mutex<HashMap<String, AquaPackage>>> =
            LazyLock::new(|| Mutex::new(HashMap::new()));
        if let Some(pkg) = CACHE.lock().await.get(id) {
            return Ok(pkg.clone());
        }
        let path_id = id.split('/').join(std::path::MAIN_SEPARATOR_STR);
        let path = self.path.join("pkgs").join(&path_id).join("registry.yaml");
        let mut pkg = self
            .fetch_package_yaml(id, &path, &path_id)
            .await?
            .packages
            .into_iter()
            .next()
            .wrap_err(format!("no package found for {id} in {path:?}"))?;
        if let Some(version_filter) = &pkg.version_filter {
            pkg.version_filter_expr = Some(expr::compile(version_filter)?);
        }
        CACHE.lock().await.insert(id.to_string(), pkg.clone());
        Ok(pkg)
    }

    pub async fn package_with_version(&self, id: &str, v: &str) -> Result<AquaPackage> {
        Ok(self.package(id).await?.with_version(v))
    }

    async fn fetch_package_yaml(
        &self,
        id: &str,
        path: &PathBuf,
        path_id: &str,
    ) -> Result<RegistryYaml> {
        let registry = if self.repo_exists {
            trace!("reading aqua-registry for {id} from repo at {path:?}");
            serde_yaml::from_reader(file::open(path)?)?
        } else if Settings::get().aqua.baked_registry
            && AQUA_STANDARD_REGISTRY_FILES.contains_key(id)
        {
            trace!("reading baked-in aqua-registry for {id}");
            serde_yaml::from_str(AQUA_STANDARD_REGISTRY_FILES.get(id).unwrap())?
        } else if !path.exists() || file::modified_duration(path)? > DAILY {
            static RATE_LIMITED: AtomicBool = AtomicBool::new(false);
            if RATE_LIMITED.load(Ordering::Relaxed) {
                warn!("aqua-registry rate limited, skipping {id}");
                return Err(eyre!("aqua-registry rate limited"));
            }
            trace!("downloading aqua-registry for {id} to {path:?}");
            let url =
                format!("https://mise-versions.jdx.dev/aqua-registry/{path_id}/registry.yaml");
            let url: Url = url.parse()?;
            match http::HTTP_FETCH.download_file(url, path, None).await {
                Ok(_) => {}
                Err(e) if http::error_code(&e) == Some(429) => {
                    warn!("aqua-registry rate limited, skipping {id}");
                    RATE_LIMITED.store(true, Ordering::Relaxed);
                    return Err(e);
                }
                Err(e) => return Err(e),
            }
            serde_yaml::from_reader(file::open(path)?)?
        } else {
            trace!("reading cached aqua-registry for {id} from {path:?}");
            serde_yaml::from_reader(file::open(path)?)?
        };
        Ok(registry)
    }
}

fn fetch_latest_repo(repo: &Git) -> Result<()> {
    if file::modified_duration(&repo.dir)? < WEEKLY {
        return Ok(());
    }
    info!("updating aqua registry repo");
    repo.update(None)?;
    Ok(())
}

impl AquaPackage {
    pub fn with_version(mut self, v: &str) -> AquaPackage {
        self = apply_override(self.clone(), self.version_override(v));
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

    fn version_override(&self, v: &str) -> &AquaPackage {
        let expr = self.expr_parser(v);
        let ctx = self.expr_ctx(v);
        vec![self]
            .into_iter()
            .chain(self.version_overrides.iter())
            .find(|vo| {
                if vo.version_constraint.is_empty() {
                    true
                } else {
                    expr.eval(&vo.version_constraint, &ctx)
                        .map_err(|e| debug!("error parsing {}: {e}", vo.version_constraint))
                        .unwrap_or(false.into())
                        .as_bool()
                        .unwrap()
                }
            })
            .unwrap_or(self)
    }

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

    pub fn asset(&self, v: &str) -> Result<String> {
        // derive asset from url if not set and url contains a path
        if self.asset.is_empty() && self.url.split("/").count() > "//".len() {
            let asset = self.url.rsplit("/").next().unwrap_or("");
            self.parse_aqua_str(asset, v, &Default::default())
        } else {
            self.parse_aqua_str(&self.asset, v, &Default::default())
        }
    }

    pub fn asset_strs(&self, v: &str) -> Result<IndexSet<String>> {
        let mut strs = IndexSet::from([self.asset(v)?]);
        if cfg!(macos) {
            let mut ctx = HashMap::default();
            ctx.insert("Arch".to_string(), "universal".to_string());
            strs.insert(self.parse_aqua_str(&self.asset, v, &ctx)?);
        } else if cfg!(windows) {
            let mut ctx = HashMap::default();
            let asset = self.parse_aqua_str(&self.asset, v, &ctx)?;
            if self.complete_windows_ext && self.format(v)? == "raw" {
                strs.insert(format!("{asset}.exe"));
            } else {
                strs.insert(asset);
            }
            if cfg!(target_arch = "aarch64") {
                // assume windows arm64 emulation is supported
                ctx.insert("Arch".to_string(), "amd64".to_string());
                strs.insert(self.parse_aqua_str(&self.asset, v, &ctx)?);
                let asset = self.parse_aqua_str(&self.asset, v, &ctx)?;
                if self.complete_windows_ext && self.format(v)? == "raw" {
                    strs.insert(format!("{asset}.exe"));
                } else {
                    strs.insert(asset);
                }
            }
        }
        Ok(strs)
    }

    pub fn url(&self, v: &str) -> Result<String> {
        let mut url = self.url.clone();
        if cfg!(windows) && self.complete_windows_ext && self.format(v)? == "raw" {
            url.push_str(".exe");
        }
        self.parse_aqua_str(&url, v, &Default::default())
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

    fn expr(&self, v: &str, program: Program) -> Result<Value> {
        let expr = self.expr_parser(v);
        expr.run(program, &self.expr_ctx(v)).map_err(|e| eyre!(e))
    }

    fn expr_parser(&self, v: &str) -> expr::Environment {
        let prefix = Regex::new(r"^[^0-9.]+").unwrap();
        let ver = versions::Versioning::new(prefix.replace(v, ""));
        let mut env = expr::Environment::new();
        env.add_function("semver", move |c| {
            if c.args.len() != 1 {
                return Err("semver() takes exactly one argument".to_string().into());
            }
            let semver = c.args[0].as_string().unwrap().replace(' ', "");
            if let Some(check_version) = versions::Requirement::new(&semver) {
                if let Some(ver) = &ver {
                    Ok(check_version.matches(ver).into())
                } else {
                    Err("invalid version".to_string().into())
                }
            } else {
                Err("invalid semver".to_string().into())
            }
        });
        env
    }

    fn expr_ctx(&self, v: &str) -> Context {
        let mut ctx = Context::default();
        ctx.insert("Version", v);
        ctx
    }

    pub fn version_filter_ok(&self, v: &str) -> Result<bool> {
        // TODO: precompile the expression
        if let Some(filter) = self.version_filter_expr.clone() {
            if let Value::Bool(expr) = self.expr(v, filter)? {
                Ok(expr)
            } else {
                warn!(
                    "invalid response from version filter: {}",
                    self.version_filter.as_ref().unwrap()
                );
                Ok(true)
            }
        } else {
            Ok(true)
        }
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
        let asset = asset.strip_suffix(".tbz").unwrap_or(asset);
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
    if let Some(avo_minisign) = avo.minisign.clone() {
        let mut minisign = orig.minisign.unwrap_or_else(|| avo_minisign.clone());
        minisign.merge(avo_minisign);
        orig.minisign = Some(minisign);
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
    pub fn url(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        pkg.parse_aqua_str(self.url.as_ref().unwrap(), v, &Default::default())
    }
    pub fn asset(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        pkg.parse_aqua_str(self.asset.as_ref().unwrap(), v, &Default::default())
    }
    pub fn public_key(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        pkg.parse_aqua_str(self.public_key.as_ref().unwrap(), v, &Default::default())
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
            version_filter_expr: None,
            version_source: None,
            checksum: None,
            slsa_provenance: None,
            minisign: None,
            overrides: vec![],
            version_constraint: "".to_string(),
            version_overrides: vec![],
        }
    }
}
