use crate::fake_asdf::get_path_with_fake_asdf;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Output;

use color_eyre::eyre::{Context, Result};
use duct::Expression;
use indexmap::indexmap;
use once_cell::sync::Lazy;

use crate::cmd::{cmd, CmdLineRunner};
use crate::config::Settings;
use crate::errors::Error;
use crate::errors::Error::ScriptFailed;
use crate::file::{basename, display_path};
use crate::ui::progress_report::ProgressReport;
use crate::{dirs, env};

#[derive(Debug, Clone)]
pub struct ScriptManager {
    pub plugin_path: PathBuf,
    pub plugin_name: String,
    pub env: HashMap<OsString, OsString>,
}

#[derive(Debug, Clone)]
pub enum Script {
    // PreInstall,
    // PostInstall,
    // PreUninstall,
    // PostUninstall,

    // Plugin
    LatestStable,
    ListAliases,
    ListAll,
    ListLegacyFilenames,
    ParseLegacyFile(String),

    // RuntimeVersion
    Download,
    ExecEnv,
    Install,
    ListBinPaths,
    RunExternalCommand(PathBuf, Vec<String>),
    Uninstall,
}

impl Display for Script {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            // Plugin
            Script::LatestStable => write!(f, "latest-stable"),
            Script::ListAll => write!(f, "list-all"),
            Script::ListLegacyFilenames => write!(f, "list-legacy-filenames"),
            Script::ListAliases => write!(f, "list-aliases"),
            Script::ParseLegacyFile(_) => write!(f, "parse-legacy-file"),

            // RuntimeVersion
            Script::Install => write!(f, "install"),
            Script::Uninstall => write!(f, "uninstall"),
            Script::ListBinPaths => write!(f, "list-bin-paths"),
            Script::RunExternalCommand(_, _) => write!(f, "run-external-command"),
            Script::ExecEnv => write!(f, "exec-env"),
            Script::Download => write!(f, "download"),
        }
    }
}

static INITIAL_ENV: Lazy<HashMap<OsString, OsString>> = Lazy::new(|| {
    let mut env: HashMap<OsString, OsString> = env::PRISTINE_ENV
        .iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
    env.extend(
        (indexmap! {
            "__RTX_SCRIPT" => "1".to_string(),
            "ASDF_CONCURRENCY" => num_cpus::get().to_string(),
            "PATH" => get_path_with_fake_asdf(),
            "RTX_CACHE_DIR" => env::RTX_CACHE_DIR.to_string_lossy().to_string(),
            "RTX_CONCURRENCY" => num_cpus::get().to_string(),
            "RTX_DATA_DIR" => dirs::ROOT.to_string_lossy().to_string(),
            "RTX_EXE" => env::RTX_EXE.to_string_lossy().to_string(),
        })
        .into_iter()
        .map(|(k, v)| (k.into(), v.into())),
    );
    env
});

impl ScriptManager {
    pub fn new(plugin_path: PathBuf) -> Self {
        Self {
            plugin_name: basename(&plugin_path).expect("invalid plugin path"),
            env: INITIAL_ENV.clone(),
            plugin_path,
        }
    }

    pub fn with_env<K, V>(mut self, k: K, v: V) -> Self
    where
        K: Into<OsString>,
        V: Into<OsString>,
    {
        self.env.insert(k.into(), v.into());
        self
    }

    pub fn get_script_path(&self, script: &Script) -> PathBuf {
        match script {
            Script::RunExternalCommand(path, _) => path.clone(),
            _ => self.plugin_path.join("bin").join(script.to_string()),
        }
    }

    pub fn script_exists(&self, script: &Script) -> bool {
        self.get_script_path(script).is_file()
    }

    pub fn cmd(&self, settings: &Settings, script: &Script) -> Expression {
        let args = match script {
            Script::ParseLegacyFile(filename) => vec![filename.clone()],
            Script::RunExternalCommand(_, args) => args.clone(),
            _ => vec![],
        };
        let script_path = self.get_script_path(script);
        // if !script_path.exists() {
        //     return Err(PluginNotInstalled(self.plugin_name.clone()).into());
        // }
        let mut cmd = cmd(script_path, args).full_env(&self.env);
        if !settings.raw {
            // ignore stdin, otherwise a prompt may show up where the user won't see it
            cmd = cmd.stdin_null();
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

    pub fn run_by_line(
        &self,
        settings: &Settings,
        script: &Script,
        pr: &ProgressReport,
    ) -> Result<()> {
        let mut cmd = CmdLineRunner::new(settings, self.get_script_path(script));
        cmd.with_pr(pr).env_clear().envs(&self.env);
        if let Err(e) = cmd.execute() {
            let status = match e.downcast_ref::<Error>() {
                Some(ScriptFailed(_, status)) => *status,
                _ => None,
            };
            let path = display_path(&self.get_script_path(script));
            return Err(ScriptFailed(path, status).into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_script_manager() {
        let plugin_path = PathBuf::from("/tmp/asdf");
        let script_manager = ScriptManager::new(plugin_path.clone());
        assert_eq!(script_manager.plugin_path, plugin_path);
        assert_eq!(script_manager.plugin_name, "asdf");
    }

    #[test]
    fn test_get_script_path() {
        let plugin_path = PathBuf::from("/tmp/asdf");
        let script_manager = ScriptManager::new(plugin_path.clone());

        let test = |script, expected| {
            assert_eq!(script_manager.get_script_path(script), expected);
        };

        test(
            &Script::LatestStable,
            plugin_path.join("bin").join("latest-stable"),
        );

        let script = Script::RunExternalCommand(PathBuf::from("/bin/ls"), vec!["-l".to_string()]);
        test(&script, PathBuf::from("/bin/ls"));
    }
}
