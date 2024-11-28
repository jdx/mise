use crate::backend::backend_type::BackendType;
use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::config::SETTINGS;
use crate::env::GITHUB_TOKEN;
use crate::install_context::InstallContext;
use crate::plugins::VERSION_REGEX;
use crate::tokio::RUNTIME;
use crate::toolset::ToolVersion;
use crate::{file, github, hash};
use eyre::bail;
use itertools::Itertools;
use regex::Regex;
use std::env;
use std::fmt::Debug;
use std::path::Path;
use std::sync::OnceLock;
use ubi::UbiBuilder;
use xx::regex;

#[derive(Debug)]
pub struct UbiBackend {
    ba: BackendArg,
}

// Uses ubi for installations https://github.com/houseabsolute/ubi
impl Backend for UbiBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Ubi
    }

    fn ba(&self) -> &BackendArg {
        &self.ba
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        if name_is_url(&self.tool_name()) {
            Ok(vec!["latest".to_string()])
        } else {
            let opts = self.ba.opts();
            let tag_regex = OnceLock::new();
            let mut versions = github::list_releases(&self.tool_name())?
                .into_iter()
                .map(|r| r.tag_name)
                .collect::<Vec<String>>();
            if versions.is_empty() {
                versions = github::list_tags(&self.tool_name())?.into_iter().collect();
            }
            Ok(versions
                .into_iter()
                // trim 'v' prefixes if they exist
                .map(|t| match regex!(r"^v[0-9]").is_match(&t) {
                    true => t[1..].to_string(),
                    false => t,
                })
                .sorted_by_cached_key(|v| !regex!(r"^[0-9]").is_match(v))
                .filter(|v| {
                    if let Some(re) = opts.get("tag_regex") {
                        let re = tag_regex.get_or_init(|| Regex::new(re).unwrap());
                        re.is_match(v)
                    } else {
                        true
                    }
                })
                .rev()
                .collect())
        }
    }

    fn install_version_impl(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        let mut v = tv.version.to_string();

        if let Err(err) = github::get_release(&self.tool_name(), &tv.version) {
            // this can fail with a rate limit error or 404, either way, try prefixing and if it fails, try without the prefix
            // if http::error_code(&err) == Some(404) {
            debug!(
                "Failed to get release for {}, trying with 'v' prefix: {}",
                tv, err
            );
            v = format!("v{v}");
            // }
        }

        let install = |v: &str| {
            let opts = tv.request.options();
            // Workaround because of not knowing how to pull out the value correctly without quoting
            let path_with_bin = tv.install_path().join("bin");
            let name = self.tool_name();

            let mut builder = UbiBuilder::new()
                .project(&name)
                .install_dir(path_with_bin.clone());

            if let Some(token) = &*GITHUB_TOKEN {
                builder = builder.github_token(token);
            }

            if v != "latest" {
                builder = builder.tag(v);
            }

            if let Some(exe) = opts.get("exe") {
                builder = builder.exe(exe);
            }
            if let Some(matching) = opts.get("matching") {
                builder = builder.matching(matching);
            }

            let mut ubi = builder.build().map_err(|e| eyre::eyre!(e))?;

            RUNTIME
                .block_on(ubi.install_binary())
                .map_err(|e| eyre::eyre!(e))
        };

        install(&v).or_else(|err: eyre::Error| {
            debug!(
                "Failed to install with ubi version '{}': {}, trying with '{}'",
                v, err, tv
            );
            install(&tv.version).or_else(|_| {
                bail!("Failed to install with ubi '{}': {}", tv, err);
            })
        })?;

        let bin_dir = tv.install_path().join("bin");
        let mut possible_exes = vec![tv
            .request
            .options()
            .get("exe")
            .cloned()
            .unwrap_or(tv.ba().short.to_string())];
        if cfg!(windows) {
            possible_exes.push(format!("{}.exe", possible_exes[0]));
        }
        let full_binary_path = if let Some(bin_file) = possible_exes
            .into_iter()
            .map(|e| bin_dir.join(e))
            .find(|f| f.exists())
        {
            bin_file
        } else {
            file::ls(&bin_dir)?
                .into_iter()
                .find(|f| {
                    !f.file_name()
                        .is_some_and(|f| f.to_string_lossy().starts_with("."))
                })
                .unwrap()
        };
        self.verify_checksum(ctx, &mut tv, &full_binary_path)?;

        Ok(tv)
    }

    fn fuzzy_match_filter(&self, versions: Vec<String>, query: &str) -> eyre::Result<Vec<String>> {
        let escaped_query = regex::escape(query);
        let query = if query == "latest" {
            "\\D*[0-9].*"
        } else {
            &escaped_query
        };
        let query_regex = Regex::new(&format!("^{}([-.].+)?$", query))?;
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

    fn verify_checksum(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        file: &Path,
    ) -> eyre::Result<()> {
        let mut checksum_key = file.file_name().unwrap().to_string_lossy().to_string();
        if let Some(exe) = tv.request.options().get("exe") {
            checksum_key = format!("{}-{}", checksum_key, exe);
        }
        if let Some(matching) = tv.request.options().get("matching") {
            checksum_key = format!("{}-{}", checksum_key, matching);
        }
        checksum_key = format!("{}-{}-{}", checksum_key, env::consts::OS, env::consts::ARCH);
        if let Some(checksum) = &tv.checksums.get(&checksum_key) {
            ctx.pr
                .set_message(format!("checksum verify {checksum_key}"));
            if let Some((algo, check)) = checksum.split_once(':') {
                hash::ensure_checksum(file, check, Some(ctx.pr.as_ref()), algo)?;
            } else {
                bail!("Invalid checksum: {checksum_key}");
            }
        } else if SETTINGS.lockfile && SETTINGS.experimental {
            ctx.pr
                .set_message(format!("checksum generate {checksum_key}"));
            let hash = hash::file_hash_sha256(file, Some(ctx.pr.as_ref()))?;
            tv.checksums.insert(checksum_key, format!("sha256:{hash}"));
        }
        Ok(())
    }
}

impl UbiBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba }
    }
}

fn name_is_url(n: &str) -> bool {
    n.starts_with("http")
}
