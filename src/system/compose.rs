use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use eyre::{Result, bail, eyre};
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::system::resources::{ResourceAction, ResourceId, ResourcePlan};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposeState {
    #[default]
    Running,
    Stopped,
    Absent,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposePullPolicy {
    Always,
    #[default]
    Missing,
    Never,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposeBuildPolicy {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposeRecreatePolicy {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposeRemoveImages {
    Local,
    All,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ComposeTomlConfig {
    pub project_dir: PathBuf,
    #[serde(default)]
    pub files: Vec<PathBuf>,
    #[serde(default)]
    pub env_files: Vec<PathBuf>,
    #[serde(default)]
    pub project_name: Option<String>,
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default)]
    pub oneshot: Vec<String>,
    #[serde(default)]
    pub state: ComposeState,
    #[serde(default)]
    pub pull: ComposePullPolicy,
    #[serde(default)]
    pub build: ComposeBuildPolicy,
    #[serde(default)]
    pub recreate: ComposeRecreatePolicy,
    #[serde(default = "default_true")]
    pub wait: bool,
    #[serde(default)]
    pub wait_timeout: Option<u64>,
    #[serde(default)]
    pub timeout: Option<u64>,
    #[serde(default = "default_true")]
    pub remove_orphans: bool,
    #[serde(default)]
    pub renew_anonymous_volumes: bool,
    #[serde(default)]
    pub down_volumes: bool,
    #[serde(default)]
    pub down_images: Option<ComposeRemoveImages>,
    #[serde(default)]
    pub sudo: bool,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub engine_command: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ComposeRequest {
    pub name: String,
    project_dir: PathBuf,
    files: Vec<PathBuf>,
    env_files: Vec<PathBuf>,
    project_name: Option<String>,
    profiles: Vec<String>,
    services: Vec<String>,
    oneshot: IndexSet<String>,
    state: ComposeState,
    pull: ComposePullPolicy,
    build: ComposeBuildPolicy,
    recreate: ComposeRecreatePolicy,
    wait: bool,
    wait_timeout: Option<u64>,
    timeout: Option<u64>,
    remove_orphans: bool,
    renew_anonymous_volumes: bool,
    down_volumes: bool,
    down_images: Option<ComposeRemoveImages>,
    sudo: bool,
    command: Vec<String>,
    engine_command: Vec<String>,
    explicit_dependencies: Vec<ResourceId>,
    path_dependencies: Vec<ResourceId>,
    inspection: Option<ComposeInspection>,
}

#[derive(Clone, Debug)]
enum ComposeInspection {
    Unavailable(String),
    Present {
        configured_services: IndexSet<String>,
        target_services: IndexSet<String>,
        containers: Vec<ComposeContainer>,
        config_hashes: HashMap<String, String>,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ComposeContainer {
    #[serde(rename = "ID")]
    id: String,
    service: String,
    state: String,
    #[serde(default)]
    health: String,
    #[serde(default)]
    exit_code: i64,
    #[serde(skip)]
    config_hash: Option<String>,
}

pub fn prepare_requests_from_config(config: &Config) -> Result<Vec<ComposeRequest>> {
    let mut merged = IndexMap::new();
    for cf in config.config_files.values() {
        if let Some(bootstrap) = cf.bootstrap_config() {
            for (name, project) in bootstrap.compose {
                merged.entry(name).or_insert(project);
            }
        }
    }
    merged
        .into_iter()
        .map(|(name, config)| ComposeRequest::from_toml(name, config))
        .collect()
}

pub fn requests_from_config(config: &Config) -> Result<Vec<ComposeRequest>> {
    let mut requests = prepare_requests_from_config(config)?;
    inspect_requests(&mut requests);
    Ok(requests)
}

pub fn inspect_requests(requests: &mut [ComposeRequest]) {
    for request in requests {
        request.inspection = Some(
            request
                .inspect()
                .unwrap_or_else(|error| ComposeInspection::Unavailable(error.to_string())),
        );
    }
}

pub fn plans(requests: &[ComposeRequest], dependency_changed: bool) -> Vec<ResourcePlan> {
    requests
        .iter()
        .map(|request| request.plan_with_dependency_change(dependency_changed))
        .collect()
}

pub fn apply(requests: &[ComposeRequest], dry_run: bool, yes: bool) -> Result<()> {
    apply_with_dry_run_actions(requests, &HashMap::new(), dry_run, yes)
}

pub fn apply_with_dry_run_actions(
    requests: &[ComposeRequest],
    dry_run_actions: &HashMap<String, ResourceAction>,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    let mut changes = vec![];
    for request in requests {
        let action = apply_action(request, dry_run_actions, dry_run);
        match action {
            ResourceAction::Unknown => bail!(
                "refusing unsafe change to bootstrap compose project '{}'; inspect `mise bootstrap plan`",
                request.name
            ),
            ResourceAction::Noop => {}
            ResourceAction::Create | ResourceAction::Update | ResourceAction::Remove => {
                changes.push(request)
            }
        }
    }
    if changes.is_empty() {
        info!("compose projects: already converged");
        return Ok(());
    }
    if dry_run {
        for request in changes {
            for argv in request.action_argvs()? {
                miseprintln!("would run {}", shell_words::join(argv));
            }
        }
        return Ok(());
    }
    if !yes
        && console::user_attended_stderr()
        && !crate::ui::prompt::confirm(format!(
            "compose projects: apply {} change(s)?",
            changes.len()
        ))?
    {
        info!("compose projects: skipped");
        return Ok(());
    }
    for request in changes {
        request.run_action()?;
    }
    info!("compose projects: applied changes");
    Ok(())
}

fn apply_action(
    request: &ComposeRequest,
    dry_run_actions: &HashMap<String, ResourceAction>,
    dry_run: bool,
) -> ResourceAction {
    let action = request.plan().action;
    if !dry_run {
        return action;
    }
    dry_run_actions
        .get(&request.name)
        .copied()
        .filter(|action| {
            matches!(
                action,
                ResourceAction::Create | ResourceAction::Update | ResourceAction::Remove
            )
        })
        .unwrap_or(action)
}

impl ComposeRequest {
    fn from_toml(name: String, config: ComposeTomlConfig) -> Result<Self> {
        if name.is_empty() {
            bail!("bootstrap compose project names cannot be empty");
        }
        if !config.project_dir.is_absolute() {
            bail!(
                "bootstrap compose project '{name}' project_dir must be absolute: {}",
                config.project_dir.display()
            );
        }
        validate_project_name(config.project_name.as_deref())?;
        validate_values("profile", &config.profiles)?;
        validate_values("service", &config.services)?;
        validate_values("oneshot service", &config.oneshot)?;
        validate_command("command", &config.command)?;
        validate_command("engine_command", &config.engine_command)?;
        if config.state == ComposeState::Absent && !config.services.is_empty() {
            bail!(
                "bootstrap compose project '{name}' services cannot be combined with state = \"absent\" because compose down removes the entire project"
            );
        }
        if !config.wait && config.wait_timeout.is_some() {
            bail!("bootstrap compose project '{name}' wait_timeout requires wait = true");
        }
        if config.wait_timeout == Some(0) {
            bail!("bootstrap compose project '{name}' wait_timeout must be greater than zero");
        }
        if config.state != ComposeState::Absent
            && (config.down_volumes || config.down_images.is_some())
        {
            bail!(
                "bootstrap compose project '{name}' down_volumes and down_images require state = \"absent\""
            );
        }
        if config.state != ComposeState::Running && config.renew_anonymous_volumes {
            bail!(
                "bootstrap compose project '{name}' renew_anonymous_volumes requires state = \"running\""
            );
        }
        let explicit_dependencies = config
            .depends_on
            .iter()
            .map(|dependency| parse_dependency(dependency))
            .collect::<Result<Vec<_>>>()?;
        let services = dedupe(config.services);
        let oneshot: IndexSet<String> = dedupe(config.oneshot).into_iter().collect();
        if !services.is_empty() && oneshot.iter().any(|service| !services.contains(service)) {
            bail!(
                "bootstrap compose project '{name}' oneshot services must also appear in services"
            );
        }
        let files = resolve_paths(&config.project_dir, config.files);
        let env_files = resolve_paths(&config.project_dir, config.env_files);
        let path_dependencies = std::iter::once(ResourceId::new(
            "directory",
            config.project_dir.to_string_lossy(),
        ))
        .chain(
            files
                .iter()
                .chain(&env_files)
                .map(|path| ResourceId::new("file", path.to_string_lossy())),
        )
        .collect::<IndexSet<_>>()
        .into_iter()
        .collect();
        Ok(Self {
            name,
            files,
            env_files,
            project_dir: config.project_dir,
            project_name: config.project_name,
            profiles: dedupe(config.profiles),
            services,
            oneshot,
            state: config.state,
            pull: config.pull,
            build: config.build,
            recreate: config.recreate,
            wait: config.wait,
            wait_timeout: config.wait_timeout,
            timeout: config.timeout,
            remove_orphans: config.remove_orphans,
            renew_anonymous_volumes: config.renew_anonymous_volumes,
            down_volumes: config.down_volumes,
            down_images: config.down_images,
            sudo: config.sudo,
            command: config.command,
            engine_command: config.engine_command,
            explicit_dependencies,
            path_dependencies,
            inspection: None,
        })
    }

    pub fn explicit_dependencies(&self) -> &[ResourceId] {
        &self.explicit_dependencies
    }

    pub fn path_dependencies(&self) -> &[ResourceId] {
        &self.path_dependencies
    }

    pub fn plan(&self) -> ResourcePlan {
        let id = ResourceId::new("compose", &self.name);
        let desired = self.desired();
        let Some(inspection) = &self.inspection else {
            return ResourcePlan::new(id, "not inspected", desired, ResourceAction::Unknown);
        };
        match inspection {
            ComposeInspection::Unavailable(reason) => ResourcePlan::new(
                id,
                format!("unavailable: {reason}"),
                desired,
                ResourceAction::Unknown,
            ),
            ComposeInspection::Present {
                configured_services,
                target_services,
                containers,
                config_hashes,
            } => {
                let selected = containers
                    .iter()
                    .filter(|container| target_services.contains(&container.service))
                    .collect::<Vec<_>>();
                let action = match self.state {
                    ComposeState::Running if selected.is_empty() => ResourceAction::Create,
                    ComposeState::Running
                        if self.running_is_converged(
                            configured_services,
                            target_services,
                            containers,
                            config_hashes,
                        ) =>
                    {
                        ResourceAction::Noop
                    }
                    ComposeState::Running => ResourceAction::Update,
                    ComposeState::Stopped
                        if selected
                            .iter()
                            .any(|container| container_is_active(container))
                            || (self.remove_orphans
                                && containers.iter().any(|container| {
                                    !configured_services.contains(&container.service)
                                })) =>
                    {
                        ResourceAction::Update
                    }
                    ComposeState::Stopped => ResourceAction::Noop,
                    ComposeState::Absent
                        if selected.is_empty()
                            && (containers.is_empty() || !self.remove_orphans) =>
                    {
                        ResourceAction::Noop
                    }
                    ComposeState::Absent => ResourceAction::Remove,
                };
                ResourcePlan::new(
                    id,
                    describe_current(target_services, containers),
                    desired,
                    action,
                )
            }
        }
    }

    pub fn plan_with_dependency_change(&self, dependency_changed: bool) -> ResourcePlan {
        let mut plan = self.plan();
        if dependency_changed && self.state == ComposeState::Running {
            match plan.action {
                ResourceAction::Noop => plan.action = ResourceAction::Update,
                ResourceAction::Unknown => {
                    plan.current = format!("{}; pending bootstrap dependency change", plan.current);
                    plan.action = ResourceAction::Create;
                }
                _ => {}
            }
        }
        plan
    }

    fn inspect(&self) -> Result<ComposeInspection> {
        let configured_services = self
            .compose_output(&["config".to_string(), "--services".to_string()])?
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.trim().to_string())
            .collect::<IndexSet<_>>();
        let target_services = if self.services.is_empty() {
            configured_services.clone()
        } else {
            let missing = self
                .services
                .iter()
                .filter(|service| !configured_services.contains(*service))
                .cloned()
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                bail!(
                    "services not found in compose config: {}",
                    missing.join(", ")
                );
            }
            self.services.iter().cloned().collect()
        };
        let config_hashes = if self.state == ComposeState::Running {
            parse_config_hashes(
                &self.compose_output(&["config".to_string(), "--hash=*".to_string()])?,
            )?
        } else {
            HashMap::new()
        };
        let mut containers = parse_ps(&self.compose_output(&[
            "ps".to_string(),
            "--all".to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])?)?;
        if self.state == ComposeState::Running {
            self.add_container_hashes(&mut containers)?;
        }
        Ok(ComposeInspection::Present {
            configured_services,
            target_services,
            containers,
            config_hashes,
        })
    }

    fn running_is_converged(
        &self,
        configured_services: &IndexSet<String>,
        target_services: &IndexSet<String>,
        containers: &[ComposeContainer],
        config_hashes: &HashMap<String, String>,
    ) -> bool {
        if self.remove_orphans
            && containers
                .iter()
                .any(|container| !configured_services.contains(&container.service))
        {
            return false;
        }
        target_services.iter().all(|service| {
            let service_containers = containers
                .iter()
                .filter(|container| &container.service == service)
                .collect::<Vec<_>>();
            !service_containers.is_empty()
                && service_containers.iter().all(|container| {
                    let runtime_ok = if self.oneshot.contains(service) {
                        (container.state == "exited" && container.exit_code == 0)
                            || container_is_ready(container)
                    } else {
                        container_is_ready(container)
                    };
                    runtime_ok
                        && config_hashes.get(service).is_some_and(|desired_hash| {
                            container.config_hash.as_ref() == Some(desired_hash)
                        })
                })
        })
    }

    fn add_container_hashes(&self, containers: &mut [ComposeContainer]) -> Result<()> {
        if containers.is_empty() {
            return Ok(());
        }
        let ids = containers
            .iter()
            .map(|container| container.id.clone())
            .collect::<Vec<_>>();
        let mut args = vec![
            "inspect".to_string(),
            "--format".to_string(),
            "{{json .Config.Labels}}".to_string(),
        ];
        args.extend(ids);
        let output = self.engine_output(&args)?;
        let lines = output.lines().collect::<Vec<_>>();
        if lines.len() != containers.len() {
            bail!("container inspection returned an unexpected number of rows");
        }
        for (container, line) in containers.iter_mut().zip(lines) {
            let labels: HashMap<String, String> = serde_json::from_str(line)?;
            container.config_hash = labels.get("com.docker.compose.config-hash").cloned();
        }
        Ok(())
    }

    fn desired(&self) -> String {
        match self.state {
            ComposeState::Running => format!(
                "running{}; pull {:?}; build {:?}; recreate {:?}; wait {}",
                selected_suffix(&self.services),
                self.pull,
                self.build,
                self.recreate,
                self.wait
            )
            .to_lowercase(),
            ComposeState::Stopped => format!("stopped{}", selected_suffix(&self.services)),
            ComposeState::Absent => format!("absent{}", selected_suffix(&self.services)),
        }
    }

    fn action_argvs(&self) -> Result<Vec<Vec<String>>> {
        Ok(self
            .action_commands()?
            .into_iter()
            .map(|(program, args)| {
                if self.sudo {
                    crate::system::sudo::argv(&program, &args)
                } else {
                    std::iter::once(program).chain(args).collect()
                }
            })
            .collect())
    }

    fn run_action(&self) -> Result<()> {
        for (program, args) in self.action_commands()? {
            self.run_command(&program, &args)?;
        }
        Ok(())
    }

    fn action_commands(&self) -> Result<Vec<(String, Vec<String>)>> {
        let (program, mut args) = self.compose_command()?;
        args.extend(self.action_args());
        let mut commands = vec![(program, args)];
        if let Some(command) = self.stopped_orphan_removal_command()? {
            commands.push(command);
        }
        Ok(commands)
    }

    fn run_command(&self, program: &str, args: &[String]) -> Result<()> {
        if self.sudo {
            crate::system::sudo::run(program, args, &compose_env())
        } else {
            info!("$ {} {}", program, shell_words::join(args));
            let status = Command::new(program)
                .args(args)
                .envs(compose_env())
                .status()?;
            if !status.success() {
                bail!("compose command failed with {status}");
            }
            Ok(())
        }
    }

    fn stopped_orphan_removal_command(&self) -> Result<Option<(String, Vec<String>)>> {
        if self.state != ComposeState::Stopped || !self.remove_orphans {
            return Ok(None);
        }
        let Some(ComposeInspection::Present {
            configured_services,
            containers,
            ..
        }) = &self.inspection
        else {
            return Ok(None);
        };
        let orphan_ids = containers
            .iter()
            .filter(|container| !configured_services.contains(&container.service))
            .map(|container| container.id.clone())
            .collect::<Vec<_>>();
        if orphan_ids.is_empty() {
            return Ok(None);
        }
        let command = self.resolved_engine_command()?;
        let (program, prefix) = command
            .split_first()
            .ok_or_else(|| eyre!("resolved engine command is empty"))?;
        let mut args = prefix.to_vec();
        args.extend(["rm".to_string(), "--force".to_string()]);
        args.extend(orphan_ids);
        Ok(Some((program.clone(), args)))
    }

    fn action_args(&self) -> Vec<String> {
        let mut args = match self.state {
            ComposeState::Running => {
                let mut args = vec![
                    "up".to_string(),
                    "--detach".to_string(),
                    "--pull".to_string(),
                    match self.pull {
                        ComposePullPolicy::Always => "always",
                        ComposePullPolicy::Missing => "missing",
                        ComposePullPolicy::Never => "never",
                    }
                    .to_string(),
                ];
                match self.build {
                    ComposeBuildPolicy::Auto => {}
                    ComposeBuildPolicy::Always => args.push("--build".to_string()),
                    ComposeBuildPolicy::Never => args.push("--no-build".to_string()),
                }
                match self.recreate {
                    ComposeRecreatePolicy::Auto => {}
                    ComposeRecreatePolicy::Always => args.push("--force-recreate".to_string()),
                    ComposeRecreatePolicy::Never => args.push("--no-recreate".to_string()),
                }
                if self.wait {
                    args.push("--wait".to_string());
                }
                if let Some(timeout) = self.wait_timeout {
                    args.extend(["--wait-timeout".to_string(), timeout.to_string()]);
                }
                if let Some(timeout) = self.timeout {
                    args.extend(["--timeout".to_string(), timeout.to_string()]);
                }
                if self.remove_orphans {
                    args.push("--remove-orphans".to_string());
                }
                if self.renew_anonymous_volumes {
                    args.push("--renew-anon-volumes".to_string());
                }
                args
            }
            ComposeState::Stopped => {
                let mut args = vec!["stop".to_string()];
                if let Some(timeout) = self.timeout {
                    args.extend(["--timeout".to_string(), timeout.to_string()]);
                }
                args
            }
            ComposeState::Absent => {
                let mut args = vec!["down".to_string()];
                if let Some(timeout) = self.timeout {
                    args.extend(["--timeout".to_string(), timeout.to_string()]);
                }
                if self.remove_orphans {
                    args.push("--remove-orphans".to_string());
                }
                if self.down_volumes {
                    args.push("--volumes".to_string());
                }
                if let Some(images) = self.down_images {
                    args.extend([
                        "--rmi".to_string(),
                        match images {
                            ComposeRemoveImages::Local => "local",
                            ComposeRemoveImages::All => "all",
                        }
                        .to_string(),
                    ]);
                }
                args
            }
        };
        // `compose down` is project-scoped and does not accept service names.
        // Validation rejects that ambiguous configuration, but keep command
        // construction defensive in case a request is assembled internally.
        if self.state != ComposeState::Absent {
            args.extend(self.services.iter().cloned());
        }
        args
    }

    fn compose_output(&self, command_args: &[String]) -> Result<String> {
        let (program, mut args) = self.compose_command()?;
        args.extend(command_args.iter().cloned());
        command_output(&program, &args, self.sudo)
    }

    fn engine_output(&self, args: &[String]) -> Result<String> {
        let command = self.resolved_engine_command()?;
        let (program, prefix) = command
            .split_first()
            .ok_or_else(|| eyre!("resolved engine command is empty"))?;
        let mut command_args = prefix.to_vec();
        command_args.extend(args.iter().cloned());
        command_output(program, &command_args, self.sudo)
    }

    fn compose_command(&self) -> Result<(String, Vec<String>)> {
        let command = self.resolved_compose_command()?;
        let (program, prefix) = command
            .split_first()
            .ok_or_else(|| eyre!("resolved compose command is empty"))?;
        let mut args = prefix.to_vec();
        args.extend([
            "--project-directory".to_string(),
            self.project_dir.to_string_lossy().to_string(),
        ]);
        if let Some(project_name) = &self.project_name {
            args.extend(["--project-name".to_string(), project_name.clone()]);
        }
        for file in &self.files {
            args.extend(["--file".to_string(), file.to_string_lossy().to_string()]);
        }
        for env_file in &self.env_files {
            args.extend([
                "--env-file".to_string(),
                env_file.to_string_lossy().to_string(),
            ]);
        }
        for profile in &self.profiles {
            args.extend(["--profile".to_string(), profile.clone()]);
        }
        Ok((program.clone(), args))
    }

    fn resolved_compose_command(&self) -> Result<Vec<String>> {
        if !self.command.is_empty() {
            return resolve_command(&self.command);
        }
        if let Some(docker) = crate::file::which("docker") {
            let docker = docker.to_string_lossy().to_string();
            if compose_plugin_available(&docker) {
                return Ok(vec![docker, "compose".to_string()]);
            }
        }
        if let Some(compose) = crate::file::which("docker-compose") {
            let compose = compose.to_string_lossy().to_string();
            if standalone_compose_v2_available(&compose) {
                return Ok(vec![compose]);
            }
            bail!(
                "legacy 'docker-compose' v1 is unsupported; install Docker Compose v2 or set command to a compatible frontend"
            );
        }
        bail!("neither 'docker compose' nor 'docker-compose' was found")
    }

    fn resolved_engine_command(&self) -> Result<Vec<String>> {
        if !self.engine_command.is_empty() {
            return resolve_command(&self.engine_command);
        }
        if let Some(command) = engine_command_from_compose_command(&self.command) {
            return resolve_command(command);
        }
        if let Some(docker) = crate::file::which("docker") {
            return Ok(vec![docker.to_string_lossy().to_string()]);
        }
        bail!("container engine command 'docker' was not found; set engine_command explicitly")
    }
}

fn compose_plugin_available(docker: &str) -> bool {
    Command::new(docker)
        .args(["compose", "version"])
        .envs(compose_env())
        .output()
        .is_ok_and(|output| output.status.success())
}

fn standalone_compose_v2_available(compose: &str) -> bool {
    Command::new(compose)
        .args(["version", "--short"])
        .envs(compose_env())
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && String::from_utf8(output.stdout)
                    .ok()
                    .is_some_and(|version| is_compose_v2_version(&version))
        })
}

fn is_compose_v2_version(version: &str) -> bool {
    version.split_whitespace().find_map(|part| {
        let version = part.trim_start_matches('v');
        version.split_once('.').map(|(major, _)| major)
    }) == Some("2")
}

fn engine_command_from_compose_command(command: &[String]) -> Option<&[String]> {
    let compose = command.iter().position(|part| part == "compose")?;
    (compose > 0).then_some(&command[..compose])
}

fn command_output(program: &str, args: &[String], sudo: bool) -> Result<String> {
    let env = compose_env();
    let output = if sudo {
        crate::system::sudo::output(program, args, &env)?
    } else {
        info!("$ {} {}", program, shell_words::join(args));
        Command::new(program).args(args).envs(env).output()?
    };
    checked_stdout(output, program, args)
}

fn checked_stdout(output: Output, program: &str, args: &[String]) -> Result<String> {
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(
            "{} {} failed: {}",
            program,
            shell_words::join(args),
            if error.is_empty() {
                output.status.to_string()
            } else {
                error
            }
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn parse_ps(output: &str) -> Result<Vec<ComposeContainer>> {
    let output = output.trim();
    if output.is_empty() {
        return Ok(vec![]);
    }
    if output.starts_with('[') {
        return Ok(serde_json::from_str(output)?);
    }
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| Ok(serde_json::from_str(line)?))
        .collect()
}

fn parse_config_hashes(output: &str) -> Result<HashMap<String, String>> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut fields = line.split_whitespace();
            let service = fields
                .next()
                .ok_or_else(|| eyre!("compose config hash row is missing a service"))?;
            let hash = fields.next().ok_or_else(|| {
                eyre!("compose config hash row for '{service}' is missing a hash")
            })?;
            if fields.next().is_some() {
                bail!("unexpected compose config hash row: {line}");
            }
            Ok((service.to_string(), hash.to_string()))
        })
        .collect()
}

fn container_is_ready(container: &ComposeContainer) -> bool {
    container.state == "running" && matches!(container.health.as_str(), "" | "healthy")
}

fn container_is_active(container: &ComposeContainer) -> bool {
    matches!(
        container.state.as_str(),
        "running" | "restarting" | "paused"
    )
}

fn describe_current(target_services: &IndexSet<String>, containers: &[ComposeContainer]) -> String {
    let selected = containers
        .iter()
        .filter(|container| target_services.contains(&container.service))
        .collect::<Vec<_>>();
    let running = selected
        .iter()
        .filter(|container| container.state == "running")
        .count();
    let healthy = selected
        .iter()
        .filter(|container| container.health == "healthy")
        .count();
    let unhealthy = selected
        .iter()
        .filter(|container| container.health == "unhealthy")
        .count();
    format!(
        "{} container(s); {running} running; {healthy} healthy; {unhealthy} unhealthy",
        selected.len()
    )
}

fn selected_suffix(services: &[String]) -> String {
    if services.is_empty() {
        String::new()
    } else {
        format!(" ({})", services.join(", "))
    }
}

fn resolve_paths(project_dir: &Path, paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                project_dir.join(path)
            }
        })
        .collect()
}

fn resolve_command(command: &[String]) -> Result<Vec<String>> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| eyre!("command cannot be empty"))?;
    let resolved = if Path::new(program).is_absolute() {
        let path = PathBuf::from(program);
        if !path.is_file() {
            bail!("command does not exist: {}", path.display());
        }
        path
    } else {
        crate::file::which(program).ok_or_else(|| eyre!("command not found: {program}"))?
    };
    Ok(std::iter::once(resolved.to_string_lossy().to_string())
        .chain(args.iter().cloned())
        .collect())
}

fn validate_project_name(name: Option<&str>) -> Result<()> {
    let Some(name) = name else {
        return Ok(());
    };
    if name.is_empty()
        || !name
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
        || !name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || "-_".contains(character)
        })
    {
        bail!(
            "invalid compose project_name '{name}': use lowercase letters, digits, dashes, and underscores"
        );
    }
    Ok(())
}

fn parse_dependency(value: &str) -> Result<ResourceId> {
    let Some((kind, name)) = value.split_once(':') else {
        bail!("invalid compose dependency '{value}': expected '<kind>:<name>'");
    };
    if name.is_empty()
        || !matches!(
            kind,
            "package" | "file" | "directory" | "service" | "user" | "group"
        )
    {
        bail!(
            "invalid compose dependency '{value}': supported kinds are package, file, directory, service, user, and group"
        );
    }
    Ok(ResourceId::new(kind, name))
}

fn validate_values(kind: &str, values: &[String]) -> Result<()> {
    if let Some(value) = values
        .iter()
        .find(|value| value.is_empty() || value.starts_with('-') || value.contains('\0'))
    {
        bail!("invalid compose {kind} value '{value}'");
    }
    Ok(())
}

fn validate_command(kind: &str, command: &[String]) -> Result<()> {
    if command
        .iter()
        .any(|part| part.is_empty() || part.contains('\0'))
    {
        bail!("compose {kind} cannot contain empty values or NUL bytes");
    }
    Ok(())
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .collect::<IndexSet<_>>()
        .into_iter()
        .collect()
}

fn compose_env() -> Vec<(String, String)> {
    vec![
        ("COMPOSE_ANSI".to_string(), "never".to_string()),
        ("COMPOSE_PROGRESS".to_string(), "plain".to_string()),
    ]
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(state: ComposeState) -> ComposeRequest {
        ComposeRequest::from_toml(
            "cache".to_string(),
            ComposeTomlConfig {
                project_dir: std::env::temp_dir().join("mise-cache"),
                files: vec![PathBuf::from("compose.yaml")],
                env_files: vec![],
                project_name: Some("mise-cache".to_string()),
                profiles: vec![],
                services: vec![],
                oneshot: vec![],
                state,
                pull: ComposePullPolicy::Missing,
                build: ComposeBuildPolicy::Auto,
                recreate: ComposeRecreatePolicy::Auto,
                wait: true,
                wait_timeout: Some(120),
                timeout: Some(30),
                remove_orphans: true,
                renew_anonymous_volumes: false,
                down_volumes: false,
                down_images: None,
                sudo: true,
                command: vec!["docker".to_string(), "compose".to_string()],
                engine_command: vec!["docker".to_string()],
                depends_on: vec![],
            },
        )
        .unwrap()
    }

    #[test]
    fn parses_json_lines_and_arrays() {
        let line =
            r#"{"ID":"abc","Service":"api","State":"running","Health":"healthy","ExitCode":0}"#;
        assert_eq!(parse_ps(line).unwrap().len(), 1);
        assert_eq!(parse_ps(&format!("[{line}]")).unwrap().len(), 1);
    }

    #[test]
    fn plans_runtime_and_config_hash_drift() {
        let mut request = request(ComposeState::Running);
        let container = ComposeContainer {
            id: "abc".to_string(),
            service: "api".to_string(),
            state: "running".to_string(),
            health: "healthy".to_string(),
            exit_code: 0,
            config_hash: Some("same".to_string()),
        };
        request.inspection = Some(ComposeInspection::Present {
            configured_services: IndexSet::from(["api".to_string()]),
            target_services: IndexSet::from(["api".to_string()]),
            containers: vec![container.clone()],
            config_hashes: HashMap::from([("api".to_string(), "same".to_string())]),
        });
        assert_eq!(request.plan().action, ResourceAction::Noop);
        request.inspection = Some(ComposeInspection::Present {
            configured_services: IndexSet::from(["api".to_string()]),
            target_services: IndexSet::from(["api".to_string()]),
            containers: vec![container],
            config_hashes: HashMap::from([("api".to_string(), "changed".to_string())]),
        });
        assert_eq!(request.plan().action, ResourceAction::Update);
    }

    #[test]
    fn dry_run_uses_dependency_driven_changes_for_converged_projects() {
        let mut request = request(ComposeState::Running);
        request.inspection = Some(ComposeInspection::Present {
            configured_services: IndexSet::from(["api".to_string()]),
            target_services: IndexSet::from(["api".to_string()]),
            containers: vec![ComposeContainer {
                id: "abc".to_string(),
                service: "api".to_string(),
                state: "running".to_string(),
                health: "healthy".to_string(),
                exit_code: 0,
                config_hash: Some("same".to_string()),
            }],
            config_hashes: HashMap::from([("api".to_string(), "same".to_string())]),
        });
        assert_eq!(request.plan().action, ResourceAction::Noop);
        assert_eq!(
            apply_action(
                &request,
                &HashMap::from([("cache".to_string(), ResourceAction::Update)]),
                true,
            ),
            ResourceAction::Update,
        );
    }

    #[test]
    fn removes_orphans_from_absent_projects() {
        let mut request = request(ComposeState::Absent);
        request.inspection = Some(ComposeInspection::Present {
            configured_services: IndexSet::from(["api".to_string()]),
            target_services: IndexSet::from(["api".to_string()]),
            containers: vec![ComposeContainer {
                id: "old".to_string(),
                service: "removed".to_string(),
                state: "running".to_string(),
                health: String::new(),
                exit_code: 0,
                config_hash: None,
            }],
            config_hashes: HashMap::new(),
        });
        assert_eq!(request.plan().action, ResourceAction::Remove);
    }

    #[test]
    fn removes_orphans_from_stopped_projects() {
        let mut request = request(ComposeState::Stopped);
        let executable = std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .to_string();
        request.command = vec![executable.clone(), "compose".to_string()];
        request.engine_command = vec![executable.clone()];
        request.inspection = Some(ComposeInspection::Present {
            configured_services: IndexSet::from(["api".to_string()]),
            target_services: IndexSet::from(["api".to_string()]),
            containers: vec![
                ComposeContainer {
                    id: "api-1".to_string(),
                    service: "api".to_string(),
                    state: "exited".to_string(),
                    health: String::new(),
                    exit_code: 0,
                    config_hash: None,
                },
                ComposeContainer {
                    id: "old-1".to_string(),
                    service: "removed".to_string(),
                    state: "running".to_string(),
                    health: String::new(),
                    exit_code: 0,
                    config_hash: None,
                },
            ],
            config_hashes: HashMap::new(),
        });
        assert_eq!(request.plan().action, ResourceAction::Update);
        let commands = request.action_commands().unwrap();
        assert_eq!(commands[1].0, executable);
        assert_eq!(
            commands[1].1,
            vec!["rm".to_string(), "--force".to_string(), "old-1".to_string()]
        );
    }

    #[test]
    fn builds_full_lifecycle_commands() {
        let running = request(ComposeState::Running);
        let args = running.action_args();
        assert!(args.starts_with(&["up".to_string(), "--detach".to_string()]));
        assert!(args.contains(&"--wait".to_string()));
        assert!(args.contains(&"--remove-orphans".to_string()));

        let mut request = request(ComposeState::Absent);
        request.down_volumes = true;
        request.down_images = Some(ComposeRemoveImages::Local);
        assert_eq!(
            request.action_args(),
            vec![
                "down",
                "--timeout",
                "30",
                "--remove-orphans",
                "--volumes",
                "--rmi",
                "local",
            ]
        );

        request.services = vec!["api".to_string()];
        assert!(!request.action_args().contains(&"api".to_string()));
    }

    #[test]
    fn derives_engine_command_with_global_options() {
        let command = [
            "docker".to_string(),
            "--context".to_string(),
            "remote".to_string(),
            "compose".to_string(),
        ];
        assert_eq!(
            engine_command_from_compose_command(&command),
            Some(&command[..3])
        );
        assert_eq!(engine_command_from_compose_command(&command[..3]), None);
    }

    #[test]
    fn recognizes_only_standalone_compose_v2() {
        assert!(is_compose_v2_version("2.40.3\n"));
        assert!(is_compose_v2_version("v2.40.3\n"));
        assert!(is_compose_v2_version("Docker Compose version v2.40.3"));
        assert!(!is_compose_v2_version("1.29.2\n"));
        assert!(!is_compose_v2_version("Docker Compose version 1.29.2"));
        assert!(!is_compose_v2_version("unknown"));
    }
}
