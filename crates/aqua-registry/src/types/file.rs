use super::*;

impl AquaFile {
    fn template_ctx(
        &self,
        pkg: &AquaPackage,
        v: &str,
        os: &str,
        arch: &str,
    ) -> Result<HashMap<String, String>> {
        let asset = pkg.asset(v, os, arch)?;
        let asset = asset_without_ext(&asset);

        let mut ctx = HashMap::new();
        ctx.insert("AssetWithoutExt".to_string(), asset.to_string());
        ctx.insert("FileName".to_string(), self.name.to_string());
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

    /// Get the link path for this file.
    pub fn link(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<Option<String>> {
        let ctx = self.template_ctx(pkg, v, os, arch)?;
        self.link
            .as_ref()
            .map(|link| pkg.parse_aqua_str(link, v, &ctx, os, arch))
            .transpose()
    }
}
