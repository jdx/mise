use std::collections::HashMap;
use std::env;

use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::settings::MissingRuntimeBehavior::Ignore;
use crate::config::Config;
use crate::env_diff::{EnvDiff, EnvDiffOperation, EnvDiffPatches};
use crate::hook_env::HookEnvWatches;
use crate::output::Output;
use crate::shell::{get_shell, ShellType};
use crate::{dirs, hook_env};

/// [internal] called by activate hook to update env vars directory change
#[derive(Debug, clap::Args)]
#[clap(hide = true)]
pub struct HookEnv {
    /// Shell type to generate script for
    ///
    /// e.g.: bash, zsh, fish
    #[clap(long, short)]
    shell: Option<ShellType>,
}

impl Command for HookEnv {
    fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        config.settings.missing_runtime_behavior = Ignore;
        config.ensure_installed()?;

        let current_env = self.clear_old_env(env::vars().collect(), out)?;
        let mut env: HashMap<String, String> = config
            .env()?
            .iter()
            .map(|(k, v)| (k.to_string_lossy().into(), v.to_string_lossy().into()))
            .collect();
        env.insert("__RTX_DIR".into(), dirs::CURRENT.to_string_lossy().into());
        let diff = EnvDiff::new(&current_env, &env);
        let mut patches = diff.to_patches();
        patches.push(EnvDiffOperation::Add(
            "__RTX_DIFF".into(),
            diff.serialize()?,
        ));

        patches.push(EnvDiffOperation::Add(
            "__RTX_WATCH".into(),
            hook_env::serialize_watches(&get_watches(&config)?)?,
        ));
        let output = self.build_env_commands(&patches);
        out.stdout.write(output);

        Ok(())
    }
}

fn get_watches(config: &Config) -> Result<HookEnvWatches> {
    let mut watches = HookEnvWatches::new();
    for cf in &config.config_files {
        watches.insert(cf.clone(), cf.metadata()?.modified()?);
    }

    Ok(watches)
}

impl HookEnv {
    fn build_env_commands(&self, patches: &EnvDiffPatches) -> String {
        let shell = get_shell(self.shell);
        let mut output = String::new();

        for patch in patches.iter() {
            match patch {
                EnvDiffOperation::Add(k, v) | EnvDiffOperation::Change(k, v) => {
                    output.push_str(&shell.set_env(k, v));
                }
                EnvDiffOperation::Remove(k) => {
                    output.push_str(&shell.unset_env(k));
                }
            }
        }

        output
    }

    fn clear_old_env(
        &self,
        env: HashMap<String, String>,
        out: &mut Output,
    ) -> Result<HashMap<String, String>> {
        let patches = get_env_diff(&env)?.reverse().to_patches();
        let output = self.build_env_commands(&patches);
        out.stdout.write(output);

        Ok(apply_patches(&env, &patches))
    }
}

fn get_env_diff(env: &HashMap<String, String>) -> Result<EnvDiff> {
    let json = env.get("__RTX_DIFF").cloned();
    match json {
        Some(json) => Ok(EnvDiff::deserialize(&json)?),
        None => Ok(EnvDiff::default()),
    }
}

fn apply_patches(
    env: &HashMap<String, String>,
    patches: &EnvDiffPatches,
) -> HashMap<String, String> {
    let mut new_env = env.clone();
    for patch in patches {
        match patch {
            EnvDiffOperation::Add(k, v) | EnvDiffOperation::Change(k, v) => {
                new_env.insert(k.into(), v.into());
            }
            EnvDiffOperation::Remove(k) => {
                new_env.remove(k);
            }
        }
    }

    new_env
}

#[cfg(test)]
mod test {
    use crate::assert_cli;

    #[test]
    fn test_hook_env() {
        assert_cli!("hook-env", "-s", "fish");
    }
}
