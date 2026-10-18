use super::*;

impl AquaFile {
    fn template_ctx(
        &self,
        pkg: &AquaPackage,
        v: &str,
        os: &str,
        arch: &str,
    ) -> Result<HashMap<String, String>> {
        Self::template_ctx_for_asset(&self.name, &pkg.asset(v, os, arch)?)
    }

    fn template_ctx_for_asset(name: &str, asset: &str) -> Result<HashMap<String, String>> {
        let asset = asset_without_ext(asset);

        let mut ctx = HashMap::new();
        ctx.insert("AssetWithoutExt".to_string(), asset.to_string());
        ctx.insert("FileName".to_string(), name.to_string());
        Ok(ctx)
    }

    /// Get the source path for this file within the package
    pub fn src(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<Option<String>> {
        let ctx = self.template_ctx(pkg, v, os, arch)?;
        self.src
            .as_ref()
            .map(|src| pkg.parse_aqua_str(src, v, &ctx, os, arch))
            .transpose()
    }

    /// Source paths to look for, in order of preference. On macOS this adds the path inside
    /// a universal build, because a release can ship one in place of per-architecture assets
    /// (fastfetch 2.70.0), which the registry's per-architecture `src` template would miss.
    pub fn src_candidates(
        &self,
        pkg: &AquaPackage,
        v: &str,
        os: &str,
        arch: &str,
    ) -> Result<Vec<Option<String>>> {
        let mut srcs = vec![self.src(pkg, v, os, arch)?];
        if os == "darwin"
            && let Some(src) = &self.src
        {
            let (asset, _) = pkg.darwin_universal_assets(v, os, arch)?;
            let ctx = Self::template_ctx_for_asset(&self.name, &asset)?;
            let src = Some(pkg.parse_aqua_str(src, v, &ctx, os, arch)?);
            if !srcs.contains(&src) {
                srcs.push(src);
            }
        }
        Ok(srcs)
    }

    /// Get the link path for this file.
    pub fn link(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<Option<String>> {
        let ctx = self.template_ctx(pkg, v, os, arch)?;
        self.link
            .as_ref()
            .map(|link| pkg.parse_aqua_str(link, v, &ctx, os, arch))
            .transpose()
    }
}
