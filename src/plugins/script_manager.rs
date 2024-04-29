use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use color_eyre::eyre::{Context, Result};
use duct::Expression;
use indexmap::indexmap;
use once_cell::sync::Lazy;

use crate::cmd::{cmd, CmdLineRunner};
use crate::config::Settings;
use crate::errors::Error;
use crate::errors::Error::ScriptFailed;
use crate::fake_asdf::get_path_with_fake_asdf;
use crate::file::display_path;
use crate::ui::progress_report::SingleReport;
use crate::{dirs, env};

#[derive(Debug, Clone)]
pub struct ScriptManager {
    pub plugin_path: PathBuf,
    pub env: HashMap<OsString, OsString>,
}

#[derive(Debug, Clone)]
pub enum Script {
    Hook(String),

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
            Script::Hook(script) => write!(f, "{script}"),

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
    let settings = Settings::get();
    let mut env: HashMap<OsString, OsString> = env::PRISTINE_ENV
        .iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
    if settings.trace {
        env.insert("MISE_TRACE".into(), "1".into());
    }
    if settings.debug {
        env.insert("MISE_DEBUG".into(), "1".into());
        env.insert("MISE_VERBOSE".into(), "1".into());
    }
    env.extend(
        (indexmap! {
            "ASDF_CONCURRENCY" => num_cpus::get().to_string(),
            "PATH" => get_path_with_fake_asdf(),
            "MISE_CACHE_DIR" => env::MISE_CACHE_DIR.to_string_lossy().to_string(),
            "MISE_CONCURRENCY" => num_cpus::get().to_string(),
            "MISE_DATA_DIR" => dirs::DATA.to_string_lossy().to_string(),
            "MISE_LOG_LEVEL" => settings.log_level.to_string(),
            "__MISE_BIN" => env::MISE_BIN.to_string_lossy().to_string(),
            "__MISE_SCRIPT" => "1".to_string(),
        })
        .into_iter()
        .map(|(k, v)| (k.into(), v.into())),
    );
    env
});

impl ScriptManager {
    pub fn new(plugin_path: PathBuf) -> Self {
        let mut env = INITIAL_ENV.clone();
        if let Some(failure) = env::var_os("MISE_FAILURE") {
            // used for testing failure cases
            env.insert("MISE_FAILURE".into(), failure);
        }
        Self { env, plugin_path }
    }

    pub fn with_env<K, V>(mut self, k: K, v: V) -> Self
    where
        K: Into<OsString>,
        V: Into<OsString>,
    {
        self.env.insert(k.into(), v.into());
        self
    }

    pub fn prepend_path(&mut self, path: PathBuf) {
        let k: OsString = "PATH".into();
        let mut paths = env::split_paths(&self.env[&k]).collect::<Vec<_>>();
        paths.insert(0, path);
        self.env
            .insert("PATH".into(), env::join_paths(paths).unwrap());
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

    pub fn cmd(&self, script: &Script) -> Expression {
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
        let settings = &Settings::get();
        if !settings.raw {
            // ignore stdin, otherwise a prompt may show up where the user won't see it
            cmd = cmd.stdin_null();
        }
        cmd
    }

    pub fn read(&self, script: &Script) -> Result<String> {
        let mut cmd = self.cmd(script);
        let settings = &Settings::try_get()?;
        if !settings.verbose {
            cmd = cmd.stderr_null();
        }
        cmd.read()
            .wrap_err_with(|| ScriptFailed(display_path(self.get_script_path(script)), None))
    }

    pub fn run_by_line(&self, script: &Script, pr: &dyn SingleReport) -> Result<()> {
        let path = self.get_script_path(script);
        pr.set_message(display_path(&path));
        let cmd = CmdLineRunner::new(path.clone())
            .with_pr(pr)
            .env_clear()
            .envs(&self.env);
        if let Err(e) = cmd.execute() {
            let status = match e.downcast_ref::<Error>() {
                Some(ScriptFailed(_, status)) => *status,
                _ => None,
            };
            return Err(ScriptFailed(display_path(&path), status).into());
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
