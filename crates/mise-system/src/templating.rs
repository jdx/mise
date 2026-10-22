//! Template rendering for `[bootstrap]` resource declarations.
//!
//! Most of `[bootstrap]` is deserialized straight into typed structs, which
//! means a value like `working_directory = "{{ config_root }}/svc"` would reach
//! the generated unit file verbatim. [`Templated`] keeps the raw TOML next to
//! the parsed value so a section can render its string values against the
//! declaring config file's bootstrap template context before use.
//!
//! This is for declarative *values* — paths, commands, descriptions. Managed
//! file content keeps its explicit `template = true` opt-in so literal
//! `{{ ... }}` payloads stay untouched.

use std::path::Path;

use eyre::{Result, eyre};
use serde::de::DeserializeOwned;
use tera::Context;

use crate::host::Config;
use mise_util::tera::{TeraEngine, contains_template_syntax, get_tera_without_exec, render_str};

pub(crate) use mise_config::templating::Templated;

/// Rendering for [`Templated`] values; it needs the config's template context, so it lives in
/// mise rather than next to the type.
pub(crate) trait TemplatedExt<T: DeserializeOwned>: Sized {
    fn render(self, config: &Config, config_path: &Path) -> Result<T>;
    fn render_with(self, ctx: &Context, config_path: &Path) -> Result<T>;
}

impl<T: DeserializeOwned> TemplatedExt<T> for Templated<T> {
    /// Render every string value against the bootstrap template context of the
    /// config file that declared this entry, then reparse it.
    ///
    /// Entries without template syntax skip the renderer entirely and return
    /// the value parsed at load time.
    fn render(self, config: &Config, config_path: &Path) -> Result<T> {
        if !value_contains_template_syntax(&self.raw) {
            return Ok(self.parsed);
        }
        let mut ctx = config.bootstrap_tera_ctx(config_path).clone();
        if ctx.get("config_root").is_none() {
            let config_root = crate::host::config_root(config_path);
            ctx.insert("config_root", &config_root);
        }
        self.render_with(&ctx, config_path)
    }

    /// [`Templated::render`] against an explicit context.
    fn render_with(self, ctx: &Context, config_path: &Path) -> Result<T> {
        if !value_contains_template_syntax(&self.raw) {
            return Ok(self.parsed);
        }
        let mut tera = get_tera_without_exec(config_path.parent());
        let mut raw = self.raw;
        remove_ignored_fields(&mut raw, &self.ignored);
        // Flatten the cause into the message: these surface through `warn!`,
        // which prints only the top-level error.
        render_value(&mut raw, &mut tera, ctx).map_err(|err| {
            eyre!(
                "failed to render template in {}: {err}",
                config_path.display()
            )
        })?;
        T::deserialize(raw).map_err(|err| {
            eyre!(
                "rendered template is no longer valid in {}: {err}",
                config_path.display()
            )
        })
    }
}

fn remove_ignored_fields(raw: &mut toml::Value, ignored: &[String]) {
    let Some(table) = raw.as_table_mut() else {
        return;
    };
    for field in ignored {
        if table.contains_key(field) {
            table.remove(field);
        }
    }
}

fn value_contains_template_syntax(value: &toml::Value) -> bool {
    match value {
        toml::Value::String(s) => contains_template_syntax(s),
        toml::Value::Array(values) => values.iter().any(value_contains_template_syntax),
        toml::Value::Table(table) => table.values().any(value_contains_template_syntax),
        _ => false,
    }
}

fn render_value(value: &mut toml::Value, tera: &mut TeraEngine, ctx: &Context) -> Result<()> {
    match value {
        toml::Value::String(s) if contains_template_syntax(s) => {
            *s = render_str(tera, s, ctx)?;
        }
        toml::Value::Array(values) => {
            for value in values {
                render_value(value, tera, ctx)?;
            }
        }
        toml::Value::Table(table) => {
            for (_, value) in table.iter_mut() {
                render_value(value, tera, ctx)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemd::SystemdTomlConfig;

    fn templated(toml: &str) -> Templated<SystemdTomlConfig> {
        toml::from_str(toml).unwrap()
    }

    fn ctx() -> Context {
        let mut ctx = Context::new();
        ctx.insert("config_root", "/home/u/proj");
        ctx
    }

    #[test]
    fn test_renders_strings_in_scalars_arrays_and_tables() {
        let unit = templated(
            r#"
            exec_start = "{{ config_root }}/bin/serve"
            working_directory = "{{ config_root }}"
            environment_file = ["{{ config_root }}/.env", "-%h/extra.env"]
            environment = { ROOT = "{{ config_root }}" }
            "#,
        );
        let rendered = unit
            .render_with(&ctx(), Path::new("/home/u/proj/mise.toml"))
            .unwrap();
        assert_eq!(
            rendered.exec_start.as_deref(),
            Some("/home/u/proj/bin/serve")
        );
        assert_eq!(rendered.working_directory.as_deref(), Some("/home/u/proj"));
        assert_eq!(
            rendered.environment_file,
            vec!["/home/u/proj/.env".to_string(), "-%h/extra.env".to_string()]
        );
        assert_eq!(rendered.environment["ROOT"], "/home/u/proj");
    }

    #[test]
    fn test_leaves_non_template_values_alone() {
        let unit = templated(
            r#"
            exec_start = "/bin/true"
            environment_file = ["-%h/.config/svc.env"]
            nice = 10
            "#,
        );
        assert!(!value_contains_template_syntax(&unit.raw));
        let rendered = unit
            .render_with(&ctx(), Path::new("/home/u/proj/mise.toml"))
            .unwrap();
        assert_eq!(rendered.exec_start.as_deref(), Some("/bin/true"));
        assert_eq!(rendered.environment_file, vec!["-%h/.config/svc.env"]);
        assert_eq!(rendered.nice, Some(10));
    }

    #[test]
    fn test_unparsed_entry_still_derefs_to_the_declaration() {
        let unit = templated(r#"exec_start = "{{ config_root }}/bin/serve""#);
        assert_eq!(
            unit.exec_start.as_deref(),
            Some("{{ config_root }}/bin/serve")
        );
    }

    #[test]
    fn test_unknown_fields_are_recorded_for_the_caller_to_report() {
        let unit = templated(
            r#"
            exec_start = "/bin/true"
            exec_startt = "typo"
            "#,
        );
        assert_eq!(unit.ignored_fields(), ["exec_startt"]);
    }

    #[test]
    fn test_ignored_fields_do_not_participate_in_rendering() {
        let unit = templated(
            r#"
            exec_start = "{{ config_root }}/bin/serve"
            exec_startt = "{{ unterminated"
            "#,
        );
        let rendered = unit
            .render_with(&ctx(), Path::new("/home/u/proj/mise.toml"))
            .unwrap();
        assert_eq!(
            rendered.exec_start.as_deref(),
            Some("/home/u/proj/bin/serve")
        );
    }

    #[test]
    fn test_ignored_dotted_root_fields_do_not_participate_in_rendering() {
        let unit = templated(
            r#"
            exec_start = "{{ config_root }}/bin/serve"
            "exec.startt" = "{{ unterminated"
            "#,
        );
        let rendered = unit
            .render_with(&ctx(), Path::new("/home/u/proj/mise.toml"))
            .unwrap();
        assert_eq!(
            rendered.exec_start.as_deref(),
            Some("/home/u/proj/bin/serve")
        );
    }

    #[test]
    fn test_exec_is_not_available_in_resource_values() {
        let unit = templated(r#"exec_start = "{{ exec(command='echo hi') }}""#);
        let err = unit
            .render_with(&ctx(), Path::new("/home/u/proj/mise.toml"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("exec() is not available"), "{err}");
    }

    #[test]
    fn test_render_failure_is_reported_with_the_config_path() {
        let unit = templated(r#"exec_start = "{{ unterminated""#);
        let err = unit
            .render_with(&ctx(), Path::new("/home/u/proj/mise.toml"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("/home/u/proj/mise.toml"), "{err}");
    }
}
