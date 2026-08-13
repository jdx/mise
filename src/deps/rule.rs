use std::collections::BTreeMap;
use std::path::PathBuf;

use eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};
use tera::Context as TeraContext;

use crate::config::Settings;
use crate::config::env_directive::shell_expand_env;
use crate::file::display_path;
use crate::tera::{contains_template_syntax, get_tera, render_str};

#[derive(Debug, Clone)]
pub(crate) struct DepsTemplateContext {
    pub(crate) config_path: PathBuf,
    pub(crate) provider_id: String,
    pub(crate) context: TeraContext,
}

/// List of built-in provider names that have specialized implementations
pub const BUILTIN_PROVIDERS: &[&str] = &[
    "npm",
    "yarn",
    "pnpm",
    "bun",           // Node.js
    "aube",          // Node.js
    "deno",          // Deno
    "go",            // Go
    "pip",           // Python (requirements.txt)
    "poetry",        // Python (poetry)
    "uv",            // Python (uv)
    "bundler",       // Ruby
    "composer",      // PHP
    "dart",          // Dart
    "flutter",       // Flutter
    "git-submodule", // Git
];

/// Configuration for a deps provider (both built-in and custom)
///
/// Built-in providers have auto-detected sources/outputs and default run commands.
/// Custom providers require explicit sources, outputs, and run.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DepsProviderConfig {
    /// Whether to auto-run this provider before mise x/run (default: false)
    #[serde(default)]
    pub auto: bool,
    /// Command to run when stale (required for custom, optional override for built-in)
    pub run: Option<String>,
    /// Files/patterns to check for changes (required for custom, auto-detected for built-in)
    #[serde(default)]
    pub sources: Vec<String>,
    /// Files/directories that should be newer than sources (required for custom, auto-detected for built-in)
    #[serde(default)]
    pub outputs: Vec<String>,
    /// Environment variables to set
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Working directory
    pub dir: Option<String>,
    /// Optional description
    pub description: Option<String>,
    /// Other deps providers that must complete before this one runs
    #[serde(default)]
    pub depends: Vec<String>,
    /// Timeout for the run command (e.g., "30s", "5m", "1h")
    pub timeout: Option<String>,
    /// Context retained for rendering env values after the toolset environment is known.
    #[serde(skip)]
    pub(crate) template_context: Option<DepsTemplateContext>,
}

impl DepsProviderConfig {
    pub(crate) fn rendered(&self, effective_env: &BTreeMap<String, String>) -> Result<Self> {
        let Some(template) = &self.template_context else {
            return Ok(self.clone());
        };
        let mut rendered = self.clone();
        let mut context = template.context.clone();
        let mut env_vars: BTreeMap<String, String> = context
            .get("env")
            .and_then(|value| Deserialize::deserialize(value.clone()).ok())
            .unwrap_or_default();
        env_vars.extend(effective_env.clone());
        context.insert("env", &env_vars);

        rendered.render_strings(|field, input, shell_expand| {
            let mut output = render_template_field(template, field, input, &context)?;
            if shell_expand && output.contains('$') && Settings::get().env_shell_expand {
                output = expand_shell_vars(&output, &env_vars);
            }
            Ok(output)
        })?;
        rendered.env = self.render_env(effective_env)?;
        Ok(rendered)
    }

    pub(crate) fn validate_template_syntax(&self) -> Result<()> {
        let Some(template) = &self.template_context else {
            return Ok(());
        };
        let validate = |field: &str, input: &str| -> Result<()> {
            if contains_template_syntax(input) {
                let mut tera = get_tera(template.config_path.parent());
                tera.validate_template_syntax("__deps_validation", input)
                    .wrap_err_with(|| template_error_context(template, field))?;
            }
            Ok(())
        };
        if let Some(run) = &self.run {
            validate("run", run)?;
        }
        for (index, source) in self.sources.iter().enumerate() {
            validate(&format!("sources[{index}]"), source)?;
        }
        for (index, output) in self.outputs.iter().enumerate() {
            validate(&format!("outputs[{index}]"), output)?;
        }
        for (key, value) in &self.env {
            validate(&format!("env.{key}"), value)?;
        }
        if let Some(dir) = &self.dir {
            validate("dir", dir)?;
        }
        if let Some(description) = &self.description {
            validate("description", description)?;
        }
        for (index, dependency) in self.depends.iter().enumerate() {
            validate(&format!("depends[{index}]"), dependency)?;
        }
        if let Some(timeout) = &self.timeout {
            validate("timeout", timeout)?;
        }
        Ok(())
    }

    pub fn render_strings(
        &mut self,
        mut render: impl FnMut(&str, &str, bool) -> Result<String>,
    ) -> Result<()> {
        if let Some(run) = &mut self.run {
            *run = render("run", run, false)?;
        }
        for (index, source) in self.sources.iter_mut().enumerate() {
            *source = render(&format!("sources[{index}]"), source, true)?;
        }
        for (index, output) in self.outputs.iter_mut().enumerate() {
            *output = render(&format!("outputs[{index}]"), output, true)?;
        }
        if let Some(dir) = &mut self.dir {
            *dir = render("dir", dir, true)?;
        }
        if let Some(description) = &mut self.description {
            *description = render("description", description, true)?;
        }
        for (index, dependency) in self.depends.iter_mut().enumerate() {
            *dependency = render(&format!("depends[{index}]"), dependency, true)?;
        }
        if let Some(timeout) = &mut self.timeout {
            *timeout = render("timeout", timeout, true)?;
        }
        Ok(())
    }

    pub(crate) fn render_env(
        &self,
        effective_env: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, String>> {
        let Some(template) = &self.template_context else {
            return Ok(self.env.clone());
        };
        let mut context = template.context.clone();
        let mut env_vars: BTreeMap<String, String> = context
            .get("env")
            .and_then(|value| Deserialize::deserialize(value.clone()).ok())
            .unwrap_or_default();
        env_vars.extend(effective_env.clone());
        context.insert("env", &env_vars);

        self.env
            .iter()
            .map(|(key, input)| {
                let field = format!("env.{key}");
                let mut output = render_template_field(template, &field, input, &context)?;
                if output.contains('$') && Settings::get().env_shell_expand {
                    output = expand_shell_vars(&output, &env_vars);
                }
                Ok((key.clone(), output))
            })
            .collect()
    }
}

fn render_template_field(
    template: &DepsTemplateContext,
    field: &str,
    input: &str,
    context: &TeraContext,
) -> Result<String> {
    if !contains_template_syntax(input) {
        return Ok(input.to_string());
    }
    let mut tera = get_tera(template.config_path.parent());
    render_str(&mut tera, input, context).wrap_err_with(|| template_error_context(template, field))
}

fn template_error_context(template: &DepsTemplateContext, field: &str) -> String {
    format!(
        "failed to render deps provider {:?} field {:?} in {}",
        template.provider_id,
        field,
        display_path(&template.config_path)
    )
}

fn expand_shell_vars(input: &str, env_vars: &BTreeMap<String, String>) -> String {
    let mut missing_vars = Vec::new();
    let output = shell_expand_env(input, env_vars, &mut missing_vars);
    for var in missing_vars {
        warn_once!(
            "env var '{var}' is not defined and will be left unexpanded. \
             Use ${{{var}:-}} to default to an empty string and suppress \
             this warning."
        );
    }
    output
}

/// Top-level [deps] configuration section
///
/// All providers are configured at the same level:
/// - `[deps.npm]` - built-in npm provider
/// - `[deps.codegen]` - custom provider
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DepsConfig {
    /// List of provider IDs to disable at runtime
    #[serde(default)]
    pub disable: Vec<String>,
    /// All provider configurations (both built-in and custom)
    #[serde(flatten)]
    pub providers: BTreeMap<String, DepsProviderConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_strings_covers_all_provider_values() {
        let mut context = TeraContext::new();
        context.insert("env", &BTreeMap::<String, String>::new());
        let mut config = DepsProviderConfig {
            run: Some("run".into()),
            sources: vec!["source".into()],
            outputs: vec!["output".into()],
            env: BTreeMap::from([("KEY".into(), "{{ env.EFFECTIVE }}".into())]),
            dir: Some("dir".into()),
            description: Some("description".into()),
            depends: vec!["dependency".into()],
            timeout: Some("timeout".into()),
            template_context: Some(DepsTemplateContext {
                config_path: PathBuf::from("mise.toml"),
                provider_id: "test".into(),
                context,
            }),
            ..Default::default()
        };
        let mut fields = Vec::new();

        config
            .render_strings(|field, value, shell_expand| {
                fields.push((field.to_string(), shell_expand));
                Ok(format!("rendered-{value}"))
            })
            .unwrap();

        assert_eq!(config.run.as_deref(), Some("rendered-run"));
        assert_eq!(config.sources, ["rendered-source"]);
        assert_eq!(config.outputs, ["rendered-output"]);
        assert_eq!(config.env["KEY"], "{{ env.EFFECTIVE }}");
        assert_eq!(
            config
                .render_env(&BTreeMap::from([(
                    "EFFECTIVE".into(),
                    "effective-value".into()
                )]))
                .unwrap()["KEY"],
            "effective-value"
        );
        assert_eq!(config.dir.as_deref(), Some("rendered-dir"));
        assert_eq!(config.description.as_deref(), Some("rendered-description"));
        assert_eq!(config.depends, ["rendered-dependency"]);
        assert_eq!(config.timeout.as_deref(), Some("rendered-timeout"));
        assert_eq!(
            fields,
            [
                ("run".into(), false),
                ("sources[0]".into(), true),
                ("outputs[0]".into(), true),
                ("dir".into(), true),
                ("description".into(), true),
                ("depends[0]".into(), true),
                ("timeout".into(), true),
            ]
        );
    }

    #[test]
    fn test_rendered_uses_effective_env_for_all_provider_values() {
        let mut context = TeraContext::new();
        context.insert("env", &BTreeMap::<String, String>::new());
        let config = DepsProviderConfig {
            run: Some("echo {{ env.EFFECTIVE }}".into()),
            sources: vec!["{{ env.EFFECTIVE }}/source".into()],
            outputs: vec!["{{ env.EFFECTIVE }}/output".into()],
            env: BTreeMap::from([("KEY".into(), "{{ env.EFFECTIVE }}".into())]),
            dir: Some("{{ env.EFFECTIVE }}".into()),
            description: Some("{{ env.EFFECTIVE }} description".into()),
            depends: vec!["{{ env.EFFECTIVE }}".into()],
            timeout: Some("{{ env.TIMEOUT }}s".into()),
            template_context: Some(DepsTemplateContext {
                config_path: PathBuf::from("mise.toml"),
                provider_id: "test".into(),
                context,
            }),
            ..Default::default()
        };

        let rendered = config
            .rendered(&BTreeMap::from([
                ("EFFECTIVE".into(), "effective".into()),
                ("TIMEOUT".into(), "5".into()),
            ]))
            .unwrap();

        assert_eq!(rendered.run.as_deref(), Some("echo effective"));
        assert_eq!(rendered.sources, ["effective/source"]);
        assert_eq!(rendered.outputs, ["effective/output"]);
        assert_eq!(rendered.env["KEY"], "effective");
        assert_eq!(rendered.dir.as_deref(), Some("effective"));
        assert_eq!(
            rendered.description.as_deref(),
            Some("effective description")
        );
        assert_eq!(rendered.depends, ["effective"]);
        assert_eq!(rendered.timeout.as_deref(), Some("5s"));
    }
}
