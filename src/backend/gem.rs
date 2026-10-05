use crate::args::BackendArg;
use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::cmd::CmdLineRunner;
#[cfg(unix)]
use crate::env;
use crate::file;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::{Result, config::Config};
use async_trait::async_trait;
use base64::{Engine, prelude::BASE64_STANDARD};
use eyre::{WrapErr, bail, ensure};
use indoc::formatdoc;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use std::path::Path;
use std::{fmt::Debug, sync::Arc};
use tokio::sync::OnceCell as TokioOnceCell;
use url::Url;

/// Cached gem source URL, memoized globally after first successful detection
static GEM_SOURCE: TokioOnceCell<String> = TokioOnceCell::const_new();

#[derive(Debug)]
pub(crate) struct GemBackend {
    ba: Arc<BackendArg>,
}

#[async_trait]
impl Backend for GemBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Gem
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["ruby"])
    }

    fn mark_prereleases_from_version_pattern(&self) -> bool {
        true
    }

    /// Gem installs via `gem install`, delegating fetch/resolve to RubyGems rather
    /// than downloading an artifact mise verifies, so a lockfile URL can't be
    /// enforced even though rubygems.org exposes one. Opt out of `--locked`.
    fn supports_lockfile_url(&self) -> bool {
        false
    }

    /// `source` selects which registry the version list comes from, so it has
    /// to be part of that list's cache key.
    ///
    /// Without this, two projects naming the same gem with different `source`
    /// values share one cache entry: a list fetched from registry A answers
    /// `latest` for registry B, and the install then asks B for a version only A
    /// publishes. Declared here rather than through
    /// `remote_version_cache_context` because this is exactly what the
    /// declarative hook is for: mise folds only locally overridden options into
    /// the digest, so an ordinary rubygems.org tool keeps one shared entry.
    fn remote_version_listing_tool_option_keys(&self) -> &'static [&'static str] {
        &["source"]
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        // Resolution and installation have to agree on where the gem comes
        // from. Listing versions from rubygems.org and then installing from a
        // private registry would resolve `latest` against the wrong catalogue.
        let url = match self.configured_source(config).await? {
            // `join` rather than concatenation. The source is normalized to end
            // in `/`, so this appends within it instead of replacing its last
            // path segment, and it cannot smuggle the API path into a query
            // string or a fragment the way string-building could.
            // GitHub Packages has no JSON versions API.
            Some(source) if source.host_str() == Some(GITHUB_PACKAGES_HOST) => bail!(
                "GitHub Packages has no version-listing API mise can read, so `{}` \
                 cannot resolve `latest` or a prefix there: pin an exact version",
                self.ba()
            ),
            Some(source) => source.join(&format!("api/v1/versions/{}.json", self.tool_name()))?,
            None => format!(
                "{}api/v1/versions/{}.json",
                self.get_gem_source(config).await,
                self.tool_name()
            )
            .parse()?,
        };
        let (url, headers) = credentials_as_header(url)?;
        let response: Vec<RubyGemsVersion> = HTTP_FETCH.json_with_headers(url, &headers).await?;

        // RubyGems API returns newest-first, mise expects oldest-first
        let mut versions: Vec<VersionInfo> = response
            .into_iter()
            .map(|v| VersionInfo {
                version: v.number,
                created_at: v.created_at,
                ..Default::default()
            })
            .collect();
        versions.reverse();

        Ok(versions)
    }

    async fn resolve_exact_version(
        &self,
        _config: &Arc<Config>,
        version: &str,
    ) -> eyre::Result<Option<String>> {
        // RubyGems allows non-semver versions (4-part like rails 5.0.0.1,
        // dotted prereleases like 1.2.3.rc1) — those keep using remote
        // discovery. A full semver request is exact; `gem install --version`
        // fails when it does not exist upstream.
        Ok(versions::SemVer::new(version).map(|_| version.to_string()))
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        // Check if gem is available
        self.warn_if_dependency_missing(
            &ctx.config,
            "gem",
            &["ruby", "gem"],
            "To use gem packages with mise, you need to install Ruby first:\n\
              mise use ruby@latest",
        )
        .await;

        let source = self.configured_source(&ctx.config).await?;
        let mut cmd = match &source {
            // A credential in argv is visible to other users in `ps`. RubyGems
            // only reads credentials from the source URL, so run `bin/gem`'s
            // two lines under `ruby -e` and hand the URL over in the
            // environment, which only the same user can read.
            Some(source) if carries_credentials(source) => {
                CmdLineRunner::new(self.spawn_program(&ctx.config, Some(&ctx.ts), "ruby").await)
                    .arg("-e")
                    .arg(GEM_WITH_ENV_SOURCE)
                    .arg("--")
                    .env("MISE_GEM_SOURCE", source.as_str())
                    // Raw mode would bypass the output redaction below.
                    .never_raw()
            }
            _ => CmdLineRunner::new(self.spawn_program(&ctx.config, Some(&ctx.ts), "gem").await),
        }
        .arg("install")
        .arg(self.tool_name())
        .arg("--version")
        .arg(&tv.version)
        .arg("--install-dir")
        .arg(tv.install_path().join("libexec"));
        if let Some(source) = &source {
            // `--source` rather than `--clear-sources --source`: the latter
            // would also cut off the registry holding this gem's dependencies,
            // which commonly still live on rubygems.org.
            if !carries_credentials(source) {
                cmd = cmd.arg("--source").arg(source.as_str());
            }
            // gem's own fetch errors redact the source (`Gem::Uri#redacted`),
            // but `-V`, a `~/.gemrc` verbosity setting or a wrapper can still
            // echo it, so scrub the streamed output too.
            cmd = cmd.redact(ctx.config.redactions().iter().cloned());
        }
        cmd.with_pr(ctx.pr.as_ref())
            .envs(self.dependency_env(&ctx.config).await?)
            .env_values(tv.install_env())
            .execute()?;

        // We install the gem to {install_path}/libexec and create a wrapper script for each executable
        // in {install_path}/bin that sets GEM_HOME and executes the gem installed
        env_script_all_bin_files(&tv.install_path())?;

        #[cfg(unix)]
        {
            // Rewrite shebangs for better compatibility:
            // - System Ruby: uses `#!/usr/bin/env ruby` for PATH-based resolution
            // - Mise Ruby: uses minor version symlink (e.g., .../ruby/3.1/bin/ruby) so patch
            //   upgrades don't break gems, while still being pinned to a minor version
            rewrite_gem_shebangs(&tv.install_path())?;

            // Create a ruby symlink in libexec/bin for polyglot script fallback
            // RubyGems polyglot scripts have: exec "$bindir/ruby" "-x" "$0" "$@"
            create_ruby_symlink(&tv.install_path())?;
        }

        Ok(tv)
    }
}

impl GemBackend {
    pub(crate) fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    /// A `source` option, which pins one tool to one gem registry.
    ///
    /// Without it the only way to install from a private registry is to make it
    /// the machine's primary `gem sources` entry, because that is what
    /// [`Self::get_gem_source`] reads and what a bare `gem install` uses. That
    /// redirects every unrelated `gem install` on the machine to satisfy one
    /// tool, which is too blunt to ask of anyone.
    ///
    /// A private registry usually needs a credential, and RubyGems takes it as
    /// basic-auth userinfo on the source URL: `Gem::Request` reads `uri.user`
    /// and `uri.password`, and `~/.gem/credentials` is only consulted by the
    /// publishing commands. So a token genuinely can appear here, and anything
    /// it touches is registered for redaction rather than assumed harmless.
    ///
    /// Read through `get_tool_opts_with_overrides` rather than `ba().opts()`.
    /// A backend built straight from a CLI argument, as `mise ls-remote
    /// gem:foo` does, carries no options from the config at all, so reading the
    /// arg would silently fall back to the machine's primary source while the
    /// cache key, which mise builds from the resolved options, still filed the
    /// answer under the configured registry. The next install would then take a
    /// version from one registry and ask the other for it.
    async fn configured_source(&self, config: &Arc<Config>) -> Result<Option<Url>> {
        let opts = config.get_tool_opts_with_overrides(self.ba()).await?;
        let Some(raw) = opts.get("source") else {
            return Ok(None);
        };
        let Some(url) = parse_source(raw)? else {
            return Ok(None);
        };
        let url = with_github_packages_token(url, || {
            crate::github::resolve_token("github.com").map(|(token, _)| token)
        });
        config.add_redactions(source_secrets(&url));
        Ok(Some(url))
    }

    /// Get the primary gem source URL using the mise-managed Ruby environment.
    /// The result is memoized globally after first successful detection.
    async fn get_gem_source(&self, config: &Arc<Config>) -> &'static str {
        const DEFAULT_SOURCE: &str = "https://rubygems.org/";

        // Get the mise-managed Ruby environment
        let env = self.dependency_env(config).await.unwrap_or_default();
        let gem = self.spawn_program(config, None, "gem").await;

        // Try to initialize the source - only memoize on success
        match GEM_SOURCE
            .get_or_try_init(|| async {
                let output = crate::cmd::cmd_read_async(&gem, &["sources"], &env)
                    .await
                    .map_err(|e| eyre::eyre!("failed to run `gem sources`: {e}"))?;

                Ok::<_, eyre::Report>(parse_gem_source_output(&output))
            })
            .await
        {
            Ok(source) => source.as_str(),
            Err(e) => {
                warn!("{e}, falling back to rubygems.org");
                DEFAULT_SOURCE
            }
        }
    }
}

/// Parse and validate a configured `source`, returning `None` when it carries
/// nothing.
///
/// Parsed rather than pasted together. The version-listing endpoint lives under
/// this URL, and appending to a raw string gets that wrong in ways that surface
/// as a puzzling 404 rather than as the configuration error they are: a query
/// string swallows the API path (`…/acme?token=x` asks for `/acme` with the
/// query `token=x/api/v1/…`), a fragment discards it entirely, and a missing
/// trailing slash runs the host and the path together.
///
/// The trailing slash is added here so `Url::join` treats the source as a
/// directory to append within rather than a file to replace.
fn parse_source(raw: &str) -> Result<Option<Url>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let mut url: Url = raw
        .parse()
        .wrap_err_with(|| format!("gem `source` is not a valid URL: {}", redacted(raw)))?;
    ensure!(
        matches!(url.scheme(), "http" | "https"),
        "gem `source` must be an http or https URL, got scheme `{}`",
        url.scheme()
    );
    ensure!(
        url.scheme() == "https" || !carries_credentials(&url) || is_loopback(&url),
        "gem `source` would send its credential in clear text: use https, or a \
         loopback host for a registry running locally"
    );
    ensure!(
        url.query().is_none() && url.fragment().is_none(),
        "gem `source` must not carry a query string or fragment: the version API path is \
         appended to it, and both would swallow that path"
    );
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(Some(url))
}

/// `bin/gem`, plus `--source` from `MISE_GEM_SOURCE`. The variable is deleted
/// first so native extension builds do not inherit it.
const GEM_WITH_ENV_SOURCE: &str = r#"require "rubygems/gem_runner"
Gem::GemRunner.new.run(ARGV + ["--source", ENV.delete("MISE_GEM_SOURCE")])"#;

/// GitHub Packages' RubyGems registry.
const GITHUB_PACKAGES_HOST: &str = "rubygems.pkg.github.com";

/// Add the resolved GitHub token to an https GitHub Packages source that has
/// no credentials. Other hosts and explicit credentials are left alone.
fn with_github_packages_token(mut url: Url, token: impl FnOnce() -> Option<String>) -> Url {
    if url.scheme() != "https"
        || url.host_str() != Some(GITHUB_PACKAGES_HOST)
        || carries_credentials(&url)
    {
        return url;
    }
    if let Some(token) = token().filter(|t| !t.is_empty()) {
        // GitHub Packages takes the token as the username.
        let _ = url.set_username(&token);
    }
    url
}

/// Move a URL's userinfo into an `Authorization` header, since the HTTP
/// client can print URLs unredacted (e.g. `MISE_LOG_HTTP`).
fn credentials_as_header(mut url: Url) -> Result<(Url, HeaderMap)> {
    let mut headers = HeaderMap::new();
    if !carries_credentials(&url) {
        return Ok((url, headers));
    }
    let user = urlencoding::decode(url.username())?.into_owned();
    let password = urlencoding::decode(url.password().unwrap_or_default())?.into_owned();
    let mut value = HeaderValue::from_str(&format!(
        "Basic {}",
        BASE64_STANDARD.encode(format!("{user}:{password}"))
    ))?;
    value.set_sensitive(true);
    headers.insert(AUTHORIZATION, value);
    let _ = url.set_username("");
    let _ = url.set_password(None);
    Ok((url, headers))
}

/// Whether this URL carries basic-auth userinfo.
fn carries_credentials(url: &Url) -> bool {
    !url.username().is_empty() || url.password().is_some_and(|p| !p.is_empty())
}

/// Whether this URL points at the machine it is running on.
///
/// A registry on loopback is the ordinary way to test one, and the credential
/// never crosses a network, so plain HTTP is allowed there and nowhere else.
fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(host)) => host == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// The shortest credential safe to register on its own.
///
/// Redaction is substring replacement across every later log line, so a bare
/// one or two character value would blank out unrelated text everywhere. That
/// is a reason to be careful about the BARE value only: the userinfo pattern
/// below carries its own delimiters and is specific at any length, so a short
/// credential is still covered wherever it appears as part of a URL, which is
/// everywhere mise or gem renders it.
const MIN_REDACTABLE_SECRET: usize = 8;

/// The parts of a source URL that must never be printed.
///
/// A registry that authenticates with a token takes it as userinfo. GitHub
/// Packages puts the token in the user position with no password at all, so
/// both halves are candidates; the password is always a secret, and the
/// username is treated as one only when it stands alone, since otherwise it is
/// a name like `x-access-token` and redacting it everywhere is just noise.
fn source_secrets(url: &Url) -> Vec<String> {
    let user = url.username();
    let password = url.password().unwrap_or_default();
    if user.is_empty() && password.is_empty() {
        return vec![];
    }

    // The userinfo exactly as a URL spells it, trailing `@` included. That
    // delimiter is what makes it specific: it cannot collide with ordinary text
    // the way a bare `ab` would, so it is registered whatever its length, and a
    // short credential is protected rather than skipped.
    let mut secrets = vec![if password.is_empty() {
        format!("{user}@")
    } else {
        format!("{user}:{password}@")
    }];

    // The bare secret as well, in case something prints it outside a URL, but
    // only when it is long enough for substring replacement to be safe.
    let bare = if password.is_empty() { user } else { password };
    if bare.len() >= MIN_REDACTABLE_SECRET {
        secrets.push(bare.to_string());
    }
    secrets
}

/// Strip userinfo so an unparseable value can still be named in an error.
///
/// This runs before the value has been registered for redaction, precisely
/// because it failed to parse, so it cannot lean on the global redactor.
///
/// Splits on the LAST `@` and does not require a scheme. Omitting the scheme is
/// exactly the typo this has to survive: the documented example puts the token
/// in the user position, so `{{ env.GEM_TOKEN }}@gems.example.com` is an easy
/// thing to write, it fails to parse, and everything before the `@` is the
/// secret.
fn redacted(raw: &str) -> String {
    let (scheme, rest) = match raw.split_once("://") {
        Some((scheme, rest)) => (format!("{scheme}://"), rest),
        None => (String::new(), raw),
    };
    match rest.rsplit_once('@') {
        Some((_, host)) => format!("{scheme}[redacted]@{host}"),
        None => raw.to_string(),
    }
}

/// RubyGems API response for version info
#[derive(Debug, Deserialize)]
struct RubyGemsVersion {
    number: String,
    created_at: Option<String>,
}

/// Parse gem sources output to extract the primary source URL.
/// Output format:
/// ```text
/// *** CURRENT SOURCES ***
///
/// https://rubygems.org/
/// ```
fn parse_gem_source_output(output: &str) -> String {
    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("http://") || line.starts_with("https://") {
            // Ensure URL ends with /
            return if line.ends_with('/') {
                line.to_string()
            } else {
                format!("{}/", line)
            };
        }
    }
    // Default to rubygems.org if no source found
    "https://rubygems.org/".to_string()
}

#[cfg(unix)]
fn env_script_all_bin_files(install_path: &Path) -> eyre::Result<bool> {
    let install_bin_path = install_path.join("bin");
    let install_libexec_path = install_path.join("libexec");

    file::create_dir_all(&install_bin_path)?;

    for path in get_gem_executables(install_path)? {
        let file_name = path
            .file_name()
            .ok_or_else(|| eyre::eyre!("invalid gem executable path: {}", path.display()))?;
        let exec_path = install_bin_path.join(file_name);
        let gem_exec_path = path.to_str().ok_or_else(|| {
            eyre::eyre!(
                "gem executable path contains invalid UTF-8: {}",
                path.display()
            )
        })?;
        let gem_home = install_libexec_path.to_str().ok_or_else(|| {
            eyre::eyre!(
                "libexec path contains invalid UTF-8: {}",
                install_libexec_path.display()
            )
        })?;
        file::write(
            &exec_path,
            formatdoc!(
                r#"
                #!/usr/bin/env bash
                GEM_HOME="{gem_home}" exec {gem_exec_path} "$@"
                "#,
                gem_home = gem_home,
                gem_exec_path = gem_exec_path,
            ),
        )?;
        file::make_executable(&exec_path)?;
    }

    Ok(true)
}

#[cfg(windows)]
fn env_script_all_bin_files(install_path: &Path) -> eyre::Result<bool> {
    let install_bin_path = install_path.join("bin");
    let install_libexec_path = install_path.join("libexec");

    file::create_dir_all(&install_bin_path)?;

    for path in get_gem_executables(install_path)? {
        // On Windows, create .cmd wrapper scripts
        let file_stem = path
            .file_stem()
            .ok_or_else(|| eyre::eyre!("invalid gem executable path: {}", path.display()))?
            .to_string_lossy();
        let exec_path = install_bin_path.join(format!("{}.cmd", file_stem));
        let gem_exec_path = path.to_str().ok_or_else(|| {
            eyre::eyre!(
                "gem executable path contains invalid UTF-8: {}",
                path.display()
            )
        })?;
        let gem_home = install_libexec_path.to_str().ok_or_else(|| {
            eyre::eyre!(
                "libexec path contains invalid UTF-8: {}",
                install_libexec_path.display()
            )
        })?;
        file::write(
            &exec_path,
            formatdoc!(
                r#"@echo off
                set "GEM_HOME={gem_home}"
                "{gem_exec_path}" %*
                "#,
                gem_home = gem_home,
                gem_exec_path = gem_exec_path,
            ),
        )?;
    }

    Ok(true)
}

fn get_gem_executables(install_path: &Path) -> eyre::Result<Vec<std::path::PathBuf>> {
    // TODO: Find a way to get the list of executables from the gemspec of the
    //       installed gem rather than just listing the files in the bin directory.
    let install_libexec_bin_path = install_path.join("libexec/bin");
    let files = file::ls(&install_libexec_bin_path)?
        .into_iter()
        .filter(|p| file::is_executable(p))
        .collect();
    Ok(files)
}

#[cfg(unix)]
/// Creates a `ruby` symlink in libexec/bin/ for RubyGems polyglot script fallback.
///
/// RubyGems polyglot scripts include: `exec "$bindir/ruby" "-x" "$0" "$@"`
/// This fallback runs when the script is executed via /bin/sh instead of ruby.
/// We create a symlink to the mise-managed Ruby (using minor version) so
/// the fallback works correctly.
fn create_ruby_symlink(install_path: &Path) -> eyre::Result<()> {
    create_ruby_symlink_in(&env::MISE_INSTALLS_DIR, install_path)
}

#[cfg(unix)]
/// [`create_ruby_symlink`] for the installs root `installs_dir`.
fn create_ruby_symlink_in(installs_dir: &Path, install_path: &Path) -> eyre::Result<()> {
    let libexec_bin = install_path.join("libexec/bin");
    let ruby_symlink = libexec_bin.join("ruby");

    // Don't overwrite if it already exists
    if ruby_symlink.exists() || ruby_symlink.is_symlink() {
        return Ok(());
    }

    // Find which Ruby we're using by checking an existing gem executable's shebang
    let executables = get_gem_executables(install_path)?;
    let Some(exec_path) = executables.first() else {
        return Ok(());
    };

    let content = file::read_to_string(exec_path)?;
    let lines: Vec<&str> = content.lines().collect();
    let Some((_, shebang_line)) = find_ruby_shebang(&lines) else {
        return Ok(());
    };

    // Extract the ruby path from the shebang
    let ruby_path = shebang_line
        .trim_start_matches("#!")
        .split_whitespace()
        .next()
        .unwrap_or("");

    if ruby_path.is_empty() {
        return Ok(());
    }
    let ruby_path = mise_ruby_path(installs_dir, ruby_path);

    // Only create symlink for mise-managed Ruby
    // For system Ruby, the shebang is #!/usr/bin/env ruby, which we can't symlink to
    if !is_mise_ruby_path(installs_dir, &ruby_path) {
        return Ok(());
    }

    // Create symlink to the ruby executable
    file::make_symlink(Path::new(&ruby_path), &ruby_symlink)?;
    Ok(())
}

#[cfg(unix)]
/// Rewrites shebangs in gem executables to improve compatibility.
///
/// For system Ruby: Uses `#!/usr/bin/env ruby` for PATH-based resolution.
/// For mise-managed Ruby: Uses minor version symlink (e.g., `.../ruby/3.1/bin/ruby`)
/// so that patch upgrades (3.1.0 → 3.1.1) don't break gems.
///
/// Handles both regular Ruby scripts and RubyGems polyglot scripts which have
/// `#!/bin/sh` on line 1 but the actual Ruby shebang after `=end`.
fn rewrite_gem_shebangs(install_path: &Path) -> eyre::Result<()> {
    rewrite_gem_shebangs_in(&env::MISE_INSTALLS_DIR, install_path)
}

#[cfg(unix)]
/// [`rewrite_gem_shebangs`] for the installs root `installs_dir`.
fn rewrite_gem_shebangs_in(installs_dir: &Path, install_path: &Path) -> eyre::Result<()> {
    let executables = get_gem_executables(install_path)?;

    for exec_path in executables {
        let content = file::read_to_string(&exec_path)?;
        let lines: Vec<&str> = content.lines().collect();

        if lines.is_empty() {
            continue;
        }

        // Find the Ruby shebang line - either line 1 or after =end for polyglot scripts
        let (shebang_line_idx, shebang_line) = if let Some(info) = find_ruby_shebang(&lines) {
            info
        } else {
            continue;
        };

        // Extract the Ruby path and any arguments from the shebang
        let shebang_content = shebang_line.trim_start_matches("#!");
        let mut parts = shebang_content.split_whitespace();
        let ruby_path = mise_ruby_path(installs_dir, parts.next().unwrap_or(""));
        let shebang_args: Vec<&str> = parts.collect();

        let new_shebang = if is_mise_ruby_path(installs_dir, &ruby_path) {
            // Mise-managed Ruby: use minor version symlink, preserving any arguments
            match to_minor_version_shebang(installs_dir, &ruby_path) {
                Some(path) => {
                    if shebang_args.is_empty() {
                        format!("#!{path}")
                    } else {
                        format!("#!{path} {}", shebang_args.join(" "))
                    }
                }
                None => continue, // Keep original if we can't parse
            }
        } else {
            // System Ruby: use env-based shebang
            // Note: env shebangs generally can't preserve arguments portably
            "#!/usr/bin/env ruby".to_string()
        };

        // Rewrite the file with new shebang at the correct line
        let mut new_lines: Vec<&str> = lines.clone();
        let new_shebang_ref: &str = &new_shebang;
        new_lines[shebang_line_idx] = new_shebang_ref;
        let trailing_newline = if content.ends_with('\n') { "\n" } else { "" };
        let new_content = format!("{}{trailing_newline}", new_lines.join("\n"));
        file::write(&exec_path, &new_content)?;
    }

    Ok(())
}

#[cfg(unix)]
/// Finds the Ruby shebang line in a script.
/// Returns (line_index, line_content) or None if not found.
///
/// For regular Ruby scripts, this is line 0 with `#!...ruby...`.
/// For RubyGems polyglot scripts (starting with `#!/bin/sh`), the Ruby shebang
/// is the first `#!...ruby...` line after `=end`.
fn find_ruby_shebang<'a>(lines: &'a [&'a str]) -> Option<(usize, &'a str)> {
    let first_line = lines.first()?;

    // Check if first line is a Ruby shebang
    if first_line.starts_with("#!") && first_line.contains("ruby") {
        return Some((0, first_line));
    }

    // Check for polyglot format: #!/bin/sh followed by =end and then Ruby shebang
    if first_line.starts_with("#!/bin/sh") {
        let mut found_end = false;
        for (idx, line) in lines.iter().enumerate().skip(1) {
            if line.trim() == "=end" {
                found_end = true;
                continue;
            }
            if found_end && line.starts_with("#!") && line.contains("ruby") {
                return Some((idx, line));
            }
        }
    }

    None
}

#[cfg(unix)]
/// `ruby_path` as the installs tree names it. A ruby that RubyGems recorded by its
/// identity-layout directory (`installs/ruby-<hash>/bin/ruby`) is named through its version
/// link (`installs/ruby/3.1.0/bin/ruby`) instead, which is what the minor version link below
/// is built on. Any other path comes back unchanged.
fn mise_ruby_path(installs_dir: &Path, ruby_path: &str) -> String {
    crate::backend::static_helpers::tool_link_path(installs_dir, Path::new(ruby_path), "ruby")
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| ruby_path.to_string())
}

#[cfg(unix)]
/// Checks if a Ruby path is within mise's installs directory.
fn is_mise_ruby_path(installs_dir: &Path, ruby_path: &str) -> bool {
    let ruby_installs = installs_dir.join("ruby");
    Path::new(ruby_path).starts_with(&ruby_installs)
}

#[cfg(unix)]
/// Converts a full version Ruby shebang to use the minor version symlink.
/// e.g., `/home/user/.mise/installs/ruby/3.1.0/bin/ruby` → `/home/user/.mise/installs/ruby/3.1/bin/ruby`
fn to_minor_version_shebang(installs_dir: &Path, ruby_path: &str) -> Option<String> {
    let ruby_installs = installs_dir.join("ruby");
    let ruby_installs_str = ruby_installs.to_string_lossy();

    // Check if path matches pattern: {installs}/ruby/{version}/bin/ruby
    let path = Path::new(ruby_path);
    let rel_path = path.strip_prefix(&ruby_installs).ok()?;
    let mut components = rel_path.components();

    // First component should be the version (e.g., "3.1.0")
    let version_component = components.next()?.as_os_str().to_string_lossy();
    let version_str = version_component.as_ref();

    // Extract minor version (e.g., "3.1.0" → "3.1")
    let minor_version = extract_minor_version(version_str)?;

    // Reconstruct the path with minor version
    let remaining: std::path::PathBuf = components.collect();
    Some(format!(
        "{}/{}/{}",
        ruby_installs_str,
        minor_version,
        remaining.display()
    ))
}

#[cfg(any(unix, test))]
/// Extracts major.minor from a version string.
/// e.g., "3.1.0" → "3.1", "3.2.1-preview1" → "3.2"
fn extract_minor_version(version: &str) -> Option<String> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() >= 2 {
        Some(format!("{}.{}", parts[0], parts[1]))
    } else {
        None
    }
}

/// Search rubygems.org for gems matching `query`. The endpoint has a fixed
/// page size, so results are truncated to `limit`.
pub(crate) async fn search_tools(query: &str, limit: usize) -> Result<Vec<vfox::BackendTool>> {
    #[derive(Deserialize)]
    struct SearchGem {
        name: String,
        info: Option<String>,
    }

    let url = url::Url::parse_with_params(
        "https://rubygems.org/api/v1/search.json",
        &[("query", query)],
    )?;
    let res: Vec<SearchGem> = HTTP_FETCH.json(url).await?;
    Ok(res
        .into_iter()
        .take(limit)
        .map(|g| vfox::BackendTool {
            name: g.name,
            description: g.info,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn exact_semver_versions_resolve_without_remote_discovery() {
        let config = Config::get().await.unwrap();
        let backend = GemBackend::from_arg("gem:rubocop".into());

        assert_eq!(
            backend
                .resolve_exact_version(&config, "1.69.0")
                .await
                .unwrap()
                .as_deref(),
            Some("1.69.0")
        );
    }

    #[tokio::test]
    async fn non_semver_versions_require_remote_discovery() {
        let config = Config::get().await.unwrap();
        let backend = GemBackend::from_arg("gem:rails".into());

        // 4-part and dotted-prerelease RubyGems versions are not semver and
        // must keep resolving against the remote version list.
        for version in ["latest", "5", "5.0", "5.0.0.1", "1.2.3.rc1"] {
            assert_eq!(
                backend
                    .resolve_exact_version(&config, version)
                    .await
                    .unwrap(),
                None,
                "{version} should use remote discovery"
            );
        }
    }

    /// The version API path is appended to the source, so a source that does
    /// not end in `/` would otherwise run the host and the path together.
    #[test]
    fn a_source_is_normalized_to_end_with_a_slash() {
        for input in [
            "https://gems.example.com",
            "https://gems.example.com/",
            "  https://gems.example.com  ",
        ] {
            assert_eq!(
                parse_source(input).unwrap().map(|u| u.to_string()),
                Some("https://gems.example.com/".to_string()),
                "{input:?}"
            );
        }
    }

    fn github_packages(raw: &str, token: Option<&str>) -> String {
        let url = parse_source(raw).unwrap().unwrap();
        with_github_packages_token(url, || token.map(str::to_string)).to_string()
    }

    /// A bare GitHub Packages source borrows the resolved GitHub token.
    #[test]
    fn a_github_packages_source_gets_the_github_token() {
        assert_eq!(
            github_packages("https://rubygems.pkg.github.com/acme", Some("ghp_secret")),
            "https://ghp_secret@rubygems.pkg.github.com/acme/"
        );
    }

    /// Credentials already in the URL win over the resolved token.
    #[test]
    fn a_github_packages_source_keeps_its_own_credentials() {
        assert_eq!(
            github_packages(
                "https://me:mine@rubygems.pkg.github.com/acme",
                Some("ghp_secret")
            ),
            "https://me:mine@rubygems.pkg.github.com/acme/"
        );
    }

    /// The GitHub token never goes to any other registry.
    #[test]
    fn other_sources_never_get_the_github_token() {
        assert_eq!(
            github_packages("https://gems.example.com/acme", Some("ghp_secret")),
            "https://gems.example.com/acme/"
        );
    }

    /// An http source never gets the token.
    #[test]
    fn an_http_github_packages_source_never_gets_the_github_token() {
        assert_eq!(
            github_packages("http://rubygems.pkg.github.com/acme", Some("ghp_secret")),
            "http://rubygems.pkg.github.com/acme/"
        );
    }

    /// Credentials move from the URL to a Basic header.
    #[test]
    fn source_credentials_travel_as_a_header() {
        let url: Url = "https://ghp_secret@rubygems.pkg.github.com/acme/api/v1/versions/x.json"
            .parse()
            .unwrap();
        let (url, headers) = credentials_as_header(url).unwrap();
        assert_eq!(
            url.as_str(),
            "https://rubygems.pkg.github.com/acme/api/v1/versions/x.json"
        );
        let value = &headers[AUTHORIZATION];
        assert!(value.is_sensitive());
        assert_eq!(
            value.to_str().unwrap(),
            format!("Basic {}", BASE64_STANDARD.encode("ghp_secret:"))
        );

        // Percent-decoded; `+` stays literal.
        let url: Url = "https://me:p%40s+s@gems.example.com/".parse().unwrap();
        let (_, headers) = credentials_as_header(url).unwrap();
        assert_eq!(
            headers[AUTHORIZATION].to_str().unwrap(),
            format!("Basic {}", BASE64_STANDARD.encode("me:p@s+s"))
        );

        let url: Url = "https://rubygems.org/".parse().unwrap();
        let (url, headers) = credentials_as_header(url).unwrap();
        assert_eq!(url.as_str(), "https://rubygems.org/");
        assert!(headers.is_empty());
    }

    /// No token resolved, or an empty one, leaves the source anonymous.
    #[test]
    fn a_github_packages_source_without_a_token_stays_anonymous() {
        for token in [None, Some("")] {
            assert_eq!(
                github_packages("https://rubygems.pkg.github.com/acme", token),
                "https://rubygems.pkg.github.com/acme/",
                "{token:?}"
            );
        }
    }

    /// An empty or whitespace-only value means "not configured" rather than
    /// "install from the empty string", which would build a nonsense URL.
    #[test]
    fn an_empty_source_is_treated_as_unset() {
        for input in ["", "   ", "\t"] {
            assert!(parse_source(input).unwrap().is_none(), "{input:?}");
        }
    }

    /// A path under the host is kept: registries commonly scope gems per owner,
    /// as in `https://gems.example.com/acme`.
    #[test]
    fn a_source_path_is_preserved() {
        assert_eq!(
            parse_source("https://gems.example.com/acme")
                .unwrap()
                .map(|u| u.to_string()),
            Some("https://gems.example.com/acme/".to_string())
        );
    }

    /// This is the assertion that matters: the endpoint has to land *under* the
    /// source. Built by concatenation, `…/acme?token=x` asked for `/acme` with
    /// the query `token=x/api/v1/…`, and a fragment dropped the path entirely.
    #[test]
    fn the_versions_endpoint_lands_under_the_source() {
        let source = parse_source("https://gems.example.com/acme")
            .unwrap()
            .unwrap();
        assert_eq!(
            source
                .join("api/v1/versions/internal-cli.json")
                .unwrap()
                .as_str(),
            "https://gems.example.com/acme/api/v1/versions/internal-cli.json"
        );
    }

    /// A query or fragment cannot survive having a path appended to it, so it
    /// is a configuration error rather than a 404 to puzzle over later.
    #[test]
    fn a_source_with_a_query_or_fragment_is_rejected() {
        for input in [
            "https://gems.example.com/acme?token=x",
            "https://gems.example.com/acme#mirror",
            "https://gems.example.com/acme?",
        ] {
            assert!(parse_source(input).is_err(), "{input:?} should be rejected");
        }
    }

    /// Basic-auth userinfo over plain HTTP puts the token on the wire for
    /// anything between here and the registry.
    #[test]
    fn a_credential_may_not_travel_over_plain_http() {
        assert!(parse_source("http://ghp_tok3n@gems.example.com").is_err());
        assert!(parse_source("http://user:s3cretpw@gems.example.com").is_err());

        // No credential, no exposure: a plain-HTTP public mirror is still fine.
        assert!(parse_source("http://gems.example.com").unwrap().is_some());
        // And a registry on this machine never puts it on a network, which is
        // how a local one is tested.
        for host in [
            "http://ghp_tok3n@127.0.0.1:8080",
            "http://ghp_tok3n@localhost:8080",
            "http://ghp_tok3n@[::1]:8080",
        ] {
            assert!(parse_source(host).unwrap().is_some(), "{host}");
        }
        assert!(
            parse_source("https://ghp_tok3n@gems.example.com")
                .unwrap()
                .is_some()
        );
    }

    /// Redaction is substring replacement over every later log line, so a very
    /// short pattern would blank out unrelated text everywhere. Leaving a
    /// two-character value unmasked is the lesser harm, and nothing a registry
    /// issues as a token is that short.
    #[test]
    fn a_short_credential_is_still_redacted_through_its_userinfo() {
        let short = parse_source("https://ab@gems.example.com")
            .unwrap()
            .unwrap();
        assert_eq!(source_secrets(&short), vec!["ab@".to_string()]);
        assert!(
            !source_secrets(&short).contains(&"ab".to_string()),
            "a two-character pattern would blank out unrelated text"
        );
    }

    /// Whatever is registered has to actually mask the URL as it is rendered,
    /// since that is how the credential reaches a log line or an argv.
    #[test]
    fn the_registered_patterns_mask_the_rendered_url() {
        for raw in [
            "https://ab@gems.example.com/acme",
            "https://user:pw@gems.example.com/acme",
            "https://ghp_tok3n@gems.example.com/acme",
        ] {
            let url = parse_source(raw).unwrap().unwrap();
            let secret = url
                .password()
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| url.username())
                .to_string();
            let masked =
                crate::redactions::Redactor::new(source_secrets(&url)).redact(url.as_str());
            assert!(!masked.contains(&secret), "{raw} -> {masked}");
        }
    }

    #[test]
    fn a_source_must_be_http_or_https() {
        assert!(parse_source("file:///tmp/gems").is_err());
        assert!(parse_source("gems.example.com").is_err(), "no scheme");
    }

    /// RubyGems authenticates a private source with basic-auth userinfo, so a
    /// token really does live in this URL and must never reach a log line.
    #[test]
    fn credentials_in_a_source_are_collected_for_redaction() {
        let with_password = parse_source("https://user:s3cret-pw@gems.example.com")
            .unwrap()
            .unwrap();
        // The userinfo pattern first, then the bare secret.
        assert_eq!(
            source_secrets(&with_password),
            vec!["user:s3cret-pw@".to_string(), "s3cret-pw".to_string()]
        );

        // GitHub Packages puts the token in the user position with no password.
        let token_only = parse_source("https://ghp_tok3n@rubygems.pkg.github.com/acme")
            .unwrap()
            .unwrap();
        assert_eq!(
            source_secrets(&token_only),
            vec!["ghp_tok3n@".to_string(), "ghp_tok3n".to_string()]
        );

        let anonymous = parse_source("https://gems.example.com").unwrap().unwrap();
        assert!(source_secrets(&anonymous).is_empty());
    }

    /// The parse error names the source, and a value that failed to parse has
    /// not been registered for redaction yet, so it is stripped by hand.
    #[test]
    fn an_unparseable_source_is_named_without_its_credential() {
        assert_eq!(
            redacted("https://ghp_tok3n@gems.example.com/ acme"),
            "https://[redacted]@gems.example.com/ acme"
        );
        // No scheme is the typo that matters: the documented example puts the
        // token in the user position, so this is an easy thing to write, and it
        // fails to parse before the value is registered for redaction.
        assert_eq!(
            redacted("ghp_tok3n@gems.example.com"),
            "[redacted]@gems.example.com"
        );
        assert_eq!(
            redacted("https://user:ghp_tok3n@gems.example.com"),
            "https://[redacted]@gems.example.com"
        );
        assert_eq!(
            redacted("https://gems.example.com"),
            "https://gems.example.com"
        );
        let err = parse_source("ghp_tok3n@gems.example.com")
            .unwrap_err()
            .to_string();
        assert!(!err.contains("ghp_tok3n"), "{err}");
        let err = parse_source("https://ghp_tok3n@gems.example.com:notaport")
            .unwrap_err()
            .to_string();
        assert!(!err.contains("ghp_tok3n"), "{err}");
    }

    /// A version list is only valid for the registry it came from, so `source`
    /// has to reach the cache key. Asserted on the declaration rather than on a
    /// cached file, because the digest is mise's to build: what this backend
    /// owns is naming the option.
    #[test]
    fn the_source_option_partitions_the_remote_version_cache() {
        let backend = GemBackend::from_arg("gem:rubocop".into());
        assert!(
            backend
                .remote_version_listing_tool_option_keys()
                .contains(&"source"),
            "a version list from one registry must not answer for another"
        );
    }

    /// A gem install whose one executable has `shebang` as its first line.
    #[cfg(unix)]
    fn gem_install_with_shebang(install: &Path, shebang: &str) -> std::path::PathBuf {
        let exe = install.join("libexec/bin/rubocop");
        file::create_dir_all(exe.parent().unwrap()).unwrap();
        file::write(&exe, format!("{shebang}\nputs 'hi'\n")).unwrap();
        file::make_executable(&exe).unwrap();
        exe
    }

    #[cfg(unix)]
    #[test]
    fn test_gem_shebangs_follow_the_minor_version_link_in_both_layouts() {
        use crate::backend::static_helpers::{test_identity_install, test_link_install};

        let tmp = tempfile::tempdir().unwrap();
        let installs = tmp.path().join("installs");
        let hashed = test_identity_install(&installs, "core:ruby", "3.3.0", "ruby-aaaaaaaa");
        file::write(hashed.join("bin/ruby"), "").unwrap();
        test_link_install(&installs, "ruby", "3.3.0", "ruby-aaaaaaaa");
        let legacy = installs.join("ruby/3.2.4");
        file::create_dir_all(legacy.join("bin")).unwrap();
        file::write(legacy.join("bin/ruby"), "").unwrap();

        let cases = [
            // Identity layout, recorded by the hashed directory.
            (
                format!("#!{}", hashed.join("bin/ruby").display()),
                format!("#!{}", installs.join("ruby/3.3/bin/ruby").display()),
            ),
            // Identity layout, recorded by the version link, with an interpreter argument.
            (
                format!("#!{} -w", installs.join("ruby/3.3.0/bin/ruby").display()),
                format!("#!{} -w", installs.join("ruby/3.3/bin/ruby").display()),
            ),
            // Legacy layout.
            (
                format!("#!{}", legacy.join("bin/ruby").display()),
                format!("#!{}", installs.join("ruby/3.2/bin/ruby").display()),
            ),
            // A system ruby goes through env.
            (
                "#!/usr/bin/ruby".to_string(),
                "#!/usr/bin/env ruby".to_string(),
            ),
        ];
        for (shebang, expected) in cases {
            let install = tmp.path().join("gem");
            let exe = gem_install_with_shebang(&install, &shebang);
            rewrite_gem_shebangs_in(&installs, &install).unwrap();
            let rewritten = file::read_to_string(&exe).unwrap();
            assert_eq!(
                rewritten.lines().next(),
                Some(expected.as_str()),
                "{shebang}"
            );
            file::remove_all(&install).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_gem_polyglot_fallback_links_the_identity_layout_ruby() {
        use crate::backend::static_helpers::{test_identity_install, test_link_install};

        let tmp = tempfile::tempdir().unwrap();
        let installs = tmp.path().join("installs");
        let hashed = test_identity_install(&installs, "core:ruby", "3.3.0", "ruby-aaaaaaaa");
        file::write(hashed.join("bin/ruby"), "").unwrap();
        test_link_install(&installs, "ruby", "3.3.0", "ruby-aaaaaaaa");

        let install = tmp.path().join("gem");
        gem_install_with_shebang(
            &install,
            &format!("#!{}", hashed.join("bin/ruby").display()),
        );
        create_ruby_symlink_in(&installs, &install).unwrap();

        // The fallback ruby is reached through the version link, not the hashed directory.
        assert_eq!(
            file::resolve_symlink(&install.join("libexec/bin/ruby")).unwrap(),
            Some(installs.join("ruby/3.3.0/bin/ruby"))
        );
    }

    #[test]
    fn test_extract_minor_version() {
        assert_eq!(extract_minor_version("3.1.0"), Some("3.1".to_string()));
        assert_eq!(extract_minor_version("3.2.1"), Some("3.2".to_string()));
        assert_eq!(
            extract_minor_version("3.1.0-preview1"),
            Some("3.1".to_string())
        );
        assert_eq!(extract_minor_version("2.7.8"), Some("2.7".to_string()));
        assert_eq!(extract_minor_version("3"), None);
        assert_eq!(extract_minor_version("latest"), None);
    }
}
