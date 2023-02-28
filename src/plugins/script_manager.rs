use std::fmt;
use std::fmt::{Display, Formatter};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::Output;
use std::sync::Arc;

use color_eyre::eyre::{Context, Result};
use duct::Expression;
use indexmap::{indexmap, IndexMap};
use once_cell::sync::Lazy;

use crate::cmd::cmd;
use crate::config::Settings;
use crate::env;
use crate::errors::Error::ScriptFailed;
use crate::file::basename;

#[derive(Debug, Clone)]
pub struct ScriptManager {
    pub plugin_path: PathBuf,
    pub plugin_name: String,
    pub env: IndexMap<String, String>,
}

#[derive(Debug, Clone)]
pub enum Script {
    // PreInstall,
    // PostInstall,
    // PreUninstall,
    // PostUninstall,

    // Plugin
    ListAll,
    ListLegacyFilenames,
    ListAliases,
    ParseLegacyFile(String),

    // RuntimeVersion
    Download(InstallType),
    Install(InstallType),
    Uninstall,
    ListBinPaths,
    // ExecEnv,
}

impl Display for Script {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            // Plugin
            Script::ListAll => write!(f, "list-all"),
            Script::ListLegacyFilenames => write!(f, "list-legacy-filenames"),
            Script::ListAliases => write!(f, "list-aliases"),
            Script::ParseLegacyFile(_) => write!(f, "parse-legacy-file"),

            // RuntimeVersion
            Script::Install(_) => write!(f, "install"),
            Script::Uninstall => write!(f, "uninstall"),
            Script::ListBinPaths => write!(f, "list-bin-paths"),
            // Script::ExecEnv => write!(f, "exec-env"),
            Script::Download(_) => write!(f, "download"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum InstallType {
    Version(String),
    Ref(String),
    Path(PathBuf),
    System,
}

impl Display for InstallType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            InstallType::Version(v) => write!(f, "{v}"),
            InstallType::Ref(r) => write!(f, "ref:{r}"),
            InstallType::Path(p) => write!(f, "path:{}", p.display()),
            InstallType::System => write!(f, "system"),
        }
    }
}

static INITIAL_ENV: Lazy<IndexMap<String, String>> = Lazy::new(|| {
    (indexmap! {
        "RTX_EXE" => env::RTX_EXE.to_string_lossy(),
    })
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
});

impl ScriptManager {
    pub fn new(plugin_path: PathBuf) -> Self {
        Self {
            plugin_name: basename(&plugin_path).expect("invalid plugin path"),
            env: INITIAL_ENV.clone(),
            plugin_path,
        }
    }

    pub fn with_env(mut self, k: String, v: String) -> Self {
        self.env.insert(k, v);
        self
    }

    pub fn with_envs<I>(mut self, envs: I) -> Self
    where
        I: IntoIterator<Item = (String, String)>,
    {
        self.env.extend(envs);
        self
    }

    pub fn get_script_path(&self, script: &Script) -> PathBuf {
        self.plugin_path.join("bin").join(script.to_string())
    }

    pub fn script_exists(&self, script: &Script) -> bool {
        self.get_script_path(script).is_file()
    }

    pub fn cmd(&self, settings: &Settings, script: Script) -> Expression {
        let args = match &script {
            Script::ParseLegacyFile(filename) => vec![filename.clone()],
            _ => vec![],
        };
        let script_path = self.get_script_path(&script);
        // if !script_path.exists() {
        //     return Err(PluginNotInstalled(self.plugin_name.clone()).into());
        // }
        let mut cmd = cmd(&script_path, args);
        if !settings.raw {
            // ignore stdin, otherwise a prompt may show up where the user won't see it
            cmd = cmd.stdin_null();
        }
        for (k, v) in self.env.iter() {
            cmd = cmd.env(k, v);
        }
        cmd
    }

    pub fn run(&self, settings: &Settings, script: Script) -> Result<()> {
        let cmd = self.cmd(settings, script);
        let Output { status, .. } = cmd.unchecked().run()?;

        match status.success() {
            true => Ok(()),
            false => Err(ScriptFailed(self.plugin_name.clone(), Some(status)).into()),
        }
    }

    pub fn read(&self, settings: &Settings, script: Script, verbose: bool) -> Result<String> {
        let mut cmd = self.cmd(settings, script);
        if !verbose && !settings.raw {
            cmd = cmd.stderr_null();
        }
        cmd.read()
            .with_context(|| ScriptFailed(self.plugin_name.clone(), None))
    }

    pub fn run_by_line<F1, F2>(
        &self,
        settings: &Settings,
        script: Script,
        on_error: F1,
        on_output: F2,
    ) -> Result<()>
    where
        F1: Fn(String),
        F2: Fn(&str),
    {
        let cmd = self.cmd(settings, script);
        let mut output = vec![];
        if settings.raw {
            let Output { status, .. } = cmd.unchecked().run()?;
            match status.success() {
                true => Ok(()),
                false => {
                    on_error(output.join("\n"));
                    Err(ScriptFailed(self.plugin_name.clone(), Some(status)).into())
                }
            }
        } else {
            let reader = cmd.stdin_null().stderr_to_stdout().unchecked().reader()?;
            let reader = Arc::new(reader);
            for line in BufReader::new(&*reader).lines() {
                let line = line.unwrap();
                on_output(&line);
                output.push(line);
            }

            match reader.try_wait() {
                Err(err) => {
                    on_error(output.join("\n"));
                    Err(err)?
                }
                Ok(out) => match out.unwrap().status.success() {
                    true => Ok(()),
                    false => {
                        on_error(output.join("\n"));
                        let err = ScriptFailed(self.plugin_name.clone(), Some(out.unwrap().status));
                        Err(err)?
                    }
                },
            }
        }
    }
}
