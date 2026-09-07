use std::path::{Component, Path};

use crate::backend::normalize_idiomatic_contents;
use crate::file;
use crate::github;
use crate::lockfile::PlatformInfo;
use eyre::Result;
use xx::regex;

const RUBYINSTALLER_REPO: &str = "oneclick/rubyinstaller2";

/// Build revision assumed when the release list cannot be consulted. mise used
/// this unconditionally before, so falling back to it keeps offline and
/// API-failure behavior unchanged.
const FALLBACK_BUILD_REVISION: u32 = 1;

/// Check if a Ruby version string is a standard MRI version (starts with a digit).
/// Non-MRI engines like jruby, truffleruby, etc. have prefixed version strings.
pub(super) fn is_mri_version(version: &str) -> bool {
    version.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// The tag prefix shared by a version's RubyInstaller2 releases, which are
/// tagged `RubyInstaller-<version>-<build revision>`.
fn rubyinstaller_tag_prefix(version: &str) -> String {
    format!("RubyInstaller-{version}")
}

/// Build the RubyInstaller2 release tag for a version and build revision.
pub(super) fn rubyinstaller_tag(version: &str, revision: u32) -> String {
    format!("{}-{revision}", rubyinstaller_tag_prefix(version))
}

/// Build the RubyInstaller2 asset filename for a version and build revision.
pub(super) fn rubyinstaller_asset_name(version: &str, revision: u32) -> String {
    // RubyInstaller2 publishes arm and x86 archives too, but mise only installs x64.
    format!("rubyinstaller-{version}-{revision}-x64.7z")
}

/// Build the RubyInstaller2 download URL for a version and build revision.
pub(super) fn rubyinstaller_url(version: &str, revision: u32) -> String {
    let tag = rubyinstaller_tag(version, revision);
    let asset = rubyinstaller_asset_name(version, revision);
    format!("https://github.com/{RUBYINSTALLER_REPO}/releases/download/{tag}/{asset}")
}

/// A resolved RubyInstaller2 download.
#[derive(Debug, Clone)]
pub(super) struct RubyInstallerArtifact {
    pub url: String,
    pub checksum: Option<String>,
    /// e.g. `rubyinstaller-3.4.4-2-x64.7z`. Only the Windows installer downloads
    /// the archive; elsewhere this type is built solely to record lockfile info.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub filename: String,
}

/// Resolve the archive to download for an MRI version.
///
/// RubyInstaller2 republishes a corrected build of the same Ruby version as
/// `-2`, `-3`, … and leaves the superseded `-1` release in place. Pinning `-1`
/// therefore always installs the build that was corrected, so pick the highest
/// build revision instead. See discussion #5227.
pub(super) async fn resolve_rubyinstaller_artifact(version: &str) -> RubyInstallerArtifact {
    if let Some(artifact) = resolve_from_releases(version).await {
        return artifact;
    }
    RubyInstallerArtifact {
        url: rubyinstaller_url(version, FALLBACK_BUILD_REVISION),
        checksum: None,
        filename: rubyinstaller_asset_name(version, FALLBACK_BUILD_REVISION),
    }
}

async fn resolve_from_releases(version: &str) -> Option<RubyInstallerArtifact> {
    let prefix = rubyinstaller_tag_prefix(version);
    // Passing the prefixed tag lets the shared build-revision picker work
    // unchanged, and reuses the release-list cache that `_list_remote_versions`
    // already fills, so this costs no extra request.
    let (release, _) =
        github::get_release_with_build_revision_status(RUBYINSTALLER_REPO, &prefix, true)
            .await
            .ok()?;
    let revision: u32 = release
        .tag_name
        .strip_prefix(&format!("{prefix}-"))?
        .parse()
        .ok()?;
    let filename = rubyinstaller_asset_name(version, revision);
    let asset = release.assets.iter().find(|a| a.name == filename)?;
    Some(RubyInstallerArtifact {
        url: asset.browser_download_url.clone(),
        checksum: asset.digest.clone(),
        filename,
    })
}

/// Resolve RubyInstaller2 binary URL and checksum from GitHub releases.
/// Returns `Ok(PlatformInfo::default())` for non-MRI versions since
/// RubyInstaller2 only distributes standard MRI Ruby.
#[cfg_attr(windows, allow(dead_code))]
pub(super) async fn resolve_rubyinstaller_lock_info(version: &str) -> Result<PlatformInfo> {
    if !is_mri_version(version) {
        return Ok(PlatformInfo::default());
    }

    // Resolve through the same path the installer uses so a lockfile records the
    // archive that would actually be downloaded.
    let artifact = resolve_rubyinstaller_artifact(version).await;
    Ok(PlatformInfo {
        url: Some(artifact.url),
        checksum: artifact.checksum,
        size: None,
        url_api: None,
        conda_deps: None,
        ..Default::default()
    })
}

/// Parse a Bundler `Gemfile` for a ruby version request.
///
/// Supports `ruby "3.3.6"`, engine options, and Bundler's
/// `ruby file: ".ruby-version"` (also `:file =>`). The referenced file is
/// resolved relative to the Gemfile and must stay in that directory.
pub(super) fn parse_gemfile(path: &Path) -> Result<String> {
    Ok(parse_gemfile_contents(
        &file::read_to_string(path)?,
        Some(path),
    ))
}

fn parse_gemfile_contents(body: &str, gemfile_path: Option<&Path>) -> String {
    let line = gemfile_ruby_directive(body).unwrap_or_default();
    let line = if let Some(filename) = ruby_file_option(&line) {
        let Some(file_version) =
            gemfile_path.and_then(|path| read_ruby_file_option(path, &filename))
        else {
            return String::new();
        };
        rewrite_ruby_line_with_file_version(&line, &file_version)
    } else {
        line
    };
    let v = line
        .replace("engine:", ":engine =>")
        .replace("engine_version:", ":engine_version =>");
    let v = regex!(r#".*:engine *=> *['"](?<engine>[^'"]*).*:engine_version *=> *['"](?<engine_version>[^'"]*).*"#).replace_all(&v, "${engine_version}__ENGINE__${engine}").to_string();
    let v = regex!(r#".*:engine_version *=> *['"](?<engine_version>[^'"]*).*:engine *=> *['"](?<engine>[^'"]*).*"#).replace_all(&v, "${engine_version}__ENGINE__${engine}").to_string();
    let v = regex!(r#" *ruby *['"]([^'"]*).*"#)
        .replace_all(&v, "$1")
        .to_string();
    let v = regex!(r#"^[^0-9]"#).replace_all(&v, "").to_string();
    let v = regex!(r#"(.*)__ENGINE__(.*)"#)
        .replace_all(&v, "$2-$1")
        .to_string();
    if !is_ruby_version_string(&v) {
        return String::new();
    }
    v
}

fn gemfile_ruby_directive(body: &str) -> Option<String> {
    body.lines()
        .find(|line| line.trim().starts_with("ruby "))
        .map(|line| strip_ruby_line_comment(line.trim()).to_string())
}

/// Drop a `#` comment unless it sits inside quotes (`file: ".ruby-version#ci"`).
fn strip_ruby_line_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    for (i, c) in line.char_indices() {
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => return line[..i].trim_end(),
            _ => {}
        }
    }
    line
}

fn ruby_file_option(line: &str) -> Option<String> {
    regex!(r#"(?:^|\s)(?:file\s*:|:file\s*=>)\s*['"]([^'"]+)['"]"#)
        .captures(line)
        .map(|caps| caps[1].to_string())
}

fn rewrite_ruby_line_with_file_version(line: &str, version: &str) -> String {
    let stripped = regex!(r#"(?:,\s*)?(?:file\s*:|:file\s*=>)\s*['"][^'"]+['"]"#)
        .replace(line, "")
        .to_string();
    let rest = stripped
        .trim()
        .strip_prefix("ruby")
        .unwrap_or(stripped.trim())
        .trim_start()
        .trim_start_matches(',')
        .trim();
    if rest.is_empty() {
        format!("ruby \"{version}\"")
    } else {
        format!("ruby \"{version}\", {rest}")
    }
}

fn is_safe_relative_ruby_file(filename: &str) -> bool {
    let rel = Path::new(filename);
    !filename.is_empty()
        && !rel.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
}

/// Relative paths from a Gemfile `ruby file:` option, for hook-env watches.
pub(crate) fn gemfile_watch_patterns(gemfile_path: &Path) -> Vec<String> {
    if gemfile_path
        .file_name()
        .is_none_or(|name| name != "Gemfile")
    {
        return vec![];
    }
    let Ok(body) = file::read_to_string(gemfile_path) else {
        return vec![];
    };
    match gemfile_ruby_directive(&body).and_then(|line| ruby_file_option(&line)) {
        Some(filename) if is_safe_relative_ruby_file(&filename) && filename != "Gemfile" => {
            vec![filename]
        }
        _ => vec![],
    }
}

fn read_ruby_file_option(gemfile_path: &Path, filename: &str) -> Option<String> {
    if !is_safe_relative_ruby_file(filename) {
        return None;
    }
    let path = gemfile_path.parent()?.join(filename);
    if path.file_name().is_some_and(|name| name == "Gemfile") {
        return None;
    }
    let version = parse_bundler_ruby_version_file(&file::read_to_string(&path).ok()?);
    if is_ruby_version_string(&version) {
        Some(version)
    } else {
        None
    }
}

/// Bundler `normalize_ruby_file`: `ruby-3.2.2`, `ruby 3.2.2`, `ruby = "3.2.2"`,
/// or a single-line `.ruby-version` body.
fn parse_bundler_ruby_version_file(body: &str) -> String {
    let normalized = normalize_idiomatic_contents(body);
    for line in normalized.lines() {
        if let Some(version) = capture_bundler_ruby_line(line.trim())
            && is_ruby_version_string(&version)
        {
            return version;
        }
    }
    if normalized.trim().lines().nth(1).is_some() {
        return String::new();
    }
    normalized
        .trim()
        .trim_start_matches("ruby-")
        .trim_start_matches('v')
        .to_string()
}

fn capture_bundler_ruby_line(line: &str) -> Option<String> {
    // `ruby-3.2.2` is a version; `ruby-build 2024.1.1` is a different tool.
    if let Some(caps) = regex!(r"^ruby-(\d[\w.]*)$").captures(line) {
        return Some(caps[1].to_string());
    }
    let caps =
        regex!(r#"^ruby(?:\s*=\s*|\s+)(?:"([^"]+)"|'([^']+)'|([^\s#"']+))"#).captures(line)?;
    Some(
        caps.get(1)
            .or_else(|| caps.get(2))
            .or_else(|| caps.get(3))?
            .as_str()
            .to_string(),
    )
}

fn is_ruby_version_string(version: &str) -> bool {
    // optional engine prefix, one or more numeric segments: "3.0.0", "3.4.10",
    // "ruby-3.0.0", "jruby-9.4.12.0"
    regex!(r"^(\w+-)?\d+(\.\d+)*$").is_match(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file;
    use pretty_assertions::assert_eq;

    #[test]
    fn tag_and_asset_names_use_the_resolved_revision() {
        assert_eq!(rubyinstaller_tag("3.4.4", 2), "RubyInstaller-3.4.4-2");
        assert_eq!(
            rubyinstaller_asset_name("3.4.4", 2),
            "rubyinstaller-3.4.4-2-x64.7z"
        );
        assert_eq!(
            rubyinstaller_url("3.4.4", 2),
            "https://github.com/oneclick/rubyinstaller2/releases/download/RubyInstaller-3.4.4-2/rubyinstaller-3.4.4-2-x64.7z"
        );
    }

    #[test]
    fn fallback_keeps_the_previous_revision_one_urls() {
        assert_eq!(
            rubyinstaller_url("3.4.4", FALLBACK_BUILD_REVISION),
            "https://github.com/oneclick/rubyinstaller2/releases/download/RubyInstaller-3.4.4-1/rubyinstaller-3.4.4-1-x64.7z"
        );
    }

    #[test]
    fn tag_prefix_does_not_match_a_longer_patch_version() {
        // `3.4.4` must not pick up `3.4.10` releases when tags are compared by prefix.
        let prefix = format!("{}-", rubyinstaller_tag_prefix("3.4.4"));
        assert!(rubyinstaller_tag("3.4.4", 2).starts_with(&prefix));
        assert!(!rubyinstaller_tag("3.4.10", 1).starts_with(&prefix));
    }

    #[test]
    fn is_mri_version_rejects_named_engines() {
        assert!(is_mri_version("3.4.4"));
        assert!(!is_mri_version("jruby-9.4.0.0"));
        assert!(!is_mri_version("truffleruby-24.1.1"));
    }

    #[test]
    fn parse_gemfile_literals() {
        assert_eq!(parse_gemfile_contents("ruby '2.7.2'\n", None), "2.7.2");
        assert_eq!(parse_gemfile_contents("ruby \"3.4.10\"\n", None), "3.4.10");
        assert_eq!(parse_gemfile_contents("ruby \"4.0.6\"\n", None), "4.0.6");
        assert_eq!(
            parse_gemfile_contents(
                "ruby '1.9.3', engine: 'jruby', engine_version: \"1.6.7\"\n",
                None
            ),
            "jruby-1.6.7"
        );
        assert_eq!(
            parse_gemfile_contents(
                "ruby '1.9.3', :engine => 'jruby', :engine_version => '1.6.7'\n",
                None
            ),
            "jruby-1.6.7"
        );
        assert_eq!(
            parse_gemfile_contents(
                "ruby '1.9.3', :engine_version => '1.6.7', :engine => 'jruby'\n",
                None
            ),
            "jruby-1.6.7"
        );
        assert_eq!(
            parse_gemfile_contents(
                "ruby \"3.3.0\", engine: \"jruby\", engine_version: \"9.4.12.0\"\n",
                None
            ),
            "jruby-9.4.12.0"
        );
        assert_eq!(
            parse_gemfile_contents(
                "source \"https://rubygems.org\"\nruby File.read(File.expand_path(\".ruby-version\", __dir__)).strip\n",
                None
            ),
            ""
        );
    }

    #[test]
    fn parse_gemfile_file_option_reads_ruby_version() {
        let dir = tempfile::tempdir().unwrap();
        let gemfile = dir.path().join("Gemfile");
        file::write(dir.path().join(".ruby-version"), "3.3.6\n").unwrap();
        file::write(
            &gemfile,
            "source \"https://rubygems.org\"\nruby file: \".ruby-version\"\n",
        )
        .unwrap();
        assert_eq!(parse_gemfile(&gemfile).unwrap(), "3.3.6");
    }

    #[test]
    fn parse_gemfile_file_option_accepts_hash_rocket_and_single_quotes() {
        let dir = tempfile::tempdir().unwrap();
        let gemfile = dir.path().join("Gemfile");
        file::write(dir.path().join(".ruby-version"), "ruby-3.4.10\n").unwrap();
        file::write(&gemfile, "ruby :file => '.ruby-version'\n").unwrap();
        assert_eq!(parse_gemfile(&gemfile).unwrap(), "3.4.10");
    }

    #[test]
    fn parse_gemfile_file_option_reads_tool_versions_and_mise_toml() {
        let dir = tempfile::tempdir().unwrap();
        file::write(
            dir.path().join(".tool-versions"),
            "nodejs 20\nruby 3.3.6 # comment\n",
        )
        .unwrap();
        file::write(dir.path().join("mise.toml"), "[tools]\nruby = \"4.0.6\"\n").unwrap();

        let gemfile = dir.path().join("Gemfile");
        file::write(&gemfile, "ruby file: \".tool-versions\"\n").unwrap();
        assert_eq!(parse_gemfile(&gemfile).unwrap(), "3.3.6");

        file::write(&gemfile, "ruby file: \"mise.toml\"\n").unwrap();
        assert_eq!(parse_gemfile(&gemfile).unwrap(), "4.0.6");
    }

    #[test]
    fn parse_gemfile_file_option_rejects_parent_dir_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let gemfile = dir.path().join("Gemfile");
        file::write(&gemfile, "ruby file: \"../.ruby-version\"\n").unwrap();
        assert_eq!(parse_gemfile(&gemfile).unwrap(), "");

        file::write(&gemfile, "ruby file: \".ruby-version\"\n").unwrap();
        assert_eq!(parse_gemfile(&gemfile).unwrap(), "");
    }

    #[test]
    fn parse_gemfile_file_option_applies_engine_options() {
        let dir = tempfile::tempdir().unwrap();
        let gemfile = dir.path().join("Gemfile");
        file::write(dir.path().join(".ruby-version"), "1.9.3\n").unwrap();
        file::write(
            &gemfile,
            "ruby file: \".ruby-version\", engine: \"jruby\", engine_version: \"1.6.7\"\n",
        )
        .unwrap();
        assert_eq!(parse_gemfile(&gemfile).unwrap(), "jruby-1.6.7");
    }

    #[test]
    fn parse_gemfile_file_option_skips_hyphenated_tools() {
        let dir = tempfile::tempdir().unwrap();
        let gemfile = dir.path().join("Gemfile");
        file::write(
            dir.path().join(".tool-versions"),
            "ruby-build 2024.1.1\nruby 3.3.6\n",
        )
        .unwrap();
        file::write(&gemfile, "ruby file: \".tool-versions\"\n").unwrap();
        assert_eq!(parse_gemfile(&gemfile).unwrap(), "3.3.6");
    }

    #[test]
    fn parse_gemfile_file_option_keeps_hash_in_quoted_filename() {
        let dir = tempfile::tempdir().unwrap();
        let gemfile = dir.path().join("Gemfile");
        file::write(dir.path().join(".ruby-version#ci"), "3.3.6\n").unwrap();
        file::write(&gemfile, "ruby file: \".ruby-version#ci\" # comment\n").unwrap();
        assert_eq!(parse_gemfile(&gemfile).unwrap(), "3.3.6");
    }

    #[test]
    fn gemfile_watch_patterns_include_referenced_file() {
        let dir = tempfile::tempdir().unwrap();
        let gemfile = dir.path().join("Gemfile");
        file::write(&gemfile, "ruby file: \".ruby-version\"\n").unwrap();
        assert_eq!(
            gemfile_watch_patterns(&gemfile),
            vec![".ruby-version".to_string()]
        );
        assert!(gemfile_watch_patterns(&dir.path().join(".ruby-version")).is_empty());
    }
}
