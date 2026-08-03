use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

use demand::Input;
use eyre::{Result, bail};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tera::{Kwargs, State, TeraResult, Value};

use crate::config::Config;
use crate::env_diff::EnvMap;
use crate::tera::{BASE_CONTEXT, get_tera_v2, render_str_v2};

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum SecretTomlConfig {
    Env(String),
    Options(SecretOptionsTomlConfig),
}

#[derive(Clone, Debug, Deserialize)]
pub struct SecretOptionsTomlConfig {
    pub env: String,
    pub description: Option<String>,
    #[serde(default)]
    pub allow_empty: bool,
}

#[derive(Clone, Debug)]
pub struct SecretDeclaration {
    pub name: String,
    pub env: String,
    pub description: Option<String>,
    pub allow_empty: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretState {
    Available,
    Missing,
    Empty,
    InvalidUnicode,
}

impl std::fmt::Display for SecretState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Available => write!(f, "available"),
            Self::Missing => write!(f, "missing"),
            Self::Empty => write!(f, "empty"),
            Self::InvalidUnicode => write!(f, "invalid_unicode"),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SecretStatus {
    pub name: String,
    pub env: String,
    pub state: SecretState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Default)]
struct SecretResolution {
    declarations: IndexMap<String, SecretDeclaration>,
    values: IndexMap<String, String>,
    redaction_env: EnvMap,
    unavailable: IndexMap<String, String>,
}

#[derive(Clone, Debug, Default)]
pub struct SecretValues {
    resolution: Arc<Mutex<SecretResolution>>,
    prompt: bool,
}

#[derive(Debug, thiserror::Error)]
#[error(
    "required bootstrap secrets are unavailable: {details}. Supply them in the environment (for example, `fnox exec -- mise bootstrap ...`) or pass --prompt-secrets"
)]
struct SecretUnavailable {
    details: String,
}

pub fn declarations_from_config(config: &Config) -> Result<Vec<SecretDeclaration>> {
    let mut merged = IndexMap::new();
    for cf in config.config_files.values() {
        if let Some(bootstrap) = cf.bootstrap_config() {
            for (name, declaration) in bootstrap.secrets {
                merged.entry(name).or_insert(declaration);
            }
        }
    }
    merged
        .into_iter()
        .map(|(name, declaration)| declaration_from_toml(name, declaration))
        .collect()
}

pub fn statuses(config: &Config) -> Result<Vec<SecretStatus>> {
    Ok(declarations_from_config(config)?
        .into_iter()
        .map(|declaration| SecretStatus {
            state: env_state(&declaration),
            name: declaration.name,
            env: declaration.env,
            description: declaration.description,
        })
        .collect())
}

pub fn resolve(config: &Config, prompt: bool) -> Result<SecretValues> {
    let declarations = declarations_from_config(config)?
        .into_iter()
        .map(|declaration| (declaration.name.clone(), declaration))
        .collect();
    Ok(SecretValues {
        resolution: Arc::new(Mutex::new(SecretResolution {
            declarations,
            ..Default::default()
        })),
        prompt,
    })
}

impl SecretValues {
    pub fn used_statuses(&self) -> Result<Vec<SecretStatus>> {
        let resolution = self
            .resolution
            .lock()
            .map_err(|_| eyre::eyre!("bootstrap secret resolver is unavailable"))?;
        Ok(resolution
            .declarations
            .values()
            .filter(|declaration| {
                resolution.values.contains_key(&declaration.name)
                    || resolution.unavailable.contains_key(&declaration.name)
            })
            .map(|declaration| SecretStatus {
                state: if resolution.values.contains_key(&declaration.name) {
                    SecretState::Available
                } else {
                    env_state(declaration)
                },
                name: declaration.name.clone(),
                env: declaration.env.clone(),
                description: declaration.description.clone(),
            })
            .collect())
    }

    pub fn render(
        &self,
        config: &Config,
        input: &str,
        base: &Path,
        target: &Path,
    ) -> Result<String> {
        self.render_inner(Some(config), input, base, target)
    }

    fn render_inner(
        &self,
        config: Option<&Config>,
        input: &str,
        base: &Path,
        target: &Path,
    ) -> Result<String> {
        let resolution = self.resolution.clone();
        let used = Arc::new(Mutex::new(BTreeSet::new()));
        let used_by_function = used.clone();
        let prompt = self.prompt;
        let mut tera = get_tera_v2(Some(base));
        tera.register_function(
            "secret",
            move |args: Kwargs, _: &State| -> TeraResult<Value> {
                let name = args.must_get::<&str>("name")?;
                used_by_function
                    .lock()
                    .map_err(|_| tera::Error::message("bootstrap secret resolver is unavailable"))?
                    .insert(name.to_string());
                let mut resolution = resolution.lock().map_err(|_| {
                    tera::Error::message("bootstrap secret resolver is unavailable")
                })?;
                if let Some(value) = resolution.values.get(name) {
                    return Ok(Value::from(value.as_str()));
                }
                if resolution.unavailable.contains_key(name) {
                    return Ok(Value::from(""));
                }
                let declaration = resolution.declarations.get(name).cloned().ok_or_else(|| {
                    tera::Error::message(format!(
                        "bootstrap secret '{name}' is not declared in [bootstrap.secrets]"
                    ))
                })?;
                match resolve_declaration(&declaration, prompt) {
                    Ok(value) => {
                        resolution
                            .redaction_env
                            .insert(declaration.env, value.clone());
                        resolution.values.insert(name.to_string(), value.clone());
                        Ok(Value::from(value))
                    }
                    Err(detail) => {
                        resolution.unavailable.insert(name.to_string(), detail);
                        Ok(Value::from(""))
                    }
                }
            },
        );
        let mut context = BASE_CONTEXT.clone();
        context.insert("config_root", base);
        context.insert("target", target);
        let rendered = render_str_v2(&mut tera, input, &context);
        let resolution = self
            .resolution
            .lock()
            .map_err(|_| eyre::eyre!("bootstrap secret resolver is unavailable"))?;
        if let Some(config) = config {
            config.add_redactions_excluding(
                resolution.redaction_env.keys().cloned(),
                &resolution.redaction_env,
                &BTreeSet::new(),
            );
        }
        let used = used
            .lock()
            .map_err(|_| eyre::eyre!("bootstrap secret resolver is unavailable"))?;
        let unavailable = used
            .iter()
            .filter_map(|name| resolution.unavailable.get(name))
            .cloned()
            .collect::<Vec<_>>();
        if !unavailable.is_empty() {
            return Err(SecretUnavailable {
                details: unavailable.join(", "),
            }
            .into());
        }
        rendered.map_err(Into::into)
    }

    #[cfg(test)]
    fn from_values(values: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            resolution: Arc::new(Mutex::new(SecretResolution {
                values: values.into_iter().collect(),
                ..Default::default()
            })),
            prompt: false,
        }
    }
}

pub fn is_unavailable(error: &eyre::Report) -> bool {
    error
        .chain()
        .any(|cause| cause.downcast_ref::<SecretUnavailable>().is_some())
}

fn resolve_declaration(declaration: &SecretDeclaration, prompt: bool) -> Result<String, String> {
    let value = match std::env::var_os(&declaration.env) {
        Some(value) => match value.into_string() {
            Ok(value) if declaration.allow_empty || !value.is_empty() => Some(value),
            Ok(_) => None,
            Err(_) => {
                return Err(format!(
                    "{} ({}) contains non-Unicode data",
                    declaration.name, declaration.env
                ));
            }
        },
        None => None,
    };
    let value = match value {
        Some(value) => value,
        None if prompt => prompt_value(declaration).map_err(|error| error.to_string())?,
        None => return Err(format!("{} ({})", declaration.name, declaration.env)),
    };
    if value.is_empty() && !declaration.allow_empty {
        return Err(format!(
            "{} ({}) must not be empty",
            declaration.name, declaration.env
        ));
    }
    Ok(value)
}

fn declaration_from_toml(name: String, declaration: SecretTomlConfig) -> Result<SecretDeclaration> {
    if name.is_empty()
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_.-".contains(character))
    {
        bail!("invalid bootstrap secret name '{name}': use ASCII letters, digits, '.', '_' or '-'");
    }
    let (env, description, allow_empty) = match declaration {
        SecretTomlConfig::Env(env) => (env, None, false),
        SecretTomlConfig::Options(options) => {
            (options.env, options.description, options.allow_empty)
        }
    };
    if !valid_env_name(&env) {
        bail!("bootstrap secret '{name}' has invalid environment variable name '{env}'");
    }
    Ok(SecretDeclaration {
        name,
        env,
        description,
        allow_empty,
    })
}

fn valid_env_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn env_state(declaration: &SecretDeclaration) -> SecretState {
    match std::env::var_os(&declaration.env) {
        None => SecretState::Missing,
        Some(value) => match value.into_string() {
            Err(_) => SecretState::InvalidUnicode,
            Ok(value) if value.is_empty() && !declaration.allow_empty => SecretState::Empty,
            Ok(_) => SecretState::Available,
        },
    }
}

fn prompt_value(declaration: &SecretDeclaration) -> Result<String> {
    if !console::user_attended_stderr() {
        bail!(
            "cannot prompt for bootstrap secret '{}' without an interactive terminal",
            declaration.name
        );
    }
    let prompt = declaration
        .description
        .clone()
        .unwrap_or_else(|| format!("Enter bootstrap secret {}", declaration.name));
    Ok(Input::new(&prompt)
        .password(true)
        .theme(&crate::ui::theme::get_theme())
        .run()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_names() {
        assert!(valid_env_name("CACHE_TOKEN"));
        assert!(!valid_env_name("CACHE-TOKEN"));
        assert!(
            declaration_from_toml(
                "cache.token".to_string(),
                SecretTomlConfig::Env("CACHE_TOKEN".to_string()),
            )
            .is_ok()
        );
        assert!(
            declaration_from_toml(
                "cache token".to_string(),
                SecretTomlConfig::Env("CACHE_TOKEN".to_string()),
            )
            .is_err()
        );
    }

    #[test]
    fn renders_only_declared_secret_values() {
        let values = SecretValues::from_values([("token".to_string(), "sensitive".to_string())]);
        assert_eq!(
            values
                .render_inner(
                    None,
                    "token={{ secret(name=\"token\") }}",
                    Path::new("/tmp"),
                    Path::new("/etc/example"),
                )
                .unwrap(),
            "token=sensitive"
        );
        assert!(
            values
                .render_inner(
                    None,
                    "{{ secret(name=\"missing\") }}",
                    Path::new("/tmp"),
                    Path::new("/etc/example"),
                )
                .is_err()
        );
    }

    #[test]
    fn unavailable_secret_is_scoped_to_templates_that_use_it() {
        let name = "token".to_string();
        let values = SecretValues {
            resolution: Arc::new(Mutex::new(SecretResolution {
                declarations: [(
                    name.clone(),
                    SecretDeclaration {
                        name: name.clone(),
                        env: "MISE_TEST_UNAVAILABLE_SECRET".to_string(),
                        description: None,
                        allow_empty: false,
                    },
                )]
                .into_iter()
                .collect(),
                unavailable: [(name, "token is unavailable".to_string())]
                    .into_iter()
                    .collect(),
                ..Default::default()
            })),
            prompt: false,
        };
        assert_eq!(
            values
                .render_inner(None, "literal", Path::new("/tmp"), Path::new("/etc/static"))
                .unwrap(),
            "literal"
        );
        assert!(
            values
                .render_inner(
                    None,
                    "{{ secret(name=\"token\") }}",
                    Path::new("/tmp"),
                    Path::new("/etc/secret"),
                )
                .is_err()
        );
    }
}
