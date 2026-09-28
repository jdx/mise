use super::*;

impl AquaPackage {
    /// Detect the format of an archive based on its filename
    fn detect_format(&self, asset_name: &str) -> &'static str {
        for &format in AQUA_ASSET_FORMATS {
            if asset_name.ends_with(&format!(".{format}")) {
                return format;
            }
        }
        "raw"
    }

    fn append_ext_enabled(&self) -> bool {
        self.append_ext.unwrap_or(true)
    }

    pub fn windows_ext(&self) -> &str {
        if self.windows_ext.is_empty() {
            match self.package_type() {
                AquaPackageType::GithubArchive | AquaPackageType::GithubContent => ".sh",
                _ => ".exe",
            }
        } else {
            &self.windows_ext
        }
    }

    pub fn complete_windows_ext_enabled(&self) -> bool {
        match self.complete_windows_ext {
            Some(complete) => complete,
            None => !matches!(
                self.package_type(),
                AquaPackageType::GithubArchive | AquaPackageType::GithubContent
            ),
        }
    }

    fn complete_windows_ext(&self, s: &str) -> String {
        if self.complete_windows_ext_enabled() {
            append_str_ext(s, self.windows_ext())
        } else {
            s.to_string()
        }
    }

    fn os_file_ext_is_empty(&self, s: &str, version: &str) -> bool {
        let filename = s.rsplit('/').next().unwrap_or_default();
        file_ext_is_empty(filename, version)
    }

    fn os_file_ext(&self, s: &str, version: &str) -> Option<String> {
        let filename = s.rsplit('/').next().unwrap_or_default();
        file_ext(filename, version)
    }

    fn append_ext(&self, s: String) -> String {
        if !self.append_ext_enabled() || self.format.is_empty() || self.format == "raw" {
            return s;
        }
        if self.detect_format(&s) != "raw" || s.ends_with(&format!(".{}", self.format)) {
            return s;
        }
        format!("{}.{}", s, self.format)
    }

    fn asset_without_appended_ext(
        &self,
        v: &str,
        overrides: &HashMap<String, String>,
        os: &str,
        arch: &str,
    ) -> Result<String> {
        if self.asset.is_empty() && self.url.split('/').count() > "//".len() {
            let asset = self.url.rsplit('/').next().unwrap_or("");
            self.parse_aqua_str(asset, v, overrides, os, arch)
        } else {
            self.parse_aqua_str(&self.asset, v, overrides, os, arch)
        }
    }

    fn finish_asset(&self, asset: String, v: &str, os: &str) -> Result<String> {
        let asset = self.append_ext(asset);
        self.complete_windows_ext_to_asset(&asset, v, os)
    }

    /// Get the format for this package and version
    pub fn format(&self, v: &str, os: &str, arch: &str) -> Result<&str> {
        if self.package_type() == AquaPackageType::GithubArchive {
            return Ok("tar.gz");
        }
        let format = if self.format.is_empty() {
            let asset = if !self.asset.is_empty() {
                self.asset_without_appended_ext(v, &Default::default(), os, arch)?
            } else if !self.url.is_empty() {
                self.parse_aqua_str(&self.url, v, &Default::default(), os, arch)?
            } else {
                log::debug!("no asset or url for {}/{}", self.repo_owner, self.repo_name);
                String::new()
            };
            self.detect_format(&asset)
        } else {
            &self.format
        };
        Ok(format)
    }

    /// Get the asset name for this package and version
    pub fn asset(&self, v: &str, os: &str, arch: &str) -> Result<String> {
        let asset = self.asset_without_appended_ext(v, &Default::default(), os, arch)?;
        self.finish_asset(asset, v, os)
    }

    /// Get all possible asset strings for this package, version and platform
    pub fn asset_strs(&self, v: &str, os: &str, arch: &str) -> Result<IndexSet<String>> {
        let mut strs = IndexSet::new();
        let asset = self.asset_without_appended_ext(v, &Default::default(), os, arch)?;
        strs.insert(self.finish_asset(asset.clone(), v, os)?);
        strs.insert(asset);
        if os == "darwin" {
            let mut ctx = HashMap::default();
            ctx.insert("Arch".to_string(), "universal".to_string());
            let asset = self.asset_without_appended_ext(v, &ctx, os, arch)?;
            strs.insert(self.finish_asset(asset.clone(), v, os)?);
            strs.insert(asset);
        } else if os == "windows" {
            let mut ctx = HashMap::default();
            if arch == "arm64" {
                let fallback_arch = self
                    .replacements
                    .get("amd64")
                    .cloned()
                    .unwrap_or_else(|| "amd64".to_string());
                ctx.insert("Arch".to_string(), fallback_arch.clone());
                ctx.insert("GOARCH".to_string(), fallback_arch);
                let asset = self.asset_without_appended_ext(v, &ctx, os, arch)?;
                strs.insert(self.finish_asset(asset.clone(), v, os)?);
                strs.insert(asset);
            }
        }
        Ok(strs)
    }

    /// Apply Windows executable extension to an asset or URL string if appropriate.
    /// Mirrors upstream aqua's `completeWindowsExtToAsset` decision tree.
    fn complete_windows_ext_to_asset(&self, s: &str, v: &str, os: &str) -> Result<String> {
        if os != "windows" || s.ends_with(".exe") || s.ends_with(".jar") {
            return Ok(s.to_string());
        }
        if self.format == "raw" {
            return Ok(self.complete_windows_ext(s));
        }
        if !self.format.is_empty() {
            return Ok(s.to_string());
        }
        if self.os_file_ext_is_empty(s, v) {
            return Ok(self.complete_windows_ext(s));
        }
        Ok(s.to_string())
    }

    /// Apply Windows executable completion to install file source paths.
    /// Mirrors upstream aqua's `completeWindowsExtToFileSrc`.
    pub fn complete_windows_ext_to_file_src(&self, src: &str, v: &str, os: &str) -> String {
        if os != "windows" || !self.complete_windows_ext_enabled() {
            return src.to_string();
        }
        if self.os_file_ext_is_empty(src, v) {
            self.complete_windows_ext(src)
        } else {
            src.to_string()
        }
    }

    /// Apply Windows executable completion to link destinations, preserving an
    /// explicit source extension when the destination omits one.
    pub fn complete_windows_ext_to_file_dst(
        &self,
        src: &str,
        dst: &str,
        v: &str,
        os: &str,
    ) -> String {
        if os != "windows"
            || !self.complete_windows_ext_enabled()
            || !self.os_file_ext_is_empty(dst, v)
        {
            return dst.to_string();
        }
        match self.os_file_ext(src, v) {
            Some(ext) => append_str_ext(dst, &ext),
            None => self.complete_windows_ext(dst),
        }
    }

    /// Get the URL for this package and version
    pub fn url(&self, v: &str, os: &str, arch: &str) -> Result<String> {
        let url = self.parse_aqua_str(&self.url, v, &Default::default(), os, arch)?;
        let url = self.append_ext(url);
        self.complete_windows_ext_to_asset(&url, v, os)
    }

    /// Parse an Aqua template string with variable substitution and platform info
    pub fn parse_aqua_str(
        &self,
        s: &str,
        v: &str,
        overrides: &HashMap<String, String>,
        os: &str,
        arch: &str,
    ) -> Result<String> {
        let mut ctx = self.template_context(&self.replacements, v, os, arch);
        ctx.extend(self.vars_ctx()?);
        ctx.extend(overrides.clone());

        crate::template::render(s, &ctx)
    }

    fn actual_arch<'a>(&self, os: &str, arch: &'a str) -> &'a str {
        if (os == "darwin" && arch == "arm64" && self.rosetta2.unwrap_or(false))
            || (os == "windows" && arch == "arm64" && self.windows_arm_emulation.unwrap_or(false))
        {
            "amd64"
        } else {
            arch
        }
    }

    pub(super) fn template_context(
        &self,
        replacements: &HashMap<String, String>,
        v: &str,
        os: &str,
        arch: &str,
    ) -> HashMap<String, String> {
        let actual_arch = self.actual_arch(os, arch);
        let replace = |s: &str| {
            replacements
                .get(s)
                .map(|s| s.to_string())
                .unwrap_or_else(|| s.to_string())
        };

        let semver = if let Some(prefix) = &self.version_prefix {
            v.strip_prefix(prefix).unwrap_or(v)
        } else {
            v
        };

        HashMap::from([
            ("Version".to_string(), replace(v)),
            ("SemVer".to_string(), replace(semver)),
            ("OS".to_string(), replace(os)),
            ("GOOS".to_string(), replace(os)),
            ("GOARCH".to_string(), replace(actual_arch)),
            ("Arch".to_string(), replace(actual_arch)),
            ("Format".to_string(), replace(&self.format)),
        ])
    }

    fn vars_ctx(&self) -> Result<HashMap<String, String>> {
        self.validate_vars()?;
        let mut ctx = HashMap::new();
        for var in &self.vars {
            if let Some(value) = self.var_value(var)? {
                ctx.insert(format!("Vars.{}", var.name), value);
            }
        }
        Ok(ctx)
    }

    pub(super) fn validate_vars(&self) -> Result<()> {
        for var in &self.vars {
            if var.name.is_empty() {
                return Err(eyre!("aqua var name is empty"));
            }
            if var.required && self.var_value(var)?.is_none() {
                return Err(eyre!("required aqua var not set: {}", var.name));
            }
        }
        Ok(())
    }

    fn var_value(&self, var: &AquaVar) -> Result<Option<String>> {
        if let Some(value) = self.var_values.get(&var.name) {
            return Ok(Some(value.clone()));
        }
        Ok(var.default.clone())
    }
}
