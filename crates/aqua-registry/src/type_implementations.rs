use crate::types::*;
use crate::utils::hashmap;
use eyre::Result;
use indexmap::IndexSet;

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
