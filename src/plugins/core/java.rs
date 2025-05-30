use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Display;
use std::fs::{self};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::backend::Backend;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cli::version::OS;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::file::{TarFormat, TarOptions};
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::plugins::VERSION_REGEX;
use crate::toolset::{ToolVersion, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{file, plugins};
use async_trait::async_trait;
use color_eyre::eyre::{Result, eyre};
use indoc::formatdoc;
use itertools::Itertools;
use regex::Regex;
use serde_derive::{Deserialize, Serialize};
use std::sync::LazyLock as Lazy;
use versions::Versioning;
use xx::regex;

#[derive(Debug)]
pub struct JavaPlugin {
    ba: Arc<BackendArg>,
    java_metadata_ea_cache: CacheManager<HashMap<String, JavaMetadata>>,
    java_metadata_ga_cache: CacheManager<HashMap<String, JavaMetadata>>,
}

impl JavaPlugin {
    pub fn new() -> Self {
        let settings = Settings::get();
        let ba = Arc::new(plugins::core::new_backend_arg("java"));
        let java_metadata_ga_cache_filename =
            format!("java_metadata_ga_{}_{}.msgpack.z", os(), arch(&settings));
        let java_metadata_ea_cache_filename =
            format!("java_metadata_ea_{}_{}.msgpack.z", os(), arch(&settings));
        Self {
            java_metadata_ea_cache: CacheManagerBuilder::new(
                ba.cache_path.join(java_metadata_ea_cache_filename),
            )
            .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
            .build(),
            java_metadata_ga_cache: CacheManagerBuilder::new(
                ba.cache_path.join(java_metadata_ga_cache_filename),
            )
            .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
            .build(),
            ba,
        }
    }

    async fn fetch_java_metadata(
        &self,
        release_type: &str,
    ) -> Result<&HashMap<String, JavaMetadata>> {
        let cache = if release_type == "ea" {
            &self.java_metadata_ea_cache
        } else {
            &self.java_metadata_ga_cache
        };
        let release_type = release_type.to_string();
        cache
            .get_or_try_init_async(async || {
                let mut metadata = HashMap::new();

                for m in self.download_java_metadata(&release_type).await? {
                    // add openjdk short versions like "java@17.0.0" which default to openjdk
                    if m.vendor == "openjdk" {
                        metadata.insert(m.version.to_string(), m.clone());
                    }
                    metadata.insert(m.to_string(), m);
                }

                Ok(metadata)
            })
            .await
    }

    fn java_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/java")
    }

    fn test_java(&self, tv: &ToolVersion, pr: &Box<dyn SingleReport>) -> Result<()> {
        CmdLineRunner::new(self.java_bin(tv))
            .with_pr(pr)
            .env("JAVA_HOME", tv.install_path())
            .arg("-version")
            .execute()
    }

    async fn download(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        pr: &Box<dyn SingleReport>,
        m: &JavaMetadata,
    ) -> Result<PathBuf> {
        let filename = m.url.split('/').next_back().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("download {filename}"));
        HTTP.download_file(&m.url, &tarball_path, Some(pr)).await?;

        if !tv.checksums.contains_key(filename) && m.checksum.is_some() {
            tv.checksums
                .insert(filename.to_string(), m.checksum.as_ref().unwrap().clone());
        }
        self.verify_checksum(ctx, tv, &tarball_path)?;

        Ok(tarball_path)
    }

    fn install(
        &self,
        tv: &ToolVersion,
        pr: &Box<dyn SingleReport>,
        tarball_path: &Path,
        m: &JavaMetadata,
    ) -> Result<()> {
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        pr.set_message(format!("extract {filename}"));
        if m.file_type
            .as_ref()
            .is_some_and(|file_type| file_type == "zip")
        {
            file::unzip(tarball_path, &tv.download_path())?;
        } else {
            file::untar(
                tarball_path,
                &tv.download_path(),
                &TarOptions {
                    format: TarFormat::TarGz,
                    pr: Some(pr),
                    ..Default::default()
                },
            )?;
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
                    .join(format!("zulu-{major_version}.jdk"))
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

    fn verify(&self, tv: &ToolVersion, pr: &Box<dyn SingleReport>) -> Result<()> {
        pr.set_message("java -version".into());
        self.test_java(tv, pr)
    }

    fn tv_release_type(&self, tv: &ToolVersion) -> String {
        tv.request
            .options()
            .get("release_type")
            .cloned()
            .unwrap_or(String::from("ga"))
    }

    fn tv_to_java_version(&self, tv: &ToolVersion) -> String {
        if regex!(r"^\d").is_match(&tv.version) {
            // undo openjdk shorthand
            format!("openjdk-{}", tv.version)
        } else {
            tv.version.clone()
        }
    }

    async fn tv_to_metadata(&self, tv: &ToolVersion) -> Result<&JavaMetadata> {
        let v: String = self.tv_to_java_version(tv);
        let release_type = self.tv_release_type(tv);
        let m = self
            .fetch_java_metadata(&release_type)
            .await?
            .get(&v)
            .ok_or_else(|| eyre!("no metadata found for version {}", tv.version))?;
        Ok(m)
    }

    async fn download_java_metadata(&self, release_type: &str) -> Result<Vec<JavaMetadata>> {
        let settings = Settings::get();
        let url = format!(
            "https://mise-java.jdx.dev/jvm/{}/{}/{}.json",
            release_type,
            os(),
            arch(&settings)
        );

        let metadata = HTTP_FETCH
            .json::<Vec<JavaMetadata>, _>(url)
            .await?
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

#[async_trait]
impl Backend for JavaPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<String>> {
        // TODO: find out how to get this to work for different os/arch
        // See https://github.com/jdx/mise/issues/1196
        // match self.core.fetch_remote_versions_from_mise() {
        //     Ok(Some(versions)) => return Ok(versions),
        //     Ok(None) => {}
        //     Err(e) => warn!("failed to fetch remote versions: {}", e),
        // }
        let versions = self
            .fetch_java_metadata("ga")
            .await?
            .iter()
            .sorted_by_cached_key(|(v, m)| {
                let is_shorthand = regex!(r"^\d").is_match(v);
                let vendor = &m.vendor;
                let is_jdk = m
                    .image_type
                    .as_ref()
                    .is_some_and(|image_type| image_type == "jdk");
                let features = 10 - m.features.as_ref().map_or(0, |f| f.len());
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

    fn list_installed_versions_matching(&self, query: &str) -> Vec<String> {
        let versions = self.list_installed_versions();
        self.fuzzy_match_filter(versions, query)
    }

    async fn list_versions_matching(
        &self,
        config: &Arc<Config>,
        query: &str,
    ) -> eyre::Result<Vec<String>> {
        let versions = self.list_remote_versions(config).await?;
        Ok(self.fuzzy_match_filter(versions, query))
    }

    fn get_aliases(&self) -> Result<BTreeMap<String, String>> {
        let aliases = BTreeMap::from([("lts".into(), "21".into())]);
        Ok(aliases)
    }

    fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".java-version".into(), ".sdkmanrc".into()])
    }

    fn parse_idiomatic_file(&self, path: &Path) -> Result<String> {
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
            Ok(format!("{vendor}-{version}"))
        } else {
            Ok(contents)
        }
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        let metadata = self.tv_to_metadata(&tv).await?;
        let tarball_path = self.download(ctx, &mut tv, &ctx.pr, metadata).await?;
        self.install(&tv, &ctx.pr, &tarball_path, metadata)?;
        self.verify(&tv, &ctx.pr)?;

        Ok(tv)
    }

    async fn exec_env(
        &self,
        _config: &Arc<Config>,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        let map = BTreeMap::from([(
            "JAVA_HOME".into(),
            tv.install_path().to_string_lossy().into(),
        )]);
        Ok(map)
    }

    fn fuzzy_match_filter(&self, versions: Vec<String>, query: &str) -> Vec<String> {
        let query_trim = regex::escape(query.trim_end_matches('-'));
        let query_version = format!("{}[0-9.]+", regex::escape(query));
        let query_trim_version = format!("{query_trim}-[0-9.]+");
        let query = match query {
            "latest" => "[0-9].*",
            // ends with a dash; use <query><version>
            q if q.ends_with('-') => &query_version,
            // not a shorthand version; use <query>-<version>
            q if regex!("^[a-zA-Z]+$").is_match(q) => &query_trim_version,
            // else; use trimmed query
            _ => &query_trim,
        };
        let query_regex = Regex::new(&format!("^{query}([+-.].+)?$")).unwrap();

        versions
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
            .collect()
    }
}

fn os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macosx"
    } else {
        &OS
    }
}

fn arch(settings: &Settings) -> &str {
    let arch = settings.arch();
    if arch == "x86_64" {
        "x86_64"
    } else if arch == "arm" {
        "arm32-vfp-hflt"
    } else if arch == "aarch64" {
        "aarch64"
    } else {
        arch
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct JavaMetadata {
    // architecture: String,
    checksum: Option<String>,
    // checksum_url: Option<String>,
    features: Option<Vec<String>>,
    file_type: Option<String>,
    // filename: String,
    image_type: Option<String>,
    // java_version: String,
    jvm_impl: String,
    // os: String,
    // release_type: String,
    // size: Option<i32>,
    url: String,
    vendor: String,
    version: String,
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
        if let Some(features) = &self.features {
            for f in features {
                if JAVA_FEATURES.contains(f) {
                    v.push(f.clone());
                }
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
static JAVA_FEATURES: Lazy<HashSet<String>> = Lazy::new(|| {
    HashSet::from(["crac", "javafx", "jcef", "leyden", "lite", "musl"].map(|s| s.to_string()))
});
#[cfg(unix)]
static JAVA_FILE_TYPES: Lazy<HashSet<String>> =
    Lazy::new(|| HashSet::from(["tar.gz"].map(|s| s.to_string())));
#[cfg(windows)]
static JAVA_FILE_TYPES: Lazy<HashSet<String>> =
    Lazy::new(|| HashSet::from(["zip"].map(|s| s.to_string())));
