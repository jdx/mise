use crate::expressions::*;
use crate::types::*;
use crate::utils::*;
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
        let _expressions = versions
            .iter()
            .map(|v| (self.expr_parser(v), self.expr_ctx(v)))
            .collect_vec();
        vec![self]
            .into_iter()
            .chain(self.version_overrides.iter())
            .find(|vo| {
                if vo.version_constraint.is_empty() {
                    true
                } else {
                    // Check if any of the provided versions match the constraint
                    versions
                        .iter()
                        .any(|version| matches_version_constraint(version, &vo.version_constraint))
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
        if self.asset.is_empty() && self.url.split("/").count() > "//".len() {
            let asset = self.url.rsplit("/").next().unwrap_or("");
            self.parse_aqua_str(asset, v, &Default::default())
        } else {
            self.parse_aqua_str(&self.asset, v, &Default::default())
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

    fn expr_parser(&self, v: &str) -> ExprEnvironment {
        let (_, v) = split_version_prefix(v);
        let ver = versions_versioning_new(v);
        let mut env = expr_environment_new();
        env.add_function("semver", move |c| {
            if c.args.len() != 1 {
                return Err("semver() takes exactly one argument".to_string().into());
            }
            let requirements = c.args[0]
                .as_string()
                .unwrap()
                .replace(' ', "")
                .split(',')
                .map(versions_requirement_new)
                .collect::<Vec<_>>();
            if requirements.iter().any(|r| r.is_none()) {
                return Err("invalid semver requirement".to_string().into());
            }
            if let Some(_ver) = &ver {
                // Simplified stub - return true for now
                Ok(true.into())
            } else {
                Err("invalid version".to_string().into())
            }
        });
        env
    }

    fn expr_ctx(&self, v: &str) -> ExprContext {
        let mut ctx = expr_context_default();
        ctx.insert("Version", v);
        ctx
    }

    pub fn version_filter_ok(&self, v: &str) -> Result<bool> {
        if let Some(filter) = &self.version_filter {
            // Compile and evaluate the expression
            if let Ok(program) = expr_compile(filter) {
                let env = self.expr_parser(v);
                let ctx = self.expr_ctx(v);
                if let Ok(result) = env.run(program, &ctx) {
                    if let Some(bool_val) = result.as_bool() {
                        return Ok(bool_val);
                    } else {
                        eprintln!("invalid response from version filter: {}", filter);
                        return Ok(true);
                    }
                }
            }
        }
        Ok(true)
    }
}
