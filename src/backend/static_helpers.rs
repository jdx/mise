// Shared template logic for backends
use crate::config::Settings;
use crate::file;
use crate::hash;
use crate::toolset::ToolVersion;
use crate::toolset::ToolVersionOptions;
use eyre::{Result, bail};
use std::path::Path;

pub fn template_string(template: &str, tv: &ToolVersion) -> String {
    let name = tv.ba().tool_name();
    let version = &tv.version;
    let settings = Settings::get();
    let os = settings.os();
    let arch = settings.arch();
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };

    template
        .replace("{name}", &name)
        .replace("{version}", version)
        .replace("{os}", os)
        .replace("{arch}", arch)
        .replace("{ext}", ext)
}

pub fn get_filename_from_url(url: &str) -> String {
    url.split('/').next_back().unwrap_or("download").to_string()
}

pub fn install_artifact(
    tv: &crate::toolset::ToolVersion,
    file_path: &Path,
    opts: &ToolVersionOptions,
) -> eyre::Result<()> {
    let install_path = tv.install_path();
    let strip_components = opts
        .get("strip_components")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    file::remove_all(&install_path)?;
    file::create_dir_all(&install_path)?;

    // Use TarFormat for format detection
    let ext = file_path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let format = file::TarFormat::from_ext(ext);
    let tar_opts = file::TarOptions {
        format,
        strip_components,
        pr: None,
    };
    if format == file::TarFormat::Zip {
        file::unzip(file_path, &install_path)?;
    } else if format == file::TarFormat::Raw {
        // Copy the file directly to the bin_path directory or install_path
        if let Some(bin_path_template) = opts.get("bin_path") {
            let bin_path = template_string(bin_path_template, tv);
            let bin_dir = install_path.join(bin_path);
            file::create_dir_all(&bin_dir)?;
            let dest = bin_dir.join(file_path.file_name().unwrap());
            file::copy(file_path, &dest)?;
            file::make_executable(&dest)?;
        } else {
            let dest = install_path.join(file_path.file_name().unwrap());
            file::copy(file_path, &dest)?;
            file::make_executable(&dest)?;
        }
    } else {
        file::untar(file_path, &install_path, &tar_opts)?;
    }
    Ok(())
}

pub fn verify_artifact(
    _tv: &crate::toolset::ToolVersion,
    file_path: &Path,
    opts: &crate::toolset::ToolVersionOptions,
) -> Result<()> {
    // Check checksum if specified
    if let Some(checksum) = opts.get("checksum") {
        verify_checksum_str(file_path, checksum)?;
    }

    // Check size if specified
    if let Some(size_str) = opts.get("size") {
        let expected_size: u64 = size_str.parse()?;
        let actual_size = file_path.metadata()?.len();
        if actual_size != expected_size {
            bail!(
                "Size mismatch: expected {}, got {}",
                expected_size,
                actual_size
            );
        }
    }

    Ok(())
}

pub fn verify_checksum_str(file_path: &Path, checksum: &str) -> Result<()> {
    if let Some((algo, hash_str)) = checksum.split_once(':') {
        hash::ensure_checksum(file_path, hash_str, None, algo)?;
    } else {
        bail!("Invalid checksum format: {}", checksum);
    }
    Ok(())
}
