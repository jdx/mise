use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Display;
use std::fs::{self};
use std::path::{Path, PathBuf};

use color_eyre::eyre::{eyre, Result};
use itertools::Itertools;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_derive::{Deserialize, Serialize};
use versions::Versioning;

use crate::cache::CacheManager;
use crate::cli::args::ForgeArg;
use crate::cli::version::{ARCH, OS};
use crate::cmd::CmdLineRunner;
use crate::config::Config;
use crate::forge::Forge;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::plugins::core::CorePlugin;
use crate::plugins::VERSION_REGEX;
use crate::toolset::{ToolVersion, ToolVersionRequest, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{env, file, hash};

#[derive(Debug)]
pub struct JavaPlugin {
    core: CorePlugin,
    java_metadata_ea_cache: CacheManager<HashMap<String, JavaMetadata>>,
    java_metadata_ga_cache: CacheManager<HashMap<String, JavaMetadata>>,
}

impl JavaPlugin {
    pub fn new() -> Self {
        let core = CorePlugin::new("java");
        let java_metadata_ga_cache_filename =
            format!("java_metadata_ga_{}_{}.msgpack.z", os(), arch());
        let java_metadata_ea_cache_filename =
            format!("java_metadata_ea_{}_{}.msgpack.z", os(), arch());
        Self {
            java_metadata_ea_cache: CacheManager::new(
                core.fa.cache_path.join(java_metadata_ea_cache_filename),
            )
            .with_fresh_duration(*env::MISE_FETCH_REMOTE_VERSIONS_CACHE),
            java_metadata_ga_cache: CacheManager::new(
                core.fa.cache_path.join(java_metadata_ga_cache_filename),
            )
            .with_fresh_duration(*env::MISE_FETCH_REMOTE_VERSIONS_CACHE),
            core,
        }
    }

    fn fetch_java_metadata(&self, release_type: &str) -> Result<&HashMap<String, JavaMetadata>> {
        let cache = if release_type == "ea" {
            &self.java_metadata_ea_cache
        } else {
            &self.java_metadata_ga_cache
        };
        let release_type = release_type.to_string();
        cache.get_or_try_init(|| {
            let mut metadata = HashMap::new();

            for m in self.download_java_metadata(&release_type)?.into_iter() {
                // add openjdk short versions like "java@17.0.0" which default to openjdk
                if m.vendor == "openjdk" {
                    if m.version.contains('.') {
                        metadata.insert(m.version.to_string(), m.clone());
                    } else {
                        // mise expects full versions like ".0.0"
                        metadata.insert(format!("{}.0.0", m.version), m.clone());
                    }
                }
                metadata.insert(m.to_string(), m);
            }

            Ok(metadata)
        })
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        // TODO: find out how to get this to work for different os/arch
        // See https://github.com/jdx/mise/issues/1196
        // match self.core.fetch_remote_versions_from_mise() {
        //     Ok(Some(versions)) => return Ok(versions),
        //     Ok(None) => {}
        //     Err(e) => warn!("failed to fetch remote versions: {}", e),
        // }
        let versions = self
            .fetch_java_metadata("ga")?
            .iter()
            .sorted_by_cached_key(|(v, m)| {
                let is_shorthand = regex!(r"^\d").is_match(v);
                let vendor = &m.vendor;
                let is_jdk = m
                    .image_type
                    .as_ref()
                    .is_some_and(|image_type| image_type == "jdk");
                let features = 10 - m.features.len();
                let version = Versioning::new(v);
                (
                    is_shorthand,
                    vendor,
                    is_jdk,
                    features,
                    version,
                    v.to_string(),
                )
            })
            .map(|(v, _)| v.clone())
            .unique()
            .collect();

        Ok(versions)
    }

    fn java_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/java")
    }

    fn test_java(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        CmdLineRunner::new(self.java_bin(tv))
            .with_pr(pr)
            .env("JAVA_HOME", tv.install_path())
            .arg("-version")
            .execute()
    }

    fn download(
        &self,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
        m: &JavaMetadata,
    ) -> Result<PathBuf> {
        let filename = m.url.split('/').last().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("downloading {filename}"));
        HTTP.download_file(&m.url, &tarball_path, Some(pr))?;

        hash::ensure_checksum_sha256(&tarball_path, &m.sha256, Some(pr))?;

        Ok(tarball_path)
    }

    fn install(
        &self,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
        tarball_path: &Path,
        m: &JavaMetadata,
    ) -> Result<()> {
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        pr.set_message(format!("installing {filename}"));
        file::untar(tarball_path, &tv.download_path())?;
        self.move_to_install_path(tv, m)
    }

    fn move_to_install_path(&self, tv: &ToolVersion, m: &JavaMetadata) -> Result<()> {
        let basedir = tv
            .download_path()
            .read_dir()?
            .find(|e| e.as_ref().unwrap().file_type().unwrap().is_dir())
            .unwrap()?
            .path();
        let contents_dir = basedir.join("Contents");
        let source_dir = match m.vendor.as_str() {
            "zulu" | "liberica" => basedir,
            _ if os() == "macosx" => basedir.join("Contents").join("Home"),
            _ => basedir,
        };
        file::remove_all(tv.install_path())?;
        file::create_dir_all(tv.install_path())?;
        for entry in fs::read_dir(source_dir)? {
            let entry = entry?;
            let dest = tv.install_path().join(entry.file_name());
            trace!("moving {:?} to {:?}", entry.path(), &dest);
            file::rename(entry.path(), dest)?;
        }

        if cfg!(target_os = "macos") {
            self.handle_macos_integration(&contents_dir, tv, m)?;
        }

        Ok(())
    }

    fn handle_macos_integration(
        &self,
        contents_dir: &Path,
        tv: &ToolVersion,
        m: &JavaMetadata,
    ) -> Result<()> {
        // move Contents dir to install path for macOS, if it exists
        if contents_dir.exists() {
            file::create_dir_all(tv.install_path().join("Contents"))?;
            for entry in fs::read_dir(contents_dir)? {
                let entry = entry?;
                // skip Home dir, so we can symlink it later
                if entry.file_name() == "Home" {
                    continue;
                }
                let dest = tv.install_path().join("Contents").join(entry.file_name());
                trace!("moving {:?} to {:?}", entry.path(), &dest);
                file::rename(entry.path(), dest)?;
            }
            file::make_symlink(
                tv.install_path().as_path(),
                &tv.install_path().join("Contents").join("Home"),
            )?;
        }

        // if vendor is Zulu, symlink zulu-{major_version}.jdk/Contents to install path for macOS
        if m.vendor.as_str() == "zulu" {
            let (major_version, _) = m
                .version
                .split_once('.')
                .unwrap_or_else(|| (&m.version, ""));
            file::make_symlink(
                tv.install_path()
                    .join(format!("zulu-{}.jdk", major_version))
                    .join("Contents")
                    .as_path(),
                &tv.install_path().join("Contents"),
            )?;
        }

        if tv.install_path().join("Contents").exists() {
            info!(
                "{}",
                formatdoc! {r#"
                To enable macOS integration, run the following commands:
                sudo mkdir /Library/Java/JavaVirtualMachines/{version}.jdk
                sudo ln -s {path}/Contents /Library/Java/JavaVirtualMachines/{version}.jdk/Contents
                "#,
                    version = tv.version,
                    path = tv.install_path().display(),
                }
            );
        }
        Ok(())
    }

    fn verify(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        pr.set_message("java -version".into());
        self.test_java(tv, pr)
    }

    fn tv_release_type(&self, tv: &ToolVersion) -> String {
        tv.opts
            .get("release_type")
            .cloned()
            .unwrap_or(String::from("ga"))
    }

    fn tv_to_java_version(&self, tv: &ToolVersion) -> String {
        if regex!(r"^\d").is_match(&tv.version) {
            // undo openjdk shorthand
            if tv.version.ends_with(".0.0") {
                // undo mise's full "*.0.0" version
                format!("openjdk-{}", &tv.version[..tv.version.len() - 4])
            } else {
                format!("openjdk-{}", tv.version)
            }
        } else {
            tv.version.clone()
        }
    }

    fn tv_to_metadata(&self, tv: &ToolVersion) -> Result<&JavaMetadata> {
        let v: String = self.tv_to_java_version(tv);
        let release_type = self.tv_release_type(tv);
        let m = self
            .fetch_java_metadata(&release_type)?
            .get(&v)
            .ok_or_else(|| eyre!("no metadata found for version {}", tv.version))?;
        Ok(m)
    }

    fn download_java_metadata(&self, release_type: &str) -> Result<Vec<JavaMetadata>> {
        let url = format!(
            "https://rtx-java-metadata.jdx.dev/metadata/{}/{}/{}.json",
            release_type,
            os(),
            arch()
        );

        let metadata = HTTP_FETCH
            .json::<Vec<JavaMetadata>, _>(url)?
            .into_iter()
            .filter(|m| {
                m.file_type
                    .as_ref()
                    .is_some_and(|file_type| JAVA_FILE_TYPES.contains(file_type))
            })
            .collect();
        Ok(metadata)
    }
}

impl Forge for JavaPlugin {
    fn fa(&self) -> &ForgeArg {
        &self.core.fa
    }

    fn _list_remote_versions(&self) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    fn list_installed_versions_matching(&self, query: &str) -> eyre::Result<Vec<String>> {
        let versions = self.list_installed_versions()?;
        fuzzy_match_filter(versions, query)
    }

    fn list_versions_matching(&self, query: &str) -> eyre::Result<Vec<String>> {
        let versions = self.list_remote_versions()?;
        fuzzy_match_filter(versions, query)
    }

    fn get_aliases(&self) -> Result<BTreeMap<String, String>> {
        let aliases = BTreeMap::from([("lts".into(), "21".into())]);
        Ok(aliases)
    }

    fn legacy_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".java-version".into(), ".sdkmanrc".into()])
    }

    fn parse_legacy_file(&self, path: &Path) -> Result<String> {
        let contents = file::read_to_string(path)?;
        if path.file_name() == Some(".sdkmanrc".as_ref()) {
            let version = contents
                .lines()
                .find(|l| l.starts_with("java"))
                .unwrap_or("java=")
                .split_once('=')
                .unwrap_or_default()
                .1;
            if !version.contains('-') {
                return Ok(version.to_string());
            }
            let (version, vendor) = version.rsplit_once('-').unwrap_or_default();
            let vendor = match vendor {
                "amzn" => "corretto",
                "albba" => "dragonwell",
                "graalce" => "graalvm-community",
                "librca" => "liberica",
                "open" => "openjdk",
                "ms" => "microsoft",
                "sapmchn" => "sapmachine",
                "sem" => "semeru-openj9",
                "tem" => "temurin",
                _ => vendor, // either same vendor name or unsupported
            };
            let mut version = version.split(['+', '-'].as_ref()).collect::<Vec<&str>>()[0];
            // if vendor is zulu, we can only match the major version
            if vendor == "zulu" {
                version = version.split_once('.').unwrap_or_default().0;
            }
            Ok(format!("{}-{}", vendor, version))
        } else {
            Ok(contents)
        }
    }

    #[requires(matches!(ctx.tv.request, ToolVersionRequest::Version { .. } | ToolVersionRequest::Prefix { .. }), "unsupported tool version request type")]
    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        let metadata = self.tv_to_metadata(&ctx.tv)?;
        let tarball_path = self.download(&ctx.tv, ctx.pr.as_ref(), metadata)?;
        self.install(&ctx.tv, ctx.pr.as_ref(), &tarball_path, metadata)?;
        self.verify(&ctx.tv, ctx.pr.as_ref())?;

        Ok(())
    }

    fn exec_env(
        &self,
        _config: &Config,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        let map = BTreeMap::from([(
            "JAVA_HOME".into(),
            tv.install_path().to_string_lossy().into(),
        )]);
        Ok(map)
    }
}

fn os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macosx"
    } else {
        &OS
    }
}

fn arch() -> &'static str {
    if cfg!(target_arch = "x86_64") || cfg!(target_arch = "amd64") {
        "x86_64"
    } else if cfg!(target_arch = "armv6l") || cfg!(target_arch = "armv7l") {
        "arm32-vfp-hflt"
    } else if cfg!(target_arch = "aarch64") || cfg!(target_arch = "arm64") {
        "aarch64"
    } else {
        &ARCH
    }
}

fn fuzzy_match_filter(versions: Vec<String>, query: &str) -> eyre::Result<Vec<String>> {
    let mut query = query;
    if query == "latest" {
        query = "[0-9].*";
    }
    let query_regex = Regex::new(&format!("^{}([+-.].+)?$", query))?;
    let versions = versions
        .into_iter()
        .filter(|v| {
            if query == v {
                return true;
            }
            if VERSION_REGEX.is_match(v) {
                return false;
            }
            query_regex.is_match(v)
        })
        .collect();
    Ok(versions)
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct JavaMetadata {
    vendor: String,
    // filename: String,
    release_type: String,
    version: String,
    jvm_impl: String,
    os: String,
    architecture: String,
    file_type: Option<String>,
    image_type: Option<String>,
    features: Vec<String>,
    url: String,
    sha256: String,
    // md5: String,
    // md5_file: String,
    // sha1: String,
    // sha1_file: String,
    // sha256_file: String,
    // sha512: String,
    // sha512_file: String,
    // size: u64,
}

impl Display for JavaMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut v = vec![self.vendor.clone()];
        if self
            .image_type
            .as_ref()
            .is_some_and(|image_type| image_type == "jre")
        {
            v.push(self.image_type.clone().unwrap());
        } else if self.image_type.is_none() {
            v.push("unknown".to_string());
        }
        for f in self.features.iter() {
            if JAVA_FEATURES.contains(f) {
                v.push(f.clone());
            }
        }
        if self.jvm_impl == "openj9" {
            v.push(self.jvm_impl.clone());
        }
        v.push(self.version.clone());
        write!(f, "{}", v.join("-"))
    }
}

// only care about these features
static JAVA_FEATURES: Lazy<HashSet<String>> =
    Lazy::new(|| HashSet::from(["musl", "javafx", "lite", "large_heap"].map(|s| s.to_string())));
static JAVA_FILE_TYPES: Lazy<HashSet<String>> =
    Lazy::new(|| HashSet::from(["tar.gz"].map(|s| s.to_string())));
