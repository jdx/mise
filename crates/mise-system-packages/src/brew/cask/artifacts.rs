use super::*;

pub(super) fn validate_platform_support(cask: &Cask, artifacts: &CaskArtifacts) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let font_only = !artifacts.fonts.is_empty()
            && artifacts.apps.is_empty()
            && artifacts.binaries.is_empty()
            && artifacts.command_wrappers.is_empty()
            && artifacts.pkgs.is_empty()
            && artifacts.installers.is_empty()
            && artifacts.generic.is_empty()
            && artifacts.completions.is_empty()
            && artifacts.generated_completions.is_empty()
            && artifacts.preflight_steps.is_empty()
            && artifacts.postflight_steps.is_empty()
            && !has_lifecycle_hook(cask, "preflight")
            && !has_lifecycle_hook(cask, "postflight");
        if !font_only {
            bail!(
                "{}:{}: only font-only casks without lifecycle hooks are supported on linux",
                cask.label(),
                cask.token
            );
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (cask, artifacts);
    Ok(())
}

pub(super) fn platform_unavailable_state(
    cask: &Cask,
    artifacts: &CaskArtifacts,
) -> Option<PackageState> {
    validate_platform_support(cask, artifacts)
        .err()
        .map(|err| PackageState::unavailable(err.to_string()))
}
