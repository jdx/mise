use crate::types::*;
use crate::utils::*;
use expr::{Context, Environment};
use eyre::Result;
use indexmap::IndexSet;
use itertools::Itertools;
use semver::{Version, VersionReq};
use std::collections::HashMap;

fn matches_version_constraint(version: &str, constraint: &str) -> bool {
    // Extract the clean semver part from the version (remove prefixes like 'v')
    let (_, clean_version) = split_version_prefix(version);

    // Try to parse both the version and constraint
    let Ok(version) = Version::parse(clean_version) else {
        return false;
    };

    let Ok(req) = VersionReq::parse(constraint) else {
        return false;
    };

    req.matches(&version)
}

impl AquaPackage {
    pub fn with_version(mut self, versions: &[&str]) -> AquaPackage {
        self = apply_override(self.clone(), self.version_override(versions));
        if let Some(avo) = self.overrides.clone().into_iter().find(|o| {
            if let (Some(goos), Some(goarch)) = (&o.goos, &o.goarch) {
                goos == os() && goarch == arch()
            } else if let Some(goos) = &o.goos {
                goos == os()
            } else if let Some(goarch) = &o.goarch {
                goarch == arch()
            } else {
                false
            }
        }) {
            self = apply_override(self, &avo.pkg)
        }
        self
    }

    fn version_override(&self, versions: &[&str]) -> &AquaPackage {
        let expressions = versions
            .iter()
            .map(|v| (self.expr_parser(v), self.expr_ctx(v)))
            .collect_vec();
        vec![self]
            .into_iter()
            .chain(self.version_overrides.iter())
            .find(|vo| {
                if vo.version_constraint.is_empty() {
                    true
                } else if vo.version_constraint == "true" {
                    // Special case: "true" always matches
                    true
                } else if vo.version_constraint == "false" {
                    // Special case: "false" never matches
                    false
                } else {
                    // Try expression evaluation first for complex constraints
                    expressions.iter().any(|(expr, ctx)| {
                        expr.eval(&vo.version_constraint, ctx)
                            .unwrap_or(false.into())
                            .as_bool()
                            .unwrap_or(false)
                    })
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
                eprintln!("no asset or url for {}/{}", self.repo_owner, self.repo_name);
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
        if self.asset.is_empty() && self.url_has_path() {
            let asset = self.url.rsplit("/").next().unwrap_or("");
            self.parse_aqua_str(asset, v, &Default::default())
        } else {
            self.parse_aqua_str(&self.asset, v, &Default::default())
        }
    }

    /// Check if the URL has an actual path component (not just a domain)
    fn url_has_path(&self) -> bool {
        if self.url.is_empty() {
            return false;
        }

        // Split by '/' and check if there's actually a path component
        let parts: Vec<&str> = self.url.split('/').collect();

        if self.url.starts_with("http://") || self.url.starts_with("https://") {
            // For HTTP URLs:
            // "https://example.com" -> ["https:", "", "example.com"]
            // "https://example.com/" -> ["https:", "", "example.com", ""]
            // "https://example.com/file.zip" -> ["https:", "", "example.com", "file.zip"]
            // We need at least 4 parts with a non-empty filename
            parts.len() > 3 && parts.get(3).is_some_and(|part| !part.is_empty())
        } else {
            // For URLs without protocol:
            // "example.com" -> ["example.com"]
            // "example.com/" -> ["example.com", ""]
            // "example.com/file.zip" -> ["example.com", "file.zip"]
            // We need at least 2 parts with a non-empty filename
            parts.len() > 1 && parts.get(1).is_some_and(|part| !part.is_empty())
        }
    }

    pub fn asset_strs(&self, v: &str) -> Result<IndexSet<String>> {
        let mut strs = IndexSet::from([self.asset(v)?]);
        if cfg!(target_os = "macos") {
            let mut ctx = HashMap::default();
            ctx.insert("Arch".to_string(), "universal".to_string());
            strs.insert(self.parse_aqua_str(&self.asset, v, &ctx)?);
        } else if cfg!(target_os = "windows") {
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
        if cfg!(target_os = "windows") && self.complete_windows_ext && self.format(v)? == "raw" {
            url.push_str(".exe");
        }
        self.parse_aqua_str(&url, v, &Default::default())
    }

    pub fn parse_aqua_str(
        &self,
        s: &str,
        v: &str,
        overrides: &HashMap<String, String>,
    ) -> Result<String> {
        let os_str = os();
        let mut arch_str = arch();
        if os_str == "darwin" && arch_str == "arm64" && self.rosetta2 {
            arch_str = "amd64";
        }
        if os_str == "windows" && arch_str == "arm64" && self.windows_arm_emulation {
            arch_str = "amd64";
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
            "OS".to_string() => replace(os_str),
            "GOOS".to_string() => replace(os_str),
            "GOARCH".to_string() => replace(arch_str),
            "Arch".to_string() => replace(arch_str),
            "Format".to_string() => replace(&self.format),
        };
        ctx.extend(overrides.clone());
        aqua_template_render(s, &ctx)
    }

    pub fn version_filter_ok(&self, v: &str) -> Result<bool> {
        if let Some(filter) = &self.version_filter {
            // Use the expression evaluation
            let env = self.expr_parser(v);
            let ctx = self.expr_ctx(v);
            match env.eval(filter, &ctx) {
                Ok(result) => {
                    if let Some(expr) = result.as_bool() {
                        return Ok(expr);
                    } else {
                        eprintln!("invalid response from version filter: {}", filter);
                        return Ok(true);
                    }
                }
                Err(_) => {
                    eprintln!("invalid response from version filter: {}", filter);
                    return Ok(true);
                }
            }
        }
        Ok(true)
    }

    fn expr_parser(&self, v: &str) -> Environment {
        let (_, v) = split_version_prefix(v);
        let ver = versions::Versioning::new(v);
        let mut env = Environment::new();
        env.add_function("semver", move |c| {
            if c.args.len() != 1 {
                return Err("semver() takes exactly one argument".to_string().into());
            }
            let requirements = c.args[0]
                .as_string()
                .unwrap()
                .replace(' ', "")
                .split(',')
                .map(versions::Requirement::new)
                .collect::<Vec<_>>();
            if requirements.iter().any(|r| r.is_none()) {
                return Err("invalid semver requirement".to_string().into());
            }
            if let Some(ver) = &ver {
                Ok(requirements
                    .iter()
                    .all(|r| r.clone().is_some_and(|r| r.matches(ver)))
                    .into())
            } else {
                Err("invalid version".to_string().into())
            }
        });
        env
    }

    fn expr_ctx(&self, v: &str) -> Context {
        let mut ctx = Context::default();
        ctx.insert("Version", v);
        ctx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_has_path() {
        let mut package = AquaPackage::default();

        // Test case 1: empty URL
        package.url = String::new();
        assert!(!package.url_has_path());

        // Test case 2: URL with no path (just domain)
        package.url = "https://example.com".to_string();
        assert!(!package.url_has_path());

        // Test case 3: URL with trailing slash (still no path)
        package.url = "https://example.com/".to_string();
        assert!(!package.url_has_path());

        // Test case 4: URL with actual file
        package.url = "https://example.com/file.zip".to_string();
        assert!(package.url_has_path());

        // Test case 5: URL with path and file
        package.url = "https://example.com/path/to/file.zip".to_string();
        assert!(package.url_has_path());

        // Test case 6: GitHub release URL
        package.url =
            "https://github.com/owner/repo/releases/download/v1.0.0/binary-linux-amd64.tar.gz"
                .to_string();
        assert!(package.url_has_path());

        // Test case 7: URL without protocol (should still work)
        package.url = "example.com/file.zip".to_string();
        assert!(package.url_has_path());

        // Test case 8: malformed URL that doesn't follow HTTP convention
        package.url = "not-a-url".to_string();
        assert!(!package.url_has_path());
    }

    #[test]
    fn test_asset_method_url_derivation() {
        let version = "1.0.0";

        // Test case 1: asset field is not empty - should use the asset field
        let mut package = AquaPackage::default();
        package.asset = "custom-asset.tar.gz".to_string();
        package.url = "https://example.com/some/file.zip".to_string();
        assert_eq!(package.asset(version).unwrap(), "custom-asset.tar.gz");

        // Test case 2: asset is empty, URL has a filename - should extract filename
        let mut package = AquaPackage::default();
        package.asset = String::new();
        package.url = "https://example.com/path/to/file.zip".to_string();
        assert_eq!(package.asset(version).unwrap(), "file.zip");

        // Test case 3: asset is empty, URL has no path (just domain) - should use empty asset
        let mut package = AquaPackage::default();
        package.asset = String::new();
        package.url = "https://example.com".to_string();
        assert_eq!(package.asset(version).unwrap(), "");

        // Test case 4: asset is empty, URL has trailing slash - should use empty asset
        let mut package = AquaPackage::default();
        package.asset = String::new();
        package.url = "https://example.com/".to_string();
        assert_eq!(package.asset(version).unwrap(), "");

        // Test case 5: asset is empty, URL has multiple path components - should get last one
        let mut package = AquaPackage::default();
        package.asset = String::new();
        package.url =
            "https://github.com/owner/repo/releases/download/v1.0.0/binary-linux-amd64.tar.gz"
                .to_string();
        assert_eq!(package.asset(version).unwrap(), "binary-linux-amd64.tar.gz");
    }
}
