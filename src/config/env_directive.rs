use std::collections::{BTreeSet, HashMap};
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use eyre::{eyre, Context};
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer};

use crate::cmd::CmdLineRunner;
use crate::config::config_file::trust_check;
use crate::config::settings::SETTINGS;
use crate::config::Config;
use crate::env::PATH_KEY;
use crate::env_diff::{EnvDiff, EnvDiffOperation};
use crate::file::{display_path, which_non_pristine};
use crate::tera::{get_tera, BASE_CONTEXT};
use crate::toolset::ToolsetBuilder;
use crate::{dirs, env};

#[derive(Debug, Clone)]
pub enum PathEntry {
    Normal(PathBuf),
    Lazy(PathBuf),
}

impl From<&str> for PathEntry {
    fn from(s: &str) -> Self {
        let pb = PathBuf::from(s);
        Self::Normal(pb)
    }
}

impl FromStr for PathEntry {
    type Err = eyre::Error;

    fn from_str(s: &str) -> eyre::Result<Self> {
        let pb = PathBuf::from_str(s)?;
        Ok(Self::Normal(pb))
    }
}

impl AsRef<Path> for PathEntry {
    #[inline]
    fn as_ref(&self) -> &Path {
        match self {
            PathEntry::Normal(pb) => pb.as_ref(),
            PathEntry::Lazy(pb) => pb.as_ref(),
        }
    }
}

impl<'de> Deserialize<'de> for PathEntry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Debug, Deserialize)]
        struct MapPathEntry {
            value: PathBuf,
        }

        #[derive(Debug, Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Normal(PathBuf),
            Lazy(MapPathEntry),
        }

        Ok(match Helper::deserialize(deserializer)? {
            Helper::Normal(value) => Self::Normal(value),
            Helper::Lazy(this) => Self::Lazy(this.value),
        })
    }
}

#[derive(Debug, Clone)]
pub enum EnvDirective {
    /// simple key/value pair
    Val(String, String),
    /// remove a key
    Rm(String),
    /// dotenv file
    File(PathBuf),
    /// add a path to the PATH
    Path(PathEntry),
    /// run a bash script and apply the resulting env diff
    Source(PathBuf),
    PythonVenv {
        path: PathBuf,
        create: bool,
    },
}

impl From<(String, String)> for EnvDirective {
    fn from((k, v): (String, String)) -> Self {
        Self::Val(k, v)
    }
}

impl From<(String, i64)> for EnvDirective {
    fn from((k, v): (String, i64)) -> Self {
        (k, v.to_string()).into()
    }
}

impl Display for EnvDirective {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvDirective::Val(k, v) => write!(f, "{k}={v}"),
            EnvDirective::Rm(k) => write!(f, "unset {k}"),
            EnvDirective::File(path) => write!(f, "dotenv {}", display_path(path)),
            EnvDirective::Path(path) => write!(f, "path_add {}", display_path(path)),
            EnvDirective::Source(path) => write!(f, "source {}", display_path(path)),
            EnvDirective::PythonVenv { path, create } => {
                write!(f, "python venv path={}", display_path(path))?;
                if *create {
                    write!(f, " create")?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug)]
pub struct EnvResults {
    pub env: IndexMap<String, (String, PathBuf)>,
    pub env_remove: BTreeSet<String>,
    pub env_files: Vec<PathBuf>,
    pub env_paths: Vec<PathBuf>,
    pub env_scripts: Vec<PathBuf>,
}

impl EnvResults {
    pub fn resolve(
        initial: &HashMap<String, String>,
        input: Vec<(EnvDirective, PathBuf)>,
    ) -> eyre::Result<Self> {
        let mut ctx = BASE_CONTEXT.clone();
        trace!("resolve: input: {:#?}", &input);
        let mut env = initial
            .iter()
            .map(|(k, v)| (k.clone(), (v.clone(), None)))
            .collect::<IndexMap<_, _>>();
        let mut r = Self {
            env: Default::default(),
            env_remove: BTreeSet::new(),
            env_files: Vec::new(),
            env_paths: Vec::new(),
            env_scripts: Vec::new(),
        };
        let normalize_path = |config_root: &PathBuf, p: PathBuf| {
            let p = p.strip_prefix("./").unwrap_or(&p);
            match p.strip_prefix("~/") {
                Ok(p) => dirs::HOME.join(p),
                _ if p.is_relative() => config_root.join(p),
                _ => p.to_path_buf(),
            }
        };
        let mut paths: Vec<(PathEntry, PathBuf)> = Vec::new();
        for (directive, source) in input.clone() {
            trace!(
                "resolve: directive: {:?}, source: {:?}",
                &directive,
                &source
            );
            let config_root = source
                .parent()
                .map(Path::to_path_buf)
                .or_else(|| dirs::CWD.clone())
                .unwrap_or_default();
            ctx.insert("cwd", &*dirs::CWD);
            ctx.insert("config_root", &config_root);
            let env_vars = env
                .iter()
                .map(|(k, (v, _))| (k.clone(), v.clone()))
                .collect::<HashMap<_, _>>();
            ctx.insert("env", &env_vars);
            trace!("resolve: ctx.get('env'): {:#?}", &ctx.get("env"));
            match directive {
                EnvDirective::Val(k, v) => {
                    let v = r.parse_template(&ctx, &source, &v)?;
                    r.env_remove.remove(&k);
                    trace!("resolve: inserting {:?}={:?} from {:?}", &k, &v, &source);
                    env.insert(k, (v, Some(source.clone())));
                }
                EnvDirective::Rm(k) => {
                    env.shift_remove(&k);
                    r.env_remove.insert(k);
                }
                EnvDirective::Path(input_str) => {
                    trace!("resolve: input_str: {:#?}", input_str);
                    match input_str {
                        PathEntry::Normal(input) => {
                            trace!(
                                "resolve: normal: input: {:?}, input.to_string(): {:?}",
                                &input,
                                input.to_string_lossy().as_ref()
                            );
                            let s =
                                r.parse_template(&ctx, &source, input.to_string_lossy().as_ref())?;
                            trace!("resolve: s: {:?}", &s);
                            paths.push((PathEntry::Normal(s.into()), source));
                        }
                        PathEntry::Lazy(input) => {
                            trace!(
                                "resolve: lazy: input: {:?}, input.to_string(): {:?}",
                                &input,
                                input.to_string_lossy().as_ref()
                            );
                            paths.push((PathEntry::Lazy(input), source));
                        }
                    }
                }
                EnvDirective::File(input) => {
                    trust_check(&source)?;
                    let s = r.parse_template(&ctx, &source, input.to_string_lossy().as_ref())?;
                    for p in xx::file::glob(normalize_path(&config_root, s.into()))? {
                        r.env_files.push(p.clone());
                        let errfn = || eyre!("failed to parse dotenv file: {}", display_path(&p));
                        for item in dotenvy::from_path_iter(&p).wrap_err_with(errfn)? {
                            let (k, v) = item.wrap_err_with(errfn)?;
                            r.env_remove.remove(&k);
                            env.insert(k, (v, Some(p.clone())));
                        }
                    }
                }
                EnvDirective::Source(input) => {
                    SETTINGS.ensure_experimental("env._.source")?;
                    trust_check(&source)?;
                    let s = r.parse_template(&ctx, &source, input.to_string_lossy().as_ref())?;
                    for p in xx::file::glob(normalize_path(&config_root, s.into()))? {
                        r.env_scripts.push(p.clone());
                        let env_diff = EnvDiff::from_bash_script(&p, env_vars.clone())?;
                        for p in env_diff.to_patches() {
                            match p {
                                EnvDiffOperation::Add(k, v) | EnvDiffOperation::Change(k, v) => {
                                    r.env_remove.remove(&k);
                                    env.insert(k.clone(), (v.clone(), Some(source.clone())));
                                }
                                EnvDiffOperation::Remove(k) => {
                                    env.shift_remove(&k);
                                    r.env_remove.insert(k);
                                }
                            }
                        }
                    }
                }
                EnvDirective::PythonVenv { path, create } => {
                    trace!("python venv: {} create={create}", display_path(&path));
                    trust_check(&source)?;
                    let venv = r.parse_template(&ctx, &source, path.to_string_lossy().as_ref())?;
                    let venv = normalize_path(&config_root, venv.into());
                    if !venv.exists() && create {
                        // TODO: the toolset stuff doesn't feel like it's in the right place here
                        let config = Config::get();
                        let ts = ToolsetBuilder::new().build(&config)?;
                        let path = ts
                            .list_paths()
                            .into_iter()
                            .chain(env::split_paths(&env_vars[&*PATH_KEY]))
                            .collect::<Vec<_>>();
                        if ts
                            .list_missing_versions()
                            .iter()
                            .any(|tv| tv.backend.name == "python")
                        {
                            debug!("python not installed, skipping venv creation");
                        } else {
                            let cmd = if let (false, Some(_uv_in_path)) =
                                (SETTINGS.python.venv_stdlib, which_non_pristine("uv"))
                            {
                                CmdLineRunner::new("uv").args(["venv", &venv.to_string_lossy()])
                            } else {
                                CmdLineRunner::new("python3").args([
                                    "-m",
                                    "venv",
                                    &venv.to_string_lossy(),
                                ])
                            }
                            .envs(&env_vars)
                            .env(
                                PATH_KEY.to_string(),
                                env::join_paths(&path)?.to_string_lossy().to_string(),
                            );
                            info!("creating venv at: {}", display_path(&venv));
                            cmd.execute()?;
                        }
                    }
                    if venv.exists() {
                        r.env_paths.insert(0, venv.join("bin"));
                        env.insert(
                            "VIRTUAL_ENV".into(),
                            (venv.to_string_lossy().to_string(), Some(source.clone())),
                        );
                    } else {
                        warn!(
                            "no venv found at: {p}\n\n\
                            To create a virtualenv manually, run:\n\
                            python -m venv {p}",
                            p = display_path(&venv)
                        );
                    }
                }
            };
        }
        let env_vars = env
            .iter()
            .map(|(k, (v, _))| (k.clone(), v.clone()))
            .collect::<HashMap<_, _>>();
        ctx.insert("env", &env_vars);
        for (k, (v, source)) in env {
            if let Some(source) = source {
                r.env.insert(k, (v, source));
            }
        }
        trace!("resolve: paths: {:#?}", &paths);
        trace!("resolve: ctx.env: {:#?}", &ctx.get("env"));
        for (entry, source) in paths {
            trace!("resolve: entry: {:?}, source: {:?}", &entry, &source);
            let config_root = source
                .parent()
                .map(Path::to_path_buf)
                .or_else(|| dirs::CWD.clone())
                .unwrap_or_default();
            let s = match entry {
                PathEntry::Normal(pb) => pb.to_string_lossy().to_string(),
                PathEntry::Lazy(pb) => {
                    let s = r.parse_template(&ctx, &source, pb.to_string_lossy().as_ref())?;
                    trace!("resolve: s: {:?}", &s);
                    s
                }
            };
            env::split_paths(&s)
                .map(|s| normalize_path(&config_root, s))
                .for_each(|p| r.env_paths.push(p.clone()));
        }
        Ok(r)
    }

    fn parse_template(
        &self,
        ctx: &tera::Context,
        path: &Path,
        input: &str,
    ) -> eyre::Result<String> {
        if !input.contains("{{") && !input.contains("{%") && !input.contains("{#") {
            return Ok(input.to_string());
        }
        trust_check(path)?;
        let dir = path.parent();
        let output = get_tera(dir)
            .render_str(input, ctx)
            .wrap_err_with(|| eyre!("failed to parse template: '{input}'"))?;
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_debug_snapshot;
    use test_log::test;

    use crate::test::{replace_path, reset};

    use super::*;

    #[test]
    fn test_env_path() {
        reset();
        let mut env = HashMap::new();
        env.insert("A".to_string(), "1".to_string());
        env.insert("B".to_string(), "2".to_string());
        let results = EnvResults::resolve(
            &env,
            vec![
                (
                    EnvDirective::Path("/path/1".into()),
                    PathBuf::from("/config"),
                ),
                (
                    EnvDirective::Path("/path/2".into()),
                    PathBuf::from("/config"),
                ),
                (
                    EnvDirective::Path("~/foo/{{ env.A }}".into()),
                    Default::default(),
                ),
                (
                    EnvDirective::Path("./rel/{{ env.A }}:./rel2/{{env.B}}".into()),
                    Default::default(),
                ),
            ],
        )
        .unwrap();
        assert_debug_snapshot!(
            results.env_paths.into_iter().map(|p| replace_path(&p.display().to_string())).collect::<Vec<_>>(),
            @r###"
        [
            "/path/1",
            "/path/2",
            "~/foo/1",
            "~/cwd/rel/1",
            "~/cwd/rel2/2",
        ]
        "###
        );
    }

    #[test]
    fn test_venv_path() {
        reset();
        let env = HashMap::new();
        let results = EnvResults::resolve(
            &env,
            vec![
                (
                    EnvDirective::PythonVenv {
                        path: PathBuf::from("/"),
                        create: false,
                    },
                    Default::default(),
                ),
                (
                    EnvDirective::PythonVenv {
                        path: PathBuf::from("./"),
                        create: false,
                    },
                    Default::default(),
                ),
            ],
        )
        .unwrap();
        // expect order to be reversed as it processes directives from global to dir specific
        assert_debug_snapshot!(
            results.env_paths.into_iter().map(|p| replace_path(&p.display().to_string())).collect::<Vec<_>>(),
            @r###"
        [
            "~/cwd/bin",
            "/bin",
        ]
        "###
        );
    }
}
