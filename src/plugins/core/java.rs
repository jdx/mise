use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Display;
use std::fs::{self};
use std::path::{Path, PathBuf};

use color_eyre::eyre::{eyre, Result};
use itertools::Itertools;
use once_cell::sync::Lazy;
use serde_derive::{Deserialize, Serialize};
use versions::Versioning;

use crate::cache::CacheManager;
use crate::cli::version::{ARCH, OS};
use crate::cmd::CmdLineRunner;
use crate::config::Config;
use crate::install_context::InstallContext;
use crate::plugins::core::CorePlugin;
use crate::plugins::Plugin;
use crate::toolset::{ToolVersion, ToolVersionRequest, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{env, file, hash, http};

#[derive(Debug)]
pub struct JavaPlugin {
    core: CorePlugin,
    http: http::Client,
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
                core.cache_path.join(java_metadata_ea_cache_filename),
            )
            .with_fresh_duration(*env::RTX_FETCH_REMOTE_VERSIONS_CACHE),
            java_metadata_ga_cache: CacheManager::new(
                core.cache_path.join(java_metadata_ga_cache_filename),
            )
            .with_fresh_duration(*env::RTX_FETCH_REMOTE_VERSIONS_CACHE),
            core,
            http: http::Client::new_with_timeout(*env::RTX_FETCH_REMOTE_VERSIONS_TIMEOUT).unwrap(),
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
                        // rtx expects full versions like ".0.0"
                        metadata.insert(format!("{}.0.0", m.version), m.clone());
                    }
                }
                metadata.insert(m.to_string(), m);
            }

            Ok(metadata)
        })
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        match self.core.fetch_remote_versions_from_rtx() {
            Ok(Some(versions)) => return Ok(versions),
            Ok(None) => {}
            Err(e) => warn!("failed to fetch remote versions: {}", e),
        }
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
        let http = http::Client::new()?;
        let filename = m.url.split('/').last().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("downloading {}", &m.url));
        http.download_file(&m.url, &tarball_path)?;

        hash::ensure_checksum_sha256(&tarball_path, &m.sha256)?;

        Ok(tarball_path)
    }

    fn install(
        &self,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
        tarball_path: &Path,
        m: &JavaMetadata,
    ) -> Result<()> {
        pr.set_message(format!("installing {}", tarball_path.display()));
        if m.file_type == Some("zip".to_string()) {
            file::unzip(tarball_path, &tv.download_path())?;
        } else {
            file::untar(tarball_path, &tv.download_path())?;
        }
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

        // move Contents dir to install path for macOS, if it exists
        if os() == "macosx" && contents_dir.exists() {
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
                // undo rtx's full "*.0.0" version
                format!("openjdk-{}", &tv.version[..tv.version.len() - 4])
            } else {
                format!("openjdk-{}", tv.version)
            }
        } else {
            tv.version.clone()
        }
    }

    fn tv_to_metadata(&self, tv: &ToolVersion) -> Result<&JavaMetadata> {
        let v = self.tv_to_java_version(tv);
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

        let metadata = self
            .http
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

impl Plugin for JavaPlugin {
    fn name(&self) -> &str {
        "java"
    }

    fn list_remote_versions(&self) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    fn get_aliases(&self) -> Result<BTreeMap<String, String>> {
        let aliases = BTreeMap::from([("lts".into(), "21".into())]);
        Ok(aliases)
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        assert!(matches!(
            &ctx.tv.request,
            ToolVersionRequest::Version { .. }
        ));

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
    ) -> Result<HashMap<String, String>> {
        let map = HashMap::from([(
            "JAVA_HOME".into(),
            tv.install_path().to_string_lossy().into(),
        )]);
        Ok(map)
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
            Ok(version.to_string())
        } else {
            Ok(contents)
        }
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
    Lazy::new(|| HashSet::from(["tar.gz", "zip"].map(|s| s.to_string())));
