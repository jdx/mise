use std::collections::HashMap;

use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::settings::MissingRuntimeBehavior::Ignore;
use crate::config::Config;
use crate::env_diff::{EnvDiff, EnvDiffOperation, EnvDiffPatches};
use crate::hook_env::HookEnvWatches;
use crate::output::Output;
use crate::shell::{get_shell, ShellType};
use crate::{dirs, env, hook_env};

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

        self.clear_old_env(out);
        let mut env: HashMap<String, String> = config
            .env()?
            .iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        env.insert("PATH".into(), config.path_env()?.into());
        env.insert("__RTX_DIR".into(), dirs::CURRENT.to_string_lossy().into());
        let diff = EnvDiff::new(&env::PRISTINE_ENV, &env);
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

    fn clear_old_env(&self, out: &mut Output) {
        let patches = env::__RTX_DIFF.reverse().to_patches();
        let output = self.build_env_commands(&patches);
        out.stdout.write(output);
    }
}

#[cfg(test)]
mod test {
    use crate::assert_cli;

    #[test]
    fn test_hook_env() {
        assert_cli!("hook-env", "-s", "fish");
    }
}
