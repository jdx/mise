use super::*;

// Implementation of merge methods for various types
impl AquaChecksum {
    pub fn _type(&self) -> &AquaChecksumType {
        self.r#type.as_ref().unwrap()
    }

    pub fn algorithm(&self) -> &AquaChecksumAlgorithm {
        self.algorithm.as_ref().unwrap()
    }

    pub fn asset_strs(
        &self,
        pkg: &AquaPackage,
        v: &str,
        os: &str,
        arch: &str,
    ) -> Result<IndexSet<String>> {
        let mut asset_strs = IndexSet::new();
        for asset in pkg.asset_strs(v, os, arch)? {
            let checksum_asset = self.asset.as_ref().unwrap();
            let mut ctx = self.template_ctx(pkg, v, os, arch)?;
            ctx.insert("Asset".to_string(), asset.to_string());
            asset_strs.insert(pkg.parse_aqua_str(checksum_asset, v, &ctx, os, arch)?);
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

    pub fn url(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<String> {
        pkg.parse_aqua_str(
            self.url.as_ref().unwrap(),
            v,
            &self.template_ctx(pkg, v, os, arch)?,
            os,
            arch,
        )
    }

    pub(super) fn merge(&mut self, other: Self) {
        if let Some(r#type) = other.r#type {
            self.r#type = Some(r#type);
        }
        if let Some(algorithm) = other.algorithm {
            self.algorithm = Some(algorithm);
        }
        if let Some(pattern) = other.pattern {
            self.pattern = Some(pattern);
        }
        if let Some(enabled) = other.enabled {
            self.enabled = Some(enabled);
        }
        if let Some(asset) = other.asset {
            self.asset = Some(asset);
        }
        if let Some(url) = other.url {
            self.url = Some(url);
        }
        if let Some(file_format) = other.file_format {
            self.file_format = Some(file_format);
        }
        if let Some(replacements) = other.replacements {
            if replacements.is_empty() {
                self.replacements = Some(HashMap::new());
            } else {
                self.replacements
                    .get_or_insert_with(HashMap::new)
                    .extend(replacements);
            }
        }
        if let Some(cosign) = other.cosign {
            if self.cosign.is_none() {
                self.cosign = Some(cosign.clone());
            }
            self.cosign.as_mut().unwrap().merge(cosign);
        }
        if let Some(minisign) = other.minisign {
            if self.minisign.is_none() {
                self.minisign = Some(minisign.clone());
            }
            self.minisign.as_mut().unwrap().merge(minisign);
        }
        if let Some(attestations) = other.github_artifact_attestations {
            if self.github_artifact_attestations.is_none() {
                self.github_artifact_attestations = Some(attestations.clone());
            }
            self.github_artifact_attestations
                .as_mut()
                .unwrap()
                .merge(attestations);
        }
    }

    pub fn template_ctx(
        &self,
        pkg: &AquaPackage,
        v: &str,
        os: &str,
        arch: &str,
    ) -> Result<HashMap<String, String>> {
        let mut ctx = pkg.template_context(&self.effective_replacements(pkg), v, os, arch);
        if pkg.package_type() == AquaPackageType::Http {
            ctx.insert("AssetURL".to_string(), pkg.url(v, os, arch)?);
        }
        Ok(ctx)
    }

    fn effective_replacements(&self, pkg: &AquaPackage) -> HashMap<String, String> {
        match &self.replacements {
            None => pkg.replacements.clone(),
            Some(replacements) if replacements.is_empty() => HashMap::new(),
            Some(replacements) => {
                let mut merged = pkg.replacements.clone();
                merged.extend(replacements.clone());
                merged
            }
        }
    }
}

impl AquaCosign {
    // TODO: This does not support `{{.Asset}}`.
    pub fn opts(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<Vec<String>> {
        self.opts
            .iter()
            .map(|opt| pkg.parse_aqua_str(opt, v, &Default::default(), os, arch))
            .collect()
    }

    pub(super) fn merge(&mut self, other: Self) {
        if let Some(enabled) = other.enabled {
            self.enabled = Some(enabled);
        }
        if let Some(signature) = other.signature.clone() {
            if self.signature.is_none() {
                self.signature = Some(signature.clone());
            }
            self.signature.as_mut().unwrap().merge(signature);
        }
        if let Some(key) = other.key.clone() {
            if self.key.is_none() {
                self.key = Some(key.clone());
            }
            self.key.as_mut().unwrap().merge(key);
        }
        if let Some(certificate) = other.certificate.clone() {
            if self.certificate.is_none() {
                self.certificate = Some(certificate.clone());
            }
            self.certificate.as_mut().unwrap().merge(certificate);
        }
        if let Some(bundle) = other.bundle.clone() {
            if self.bundle.is_none() {
                self.bundle = Some(bundle.clone());
            }
            self.bundle.as_mut().unwrap().merge(bundle);
        }
        if !other.opts.is_empty() {
            self.opts = other.opts.clone();
        }
    }
}

impl AquaCosignSignature {
    pub fn url(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<String> {
        pkg.parse_aqua_str(self.url.as_ref().unwrap(), v, &Default::default(), os, arch)
    }

    pub fn asset_strs(
        &self,
        pkg: &AquaPackage,
        v: &str,
        os: &str,
        arch: &str,
    ) -> Result<IndexSet<String>> {
        let mut asset_strs = IndexSet::new();
        if let Some(cosign_asset_template) = &self.asset {
            for asset in pkg.asset_strs(v, os, arch)? {
                let mut ctx = HashMap::new();
                ctx.insert("Asset".to_string(), asset.to_string());
                asset_strs.insert(pkg.parse_aqua_str(cosign_asset_template, v, &ctx, os, arch)?);
            }
        }
        Ok(asset_strs)
    }

    pub(super) fn merge(&mut self, other: Self) {
        if let Some(r#type) = other.r#type {
            self.r#type = Some(r#type);
        }
        if let Some(repo_owner) = other.repo_owner {
            self.repo_owner = Some(repo_owner);
        }
        if let Some(repo_name) = other.repo_name {
            self.repo_name = Some(repo_name);
        }
        if let Some(url) = other.url {
            self.url = Some(url);
        }
        if let Some(asset) = other.asset {
            self.asset = Some(asset);
        }
    }
}

impl AquaSlsaProvenance {
    pub fn has_signer_identity(&self) -> bool {
        self.signer_identity
            .as_deref()
            .is_some_and(|s| !s.is_empty())
            && self.signer_issuer.as_deref().is_some_and(|s| !s.is_empty())
    }

    pub fn asset_strs(
        &self,
        pkg: &AquaPackage,
        v: &str,
        os: &str,
        arch: &str,
    ) -> Result<IndexSet<String>> {
        let mut asset_strs = IndexSet::new();
        if let Some(slsa_asset_template) = &self.asset {
            for asset in pkg.asset_strs(v, os, arch)? {
                let mut ctx = HashMap::new();
                ctx.insert("Asset".to_string(), asset.to_string());
                asset_strs.insert(pkg.parse_aqua_str(slsa_asset_template, v, &ctx, os, arch)?);
            }
        }
        Ok(asset_strs)
    }

    pub fn url(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<String> {
        pkg.parse_aqua_str(self.url.as_ref().unwrap(), v, &Default::default(), os, arch)
    }

    pub(super) fn merge(&mut self, other: Self) {
        if let Some(enabled) = other.enabled {
            self.enabled = Some(enabled);
        }
        if let Some(r#type) = other.r#type {
            self.r#type = Some(r#type);
        }
        if let Some(repo_owner) = other.repo_owner {
            self.repo_owner = Some(repo_owner);
        }
        if let Some(repo_name) = other.repo_name {
            self.repo_name = Some(repo_name);
        }
        if let Some(url) = other.url {
            self.url = Some(url);
        }
        if let Some(asset) = other.asset {
            self.asset = Some(asset);
        }
        if let Some(source_uri) = other.source_uri {
            self.source_uri = Some(source_uri);
        }
        if let Some(source_tag) = other.source_tag {
            self.source_tag = Some(source_tag);
        }
        if let Some(signer_identity) = other.signer_identity {
            self.signer_identity = Some(signer_identity);
        }
        if let Some(signer_issuer) = other.signer_issuer {
            self.signer_issuer = Some(signer_issuer);
        }
    }
}

impl AquaMinisign {
    pub fn _type(&self) -> &AquaMinisignType {
        self.r#type.as_ref().unwrap()
    }

    pub fn url(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<String> {
        pkg.parse_aqua_str(self.url.as_ref().unwrap(), v, &Default::default(), os, arch)
    }

    pub fn asset(
        &self,
        pkg: &AquaPackage,
        package_asset: &str,
        v: &str,
        os: &str,
        arch: &str,
    ) -> Result<String> {
        let mut ctx = HashMap::new();
        ctx.insert("Asset".to_string(), package_asset.to_string());
        pkg.parse_aqua_str(self.asset.as_ref().unwrap(), v, &ctx, os, arch)
    }

    pub fn public_key(&self, pkg: &AquaPackage, v: &str, os: &str, arch: &str) -> Result<String> {
        pkg.parse_aqua_str(
            self.public_key.as_ref().unwrap(),
            v,
            &Default::default(),
            os,
            arch,
        )
    }

    pub(super) fn merge(&mut self, other: Self) {
        if let Some(enabled) = other.enabled {
            self.enabled = Some(enabled);
        }
        if let Some(r#type) = other.r#type {
            self.r#type = Some(r#type);
        }
        if let Some(repo_owner) = other.repo_owner {
            self.repo_owner = Some(repo_owner);
        }
        if let Some(repo_name) = other.repo_name {
            self.repo_name = Some(repo_name);
        }
        if let Some(url) = other.url {
            self.url = Some(url);
        }
        if let Some(asset) = other.asset {
            self.asset = Some(asset);
        }
        if let Some(public_key) = other.public_key {
            self.public_key = Some(public_key);
        }
    }
}

impl AquaGithubArtifactAttestations {
    pub(super) fn merge(&mut self, other: Self) {
        if let Some(enabled) = other.enabled {
            self.enabled = Some(enabled);
        }
        if let Some(predicate_type) = other.predicate_type {
            self.predicate_type = Some(predicate_type);
        }
        if let Some(signer_workflow) = other.signer_workflow {
            self.signer_workflow = Some(signer_workflow);
        }
    }
}
