use crate::config::env_directive::EnvResults;
use crate::env_diff;
use crate::file::display_path;
use crate::{Result, file, sops};
use eyre::{WrapErr, bail, eyre};
use indexmap::IndexMap;
use rops::file::format::{JsonFileFormat, YamlFileFormat};
use std::path::{Path, PathBuf};

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
    pub fn file(
        ctx: &mut tera::Context,
        tera: &mut tera::Tera,
        r: &mut EnvResults,
        normalize_path: fn(&Path, PathBuf) -> PathBuf,
        source: &Path,
        config_root: &Path,
        env_vars: &env_diff::EnvMap,
        input: String,
    ) -> Result<IndexMap<PathBuf, EnvMap>> {
        let mut out = IndexMap::new();
        let s = r.parse_template(ctx, tera, source, &input)?;
        let mut tmpenv = env_vars.clone();
        for p in xx::file::glob(normalize_path(config_root, s.into())).unwrap_or_default() {
            let new_env = out.entry(p.clone()).or_insert_with(IndexMap::new);
            let parse_template = |s: String| r.parse_template(ctx, tera, source, &s);
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            *new_env = match ext.as_str() {
                "json" => Self::json(&p, parse_template)?,
                "yaml" => Self::yaml(&p, parse_template)?,
                "toml" => Self::toml(&p)?,
                _ => Self::dotenv(&p, &tmpenv)?,
            };
            tmpenv.extend(new_env.clone());
        }
        Ok(out)
    }

    fn json<PT>(p: &Path, parse_template: PT) -> Result<EnvMap>
    where
        PT: FnMut(String) -> Result<String>,
    {
        let errfn = || eyre!("failed to parse json file: {}", display_path(p));
        if let Ok(raw) = file::read_to_string(p) {
            let mut f: Env<serde_json::Value> = serde_json::from_str(&raw).wrap_err_with(errfn)?;
            if !f.sops.is_empty() {
                let raw = sops::decrypt::<_, JsonFileFormat>(&raw, parse_template, "json")?;
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

    fn yaml<PT>(p: &Path, parse_template: PT) -> Result<EnvMap>
    where
        PT: FnMut(String) -> Result<String>,
    {
        let errfn = || eyre!("failed to parse yaml file: {}", display_path(p));
        if let Ok(raw) = file::read_to_string(p) {
            let mut f: Env<serde_yaml::Value> = serde_yaml::from_str(&raw).wrap_err_with(errfn)?;
            if !f.sops.is_empty() {
                let raw = sops::decrypt::<_, YamlFileFormat>(&raw, parse_template, "yaml")?;
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

    fn toml(p: &Path) -> Result<EnvMap> {
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

    fn dotenv(p: &Path, env: &env_diff::EnvMap) -> Result<EnvMap> {
        let errfn = || eyre!("failed to parse dotenv file: {}", display_path(p));
        let mut full_env = EnvMap::new();
        let mut new_env = EnvMap::new();

        // Convert env vars to string format
        let env_as_string = env
            .iter()
            .map(|(key, value)| format!("{}='{}'", key, value.replace("'", "\\'")))
            .collect::<Vec<_>>()
            .join("\n");

        // Read the file content and concatenate with env_as_string
        let file_content = file::read_to_string(p).unwrap_or_default();
        let combined_content = format!("{}\n{}", env_as_string, file_content);

        for item in dotenvy::from_read_iter(combined_content.as_bytes()) {
            let (k, v) = item.wrap_err_with(errfn)?;
            full_env.insert(k, v);
        }
        // A 2nd pass is needed to only keep new values
        if let Ok(dotenv) = dotenvy::from_path_iter(p) {
            for item in dotenv {
                let (k, _) = item.wrap_err_with(errfn)?;
                let value = full_env.get(&k).cloned().unwrap_or_default();
                new_env.insert(k, value);
            }
        }

        Ok(new_env)
    }
}
