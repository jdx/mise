//! `[bootstrap.linux.firewall]`. Only Linux applies it, but every platform parses it the same
//! way, so a shared config is checked everywhere and the schema does not depend on the host.

use serde::{Deserialize, Serialize};

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
    pub fn program(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Ufw => Some("ufw"),
            Self::Firewalld => Some("firewall-cmd"),
            Self::Nftables => Some("nft"),
        }
    }

    pub fn label(self) -> &'static str {
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
    pub fn nft(self) -> &'static str {
        match self {
            Self::Allow => "accept",
            Self::Deny | Self::Reject => "drop",
        }
    }

    pub fn firewalld(self) -> &'static str {
        match self {
            Self::Allow => "ACCEPT",
            Self::Deny => "DROP",
            Self::Reject => "REJECT",
        }
    }

    pub fn ufw(self) -> &'static str {
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
    Limit,
    Deny,
    Reject,
}

impl FirewallAction {
    pub fn ufw(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Limit => "limit",
            Self::Deny => "deny",
            Self::Reject => "reject",
        }
    }

    pub fn nft(self) -> &'static str {
        match self {
            Self::Allow => "accept",
            Self::Limit => unreachable!("nftables limit rules use a dedicated renderer"),
            Self::Deny => "drop",
            Self::Reject => "reject",
        }
    }

    pub fn firewalld(self) -> &'static str {
        match self {
            Self::Allow => "accept",
            Self::Limit => unreachable!("firewalld limit rules are rejected during validation"),
            Self::Deny => "drop",
            Self::Reject => "reject",
        }
    }

    pub fn preserves_access(self) -> bool {
        matches!(self, Self::Allow | Self::Limit)
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
    pub fn as_str(self) -> &'static str {
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
