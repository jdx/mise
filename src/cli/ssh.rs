use eyre::{Result, bail};
use std::path::PathBuf;

/// Open an SSH session, optionally borrowing read-only GitHub access
#[derive(Debug, usage_rs::Args)]
pub(crate) struct Ssh {
    /// OpenSSH destination or SSH-config alias
    destination: Option<String>,
    /// SSH identity file
    #[usage(long, short = 'i')]
    identity_file: Option<PathBuf>,
    /// SSH port
    #[usage(long, short = 'p')]
    port: Option<u16>,
    /// OpenSSH option; repeat for multiple options
    #[usage(long, short = 'o')]
    ssh_option: Vec<String>,
    /// Borrow read-only GitHub access for this session only
    #[usage(long)]
    github_relay_read_only: bool,
    /// Approved GitHub repository; repeat to authorize more repositories
    #[usage(long, value_name = "OWNER/REPO")]
    github_relay_repo: Vec<String>,
    /// Explicitly authorize reads of all repositories accessible locally
    #[usage(long)]
    github_relay_all_repos: bool,
    /// Log sanitized relay requests on local stderr
    #[usage(long, conflicts = "github_relay_no_log_requests")]
    github_relay_log_requests: bool,
    /// Disable request logging, overriding the saved preference
    #[usage(long)]
    github_relay_no_log_requests: bool,
    /// Relay log and summary format: text or jsonl
    #[usage(long, value_name = "FORMAT")]
    github_relay_log_format: Option<String>,
    /// Expire borrowed access after a duration such as 1h (0s: session lifetime)
    #[usage(long, value_name = "DURATION")]
    github_relay_max_duration: Option<String>,
    /// Internal session adapter
    #[usage(long, hide = true)]
    relay_session: Option<PathBuf>,
    #[usage(long, hide = true)]
    repository_bundle: Option<PathBuf>,
    #[usage(long, hide = true)]
    repository_origin: Option<String>,
    #[usage(long, hide = true)]
    repository_revision: Option<String>,
    #[usage(long, hide = true)]
    repository_update: bool,
    #[usage(long, hide = true)]
    repository_yes: bool,
    /// Preview the transferred repository's installation without writing it
    #[usage(long, hide = true)]
    repository_dry_run: bool,
    #[usage(long, hide = true)]
    global_config_directory: bool,
    /// Command to execute after --; omit for an interactive shell
    #[usage(trailing_var_arg = true, allow_hyphen_values = true)]
    command: Vec<String>,
}

impl Ssh {
    pub(crate) async fn run(self) -> Result<()> {
        self.run_inner().await
    }

    async fn run_inner(self) -> Result<()> {
        if self.global_config_directory {
            println!(
                "{}",
                crate::system::remote_repository::global_directory().display()
            );
            return Ok(());
        }
        if let Some(bundle) = self.repository_bundle {
            let origin = self
                .repository_origin
                .ok_or_else(|| eyre::eyre!("missing repository origin"))?;
            let revision = self
                .repository_revision
                .ok_or_else(|| eyre::eyre!("missing repository revision"))?;
            // a history-managed setup repository is set up from, never
            // checked out into the configuration directory
            if let Some(branch) =
                crate::system::remote_repository::history_branch(&bundle, &revision)?
            {
                use crate::system::history::sync::onboard;
                crate::config::Settings::get().ensure_experimental("dotfile tracking")?;
                let store = crate::system::history::checkpoint::Store::open()?;
                if let Some(reason) = store.unavailable() {
                    bail!("cannot set up from a setup repository: {reason}");
                }
                let fetch_from = bundle.to_string_lossy().into_owned();
                onboard::probe(&store, &fetch_from, &branch)?;
                let outcome = onboard::run(
                    &store,
                    &onboard::Onboarding {
                        fetch_from,
                        origin,
                        branch,
                        yes: self.repository_yes,
                        dry_run: self.repository_dry_run,
                    },
                )
                .await?;
                if !self.repository_dry_run
                    && (outcome.configuration_held
                        || !crate::system::history::sync::run::read_status(store.state_dir())?
                            .conflicts
                            .is_empty())
                {
                    bail!(
                        "setup has conflicts; nothing was bootstrapped; use `mise bootstrap dotfiles status` to resolve them"
                    );
                }
                return Ok(());
            }
            crate::system::remote_repository::install(
                &bundle,
                &origin,
                &revision,
                self.repository_update,
                self.repository_yes,
                self.repository_dry_run,
            )?;
            return Ok(());
        }
        let scope = crate::github_relay::Scope::from_flags(
            self.github_relay_read_only,
            &self.github_relay_repo,
            self.github_relay_all_repos,
        )?;
        let scope = crate::github_relay::configure(
            scope,
            self.github_relay_log_requests,
            self.github_relay_no_log_requests,
            self.github_relay_log_format.as_deref(),
            self.github_relay_max_duration.as_deref(),
        )?;
        if let Some(socket) = self.relay_session {
            if scope.is_some() {
                bail!("cannot nest relay sessions");
            }
            #[cfg(unix)]
            {
                crate::ui::ctrlc::exit_on_ctrl_c(false);
                let mut command = self.command;
                if let Some(first) = self.destination {
                    command.insert(0, first);
                }
                return crate::system::remote::interruptible(crate::github_relay::unix::session(
                    &socket, command,
                ))
                .await;
            }
            #[cfg(not(unix))]
            bail!("GitHub relay requires a POSIX target: {}", socket.display());
        }
        let destination = self
            .destination
            .ok_or_else(|| eyre::eyre!("an SSH destination is required"))?;
        if let Some(scope) = scope {
            #[cfg(unix)]
            {
                let mut host = crate::system::remote::ad_hoc_host(
                    &destination,
                    std::env::current_dir()?,
                    &[],
                )?;
                host.port = self.port;
                host.identity_file = self.identity_file;
                host.ssh_options = self.ssh_option;
                return crate::system::remote::ssh(&host, scope, &self.command).await;
            }
            #[cfg(not(unix))]
            {
                let _ = scope;
                bail!("GitHub relay requires Linux or macOS");
            }
        }
        let mut command = tokio::process::Command::new("ssh");
        command.kill_on_drop(true);
        if let Some(port) = self.port {
            command.args(["-p", &port.to_string()]);
        }
        if let Some(identity) = self.identity_file {
            command.arg("-i").arg(identity);
        }
        for option in self.ssh_option {
            command.args(["-o", &option]);
        }
        command.arg("--").arg(destination);
        if !self.command.is_empty() {
            command.arg(shell_words::join(&self.command));
        }
        crate::ui::ctrlc::exit_on_ctrl_c(false);
        let status = crate::system::remote::interruptible(async {
            #[cfg(unix)]
            {
                crate::github_relay::unix::wait_command(&mut command, None).await
            }
            #[cfg(not(unix))]
            {
                Ok(command.status().await?)
            }
        })
        .await?;
        Err(crate::request_exit(status.code().unwrap_or(255)))
    }
}

#[cfg(test)]
mod tests {
    use crate::cli::{Cli, Commands};
    use std::ffi::OsStr;
    #[test]
    fn parses_destination_options_and_command() {
        let argv = [
            "mise",
            "ssh",
            "devbox",
            "--github-relay-read-only",
            "--github-relay-repo",
            "jdx/mise",
            "--github-relay-log-requests",
            "--github-relay-log-format=jsonl",
            "--github-relay-max-duration=1h",
            "-p",
            "2222",
            "-o",
            "ServerAliveInterval=10",
            "--",
            "git",
            "fetch",
            "--all",
        ]
        .map(OsStr::new);
        let Some(Commands::Ssh(args)) = Cli::parse_from_argv(&argv).unwrap().command else {
            panic!("expected ssh")
        };
        assert_eq!(args.destination.as_deref(), Some("devbox"));
        assert!(args.github_relay_read_only);
        assert!(args.github_relay_log_requests);
        assert_eq!(args.github_relay_log_format.as_deref(), Some("jsonl"));
        assert_eq!(args.github_relay_max_duration.as_deref(), Some("1h"));
        assert_eq!(args.github_relay_repo, ["jdx/mise"]);
        assert_eq!(args.command, ["git", "fetch", "--all"]);
        assert_eq!(args.port, Some(2222));
    }
    #[test]
    fn parses_internal_session_command_without_destination() {
        let argv = [
            "mise",
            "ssh",
            "--relay-session",
            "/tmp/socket",
            "--",
            "mise",
            "--version",
        ]
        .map(OsStr::new);
        let Some(Commands::Ssh(args)) = Cli::parse_from_argv(&argv).unwrap().command else {
            panic!("expected ssh")
        };
        let mut command = args.command;
        if let Some(first) = args.destination {
            command.insert(0, first);
        }
        assert_eq!(command, ["mise", "--version"]);
    }
}
