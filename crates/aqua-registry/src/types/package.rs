use super::*;

impl AquaPackage {
    /// Return every command name this package may install across its version and platform
    /// overrides.
    ///
    /// This is intentionally conservative: callers that need command metadata before resolving a
    /// concrete version can safely create provider shims for the union. Once installed, the
    /// backend still uses the resolved package's exact file destinations.
    pub fn possible_bin_names(&self) -> Vec<String> {
        let mut bins = BTreeSet::new();

        if self.version_constraint.is_empty() {
            collect_package_bin_names(self, &mut bins);
            return bins.into_iter().collect();
        }
        if self.version_constraint.trim() != "false" {
            collect_package_bin_names(self, &mut bins);
        }
        for version_override in &self.version_overrides {
            let package = apply_override(self.clone(), version_override);
            collect_package_bin_names(&package, &mut bins);
        }

        bins.into_iter().collect()
    }

    /// Return the package type, preserving aqua's default of `github_release`
    /// when the field is omitted.
    pub fn package_type(&self) -> AquaPackageType {
        self.r#type.unwrap_or_default()
    }

    /// Apply version-specific configurations and overrides
    pub fn with_version(self, versions: &[&str], os: &str, arch: &str) -> AquaPackage {
        self.with_version_runtime(versions, os, arch, AquaRuntime::default())
    }

    /// Apply version-specific configurations and overrides for a libc runtime variant.
    pub fn with_version_libc(
        self,
        versions: &[&str],
        os: &str,
        arch: &str,
        libc: Option<&str>,
    ) -> AquaPackage {
        self.with_version_runtime(versions, os, arch, AquaRuntime { libc })
    }

    /// Apply a catch-all version fallback when the root package is explicitly disabled.
    ///
    /// The boolean is false when the root package is explicitly disabled,
    /// even if no catch-all override exists to replace it. Conditional roots
    /// are preserved because determining whether they match requires a
    /// concrete version.
    pub fn with_unconditional_version_override(mut self) -> (AquaPackage, bool) {
        if self.version_constraint.trim() != "false" {
            return (self, true);
        }
        if let Some(version_override) = self
            .version_overrides
            .iter()
            .find(|version_override| {
                matches!(version_override.version_constraint.trim(), "" | "true")
            })
            .cloned()
        {
            self = apply_override(self, &version_override);
        }
        (self, false)
    }

    /// Return platform overrides after applying them to this package.
    ///
    /// Overrides containing runtime variants mise does not understand are
    /// omitted because they can never match in the Aqua resolver either.
    pub fn platform_overrides(&self) -> Vec<AquaPackagePlatformOverride> {
        self.overrides
            .iter()
            .filter_map(|package_override| {
                let mut libc = None;
                for variant in &package_override.variants {
                    if variant.key != "libc" {
                        return None;
                    }
                    let variant_libc = normalize_libc(Some(&variant.value))?.to_string();
                    if libc.as_ref().is_some_and(|libc| libc != &variant_libc) {
                        return None;
                    }
                    libc = Some(variant_libc);
                }
                Some(AquaPackagePlatformOverride {
                    package: apply_override(self.clone(), &package_override.pkg),
                    goos: package_override.goos.clone(),
                    goarch: package_override.goarch.clone(),
                    envs: package_override.envs.clone(),
                    libc,
                })
            })
            .collect()
    }

    fn with_version_runtime(
        mut self,
        versions: &[&str],
        os: &str,
        arch: &str,
        runtime: AquaRuntime<'_>,
    ) -> AquaPackage {
        if let Some(version_override) = self
            .version_override(versions)
            .filter(|version_override| !std::ptr::eq(*version_override, &self))
            .cloned()
        {
            self = apply_override(self, &version_override);
        }
        self.apply_format_override(os);
        if let Some(pkg) = self
            .overrides
            .iter()
            .find(|o| o.matches(os, arch, runtime))
            .map(|o| o.pkg.clone())
        {
            self = apply_override(self, &pkg)
        }
        self
    }

    fn apply_format_override(&mut self, os: &str) {
        if let Some(format_override) = self
            .format_overrides
            .iter()
            .find(|format_override| format_override.matches(os))
        {
            self.format = format_override.format.clone();
        }
    }

    /// Apply user-provided variable values used by aqua `vars` templates.
    pub fn with_var_values(mut self, var_values: HashMap<String, String>) -> Result<AquaPackage> {
        self.var_values = var_values;
        self.validate_vars()?;
        Ok(self)
    }

    pub fn version_constraint_ok(&self, versions: &[&str]) -> bool {
        self.version_override(versions).is_some()
    }

    pub(super) fn version_override(&self, versions: &[&str]) -> Option<&AquaPackage> {
        // Aqua treats a package without a top-level constraint as unconditional.
        // In that case version overrides are not considered.
        if self.version_constraint.is_empty() {
            return Some(self);
        }
        let expressions = versions
            .iter()
            .map(|v| (*v, self.expr_parser(v), self.expr_ctx(v)))
            .collect_vec();
        vec![self]
            .into_iter()
            .chain(self.version_overrides.iter())
            .find(|vo| {
                expressions.iter().any(|(version, expr, ctx)| {
                    let version_prefix =
                        vo.version_prefix.as_ref().or(self.version_prefix.as_ref());
                    if version_prefix.is_some_and(|prefix| !version.starts_with(prefix)) {
                        return false;
                    }
                    if vo.version_constraint.is_empty() {
                        true
                    } else {
                        expr.eval(&vo.version_constraint, ctx)
                            .map_err(|e| {
                                log::debug!("error parsing {}: {e}", vo.version_constraint)
                            })
                            .unwrap_or(false.into())
                            .as_bool()
                            .unwrap()
                    }
                })
            })
    }
}

fn collect_package_bin_names(package: &AquaPackage, bins: &mut BTreeSet<String>) {
    if package.no_asset == Some(true) {
        return;
    }

    collect_direct_bin_names(package, bins);
    for platform_override in package.platform_overrides() {
        collect_direct_bin_names(&platform_override.package, bins);
    }
}

fn collect_direct_bin_names(package: &AquaPackage, bins: &mut BTreeSet<String>) {
    if package.files.is_empty() {
        let name = package
            .name
            .as_deref()
            .and_then(|name| name.rsplit('/').next())
            .unwrap_or(&package.repo_name);
        if !name.is_empty() {
            bins.insert(name.to_string());
        }
        return;
    }

    for file in &package.files {
        let destination = file.link.as_deref().unwrap_or(&file.name);
        // Aqua file links are paths relative to the extracted artifact. Accept both separators so
        // registry generation is target-independent.
        if let Some(name) = destination.rsplit(['/', '\\']).next()
            && !name.is_empty()
            && !name.contains("{{")
        {
            bins.insert(
                name.strip_suffix(".exe")
                    .or_else(|| name.strip_suffix(".EXE"))
                    .unwrap_or(name)
                    .to_string(),
            );
        }
    }
}
