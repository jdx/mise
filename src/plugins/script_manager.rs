use std::fmt::{Display, Formatter};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::mpsc::channel;
use std::{fmt, thread};

use color_eyre::eyre::{Context, Result};
use duct::Expression;
use indexmap::{indexmap, IndexMap};
use once_cell::sync::Lazy;

use crate::cmd::cmd;
use crate::config::Settings;
use crate::env;
use crate::errors::Error::ScriptFailed;
use crate::file::{basename, display_path};

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
    Download,
    Install,
    Uninstall,
    ListBinPaths,
    ExecEnv,
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
            Script::Install => write!(f, "install"),
            Script::Uninstall => write!(f, "uninstall"),
            Script::ListBinPaths => write!(f, "list-bin-paths"),
            Script::ExecEnv => write!(f, "exec-env"),
            Script::Download => write!(f, "download"),
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

    pub fn cmd(&self, settings: &Settings, script: &Script) -> Expression {
        let args = match script {
            Script::ParseLegacyFile(filename) => vec![filename.clone()],
            _ => vec![],
        };
        let script_path = self.get_script_path(script);
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

    pub fn run(&self, settings: &Settings, script: &Script) -> Result<()> {
        let cmd = self.cmd(settings, script);
        let Output { status, .. } = cmd.unchecked().run()?;

        match status.success() {
            true => Ok(()),
            false => {
                Err(ScriptFailed(display_path(&self.get_script_path(script)), Some(status)).into())
            }
        }
    }

    pub fn read(&self, settings: &Settings, script: &Script, verbose: bool) -> Result<String> {
        let mut cmd = self.cmd(settings, script);
        if !verbose && !settings.raw {
            cmd = cmd.stderr_null();
        }
        cmd.read()
            .with_context(|| ScriptFailed(display_path(&self.get_script_path(script)), None))
    }

    pub fn run_by_line<'a, F1, F2, F3>(
        &'a self,
        settings: &Settings,
        script: &Script,
        on_error: F1,
        on_stdout: F2,
        on_stderr: F3,
    ) -> Result<()>
    where
        F1: Fn(String),
        F2: Fn(&str) + Send + Sync + 'a,
        F3: Fn(&str) + Send + Sync,
    {
        let mut cmd = Command::new(self.get_script_path(script));
        for (k, v) in self.env.iter() {
            cmd.env(k, v);
        }
        if settings.raw {
            let status = cmd.spawn()?.wait()?;
            match status.success() {
                true => Ok(()),
                false => {
                    on_error(String::new());
                    Err(
                        ScriptFailed(display_path(&self.get_script_path(script)), Some(status))
                            .into(),
                    )
                }
            }
        } else {
            let mut cp = cmd
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;
            let stdout = BufReader::new(cp.stdout.take().unwrap());
            let stderr = BufReader::new(cp.stderr.take().unwrap());
            let (tx, rx) = channel();
            thread::spawn({
                let tx = tx.clone();
                move || {
                    for line in stdout.lines() {
                        let line = line.unwrap();
                        tx.send(ChildProcessOutput::Stdout(line)).unwrap();
                    }
                }
            });
            thread::spawn({
                let tx = tx.clone();
                move || {
                    for line in stderr.lines() {
                        let line = line.unwrap();
                        tx.send(ChildProcessOutput::Stderr(line)).unwrap();
                    }
                }
            });
            thread::spawn(move || {
                let status = cp.wait().unwrap();
                tx.send(ChildProcessOutput::ExitStatus(status)).unwrap();
            });
            let mut combined_output = vec![];
            for line in rx {
                match line {
                    ChildProcessOutput::Stdout(line) => {
                        on_stdout(&line);
                        combined_output.push(line);
                    }
                    ChildProcessOutput::Stderr(line) => {
                        on_stderr(&line);
                        combined_output.push(line);
                    }
                    ChildProcessOutput::ExitStatus(status) => match status.success() {
                        true => return Ok(()),
                        false => {
                            on_error(combined_output.join("\n"));
                            Err(ScriptFailed(
                                display_path(&self.get_script_path(script)),
                                Some(status),
                            ))?;
                        }
                    },
                }
            }

            Ok(())
        }
    }
}

enum ChildProcessOutput {
    Stdout(String),
    Stderr(String),
    ExitStatus(ExitStatus),
}
