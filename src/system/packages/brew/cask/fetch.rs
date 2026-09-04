use super::*;

pub(super) async fn fetch_cask(req: &PackageRequest, provision_ruby: bool) -> Result<Cask> {
    let name = &req.name;
    let tap_name = split_tap_name(name);
    let (requested_token, official_api) = match tap_name {
        Some(("homebrew", "cask", token)) => (token, true),
        Some((_, _, token)) => (token, false),
        None => (name.as_str(), true),
    };
    validate_cask_path_component("requested token", requested_token)?;
    if tap_name.is_none()
        && let Some(raw_base) = req
            .tap_url
            .as_deref()
            .and_then(super::super::api::github_raw_base)
    {
        let url = format!("{raw_base}/api/cask/{name}.json");
        match fetch_cask_url(name, &url, Some(normalize_cask_raw_base(raw_base)), false).await {
            Ok(cask) => return Ok(cask),
            Err(err) => debug!(
                "brew-cask: {name} unavailable in parent tap metadata ({err}); falling back to official metadata"
            ),
        }
    }
    let (url, raw_base) = match tap_name {
        Some(("homebrew", "cask", token)) => (
            format!("{API_BASE}/cask/{token}.json"),
            Some(HOMEBREW_CASK_RAW.to_string()),
        ),
        Some((owner, tap, token)) => {
            let Some(base) = super::super::api::tap_raw_base(owner, tap, req.tap_url.as_deref())
            else {
                bail!(
                    "brew-cask: unsupported tap URL for '{name}'; only GitHub tap URLs can be fetched directly"
                );
            };
            (
                format!("{base}/api/cask/{token}.json"),
                Some(normalize_cask_raw_base(base)),
            )
        }
        None => (
            format!("{API_BASE}/cask/{name}.json"),
            Some(HOMEBREW_CASK_RAW.to_string()),
        ),
    };
    match fetch_cask_url(requested_token, &url, raw_base.clone(), official_api).await {
        Ok(cask) => Ok(cask),
        Err(api_err) => {
            let Some((owner, tap, _)) =
                tap_name.filter(|(owner, tap, _)| !(*owner == "homebrew" && *tap == "cask"))
            else {
                return Err(api_err);
            };
            let mut cask = super::super::tap::cask_from_ruby(
                owner,
                tap,
                requested_token,
                req.tap_url.as_deref(),
                provision_ruby,
            )
            .await
            .wrap_err_with(|| {
                format!(
                    "published cask metadata was unavailable ({api_err}) and mise could not evaluate Casks/{requested_token}.rb"
                )
            })?;
            cask.raw_base = raw_base;
            Ok(cask)
        }
    }
}

pub(super) fn normalize_cask_raw_base(mut raw_base: String) -> String {
    if raw_base.ends_with("/HEAD") {
        raw_base.truncate(raw_base.len() - "/HEAD".len());
    }
    raw_base
}

pub(super) async fn fetch_cask_url(
    requested_token: &str,
    url: &str,
    raw_base: Option<String>,
    official_api: bool,
) -> Result<Cask> {
    let mut cask = HTTP_FETCH
        .json_cached::<Cask, _>(url)
        .await
        .wrap_err_with(|| {
            format!(
                "failed to fetch Homebrew cask '{requested_token}' directly. \
                 mise needs API metadata at api/cask/{requested_token}.json; for a \
                 third-party tap that means a JSON file on the tap's default branch, \
                 which most taps do not publish. mise will not proxy to the brew CLI; \
                 install it with `brew`, or see \
                 https://mise.jdx.dev/bootstrap/packages/brew.html#third-party-taps"
            )
        })?;
    cask.raw_base = raw_base;
    validate_cask_identity(&cask, requested_token, official_api)?;
    Ok(cask)
}

pub(super) fn validate_cask_identity(
    cask: &Cask,
    requested_token: &str,
    official_api: bool,
) -> Result<()> {
    validate_cask_path_component("API token", &cask.token)?;
    validate_cask_path_component("version", &cask.version)?;
    let trusted_alias = official_api
        && cask
            .aliases
            .iter()
            .chain(&cask.old_tokens)
            .any(|alias| alias == requested_token);
    if cask.token != requested_token && !trusted_alias {
        bail!(
            "brew-cask: requested token '{requested_token}' does not match API token '{}'",
            cask.token
        );
    }
    Ok(())
}

pub(super) fn validate_cask_path_component(kind: &str, value: &str) -> Result<()> {
    let mut components = Path::new(value).components();
    let valid = !value.is_empty()
        && !value.contains('\0')
        && !value.contains('\\')
        && matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && value != ".metadata"
        && !value.starts_with(".mise-");
    if !valid {
        bail!("brew-cask: invalid {kind} '{value}'");
    }
    Ok(())
}

pub(super) async fn fetch_and_stage(cask: &Cask, pr: Option<&dyn SingleReport>) -> Result<PathBuf> {
    if cask.url.ends_with(".git") {
        return fetch_git_clone_and_stage(cask, pr).await;
    }
    let archive = fetch_archive(cask, pr).await?;
    extract_archive(cask, &archive, pr)
}

pub(super) async fn fetch_git_clone_and_stage(
    cask: &Cask,
    pr: Option<&dyn SingleReport>,
) -> Result<PathBuf> {
    let extract_dir = crate::dirs::CACHE
        .join("system-brew")
        .join("cask-extract")
        .join(format!("{}-{}", cask.token, cask.version));
    file::remove_all(&extract_dir)?;
    file::create_dir_all(&extract_dir)?;
    let clone_dir = crate::dirs::CACHE
        .join("system-brew")
        .join("cask-git-clone")
        .join(format!("{}-{}", cask.token, cask.version));
    file::remove_all(&clone_dir)?;
    let mut clone_opts = CloneOptions::default();
    if let Some(branch) = cask.url_specs.branch.as_deref() {
        clone_opts = clone_opts.branch(branch);
    }
    if let Some(pr) = pr {
        clone_opts = clone_opts.pr(pr);
    }
    Git::new(&clone_dir)
        .clone(&cask.url, clone_opts)
        .wrap_err_with(|| format!("brew-cask:{}: failed to clone {}", cask.token, cask.url))?;
    if let Some(only_path) = &cask.url_specs.only_path {
        let source = git_only_path_source(cask, &clone_dir, Path::new(only_path))?;
        for entry in std::fs::read_dir(&source)? {
            let entry = entry?;
            let dest = extract_dir.join(entry.file_name());
            file::rename(entry.path(), &dest)?;
        }
    } else {
        for entry in std::fs::read_dir(&clone_dir)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            let dest = extract_dir.join(entry.file_name());
            file::rename(entry.path(), &dest)?;
        }
    }
    file::remove_all(&clone_dir)?;
    Ok(extract_dir)
}

pub(super) fn git_only_path_source(
    cask: &Cask,
    clone_dir: &Path,
    only_path: &Path,
) -> Result<PathBuf> {
    if only_path.is_absolute()
        || only_path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        bail!(
            "brew-cask:{}: git only_path must stay within the checkout",
            cask.token
        );
    }
    let clone_root = clone_dir.canonicalize()?;
    let source = clone_dir.join(only_path).canonicalize().wrap_err_with(|| {
        format!(
            "brew-cask:{}: git only_path does not exist: {}",
            cask.token,
            only_path.display()
        )
    })?;
    if !source.starts_with(&clone_root) || !source.is_dir() {
        bail!(
            "brew-cask:{}: git only_path must name a directory within the checkout",
            cask.token
        );
    }
    Ok(source)
}

pub(super) async fn fetch_archive(cask: &Cask, pr: Option<&dyn SingleReport>) -> Result<PathBuf> {
    let filename = archive_filename(&cask.url)
        .ok_or_else(|| eyre!("brew-cask:{}: URL has no file name", cask.token))?;
    let cache_dir = crate::dirs::CACHE.join("system-brew").join("casks");
    file::create_dir_all(&cache_dir)?;
    let url_hash = &hash::hash_sha256_to_str(&cask.url)[..12];
    let archive = cache_dir.join(format!(
        "{}-{}-{url_hash}-{filename}",
        cask.token, cask.version
    ));
    if !archive.exists() {
        HTTP.download_file(&cask.url, &archive, pr).await?;
        // Strip macOS quarantine so it doesn't propagate into extracted/copied artifacts.
        let _ = std::process::Command::new("xattr")
            .args(["-d", "com.apple.quarantine"])
            .arg(&archive)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    match cask.sha256.as_deref() {
        Some("no_check") => {}
        Some(sha256) => hash::ensure_checksum(&archive, sha256, pr, "sha256")?,
        None => bail!("brew-cask:{}: cask metadata has no sha256", cask.token),
    }
    Ok(archive)
}

pub(super) fn extract_archive(
    cask: &Cask,
    archive: &Path,
    pr: Option<&dyn SingleReport>,
) -> Result<PathBuf> {
    let extract_dir = crate::dirs::CACHE
        .join("system-brew")
        .join("cask-extract")
        .join(format!("{}-{}", cask.token, cask.version));
    file::remove_all(&extract_dir)?;
    file::create_dir_all(&extract_dir)?;
    let filename = archive
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default();
    if is_dmg_archive(archive, filename)? {
        file::un_dmg(archive, &extract_dir)?;
    } else {
        let format = cask_extraction_format(archive, filename)?;
        if format == ExtractionFormat::Raw {
            // A direct pkg download may have an opaque URL path while the response's
            // Content-Disposition and the cask artifact supply its real name. Stage
            // XAR installers under the declared artifact name; other raw downloads
            // are executable binaries and retain the original URL filename.
            let (stage_filename, executable) = raw_cask_artifact_name(cask, archive, filename)?;
            let dest = extract_dir.join(stage_filename);
            file::copy(archive, &dest)?;
            if executable {
                file::make_executable(&dest)?;
            }
        } else if !format.is_archive() {
            bail!(
                "brew-cask:{}: unsupported archive type for {}",
                cask.token,
                filename
            );
        } else {
            file::extract_archive(
                archive,
                &extract_dir,
                format,
                &ExtractOptions {
                    pr,
                    ..Default::default()
                },
            )?;
        }
    }
    extract_nested_cask_archives(&extract_dir, pr)?;
    Ok(extract_dir)
}

pub(super) fn extract_nested_cask_archives(
    extract_dir: &Path,
    pr: Option<&dyn SingleReport>,
) -> Result<()> {
    for depth in 0..MAX_NESTED_CASK_ARCHIVES {
        let Some(archive) = single_nested_cask_archive(extract_dir)? else {
            return Ok(());
        };
        let filename = archive
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| eyre!("brew-cask: nested archive name is not valid UTF-8"))?
            .to_string();
        let nested = extract_dir.with_file_name(format!(
            ".{}-nested-{depth}",
            extract_dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("cask")
        ));
        file::remove_all(&nested)?;
        file::rename(&archive, &nested)?;
        file::remove_all(extract_dir)?;
        file::create_dir_all(extract_dir)?;
        let result = extract_nested_cask_archive(&nested, extract_dir, &filename, pr);
        let cleanup = file::remove_all(&nested);
        result?;
        cleanup?;
    }
    if single_nested_cask_archive(extract_dir)?.is_some() {
        bail!("brew-cask: nested archive depth exceeds {MAX_NESTED_CASK_ARCHIVES}");
    }
    Ok(())
}

pub(super) fn single_nested_cask_archive(root: &Path) -> Result<Option<PathBuf>> {
    let mut entries = std::fs::read_dir(root)?.filter(|entry| match entry {
        Ok(entry) => entry.file_name() != "__MACOSX",
        Err(_) => true,
    });
    let Some(entry) = entries.next().transpose()? else {
        return Ok(None);
    };
    if entries.next().is_some() || !entry.file_type()?.is_file() {
        return Ok(None);
    }
    let path = entry.path();
    let filename = entry.file_name();
    let filename = filename
        .to_str()
        .ok_or_else(|| eyre!("brew-cask: nested archive name is not valid UTF-8"))?;
    if is_dmg_archive(&path, filename)? {
        return Ok(Some(path));
    }
    let format = cask_extraction_format(&path, filename)?;
    Ok(matches!(
        format,
        ExtractionFormat::TarGz
            | ExtractionFormat::TarXz
            | ExtractionFormat::TarBz2
            | ExtractionFormat::TarZst
            | ExtractionFormat::Tar
            | ExtractionFormat::Zip
            | ExtractionFormat::SevenZip
    )
    .then_some(path))
}

pub(super) fn extract_nested_cask_archive(
    archive: &Path,
    extract_dir: &Path,
    filename: &str,
    pr: Option<&dyn SingleReport>,
) -> Result<()> {
    if is_dmg_archive(archive, filename)? {
        file::un_dmg(archive, extract_dir)
    } else {
        file::extract_archive(
            archive,
            extract_dir,
            cask_extraction_format(archive, filename)?,
            &ExtractOptions {
                pr,
                ..Default::default()
            },
        )
    }
}

pub(super) async fn execute_lifecycle_hook(
    cask: &Cask,
    staged_path: &Path,
    appdir: &Path,
    hook: &str,
    pr: Option<&dyn SingleReport>,
) -> Result<()> {
    if !has_lifecycle_hook(cask, hook) {
        return Ok(());
    }
    let ruby = cask_ruby_bin().await?;
    let cask_rb = fetch_cask_rb(cask, pr).await?;
    let shim_path = crate::dirs::CACHE
        .join("system-brew")
        .join("casks")
        .join("mise-brew-cask-shim.rb");
    ensure_cask_shim(&shim_path)?;
    if let Some(pr) = pr {
        pr.set_message(format!("run cask {hook}"));
    }
    let runner = CmdLineRunner::new(&ruby).arg(&shim_path).envs([
        ("MISE_BREW_CASK_FILE", cask_rb.display().to_string()),
        ("MISE_BREW_CASK_TOKEN", cask.token.clone()),
        ("MISE_BREW_CASK_VERSION", cask.version.clone()),
        (
            "MISE_BREW_CASK_STAGED_PATH",
            staged_path.display().to_string(),
        ),
        ("MISE_BREW_CASK_APPDIR", appdir.display().to_string()),
        ("MISE_BREW_PREFIX", prefix::prefix().display().to_string()),
        ("MISE_BREW_CASK_HOOK", hook.to_string()),
        ("MISE_BREW_CASK_SUDO", sudo::subprocess_mode().to_string()),
    ]);
    let runner = match pr {
        Some(pr) => runner.with_pr(pr),
        None => runner,
    };
    runner
        .execute_async()
        .await
        .wrap_err_with(|| format!("brew-cask:{}: failed to run {hook}", cask.token))
}

pub(super) async fn cask_ruby_bin() -> Result<PathBuf> {
    if let Some(ruby) = file::which("ruby")
        && tokio::process::Command::new(&ruby)
            .args(["-e", "exit 0"])
            .status()
            .await
            .is_ok_and(|status| status.success())
    {
        return Ok(ruby);
    }
    source::ruby_bin().await
}

pub(super) fn ensure_cask_shim(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        file::create_dir_all(parent)?;
    }
    if file::read_to_string(path).is_ok_and(|contents| contents == CASK_SHIM_RB) {
        return Ok(());
    }
    file::write(path, CASK_SHIM_RB)
}

pub(super) async fn fetch_cask_rb(cask: &Cask, pr: Option<&dyn SingleReport>) -> Result<PathBuf> {
    let rb_path = cask.ruby_source_path.as_ref().ok_or_else(|| {
        eyre!(
            "brew-cask:{}: lifecycle hooks require ruby_source_path in API metadata",
            cask.token
        )
    })?;
    let sha256 = cask
        .ruby_source_checksum
        .as_ref()
        .and_then(|c| c.sha256.as_deref())
        .ok_or_else(|| {
            eyre!(
                "brew-cask:{}: lifecycle hooks require ruby_source_checksum in API metadata",
                cask.token
            )
        })?;
    let commit = cask.tap_git_head.as_deref().ok_or_else(|| {
        eyre!(
            "brew-cask:{}: lifecycle hooks require tap_git_head in API metadata",
            cask.token
        )
    })?;
    let raw_base = cask.raw_base.as_deref().ok_or_else(|| {
        eyre!(
            "brew-cask:{}: lifecycle hooks require a GitHub raw source URL",
            cask.token
        )
    })?;
    let cache_dir = crate::dirs::CACHE.join("system-brew").join("cask-source");
    file::create_dir_all(&cache_dir)?;
    let short_sha = sha256.get(..12).unwrap_or(sha256);
    let dest = cache_dir.join(format!("{}-{short_sha}.rb", cask.token));
    if dest.exists() && hash::ensure_checksum(&dest, sha256, None, "sha256").is_ok() {
        return Ok(dest);
    }
    let url = format!("{raw_base}/{commit}/{rb_path}");
    if let Some(pr) = pr {
        pr.set_message(format!("download {rb_path}"));
    }
    HTTP_FETCH.download_file(&url, &dest, pr).await?;
    hash::ensure_checksum(&dest, sha256, pr, "sha256")?;
    Ok(dest)
}

pub(super) fn cask_extraction_format(archive: &Path, filename: &str) -> Result<ExtractionFormat> {
    let format = ExtractionFormat::from_file_name(filename);
    if format != ExtractionFormat::Raw {
        return Ok(format);
    }
    Ok(detect_extraction_format(archive)?.unwrap_or(format))
}

pub(super) fn is_dmg_archive(archive: &Path, filename: &str) -> Result<bool> {
    if filename.ends_with(".dmg") {
        return Ok(true);
    }
    if ExtractionFormat::from_file_name(filename) != ExtractionFormat::Raw {
        return Ok(false);
    }

    // UDIF images end with a 512-byte resource footer containing this prefix.
    const UDIF_TRAILER_SIZE: i64 = 512;
    const UDIF_TRAILER_PREFIX: &[u8; 12] = b"koly\0\0\0\x04\0\0\x02\0";
    let mut file = std::fs::File::open(archive)?;
    if file.metadata()?.len() < UDIF_TRAILER_SIZE as u64 {
        return Ok(false);
    }
    file.seek(SeekFrom::End(-UDIF_TRAILER_SIZE))?;
    let mut prefix = [0; UDIF_TRAILER_PREFIX.len()];
    file.read_exact(&mut prefix)?;
    Ok(&prefix == UDIF_TRAILER_PREFIX)
}

pub(super) fn detect_extraction_format(archive: &Path) -> Result<Option<ExtractionFormat>> {
    let mut file = std::fs::File::open(archive)?;
    let mut magic = [0; 8];
    let len = file.read(&mut magic)?;
    let magic = &magic[..len];
    if magic.starts_with(b"PK\x03\x04") {
        return Ok(Some(ExtractionFormat::Zip));
    }
    Ok(None)
}

pub(super) fn raw_cask_artifact_name(
    cask: &Cask,
    archive: &Path,
    fallback: &str,
) -> Result<(String, bool)> {
    let mut magic = [0; 4];
    let len = std::fs::File::open(archive)?.read(&mut magic)?;
    if magic[..len].starts_with(b"xar!") {
        let pkgs = cask_artifacts(cask)?.pkgs;
        if let [pkg] = pkgs.as_slice() {
            let mut components = Path::new(&pkg.source).components();
            if matches!(components.next(), Some(Component::Normal(_)))
                && components.next().is_none()
            {
                return Ok((pkg.source.clone(), false));
            }
        }
    }
    Ok((
        archive_filename(&cask.url).unwrap_or_else(|| fallback.to_string()),
        true,
    ))
}
