use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::net::IpAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use eyre::{Context, Result, bail, eyre};
use indexmap::IndexMap;
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::system::resources::{ResourceAction, ResourceId, ResourcePlan};

const STATE_PATH: &str = "/var/lib/mise/bootstrap/firewall.json";
const NFT_TABLE: &str = "mise_bootstrap";
const NFT_RULES_PATH: &str = "/etc/mise/bootstrap/firewall.nft";
const NFT_UNIT_PATH: &str = "/etc/systemd/system/mise-bootstrap-firewall.service";
const FIREWALLD_INCOMING: &str = "mise-bootstrap-in";
const FIREWALLD_OUTGOING: &str = "mise-bootstrap-out";
const FIREWALLD_POLICY_DIR: &str = "/etc/firewalld/policies";

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FirewallBackend {
    #[default]
    Auto,
    Ufw,
    Firewalld,
    Nftables,
}

impl FirewallBackend {
    fn program(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Ufw => Some("ufw"),
            Self::Firewalld => Some("firewall-cmd"),
            Self::Nftables => Some("nft"),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Ufw => "ufw",
            Self::Firewalld => "firewalld",
            Self::Nftables => "nftables",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FirewallState {
    #[default]
    Enabled,
    Disabled,
    Absent,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FirewallPolicy {
    #[default]
    Allow,
    Deny,
    Reject,
}

impl FirewallPolicy {
    fn nft(self) -> &'static str {
        match self {
            Self::Allow => "accept",
            Self::Deny | Self::Reject => "drop",
        }
    }

    fn firewalld(self) -> &'static str {
        match self {
            Self::Allow => "ACCEPT",
            Self::Deny => "DROP",
            Self::Reject => "REJECT",
        }
    }

    fn ufw(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Reject => "reject",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FirewallRuleState {
    #[default]
    Present,
    Absent,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FirewallDirection {
    #[default]
    Incoming,
    Outgoing,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FirewallAction {
    #[default]
    Allow,
    Deny,
    Reject,
}

impl FirewallAction {
    fn ufw(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Reject => "reject",
        }
    }

    fn nft(self) -> &'static str {
        match self {
            Self::Allow => "accept",
            Self::Deny => "drop",
            Self::Reject => "reject",
        }
    }

    fn firewalld(self) -> &'static str {
        match self {
            Self::Allow => "accept",
            Self::Deny => "drop",
            Self::Reject => "reject",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FirewallProtocol {
    Tcp,
    Udp,
    Sctp,
    Dccp,
}

impl FirewallProtocol {
    fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
            Self::Sctp => "sctp",
            Self::Dccp => "dccp",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum FirewallPortToml {
    Single(u16),
    Range(String),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FirewallPort {
    start: u16,
    end: u16,
}

impl FirewallPort {
    fn from_toml(value: FirewallPortToml) -> Result<Self> {
        let (start, end) = match value {
            FirewallPortToml::Single(port) => (port, port),
            FirewallPortToml::Range(range) => {
                let Some((start, end)) = range.split_once(['-', ':']) else {
                    bail!("firewall port '{range}' must be a number or inclusive range")
                };
                (start.parse()?, end.parse()?)
            }
        };
        if start == 0 || end == 0 || start > end {
            bail!("firewall port range {start}-{end} is invalid");
        }
        Ok(Self { start, end })
    }

    fn contains(self, port: u16) -> bool {
        self.start <= port && port <= self.end
    }

    fn render(self, separator: char) -> String {
        if self.start == self.end {
            self.start.to_string()
        } else {
            format!("{}{separator}{}", self.start, self.end)
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct FirewallRuleTomlConfig {
    pub name: String,
    #[serde(default)]
    pub state: FirewallRuleState,
    #[serde(default)]
    pub direction: FirewallDirection,
    #[serde(default)]
    pub action: FirewallAction,
    pub port: Option<FirewallPortToml>,
    pub protocol: Option<FirewallProtocol>,
    pub source: Option<String>,
    pub destination: Option<String>,
    pub interface: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct FirewallTomlConfig {
    pub backend: Option<FirewallBackend>,
    pub state: Option<FirewallState>,
    pub default_incoming: Option<FirewallPolicy>,
    pub default_outgoing: Option<FirewallPolicy>,
    pub exclusive: Option<bool>,
    pub allow_lockout: Option<bool>,
    #[serde(default)]
    pub rules: Vec<FirewallRuleTomlConfig>,
}

fn default_incoming() -> FirewallPolicy {
    FirewallPolicy::Deny
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FirewallRule {
    name: String,
    state: FirewallRuleState,
    direction: FirewallDirection,
    action: FirewallAction,
    port: Option<FirewallPort>,
    protocol: Option<FirewallProtocol>,
    source: Option<IpNet>,
    destination: Option<IpNet>,
    interface: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FirewallRequest {
    backend: FirewallBackend,
    state: FirewallState,
    default_incoming: FirewallPolicy,
    default_outgoing: FirewallPolicy,
    exclusive: bool,
    allow_lockout: bool,
    rules: Vec<FirewallRule>,
    ssh_connection: Option<SshConnection>,
    #[serde(skip)]
    inspection: Option<FirewallInspection>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SshConnection {
    peer: IpAddr,
    server: IpAddr,
    server_port: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FirewallInspection {
    backend: Option<FirewallBackend>,
    managed: bool,
    exact: bool,
    active: bool,
    reason: Option<String>,
    current_rules: Vec<FirewallRule>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FirewallStateFile {
    backend: FirewallBackend,
    digest: String,
    #[serde(default)]
    live_fingerprint: Option<String>,
    request: FirewallRequest,
}

pub fn prepare_request_from_config(config: &Config) -> Result<Option<FirewallRequest>> {
    let mut merged = None;
    // config_files is ordered local -> global; merge global -> local so a
    // more local scalar or same-named rule overrides its inherited value.
    for cf in config.config_files.values().rev() {
        if let Some(bootstrap) = cf.bootstrap_config()
            && let Some(firewall) = bootstrap.linux.firewall
        {
            merged = Some(merge_toml_config(merged.unwrap_or_default(), firewall)?);
        }
    }
    merged.map(FirewallRequest::from_toml).transpose()
}

fn merge_toml_config(
    mut inherited: FirewallTomlConfig,
    local: FirewallTomlConfig,
) -> Result<FirewallTomlConfig> {
    let mut local_names = HashSet::new();
    for rule in &local.rules {
        if !local_names.insert(&rule.name) {
            bail!("firewall rule '{}' is declared more than once", rule.name);
        }
    }
    if local.backend.is_some() {
        inherited.backend = local.backend;
    }
    if local.state.is_some() {
        inherited.state = local.state;
    }
    if local.default_incoming.is_some() {
        inherited.default_incoming = local.default_incoming;
    }
    if local.default_outgoing.is_some() {
        inherited.default_outgoing = local.default_outgoing;
    }
    if local.exclusive.is_some() {
        inherited.exclusive = local.exclusive;
    }
    if local.allow_lockout.is_some() {
        inherited.allow_lockout = local.allow_lockout;
    }
    for rule in local.rules {
        match inherited
            .rules
            .iter()
            .position(|inherited| inherited.name == rule.name)
        {
            Some(index) => inherited.rules[index] = rule,
            None => inherited.rules.push(rule),
        }
    }
    Ok(inherited)
}

pub fn request_from_config(config: &Config) -> Result<Option<FirewallRequest>> {
    let Some(mut request) = prepare_request_from_config(config)? else {
        return Ok(None);
    };
    inspect_request(&mut request)?;
    Ok(Some(request))
}

pub fn status_request_from_config(config: &Config) -> Result<Option<FirewallRequest>> {
    request_from_config(config)
}

pub fn inspect_request(request: &mut FirewallRequest) -> Result<()> {
    let input = serde_json::to_vec(request)?;
    let executable = std::env::current_exe()?.to_string_lossy().to_string();
    let output = crate::system::sudo::run_with_input_output(
        &executable,
        &[
            "--no-config".to_string(),
            "--no-env".to_string(),
            "--no-hooks".to_string(),
            "bootstrap".to_string(),
            "__inspect-firewall-plan".to_string(),
        ],
        &input,
    )?;
    request.inspection = Some(serde_json::from_slice(&output)?);
    Ok(())
}

impl FirewallRequest {
    fn from_toml(config: FirewallTomlConfig) -> Result<Self> {
        let mut rules = vec![];
        let mut names = HashSet::new();
        for rule in config.rules {
            let name = rule.name;
            validate_name(&name)?;
            if !names.insert(name.clone()) {
                bail!("firewall rule '{name}' is declared more than once");
            }
            let interface = rule
                .interface
                .map(|interface| validate_interface(interface.trim()))
                .transpose()?;
            let source = rule
                .source
                .map(|source| source.parse::<IpNet>())
                .transpose()
                .wrap_err_with(|| format!("firewall rule '{name}' has an invalid source"))?;
            let destination = rule
                .destination
                .map(|destination| destination.parse::<IpNet>())
                .transpose()
                .wrap_err_with(|| format!("firewall rule '{name}' has an invalid destination"))?;
            if source.is_some_and(|source| {
                destination.is_some_and(|destination| {
                    source.addr().is_ipv4() != destination.addr().is_ipv4()
                })
            }) {
                bail!("firewall rule '{name}' mixes IPv4 and IPv6 source/destination networks");
            }
            let port = rule.port.map(FirewallPort::from_toml).transpose()?;
            if port.is_some() && rule.protocol.is_none() {
                bail!("firewall rule '{name}' sets port without protocol");
            }
            rules.push(FirewallRule {
                name,
                state: rule.state,
                direction: rule.direction,
                action: rule.action,
                port,
                protocol: rule.protocol,
                source,
                destination,
                interface,
            });
        }
        let ssh_connection = std::env::var("SSH_CONNECTION")
            .ok()
            .map(|value| parse_ssh_connection(&value))
            .transpose()?;
        let request = Self {
            backend: config.backend.unwrap_or_default(),
            state: config.state.unwrap_or_default(),
            default_incoming: config.default_incoming.unwrap_or_else(default_incoming),
            default_outgoing: config.default_outgoing.unwrap_or_default(),
            exclusive: config.exclusive.unwrap_or_default(),
            allow_lockout: config.allow_lockout.unwrap_or_default(),
            rules,
            ssh_connection,
            inspection: None,
        };
        request.validate_safety()?;
        Ok(request)
    }

    fn validate_safety(&self) -> Result<()> {
        self.validate_safety_with_rules(
            &self.rules,
            (self.backend != FirewallBackend::Auto).then_some(self.backend),
            ssh_ancestor_present(),
        )
    }

    fn validate_safety_with_rules(
        &self,
        rules: &[FirewallRule],
        backend: Option<FirewallBackend>,
        ssh_ancestor: Option<bool>,
    ) -> Result<()> {
        if self.state != FirewallState::Enabled
            || self.default_incoming == FirewallPolicy::Allow
            || self.allow_lockout
        {
            return Ok(());
        }
        let Some(connection) = &self.ssh_connection else {
            return match ssh_ancestor {
                Some(false) => Ok(()),
                Some(true) => bail!(
                    "refusing firewall default incoming {} from an SSH-derived process without SSH_CONNECTION: mise cannot verify that remote access will survive; preserve SSH_CONNECTION or set allow_lockout = true",
                    self.default_incoming.ufw()
                ),
                None => bail!(
                    "refusing firewall default incoming {} without SSH_CONNECTION because process ancestry could not be inspected; set allow_lockout = true to acknowledge the lockout risk",
                    self.default_incoming.ufw()
                ),
            };
        };
        let mut covered = false;
        for rule in rules.iter().filter(|rule| {
            rule.state == FirewallRuleState::Present
                && rule.direction == FirewallDirection::Incoming
                && rule
                    .protocol
                    .is_none_or(|protocol| protocol == FirewallProtocol::Tcp)
                && rule
                    .port
                    .is_none_or(|port| port.contains(connection.server_port))
                && rule
                    .source
                    .is_none_or(|source| source.contains(&connection.peer))
                && rule
                    .destination
                    .is_none_or(|destination| destination.contains(&connection.server))
        }) {
            if rule.action != FirewallAction::Allow {
                match backend {
                    Some(FirewallBackend::Nftables) if !covered => bail!(
                        "refusing firewall default incoming {} over SSH: blocking rule '{}' precedes a proven allow for peer {} on server port {}; reorder or narrow the rule, or set allow_lockout = true",
                        self.default_incoming.ufw(),
                        rule.name,
                        connection.peer,
                        connection.server_port
                    ),
                    Some(FirewallBackend::Firewalld) => bail!(
                        "refusing firewall default incoming {} over SSH: blocking rule '{}' also covers peer {} on server port {}, and firewalld cannot guarantee the allow wins; narrow the rule or set allow_lockout = true",
                        self.default_incoming.ufw(),
                        rule.name,
                        connection.peer,
                        connection.server_port
                    ),
                    // UFW installs allows before denies/rejects. An automatic
                    // backend is validated again after it is resolved.
                    _ => {}
                }
                continue;
            }
            // SSH_CONNECTION does not identify the ingress interface. An
            // interface-constrained allow cannot prove that it preserves this
            // session, so keep looking for an unrestricted covering allow.
            if rule.interface.is_none() {
                covered = true;
                if backend == Some(FirewallBackend::Nftables) {
                    break;
                }
            }
        }
        if !covered {
            bail!(
                "refusing firewall default incoming {} over SSH: no incoming TCP allow rule covers peer {} on server port {} with an unrestricted interface; add a covering rule or set allow_lockout = true",
                self.default_incoming.ufw(),
                connection.peer,
                connection.server_port
            );
        }
        Ok(())
    }

    pub fn plans(&self) -> Vec<ResourcePlan> {
        let inspection = self.inspection.as_ref();
        let backend = inspection
            .and_then(|inspection| inspection.backend)
            .unwrap_or(self.backend);
        let desired = format!(
            "{} via {}; incoming {}; outgoing {}; {}",
            match self.state {
                FirewallState::Enabled => "enabled",
                FirewallState::Disabled => "disabled",
                FirewallState::Absent => "absent",
            },
            backend.label(),
            self.default_incoming.ufw(),
            self.default_outgoing.ufw(),
            if self.exclusive {
                "exclusive"
            } else {
                "coexisting"
            }
        );
        let (current, action) = match inspection {
            None => ("not inspected".to_string(), ResourceAction::Unknown),
            Some(inspection) if inspection.reason.is_some() => (
                format!(
                    "unavailable: {}",
                    inspection.reason.as_deref().unwrap_or_default()
                ),
                ResourceAction::Unknown,
            ),
            Some(inspection) if inspection.exact => (
                match self.state {
                    FirewallState::Enabled => format!("enabled via {}", backend.label()),
                    FirewallState::Disabled => format!("disabled via {}", backend.label()),
                    FirewallState::Absent => "absent".to_string(),
                },
                ResourceAction::Noop,
            ),
            Some(inspection) if inspection.managed => (
                format!("managed drift via {}", backend.label()),
                if self.state == FirewallState::Absent {
                    ResourceAction::Remove
                } else {
                    ResourceAction::Update
                },
            ),
            Some(_) if self.state == FirewallState::Absent => {
                ("absent".to_string(), ResourceAction::Noop)
            }
            Some(_) => ("unmanaged".to_string(), ResourceAction::Create),
        };
        let mut plans = vec![ResourcePlan::new(
            ResourceId::new("firewall", "linux"),
            current,
            desired,
            action,
        )];
        let current_rules = inspection
            .map(|inspection| {
                inspection
                    .current_rules
                    .iter()
                    .map(|rule| (rule.name.as_str(), rule))
                    .collect::<IndexMap<_, _>>()
            })
            .unwrap_or_default();
        for rule in &self.rules {
            let current = current_rules.get(rule.name.as_str()).copied();
            let desired_state = if self.state == FirewallState::Absent {
                FirewallRuleState::Absent
            } else {
                rule.state
            };
            let (current_description, action) = match (desired_state, current) {
                (FirewallRuleState::Absent, None) => ("absent".to_string(), ResourceAction::Noop),
                (FirewallRuleState::Absent, Some(_)) => {
                    ("present".to_string(), ResourceAction::Remove)
                }
                (FirewallRuleState::Present, None) => {
                    ("absent".to_string(), ResourceAction::Create)
                }
                (FirewallRuleState::Present, Some(existing)) if existing == rule => {
                    (rule.describe(), ResourceAction::Noop)
                }
                (FirewallRuleState::Present, Some(existing)) => {
                    (existing.describe(), ResourceAction::Update)
                }
            };
            plans.push(ResourcePlan::new(
                ResourceId::new("firewall-rule", &rule.name),
                current_description,
                if desired_state == FirewallRuleState::Absent {
                    "absent".to_string()
                } else {
                    rule.describe()
                },
                action,
            ));
        }
        plans
    }
}

impl FirewallRule {
    fn describe(&self) -> String {
        let mut parts = vec![
            match self.direction {
                FirewallDirection::Incoming => "incoming",
                FirewallDirection::Outgoing => "outgoing",
            }
            .to_string(),
            self.action.ufw().to_string(),
        ];
        if let Some(protocol) = self.protocol {
            parts.push(protocol.as_str().to_string());
        }
        if let Some(port) = self.port {
            parts.push(format!("port {}", port.render('-')));
        }
        if let Some(source) = self.source {
            parts.push(format!("from {source}"));
        }
        if let Some(destination) = self.destination {
            parts.push(format!("to {destination}"));
        }
        if let Some(interface) = &self.interface {
            parts.push(format!("on {interface}"));
        }
        parts.join(" ")
    }
}

pub fn apply(request: &FirewallRequest, dry_run: bool, yes: bool) -> Result<()> {
    let plan = request.plans();
    let changes = plan
        .iter()
        .filter(|resource| resource.action != ResourceAction::Noop)
        .count();
    if changes == 0 {
        info!("firewall: already converged");
        return Ok(());
    }
    if plan
        .iter()
        .any(|resource| resource.action == ResourceAction::Unknown)
    {
        if dry_run {
            for resource in plan
                .iter()
                .filter(|resource| resource.action == ResourceAction::Unknown)
            {
                warn!(
                    "would not change {}: current {}, desired {} (manual action required)",
                    resource.id, resource.current, resource.desired
                );
            }
            return Ok(());
        }
        bail!("refusing unsafe firewall change; inspect `mise bootstrap firewall status`");
    }
    if dry_run {
        let inspection = request.inspection.as_ref().expect("firewall was inspected");
        let backend = inspection.backend.expect("available firewall backend");
        for command in preview_commands(request, backend)? {
            miseprintln!("would run {}", shell_words::join(command));
        }
        return Ok(());
    }
    let destructive = request.exclusive
        || matches!(
            request.state,
            FirewallState::Disabled | FirewallState::Absent
        )
        || plan
            .iter()
            .any(|resource| resource.action == ResourceAction::Remove);
    if !yes
        && console::user_attended_stderr()
        && !crate::ui::prompt::confirm(format!(
            "firewall: apply {changes} change(s){}?",
            if destructive {
                " including destructive changes"
            } else {
                ""
            }
        ))?
    {
        info!("firewall: skipped");
        return Ok(());
    }
    let input = serde_json::to_vec(request)?;
    let executable = std::env::current_exe()?.to_string_lossy().to_string();
    crate::system::sudo::run_with_input(
        &executable,
        &[
            "--no-config".to_string(),
            "--no-env".to_string(),
            "--no-hooks".to_string(),
            "bootstrap".to_string(),
            "__apply-firewall-plan".to_string(),
        ],
        &input,
    )?;
    info!("firewall: applied changes");
    Ok(())
}

pub fn inspect_privileged_plan_from_stdin() -> Result<()> {
    let request: FirewallRequest = serde_json::from_reader(std::io::stdin().lock())?;
    request.validate_safety()?;
    let inspection = inspect_privileged(&request);
    serde_json::to_writer(std::io::stdout().lock(), &inspection)?;
    Ok(())
}

pub fn apply_privileged_plan_from_stdin() -> Result<()> {
    let request: FirewallRequest = serde_json::from_reader(std::io::stdin().lock())?;
    request.validate_safety()?;
    apply_privileged(&request)
}

fn inspect_privileged(request: &FirewallRequest) -> FirewallInspection {
    let state = read_state().ok().flatten();
    let backend = match resolve_backend(request.backend, state.as_ref()) {
        Ok(backend) => backend,
        Err(error) => {
            return FirewallInspection {
                backend: None,
                managed: state.is_some(),
                exact: false,
                active: false,
                reason: Some(error.to_string()),
                current_rules: state.map(|state| state.request.rules).unwrap_or_default(),
            };
        }
    };
    if let Err(error) = validate_backend_request(request, backend) {
        return FirewallInspection {
            backend: Some(backend),
            managed: state.is_some() || managed_backend_present(backend),
            exact: false,
            active: false,
            reason: Some(error.to_string()),
            current_rules: state.map(|state| state.request.rules).unwrap_or_default(),
        };
    }
    let effective = effective_request(request, state.as_ref());
    if let Err(error) = validate_effective_backend_request(request, &effective, backend) {
        return FirewallInspection {
            backend: Some(backend),
            managed: state.is_some() || managed_backend_present(backend),
            exact: false,
            active: false,
            reason: Some(error.to_string()),
            current_rules: state.map(|state| state.request.rules).unwrap_or_default(),
        };
    }
    let expected_digest = request_digest(&effective, backend).unwrap_or_default();
    let managed = state.is_some() || managed_backend_present(backend);
    let active = managed_backend_active(backend);
    let live_matches = live_matches(
        backend,
        &effective,
        &expected_digest,
        state
            .as_ref()
            .and_then(|state| state.live_fingerprint.as_deref()),
    );
    let exact = match request.state {
        FirewallState::Absent => !managed,
        FirewallState::Disabled => state.as_ref().is_some_and(|state| {
            state.backend == backend && state.digest == expected_digest && !active && live_matches
        }),
        FirewallState::Enabled => state.as_ref().is_some_and(|state| {
            state.backend == backend && state.digest == expected_digest && active && live_matches
        }),
    };
    FirewallInspection {
        backend: Some(backend),
        managed,
        exact,
        active,
        reason: None,
        current_rules: state.map(|state| state.request.rules).unwrap_or_default(),
    }
}

fn apply_privileged(request: &FirewallRequest) -> Result<()> {
    let state = read_state()?;
    let backend = resolve_backend(request.backend, state.as_ref())?;
    validate_backend_request(request, backend)?;
    let effective = effective_request(request, state.as_ref());
    validate_effective_backend_request(request, &effective, backend)?;
    if let Some(state) = &state
        && state.backend != backend
    {
        remove_backend(state.backend, true)?;
    }
    if request.state == FirewallState::Absent {
        remove_backend(backend, true)?;
        remove_state()?;
        return Ok(());
    }
    let digest = request_digest(&effective, backend)?;
    match effective.state {
        FirewallState::Enabled => apply_backend(backend, &effective, &digest)?,
        FirewallState::Disabled => disable_backend(backend)?,
        FirewallState::Absent => unreachable!(),
    }
    let live_fingerprint = if effective.state == FirewallState::Enabled {
        backend_live_fingerprint(backend)?
    } else {
        None
    };
    write_state(&FirewallStateFile {
        backend,
        digest,
        live_fingerprint,
        request: effective,
    })
}

fn effective_request(
    request: &FirewallRequest,
    state: Option<&FirewallStateFile>,
) -> FirewallRequest {
    let declared = request
        .rules
        .iter()
        .map(|rule| rule.name.as_str())
        .collect::<HashSet<_>>();
    let mut rules = IndexMap::<String, FirewallRule>::new();
    if !request.exclusive
        && let Some(state) = state
    {
        for rule in &state.request.rules {
            if rule.state == FirewallRuleState::Present && !declared.contains(rule.name.as_str()) {
                rules.insert(rule.name.clone(), rule.clone());
            }
        }
    }
    for rule in &request.rules {
        match rule.state {
            FirewallRuleState::Present => {
                rules.insert(rule.name.clone(), rule.clone());
            }
            FirewallRuleState::Absent => {
                rules.shift_remove(&rule.name);
            }
        }
    }
    FirewallRequest {
        backend: request.backend,
        state: request.state,
        default_incoming: request.default_incoming,
        default_outgoing: request.default_outgoing,
        exclusive: request.exclusive,
        allow_lockout: request.allow_lockout,
        rules: rules.into_values().collect(),
        ssh_connection: None,
        inspection: None,
    }
}

fn resolve_backend(
    requested: FirewallBackend,
    state: Option<&FirewallStateFile>,
) -> Result<FirewallBackend> {
    if requested != FirewallBackend::Auto {
        ensure_backend_available(requested)?;
        return Ok(requested);
    }
    if let Some(state) = state
        && backend_available(state.backend)
    {
        return Ok(state.backend);
    }
    for backend in [FirewallBackend::Firewalld, FirewallBackend::Ufw] {
        if backend_available(backend) && backend_active(backend) {
            return Ok(backend);
        }
    }
    for backend in [
        FirewallBackend::Nftables,
        FirewallBackend::Firewalld,
        FirewallBackend::Ufw,
    ] {
        if backend_available(backend) {
            return Ok(backend);
        }
    }
    bail!("no supported firewall backend found (tried nft, firewall-cmd, and ufw)")
}

fn ensure_backend_available(backend: FirewallBackend) -> Result<()> {
    if backend_available(backend) {
        Ok(())
    } else {
        bail!(
            "firewall backend '{}' requires command '{}'",
            backend.label(),
            backend.program().unwrap_or_default()
        )
    }
}

fn backend_available(backend: FirewallBackend) -> bool {
    backend.program().and_then(crate::file::which).is_some()
}

fn backend_active(backend: FirewallBackend) -> bool {
    match backend {
        FirewallBackend::Nftables => command_output("nft", &["list", "table", "inet", NFT_TABLE])
            .is_ok_and(|output| output.status.success()),
        FirewallBackend::Firewalld => {
            command_output("firewall-cmd", &["--state"]).is_ok_and(|output| output.status.success())
        }
        FirewallBackend::Ufw => command_output("ufw", &["status"])
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .is_some_and(|stdout| stdout.lines().any(|line| line.trim() == "Status: active")),
        FirewallBackend::Auto => false,
    }
}

fn managed_backend_active(backend: FirewallBackend) -> bool {
    match backend {
        FirewallBackend::Nftables => managed_backend_present(backend),
        FirewallBackend::Firewalld => {
            backend_active(backend)
                && [FIREWALLD_INCOMING, FIREWALLD_OUTGOING]
                    .iter()
                    .all(|policy| {
                        command_output(
                            "firewall-cmd",
                            &["--permanent", &format!("--info-policy={policy}")],
                        )
                        .is_ok_and(|output| output.status.success())
                    })
        }
        FirewallBackend::Ufw => backend_active(backend),
        FirewallBackend::Auto => false,
    }
}

fn validate_backend_request(request: &FirewallRequest, backend: FirewallBackend) -> Result<()> {
    for rule in &request.rules {
        if backend == FirewallBackend::Firewalld && rule.interface.is_some() {
            bail!(
                "firewall rule '{}' uses interface matching, which firewalld policies cannot express safely; select backend = \"nftables\" or \"ufw\"",
                rule.name
            );
        }
        if backend == FirewallBackend::Ufw
            && matches!(
                rule.protocol,
                Some(FirewallProtocol::Sctp | FirewallProtocol::Dccp)
            )
        {
            bail!(
                "firewall rule '{}' uses protocol {}, which UFW does not support; select backend = \"nftables\" or \"firewalld\"",
                rule.name,
                rule.protocol.expect("matched protocol").as_str()
            );
        }
    }
    Ok(())
}

fn validate_effective_backend_request(
    request: &FirewallRequest,
    effective: &FirewallRequest,
    backend: FirewallBackend,
) -> Result<()> {
    if effective.state == FirewallState::Enabled {
        validate_backend_request(effective, backend)?;
        request.validate_safety_with_rules(
            &effective.rules,
            Some(backend),
            ssh_ancestor_present(),
        )?;
    }
    Ok(())
}

fn managed_backend_present(backend: FirewallBackend) -> bool {
    match backend {
        FirewallBackend::Nftables => backend_active(backend),
        FirewallBackend::Firewalld => {
            [FIREWALLD_INCOMING, FIREWALLD_OUTGOING]
                .iter()
                .any(|policy| {
                    Path::new(FIREWALLD_POLICY_DIR)
                        .join(format!("{policy}.xml"))
                        .exists()
                        || command_output(
                            "firewall-cmd",
                            &["--permanent", &format!("--info-policy={policy}")],
                        )
                        .is_ok_and(|output| output.status.success())
                })
        }
        FirewallBackend::Ufw => {
            ufw_added_rules().is_ok_and(|rules| rules.iter().any(|rule| rule.contains("mise:")))
        }
        FirewallBackend::Auto => false,
    }
}

fn live_matches(
    backend: FirewallBackend,
    request: &FirewallRequest,
    digest: &str,
    expected_live_fingerprint: Option<&str>,
) -> bool {
    match request.state {
        FirewallState::Absent => !managed_backend_present(backend),
        FirewallState::Disabled => match backend {
            FirewallBackend::Ufw => !backend_active(backend),
            FirewallBackend::Nftables | FirewallBackend::Firewalld => {
                !managed_backend_present(backend)
            }
            FirewallBackend::Auto => false,
        },
        FirewallState::Enabled => match backend {
            FirewallBackend::Nftables => {
                command_output("nft", &["list", "table", "inet", NFT_TABLE])
                    .ok()
                    .filter(|output| output.status.success())
                    .is_some_and(|output| {
                        String::from_utf8_lossy(&output.stdout)
                            .contains(&format!("mise-bootstrap:{digest}"))
                            && expected_live_fingerprint
                                .is_some_and(|expected| fingerprint(&output.stdout) == expected)
                    })
            }
            FirewallBackend::Firewalld => {
                backend_active(backend)
                    && firewalld_policy_matches(
                        FIREWALLD_INCOMING,
                        "ANY",
                        "HOST",
                        request.default_incoming,
                        request,
                        FirewallDirection::Incoming,
                        digest,
                    )
                    && firewalld_policy_matches(
                        FIREWALLD_OUTGOING,
                        "HOST",
                        "ANY",
                        request.default_outgoing,
                        request,
                        FirewallDirection::Outgoing,
                        digest,
                    )
            }
            FirewallBackend::Ufw => ufw_matches(request),
            FirewallBackend::Auto => false,
        },
    }
}

fn backend_live_fingerprint(backend: FirewallBackend) -> Result<Option<String>> {
    if backend != FirewallBackend::Nftables {
        return Ok(None);
    }
    let output = command_output("nft", &["list", "table", "inet", NFT_TABLE])?;
    if !output.status.success() {
        return Err(command_error(
            "nft",
            &["list", "table", "inet", NFT_TABLE],
            &output,
        ));
    }
    Ok(Some(fingerprint(&output.stdout)))
}

fn fingerprint(body: &[u8]) -> String {
    hex::encode(Sha256::digest(body))
}

fn apply_backend(backend: FirewallBackend, request: &FirewallRequest, digest: &str) -> Result<()> {
    match backend {
        FirewallBackend::Nftables => apply_nftables(request, digest),
        FirewallBackend::Firewalld => apply_firewalld(request, digest),
        FirewallBackend::Ufw => apply_ufw(request),
        FirewallBackend::Auto => unreachable!(),
    }
}

fn remove_backend(backend: FirewallBackend, purge: bool) -> Result<()> {
    match backend {
        FirewallBackend::Nftables => remove_nftables(purge),
        FirewallBackend::Firewalld => remove_firewalld(),
        FirewallBackend::Ufw => remove_ufw_managed_rules(),
        FirewallBackend::Auto => Ok(()),
    }
}

fn disable_backend(backend: FirewallBackend) -> Result<()> {
    match backend {
        FirewallBackend::Nftables => remove_nftables(false),
        FirewallBackend::Firewalld => remove_firewalld(),
        FirewallBackend::Ufw => {
            let ufw = crate::file::which("ufw").ok_or_else(|| eyre!("ufw not found"))?;
            run(&ufw, &["--force", "disable"])
        }
        FirewallBackend::Auto => unreachable!(),
    }
}

fn apply_nftables(request: &FirewallRequest, digest: &str) -> Result<()> {
    let nft = crate::file::which("nft").ok_or_else(|| eyre!("nft not found"))?;
    let systemctl = crate::file::which("systemctl")
        .ok_or_else(|| eyre!("nftables persistence requires systemctl"))?;
    let persistent_rules = render_nftables(request, digest, false);
    let runtime_rules = render_nftables(
        request,
        digest,
        managed_backend_present(FirewallBackend::Nftables),
    );
    run_with_stdin(&nft, &["-c", "-f", "-"], runtime_rules.as_bytes())?;
    write_atomic(
        Path::new(NFT_RULES_PATH),
        persistent_rules.as_bytes(),
        0o600,
    )?;
    let unit = render_nftables_unit(&nft);
    write_atomic(Path::new(NFT_UNIT_PATH), unit.as_bytes(), 0o644)?;
    run_with_stdin(&nft, &["-f", "-"], runtime_rules.as_bytes())?;
    run(&systemctl, &["daemon-reload"])?;
    run(&systemctl, &["enable", "mise-bootstrap-firewall.service"])
}

fn remove_nftables(purge: bool) -> Result<()> {
    if let Some(nft) = crate::file::which("nft")
        && managed_backend_present(FirewallBackend::Nftables)
    {
        run(&nft, &["delete", "table", "inet", NFT_TABLE])?;
    }
    if let Some(systemctl) = crate::file::which("systemctl") {
        let _ = run(&systemctl, &["disable", "mise-bootstrap-firewall.service"]);
        if purge {
            let _ = fs::remove_file(NFT_UNIT_PATH);
            let _ = fs::remove_file(NFT_RULES_PATH);
            let _ = run(&systemctl, &["daemon-reload"]);
        }
    }
    Ok(())
}

fn render_nftables(request: &FirewallRequest, digest: &str, replace: bool) -> String {
    let mut lines = vec![];
    if replace {
        lines.push(format!("delete table inet {NFT_TABLE}"));
    }
    lines.extend([
        format!("add table inet {NFT_TABLE}"),
        format!(
            "add chain inet {NFT_TABLE} input {{ type filter hook input priority filter; policy {}; comment \"mise-bootstrap:{digest}\"; }}",
            request.default_incoming.nft()
        ),
        format!(
            "add chain inet {NFT_TABLE} output {{ type filter hook output priority filter; policy {}; comment \"mise-bootstrap:{digest}\"; }}",
            request.default_outgoing.nft()
        ),
        format!("add rule inet {NFT_TABLE} input ct state established,related accept"),
        format!("add rule inet {NFT_TABLE} input iifname lo accept"),
        format!("add rule inet {NFT_TABLE} input meta l4proto icmp accept"),
        format!("add rule inet {NFT_TABLE} input meta l4proto ipv6-icmp accept"),
    ]);
    for rule in &request.rules {
        lines.push(render_nftables_rule(rule));
    }
    if request.default_incoming == FirewallPolicy::Reject {
        lines.push(format!("add rule inet {NFT_TABLE} input reject"));
    }
    if request.default_outgoing == FirewallPolicy::Reject {
        lines.push(format!("add rule inet {NFT_TABLE} output reject"));
    }
    lines.join("\n") + "\n"
}

fn render_nftables_rule(rule: &FirewallRule) -> String {
    let chain = match rule.direction {
        FirewallDirection::Incoming => "input",
        FirewallDirection::Outgoing => "output",
    };
    let mut parts = vec![
        "add".to_string(),
        "rule".to_string(),
        "inet".to_string(),
        NFT_TABLE.to_string(),
        chain.to_string(),
    ];
    if let Some(interface) = &rule.interface {
        parts.extend([
            match rule.direction {
                FirewallDirection::Incoming => "iifname",
                FirewallDirection::Outgoing => "oifname",
            }
            .to_string(),
            format!("\"{interface}\""),
        ]);
    }
    for (network, endpoint) in [(rule.source, "saddr"), (rule.destination, "daddr")]
        .into_iter()
        .filter_map(|(network, endpoint)| network.map(|network| (network, endpoint)))
    {
        parts.extend([
            if network.addr().is_ipv4() {
                "ip"
            } else {
                "ip6"
            }
            .to_string(),
            endpoint.to_string(),
            network.to_string(),
        ]);
    }
    if let Some(protocol) = rule.protocol {
        parts.extend([
            "meta".to_string(),
            "l4proto".to_string(),
            protocol.as_str().to_string(),
        ]);
        if let Some(port) = rule.port {
            parts.extend([
                protocol.as_str().to_string(),
                "dport".to_string(),
                port.render('-'),
            ]);
        }
    }
    parts.push(rule.action.nft().to_string());
    parts.extend(["comment".to_string(), format!("\"mise:{}\"", rule.name)]);
    parts.join(" ")
}

fn render_nftables_unit(nft: &Path) -> String {
    format!(
        "[Unit]\nDescription=mise bootstrap firewall\nBefore=network-pre.target\nWants=network-pre.target\n\n[Service]\nType=oneshot\nRemainAfterExit=yes\nExecStartPre=-{nft} delete table inet {NFT_TABLE}\nExecStart={nft} -f {NFT_RULES_PATH}\nExecReload=-{nft} delete table inet {NFT_TABLE}\nExecReload={nft} -f {NFT_RULES_PATH}\nExecStop=-{nft} delete table inet {NFT_TABLE}\n\n[Install]\nWantedBy=multi-user.target\n",
        nft = nft.display()
    )
}

fn apply_firewalld(request: &FirewallRequest, digest: &str) -> Result<()> {
    let firewall_cmd = ensure_firewalld_running()?;
    for policy in [FIREWALLD_INCOMING, FIREWALLD_OUTGOING] {
        if command_output_path(
            &firewall_cmd,
            &["--permanent", &format!("--info-policy={policy}")],
        )
        .is_ok_and(|output| output.status.success())
        {
            run(
                &firewall_cmd,
                &["--permanent", &format!("--delete-policy={policy}")],
            )?;
        }
        run(
            &firewall_cmd,
            &["--permanent", &format!("--new-policy={policy}")],
        )?;
        run(
            &firewall_cmd,
            &[
                "--permanent",
                &format!("--policy={policy}"),
                &format!("--set-description=mise-bootstrap:{digest}"),
            ],
        )?;
        run(
            &firewall_cmd,
            &[
                "--permanent",
                &format!("--policy={policy}"),
                "--set-priority=-1000",
            ],
        )?;
    }
    configure_firewalld_policy(
        &firewall_cmd,
        FIREWALLD_INCOMING,
        "ANY",
        "HOST",
        request.default_incoming,
    )?;
    configure_firewalld_policy(
        &firewall_cmd,
        FIREWALLD_OUTGOING,
        "HOST",
        "ANY",
        request.default_outgoing,
    )?;
    for rule in &request.rules {
        let policy = match rule.direction {
            FirewallDirection::Incoming => FIREWALLD_INCOMING,
            FirewallDirection::Outgoing => FIREWALLD_OUTGOING,
        };
        let rich_rule = render_firewalld_rule(rule);
        run(
            &firewall_cmd,
            &[
                "--permanent",
                &format!("--policy={policy}"),
                &format!("--add-rich-rule={rich_rule}"),
            ],
        )?;
    }
    run(&firewall_cmd, &["--check-config"])?;
    run(&firewall_cmd, &["--reload"])
}

fn configure_firewalld_policy(
    command: &Path,
    name: &str,
    ingress: &str,
    egress: &str,
    target: FirewallPolicy,
) -> Result<()> {
    for option in [
        format!("--add-ingress-zone={ingress}"),
        format!("--add-egress-zone={egress}"),
        format!("--set-target={}", target.firewalld()),
    ] {
        run(
            command,
            &["--permanent", &format!("--policy={name}"), &option],
        )?;
    }
    Ok(())
}

fn render_firewalld_rule(rule: &FirewallRule) -> String {
    let mut parts = vec!["rule".to_string()];
    let family = rule.source.or(rule.destination).map(|network| {
        if network.addr().is_ipv4() {
            "ipv4"
        } else {
            "ipv6"
        }
    });
    if let Some(family) = family {
        parts.push(format!("family=\"{family}\""));
    }
    if let Some(source) = rule.source {
        parts.push(format!("source address=\"{source}\""));
    }
    if let Some(destination) = rule.destination {
        parts.push(format!("destination address=\"{destination}\""));
    }
    if let Some(protocol) = rule.protocol {
        if let Some(port) = rule.port {
            parts.push(format!(
                "port port=\"{}\" protocol=\"{}\"",
                port.render('-'),
                protocol.as_str()
            ));
        } else {
            parts.push(format!("protocol value=\"{}\"", protocol.as_str()));
        }
    }
    parts.push(rule.action.firewalld().to_string());
    parts.join(" ")
}

fn firewalld_policy_matches(
    policy: &str,
    ingress: &str,
    egress: &str,
    target: FirewallPolicy,
    request: &FirewallRequest,
    direction: FirewallDirection,
    digest: &str,
) -> bool {
    let path = Path::new(FIREWALLD_POLICY_DIR).join(format!("{policy}.xml"));
    if !fs::read_to_string(path)
        .is_ok_and(|body| body.contains(&format!("mise-bootstrap:{digest}")))
    {
        return false;
    }
    let Some(info) = command_output(
        "firewall-cmd",
        &["--permanent", &format!("--info-policy={policy}")],
    )
    .ok()
    .filter(|output| output.status.success())
    .and_then(|output| String::from_utf8(output.stdout).ok()) else {
        return false;
    };
    let lines = info.lines().map(str::trim).collect::<HashSet<_>>();
    if !lines.contains("priority: -1000")
        || !lines.contains(format!("target: {}", target.firewalld()).as_str())
        || !lines.contains(format!("ingress-zones: {ingress}").as_str())
        || !lines.contains(format!("egress-zones: {egress}").as_str())
        || ![
            "services:",
            "ports:",
            "protocols:",
            "forward-ports:",
            "source-ports:",
            "icmp-blocks:",
        ]
        .iter()
        .all(|empty| lines.contains(empty))
        || !lines.contains("masquerade: no")
    {
        return false;
    }
    let actual_rules = lines
        .iter()
        .filter(|line| line.starts_with("rule "))
        .map(|line| (*line).to_string())
        .collect::<HashSet<_>>();
    let expected_rules = request
        .rules
        .iter()
        .filter(|rule| rule.direction == direction)
        .map(render_firewalld_rule)
        .collect::<HashSet<_>>();
    actual_rules == expected_rules
}

fn remove_firewalld() -> Result<()> {
    if !managed_backend_present(FirewallBackend::Firewalld) {
        return Ok(());
    }
    let command = ensure_firewalld_running()?;
    let mut changed = false;
    for policy in [FIREWALLD_INCOMING, FIREWALLD_OUTGOING] {
        if command_output_path(
            &command,
            &["--permanent", &format!("--info-policy={policy}")],
        )
        .is_ok_and(|output| output.status.success())
        {
            run(
                &command,
                &["--permanent", &format!("--delete-policy={policy}")],
            )?;
            changed = true;
        }
    }
    if changed {
        run(&command, &["--reload"])?;
    }
    Ok(())
}

fn apply_ufw(request: &FirewallRequest) -> Result<()> {
    let ufw = crate::file::which("ufw").ok_or_else(|| eyre!("ufw not found"))?;
    if request.exclusive {
        run(&ufw, &["--force", "reset"])?;
    } else {
        remove_ufw_managed_rules()?;
    }
    let mut rules = request.rules.iter().collect::<Vec<_>>();
    rules.sort_by_key(|rule| rule.action != FirewallAction::Allow);
    for rule in rules {
        let args = render_ufw_rule(rule);
        run_owned(&ufw, &args)?;
    }
    run(
        &ufw,
        &["default", request.default_incoming.ufw(), "incoming"],
    )?;
    run(
        &ufw,
        &["default", request.default_outgoing.ufw(), "outgoing"],
    )?;
    run(&ufw, &["--force", "enable"])
}

fn render_ufw_rule(rule: &FirewallRule) -> Vec<String> {
    let mut args = vec![rule.action.ufw().to_string()];
    if rule.direction == FirewallDirection::Outgoing {
        args.push("out".to_string());
    }
    if let Some(interface) = &rule.interface {
        args.extend(["on".to_string(), interface.clone()]);
    }
    if let Some(protocol) = rule.protocol {
        args.extend(["proto".to_string(), protocol.as_str().to_string()]);
    }
    if let Some(source) = rule.source {
        args.extend(["from".to_string(), source.to_string()]);
    } else {
        args.extend(["from".to_string(), "any".to_string()]);
    }
    if let Some(destination) = rule.destination {
        args.extend(["to".to_string(), destination.to_string()]);
    } else {
        args.extend(["to".to_string(), "any".to_string()]);
    }
    if let Some(port) = rule.port {
        args.extend(["port".to_string(), port.render(':')]);
    }
    args.extend(["comment".to_string(), format!("mise:{}", rule.name)]);
    args
}

fn remove_ufw_managed_rules() -> Result<()> {
    let Some(ufw) = crate::file::which("ufw") else {
        return Ok(());
    };
    for added in ufw_added_rules()? {
        let Some(command) = added.strip_prefix("ufw ") else {
            continue;
        };
        let command = shell_words::split(command)?;
        if !command.windows(2).any(|pair| {
            pair[0] == "comment"
                && pair[1]
                    .strip_prefix("mise:")
                    .is_some_and(|name| validate_name(name).is_ok())
        }) {
            continue;
        }
        let mut args = vec!["--force".to_string(), "delete".to_string()];
        args.extend(command);
        run_owned(&ufw, &args)?;
    }
    Ok(())
}

fn ufw_added_rules() -> Result<Vec<String>> {
    let output = command_output("ufw", &["show", "added"])?;
    if !output.status.success() {
        return Err(command_error("ufw", &["show", "added"], &output));
    }
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("ufw "))
        .map(str::to_string)
        .collect())
}

fn ufw_matches(request: &FirewallRequest) -> bool {
    if !backend_active(FirewallBackend::Ufw) {
        return false;
    }
    let Ok(output) = command_output("ufw", &["status", "verbose"]) else {
        return false;
    };
    let Ok(status) = String::from_utf8(output.stdout) else {
        return false;
    };
    let defaults_match = status.lines().any(|line| {
        line.starts_with("Default:")
            && line.contains(&format!("{} (incoming)", request.default_incoming.ufw()))
            && line.contains(&format!("{} (outgoing)", request.default_outgoing.ufw()))
    });
    if !defaults_match {
        return false;
    }
    let Ok(added) = ufw_added_rules() else {
        return false;
    };
    let parsed = added
        .iter()
        .filter_map(|line| line.strip_prefix("ufw "))
        .filter_map(|line| shell_words::split(line).ok())
        .collect::<Vec<_>>();
    let managed = parsed
        .iter()
        .filter(|tokens| {
            tokens
                .windows(2)
                .any(|pair| pair[0] == "comment" && pair[1].starts_with("mise:"))
        })
        .collect::<Vec<_>>();
    managed.len() == request.rules.len()
        && request
            .rules
            .iter()
            .all(|rule| managed.iter().any(|tokens| ufw_rule_matches(tokens, rule)))
}

fn ufw_rule_matches(tokens: &[String], rule: &FirewallRule) -> bool {
    if tokens.first().map(String::as_str) != Some(rule.action.ufw()) {
        return false;
    }
    let outgoing = tokens.iter().any(|token| token == "out");
    if outgoing != (rule.direction == FirewallDirection::Outgoing) {
        return false;
    }
    let marker = format!("mise:{}", rule.name);
    if value_after(tokens, "comment") != Some(marker.as_str()) {
        return false;
    }
    if value_after(tokens, "on") != rule.interface.as_deref() {
        return false;
    }
    if !ufw_endpoint_matches(value_after(tokens, "from"), rule.source)
        || !ufw_endpoint_matches(value_after(tokens, "to"), rule.destination)
    {
        return false;
    }
    let compound = tokens.iter().find_map(|token| token.split_once('/'));
    let actual_protocol =
        value_after(tokens, "proto").or_else(|| compound.map(|(_, protocol)| protocol));
    if actual_protocol != rule.protocol.map(FirewallProtocol::as_str) {
        return false;
    }
    let actual_port = value_after(tokens, "port").or_else(|| compound.map(|(port, _)| port));
    actual_port == rule.port.map(|port| port.render(':')).as_deref()
}

fn value_after<'a>(tokens: &'a [String], key: &str) -> Option<&'a str> {
    tokens
        .windows(2)
        .find(|pair| pair[0] == key)
        .map(|pair| pair[1].as_str())
}

fn ufw_endpoint_matches(actual: Option<&str>, desired: Option<IpNet>) -> bool {
    match (actual, desired) {
        (None | Some("any"), None) => true,
        (Some(actual), Some(desired)) => actual
            .parse::<IpNet>()
            .map(|actual| actual == desired)
            .or_else(|_| {
                actual
                    .parse::<IpAddr>()
                    .map(IpNet::from)
                    .map(|actual| actual == desired)
            })
            .unwrap_or(false),
        _ => false,
    }
}

fn preview_commands(
    request: &FirewallRequest,
    backend: FirewallBackend,
) -> Result<Vec<Vec<String>>> {
    let effective = effective_request(request, read_state()?.as_ref());
    validate_effective_backend_request(request, &effective, backend)?;
    let mut commands = vec![];
    match effective.state {
        FirewallState::Absent => commands.push(vec![
            backend.label().to_string(),
            "remove mise-managed firewall state".to_string(),
        ]),
        FirewallState::Disabled => commands.push(vec![
            backend.label().to_string(),
            "disable mise-managed firewall state".to_string(),
        ]),
        FirewallState::Enabled => match backend {
            FirewallBackend::Nftables => commands.push(vec![
                "nft".to_string(),
                "-f".to_string(),
                NFT_RULES_PATH.to_string(),
            ]),
            FirewallBackend::Firewalld => {
                commands.push(vec![
                    "firewall-cmd".to_string(),
                    "--permanent".to_string(),
                    "reconcile mise-bootstrap policies".to_string(),
                ]);
                commands.push(vec!["firewall-cmd".to_string(), "--reload".to_string()]);
            }
            FirewallBackend::Ufw => {
                for rule in &effective.rules {
                    let mut command = vec!["ufw".to_string()];
                    command.extend(render_ufw_rule(rule));
                    commands.push(command);
                }
                commands.push(vec![
                    "ufw".to_string(),
                    "--force".to_string(),
                    "enable".to_string(),
                ]);
            }
            FirewallBackend::Auto => unreachable!(),
        },
    }
    Ok(commands)
}

fn request_digest(request: &FirewallRequest, backend: FirewallBackend) -> Result<String> {
    let mut canonical = request.clone();
    canonical.backend = backend;
    canonical.exclusive = false;
    canonical.allow_lockout = false;
    canonical.ssh_connection = None;
    canonical.inspection = None;
    let bytes = serde_json::to_vec(&(backend, canonical))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn ensure_firewalld_running() -> Result<PathBuf> {
    let firewall_cmd =
        crate::file::which("firewall-cmd").ok_or_else(|| eyre!("firewall-cmd not found"))?;
    if !backend_active(FirewallBackend::Firewalld) {
        let systemctl = crate::file::which("systemctl")
            .ok_or_else(|| eyre!("starting firewalld requires systemctl"))?;
        run(&systemctl, &["enable", "--now", "firewalld.service"])?;
    }
    Ok(firewall_cmd)
}

fn read_state() -> Result<Option<FirewallStateFile>> {
    let path = Path::new(STATE_PATH);
    if !path.exists() {
        return Ok(None);
    }
    let mut body = vec![];
    fs::File::open(path)?.read_to_end(&mut body)?;
    Ok(Some(serde_json::from_slice(&body)?))
}

fn write_state(state: &FirewallStateFile) -> Result<()> {
    let body = serde_json::to_vec_pretty(state)?;
    write_atomic(Path::new(STATE_PATH), &body, 0o600)
}

fn remove_state() -> Result<()> {
    match fs::remove_file(STATE_PATH) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn write_atomic(path: &Path, body: &[u8], mode: u32) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("{} has no parent", path.display()))?;
    fs::create_dir_all(parent)?;
    let prefix = format!(
        ".{}.mise-",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    let mut temporary = tempfile::Builder::new()
        .prefix(&prefix)
        .tempfile_in(parent)?;
    std::io::Write::write_all(&mut temporary, body)?;
    temporary.as_file().sync_all()?;
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(mode))?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn command_output(program: &str, args: &[&str]) -> Result<Output> {
    let path = crate::file::which(program).ok_or_else(|| eyre!("{program} not found"))?;
    command_output_path(&path, args)
}

fn command_output_path(program: &Path, args: &[&str]) -> Result<Output> {
    Ok(Command::new(program).args(args).output()?)
}

fn run(program: &Path, args: &[&str]) -> Result<()> {
    let output = command_output_path(program, args)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_error(&program.display().to_string(), args, &output))
    }
}

fn run_owned(program: &Path, args: &[String]) -> Result<()> {
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    run(program, &refs)
}

fn run_with_stdin(program: &Path, args: &[&str], input: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child.stdin.take().expect("piped stdin").write_all(input)?;
    let output = child.wait_with_output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_error(&program.display().to_string(), args, &output))
    }
}

fn command_error(program: &str, args: &[&str], output: &Output) -> eyre::Report {
    eyre!(
        "{} failed with {}: {}",
        shell_words::join(
            std::iter::once(program.to_string()).chain(args.iter().map(|arg| (*arg).to_string()))
        ),
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("firewall rule name '{name}' must contain only ASCII letters, numbers, '-' or '_'");
    }
    Ok(())
}

fn validate_interface(interface: &str) -> Result<String> {
    if interface.is_empty()
        || interface.len() > 15
        || !interface
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        bail!("firewall interface '{interface}' is invalid");
    }
    Ok(interface.to_string())
}

fn parse_ssh_connection(value: &str) -> Result<SshConnection> {
    let fields = value.split_ascii_whitespace().collect::<Vec<_>>();
    if fields.len() != 4 {
        bail!("SSH_CONNECTION must contain client address/port and server address/port");
    }
    Ok(SshConnection {
        peer: fields[0].parse()?,
        server: fields[2].parse()?,
        server_port: fields[3].parse()?,
    })
}

/// Detect an sshd ancestor when SSH_CONNECTION was stripped by sudo, env -i,
/// or a wrapper. `None` fails closed because ancestry could not be inspected.
fn ssh_ancestor_present() -> Option<bool> {
    let mut pid = std::process::id();
    let mut visited = HashSet::new();
    for _ in 0..64 {
        if !visited.insert(pid) {
            return None;
        }
        let comm = fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        let comm = comm.trim();
        if comm == "sshd" || comm == "sshd-session" || comm.starts_with("sshd:") {
            return Some(true);
        }
        if pid <= 1 {
            return Some(false);
        }
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        pid = proc_parent_pid(&stat)?;
        if pid == 0 {
            return Some(false);
        }
    }
    None
}

fn proc_parent_pid(stat: &str) -> Option<u32> {
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct FirewallWrapper {
        firewall: FirewallTomlConfig,
    }

    fn request_with_ssh(source: Option<&str>) -> FirewallRequest {
        FirewallRequest {
            backend: FirewallBackend::Nftables,
            state: FirewallState::Enabled,
            default_incoming: FirewallPolicy::Deny,
            default_outgoing: FirewallPolicy::Allow,
            exclusive: false,
            allow_lockout: false,
            rules: vec![FirewallRule {
                name: "ssh".to_string(),
                state: FirewallRuleState::Present,
                direction: FirewallDirection::Incoming,
                action: FirewallAction::Allow,
                port: Some(FirewallPort { start: 22, end: 22 }),
                protocol: Some(FirewallProtocol::Tcp),
                source: source.map(|source| source.parse().unwrap()),
                destination: None,
                interface: None,
            }],
            ssh_connection: Some(SshConnection {
                peer: "203.0.113.10".parse().unwrap(),
                server: "192.0.2.20".parse().unwrap(),
                server_port: 22,
            }),
            inspection: None,
        }
    }

    #[test]
    fn ssh_lockout_guard_accepts_covering_rule() {
        request_with_ssh(Some("203.0.113.0/24"))
            .validate_safety()
            .unwrap();
    }

    #[test]
    fn ssh_lockout_guard_rejects_uncovered_peer() {
        let error = request_with_ssh(Some("198.51.100.0/24"))
            .validate_safety()
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("refusing firewall default incoming")
        );
    }

    #[test]
    fn ssh_lockout_guard_rejects_interface_constrained_coverage() {
        let mut request = request_with_ssh(Some("203.0.113.0/24"));
        request.rules[0].interface = Some("eth1".to_string());
        assert!(request.validate_safety().is_err());
    }

    #[test]
    fn ssh_lockout_guard_distinguishes_local_and_stripped_ssh_context() {
        let mut request = request_with_ssh(None);
        request.ssh_connection = None;
        request
            .validate_safety_with_rules(
                &request.rules,
                Some(FirewallBackend::Nftables),
                Some(false),
            )
            .unwrap();
        assert!(
            request
                .validate_safety_with_rules(
                    &request.rules,
                    Some(FirewallBackend::Nftables),
                    Some(true),
                )
                .unwrap_err()
                .to_string()
                .contains("SSH-derived process without SSH_CONNECTION")
        );
        assert!(
            request
                .validate_safety_with_rules(&request.rules, Some(FirewallBackend::Nftables), None)
                .is_err()
        );
    }

    #[test]
    fn ssh_lockout_guard_rejects_earlier_covering_block_rule() {
        let mut request = request_with_ssh(None);
        let mut block = request.rules[0].clone();
        block.name = "block-ssh".to_string();
        block.action = FirewallAction::Deny;
        request.rules.insert(0, block);
        assert!(
            request
                .validate_safety()
                .unwrap_err()
                .to_string()
                .contains("blocking rule 'block-ssh' precedes")
        );

        request.backend = FirewallBackend::Ufw;
        request.validate_safety().unwrap();
    }

    #[test]
    fn parses_proc_parent_pid_after_complex_process_names() {
        assert_eq!(
            proc_parent_pid("123 (name with ) paren) S 42 1 2 3"),
            Some(42)
        );
    }

    #[test]
    fn layered_config_inherits_scalars_and_merges_rules_by_name() {
        let inherited: FirewallWrapper = toml::from_str(
            r#"
                [firewall]
                backend = "nftables"
                default_incoming = "reject"

                [[firewall.rules]]
                name = "ssh"
                port = 22
                protocol = "tcp"
                action = "allow"
            "#,
        )
        .unwrap();
        let local: FirewallWrapper = toml::from_str(
            r#"
                [firewall]
                state = "disabled"
                exclusive = true

                [[firewall.rules]]
                name = "ssh"
                port = 2222
                protocol = "tcp"
                action = "allow"

                [[firewall.rules]]
                name = "https"
                port = 443
                protocol = "tcp"
                action = "allow"
            "#,
        )
        .unwrap();

        let request = FirewallRequest::from_toml(
            merge_toml_config(inherited.firewall, local.firewall).unwrap(),
        )
        .unwrap();
        assert_eq!(request.backend, FirewallBackend::Nftables);
        assert_eq!(request.state, FirewallState::Disabled);
        assert_eq!(request.default_incoming, FirewallPolicy::Reject);
        assert!(request.exclusive);
        assert_eq!(request.rules.len(), 2);
        assert_eq!(request.rules[0].name, "ssh");
        assert_eq!(request.rules[0].port.unwrap().start, 2222);
        assert_eq!(request.rules[1].name, "https");
    }

    #[test]
    fn effective_request_preserves_undeclared_rules_without_exclusive_ownership() {
        let current = request_with_ssh(None);
        let state = FirewallStateFile {
            backend: FirewallBackend::Nftables,
            digest: "old".to_string(),
            live_fingerprint: None,
            request: current.clone(),
        };
        let mut desired = current;
        desired.rules.clear();
        desired.ssh_connection = None;
        let effective = effective_request(&desired, Some(&state));
        assert_eq!(effective.rules.len(), 1);
        assert_eq!(effective.rules[0].name, "ssh");
    }

    #[test]
    fn nftables_rules_are_isolated_and_allow_before_default_policy() {
        let request = request_with_ssh(None);
        let rendered = render_nftables(&request, "abc", false);
        assert!(rendered.contains("add table inet mise_bootstrap"));
        assert!(rendered.contains("policy drop"));
        assert!(rendered.contains("tcp dport 22 accept comment \"mise:ssh\""));
        assert!(rendered.contains("mise-bootstrap:abc"));
    }

    #[test]
    fn parses_documented_firewall_config() {
        let parsed: FirewallWrapper = toml::from_str(
            r#"
                [firewall]
                backend = "auto"
                state = "enabled"
                default_incoming = "deny"
                default_outgoing = "allow"
                allow_lockout = true

                [[firewall.rules]]
                name = "web"
                port = "8000-8010"
                protocol = "tcp"
                source = "2001:db8::/32"
                action = "allow"
            "#,
        )
        .unwrap();
        let request = FirewallRequest::from_toml(parsed.firewall).unwrap();
        assert_eq!(request.rules.len(), 1);
        assert_eq!(
            request.rules[0].port,
            Some(FirewallPort {
                start: 8000,
                end: 8010
            })
        );
        assert_eq!(
            request.rules[0].source,
            Some("2001:db8::/32".parse().unwrap())
        );
    }

    #[test]
    fn rejects_duplicate_names_and_ports_without_protocols() {
        let duplicate: FirewallWrapper = toml::from_str(
            r#"
                [firewall]
                [[firewall.rules]]
                name = "web"
                [[firewall.rules]]
                name = "web"
            "#,
        )
        .unwrap();
        assert!(FirewallRequest::from_toml(duplicate.firewall).is_err());

        let missing_protocol: FirewallWrapper = toml::from_str(
            r#"
                [firewall]
                [[firewall.rules]]
                name = "web"
                port = 443
            "#,
        )
        .unwrap();
        assert!(FirewallRequest::from_toml(missing_protocol.firewall).is_err());
    }

    #[test]
    fn renders_backend_native_actions() {
        let mut request = request_with_ssh(None);
        request.rules[0].action = FirewallAction::Deny;
        assert!(render_nftables_rule(&request.rules[0]).contains(" drop comment"));
        assert!(render_firewalld_rule(&request.rules[0]).ends_with(" drop"));
        assert_eq!(render_ufw_rule(&request.rules[0])[0], "deny");
    }

    #[test]
    fn matches_ufw_canonical_rule_output_semantically() {
        let request = request_with_ssh(None);
        let compact = shell_words::split("allow 22/tcp comment 'mise:ssh'").unwrap();
        assert!(ufw_rule_matches(&compact, &request.rules[0]));

        let mut outgoing = request.rules[0].clone();
        outgoing.name = "dns".to_string();
        outgoing.direction = FirewallDirection::Outgoing;
        outgoing.protocol = Some(FirewallProtocol::Udp);
        outgoing.port = Some(FirewallPort { start: 53, end: 53 });
        outgoing.source = None;
        outgoing.destination = Some("1.1.1.1/32".parse().unwrap());
        let expanded =
            shell_words::split("allow out to 1.1.1.1 port 53 proto udp comment 'mise:dns'")
                .unwrap();
        assert!(ufw_rule_matches(&expanded, &outgoing));
    }

    #[test]
    fn renders_reject_defaults_as_terminal_nft_rules() {
        let mut request = request_with_ssh(None);
        request.default_incoming = FirewallPolicy::Reject;
        request.default_outgoing = FirewallPolicy::Reject;
        let rendered = render_nftables(&request, "abc", false);
        assert!(rendered.contains("add rule inet mise_bootstrap input reject"));
        assert!(rendered.contains("add rule inet mise_bootstrap output reject"));
    }

    #[test]
    fn explicit_absence_removes_a_previously_managed_rule() {
        let current = request_with_ssh(None);
        let state = FirewallStateFile {
            backend: FirewallBackend::Nftables,
            digest: "old".to_string(),
            live_fingerprint: None,
            request: current.clone(),
        };
        let mut desired = current;
        desired.ssh_connection = None;
        desired.rules[0].state = FirewallRuleState::Absent;
        let effective = effective_request(&desired, Some(&state));
        assert!(effective.rules.is_empty());
    }

    #[test]
    fn absent_parent_makes_nested_rules_absent_in_the_plan() {
        let mut request = request_with_ssh(None);
        request.state = FirewallState::Absent;
        request.ssh_connection = None;
        request.inspection = Some(FirewallInspection {
            backend: Some(FirewallBackend::Nftables),
            managed: false,
            exact: true,
            active: false,
            reason: None,
            current_rules: vec![],
        });
        let plans = request.plans();
        assert!(plans.iter().all(|plan| plan.action == ResourceAction::Noop));
        assert_eq!(plans[0].current, "absent");
        assert_eq!(plans[1].desired, "absent");
    }

    #[test]
    fn backend_capability_mismatches_fail_closed() {
        let mut request = request_with_ssh(None);
        request.rules[0].interface = Some("eth0".to_string());
        assert!(validate_backend_request(&request, FirewallBackend::Firewalld).is_err());
        request.rules[0].interface = None;
        request.rules[0].protocol = Some(FirewallProtocol::Sctp);
        assert!(validate_backend_request(&request, FirewallBackend::Ufw).is_err());
    }

    #[test]
    fn backend_capability_checks_include_inherited_state_rules() {
        let mut inherited = request_with_ssh(None);
        inherited.rules[0].interface = Some("eth0".to_string());
        let state = FirewallStateFile {
            backend: FirewallBackend::Firewalld,
            digest: "old".to_string(),
            live_fingerprint: None,
            request: inherited,
        };
        let mut desired = request_with_ssh(None);
        desired.rules.clear();
        desired.ssh_connection = None;
        let effective = effective_request(&desired, Some(&state));

        assert!(validate_backend_request(&desired, FirewallBackend::Firewalld).is_ok());
        assert!(
            validate_effective_backend_request(&desired, &effective, FirewallBackend::Firewalld)
                .is_err()
        );
    }
}
