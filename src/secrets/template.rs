//! `{{ secrets.NAME }}` in a task's own env values.
//!
//! This is deliberately not Tera. A value that uses secrets may hold only literal text and
//! `{{ secrets.NAME }}` (spaces inside the braces are optional), so there is no `exec()`, no
//! compile cache, no context, and no Tera error text that could carry a value. Rendering is
//! literal substitution, done just before the child is spawned.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::{SecretName, SecretValue};
use crate::file::display_path;

/// An env value of a task that is rendered when the task starts, not when mise loads config.
/// Holds the template text only; never a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LateSecretEnv {
    /// the env var the composite is exported as
    pub(crate) key: String,
    /// the value as written, e.g. `postgres://app:{{ secrets.DB_PASSWORD }}@db/app`
    pub(crate) template: String,
    pub(crate) refs: BTreeSet<SecretName>,
    pub(crate) file: PathBuf,
    /// came from a `[tasks.<name>]` overlay on a file task rather than the task's own env
    pub(crate) overlay: bool,
}

/// A value that uses `{{ secrets.* }}` together with anything else Tera would interpret.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TemplateError;

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("only literal text and {{ secrets.NAME }} are supported")
    }
}

impl std::error::Error for TemplateError {}

/// Whether a TOML body could contain a decoded string that references `secrets`: the word
/// itself, or an escape or line continuation that could spell it. A cheap gate, so a config
/// that cannot is not parsed a second time.
pub(crate) fn may_name_secrets(body: &str) -> bool {
    body.contains("secrets") || body.contains('\\')
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether the identifier `secrets` appears inside a Tera tag of `s`. Lexical, like
/// `tera_template_has_usage_ref`: `secrets` not preceded by `[A-Za-z0-9_.]`, not part of a
/// longer identifier and not inside a string literal. `{% raw %}...{% endraw %}` blocks and
/// `{# #}` comments are skipped, because Tera does not evaluate them.
pub(crate) fn has_secret_ref(s: &str) -> bool {
    !lexical_refs(s).is_empty()
}

/// What follows each `secrets` found by [`has_secret_ref`], for messages: the name when the
/// text is `secrets.NAME`, else `None`.
pub(crate) fn lexical_refs(s: &str) -> Vec<Option<String>> {
    let mut out = vec![];
    let mut rest = s;
    while let Some(start) = next_open(rest) {
        let tag = &rest[start..];
        if tag.starts_with("{#") {
            rest = skip_past(tag, "#}");
            continue;
        }
        if tag.starts_with("{%") && is_tag(tag, "raw") {
            let after = skip_past(tag, "%}");
            rest = skip_raw_block(after);
            continue;
        }
        let close = if tag.starts_with("{{") { "}}" } else { "%}" };
        let end = tag.find(close).map(|e| e + 2).unwrap_or(tag.len());
        scan_tag(&tag[2..end], &mut out);
        rest = &tag[end..];
    }
    out
}

/// The text after a `{# comment #}` or a `{% raw %}...{% endraw %}` block that `tag` starts, or
/// `None` when `tag` starts neither, or the comment or block is not closed (a Tera parse error
/// anyway, so a caller scans it as ordinary text).
pub(super) fn skip_inert(tag: &str) -> Option<&str> {
    if tag.starts_with("{#") {
        let end = tag.find("#}")?;
        return Some(&tag[end + 2..]);
    }
    if tag.starts_with("{%") && is_tag(tag, "raw") {
        let mut rest = skip_past(tag, "%}");
        while let Some(i) = rest.find("{%") {
            let inner = &rest[i..];
            if is_tag(inner, "endraw") {
                return Some(skip_past(inner, "%}"));
            }
            rest = &inner[2..];
        }
    }
    None
}

fn next_open(s: &str) -> Option<usize> {
    ["{{", "{%", "{#"]
        .iter()
        .filter_map(|open| s.find(open))
        .min()
}

fn skip_past<'a>(tag: &'a str, close: &str) -> &'a str {
    tag.find(close).map_or("", |e| &tag[e + close.len()..])
}

/// `{% raw %}`, `{%- raw -%}`: the tag's own word, ignoring whitespace control.
fn is_tag(tag: &str, word: &str) -> bool {
    let Some(inner) = tag.strip_prefix("{%") else {
        return false;
    };
    let inner = inner.split("%}").next().unwrap_or(inner);
    inner.trim_matches(|c: char| c.is_whitespace() || c == '-' || c == '+') == word
}

/// The text after the `{% endraw %}` that closes a raw block whose opening tag is already
/// consumed. An unterminated block runs to the end.
fn skip_raw_block(s: &str) -> &str {
    let mut rest = s;
    while let Some(i) = rest.find("{%") {
        let tag = &rest[i..];
        if is_tag(tag, "endraw") {
            return skip_past(tag, "%}");
        }
        rest = &tag[2..];
    }
    ""
}

fn scan_tag(tag: &str, out: &mut Vec<Option<String>>) {
    let chars: Vec<char> = tag.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '"' | '\'' | '`') {
            // a string literal is data
            i += 1;
            while i < chars.len() && chars[i] != c {
                i += 1;
            }
            i += 1;
            continue;
        }
        let word_start = i == 0 || !(is_ident(chars[i - 1]) || chars[i - 1] == '.');
        let is_word = chars[i..].starts_with(&['s', 'e', 'c', 'r', 'e', 't', 's'])
            && chars.get(i + 7).is_none_or(|c| !is_ident(*c));
        if word_start && is_word {
            let mut j = i + 7;
            let mut name = None;
            if chars.get(j) == Some(&'.') {
                let s = j + 1;
                let mut e = s;
                while e < chars.len() && is_ident(chars[e]) {
                    e += 1;
                }
                if e > s {
                    name = Some(chars[s..e].iter().collect());
                }
                j = e;
            }
            out.push(name);
            i = j.max(i + 7);
            continue;
        }
        // skip a whole identifier so `mysecrets` cannot match from its middle
        if is_ident(c) {
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            continue;
        }
        i += 1;
    }
}

/// If `s` starts with exactly `{{ secrets.NAME }}`, the name and the length consumed.
fn simple_ref(s: &str) -> Option<(SecretName, usize)> {
    let inner = s.strip_prefix("{{")?;
    let trimmed = inner.trim_start_matches(' ');
    let rest = trimmed.strip_prefix("secrets.")?;
    let end = rest.find(|c: char| !is_ident(c)).unwrap_or(rest.len());
    let name = SecretName::new(&rest[..end])?;
    let after = rest[end..].trim_start_matches(' ');
    let after = after.strip_prefix("}}")?;
    Some((name, s.len() - after.len()))
}

/// The names `s` references. Only literal text and the exact `{{ secrets.NAME }}` form are
/// allowed; any other `{{`, `{%` or `{#` (a filter, `{{-`, `secrets["X"]`, `{% raw %}`, a
/// second variable) is an error.
pub(crate) fn secret_refs(s: &str) -> Result<BTreeSet<SecretName>, TemplateError> {
    let mut out = BTreeSet::new();
    let mut rest = s;
    while let Some(start) = next_open(rest) {
        let tag = &rest[start..];
        let Some((name, len)) = simple_ref(tag) else {
            return Err(TemplateError);
        };
        out.insert(name);
        rest = &tag[len..];
    }
    Ok(out)
}

/// Literal substitution of every `{{ secrets.NAME }}`. `None` when a value is missing, which
/// callers report; text that is not a reference is copied as it is.
pub(crate) fn render(s: &str, values: &BTreeMap<SecretName, SecretValue>) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = next_open(rest) {
        out.push_str(&rest[..start]);
        let tag = &rest[start..];
        match simple_ref(tag) {
            Some((name, len)) => {
                out.push_str(values.get(&name)?.expose());
                rest = &tag[len..];
            }
            None => {
                out.push_str(&tag[..2]);
                rest = &tag[2..];
            }
        }
    }
    out.push_str(rest);
    Some(out)
}

/// Whether the literal text around the references holds shell expansion syntax: a `$` followed
/// by `$`, `{` or a letter or underscore. Other env values get `$VAR` expansion, but a value
/// built at spawn cannot, because expanding after substitution would expand a `$` inside a
/// secret. Reference spans are skipped. Call it on a value that already passed [`secret_refs`].
pub(crate) fn literal_has_shell_expansion(s: &str) -> bool {
    let mut literal = String::new();
    let mut rest = s;
    while let Some(start) = next_open(rest) {
        literal.push_str(&rest[..start]);
        literal.push('\u{0}');
        let tag = &rest[start..];
        match simple_ref(tag) {
            Some((_, len)) => rest = &tag[len..],
            None => rest = &tag[2..],
        }
    }
    literal.push_str(rest);
    let chars: Vec<char> = literal.chars().collect();
    chars.windows(2).any(|w| {
        w[0] == '$' && (w[1] == '$' || w[1] == '{' || w[1].is_ascii_alphabetic() || w[1] == '_')
    })
}

/// Whether a `$` sits right before a reference, as in a GitHub Actions `${{ secrets.X }}`.
pub(crate) fn dollar_before_ref(s: &str) -> bool {
    s.contains("${{")
}

/// `${{ secrets.X }}`
pub(crate) fn dollar_before_ref_message(task: &str, key: &str, s: &str) -> String {
    // the reference that follows the `$`, not the first one in the value
    let name = s
        .find("${{")
        .map_or_else(|| first_name(s), |i| first_name(&s[i + 1..]));
    format!(
        "task {task}: env.{key} has ${{{{ secrets.{name} }}}}; drop the $ (mise uses {{{{ secrets.{name} }}}})"
    )
}

/// K
pub(crate) fn shell_expansion_message(task: &str, key: &str) -> String {
    format!(
        "task {task}: env.{key} uses $VAR expansion together with {{{{ secrets.* }}}}; values that use secrets are not shell-expanded. Compose the value in fnox (default = \"...${{DB_PASSWORD}}...\") or in the task's script."
    )
}

/// The first reference's name for a message, or `NAME`.
fn first_name(s: &str) -> String {
    lexical_refs(s)
        .into_iter()
        .flatten()
        .next()
        .unwrap_or_else(|| "NAME".to_string())
}

/// T3
pub(crate) fn mixed_message(task: &str, key: &str) -> String {
    format!(
        "task {task}: env.{key} mixes {{{{ secrets.* }}}} with other template syntax; only literal text and {{{{ secrets.NAME }}}} are supported. Compose the value in fnox (default = \"...${{DB_PASSWORD}}...\") or in the task's script."
    )
}

/// G3 for a late key
pub(crate) fn invalid_key_message(task: &str, key: &str) -> String {
    format!(
        "task {task}: env.\"{key}\" uses {{{{ secrets.* }}}} but is not a valid environment variable name ([A-Za-z_][A-Za-z0-9_]*)"
    )
}

/// G12 for a late key
pub(crate) fn reserved_key_message(task: &str, key: &str) -> String {
    format!("task {task}: env.{key} cannot be built from secrets; {key} is reserved by mise")
}

/// T1
fn run_message(task: &str, field: &str, s: &str) -> String {
    let name = first_name(s);
    format!(
        "task {task}: {{{{ secrets.{name} }}}} cannot be used in {field}\n  \
         {field} is passed to the shell as an argument (sh -c \"...\"), which other local users can read through ps and /proc. Grant the key and read it from the environment:\n      \
         [tasks.{task}]\n      \
         secrets = [\"{name}\"]\n      \
         run = './deploy.sh --key \"${name}\"'\n  \
         If you meant a literal GitHub Actions expression, wrap it in {{% raw %}}...{{% endraw %}}."
    )
}

/// T2
fn elsewhere_message(s: &str, path: &str, file: &Path) -> String {
    let name = first_name(s);
    format!(
        "{{{{ secrets.{name} }}}} is only allowed in a task's own env values; found in {path} ({})",
        display_path(file)
    )
}

/// T4
fn config_env_message(s: &str, section: &str, file: &Path) -> String {
    let name = first_name(s);
    format!(
        "{{{{ secrets.{name} }}}} cannot be used in [{section}] ({}): [{section}] is computed when mise loads config and reaches your shell, mise env and the env cache. Put it on the task that needs it:\n      \
         [tasks.migrate]\n      \
         env.DATABASE_URL = \"postgres://app:{{{{ secrets.{name} }}}}@db/app\"",
        display_path(file)
    )
}

/// What a TOML document is, because each places tasks differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TomlShape {
    /// a `mise.toml` body: tasks under `[tasks]`, plus `[env]`, `[vars]` and everything else
    MiseToml,
    /// a task include file: every top-level key is a task
    TaskInclude,
    /// the merged `#MISE` header table of one file task
    Header,
}

/// Walks a decoded TOML value and calls `f` with the path and text of every string, keys
/// excluded, in document order.
fn each_string(
    value: &toml::Value,
    path: &str,
    f: &mut dyn FnMut(&str, &str) -> eyre::Result<()>,
) -> eyre::Result<()> {
    match value {
        toml::Value::String(s) => f(path, s),
        toml::Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                each_string(item, &format!("{path}[{i}]"), f)?;
            }
            Ok(())
        }
        toml::Value::Table(table) => {
            for (k, v) in table {
                let child = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                each_string(v, &child, f)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Fails on the first reference outside a task's own env value:
/// - T1 in `run` or `run_windows` (or a task written as a string or array);
/// - T4 in `[env]` and `[vars]` of a `mise.toml`;
/// - T2 anywhere else (other task fields, `depends*` and their env, run-entry `{task, env}`,
///   `[task_templates]`, `task_defaults`, hooks, `[tools]`, ...).
///
/// Works on decoded strings, so an escaped `{{` is caught as well.
pub(crate) fn check_toml_locations(
    shape: TomlShape,
    table: &toml::Table,
    file: &Path,
) -> eyre::Result<()> {
    let other = |path: &str, s: &str| -> eyre::Result<()> {
        if has_secret_ref(s) {
            eyre::bail!("{}", elsewhere_message(s, path, file));
        }
        Ok(())
    };
    match shape {
        TomlShape::MiseToml => {
            for (key, value) in table {
                match (key.as_str(), value) {
                    ("tasks", toml::Value::Table(tasks)) => {
                        for (name, task) in tasks {
                            check_task(name, task, &format!("tasks.{name}"), file)?;
                        }
                    }
                    ("env" | "vars", _) => each_string(value, key, &mut |_, s| {
                        if has_secret_ref(s) {
                            eyre::bail!("{}", config_env_message(s, key, file));
                        }
                        Ok(())
                    })?,
                    _ => each_string(value, key, &mut |p, s| other(p, s))?,
                }
            }
        }
        TomlShape::TaskInclude => {
            for (name, task) in table {
                check_task(name, task, name, file)?;
            }
        }
        TomlShape::Header => {
            for (key, value) in table {
                if key == "env" {
                    check_env(value, key, file)?;
                } else {
                    each_string(value, key, &mut |p, s| other(p, s))?;
                }
            }
        }
    }
    Ok(())
}

/// One task's table (or its string and array shorthand).
fn check_task(name: &str, task: &toml::Value, path: &str, file: &Path) -> eyre::Result<()> {
    let in_run = |field: &str, value: &toml::Value| -> eyre::Result<()> {
        let field_path = format!("{path}.{field}");
        match value {
            toml::Value::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    match item {
                        toml::Value::String(s) if has_secret_ref(s) => {
                            eyre::bail!("{}", run_message(name, field, s))
                        }
                        // `{ task = "b", env = { ... } }` and friends are not run text
                        other => each_string(other, &format!("{field_path}[{i}]"), &mut |p, s| {
                            if has_secret_ref(s) {
                                eyre::bail!("{}", elsewhere_message(s, p, file));
                            }
                            Ok(())
                        })?,
                    }
                }
                Ok(())
            }
            other => each_string(other, &field_path, &mut |_, s| {
                if has_secret_ref(s) {
                    eyre::bail!("{}", run_message(name, field, s));
                }
                Ok(())
            }),
        }
    };
    match task {
        toml::Value::String(_) | toml::Value::Array(_) => in_run("run", task),
        toml::Value::Table(fields) => {
            for (field, value) in fields {
                match field.as_str() {
                    "run" | "run_windows" => in_run(field, value)?,
                    "env" => check_env(value, &format!("{path}.env"), file)?,
                    _ => each_string(value, &format!("{path}.{field}"), &mut |p, s| {
                        if has_secret_ref(s) {
                            eyre::bail!("{}", elsewhere_message(s, p, file));
                        }
                        Ok(())
                    })?,
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// A task's `env`: `K = "..."` and `K = { value = "..." }` are the legal places.
fn check_env(env: &toml::Value, path: &str, file: &Path) -> eyre::Result<()> {
    let strict = |p: &str, s: &str| -> eyre::Result<()> {
        if has_secret_ref(s) {
            eyre::bail!("{}", elsewhere_message(s, p, file));
        }
        Ok(())
    };
    let toml::Value::Table(vars) = env else {
        return each_string(env, path, &mut |p, s| strict(p, s));
    };
    for (key, value) in vars {
        let key_path = format!("{path}.{key}");
        match value {
            // `_` holds directives (`_.file`, `_.source`, ...), which are never a place
            toml::Value::String(_) if key != "_" => {}
            toml::Value::Table(options) if key != "_" => {
                for (option, v) in options {
                    if option == "value" && matches!(v, toml::Value::String(_)) {
                        continue;
                    }
                    each_string(v, &format!("{key_path}.{option}"), &mut |p, s| strict(p, s))?;
                }
            }
            other => each_string(other, &key_path, &mut |p, s| strict(p, s))?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(s: &str) -> Vec<String> {
        secret_refs(s)
            .unwrap()
            .iter()
            .map(|n| n.to_string())
            .collect()
    }

    #[test]
    fn accepts_the_exact_form() {
        assert_eq!(names("{{ secrets.A }}"), ["A"]);
        assert_eq!(names("{{secrets.A}}"), ["A"]);
        assert_eq!(names("{{   secrets.A   }}"), ["A"]);
        assert_eq!(
            names(
                "postgres://app:{{ secrets.DB_PASSWORD }}@{{secrets.HOST}}/{{ secrets.DB_PASSWORD }}"
            ),
            ["DB_PASSWORD", "HOST"]
        );
        assert_eq!(names("plain { text } and $x"), Vec::<String>::new());
    }

    #[test]
    fn everything_else_is_t3() {
        for bad in [
            "{{ secrets.A | upper }}",
            "{{ env.HOME }}{{ secrets.A }}",
            "{% if secrets.A %}x{% endif %}",
            "{{ secrets[\"A\"] }}",
            "{{- secrets.A }}",
            "{{ secrets.A -}}",
            "{% raw %}{{ secrets.A }}{% endraw %}{{ secrets.B }}",
            "{# c #}{{ secrets.A }}",
            "{{ secrets.A }} {{ \"x\" }}",
            "{{{ secrets.A }}}",
            "{{ secrets.1A }}",
            "{{ secrets. A }}",
            "{{ secrets.A",
        ] {
            assert!(secret_refs(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn finds_references_lexically() {
        for yes in [
            "{{ secrets.A }}",
            "{{secrets.A}}",
            "{{ secrets.A | upper }}",
            "{{- secrets.A }}",
            "{{ secrets[\"A\"] }}",
            "{% if secrets.A %}x{% endif %}",
            "x {{ env.HOME }} {{ secrets.A }}",
            "${{ secrets.GITHUB_TOKEN }}",
            "{{ secrets }}",
        ] {
            assert!(has_secret_ref(yes), "{yes}");
        }
        for no in [
            "mysecrets.A",
            "{{ mysecrets.A }}",
            "{{ x.secrets.A }}",
            "{{ vars.secrets }}",
            "{{ secrets_x }}",
            "{{ \"secrets.A\" }}",
            "secrets.A",
            "{% raw %}{{ secrets.A }}{% endraw %}",
            "{%- raw -%}{{ secrets.A }}{%- endraw -%}",
            "{# {{ secrets.A }} #}",
            "${{ vars.x }}",
        ] {
            assert!(!has_secret_ref(no), "{no}");
        }
        // raw ends where it says it does
        assert!(has_secret_ref("{% raw %}x{% endraw %}{{ secrets.A }}"));
    }

    #[test]
    fn shell_expansion_syntax_in_the_literal_text() {
        for yes in [
            "postgres://$U:{{ secrets.A }}@h",
            "a${B}{{ secrets.A }}",
            "a$${{ secrets.A }}",
            "{{ secrets.A }}$_x",
        ] {
            assert!(literal_has_shell_expansion(yes), "{yes}");
        }
        for no in [
            "cost 5$ {{ secrets.A }}",
            "{{ secrets.A }}",
            "{{ secrets.A }}$",
            "a$1{{ secrets.A }}",
        ] {
            assert!(!literal_has_shell_expansion(no), "{no}");
        }
    }

    #[test]
    fn a_dollar_before_a_reference_is_detected() {
        assert!(dollar_before_ref("${{ secrets.A }}"));
        assert!(dollar_before_ref("x=${{secrets.A}}"));
        assert!(!dollar_before_ref("$ {{ secrets.A }}"));
        assert!(!dollar_before_ref("{{ secrets.A }}$"));
        assert_eq!(
            dollar_before_ref_message("t", "GH_TOKEN", "${{ secrets.A }}"),
            "task t: env.GH_TOKEN has ${{ secrets.A }}; drop the $ (mise uses {{ secrets.A }})"
        );
        assert_eq!(
            dollar_before_ref_message("t", "K", "{{ secrets.A }}-${{ secrets.B }}"),
            "task t: env.K has ${{ secrets.B }}; drop the $ (mise uses {{ secrets.B }})"
        );
    }

    #[test]
    fn rendering_is_literal_substitution() {
        let values = BTreeMap::from([
            (
                SecretName::new("A").unwrap(),
                SecretValue::new("{{ secrets.B }}"),
            ),
            (SecretName::new("B").unwrap(), SecretValue::new("b$1\\n")),
        ]);
        assert_eq!(
            render("x={{ secrets.A }}/{{secrets.B}}{y}", &values).as_deref(),
            Some("x={{ secrets.B }}/b$1\\n{y}")
        );
        assert_eq!(render("{{ secrets.C }}", &values), None);
        assert_eq!(render("no refs {", &values).as_deref(), Some("no refs {"));
    }

    fn check(shape: TomlShape, body: &str) -> Result<(), String> {
        let table: toml::Table = toml::from_str(body).unwrap();
        check_toml_locations(shape, &table, Path::new("/p/mise.toml")).map_err(|e| format!("{e}"))
    }

    #[test]
    fn mise_toml_locations() {
        let ok = |body| check(TomlShape::MiseToml, body).unwrap();
        ok("[tasks.m]\nenv.A = \"x{{ secrets.B }}\"\nrun = 'echo $A'\n");
        ok("[tasks.m.env]\nA = { value = \"{{ secrets.B }}\" }\n");
        ok("[tasks.m]\nrun = '{% raw %}${{ secrets.GITHUB_TOKEN }}{% endraw %}'\n");
        ok("[env]\nA = \"{% raw %}{{ secrets.B }}{% endraw %}\"\n");

        let t1 = check(
            TomlShape::MiseToml,
            "[tasks.deploy]\nrun = './d --key {{ secrets.DEPLOY_KEY }}'\n",
        )
        .unwrap_err();
        assert!(
            t1.starts_with("task deploy: {{ secrets.DEPLOY_KEY }} cannot be used in run\n"),
            "{t1}"
        );
        assert!(
            t1.contains("secrets = [\"DEPLOY_KEY\"]") && t1.contains("{% raw %}"),
            "{t1}"
        );
        for body in [
            "[tasks.d]\nrun = ['a', '{{ secrets.A }}']\n",
            "[tasks.d]\nrun_windows = '{{ secrets.A }}'\n",
            "tasks.d = '{{ secrets.A }}'\n",
            "tasks.d = ['{{ secrets.A }}']\n",
        ] {
            let e = check(TomlShape::MiseToml, body).unwrap_err();
            assert!(e.contains("cannot be used in run"), "{body}: {e}");
        }

        for (body, path) in [
            (
                "[tasks.a]\ndescription = '{{ secrets.A }}'\n",
                "tasks.a.description",
            ),
            (
                "[tasks.a]\ndepends = ['b {{ secrets.A }}']\n",
                "tasks.a.depends[0]",
            ),
            (
                "[tasks.a]\ndepends = [{ task = 'b', env = { X = '{{ secrets.A }}' } }]\n",
                "tasks.a.depends[0].env.X",
            ),
            (
                "[tasks.a]\nrun = [{ task = 'b', env = { X = '{{ secrets.A }}' } }]\n",
                "tasks.a.run[0].env.X",
            ),
            (
                "[task_templates.t]\nenv.X = '{{ secrets.A }}'\n",
                "task_templates.t.env.X",
            ),
            ("[hooks]\nenter = '{{ secrets.A }}'\n", "hooks.enter"),
            ("[tools]\nnode = '{{ secrets.A }}'\n", "tools.node"),
            (
                "[tasks.a]\nenv._.file = '{{ secrets.A }}'\n",
                "tasks.a.env._.file",
            ),
            (
                "[tasks.a]\nenv.X = { default = '{{ secrets.A }}' }\n",
                "tasks.a.env.X.default",
            ),
            ("[tasks.a]\nvars.X = '{{ secrets.A }}'\n", "tasks.a.vars.X"),
        ] {
            let e = check(TomlShape::MiseToml, body).unwrap_err();
            assert!(
                e.contains("is only allowed in a task's own env values; found in ")
                    && e.contains(&format!("found in {path} (")),
                "{body}: {e}"
            );
        }
        for (body, section) in [
            ("[env]\nA = '{{ secrets.B }}'\n", "env"),
            ("[vars]\nA = '{{ secrets.B }}'\n", "vars"),
        ] {
            let e = check(TomlShape::MiseToml, body).unwrap_err();
            assert!(e.contains(&format!("cannot be used in [{section}]")), "{e}");
        }
    }

    #[test]
    fn task_include_locations() {
        let ok = |body| check(TomlShape::TaskInclude, body).unwrap();
        ok("[build]\nenv.X = '{{ secrets.A }}'\nrun = 'x'\n");
        // a task named like a mise.toml section is an ordinary task
        ok("[env]\nrun = 'x'\nenv.X = '{{ secrets.A }}'\n");
        ok("[vars]\nenv.X = '{{ secrets.A }}'\n");
        let e = check(TomlShape::TaskInclude, "[build]\nrun = '{{ secrets.A }}'\n").unwrap_err();
        assert!(
            e.contains("task build:") && e.contains("cannot be used in run"),
            "{e}"
        );
        let e = check(
            TomlShape::TaskInclude,
            "[build]\nrun = 'x'\ndepends = ['{{ secrets.A }}']\n",
        )
        .unwrap_err();
        assert!(e.contains("found in build.depends[0]"), "{e}");
        let e = check(TomlShape::TaskInclude, "build = '{{ secrets.A }}'\n").unwrap_err();
        assert!(e.contains("cannot be used in run"), "{e}");
    }

    #[test]
    fn header_locations() {
        check(
            TomlShape::Header,
            "env.X = '{{ secrets.A }}'\ndescription = 'x'\n",
        )
        .unwrap();
        let e = check(
            TomlShape::Header,
            "env.X = '{{ secrets.A }}'\ndepends = ['{{ secrets.A }}']\n",
        )
        .unwrap_err();
        assert!(e.contains("found in depends[0]"), "{e}");
        let e = check(TomlShape::Header, "description = '{{ secrets.A }}'\n").unwrap_err();
        assert!(e.contains("found in description"), "{e}");
    }

    #[test]
    fn escaped_delimiters_are_decoded_before_checking() {
        let e = check(
            TomlShape::MiseToml,
            "[hooks]\nenter = \"\\u007b\\u007b secrets.A }}\"\n",
        )
        .unwrap_err();
        assert!(e.contains("found in hooks.enter"), "{e}");
    }
}
