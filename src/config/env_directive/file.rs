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
        tera: &mut Option<crate::tera::TeraEngine>,
        r: &mut EnvResults,
        normalize_path: fn(&Path, PathBuf) -> PathBuf,
        source: &Path,
        exec_env: &TeraEnvMap,
        config_root: &Path,
        input: String,
    ) -> Result<IndexMap<PathBuf, EnvMap>> {
        let mut out = IndexMap::new();
        let s = r.parse_template(ctx, tera, source, exec_env, &input)?;
        let shell_expand = crate::config::Settings::get().env_shell_expand;
        // Accumulate loaded vars so a later file in the same `_.file` directive can
        // reference vars defined by an earlier one (matching how separate `[[env]]`
        // blocks accumulate through `resolve`). See discussion #3897.
        let mut acc: TeraEnvMap = exec_env.clone();
        for p in xx::file::glob(normalize_path(config_root, s.into())).unwrap_or_default() {
            let parse_template = |s: String| r.parse_template(ctx, tera, source, exec_env, &s);
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            let mut loaded = match ext.as_str() {
                "json" => Self::json(config, exec_env, &p, parse_template).await?,
                "yaml" => Self::yaml(config, exec_env, &p, parse_template).await?,
                "toml" => Self::toml(config, exec_env, &p, parse_template).await?,
                _ => Self::dotenv(&p, &acc, shell_expand).await?,
            };
            // Structured files keep `${VAR}` intact (serde doesn't expand), so run
            // their values through the same `$VAR` engine used for `KEY = value`
            // vars. Expand and accumulate key-by-key so a later value in the same
            // file can reference an earlier one (e.g. `BIN = "${BASE}/bin"`), and
            // warn on undefined refs like the normal expansion path does. dotenv is
            // expanded inside `Self::dotenv` via dotenvy itself.
            if shell_expand && matches!(ext.as_str(), "json" | "yaml" | "toml") {
                for (k, v) in loaded.iter_mut() {
                    let mut missing = Vec::new();
                    let expanded = super::shell_expand_env(&*v, &acc, &mut missing);
                    for var in missing {
                        warn_once!(
                            "env var '{var}' is not defined and will be left unexpanded. \
                             Use ${{{var}:-}} to default to an empty string and suppress \
                             this warning."
                        );
                    }
                    *v = expanded;
                    acc.insert(k.clone(), v.clone());
                }
            } else {
                for (k, v) in &loaded {
                    acc.insert(k.clone(), v.clone());
                }
            }
            out.insert(p, loaded);
        }
        Ok(out)
    }

    async fn json<PT>(
        config: &Arc<Config>,
        exec_env: &TeraEnvMap,
        p: &Path,
        parse_template: PT,
    ) -> Result<EnvMap>
    where
        PT: FnMut(String) -> Result<String>,
    {
        let errfn = || eyre!("failed to parse json file: {}", display_path(p));
        if let Ok(raw) = file::read_to_string(p) {
            let mut f: Env<serde_json::Value> = serde_json::from_str(&raw).wrap_err_with(errfn)?;
            if !f.sops.is_empty() {
                let decrypted = sops::decrypt::<_, JsonFileFormat>(
                    config,
                    exec_env,
                    &raw,
                    parse_template,
                    "json",
                )
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

    async fn yaml<PT>(
        config: &Arc<Config>,
        exec_env: &TeraEnvMap,
        p: &Path,
        parse_template: PT,
    ) -> Result<EnvMap>
    where
        PT: FnMut(String) -> Result<String>,
    {
        let errfn = || eyre!("failed to parse yaml file: {}", display_path(p));
        if let Ok(raw) = file::read_to_string(p) {
            let mut f: Env<serde_yaml::Value> = serde_yaml::from_str(&raw).wrap_err_with(errfn)?;
            if !f.sops.is_empty() {
                let decrypted = sops::decrypt::<_, YamlFileFormat>(
                    config,
                    exec_env,
                    &raw,
                    parse_template,
                    "yaml",
                )
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

    async fn toml<PT>(
        config: &Arc<Config>,
        exec_env: &TeraEnvMap,
        p: &Path,
        parse_template: PT,
    ) -> Result<EnvMap>
    where
        PT: FnMut(String) -> Result<String>,
    {
        let errfn = || eyre!("failed to parse toml file: {}", display_path(p));
        if let Ok(raw) = file::read_to_string(p) {
            let mut f: Env<toml::Value> = toml::from_str(&raw).wrap_err_with(errfn)?;
            if !f.sops.is_empty() {
                let decrypted = sops::decrypt::<_, TomlFileFormat>(
                    config,
                    exec_env,
                    &raw,
                    parse_template,
                    "toml",
                )
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

    async fn dotenv(p: &Path, acc: &TeraEnvMap, seed: bool) -> Result<EnvMap> {
        let errfn = || eyre!("failed to parse dotenv file: {}", display_path(p));
        if !seed {
            // env_shell_expand disabled: preserve the original behavior exactly
            // (dotenvy substitutes against the process env + same-file vars only).
            let mut env = EnvMap::new();
            if let Ok(dotenv) = dotenvy::from_path_iter(p) {
                for item in dotenv {
                    let (k, v) = item.wrap_err_with(errfn)?;
                    env.insert(k, v);
                }
            }
            return Ok(env);
        }
        // dotenvy substitutes `${VAR}` only against the process env + vars defined
        // earlier in the *same* file, collapsing anything else to "" — and 0.15 has
        // no API to disable substitution or supply a custom map. To make cross-file
        // references resolve (discussion #3897) we reuse dotenvy's own parser by
        // prepending the accumulated env as escaped `KEY="..."` lines, then keep
        // only the keys the file itself defines.
        let Ok(content) = file::read_to_string(p) else {
            return Ok(EnvMap::new());
        };
        // Keys the file itself defines (values here may be collapsed; we only need
        // the key set to filter the seeded parse below).
        let mut own_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
        for item in dotenvy::from_read_iter(content.as_bytes()) {
            let (k, _v) = item.wrap_err_with(errfn)?;
            own_keys.insert(k);
        }
        if own_keys.is_empty() {
            return Ok(EnvMap::new());
        }
        // Seed lines for cross-file vars. dotenvy resolves `${VAR}` against the
        // process env first and only then this prepended (same-file) data, so the
        // seed fills genuinely-missing refs. This intentionally preserves dotenv's
        // own substitution semantics; the one gap is that an ambient export still
        // wins over a mise override of the same name for dotenv values (there is no
        // dotenvy API to supply a custom substitution map).
        let mut prefix = String::new();
        for (k, v) in acc {
            if own_keys.contains(k) || !is_env_key(k) {
                continue;
            }
            prefix.push_str(k);
            prefix.push_str("=\"");
            prefix.push_str(&escape_dotenv_double_quoted(v));
            prefix.push_str("\"\n");
        }
        let augmented = format!("{prefix}{content}");
        let mut env = EnvMap::new();
        for item in dotenvy::from_read_iter(augmented.as_bytes()) {
            let (k, v) = item.wrap_err_with(errfn)?;
            if own_keys.contains(&k) {
                env.insert(k, v);
            }
        }
        Ok(env)
    }
}

/// Whether `k` is a valid env var name we can safely emit as a dotenv key.
fn is_env_key(k: &str) -> bool {
    let mut chars = k.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Escape a value for a dotenv double-quoted context so dotenvy stores it
/// verbatim (no re-substitution). dotenvy 0.15 only supports the `\\ \" \$ \n`
/// escapes inside double quotes — emitting any other backslash escape is a parse
/// error — so we escape exactly those and leave everything else literal.
fn escape_dotenv_double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
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
    static ENV_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn encrypted_toml() -> String {
        RopsFileBuilder::<TomlFileFormat>::new(r#"SECRET = "mysecret""#)
            .unwrap()
            .add_integration_key::<AgeIntegration>(
                AgeIntegration::parse_key_id(AGE_PUBLIC_KEY).unwrap(),
            )
            .encrypt::<AES256GCM, SHA512>()
            .unwrap()
            .to_string()
    }

    fn restore_env_var(key: &str, prev: Option<String>) {
        match prev {
            Some(v) => crate::env::set_var(key, v),
            None => crate::env::remove_var(key),
        }
    }

    #[tokio::test]
    async fn decrypts_sops_toml_file() {
        let _lock = ENV_MUTEX.lock().await;
        let prev_age_key = crate::env::var("MISE_SOPS_AGE_KEY").ok();
        let prev_rops = crate::env::var("MISE_SOPS_ROPS").ok();
        crate::env::remove_var("MISE_SOPS_ROPS");
        crate::env::set_var("MISE_SOPS_AGE_KEY", AGE_PRIVATE_KEY);
        Settings::reset(None);
        let config = Config::reset().await.unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join(".env.toml");

        file::write(&p, encrypted_toml()).unwrap();

        let exec_env = TeraEnvMap::new();
        let env = EnvResults::toml(&config, &exec_env, &p, Ok).await.unwrap();
        assert_eq!(env.get("SECRET").unwrap(), "mysecret");

        restore_env_var("MISE_SOPS_AGE_KEY", prev_age_key);
        restore_env_var("MISE_SOPS_ROPS", prev_rops);
        Settings::reset(None);
    }

    #[tokio::test]
    async fn decrypts_sops_toml_file_with_exec_env_mise_age_key_file() {
        let _lock = ENV_MUTEX.lock().await;
        let prev_age_key = crate::env::var("MISE_SOPS_AGE_KEY").ok();
        let prev_age_key_file = crate::env::var("MISE_SOPS_AGE_KEY_FILE").ok();
        let prev_rops = crate::env::var("MISE_SOPS_ROPS").ok();
        crate::env::remove_var("MISE_SOPS_AGE_KEY");
        crate::env::remove_var("MISE_SOPS_AGE_KEY_FILE");
        crate::env::remove_var("MISE_SOPS_ROPS");
        Settings::reset(None);
        let config = Config::reset().await.unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join(".env.toml");
        let key_file = tmp.path().join("age.txt");
        file::write(&p, encrypted_toml()).unwrap();
        file::write(&key_file, AGE_PRIVATE_KEY).unwrap();

        let mut exec_env = TeraEnvMap::new();
        exec_env.insert(
            "MISE_SOPS_AGE_KEY_FILE".into(),
            key_file.to_string_lossy().to_string(),
        );
        let env = EnvResults::toml(&config, &exec_env, &p, Ok).await.unwrap();
        assert_eq!(env.get("SECRET").unwrap(), "mysecret");

        restore_env_var("MISE_SOPS_AGE_KEY", prev_age_key);
        restore_env_var("MISE_SOPS_AGE_KEY_FILE", prev_age_key_file);
        restore_env_var("MISE_SOPS_ROPS", prev_rops);
        Settings::reset(None);
    }

    #[tokio::test]
    async fn ambient_sops_age_key_file_precedes_exec_env_sops_age_key() {
        let _lock = ENV_MUTEX.lock().await;
        let prev_mise_age_key = crate::env::var("MISE_SOPS_AGE_KEY").ok();
        let prev_sops_age_key = crate::env::var("SOPS_AGE_KEY").ok();
        let prev_sops_age_key_file = crate::env::var("SOPS_AGE_KEY_FILE").ok();
        let prev_rops = crate::env::var("MISE_SOPS_ROPS").ok();
        crate::env::remove_var("MISE_SOPS_AGE_KEY");
        crate::env::remove_var("SOPS_AGE_KEY");
        crate::env::remove_var("MISE_SOPS_ROPS");
        Settings::reset(None);
        let config = Config::reset().await.unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join(".env.toml");
        let key_file = tmp.path().join("age.txt");
        file::write(&p, encrypted_toml()).unwrap();
        file::write(&key_file, AGE_PRIVATE_KEY).unwrap();
        crate::env::set_var("SOPS_AGE_KEY_FILE", key_file.to_string_lossy().to_string());

        let mut exec_env = TeraEnvMap::new();
        exec_env.insert("SOPS_AGE_KEY".into(), "not-an-age-key".into());
        let env = EnvResults::toml(&config, &exec_env, &p, Ok).await.unwrap();
        assert_eq!(env.get("SECRET").unwrap(), "mysecret");

        restore_env_var("MISE_SOPS_AGE_KEY", prev_mise_age_key);
        restore_env_var("SOPS_AGE_KEY", prev_sops_age_key);
        restore_env_var("SOPS_AGE_KEY_FILE", prev_sops_age_key_file);
        restore_env_var("MISE_SOPS_ROPS", prev_rops);
        Settings::reset(None);
    }

    #[tokio::test]
    async fn errors_when_sops_cli_is_configured_for_toml_file() {
        let _lock = ENV_MUTEX.lock().await;
        let prev_age_key = crate::env::var("MISE_SOPS_AGE_KEY").ok();
        let prev_rops = crate::env::var("MISE_SOPS_ROPS").ok();
        crate::env::set_var("MISE_SOPS_AGE_KEY", AGE_PRIVATE_KEY);
        crate::env::set_var("MISE_SOPS_ROPS", "0");
        Settings::reset(None);
        let config = Config::reset().await.unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join(".env.toml");

        file::write(&p, encrypted_toml()).unwrap();

        let exec_env = TeraEnvMap::new();
        let err = EnvResults::toml(&config, &exec_env, &p, Ok)
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("sops.rops=false is not supported for TOML SOPS files"),
            "{err}"
        );

        restore_env_var("MISE_SOPS_AGE_KEY", prev_age_key);
        restore_env_var("MISE_SOPS_ROPS", prev_rops);
        Settings::reset(None);
    }

    #[test]
    fn test_escape_dotenv_double_quoted() {
        assert_eq!(escape_dotenv_double_quoted("plain"), "plain");
        assert_eq!(escape_dotenv_double_quoted(r#"a$b"c\d"#), r#"a\$b\"c\\d"#);
        assert_eq!(escape_dotenv_double_quoted("l1\nl2"), "l1\\nl2");
    }

    #[test]
    fn test_is_env_key() {
        assert!(is_env_key("PGHOST"));
        assert!(is_env_key("_FOO123"));
        assert!(!is_env_key("1FOO"));
        assert!(!is_env_key("FOO-BAR"));
        assert!(!is_env_key(""));
    }
}
