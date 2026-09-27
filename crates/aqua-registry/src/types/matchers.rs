use super::*;

/// One comma-separated term of an aqua `semver()` constraint, e.g. `<=5.34.1.0` or `!=0.0.45`.
///
/// `versions::Requirement::new` cannot express these. It requires the whole string to parse,
/// and the `Versioning::parse` inside it tries `SemVer` first — so a bound like `5.34.1.0` is
/// consumed as `5.34.1` with `.0` left over and the requirement is rejected outright. It also
/// has no operator for `!=`. Either way the caller only sees `None`, which
/// `AquaPackage::version_override` turns into a silently false constraint.
///
/// aqua evaluates these with `hashicorp/go-version`, which accepts any number of components and
/// supports `!=`, so the operator is split off here and the bound read with `Versioning::new`,
/// which does fall back to `Version` for such strings. The comparison itself is still
/// `versions::Requirement::matches`, so ordering, tilde and caret semantics are unchanged.
pub(super) struct AquaRequirement {
    inner: versions::Requirement,
    /// `!=` has no `versions::Op`, so it is `Exact` read backwards.
    negated: bool,
}

impl AquaRequirement {
    pub(super) fn parse(s: &str) -> Option<Self> {
        if s == "*" {
            return Some(Self {
                inner: versions::Requirement {
                    op: versions::Op::Wildcard,
                    version: None,
                },
                negated: false,
            });
        }
        // Longest operators first, or `>=` would be read as `>` with a leading `=` on the bound.
        let (op, negated, bound) = [
            (">=", versions::Op::GreaterEq, false),
            ("<=", versions::Op::LessEq, false),
            ("!=", versions::Op::Exact, true),
            (">", versions::Op::Greater, false),
            ("<", versions::Op::Less, false),
            ("=", versions::Op::Exact, false),
            ("~", versions::Op::Tilde, false),
            ("^", versions::Op::Caret, false),
        ]
        .into_iter()
        .find_map(|(prefix, op, negated)| {
            s.strip_prefix(prefix).map(|bound| (op, negated, bound))
        })?;
        Some(Self {
            inner: versions::Requirement {
                op,
                version: Some(Versioning::new(bound)?),
            },
            negated,
        })
    }

    pub(super) fn matches(&self, v: &Versioning) -> bool {
        self.inner.matches(v) != self.negated
    }
}

impl AquaOverride {
    pub(super) fn matches(&self, os: &str, arch: &str, runtime: AquaRuntime<'_>) -> bool {
        self.goos.as_ref().is_none_or(|goos| goos == os)
            && self.goarch.as_ref().is_none_or(|goarch| goarch == arch)
            && (self.envs.is_empty() || envs_match(&self.envs, os, arch))
            && self
                .variants
                .iter()
                .all(|variant| variant.matches(os, runtime))
    }
}

impl AquaFormatOverride {
    pub(super) fn matches(&self, os: &str) -> bool {
        self.goos == os
    }
}

fn envs_match(envs: &[String], os: &str, arch: &str) -> bool {
    let os_arch = format!("{os}/{arch}");
    // Aqua env selectors accept GOOS, GOARCH, GOOS/GOARCH, or the wildcard "all".
    envs.iter()
        .any(|env| env == "all" || env == os || env == arch || env == &os_arch)
}

impl AquaVariant {
    fn matches(&self, os: &str, runtime: AquaRuntime<'_>) -> bool {
        match self.key.as_str() {
            "libc" => match (
                normalize_libc(runtime.libc),
                normalize_libc(Some(&self.value)),
            ) {
                (Some(actual), Some(expected)) => os == "linux" && actual == expected,
                _ => false,
            },
            key => {
                log::debug!("unsupported aqua override variant key: {key}");
                false
            }
        }
    }
}

pub(super) fn normalize_libc(libc: Option<&str>) -> Option<&str> {
    match libc? {
        "glibc" | "gnu" => Some("gnu"),
        "musl" => Some("musl"),
        _ => None,
    }
}
/// splits a version number into an optional prefix and the remaining version string
pub(super) fn split_version_prefix(version: &str) -> (String, String) {
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

pub(super) const AQUA_ASSET_FORMATS: &[&str] = &[
    "tar.br", "tar.bz2", "tar.gz", "tar.lz4", "tar.sz", "tar.xz", "tbr", "tbz", "tbz2", "tgz",
    "tlz4", "tsz", "txz", "tar.zst", "zip", "7z", "gz", "bz2", "lz4", "sz", "xz", "zst", "dmg",
    "pkg", "rar", "tar",
];

pub(super) fn asset_without_ext(asset: &str) -> &str {
    AQUA_ASSET_FORMATS
        .iter()
        .find_map(|format| asset.strip_suffix(format)?.strip_suffix('.'))
        .unwrap_or(asset)
}
