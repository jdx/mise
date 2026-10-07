//! Parsing `[secrets.*]`, choosing the source, and the project-only rule.

use std::path::{Path, PathBuf};

use std::sync::Arc;

use eyre::{Result, bail};

use crate::config::Config;
use crate::config::Settings;
use crate::config::config_file::{ConfigFile, is_path_trusted};
use crate::config::is_conf_d_folder_file;
use crate::config::settings::SettingsExt;
use crate::dirs;
use crate::file::{self, display_path};
use crate::task::Task;
use crate::task::task_context_builder::TaskContextBuilder;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SecretsToml {
    pub(crate) fnox: Option<FnoxToml>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct FnoxToml {
    pub(crate) profile: Option<String>,
}

fn is_valid_profile(s: &str) -> bool {
    if s == "." || s == ".." {
        return false;
    }
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphanumeric() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

fn unknown_field_hint(key: &str) -> Option<&'static str> {
    match key {
        "run" | "tasks" | "keys" => {
            Some("tasks receive the secrets they list: [tasks.NAME] secrets = [...]")
        }
        "exec" => {
            Some("mise x gets secrets only when asked: mise x --secrets GH_TOKEN -- <command>")
        }
        "shell" => {
            Some("mise secrets are never exported to your shell; use `fnox activate` for that")
        }
        _ => None,
    }
}

impl SecretsToml {
    /// Only for files that pass the project rule.
    pub(crate) fn from_value(v: &toml::Value, path: &Path) -> Result<SecretsToml> {
        let file = display_path(path);
        let not_table = || {
            eyre::eyre!(
                "invalid [secrets.fnox] in {file}: expected a table such as [secrets.fnox] profile = \"dev\""
            )
        };
        let table = v.as_table().ok_or_else(not_table)?;
        let mut out = SecretsToml::default();
        for (kind, value) in table {
            if kind != "fnox" {
                warn_once!(
                    "[secrets.{kind}] in {file} is ignored: this mise supports [secrets.fnox] only"
                );
                continue;
            }
            let fnox = value.as_table().ok_or_else(not_table)?;
            let mut parsed = FnoxToml::default();
            for (key, value) in fnox {
                if key == "profile" {
                    match value.as_str() {
                        Some(p) if is_valid_profile(p) => parsed.profile = Some(p.to_string()),
                        _ => bail!(
                            "invalid [secrets.fnox] in {file}: profile must be a fnox profile name such as \"dev\""
                        ),
                    }
                } else {
                    let mut msg = format!(
                        "invalid [secrets.fnox] in {file}: unknown field `{key}` (supported: profile)"
                    );
                    if let Some(hint) = unknown_field_hint(key) {
                        msg.push('\n');
                        msg.push_str(hint);
                    }
                    bail!("{msg}");
                }
            }
            out.fnox = Some(parsed);
        }
        Ok(out)
    }
}

/// false for `$HOME` itself and for every ancestor of `$HOME` (files there apply to every
/// project under `$HOME`).
pub(crate) fn is_project_secrets_root(root: &Path, home: &Path) -> bool {
    let home = file::desymlink_path(home);
    let root = file::desymlink_path(root);
    !home.starts_with(&root)
}

/// `<dir>/[.config/]{mise,.mise}/conf.d/<folder>/mise*.toml` is discovered in `<dir>`.
fn conf_d_discovery_dir(p: &Path) -> Option<PathBuf> {
    let dir = p.parent()?.parent()?.parent()?.parent()?;
    Some(if dir.file_name().is_some_and(|n| n == ".config") {
        dir.parent()?.to_path_buf()
    } else {
        dir.to_path_buf()
    })
}

fn is_project_path(path: &Path, root: Option<PathBuf>, home: &Path) -> bool {
    root.is_some_and(|r| is_project_secrets_root(&r, home))
        && (!is_conf_d_folder_file(path)
            || conf_d_discovery_dir(path).is_some_and(|d| is_project_secrets_root(&d, home)))
}

fn is_project_file(cf: &dyn ConfigFile) -> bool {
    is_project_path(cf.get_path(), cf.project_root(), &dirs::HOME)
}

#[derive(Debug)]
pub(crate) struct SelectedSource {
    pub(crate) root: PathBuf,
    /// nearest first
    pub(crate) declared_in: Vec<PathBuf>,
    pub(crate) profile: Option<String>,
}

#[derive(Debug)]
pub(crate) struct SourceSelection {
    pub(crate) source: Option<SelectedSource>,
    pub(crate) ignored: Vec<PathBuf>,
}

/// `files` in precedence order, highest first: (path, `Some(config_root)` if the file passes
/// the project rule, secrets value). The nearest declaring file wins per field.
pub(crate) fn select(
    files: &[(PathBuf, Option<PathBuf>, Option<toml::Value>)],
) -> Result<SourceSelection> {
    let mut ignored = vec![];
    let mut source: Option<SelectedSource> = None;
    for (path, root, value) in files {
        let Some(value) = value else { continue };
        let Some(root) = root else {
            ignored.push(path.clone());
            continue;
        };
        let parsed = SecretsToml::from_value(value, path)?;
        let Some(fnox) = parsed.fnox else { continue };
        match &mut source {
            None => {
                source = Some(SelectedSource {
                    root: root.clone(),
                    declared_in: vec![path.clone()],
                    profile: fnox.profile,
                });
            }
            Some(s) => {
                s.declared_in.push(path.clone());
                if s.profile.is_none() {
                    s.profile = fnox.profile;
                }
            }
        }
    }
    Ok(SourceSelection { source, ignored })
}

fn config_inputs<'a>(
    files: impl IntoIterator<Item = (&'a PathBuf, &'a Arc<dyn ConfigFile>)>,
) -> Vec<(PathBuf, Option<PathBuf>, Option<toml::Value>)> {
    files
        .into_iter()
        .map(|(path, cf)| {
            let value = cf.secrets_config();
            let root = (value.is_some() && is_project_file(cf.as_ref())).then(|| cf.config_root());
            (path.clone(), root, value)
        })
        .collect()
}

/// Safe mode refuses, and every declaring file must be trusted.
fn gate(selection: SourceSelection) -> Result<SourceSelection> {
    if let Some(source) = &selection.source {
        for file in &source.declared_in {
            if !is_path_trusted(file) {
                bail!(
                    "mise secrets: {} is not trusted, so its [secrets.fnox] is not used. Trust it with: mise trust {}",
                    display_path(file),
                    display_path(&source.root)
                );
            }
        }
    }
    Ok(selection)
}

/// Every caller inherits the gate.
pub(crate) fn select_for_cwd(config: &Config) -> Result<SourceSelection> {
    Settings::ensure_not_safe("mise secrets")?;
    gate(select(&config_inputs(config.config_files.iter()))?)
}

/// The same files the task's `[env]` is read from: the task's own hierarchy for a monorepo
/// task, the current project's files otherwise. No safe-mode or trust gate.
pub(crate) async fn select_for_task_ungated(
    config: &Arc<Config>,
    ctx_builder: &TaskContextBuilder,
    task: &Task,
) -> Result<SourceSelection> {
    let hierarchy = match task.cf(config) {
        Some(task_cf) if task.cf.is_some() => {
            ctx_builder.task_config_files(config, task, task_cf).await?
        }
        _ => None,
    };
    match &hierarchy {
        Some(files) => select(&config_inputs(files.iter())),
        None => select(&config_inputs(config.config_files.iter())),
    }
}

pub(crate) async fn select_for_task(
    config: &Arc<Config>,
    ctx_builder: &TaskContextBuilder,
    task: &Task,
) -> Result<SourceSelection> {
    Settings::ensure_not_safe("mise secrets")?;
    gate(select_for_task_ungated(config, ctx_builder, task).await?)
}

/// For `mise doctor` only: no gate, and parse errors are the caller's to report.
pub(crate) fn select_for_cwd_ungated(config: &Config) -> Result<SourceSelection> {
    select(&config_inputs(config.config_files.iter()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(s: &str) -> toml::Value {
        toml::from_str::<toml::Value>(s).unwrap()["secrets"].clone()
    }

    fn parse(s: &str) -> Result<SecretsToml> {
        SecretsToml::from_value(&val(s), Path::new("/p/mise.toml"))
    }

    #[test]
    fn parses_fnox() {
        let p = parse("[secrets.fnox]\n").unwrap();
        assert_eq!(p.fnox, Some(FnoxToml { profile: None }));
        let p = parse("[secrets.fnox]\nprofile = \"dev\"\n").unwrap();
        assert_eq!(p.fnox.unwrap().profile.as_deref(), Some("dev"));
    }

    #[test]
    fn unknown_field_is_s7() {
        let e = parse("[secrets.fnox]\nprofle = \"x\"\n")
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("unknown field `profle` (supported: profile)"),
            "{e}"
        );
        let e = parse("[secrets.fnox]\nexec = [\"A\"]\n")
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("unknown field `exec`")
                && e.contains("mise x --secrets GH_TOKEN -- <command>"),
            "{e}"
        );
        let e = parse("[secrets.fnox]\ntasks = [\"A\"]\n")
            .unwrap_err()
            .to_string();
        assert!(e.contains("[tasks.NAME] secrets"), "{e}");
        let e = parse("[secrets.fnox]\nshell = true\n")
            .unwrap_err()
            .to_string();
        assert!(e.contains("fnox activate"), "{e}");
    }

    #[test]
    fn bad_shapes_are_s8() {
        let e = parse("[secrets]\nfnox = \"x\"\n").unwrap_err().to_string();
        assert!(e.contains("expected a table such as [secrets.fnox]"), "{e}");
        let top = toml::from_str::<toml::Value>("secrets = [\"DEPLOY_KEY\"]\n").unwrap();
        let e = SecretsToml::from_value(&top["secrets"], Path::new("/p/mise.toml"))
            .unwrap_err()
            .to_string();
        assert!(e.contains("expected a table such as [secrets.fnox]"), "{e}");
        for bad in ["-x", "", ".", "..", "a b", "a/b"] {
            let e = parse(&format!("[secrets.fnox]\nprofile = \"{bad}\"\n"))
                .unwrap_err()
                .to_string();
            assert!(
                e.contains("profile must be a fnox profile name"),
                "{bad}: {e}"
            );
        }
        assert!(parse("[secrets.fnox]\nprofile = 1\n").is_err());
        assert!(parse("[secrets.fnox]\nprofile = \"a.b-c_d\"\n").is_ok());
    }

    #[test]
    fn other_kinds_are_ignored() {
        let p = parse("[secrets.vault]\nx = 1\n").unwrap();
        assert_eq!(p.fnox, None);
    }

    fn input(
        path: &str,
        root: Option<&str>,
        body: Option<&str>,
    ) -> (PathBuf, Option<PathBuf>, Option<toml::Value>) {
        (PathBuf::from(path), root.map(PathBuf::from), body.map(val))
    }

    #[test]
    fn nearest_wins_per_field() {
        let sel = select(&[
            input(
                "/p/mise.local.toml",
                Some("/p"),
                Some("[secrets.fnox]\nprofile = \"local\"\n"),
            ),
            input(
                "/p/mise.toml",
                Some("/p"),
                Some("[secrets.fnox]\nprofile = \"dev\"\n"),
            ),
            input("/p/sub/mise.toml", Some("/p/sub"), Some("[secrets.fnox]\n")),
            input("/p/other.toml", Some("/p"), None),
        ])
        .unwrap();
        let s = sel.source.unwrap();
        assert_eq!(s.profile.as_deref(), Some("local"));
        assert_eq!(s.root, PathBuf::from("/p"));
        assert_eq!(
            s.declared_in,
            vec![
                PathBuf::from("/p/mise.local.toml"),
                PathBuf::from("/p/mise.toml"),
                PathBuf::from("/p/sub/mise.toml")
            ]
        );
        assert!(sel.ignored.is_empty());

        // a nearer file without a profile does not mask a farther one
        let sel = select(&[
            input("/p/sub/mise.toml", Some("/p/sub"), Some("[secrets.fnox]\n")),
            input(
                "/p/mise.toml",
                Some("/p"),
                Some("[secrets.fnox]\nprofile = \"dev\"\n"),
            ),
        ])
        .unwrap();
        let s = sel.source.unwrap();
        assert_eq!(s.root, PathBuf::from("/p/sub"));
        assert_eq!(s.profile.as_deref(), Some("dev"));
    }

    #[test]
    fn non_project_files_are_ignored_and_never_parsed() {
        let sel = select(&[
            (
                PathBuf::from("/h/.config/mise/config.toml"),
                None,
                Some(toml::Value::Integer(1)),
            ),
            input("/p/mise.toml", Some("/p"), Some("[secrets.fnox]\n")),
        ]);
        let sel = sel.unwrap();
        assert!(sel.source.is_some());
        assert_eq!(
            sel.ignored,
            vec![PathBuf::from("/h/.config/mise/config.toml")]
        );
        let sel = select(&[input("/h/mise.toml", None, Some("[secrets.fnox]\n"))]).unwrap();
        assert!(sel.source.is_none());
        assert_eq!(sel.ignored.len(), 1);
    }

    #[test]
    fn project_root_rule() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let project = home.join("src/app");
        std::fs::create_dir_all(&project).unwrap();
        assert!(!is_project_secrets_root(&home, &home));
        assert!(!is_project_secrets_root(t.path(), &home));
        assert!(!is_project_secrets_root(
            home.ancestors().last().unwrap(),
            &home
        ));
        assert!(is_project_secrets_root(&project, &home));
        assert!(is_project_secrets_root(Path::new("/elsewhere/app"), &home));
        #[cfg(unix)]
        {
            let link = t.path().join("homelink");
            std::os::unix::fs::symlink(&home, &link).unwrap();
            assert!(!is_project_secrets_root(&link, &home));
            assert!(!is_project_secrets_root(&home, &link));
        }
    }

    #[test]
    fn home_level_files_are_not_project_files() {
        use crate::config::config_file::mise_toml::MiseToml;
        for rel in [".config/mise.toml", ".mise/config.toml", "mise/config.toml"] {
            let cf = MiseToml::init(&dirs::HOME.join(rel));
            assert_eq!(cf.project_root(), Some(dirs::HOME.to_path_buf()), "{rel}");
            assert!(!is_project_file(&cf), "{rel}");
        }
        for rel in [
            ".mise/conf.d/team/mise.toml",
            "mise/conf.d/team/mise.toml",
            ".config/mise/conf.d/team/mise.toml",
        ] {
            let path = dirs::HOME.join(rel);
            assert!(is_conf_d_folder_file(&path), "{rel}");
            let cf = MiseToml::init(&path);
            assert!(!is_project_file(&cf), "{rel}");
        }
        let project = dirs::HOME.join("src/app/.mise/conf.d/team/mise.toml");
        let cf = MiseToml::init(&project);
        assert!(is_project_file(&cf));
        assert_eq!(
            conf_d_discovery_dir(&dirs::HOME.join(".config/mise/conf.d/team/mise.toml")),
            Some(dirs::HOME.to_path_buf())
        );
    }

    #[test]
    fn home_level_filenames_map_to_home() {
        use crate::config::config_file::config_root::config_root;
        let t = tempfile::tempdir().unwrap();
        let home = t.path().to_path_buf();
        for rel in [".config/mise.toml", ".mise/config.toml", "mise/config.toml"] {
            let p = home.join(rel);
            let root = config_root(&p);
            assert_eq!(root, home, "{rel}");
            assert!(!is_project_secrets_root(&root, &home), "{rel}");
        }
    }
}
