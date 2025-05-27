use crate::config::{Config, env_directive::EnvResults};
use crate::file::display_path;
use crate::{Result, file, sops};
use eyre::{WrapErr, bail, eyre};
use indexmap::IndexMap;
use rops::file::format::{JsonFileFormat, YamlFileFormat};
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
        tera: &mut tera::Tera,
        r: &mut EnvResults,
        normalize_path: fn(&Path, PathBuf) -> PathBuf,
        source: &Path,
        config_root: &Path,
        input: String,
    ) -> Result<IndexMap<PathBuf, EnvMap>> {
        let mut out = IndexMap::new();
        let s = r.parse_template(ctx, tera, source, &input)?;
        for p in xx::file::glob(normalize_path(config_root, s.into())).unwrap_or_default() {
            let env = out.entry(p.clone()).or_insert_with(IndexMap::new);
            let parse_template = |s: String| r.parse_template(ctx, tera, source, &s);
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            *env = match ext.as_str() {
                "json" => Self::json(config, &p, parse_template).await?,
                "yaml" => Self::yaml(config, &p, parse_template).await?,
                "toml" => Self::toml(&p).await?,
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
                let raw = sops::decrypt::<_, JsonFileFormat>(config, &raw, parse_template, "json")
                    .await?;
                f = serde_json::from_str(&raw).wrap_err_with(errfn)?;
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
                let raw = sops::decrypt::<_, YamlFileFormat>(config, &raw, parse_template, "yaml")
                    .await?;
                f = serde_yaml::from_str(&raw).wrap_err_with(errfn)?;
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

    async fn toml(p: &Path) -> Result<EnvMap> {
        let errfn = || eyre!("failed to parse toml file: {}", display_path(p));
        // sops does not support toml yet, so no need to parse sops
        if let Ok(raw) = file::read_to_string(p) {
            toml::from_str::<Env<toml::Value>>(&raw)
                .wrap_err_with(errfn)?
                .env
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
