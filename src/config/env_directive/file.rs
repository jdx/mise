use crate::config::{
    Config,
    env_directive::{EnvDirectiveContext, EnvResults},
};
use crate::env_diff::EnvMap as TeraEnvMap;
use crate::file::display_path;
use crate::{Result, file, sops};
use eyre::{WrapErr, bail, eyre};
use indexmap::IndexMap;
use rops::file::format::{JsonFileFormat, TomlFileFormat, YamlFileFormat};
use std::{
    fs,
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
    pub(super) async fn file(
        ctx: &mut EnvDirectiveContext<'_>,
        input: String,
        expand: bool,
    ) -> Result<IndexMap<PathBuf, EnvMap>> {
        let mut out = IndexMap::new();
        let s = ctx.parse_template("_.file", &input)?;
        let expand = expand && crate::config::Settings::get().env_shell_expand;
        // Accumulate loaded vars so opted-in expansion can reference values from
        // an earlier file in the same directive or an earlier env block.
        let mut acc: TeraEnvMap = ctx.exec_env.clone();
        for p in xx::file::glob(ctx.normalize_path(s.into())).unwrap_or_default() {
            let config = ctx.config;
            let exec_env = ctx.exec_env;
            // The loaders expand templates in values read from the file and do
            // not carry the key from inside it, so label by the file itself.
            let origin = display_path(&p);
            let parse_template = |s: String| ctx.parse_template(&origin, &s);
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            let mut sops_used = false;
            let mut loaded = match ext.as_str() {
                "json" => Self::json(config, exec_env, &p, parse_template, &mut sops_used).await?,
                "yaml" => Self::yaml(config, exec_env, &p, parse_template, &mut sops_used).await?,
                "toml" => Self::toml(config, exec_env, &p, parse_template, &mut sops_used).await?,
                _ => Self::dotenv(&p, &acc, expand).await?,
            };
            // Decrypted values must not be written to the env cache.
            if sops_used {
                ctx.results.has_uncacheable = true;
            }
            // Structured files are literal by default. With `expand = true`, run
            // their values through the same `$VAR` engine used by `[env]` values
            // and accumulate key-by-key for same-file references.
            if expand && matches!(ext.as_str(), "json" | "yaml" | "toml") {
                for (k, v) in loaded.iter_mut() {
                    let mut missing = Vec::new();
                    let expanded = super::shell_expand_env(&*v, &acc, &mut missing);
                    super::warn_unexpanded_vars(missing, k, &p);
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
        sops_used: &mut bool,
    ) -> Result<EnvMap>
    where
        PT: FnMut(String) -> Result<String>,
    {
        let errfn = || eyre!("failed to parse json file: {}", display_path(p));
        if let Ok(raw) = file::read_to_string(p) {
            // serde_json rejects a leading byte-order mark, so an env file saved by an editor that
            // writes one fails the whole config. Measured: yaml and toml accept it and are left
            // alone; json does not.
            let raw = file::strip_utf8_bom(&raw);
            let mut f: Env<serde_json::Value> = serde_json::from_str(raw).wrap_err_with(errfn)?;
            if !f.sops.is_empty() {
                *sops_used = true;
                let decrypted = sops::decrypt::<_, JsonFileFormat>(
                    config,
                    exec_env,
                    raw,
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
        sops_used: &mut bool,
    ) -> Result<EnvMap>
    where
        PT: FnMut(String) -> Result<String>,
    {
        let errfn = || eyre!("failed to parse yaml file: {}", display_path(p));
        if let Ok(raw) = file::read_to_string(p) {
            let mut f: Env<serde_yaml::Value> = serde_yaml::from_str(&raw).wrap_err_with(errfn)?;
            if !f.sops.is_empty() {
                *sops_used = true;
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
        sops_used: &mut bool,
    ) -> Result<EnvMap>
    where
        PT: FnMut(String) -> Result<String>,
    {
        let errfn = || eyre!("failed to parse toml file: {}", display_path(p));
        if let Ok(raw) = file::read_to_string(p) {
            let mut f: Env<toml::Value> = toml::from_str(&raw).wrap_err_with(errfn)?;
            if !f.sops.is_empty() {
                *sops_used = true;
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

    async fn dotenv(p: &Path, acc: &TeraEnvMap, expand: bool) -> Result<EnvMap> {
        let errfn = || eyre!("failed to parse dotenv file: {}", display_path(p));
        // Read here rather than letting dotenvy open the file, so a byte-order mark can be taken
        // off before the parser sees it. A mark belongs to the first key's name as far as dotenvy
        // is concerned, and one bad line fails the whole file — so a `.env` saved by an editor
        // that writes one is rejected entirely, naming a character nobody can see.
        //
        // `decode_text` rather than `read_to_string`: the latter is UTF-8 only, and Windows
        // PowerShell 5.1's `>` and `Out-File` write UTF-16LE by default, so a `.env` saved with
        // the shell that ships with the OS used to be thrown away here without a word.
        let Ok(bytes) = fs::read(p) else {
            // Unchanged: a file that cannot be opened yields nothing rather than an error, which
            // is what the original `if let Ok(..)` did. A glob can match a file that has since
            // gone, and that is not worth a diagnostic.
            return Ok(EnvMap::new());
        };
        let content = match file::decode_text(&bytes) {
            Ok(content) => content,
            Err(err) => {
                // Read and then discarded is a different thing from never opened, and it is the
                // silence this exists to end: the user wrote a file mise looked at and dropped.
                warn!("ignoring {}: {err:#}", display_path(p));
                return Ok(EnvMap::new());
            }
        };
        // `${VAR}` resolves against earlier assignments in this file first, so a file's own
        // values are never shadowed by variables that happen to be set already (e.g. exported
        // by `mise activate` from another `.env`). Only then do the surrounding values apply:
        // everything loaded so far when `expand = true`, otherwise the process environment.
        let outer: Vec<(String, String)> = if expand {
            acc.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        } else {
            mise_util::env::vars_without_inherited_secrets().collect()
        };
        let mut env = EnvMap::new();
        for (k, v) in mise_dotenv::parse(&content, true, outer).wrap_err_with(errfn)? {
            env.insert(k, v);
        }
        Ok(env)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Settings, SettingsExt};
    use rops::{
        cryptography::{cipher::AES256GCM, hasher::SHA512},
        file::builder::RopsFileBuilder,
        integration::{AgeIntegration, Integration},
    };

    const AGE_PUBLIC_KEY: &str = "age1se5ghfycr4n8kcwc3qwf234ymvmr2lex2a99wh8gpfx97glwt9hqch4569";
    const AGE_PRIVATE_KEY: &str =
        "AGE-SECRET-KEY-1EQUCGFZH8UZKSZ0Z5N5T234YRNDT4U9H7QNYXWRRNJYDDVXE6FWSCPGNJ7";
    const UNRELATED_AGE_PRIVATE_KEY: &str =
        "AGE-SECRET-KEY-1W92VNVAX0YKJX4WQ6SV7T7X2PZYUC0STF5TKJLQ9ZUWM62HLMN3QYQZJ6F";
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
        let _settings = crate::test::SettingsGuard::lock();
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
        let mut sops_used = false;
        let env = EnvResults::toml(&config, &exec_env, &p, Ok, &mut sops_used)
            .await
            .unwrap();
        assert_eq!(env.get("SECRET").unwrap(), "mysecret");
        assert!(sops_used, "decrypted sops values must not be env-cached");

        restore_env_var("MISE_SOPS_AGE_KEY", prev_age_key);
        restore_env_var("MISE_SOPS_ROPS", prev_rops);
    }

    #[tokio::test]
    async fn decrypts_sops_toml_file_with_exec_env_mise_age_key_file() {
        let _settings = crate::test::SettingsGuard::lock();
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
        let env = EnvResults::toml(&config, &exec_env, &p, Ok, &mut false)
            .await
            .unwrap();
        assert_eq!(env.get("SECRET").unwrap(), "mysecret");

        restore_env_var("MISE_SOPS_AGE_KEY", prev_age_key);
        restore_env_var("MISE_SOPS_AGE_KEY_FILE", prev_age_key_file);
        restore_env_var("MISE_SOPS_ROPS", prev_rops);
    }

    #[tokio::test]
    async fn decrypts_sops_toml_file_with_multiple_age_keys() {
        let _settings = crate::test::SettingsGuard::lock();
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
        file::write(
            &key_file,
            format!(
                "# unrelated identity\r\n{UNRELATED_AGE_PRIVATE_KEY}\r\n\r\n# matching identity\r\n{AGE_PRIVATE_KEY}\r\n"
            ),
        )
        .unwrap();

        let mut exec_env = TeraEnvMap::new();
        exec_env.insert(
            "MISE_SOPS_AGE_KEY_FILE".into(),
            key_file.to_string_lossy().to_string(),
        );
        let env = EnvResults::toml(&config, &exec_env, &p, Ok, &mut false)
            .await
            .unwrap();
        assert_eq!(env.get("SECRET").unwrap(), "mysecret");

        restore_env_var("MISE_SOPS_AGE_KEY", prev_age_key);
        restore_env_var("MISE_SOPS_AGE_KEY_FILE", prev_age_key_file);
        restore_env_var("MISE_SOPS_ROPS", prev_rops);
    }

    #[tokio::test]
    async fn rejects_invalid_non_comment_age_key_lines() {
        let _settings = crate::test::SettingsGuard::lock();
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
        file::write(&key_file, format!("not-an-age-key\n{AGE_PRIVATE_KEY}\n")).unwrap();

        let mut exec_env = TeraEnvMap::new();
        exec_env.insert(
            "MISE_SOPS_AGE_KEY_FILE".into(),
            key_file.to_string_lossy().to_string(),
        );
        let err = EnvResults::toml(&config, &exec_env, &p, Ok, &mut false)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("failed to decrypt sops file"),
            "{err}"
        );

        restore_env_var("MISE_SOPS_AGE_KEY", prev_age_key);
        restore_env_var("MISE_SOPS_AGE_KEY_FILE", prev_age_key_file);
        restore_env_var("MISE_SOPS_ROPS", prev_rops);
    }

    #[tokio::test]
    async fn ambient_sops_age_key_file_precedes_exec_env_sops_age_key() {
        let _settings = crate::test::SettingsGuard::lock();
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
        let env = EnvResults::toml(&config, &exec_env, &p, Ok, &mut false)
            .await
            .unwrap();
        assert_eq!(env.get("SECRET").unwrap(), "mysecret");

        restore_env_var("MISE_SOPS_AGE_KEY", prev_mise_age_key);
        restore_env_var("SOPS_AGE_KEY", prev_sops_age_key);
        restore_env_var("SOPS_AGE_KEY_FILE", prev_sops_age_key_file);
        restore_env_var("MISE_SOPS_ROPS", prev_rops);
    }

    #[tokio::test]
    async fn errors_when_sops_cli_is_configured_for_toml_file() {
        let _settings = crate::test::SettingsGuard::lock();
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
        let err = EnvResults::toml(&config, &exec_env, &p, Ok, &mut false)
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("sops.rops=false is not supported for TOML SOPS files"),
            "{err}"
        );

        restore_env_var("MISE_SOPS_AGE_KEY", prev_age_key);
        restore_env_var("MISE_SOPS_ROPS", prev_rops);
    }

    #[tokio::test]
    async fn dotenv_reads_a_file_whatever_it_is_encoded_in() {
        // Windows PowerShell 5.1's `>` and `Out-File` write UTF-16LE by default, so the shell
        // that ships with the OS produces the second of these. It used to yield nothing at all,
        // with no diagnostic -- the variable was simply absent.
        async fn read(dir: &Path, name: &str, bytes: &[u8]) -> EnvMap {
            let path = dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            EnvResults::dotenv(&path, &TeraEnvMap::new(), false)
                .await
                .unwrap()
        }
        let tmp = tempfile::tempdir().unwrap();

        let utf8 = read(tmp.path(), "utf8.env", b"FROM_ENV=hello\n").await;
        let utf16 = read(
            tmp.path(),
            "utf16.env",
            b"\xff\xfeF\0R\0O\0M\0_\0E\0N\0V\0=\0h\0e\0l\0l\0o\0\n\0",
        )
        .await;

        assert_eq!(utf8.get("FROM_ENV").map(String::as_str), Some("hello"));
        // Stated as equality rather than two separate assertions: the claim is that the encoding
        // makes no difference, not merely that each one happens to work.
        assert_eq!(utf16, utf8);
    }
}
