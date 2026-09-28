//! Connect Homebrew's prefix and codesigning to the bottle relocation crate.

use std::path::{Path, PathBuf};

use eyre::bail;

use crate::result::Result;

pub(super) use mise_brew_relocation::RelocationReport;

pub(super) fn relocate_keg(
    keg: &Path,
    formula_name: &str,
    skip_linkage: bool,
) -> Result<RelocationReport> {
    mise_brew_relocation::relocate_keg(
        keg,
        formula_name,
        skip_linkage,
        &super::prefix::prefix(),
        &super::prefix::repository(),
    )
}

/// Ad-hoc re-sign modified Mach-O files — mandatory on arm64 macOS, where
/// the kernel kills binaries whose signature doesn't match their contents.
pub(super) fn codesign(files: &[PathBuf]) -> Result<()> {
    for file in files {
        let res = crate::cmd::cmd(
            "/usr/bin/codesign",
            [
                "--sign",
                "-",
                "--force",
                "--preserve-metadata=entitlements,requirements,flags,runtime",
                &file.to_string_lossy(),
            ],
        )
        .stderr_capture()
        .stdout_capture()
        .unchecked()
        .run()?;
        if !res.status.success() {
            bail!(
                "codesign failed for {}: {}",
                file.display(),
                String::from_utf8_lossy(&res.stderr).trim()
            );
        }
    }
    Ok(())
}
