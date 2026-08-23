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
        let s = ctx.parse_template(&input)?;
        let expand = expand && crate::config::Settings::get().env_shell_expand;
        // Accumulate loaded vars so opted-in expansion can reference values from
        // an earlier file in the same directive or an earlier env block.
        let mut acc: TeraEnvMap = ctx.exec_env.clone();
        for p in xx::file::glob(ctx.normalize_path(s.into())).unwrap_or_default() {
            let config = ctx.config;
            let exec_env = ctx.exec_env;
            let parse_template = |s: String| ctx.parse_template(&s);
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            let mut loaded = match ext.as_str() {
                "json" => Self::json(config, exec_env, &p, parse_template).await?,
                "yaml" => Self::yaml(config, exec_env, &p, parse_template).await?,
                "toml" => Self::toml(config, exec_env, &p, parse_template).await?,
                _ => Self::dotenv(config, exec_env, &p, parse_template, &acc, expand).await?,
            };
            // Structured files are literal by default. With `expand = true`, run
            // their values through the same `$VAR` engine used by `[env]` values
            // and accumulate key-by-key for same-file references.
            if expand && matches!(ext.as_str(), "json" | "yaml" | "toml") {
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

    async fn dotenv<PT>(
        config: &Arc<Config>,
        exec_env: &TeraEnvMap,
        p: &Path,
        parse_template: PT,
        acc: &TeraEnvMap,
        expand: bool,
    ) -> Result<EnvMap>
    where
        PT: FnMut(String) -> Result<String>,
    {
        let errfn = || eyre!("failed to parse dotenv file: {}", display_path(p));
        // Reading ahead only to look for the SOPS marker, so a plain dotenv file
        // still reaches dotenvy exactly the way it did before.
        if let Ok(raw) = file::read_to_string(p)
            && is_sops_dotenv(&raw)
        {
            let decrypted = sops::decrypt_dotenv(config, exec_env, &raw, parse_template).await?;
            if decrypted.is_empty() {
                // Non-strict mode skipped it, same as the structured formats.
                return Ok(EnvMap::new());
            }
            return Self::parse_dotenv(&decrypted, p, acc, expand);
        }
        if !expand {
            // Preserve dotenvy's normal behavior unless cross-file expansion was
            // explicitly requested.
            let mut env = EnvMap::new();
            if let Ok(dotenv) = dotenvy::from_path_iter(p) {
                for item in dotenv {
                    let (k, v) = item.wrap_err_with(errfn)?;
                    env.insert(k, v);
                }
            }
            return Ok(env);
        }
        let Ok(content) = file::read_to_string(p) else {
            return Ok(EnvMap::new());
        };
        Self::parse_dotenv(&content, p, acc, expand)
    }

    /// Parse dotenv text that is already in hand, decrypted or not.
    fn parse_dotenv(content: &str, p: &Path, acc: &TeraEnvMap, expand: bool) -> Result<EnvMap> {
        let errfn = || eyre!("failed to parse dotenv file: {}", display_path(p));
        if !expand {
            let mut env = EnvMap::new();
            for item in dotenvy::from_read_iter(content.as_bytes()) {
                let (k, v) = item.wrap_err_with(errfn)?;
                env.insert(k, v);
            }
            return Ok(env);
        }
        // dotenvy substitutes `${VAR}` only against the process env + vars defined
        // earlier in the same file and has no API for a custom map. Seed the parse
        // with accumulated values, then retain only keys defined by this file.
        let mut own_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
        for item in dotenvy::from_read_iter(content.as_bytes()) {
            let (k, _v) = item.wrap_err_with(errfn)?;
            own_keys.insert(k);
        }
        if own_keys.is_empty() {
            return Ok(EnvMap::new());
        }
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

/// Does this dotenv text carry SOPS metadata?
///
/// SOPS's dotenv store flattens its metadata into `sops_`-prefixed keys. Match
/// two of them at the start of a line, and require the MAC to carry a SOPS
/// ciphertext rather than any value at all.
///
/// The bar is set there because a false positive is expensive: a plain file read
/// as encrypted fails the whole config in strict mode and silently drops every
/// variable in it otherwise. Nothing stops a dotenv file from defining its own
/// `sops_version`, or even both key names, but `sops_mac=ENC[` additionally
/// requires it to hold something shaped like SOPS output.
///
/// `sops_version` and `sops_mac` are the pair because SOPS writes both in every
/// mode measured — default, `--unencrypted-suffix`, `--encrypted-regex`,
/// `mac_only_encrypted`, and a single-key file — always with the MAC encrypted.
/// `sops_unencrypted_suffix` looks like a candidate until `--encrypted-regex`
/// drops it.
///
/// Sniffing the content rather than the file name is deliberate: `_.file` sends
/// every extension it does not recognise down the dotenv path, including files
/// with no extension at all, so there is no name to key off.
fn is_sops_dotenv(raw: &str) -> bool {
    let mut version = false;
    let mut mac = false;
    for line in raw.lines() {
        version |= line.starts_with("sops_version=");
        mac |= line.starts_with("sops_mac=ENC[");
        if version && mac {
            return true;
        }
    }
    false
}

fn is_env_key(k: &str) -> bool {
    let mut chars = k.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

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

    #[test]
    fn detects_sops_metadata_in_dotenv() {
        // Trimmed from a real `sops encrypt --input-type dotenv` result.
        let raw = concat!(
            "SECRET=ENC[AES256_GCM,data:PEpqNWS5S1k=,iv:xEPy,tag:ZE+F,type:str]\n",
            "sops_age__list_0__map_enc=-----BEGIN AGE ENCRYPTED FILE-----\\n...\n",
            "sops_lastmodified=2026-08-23T11:10:00Z\n",
            "sops_mac=ENC[AES256_GCM,data:vzLw,iv:RJNu,tag:4+mn,type:str]\n",
            "sops_unencrypted_suffix=_unencrypted\n",
            "sops_version=3.13.3\n",
        );
        assert!(is_sops_dotenv(raw));
    }

    #[test]
    fn plain_dotenv_is_not_sops() {
        assert!(!is_sops_dotenv("SECRET=mysecret\nOTHER=\"two words\"\n"));
        assert!(!is_sops_dotenv(""));
    }

    #[test]
    fn a_value_mentioning_the_marker_is_not_sops() {
        // The marker only counts at the start of a line, so a file that merely
        // talks about sops keeps its plain-dotenv path.
        assert!(!is_sops_dotenv("NOTE=\"sops_version=3.13.3\"\n"));
        assert!(!is_sops_dotenv("MY_sops_version=3.13.3\n"));
    }

    #[test]
    fn a_plain_file_defining_sops_version_is_not_sops() {
        // Nothing stops a dotenv file from having its own `sops_version`
        // variable. One marker alone would send it to the decrypter, which
        // fails the config in strict mode and drops every variable otherwise.
        assert!(!is_sops_dotenv("sops_version=3.13.3\nSECRET=mysecret\n"));
        assert!(!is_sops_dotenv(
            "sops_mac=ENC[AES256_GCM,data:x]\nSECRET=s\n"
        ));
    }

    #[test]
    fn both_key_names_without_a_sops_mac_value_is_not_sops() {
        // Even a file that defines both names is plain text unless the MAC
        // holds something shaped like SOPS output.
        assert!(!is_sops_dotenv(
            "sops_version=3.13.3\nsops_mac=whatever\nSECRET=mysecret\n"
        ));
        assert!(is_sops_dotenv(
            "sops_version=3.13.3\nsops_mac=ENC[AES256_GCM,data:x,type:str]\n"
        ));
    }

    #[test]
    fn parse_dotenv_reads_decrypted_text() {
        // What the sops CLI hands back: no metadata left to strip.
        let env = EnvResults::parse_dotenv(
            "SECRET=mysecret\nOTHER=\"two words\"\n",
            Path::new(".env"),
            &TeraEnvMap::new(),
            false,
        )
        .unwrap();
        assert_eq!(env.get("SECRET").map(String::as_str), Some("mysecret"));
        assert_eq!(env.get("OTHER").map(String::as_str), Some("two words"));
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
    async fn decrypts_sops_toml_file_with_multiple_age_keys() {
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
        let env = EnvResults::toml(&config, &exec_env, &p, Ok).await.unwrap();
        assert_eq!(env.get("SECRET").unwrap(), "mysecret");

        restore_env_var("MISE_SOPS_AGE_KEY", prev_age_key);
        restore_env_var("MISE_SOPS_AGE_KEY_FILE", prev_age_key_file);
        restore_env_var("MISE_SOPS_ROPS", prev_rops);
        Settings::reset(None);
    }

    #[tokio::test]
    async fn rejects_invalid_non_comment_age_key_lines() {
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
        let err = EnvResults::toml(&config, &exec_env, &p, Ok)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("failed to decrypt sops file"),
            "{err}"
        );

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
    fn escapes_seeded_dotenv_values() {
        assert_eq!(escape_dotenv_double_quoted(r#"a$b"c\d"#), r#"a\$b\"c\\d"#);
        assert_eq!(escape_dotenv_double_quoted("l1\nl2"), "l1\\nl2");
    }

    #[test]
    fn validates_seeded_dotenv_keys() {
        assert!(is_env_key("PGHOST"));
        assert!(is_env_key("_FOO123"));
        assert!(!is_env_key("1FOO"));
        assert!(!is_env_key("FOO-BAR"));
        assert!(!is_env_key(""));
    }
}
