//! Source-build SBOM package identities matching Homebrew's supported PURLs.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Value, json};
use url::Url;

pub(super) fn source_external_refs(
    tap_name: &str,
    formula_name: &str,
    version: &str,
    source_url: &str,
) -> Vec<Value> {
    let mut purls = vec![brew_purl(tap_name, formula_name, version)];
    if let Some(upstream) = registry_purl(source_url) {
        purls.push(upstream);
    }
    purls
        .into_iter()
        .map(|purl| {
            json!({
                "referenceCategory": "PACKAGE-MANAGER",
                "referenceLocator": purl,
                "referenceType": "purl",
            })
        })
        .collect()
}

fn brew_purl(tap_name: &str, formula_name: &str, version: &str) -> String {
    format!(
        "pkg:brew/{}/{}@{}",
        encode_segments(tap_name),
        encode(formula_name),
        encode(version)
    )
}

fn registry_purl(source_url: &str) -> Option<String> {
    let url = Url::parse(source_url).ok()?;
    let host = url.host_str()?;
    let segments = url
        .path_segments()?
        .map(decode)
        .collect::<Option<Vec<_>>>()?;
    let basename = segments.last()?.as_str();

    match host {
        "files.pythonhosted.org"
            if segments.len() == 5
                && segments
                    .first()
                    .is_some_and(|segment| segment == "packages")
                && !basename.ends_with(".whl") =>
        {
            let stem = strip_archive_extension(basename)?;
            let (name, version) = stem.rsplit_once('-')?;
            nonempty_purl("pypi", None, name, version)
        }
        "registry.npmjs.org" => {
            let dash = segments.iter().position(|segment| segment == "-")?;
            let (namespace, name) = match &segments[..dash] {
                [scope, name] if scope.starts_with('@') => (Some(scope.as_str()), name.as_str()),
                [name] if !name.starts_with(['@', '%']) => (None, name.as_str()),
                _ => return None,
            };
            let stem = strip_archive_extension(basename)?;
            let version = stem.strip_prefix(&format!("{name}-"))?;
            nonempty_purl("npm", namespace, name, version)
        }
        "static.crates.io"
            if segments.len() == 3
                && segments.first().is_some_and(|segment| segment == "crates") =>
        {
            let name = segments.get(1)?;
            let stem = strip_archive_extension(basename)?;
            let version = stem.strip_prefix(&format!("{name}-"))?;
            nonempty_purl("cargo", None, name, version)
        }
        "repo.hex.pm"
            if segments.len() == 2
                && segments
                    .first()
                    .is_some_and(|segment| segment == "tarballs") =>
        {
            let stem = strip_archive_extension(basename)?;
            let (name, version) = stem.split_once('-')?;
            nonempty_purl("hex", None, name, version)
        }
        "rubygems.org"
            if matches!(
                segments.first().map(String::as_str),
                Some("downloads" | "gems")
            ) =>
        {
            let stem = basename.strip_suffix(".gem")?;
            let stem = GEM_PLATFORM_SUFFIX.replace(stem, "");
            let (name, version) = stem.rsplit_once('-')?;
            if !GEM_VERSION.is_match(version) {
                return None;
            }
            nonempty_purl("gem", None, name, version)
        }
        "hackage.haskell.org" if segments.first().is_some_and(|segment| segment == "package") => {
            let package_id = segments.get(1)?;
            let captures = HACKAGE_PACKAGE_ID.captures(package_id)?;
            nonempty_purl(
                "hackage",
                None,
                captures.get(1)?.as_str(),
                captures.get(2)?.as_str(),
            )
        }
        _ if cpan_author(&segments).is_some() => {
            let author = cpan_author(&segments)?;
            let stem = strip_archive_extension(basename)?;
            let captures = CPAN_DISTNAME.captures(stem)?;
            nonempty_purl(
                "cpan",
                Some(author),
                captures.get(1)?.as_str(),
                captures.get(2)?.as_str(),
            )
        }
        "repo1.maven.org" | "repo.maven.apache.org"
            if segments.first().is_some_and(|segment| segment == "maven2") =>
        {
            maven_purl(segments.get(1..)?)
        }
        "search.maven.org" if url.path() == "/remotecontent" => {
            let filepath = url
                .query_pairs()
                .find_map(|(key, value)| (key == "filepath").then(|| value.into_owned()))?;
            maven_purl(
                &filepath
                    .split('/')
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
            )
        }
        "cran.r-project.org" | "cloud.r-project.org" => {
            let contribution = segments
                .windows(2)
                .any(|pair| pair[0] == "src" && pair[1] == "contrib");
            if !contribution || !basename.ends_with(".tar.gz") {
                return None;
            }
            let stem = basename.strip_suffix(".tar.gz")?;
            let (name, version) = stem.split_once('_')?;
            nonempty_purl("cran", None, name, version)
        }
        "api.nuget.org"
            if segments
                .first()
                .is_some_and(|segment| segment == "v3-flatcontainer") =>
        {
            nonempty_purl("nuget", None, segments.get(1)?, segments.get(2)?)
        }
        "www.nuget.org"
            if segments.first().is_some_and(|segment| segment == "api")
                && segments.get(1).is_some_and(|segment| segment == "v2")
                && segments.get(2).is_some_and(|segment| segment == "package") =>
        {
            nonempty_purl("nuget", None, segments.get(3)?, segments.get(4)?)
        }
        _ => None,
    }
}

fn nonempty_purl(
    package_type: &str,
    namespace: Option<&str>,
    name: &str,
    version: &str,
) -> Option<String> {
    if name.is_empty() || version.is_empty() {
        return None;
    }
    let package_type = package_type.to_ascii_lowercase();
    let mut namespace = namespace.map(ToString::to_string);
    let mut name = name.to_string();
    match package_type.as_str() {
        "pypi" => name = name.to_ascii_lowercase().replace('_', "-"),
        "hex" => {
            namespace = namespace.map(|value| value.to_ascii_lowercase());
            name = name.to_ascii_lowercase();
        }
        "cpan" => namespace = namespace.map(|value| value.to_ascii_uppercase()),
        _ => {}
    }
    let namespace = namespace
        .as_deref()
        .map(|namespace| format!("{}/", encode_segments(namespace)))
        .unwrap_or_default();
    Some(format!(
        "pkg:{package_type}/{namespace}{}@{}",
        encode(&name),
        encode(version)
    ))
}

static GEM_PLATFORM_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"-(?:java|jruby|truffleruby|dalvik|dotnet|mswin\d+(?:_\d+)?|\w+-(?:aix|cygwin|darwin|freebsd|linux|macruby|mingw\w*|mswin\d*|netbsd\w*|openbsd|bitrig|solaris|wasi)(?:[-_][\w.]+)?)$",
    )
    .unwrap()
});
static GEM_VERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d[\w.]*$").unwrap());
static HACKAGE_PACKAGE_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.+)-(\d+(?:\.\d+)*)$").unwrap());
static CPAN_DISTNAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.+)-(v?\d[\d._]*)(?:-TRIAL\d*)?$").unwrap());
static CPAN_AUTHOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Z][A-Z0-9-]+$").unwrap());

fn cpan_author(segments: &[String]) -> Option<&str> {
    let authors = segments
        .windows(2)
        .position(|pair| pair[0] == "authors" && pair[1] == "id")?;
    let first = segments.get(authors + 2)?;
    let prefix = segments.get(authors + 3)?;
    let author = segments.get(authors + 4)?;
    if first.len() == 1
        && prefix.len() == 2
        && first
            .chars()
            .all(|character| character.is_ascii_uppercase())
        && prefix
            .chars()
            .all(|character| character.is_ascii_uppercase())
        && CPAN_AUTHOR.is_match(author)
    {
        Some(author)
    } else {
        None
    }
}

fn maven_purl(segments: &[String]) -> Option<String> {
    if segments.len() < 4 {
        return None;
    }
    let artifact_index = segments.len() - 3;
    let artifact = segments.get(artifact_index)?;
    let version = segments.get(artifact_index + 1)?;
    let filename = segments.get(artifact_index + 2)?;
    let prefix = format!("{artifact}-{version}");
    let suffix = filename.strip_prefix(&prefix)?;
    if !matches!(suffix.as_bytes().first(), Some(b'.' | b'-')) {
        return None;
    }
    let namespace = segments.get(..artifact_index)?.join(".");
    nonempty_purl("maven", Some(&namespace), artifact, version)
}

fn strip_archive_extension(filename: &str) -> Option<&str> {
    [
        ".tar.gz", ".tar.bz2", ".tar.xz", ".tgz", ".zip", ".gem", ".crate", ".tar", ".nupkg",
    ]
    .into_iter()
    .find_map(|extension| filename.strip_suffix(extension))
}

fn decode(segment: &str) -> Option<String> {
    urlencoding::decode(segment)
        .ok()
        .map(|value| value.into_owned())
}

fn encode_segments(value: &str) -> String {
    value.split('/').map(encode).collect::<Vec<_>>().join("/")
}

fn encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b':') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(&mut encoded, "%{byte:02X}").expect("writing to String cannot fail");
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_refs_bind_canonical_tap_and_supported_registry() {
        let refs = source_external_refs(
            "homebrew/core",
            "python@3.12",
            "3.12.8",
            "https://files.pythonhosted.org/packages/00/00/00/requests-2.25.1.tar.gz",
        );
        assert_eq!(
            refs[0]["referenceLocator"],
            "pkg:brew/homebrew/core/python%403.12@3.12.8"
        );
        assert_eq!(refs[1]["referenceLocator"], "pkg:pypi/requests@2.25.1");
    }

    #[test]
    fn purl_normalization_and_encoding_match_homebrew() {
        assert_eq!(
            nonempty_purl("PyPI", None, "Types_Setuptools", "1.0+build").as_deref(),
            Some("pkg:pypi/types-setuptools@1.0%2Bbuild")
        );
        assert_eq!(
            nonempty_purl("Hex", Some("Acme"), "Phoenix", "1.7.0-rc.0").as_deref(),
            Some("pkg:hex/acme/phoenix@1.7.0-rc.0")
        );
        assert_eq!(
            nonempty_purl("cpan", Some("abigail"), "Regexp-Common", "2024080801").as_deref(),
            Some("pkg:cpan/ABIGAIL/Regexp-Common@2024080801")
        );
        assert_eq!(
            nonempty_purl("maven", Some("com.example:legacy"), "tool", "1.0").as_deref(),
            Some("pkg:maven/com.example:legacy/tool@1.0")
        );
        assert_eq!(
            encode("Az09-._~:@/ +café"),
            "Az09-._~:%40%2F%20%2Bcaf%C3%A9"
        );
    }

    #[test]
    fn registry_mapping_matches_supported_homebrew_shapes() {
        for (url, expected) in [
            (
                "https://registry.npmjs.org/%40angular/cli/-/cli-22.0.3.tgz",
                "pkg:npm/%40angular/cli@22.0.3",
            ),
            (
                "https://static.crates.io/crates/ripgrep/ripgrep-14.1.1.crate",
                "pkg:cargo/ripgrep@14.1.1",
            ),
            (
                "https://repo.hex.pm/tarballs/plug-1.16.1.tar",
                "pkg:hex/plug@1.16.1",
            ),
            (
                "https://files.pythonhosted.org/packages/00/07/d1/Types_Setuptools-80.9.0.20251223.tar.gz",
                "pkg:pypi/types-setuptools@80.9.0.20251223",
            ),
            (
                "https://repo.hex.pm/tarballs/Phoenix-1.7.0-rc.0.tar",
                "pkg:hex/phoenix@1.7.0-rc.0",
            ),
            (
                "https://cran.r-project.org/src/contrib/data.table_1.15.4.tar.gz",
                "pkg:cran/data.table@1.15.4",
            ),
            (
                "https://api.nuget.org/v3-flatcontainer/newtonsoft.json/13.0.3/newtonsoft.json.13.0.3.nupkg",
                "pkg:nuget/newtonsoft.json@13.0.3",
            ),
            (
                "https://www.nuget.org/api/v2/package/Newtonsoft.Json/13.0.3",
                "pkg:nuget/Newtonsoft.Json@13.0.3",
            ),
            (
                "https://rubygems.org/downloads/nokogiri-1.16.0-arm64-darwin-22.gem",
                "pkg:gem/nokogiri@1.16.0",
            ),
            (
                "https://hackage.haskell.org/package/cabal-install-3.12.1.0/cabal-install-3.12.1.0.tar.gz",
                "pkg:hackage/cabal-install@3.12.1.0",
            ),
            (
                "https://cpan.metacpan.org/authors/id/R/RJ/RJBS/Dist-Zilla-6.032-TRIAL.tar.gz",
                "pkg:cpan/RJBS/Dist-Zilla@6.032",
            ),
            (
                "https://repo.maven.apache.org/maven2/org/gradle/profiler/gradle-profiler/0.24.0/gradle-profiler-0.24.0.zip",
                "pkg:maven/org.gradle.profiler/gradle-profiler@0.24.0",
            ),
            (
                "https://repo1.maven.org/maven2/org/gradle/profiler/gradle-profiler/0.24.0/gradle-profiler-0.24.0.zip",
                "pkg:maven/org.gradle.profiler/gradle-profiler@0.24.0",
            ),
            (
                "https://search.maven.org/remotecontent?filepath=org/gradle/profiler/gradle-profiler/0.24.0/gradle-profiler-0.24.0.zip",
                "pkg:maven/org.gradle.profiler/gradle-profiler@0.24.0",
            ),
        ] {
            assert_eq!(registry_purl(url).as_deref(), Some(expected), "{url}");
        }
    }

    #[test]
    fn unsupported_or_ambiguous_urls_do_not_guess() {
        for url in [
            "https://example.com/foo-1.0.tar.gz",
            "https://files.pythonhosted.org/packages/aa/bb/cc/foo-1.0-py3-none-any.whl",
            "https://registry.npmjs.org/foo/-/bar-1.0.tgz",
            "https://maven.fabricmc.net/net/fabricmc/tool/1.0/tool-1.0.jar",
            "https://repo1.maven.org/arbitrary/org/example/tool/1.0/tool-1.0.jar",
            "https://repo.maven.apache.org/arbitrary/maven2/org/example/tool/1.0/tool-1.0.jar",
        ] {
            assert_eq!(registry_purl(url), None, "{url}");
        }
    }
}
