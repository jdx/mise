use super::*;

pub(super) fn apply_override(mut orig: AquaPackage, avo: &AquaPackage) -> AquaPackage {
    if let Some(r#type) = avo.r#type {
        orig.r#type = Some(r#type);
    }
    if !avo.repo_owner.is_empty() {
        orig.repo_owner = avo.repo_owner.clone();
    }
    if !avo.repo_name.is_empty() {
        orig.repo_name = avo.repo_name.clone();
    }
    if let Some(crate_name) = avo.crate_name.clone() {
        orig.crate_name = Some(crate_name);
    }
    if !avo.asset.is_empty() {
        orig.asset = avo.asset.clone();
    }
    if !avo.url.is_empty() {
        orig.url = avo.url.clone();
    }
    if !avo.format.is_empty() {
        orig.format = avo.format.clone();
    }
    if avo.rosetta2.is_some() {
        orig.rosetta2 = avo.rosetta2;
    }
    if avo.windows_arm_emulation.is_some() {
        orig.windows_arm_emulation = avo.windows_arm_emulation;
    }
    if avo.complete_windows_ext.is_some() {
        orig.complete_windows_ext = avo.complete_windows_ext;
    }
    if !avo.windows_ext.is_empty() {
        orig.windows_ext = avo.windows_ext.clone();
    }
    if avo.append_ext.is_some() {
        orig.append_ext = avo.append_ext;
    }
    if !avo.supported_envs.is_empty() {
        orig.supported_envs = avo.supported_envs.clone();
    }
    if !avo.files.is_empty() {
        orig.files = avo.files.clone();
    }
    if !avo.vars.is_empty() {
        orig.vars = avo.vars.clone();
    }
    orig.replacements.extend(avo.replacements.clone());
    if let Some(avo_version_prefix) = avo.version_prefix.clone() {
        orig.version_prefix = Some(avo_version_prefix);
    }
    if !avo.format_overrides.is_empty() {
        orig.format_overrides = avo.format_overrides.clone();
    }
    if !avo.overrides.is_empty() {
        orig.overrides = avo.overrides.clone();
    }

    if let Some(avo_checksum) = avo.checksum.clone() {
        match &mut orig.checksum {
            Some(checksum) => {
                checksum.merge(avo_checksum.clone());
            }
            None => {
                orig.checksum = Some(avo_checksum.clone());
            }
        }
    }

    if let Some(avo_cosign) = &avo.cosign {
        match &mut orig.cosign {
            Some(cosign) => {
                cosign.merge(avo_cosign.clone());
            }
            None => {
                orig.cosign = Some(avo_cosign.clone());
            }
        }
    }

    if let Some(avo_slsa_provenance) = avo.slsa_provenance.clone() {
        match &mut orig.slsa_provenance {
            Some(slsa_provenance) => {
                slsa_provenance.merge(avo_slsa_provenance.clone());
            }
            None => {
                orig.slsa_provenance = Some(avo_slsa_provenance.clone());
            }
        }
    }

    if let Some(avo_minisign) = avo.minisign.clone() {
        match &mut orig.minisign {
            Some(minisign) => {
                minisign.merge(avo_minisign.clone());
            }
            None => {
                orig.minisign = Some(avo_minisign.clone());
            }
        }
    }

    if let Some(avo_attestations) = avo.github_artifact_attestations.clone() {
        match &mut orig.github_artifact_attestations {
            Some(orig_attestations) => {
                orig_attestations.merge(avo_attestations.clone());
            }
            None => {
                orig.github_artifact_attestations = Some(avo_attestations.clone());
            }
        }
    }

    if avo.no_asset.is_some() {
        orig.no_asset = avo.no_asset;
    }
    if let Some(error_message) = avo.error_message.clone() {
        orig.error_message = Some(error_message);
    }
    if let Some(path) = avo.path.clone() {
        orig.path = Some(path);
    }
    orig
}
