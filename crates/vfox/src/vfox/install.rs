use super::*;

impl Vfox {
    pub async fn install<ID: AsRef<Path>>(
        &self,
        sdk: &str,
        version: &str,
        install_dir: ID,
    ) -> Result<InstallResult> {
        self.install_with_download_dir(sdk, version, install_dir, &self.download_dir)
            .await
    }

    pub async fn install_with_download_dir<ID: AsRef<Path>, DD: AsRef<Path>>(
        &self,
        sdk: &str,
        version: &str,
        install_dir: ID,
        download_dir: DD,
    ) -> Result<InstallResult> {
        self.install_with_download_dir_and_options(
            sdk,
            version,
            install_dir,
            download_dir,
            Default::default(),
        )
        .await
    }

    pub async fn install_with_download_dir_and_options<ID: AsRef<Path>, DD: AsRef<Path>>(
        &self,
        sdk: &str,
        version: &str,
        install_dir: ID,
        download_dir: DD,
        options: IndexMap<String, toml::Value>,
    ) -> Result<InstallResult> {
        self.install_plugin(sdk)?;
        let sdk = self.get_sdk_with_env(sdk)?;
        let pre_install = sdk
            .pre_install_with_options(version, options.clone())
            .await?;
        let install_dir = install_dir.as_ref();
        let download_dir = download_dir.as_ref();
        trace!("{pre_install:?}");
        let mut verified_attestation = None;
        let mut checksum_verified = false;
        if let Some(url) = pre_install.url.as_ref().map(|s| Url::from_str(s)) {
            let file = self.download(&url?, &sdk, version, download_dir).await?;
            verified_attestation = self.verify(&pre_install, &file).await?;
            self.extract(&file, install_dir)?;
            // Note: sha1/md5 are verified in `verify`, but intentionally excluded here.
            // mise stands this flag in for attestation when restoring lockfile provenance,
            // so it guards against downgrades; a collision-broken hash must not satisfy it.
            checksum_verified = pre_install.sha256.is_some() || pre_install.sha512.is_some();
        }

        Self::run_post_install(&sdk, version, install_dir, options).await?;
        Ok(InstallResult {
            sha256: pre_install.sha256,
            verified_attestation,
            checksum_verified,
        })
    }

    pub async fn pre_uninstall<ID: AsRef<Path>>(
        &self,
        sdk: &str,
        version: &str,
        install_dir: ID,
    ) -> Result<()> {
        let sdk = self.get_sdk_with_env(sdk)?;
        if sdk.get_metadata()?.hooks.contains("pre_uninstall") {
            let sdk_info = sdk.sdk_info(version.to_string(), install_dir.as_ref().to_path_buf())?;
            sdk.pre_uninstall(PreUninstallContext {
                main: sdk_info.clone(),
                sdk_info: BTreeMap::from([(sdk_info.name.clone(), sdk_info)]),
            })
            .await?;
        }
        Ok(())
    }

    async fn run_post_install(
        sdk: &Plugin,
        version: &str,
        install_dir: &Path,
        options: IndexMap<String, toml::Value>,
    ) -> Result<()> {
        if sdk.get_metadata()?.hooks.contains("post_install") {
            let sdk_info = sdk.sdk_info(version.to_string(), install_dir.to_path_buf())?;
            sdk.post_install(PostInstallContext {
                root_path: install_dir.to_path_buf(),
                runtime_version: version.to_string(),
                sdk_info: BTreeMap::from([(sdk_info.name.clone(), sdk_info)]),
                options,
            })
            .await?;
        }
        Ok(())
    }

    /// Run `PostInstall` again on an existing install, without downloading or
    /// extracting, to bring it back in line with the current tool options.
    pub async fn repair_install<ID: AsRef<Path>>(
        &self,
        sdk: &str,
        version: &str,
        install_dir: ID,
        options: IndexMap<String, toml::Value>,
    ) -> Result<()> {
        let sdk = self.get_sdk_with_env(sdk)?;
        Self::run_post_install(&sdk, version, install_dir.as_ref(), options).await
    }

    /// Ask the plugin whether an installed version still satisfies the request.
    /// Returns `None` when the plugin has no `MiseInstallSatisfied` hook.
    pub async fn mise_install_satisfied<ID: AsRef<Path>, T: serde::Serialize>(
        &self,
        sdk: &str,
        version: &str,
        install_dir: ID,
        options: T,
    ) -> Result<Option<MiseInstallSatisfiedResult>> {
        let sdk = self.get_sdk_with_env(sdk)?;
        if !sdk.get_metadata()?.hooks.contains("mise_install_satisfied") {
            return Ok(None);
        }
        let ctx = MiseInstallSatisfiedContext {
            version: version.to_string(),
            path: install_dir.as_ref().to_path_buf(),
            options,
        };
        Ok(Some(sdk.mise_install_satisfied(ctx).await?))
    }

    pub fn uninstall(&self, sdk: &str, version: &str) -> Result<()> {
        let path = self.install_dir.join(sdk).join(version);
        file::remove_dir_all(&path)?;
        Ok(())
    }

    pub(super) async fn download(
        &self,
        url: &Url,
        sdk: &Plugin,
        version: &str,
        download_dir: &Path,
    ) -> Result<PathBuf> {
        let path = Self::download_path_for(download_dir, &sdk.name, version, url)?;
        let mut request_url = url.clone();
        self.rewrite_url(&mut request_url);
        self.log_emit(format!("Downloading {request_url}"));
        let url_str = request_url.to_string();
        let bytes = retry_async(&url_str, || async {
            let mut request = CLIENT.get(request_url.clone());
            if let Some(resolver) = &self.http_headers_resolver {
                request = request.headers(resolver(&request_url));
            }
            let resp = request.send().await?;
            let resp = resp.error_for_status()?;
            resp.bytes().await
        })
        .await?;
        file::mkdirp(path.parent().unwrap())?;
        let mut file = tokio::fs::File::create(&path).await?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &bytes).await?;
        file.sync_all().await?;
        Ok(path)
    }

    pub(super) fn download_path_for(
        download_dir: &Path,
        sdk: &str,
        version: &str,
        url: &Url,
    ) -> Result<PathBuf> {
        let filename = url
            .path_segments()
            .and_then(|mut s| s.next_back())
            .ok_or("No filename in URL")?;
        Ok(download_dir.join(format!("{sdk}-{version}")).join(filename))
    }

    pub(super) async fn verify(
        &self,
        pre_install: &PreInstall,
        file: &Path,
    ) -> Result<Option<VerifiedAttestation>> {
        self.log_emit(format!("Verifying {file:?} checksum"));
        if let Some(sha256) = &pre_install.sha256 {
            xx::hash::ensure_checksum_sha256(file, sha256)?;
        }
        if let Some(sha512) = &pre_install.sha512 {
            xx::hash::ensure_checksum_sha512(file, sha512)?;
        }
        if let Some(sha1) = &pre_install.sha1 {
            ensure_checksum(file, "sha1", sha1, &xx::hash::file_hash_sha1(file)?)?;
        }
        if let Some(md5) = &pre_install.md5 {
            ensure_checksum(file, "md5", md5, &xx::hash::file_hash_md5(file)?)?;
        }
        let mut verified: Option<VerifiedAttestation> = None;
        // Only skip attestation verification when the plugin provides a strong checksum
        // (sha256/sha512) — otherwise there would be no meaningful integrity check left.
        // sha1/md5 are verified above but do not qualify: both are collision-broken.
        let has_checksum = pre_install.sha256.is_some() || pre_install.sha512.is_some();
        if let Some(attestation) = &pre_install.attestation
            && !(self.skip_verification && has_checksum)
        {
            self.log_emit(format!("Verify {file:?} attestation"));
            if let Some(owner) = &attestation.github_owner
                && let Some(repo) = &attestation.github_repo
            {
                let token = std::env::var("MISE_GITHUB_TOKEN")
                    .or_else(|_| std::env::var("GITHUB_TOKEN"))
                    .or(Err("GitHub artifact attestation verification requires either the MISE_GITHUB_TOKEN or GITHUB_TOKEN environment variable set"))?;
                mise_sigstore::verify_github_attestation(
                    file,
                    owner.as_str(),
                    repo.as_str(),
                    Some(token.as_str()),
                    attestation.github_signer_workflow.as_deref(),
                    crate::http::sigstore_retry_config(),
                )
                .await?;
                // All configured verifications always execute (no short-circuit).
                // Priority only affects which variant is *recorded* in `verified`.
                // GitHub attestations have the highest recording priority.
                verified = Some(VerifiedAttestation::GithubAttestations {
                    owner: owner.clone(),
                    repo: repo.clone(),
                    signer_workflow: attestation.github_signer_workflow.clone(),
                });
            }

            if let Some(sig_or_bundle_path) = &attestation.cosign_sig_or_bundle_path {
                if let Some(public_key_path) = &attestation.cosign_public_key_path {
                    mise_sigstore::verify_cosign_signature_with_key(
                        file,
                        sig_or_bundle_path,
                        public_key_path,
                    )
                    .await?;
                } else {
                    let identity = mise_sigstore::CosignIdentity {
                        identity: attestation.cosign_certificate_identity.clone(),
                        identity_regexp: attestation.cosign_certificate_identity_regexp.clone(),
                        oidc_issuer: attestation.cosign_certificate_oidc_issuer.clone(),
                        ..Default::default()
                    };
                    mise_sigstore::verify_cosign_signature(file, sig_or_bundle_path, &identity)
                        .await?;
                }
                // Cosign has the lowest recording priority: only record it if no
                // higher-priority verification was already recorded.
                if verified.is_none() {
                    verified = Some(VerifiedAttestation::Cosign {
                        sig_or_bundle_path: sig_or_bundle_path.clone(),
                        public_key_path: attestation.cosign_public_key_path.clone(),
                    });
                }
            }

            if let Some(provenance_path) = &attestation.slsa_provenance_path {
                if let (Some(identity), Some(issuer)) = (
                    attestation.slsa_signer_identity.as_deref(),
                    attestation.slsa_signer_issuer.as_deref(),
                ) {
                    let min_level = attestation.slsa_min_level.unwrap_or(1u8);
                    let signer = mise_sigstore::SlsaSignerIdentity { identity, issuer };
                    mise_sigstore::verify_slsa_provenance(file, provenance_path, min_level, signer)
                        .await?;
                    // SLSA has mid-tier recording priority: record it unless GitHub
                    // attestation (higher priority) was already recorded.
                    // Note: if Cosign also passed, SLSA supersedes it (SLSA > Cosign).
                    if !matches!(
                        verified,
                        Some(VerifiedAttestation::GithubAttestations { .. })
                    ) {
                        verified = Some(VerifiedAttestation::Slsa {
                            provenance_path: provenance_path.clone(),
                        });
                    }
                } else {
                    debug!("skipping SLSA provenance without expected signer identity and issuer");
                }
            }
        }
        Ok(verified)
    }

    fn extract(&self, file: &Path, install_dir: &Path) -> Result<()> {
        self.log_emit(format!("Extracting {file:?} to {install_dir:?}"));
        let filename = file.file_name().unwrap().to_string_lossy().to_string();
        let parent = install_dir.parent().unwrap();
        file::mkdirp(parent)?;
        let tmp = TempDir::with_prefix_in(&filename, parent)?;
        file::remove_dir_all(install_dir)?;
        let move_to_install = || {
            let subdirs = file::ls(tmp.path())?;
            if subdirs.len() == 1 && subdirs.first().unwrap().is_dir() {
                let subdir = subdirs.first().unwrap();
                file::mv(subdir, install_dir)?;
            } else {
                file::mv(tmp.path(), install_dir)?;
            }
            Result::Ok(())
        };
        if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
            xx::archive::untar_gz(file, tmp.path())?;
            move_to_install()?;
        } else if filename.ends_with(".tar.xz") || filename.ends_with(".txz") {
            xx::archive::untar_xz(file, tmp.path())?;
            move_to_install()?;
        } else if filename.ends_with(".tar.bz2")
            || filename.ends_with(".tbz2")
            || filename.ends_with(".tbz")
        {
            xx::archive::untar_bz2(file, tmp.path())?;
            move_to_install()?;
        } else if filename.ends_with(".zip") {
            xx::archive::unzip(file, tmp.path())?;
            move_to_install()?;
        } else {
            file::mv(file, install_dir.join(&filename))?;
            #[cfg(unix)]
            file::make_executable(install_dir.join(&filename))?;
        }
        Ok(())
    }
}
