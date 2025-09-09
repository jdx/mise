use crate::types::*;
use eyre::{eyre, Result};
use indexmap::IndexSet;
use itertools::Itertools;
use std::collections::HashMap;

// Macro helper for creating hashmaps
macro_rules! hashmap {
    (@single $($x:tt)*) => (());
    (@count $($rest:expr),*) => (<[()]>::len(&[$(hashmap!(@single $rest)),*]));

    ($($key:expr => $value:expr,)+) => { hashmap!($($key => $value),+) };
    ($($key:expr => $value:expr),*) => {
        {
            let _cap = hashmap!(@count $($key),*);
            let mut _map = HashMap::with_capacity(_cap);
            $(
                let _ = _map.insert($key, $value);
            )*
            _map
        }
    };
}

impl AquaPackage {
    pub fn with_version(mut self, versions: &[&str]) -> AquaPackage {
        self = apply_override(self.clone(), self.version_override(versions));
        if let Some(avo) = self.overrides.clone().into_iter().find(|o| {
            if let (Some(goos), Some(goarch)) = (&o.goos, &o.goarch) {
                goos == &os() && goarch == &arch()
            } else if let Some(goos) = &o.goos {
                goos == &os()
            } else if let Some(goarch) = &o.goarch {
                goarch == &arch()
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
                } else {
                    // Simplified stub - always return true for now
                    true
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

    fn parse_aqua_str(
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
            if let Some(ver) = &ver {
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

impl AquaFile {
    pub fn src(&self, pkg: &AquaPackage, v: &str) -> Result<Option<String>> {
        let asset = pkg.asset(v)?;
        let asset = asset.strip_suffix(".tar.gz").unwrap_or(&asset);
        let asset = asset.strip_suffix(".tar.xz").unwrap_or(asset);
        let asset = asset.strip_suffix(".tar.bz2").unwrap_or(asset);
        let asset = asset.strip_suffix(".gz").unwrap_or(asset);
        let asset = asset.strip_suffix(".xz").unwrap_or(asset);
        let asset = asset.strip_suffix(".bz2").unwrap_or(asset);
        let asset = asset.strip_suffix(".zip").unwrap_or(asset);
        let asset = asset.strip_suffix(".tar").unwrap_or(asset);
        let asset = asset.strip_suffix(".tgz").unwrap_or(asset);
        let asset = asset.strip_suffix(".txz").unwrap_or(asset);
        let asset = asset.strip_suffix(".tbz2").unwrap_or(asset);
        let asset = asset.strip_suffix(".tbz").unwrap_or(asset);
        let ctx = hashmap! {
            "AssetWithoutExt".to_string() => asset.to_string(),
            "FileName".to_string() => self.name.to_string(),
        };
        self.src
            .as_ref()
            .map(|src| pkg.parse_aqua_str(src, v, &ctx))
            .transpose()
    }
}

impl AquaChecksum {
    pub fn _type(&self) -> &AquaChecksumType {
        self.r#type.as_ref().unwrap()
    }

    pub fn algorithm(&self) -> &AquaChecksumAlgorithm {
        self.algorithm.as_ref().unwrap()
    }

    pub fn asset_strs(&self, pkg: &AquaPackage, v: &str) -> Result<IndexSet<String>> {
        let mut asset_strs = IndexSet::new();
        for asset in pkg.asset_strs(v)? {
            let checksum_asset = self.asset.as_ref().unwrap();
            let ctx = hashmap! {
                "Asset".to_string() => asset.to_string(),
            };
            asset_strs.insert(pkg.parse_aqua_str(checksum_asset, v, &ctx)?);
        }
        Ok(asset_strs)
    }

    pub fn pattern(&self) -> &AquaChecksumPattern {
        self.pattern.as_ref().unwrap()
    }

    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    pub fn file_format(&self) -> &str {
        self.file_format.as_deref().unwrap_or("raw")
    }

    pub fn url(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        pkg.parse_aqua_str(self.url.as_ref().unwrap(), v, &Default::default())
    }
}

impl AquaCosign {
    pub fn opts(&self, pkg: &AquaPackage, v: &str) -> Result<Vec<String>> {
        self.opts
            .iter()
            .map(|opt| pkg.parse_aqua_str(opt, v, &Default::default()))
            .collect()
    }
}

impl AquaCosignSignature {
    pub fn url(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        pkg.parse_aqua_str(self.url.as_ref().unwrap(), v, &Default::default())
    }

    pub fn asset(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        pkg.parse_aqua_str(self.asset.as_ref().unwrap(), v, &Default::default())
    }

    pub fn arg(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        match self.r#type.as_deref().unwrap_or_default() {
            "github_release" => {
                let asset = self.asset(pkg, v)?;
                let repo_owner = self
                    .repo_owner
                    .clone()
                    .unwrap_or_else(|| pkg.repo_owner.clone());
                let repo_name = self
                    .repo_name
                    .clone()
                    .unwrap_or_else(|| pkg.repo_name.clone());
                let repo = format!("{repo_owner}/{repo_name}");
                Ok(format!(
                    "https://github.com/{repo}/releases/download/{v}/{asset}"
                ))
            }
            "http" => self.url(pkg, v),
            t => {
                eprintln!(
                    "unsupported cosign signature type for {}/{}: {t}",
                    pkg.repo_owner, pkg.repo_name
                );
                Ok("".to_string())
            }
        }
    }
}

impl AquaSlsaProvenance {
    pub fn asset(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        pkg.parse_aqua_str(self.asset.as_ref().unwrap(), v, &Default::default())
    }

    pub fn url(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        pkg.parse_aqua_str(self.url.as_ref().unwrap(), v, &Default::default())
    }
}

impl AquaMinisign {
    pub fn _type(&self) -> &AquaMinisignType {
        self.r#type.as_ref().unwrap()
    }

    pub fn url(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        pkg.parse_aqua_str(self.url.as_ref().unwrap(), v, &Default::default())
    }

    pub fn asset(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        pkg.parse_aqua_str(self.asset.as_ref().unwrap(), v, &Default::default())
    }

    pub fn public_key(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        pkg.parse_aqua_str(self.public_key.as_ref().unwrap(), v, &Default::default())
    }
}

fn apply_override(mut orig: AquaPackage, avo: &AquaPackage) -> AquaPackage {
    if avo.r#type != AquaPackageType::GithubRelease {
        orig.r#type = avo.r#type.clone();
    }
    if !avo.repo_owner.is_empty() {
        orig.repo_owner = avo.repo_owner.clone();
    }
    if !avo.repo_name.is_empty() {
        orig.repo_name = avo.repo_name.clone();
    }
    if !avo.asset.is_empty() {
        orig.asset = avo.asset.clone();
    }
    if !avo.url.is_empty() {
        orig.url = avo.url.clone();
    }
    if !avo.format.is_empty() {
        orig.format = avo.format.clone();
    }
    if avo.rosetta2 {
        orig.rosetta2 = true;
    }
    if avo.windows_arm_emulation {
        orig.windows_arm_emulation = true;
    }
    if !avo.complete_windows_ext {
        orig.complete_windows_ext = false;
    }
    if !avo.supported_envs.is_empty() {
        orig.supported_envs = avo.supported_envs.clone();
    }
    if !avo.files.is_empty() {
        orig.files = avo.files.clone();
    }
    orig.replacements.extend(avo.replacements.clone());
    if let Some(avo_version_prefix) = avo.version_prefix.clone() {
        orig.version_prefix = Some(avo_version_prefix);
    }
    if !avo.overrides.is_empty() {
        orig.overrides = avo.overrides.clone();
    }
    orig
}

// Platform detection helpers
fn os() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    }
}

fn arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "unknown"
    }
}

// Stub implementations for external dependencies that need to be implemented
fn aqua_template_render(template: &str, _ctx: &HashMap<String, String>) -> Result<String> {
    // For now, just return the template as-is - would need actual templating logic
    Ok(template.to_string())
}

fn split_version_prefix(v: &str) -> (&str, &str) {
    // Simple stub - would need actual version prefix splitting logic
    ("", v)
}

fn versions_versioning_new(_v: &str) -> Option<()> {
    // Stub for versions crate integration
    None
}

fn versions_requirement_new(_req: &str) -> Option<()> {
    // Stub for versions crate integration
    None
}

fn expr_environment_new() -> ExprEnvironment {
    // Stub for expr crate integration
    ExprEnvironment
}

fn expr_context_default() -> ExprContext {
    // Stub for expr crate integration
    ExprContext
}

fn expr_compile(_expr: &str) -> Result<ExprProgram> {
    // Stub for expr crate integration
    Err(eyre!("Expression compilation not implemented"))
}

// Stub types for expr integration
struct ExprEnvironment;
struct ExprContext;
struct ExprProgram;

impl ExprEnvironment {
    fn add_function<F>(&mut self, _name: &str, _func: F)
    where
        F: Fn(&ExprCallContext) -> Result<ExprValue, Box<dyn std::error::Error>> + 'static,
    {
    }

    fn run(&self, _program: ExprProgram, _ctx: &ExprContext) -> Result<ExprValue> {
        Err(eyre!("Expression evaluation not implemented"))
    }
}

impl ExprContext {
    fn insert(&mut self, _key: &str, _value: &str) {}
}

struct ExprCallContext {
    args: Vec<ExprValue>,
}

#[derive(Clone)]
enum ExprValue {
    Bool(bool),
    String(String),
}

impl ExprValue {
    fn as_bool(&self) -> Option<bool> {
        match self {
            ExprValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    fn as_string(&self) -> Option<String> {
        match self {
            ExprValue::String(s) => Some(s.clone()),
            _ => None,
        }
    }
}

impl From<bool> for ExprValue {
    fn from(b: bool) -> Self {
        ExprValue::Bool(b)
    }
}
