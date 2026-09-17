use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex};

use demand::Input;
use eyre::{Result, bail};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tera::{Kwargs, State, TeraResult, Value};

use crate::config::{Config, ConfigMap};
use crate::env_diff::EnvMap;
use crate::system::resources::ResourceOrigin;
use crate::tera::{
    BASE_CONTEXT, TeraEngine, get_tera, get_tera_for_oci, get_tera_v2, render_str, render_str_v2,
};

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum SecretTomlConfig {
    Env(String),
    Options(SecretOptionsTomlConfig),
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SecretOptionsTomlConfig {
    pub env: String,
    pub description: Option<String>,
    #[serde(default)]
    pub allow_empty: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SecretDeclaration {
    pub name: String,
    pub env: String,
    pub description: Option<String>,
    pub allow_empty: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SecretState {
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
pub(crate) struct SecretStatus {
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
pub(crate) struct SecretValues {
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

pub(crate) fn declarations_from_config(config: &Config) -> Result<Vec<SecretDeclaration>> {
    let mut merged: IndexMap<String, (SecretDeclaration, ResourceOrigin)> = IndexMap::new();
    for config_files in config.bootstrap_config_maps() {
        for (name, declaration) in secrets_from_config_files(config_files)? {
            if let Some(existing) = merged.get(&name) {
                if existing.0 == declaration.0 {
                    continue;
                }
                bail!(
                    "conflicting bootstrap secret declarations for {name}\n\n  first:\n    {}\n\n  second:\n    {}",
                    existing.1.conflict_description(),
                    declaration.1.conflict_description(),
                );
            }
            merged.insert(name, declaration);
        }
    }
    Ok(merged
        .into_values()
        .map(|(declaration, _)| declaration)
        .collect())
}

fn secrets_from_config_files(
    config_files: &ConfigMap,
) -> Result<IndexMap<String, (SecretDeclaration, ResourceOrigin)>> {
    let mut merged = IndexMap::new();
    for (path, cf) in config_files {
        if let Some(bootstrap) = cf.bootstrap_config() {
            let origin = ResourceOrigin {
                config: path.clone(),
                config_root: cf.config_root(),
                environment: crate::config::environments_for_config_path(path),
                source: None,
            };
            for (name, declaration) in bootstrap.secrets {
                let declaration = declaration_from_toml(name.clone(), declaration)?;
                merged
                    .entry(name)
                    .or_insert_with(|| (declaration, origin.clone()));
            }
        }
    }
    Ok(merged)
}

pub(crate) fn statuses(config: &Config) -> Result<Vec<SecretStatus>> {
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

pub(crate) fn resolve(config: &Config, prompt: bool) -> Result<SecretValues> {
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
    pub(crate) fn used_statuses(&self) -> Result<Vec<SecretStatus>> {
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

    pub(crate) fn render(
        &self,
        config: &Config,
        input: &str,
        base: &Path,
        target: &Path,
        config_path: &Path,
    ) -> Result<String> {
        self.render_inner(Some((config, config_path)), input, base, target)
    }

    pub(crate) fn render_dotfile(
        &self,
        config: &Config,
        input: &str,
        base: &Path,
        config_path: &Path,
    ) -> Result<String> {
        let mut tera = get_tera(Some(base));
        let used = match &mut tera {
            TeraEngine::V2(tera) => self.register_v2(tera),
            TeraEngine::V1(tera) => self.register_v1(tera),
        };
        let rendered = render_str(&mut tera, input, config.bootstrap_tera_ctx(config_path));
        self.finish_render(Some(config), used, rendered)
    }

    pub(crate) fn render_dotfile_for_oci(
        config: &Config,
        input: &str,
        base: &Path,
        config_path: &Path,
    ) -> Result<String> {
        const MESSAGE: &str = "bootstrap secrets cannot be embedded in persistent OCI image layers";
        let mut tera = get_tera_for_oci(Some(base));
        match &mut tera {
            TeraEngine::V2(tera) => tera
                .register_function("secret", |_: Kwargs, _: &State| -> TeraResult<Value> {
                    Err(tera::Error::message(MESSAGE))
                }),
            TeraEngine::V1(tera) => tera.register_function(
                "secret",
                |_: &HashMap<String, JsonValue>| -> tera1::Result<JsonValue> {
                    Err(tera1::Error::msg(MESSAGE))
                },
            ),
        }
        let mut context = config.bootstrap_tera_ctx(config_path).clone();
        context.remove("env");
        render_str(&mut tera, input, &context).map_err(Into::into)
    }

    fn render_inner(
        &self,
        config: Option<(&Config, &Path)>,
        input: &str,
        base: &Path,
        target: &Path,
    ) -> Result<String> {
        let mut tera = get_tera_v2(Some(base));
        let used = self.register_v2(&mut tera);
        let mut context = BASE_CONTEXT.clone();
        // Independently selected bootstrap roots keep their legacy template
        // context until scoped composition and execution semantics are defined.
        if let Some((config, config_path)) = config
            && !config
                .selected_bootstrap_config_maps()
                .any(|(_, files)| files.contains_key(config_path))
        {
            context.insert("vars", &config.vars);
        }
        context.insert("config_root", base);
        context.insert("target", target);
        let rendered = render_str_v2(&mut tera, input, &context);
        self.finish_render(config.map(|(config, _)| config), used, rendered)
    }

    fn register_v2(&self, tera: &mut tera::Tera) -> Arc<Mutex<BTreeSet<String>>> {
        let resolution = self.resolution.clone();
        let used = Arc::new(Mutex::new(BTreeSet::new()));
        let used_by_function = used.clone();
        let prompt = self.prompt;
        tera.register_function(
            "secret",
            move |args: Kwargs, _: &State| -> TeraResult<Value> {
                let name = args.must_get::<&str>("name")?;
                resolve_secret(&resolution, &used_by_function, prompt, name)
                    .map(Value::from)
                    .map_err(tera::Error::message)
            },
        );
        used
    }

    fn register_v1(&self, tera: &mut tera1::Tera) -> Arc<Mutex<BTreeSet<String>>> {
        let resolution = self.resolution.clone();
        let used = Arc::new(Mutex::new(BTreeSet::new()));
        let used_by_function = used.clone();
        let prompt = self.prompt;
        tera.register_function(
            "secret",
            move |args: &HashMap<String, JsonValue>| -> tera1::Result<JsonValue> {
                let name = args
                    .get("name")
                    .and_then(JsonValue::as_str)
                    .ok_or_else(|| tera1::Error::msg("missing required argument: name"))?;
                resolve_secret(&resolution, &used_by_function, prompt, name)
                    .map(JsonValue::from)
                    .map_err(tera1::Error::msg)
            },
        );
        used
    }

    fn finish_render(
        &self,
        config: Option<&Config>,
        used: Arc<Mutex<BTreeSet<String>>>,
        rendered: TeraResult<String>,
    ) -> Result<String> {
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

fn resolve_secret(
    resolution: &Arc<Mutex<SecretResolution>>,
    used: &Arc<Mutex<BTreeSet<String>>>,
    prompt: bool,
    name: &str,
) -> std::result::Result<String, String> {
    used.lock()
        .map_err(|_| "bootstrap secret resolver is unavailable".to_string())?
        .insert(name.to_string());
    let mut resolution = resolution
        .lock()
        .map_err(|_| "bootstrap secret resolver is unavailable".to_string())?;
    if let Some(value) = resolution.values.get(name) {
        return Ok(value.clone());
    }
    if resolution.unavailable.contains_key(name) {
        return Ok(String::new());
    }
    let declaration = resolution.declarations.get(name).cloned().ok_or_else(|| {
        format!("bootstrap secret '{name}' is not declared in [bootstrap.secrets]")
    })?;
    match resolve_declaration(&declaration, prompt) {
        Ok(value) => {
            resolution
                .redaction_env
                .insert(declaration.env, value.clone());
            resolution.values.insert(name.to_string(), value.clone());
            Ok(value)
        }
        Err(detail) => {
            resolution.unavailable.insert(name.to_string(), detail);
            Ok(String::new())
        }
    }
}

pub(crate) fn is_unavailable(error: &eyre::Report) -> bool {
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
