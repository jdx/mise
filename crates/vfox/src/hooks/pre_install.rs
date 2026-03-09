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

/// The type of attestation that was successfully verified.
///
/// Keep variants in sync with `ProvenanceType` in `src/lockfile.rs`.
/// Priority order (highest first): GithubAttestations > Slsa > Cosign.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifiedAttestation {
    /// GitHub artifact attestations (owner/repo, optional signer workflow).
    GithubAttestations {
        owner: String,
        repo: String,
        signer_workflow: Option<String>,
    },
    /// SLSA provenance verification.
    Slsa { provenance_path: PathBuf },
    /// Cosign signature/bundle verification.
    Cosign {
        sig_or_bundle_path: PathBuf,
        public_key_path: Option<PathBuf>,
    },
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
                validate_github_artifact_attestation_params(&table)?;
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

/// Validates that if one of the GitHub artifact attestation parameters are set, the other requisite
/// parameters are also set.
///
/// `github_repo` requires `github_owner` and vice versa, and `github_signer_workflow` requires
/// both aforementioned parameters.
fn validate_github_artifact_attestation_params(table: &Table) -> std::result::Result<(), LuaError> {
    if table.contains_key("github_owner")? && !table.contains_key("github_repo")? {
        return Err(LuaError::FromLuaConversionError {
            from: "table",
            to: "PreInstallAttestation".into(),
            message: Some("github_owner requires github_repo for artifact attestation".to_string()),
        });
    }

    if table.contains_key("github_repo")? && !table.contains_key("github_owner")? {
        return Err(LuaError::FromLuaConversionError {
            from: "table",
            to: "PreInstallAttestation".into(),
            message: Some("github_repo requires github_owner for artifact attestation".to_string()),
        });
    }

    if table.contains_key("github_signer_workflow")?
        && (!table.contains_key("github_owner")? || !table.contains_key("github_repo")?)
    {
        return Err(LuaError::FromLuaConversionError {
            from: "table",
            to: "PreInstallAttestation".into(),
            message: Some(
                "github_signer_workflow requires github_owner and github_repo for artifact attestation"
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
    use crate::hooks::pre_install::PreInstallAttestation;
    use crate::runtime::Runtime;
    use mlua::{FromLua, Lua};
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

    #[test]
    async fn test_runtime_env_type_is_nil_for_platform_override() {
        let plugin = Plugin::test("dummy");

        Runtime::set_env_type(Some("gnu".to_string()));

        let host_env_type: Option<String> = plugin
            .eval_async(chunk! {
                return RUNTIME.envType
            })
            .await
            .unwrap();
        assert_eq!(host_env_type, Some("gnu".to_string()));

        let target_runtime = Runtime::with_platform(plugin.dir.clone(), "linux", "amd64");
        let target_os = "linux".to_string();
        let target_arch = "amd64".to_string();
        let target_env_type: Option<String> = plugin
            .eval_async(chunk! {
                local saved_os = OS_TYPE
                local saved_arch = ARCH_TYPE
                local saved_runtime = RUNTIME
                OS_TYPE = $target_os
                ARCH_TYPE = $target_arch
                RUNTIME = $target_runtime
                local env_type = RUNTIME.envType
                OS_TYPE = saved_os
                ARCH_TYPE = saved_arch
                RUNTIME = saved_runtime
                return env_type
            })
            .await
            .unwrap();
        assert_eq!(target_env_type, None);

        Runtime::reset();
    }

    #[test]
    async fn test_attestation_plugin() {
        let pre_install = run("attestation", "1.2.3").await;
        assert_debug_snapshot!(pre_install);
    }

    #[test]
    async fn test_github_attestation_valid() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("github_owner", "owner").unwrap();
        table.set("github_repo", "repo").unwrap();
        let att = PreInstallAttestation::from_lua(mlua::Value::Table(table), &lua).unwrap();
        assert_eq!(att.github_owner, Some("owner".to_string()));
        assert_eq!(att.github_repo, Some("repo".to_string()));
        assert_eq!(att.github_signer_workflow, None);
    }

    #[test]
    async fn test_github_attestation_owner_without_repo() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("github_owner", "owner").unwrap();
        let result = PreInstallAttestation::from_lua(mlua::Value::Table(table), &lua);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("github_owner requires github_repo"),
            "unexpected error: {err}"
        );
    }

    #[test]
    async fn test_github_attestation_signer_without_owner_repo() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("github_signer_workflow", "wf.yml").unwrap();
        let result = PreInstallAttestation::from_lua(mlua::Value::Table(table), &lua);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("github_signer_workflow requires github_owner and github_repo"),
            "unexpected error: {err}"
        );
    }

    #[test]
    async fn test_cosign_public_key_without_sig() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("cosign_public_key_path", "/tmp/key.pub").unwrap();
        let result = PreInstallAttestation::from_lua(mlua::Value::Table(table), &lua);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("cosign_public_key_path requires cosign_sig_or_bundle_path"),
            "unexpected error: {err}"
        );
    }

    #[test]
    async fn test_slsa_min_level_without_provenance() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("slsa_min_level", 2).unwrap();
        let result = PreInstallAttestation::from_lua(mlua::Value::Table(table), &lua);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("slsa_min_level requires slsa_provenance_path"),
            "unexpected error: {err}"
        );
    }

    async fn run(plugin: &str, v: &str) -> PreInstall {
        let p = Plugin::test(plugin);
        p.pre_install(v).await.unwrap()
    }
}
