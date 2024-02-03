use assert_cmd::assert::Assert;
use assert_cmd::Command;
use eyre::Result;
use once_cell::sync::Lazy;
use predicates::prelude::*;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::{env, fs};
use tempfile::TempDir;

use walkdir::WalkDir;

pub static CONFIGS: Lazy<Map> = Lazy::new(|| {
    let config_dir = &Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cli")
        .join("data")
        .join("configs");

    Map(WalkDir::new(config_dir)
        .into_iter()
        .map(Result::unwrap)
        .filter_map(|e| match e.metadata() {
            Ok(m) if m.is_file() => {
                let name = e.path().strip_prefix(config_dir).unwrap();
                let file = File {
                    path: name.into(),
                    content: fs::read_to_string(e.path()).unwrap(),
                };

                Some((name.to_string_lossy().to_string(), file))
            }
            _ => None,
        })
        .collect())
});

pub struct Map(HashMap<String, File>);

impl Map {
    pub fn get(&self, k: &str) -> File {
        self.0.get(k).unwrap().clone()
    }
}

macro_rules! mise {
    ($($givens:expr),+; $($asserts:expr),+ $(,)?) => {
        let mut builder = crate::cli::prelude::EnvironmentBuilder::new();

        for given in [$($givens),+] {
            builder = given(builder);
        }

        let env = builder.build()?;

        let asserts: Vec<Box<dyn FnOnce(crate::cli::prelude::CommandBuilder) -> Result<()>>> = vec![$($asserts),+];

        for assert in asserts{
            assert(env.mise())?;
        }

        env.teardown()
    };
    ($($asserts:expr),+ $(,)?) => {
        let env = crate::cli::prelude::EnvironmentBuilder::new().build()?;

        let asserts: Vec<Box<dyn FnOnce(crate::cli::prelude::CommandBuilder) -> Result<()>>> = vec![$($asserts),+];

        for assert in asserts{
            assert(env.mise())?;
        }

        env.teardown()
    };
}
pub(crate) use mise;

macro_rules! given_environment {
    (has_home_files $($files:expr),+ $(,)?) => {
        |env: crate::cli::prelude::EnvironmentBuilder| {
            env.with_home_files([$($files),+])
        }
    };
    (has_root_files $($files:expr),+ $(,)?) => {
        |env: crate::cli::prelude::EnvironmentBuilder| {
            env.with_root_files([$($files),+])
        }
    };
    (has_exported_var $key:expr, $val:expr) => {
        |env: crate::cli::prelude::EnvironmentBuilder| env.with_exported_var($key, $val)
    };
}
pub(crate) use given_environment;

macro_rules! when {
    ($($givens:expr),+; $($expects:expr),+ $(,)?) => {
        Box::new(|mut cmd: crate::cli::prelude::CommandBuilder| -> Result<()> {
            let ctx = crate::cli::prelude::CommandContext{
                data_path: cmd.data_path(),
                home_path: cmd.home_path(),
                root_path: cmd.root_path(),
            };

            for given in [$($givens),+] {
                cmd = given(&ctx, cmd);
            }

            let mut res = cmd.run()?;

            let expects: Vec<Box<dyn FnOnce(&crate::cli::prelude::CommandContext, assert_cmd::assert::Assert) -> assert_cmd::assert::Assert>> = vec![$($expects),+];
            for expect in expects {
                res = expect(&ctx, res)
            }

            Ok(())
        })
    };
}
pub(crate) use when;

macro_rules! given {
    (args $($args:expr),+ $(,)?) => {
        |ctx: &crate::cli::prelude::CommandContext, cmd: crate::cli::prelude::CommandBuilder| {
            let args: Vec<String> = [$($args),+].into_iter().map(|a| ctx.substitute(a)).collect();

            cmd.args(args)
        }
    };
    (env_var $key:expr, $val:expr) => {
        |_: &crate::cli::prelude::CommandContext, cmd: crate::cli::prelude::CommandBuilder| cmd.env($key, $val)
    };
}
pub(crate) use given;

macro_rules! should {
    (output $val:expr) => {
        Box::new(
            |ctx: &crate::cli::prelude::CommandContext, assert: assert_cmd::assert::Assert| {
                assert.stdout(predicates::prelude::predicate::str::contains(
                    ctx.substitute($val),
                ))
            },
        )
    };
    (not_output $val:expr) => {
        Box::new(
            |ctx: &crate::cli::prelude::CommandContext, assert: assert_cmd::assert::Assert| {
                assert.stdout(crate::cli::prelude::not_contains(&ctx.substitute($val)))
            },
        )
    };
    (output_exactly $val:expr) => {
        Box::new(
            |ctx: &crate::cli::prelude::CommandContext, assert: assert_cmd::assert::Assert| {
                assert.stdout(predicates::prelude::predicate::eq(ctx.substitute($val)))
            },
        )
    };
    (not_output_exactly $val:expr) => {
        Box::new(
            |ctx: &crate::cli::prelude::CommandContext, assert: assert_cmd::assert::Assert| {
                assert.stdout(predicates::prelude::predicate::ne(ctx.substitute($val)))
            },
        )
    };
    (output_error $val:expr) => {
        Box::new(
            |ctx: &crate::cli::prelude::CommandContext, assert: assert_cmd::assert::Assert| {
                assert.stderr(predicates::prelude::predicate::str::contains(
                    ctx.substitute($val),
                ))
            },
        )
    };
    (output_error_exactly $val:expr) => {
        Box::new(
            |ctx: &crate::cli::prelude::CommandContext, assert: assert_cmd::assert::Assert| {
                assert.stderr(predicates::prelude::predicate::eq(ctx.substitute($val)))
            },
        )
    };
    (succeed) => {
        Box::new(
            |_: &crate::cli::prelude::CommandContext, assert: assert_cmd::assert::Assert| {
                assert.success()
            },
        )
    };
    (fail) => {
        Box::new(
            |_: &crate::cli::prelude::CommandContext, assert: assert_cmd::assert::Assert| {
                assert.failure()
            },
        )
    };
}
pub(crate) use should;

pub fn not_contains(val: &str) -> impl Predicate<str> {
    predicate::str::contains(val).not()
}

pub struct CommandContext {
    pub data_path: PathBuf,
    pub home_path: PathBuf,
    pub root_path: PathBuf,
}

impl CommandContext {
    pub fn substitute(&self, val: impl Into<String>) -> String {
        val.into()
            .replace("$DATA", self.data_path.to_str().unwrap())
            .replace("$HOME", self.home_path.to_str().unwrap())
            .replace("$ROOT", self.root_path.to_str().unwrap())
    }
}

pub struct EnvironmentBuilder {
    data_files: Vec<File>,
    home_files: Vec<File>,
    root_files: Vec<File>,
    exports: HashMap<String, OsString>,
}

impl Default for EnvironmentBuilder {
    fn default() -> Self {
        Self {
            data_files: vec![],
            home_files: vec![],
            root_files: vec![CONFIGS.get(".mise.toml")],
            exports: HashMap::new(),
        }
    }
}

impl EnvironmentBuilder {
    pub fn new() -> Self {
        Self {
            data_files: vec![],
            home_files: vec![],
            root_files: vec![],
            exports: HashMap::new(),
        }
    }

    // pub fn with_data_files(mut self, files: impl IntoIterator<Item = File>) -> Self {
    //     self.data_files.extend(files);

    //     self
    // }

    pub fn with_home_files(mut self, files: impl IntoIterator<Item = File>) -> Self {
        self.home_files.extend(files);

        self
    }

    pub fn with_root_files(mut self, files: impl IntoIterator<Item = File>) -> Self {
        self.root_files.extend(files);

        self
    }

    pub fn with_exported_var(mut self, key: &str, val: impl AsRef<OsStr>) -> Self {
        self.exports.insert(key.into(), val.as_ref().into());

        self
    }

    pub fn build(self) -> Result<Environment> {
        let env = Environment {
            cache: Dir::from_env("INTEGRATION_CACHE_DIR")?,
            data: Dir::from_env("INTEGRATION_DATA_DIR")?,
            home: Dir::from_env("INTEGRATION_HOME_DIR")?,
            root: Dir::from_env("INTEGRATION_ROOT_DIR")?,
            exports: self.exports,
        };

        env.data.write_files(self.data_files)?;
        env.home.write_files(self.home_files)?;
        env.root.write_files(self.root_files)?;

        Ok(env)
    }
}

enum Dir {
    Remote(PathBuf),
    Temp(TempDir),
}

impl Dir {
    pub fn from_env(var: impl AsRef<OsStr>) -> Result<Self> {
        Ok(match env::var_os(var) {
            Some(dir) if PathBuf::from(&dir).exists() => Self::Remote(PathBuf::from(&dir)),
            _ => Self::Temp(TempDir::new()?),
        })
    }

    pub fn path(&self) -> &Path {
        match self {
            Self::Remote(p) => p.as_path(),
            Self::Temp(t) => t.path(),
        }
    }

    pub fn write_files(&self, fs: impl IntoIterator<Item = File>) -> Result<()> {
        fs.into_iter().try_for_each(|f| self.write_file(&f))
    }

    pub fn write_file(&self, f: &File) -> Result<()> {
        let path = self.path().join(&f.path);

        fs::create_dir_all(path.parent().unwrap())?;

        Ok(fs::write(path, &f.content)?)
    }

    #[allow(clippy::permissions_set_readonly_false)]
    pub fn cleanup(self) -> Result<()> {
        if let Self::Temp(t) = self {
            // force remove temp contents in case of readonly files/dirs
            for ent in walkdir::WalkDir::new(t.path()).into_iter().flatten() {
                let mut perms = ent.metadata()?.permissions();
                perms.set_readonly(false);

                let _ = fs::set_permissions(ent.path(), perms);
            }

            t.close()?
        };
        Ok(())
    }
}

pub struct Environment {
    cache: Dir,
    data: Dir,
    home: Dir,
    root: Dir,
    exports: HashMap<String, OsString>,
}

impl Environment {
    pub fn cache_path(&self) -> &Path {
        self.cache.path()
    }

    pub fn config_path(&self) -> PathBuf {
        self.home.path().join(".config/mise")
    }

    pub fn data_path(&self) -> &Path {
        self.data.path()
    }

    pub fn home_path(&self) -> &Path {
        self.home.path()
    }

    pub fn root_path(&self) -> &Path {
        self.root.path()
    }

    pub fn mise(&self) -> CommandBuilder {
        CommandBuilder {
            args: vec![],
            env: self.exports.clone(),
            unset_env: vec![],
            cache_path: self.cache_path().into(),
            config_path: self.config_path(),
            data_path: self.data_path().into(),
            home_path: self.home_path().into(),
            root_path: self.root_path().into(),
        }
    }

    pub fn teardown(self) -> Result<()> {
        [self.data, self.home, self.root]
            .map(|d| d.cleanup())
            .into_iter()
            .collect()
    }
}

pub struct CommandBuilder {
    args: Vec<OsString>,
    env: HashMap<String, OsString>,
    unset_env: Vec<OsString>,
    cache_path: PathBuf,
    config_path: PathBuf,
    data_path: PathBuf,
    home_path: PathBuf,
    root_path: PathBuf,
}

impl CommandBuilder {
    pub fn args(self, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Self {
        Self {
            args: args.into_iter().map(|a| a.as_ref().into()).collect(),
            ..self
        }
    }

    pub fn env(mut self, key: &str, val: impl AsRef<OsStr>) -> Self {
        self.env.insert(key.into(), val.as_ref().into());

        self
    }

    pub fn unset_env(mut self, key: impl AsRef<OsStr>) -> Self {
        self.unset_env.push(key.as_ref().into());

        self
    }

    pub fn run(self) -> Result<Assert> {
        let global_config_path = self.config_path.join("config.toml");
        let trusted_paths = [
            self.root_path.to_string_lossy(),
            self.config_path.to_string_lossy(),
        ]
        .join(":");

        let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME"))?;
        cmd.current_dir(&self.root_path)
            .env_clear()
            .env("PATH", env::var_os("PATH").unwrap())
            .env("HOME", self.home_path)
            .env("MISE_USE_TOML", "0")
            .env("MISE_DATA_DIR", &self.data_path)
            .env("MISE_STATE_DIR", self.data_path)
            .env("MISE_CACHE_DIR", self.cache_path)
            .env("MISE_CONFIG_DIR", self.config_path)
            .env("MISE_GLOBAL_CONFIG_FILE", global_config_path)
            .env("MISE_ALWAYS_KEEP_DOWNLOAD", "1")
            .env("MISE_TRUSTED_CONFIG_PATHS", trusted_paths)
            .env("MISE_YES", "1")
            .env("NPM_CONFIG_FUND", "false")
            .args(self.args);

        for key in self.unset_env {
            cmd.env_remove(key);
        }

        Ok(cmd.envs(self.env).assert())
    }

    pub fn data_path(&self) -> PathBuf {
        self.data_path.clone()
    }

    pub fn home_path(&self) -> PathBuf {
        self.home_path.clone()
    }

    pub fn root_path(&self) -> PathBuf {
        self.root_path.clone()
    }
}

#[derive(Clone)]
pub struct File {
    pub path: PathBuf,
    pub content: String,
}
