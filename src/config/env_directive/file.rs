use crate::config::{Config, env_directive::EnvResults};
use crate::env_diff::EnvMap as TeraEnvMap;
use crate::file::display_path;
use crate::{Result, file, sops};
use eyre::{WrapErr, bail, eyre};
use indexmap::IndexMap;
use rops::file::format::{JsonFileFormat, TomlFileFormat, YamlFileFormat};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

// use indexmap so source is after value for `mise env --json` output
type EnvMap = IndexMap<String, String>;

#[derive(serde::Serialize, serde::Deserialize)]
struct Env<V> {
    #[serde(default = "IndexMap::new")]
    sops: IndexMap<String, V>,
    #[serde(flatten)]
    env: IndexMap<String, V>,
}

impl EnvResults {
    #[allow(clippy::too_many_arguments)]
    pub async fn file(
        config: &Arc<Config>,
        ctx: &mut tera::Context,
        tera: &mut Option<tera::Tera>,
        r: &mut EnvResults,
        normalize_path: fn(&Path, PathBuf) -> PathBuf,
        source: &Path,
        exec_env: &TeraEnvMap,
        config_root: &Path,
        input: String,
    ) -> Result<IndexMap<PathBuf, EnvMap>> {
        let mut out = IndexMap::new();
        let s = r.parse_template(ctx, tera, source, exec_env, &input)?;
        for p in xx::file::glob(normalize_path(config_root, s.into())).unwrap_or_default() {
            let env = out.entry(p.clone()).or_insert_with(IndexMap::new);
            let parse_template = |s: String| r.parse_template(ctx, tera, source, exec_env, &s);
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            *env = match ext.as_str() {
                "json" => Self::json(config, &p, parse_template).await?,
                "yaml" => Self::yaml(config, &p, parse_template).await?,
                "toml" => Self::toml(config, &p, parse_template).await?,
                _ => Self::dotenv(&p).await?,
            };
        }
        Ok(out)
    }

    async fn json<PT>(config: &Arc<Config>, p: &Path, parse_template: PT) -> Result<EnvMap>
    where
        PT: FnMut(String) -> Result<String>,
    {
        let errfn = || eyre!("failed to parse json file: {}", display_path(p));
        if let Ok(raw) = file::read_to_string(p) {
            let mut f: Env<serde_json::Value> = serde_json::from_str(&raw).wrap_err_with(errfn)?;
            if !f.sops.is_empty() {
                let decrypted =
                    sops::decrypt::<_, JsonFileFormat>(config, &raw, parse_template, "json")
                        .await?;
                if !decrypted.is_empty() {
                    f = serde_json::from_str(&decrypted).wrap_err_with(errfn)?;
                } else {
                    return Ok(EnvMap::new());
                }
            }
            f.env
                .into_iter()
                .map(|(k, v)| {
                    Ok((
                        k,
                        match v {
                            serde_json::Value::String(s) => s,
                            serde_json::Value::Number(n) => n.to_string(),
                            serde_json::Value::Bool(b) => b.to_string(),
                            _ => bail!("unsupported json value: {v:?}"),
                        },
                    ))
                })
                .collect()
        } else {
            Ok(EnvMap::new())
        }
    }

    async fn yaml<PT>(config: &Arc<Config>, p: &Path, parse_template: PT) -> Result<EnvMap>
    where
        PT: FnMut(String) -> Result<String>,
    {
        let errfn = || eyre!("failed to parse yaml file: {}", display_path(p));
        if let Ok(raw) = file::read_to_string(p) {
            let mut f: Env<serde_yaml::Value> = serde_yaml::from_str(&raw).wrap_err_with(errfn)?;
            if !f.sops.is_empty() {
                let decrypted =
                    sops::decrypt::<_, YamlFileFormat>(config, &raw, parse_template, "yaml")
                        .await?;
                if !decrypted.is_empty() {
                    f = serde_yaml::from_str(&decrypted).wrap_err_with(errfn)?;
                } else {
                    return Ok(EnvMap::new());
                }
            }
            f.env
                .into_iter()
                .map(|(k, v)| {
                    Ok((
                        k,
                        match v {
                            serde_yaml::Value::String(s) => s,
                            serde_yaml::Value::Number(n) => n.to_string(),
                            serde_yaml::Value::Bool(b) => b.to_string(),
                            _ => bail!("unsupported yaml value: {v:?}"),
                        },
                    ))
                })
                .collect()
        } else {
            Ok(EnvMap::new())
        }
    }

    async fn toml<PT>(config: &Arc<Config>, p: &Path, parse_template: PT) -> Result<EnvMap>
    where
        PT: FnMut(String) -> Result<String>,
    {
        let errfn = || eyre!("failed to parse toml file: {}", display_path(p));
        if let Ok(raw) = file::read_to_string(p) {
            let mut f: Env<toml::Value> = toml::from_str(&raw).wrap_err_with(errfn)?;
            if !f.sops.is_empty() {
                let decrypted =
                    sops::decrypt::<_, TomlFileFormat>(config, &raw, parse_template, "toml")
                        .await?;
                if !decrypted.is_empty() {
                    f = toml::from_str(&decrypted).wrap_err_with(errfn)?;
                } else {
                    return Ok(EnvMap::new());
                }
            }
            f.env
                .into_iter()
                .map(|(k, v)| {
                    Ok((
                        k,
                        match v {
                            toml::Value::String(s) => s,
                            toml::Value::Integer(n) => n.to_string(),
                            toml::Value::Boolean(b) => b.to_string(),
                            _ => bail!("unsupported toml value: {v:?}"),
                        },
                    ))
                })
                .collect()
        } else {
            Ok(EnvMap::new())
        }
    }

    async fn dotenv(p: &Path) -> Result<EnvMap> {
        let errfn = || eyre!("failed to parse dotenv file: {}", display_path(p));
        let mut env = EnvMap::new();
        if let Ok(dotenv) = dotenvy::from_path_iter(p) {
            for item in dotenv {
                let (k, v) = item.wrap_err_with(errfn)?;
                env.insert(k, v);
            }
        }
        Ok(env)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use rops::{
        cryptography::{cipher::AES256GCM, hasher::SHA512},
        file::builder::RopsFileBuilder,
        integration::{AgeIntegration, Integration},
    };

    const AGE_PUBLIC_KEY: &str = "age1se5ghfycr4n8kcwc3qwf234ymvmr2lex2a99wh8gpfx97glwt9hqch4569";
    const AGE_PRIVATE_KEY: &str =
        "AGE-SECRET-KEY-1EQUCGFZH8UZKSZ0Z5N5T234YRNDT4U9H7QNYXWRRNJYDDVXE6FWSCPGNJ7";

    #[tokio::test]
    async fn decrypts_sops_toml_file() {
        let prev_age_key = crate::env::var("MISE_SOPS_AGE_KEY").ok();
        let prev_rops = crate::env::var("MISE_SOPS_ROPS").ok();
        crate::env::remove_var("MISE_SOPS_ROPS");
        crate::env::set_var("MISE_SOPS_AGE_KEY", AGE_PRIVATE_KEY);
        Settings::reset(None);
        let config = Config::reset().await.unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join(".env.toml");

        let encrypted = RopsFileBuilder::<TomlFileFormat>::new(r#"SECRET = "mysecret""#)
            .unwrap()
            .add_integration_key::<AgeIntegration>(
                AgeIntegration::parse_key_id(AGE_PUBLIC_KEY).unwrap(),
            )
            .encrypt::<AES256GCM, SHA512>()
            .unwrap()
            .to_string();
        file::write(&p, encrypted).unwrap();

        let env = EnvResults::toml(&config, &p, Ok).await.unwrap();
        assert_eq!(env.get("SECRET").unwrap(), "mysecret");

        match prev_age_key {
            Some(v) => crate::env::set_var("MISE_SOPS_AGE_KEY", v),
            None => crate::env::remove_var("MISE_SOPS_AGE_KEY"),
        }
        match prev_rops {
            Some(v) => crate::env::set_var("MISE_SOPS_ROPS", v),
            None => crate::env::remove_var("MISE_SOPS_ROPS"),
        }
        Settings::reset(None);
    }

    #[tokio::test]
    async fn errors_when_sops_cli_is_configured_for_toml_file() {
        let prev_age_key = crate::env::var("MISE_SOPS_AGE_KEY").ok();
        let prev_rops = crate::env::var("MISE_SOPS_ROPS").ok();
        crate::env::set_var("MISE_SOPS_AGE_KEY", AGE_PRIVATE_KEY);
        crate::env::set_var("MISE_SOPS_ROPS", "0");
        Settings::reset(None);
        let config = Config::reset().await.unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join(".env.toml");

        let encrypted = RopsFileBuilder::<TomlFileFormat>::new(r#"SECRET = "mysecret""#)
            .unwrap()
            .add_integration_key::<AgeIntegration>(
                AgeIntegration::parse_key_id(AGE_PUBLIC_KEY).unwrap(),
            )
            .encrypt::<AES256GCM, SHA512>()
            .unwrap()
            .to_string();
        file::write(&p, encrypted).unwrap();

        let err = EnvResults::toml(&config, &p, Ok).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("sops.rops=false is not supported for TOML SOPS files"),
            "{err}"
        );

        match prev_age_key {
            Some(v) => crate::env::set_var("MISE_SOPS_AGE_KEY", v),
            None => crate::env::remove_var("MISE_SOPS_AGE_KEY"),
        }
        match prev_rops {
            Some(v) => crate::env::set_var("MISE_SOPS_ROPS", v),
            None => crate::env::remove_var("MISE_SOPS_ROPS"),
        }
        Settings::reset(None);
    }
}
