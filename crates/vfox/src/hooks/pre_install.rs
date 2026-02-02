use std::path::PathBuf;

use mlua::prelude::LuaError;
use mlua::{FromLua, Lua, Table, Value};

use crate::Plugin;
use crate::error::Result;
use crate::runtime::Runtime;

impl Plugin {
    pub async fn pre_install(&self, version: &str) -> Result<PreInstall> {
        debug!("[vfox:{}] pre_install", &self.name);
        let ctx = self.context(Some(version.to_string()))?;
        let pre_install = self
            .eval_async(chunk! {
                require "hooks/pre_install"
                return PLUGIN:PreInstall($ctx)
            })
            .await?;

        Ok(pre_install)
    }

    pub async fn pre_install_for_platform(
        &self,
        version: &str,
        os: &str,
        arch: &str,
    ) -> Result<PreInstall> {
        debug!(
            "[vfox:{}] pre_install_for_platform os={} arch={}",
            &self.name, os, arch
        );
        let ctx = self.context(Some(version.to_string()))?;
        let target_os = os.to_string();
        let target_arch = arch.to_string();
        let target_runtime = Runtime::with_platform(self.dir.clone(), os, arch);
        let pre_install = self
            .eval_async(chunk! {
                require "hooks/pre_install"
                -- Override globals with target platform for cross-platform URL generation
                local saved_os = OS_TYPE
                local saved_arch = ARCH_TYPE
                local saved_runtime = RUNTIME
                OS_TYPE = $target_os
                ARCH_TYPE = $target_arch
                RUNTIME = $target_runtime
                local result = PLUGIN:PreInstall($ctx)
                -- Restore original values
                OS_TYPE = saved_os
                ARCH_TYPE = saved_arch
                RUNTIME = saved_runtime
                return result
            })
            .await?;

        Ok(pre_install)
    }
}

/// Optional attestation parameters provided by the return value of the preinstall hook.
#[derive(Debug)]
pub struct PreInstallAttestation {
    // GitHub
    pub github_owner: Option<String>,
    pub github_repo: Option<String>,
    pub github_signer_workflow: Option<String>,
    // Cosign
    pub cosign_sig_or_bundle_path: Option<PathBuf>,
    pub cosign_public_key_path: Option<PathBuf>,
    // SLSA
    pub slsa_provenance_path: Option<PathBuf>,
    pub slsa_min_level: Option<u8>,
}

impl FromLua for PreInstallAttestation {
    fn from_lua(value: Value, _: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            Value::Table(table) => {
                validate_github_attestation_params(&table)?;
                validate_cosign_attestation_params(&table)?;
                validate_slsa_attestation_params(&table)?;

                Ok(PreInstallAttestation {
                    github_owner: table.get::<Option<String>>("github_owner")?,
                    github_repo: table.get::<Option<String>>("github_repo")?,
                    github_signer_workflow: table
                        .get::<Option<String>>("github_signer_workflow")?,
                    cosign_sig_or_bundle_path: table
                        .get::<Option<PathBuf>>("cosign_sig_or_bundle_path")?,
                    cosign_public_key_path: table
                        .get::<Option<PathBuf>>("cosign_public_key_path")?,
                    slsa_provenance_path: table.get::<Option<PathBuf>>("slsa_provenance_path")?,
                    slsa_min_level: table.get::<Option<u8>>("slsa_min_level")?,
                })
            }
            _ => Err(LuaError::FromLuaConversionError {
                from: "table",
                to: "PreInstallAttestation".into(),
                message: Some("expected table for attestation field".to_string()),
            }),
        }
    }
}

/// Validates that if one of the GitHub attestation parameters are set, the other requisite
/// parameters are also set.
///
/// `github_repo` requires `github_owner` and vice versa, and `github_signer_workflow` requires
/// both aforementioned parameters.
fn validate_github_attestation_params(table: &Table) -> std::result::Result<(), LuaError> {
    if table.contains_key("github_owner")? && !table.contains_key("github_repo")? {
        return Err(LuaError::FromLuaConversionError {
            from: "table",
            to: "PreInstallAttestation".into(),
            message: Some("github_owner requires github_repo for attestation".to_string()),
        });
    }

    if table.contains_key("github_repo")? && !table.contains_key("github_owner")? {
        return Err(LuaError::FromLuaConversionError {
            from: "table",
            to: "PreInstallAttestation".into(),
            message: Some("github_repo requires github_owner for attestation".to_string()),
        });
    }

    if table.contains_key("github_signer_workflow")?
        && (!table.contains_key("github_owner")? || !table.contains_key("github_repo")?)
    {
        return Err(LuaError::FromLuaConversionError {
            from: "table",
            to: "PreInstallAttestation".into(),
            message: Some(
                "github_signer_workflow requires github_owner and github_repo for attestation"
                    .to_string(),
            ),
        });
    }

    Ok(())
}

/// Validates that if the public key path is set, then the sig/bundle path must also be set.
fn validate_cosign_attestation_params(table: &Table) -> std::result::Result<(), LuaError> {
    if table.contains_key("cosign_public_key_path")?
        && !table.contains_key("cosign_sig_or_bundle_path")?
    {
        return Err(LuaError::FromLuaConversionError {
            from: "table",
            to: "PreInstallAttestation".into(),
            message: Some(
                "cosign_public_key_path requires cosign_sig_or_bundle_path for attestation"
                    .to_string(),
            ),
        });
    }

    Ok(())
}

/// Validates that if the SLSA min level is set, then the provenance path must also be set.
fn validate_slsa_attestation_params(table: &Table) -> std::result::Result<(), LuaError> {
    if table.contains_key("slsa_min_level")? && !table.contains_key("slsa_provenance_path")? {
        return Err(LuaError::FromLuaConversionError {
            from: "table",
            to: "PreInstallAttestation".into(),
            message: Some(
                "slsa_min_level requires slsa_provenance_path for attestation".to_string(),
            ),
        });
    }

    Ok(())
}

#[derive(Debug)]
pub struct PreInstall {
    pub version: String,
    pub url: Option<String>,
    pub note: Option<String>,
    pub sha256: Option<String>,
    pub md5: Option<String>,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub attestation: Option<PreInstallAttestation>,
    // pub addition: Option<Table>,
}

impl FromLua for PreInstall {
    fn from_lua(value: Value, _: &Lua) -> std::result::Result<Self, LuaError> {
        match value {
            Value::Table(table) => {
                if !table.contains_key("version")? {
                    return Err(LuaError::FromLuaConversionError {
                        from: "table",
                        to: "PreInstall".into(),
                        message: Some("no version returned from vfox plugin".to_string()),
                    });
                }
                Ok(PreInstall {
                    version: table.get::<String>("version")?,
                    url: table.get::<Option<String>>("url")?,
                    note: table.get::<Option<String>>("note")?,
                    sha256: table.get::<Option<String>>("sha256")?,
                    md5: table.get::<Option<String>>("md5")?,
                    sha1: table.get::<Option<String>>("sha1")?,
                    sha512: table.get::<Option<String>>("sha512")?,
                    attestation: table.get::<Option<PreInstallAttestation>>("attestation")?,
                    // addition,
                })
            }
            _ => panic!("Expected table"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Plugin;
    use crate::hooks::pre_install::PreInstall;
    use crate::runtime::Runtime;
    use std::string::ToString;
    use tokio::test;

    #[test]
    async fn dummy() {
        let pre_install = run("dummy", "1.0.1").await;
        assert_debug_snapshot!(pre_install);
    }

    #[test]
    async fn test_nodejs() {
        Runtime::set_os("linux".to_string());
        Runtime::set_arch("x64".to_string());
        let pre_install = run("test-nodejs", "20.0.0").await;
        assert_debug_snapshot!(pre_install);

        Runtime::set_os("macos".to_string());
        Runtime::set_arch("arm64".to_string());
        let pre_install = run("test-nodejs", "20.1.0").await;
        assert_debug_snapshot!(pre_install);

        Runtime::set_os("windows".to_string());
        Runtime::set_arch("x64".to_string());
        let pre_install = run("test-nodejs", "20.3.0").await;
        assert_debug_snapshot!(pre_install);

        Runtime::reset();
    }

    async fn run(plugin: &str, v: &str) -> PreInstall {
        let p = Plugin::test(plugin);
        p.pre_install(v).await.unwrap()
    }
}
