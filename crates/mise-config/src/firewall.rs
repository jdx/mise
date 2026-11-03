//! `[bootstrap.linux.firewall]`. Only Linux applies it, but every platform parses it the same
//! way, so a shared config is checked everywhere and the schema does not depend on the host.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FirewallState {
    #[default]
    Enabled,
    Disabled,
    Absent,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FirewallRuleState {
    #[default]
    Present,
    Absent,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FirewallDirection {
    #[default]
    Incoming,
    Outgoing,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
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

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum FirewallPortToml {
    Single(#[schemars(range(min = 1))] u16),
    Range(#[schemars(regex(pattern = r"^[0-9]+[-:][0-9]+$"))] String),
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct FirewallRuleTomlConfig {
    /// stable managed rule identifier
    #[schemars(length(min = 1, max = 64), regex(pattern = r"^[A-Za-z0-9_-]+$"))]
    pub name: String,
    /// present adds the rule; absent removes a rule mise added earlier
    #[serde(default)]
    pub state: FirewallRuleState,
    /// traffic direction the rule matches
    #[serde(default)]
    pub direction: FirewallDirection,
    /// what to do with matching traffic; limit needs incoming TCP and is not available on firewalld
    #[serde(default)]
    pub action: FirewallAction,
    /// single port number or inclusive range
    pub port: Option<FirewallPortToml>,
    /// protocol to match; any when unset, and UFW supports only tcp and udp
    pub protocol: Option<FirewallProtocol>,
    /// source IPv4 or IPv6 CIDR network
    pub source: Option<String>,
    /// destination IPv4 or IPv6 CIDR network
    pub destination: Option<String>,
    /// network interface (nftables and UFW only)
    #[schemars(length(min = 1, max = 15), regex(pattern = r"^[A-Za-z0-9_.:-]+$"))]
    pub interface: Option<String>,
}

// The defaults below are what the firewall module uses when a key is absent.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct FirewallTomlConfig {
    /// firewall backend; auto reuses managed or active host infrastructure
    #[schemars(extend("default" = "auto"))]
    pub backend: Option<FirewallBackend>,
    /// desired lifecycle of mise-managed firewall state
    #[schemars(extend("default" = "enabled"))]
    pub state: Option<FirewallState>,
    /// default action for incoming traffic
    #[schemars(extend("default" = "deny"))]
    pub default_incoming: Option<FirewallPolicy>,
    /// default action for outgoing traffic
    #[schemars(extend("default" = "allow"))]
    pub default_outgoing: Option<FirewallPolicy>,
    /// treat the declared rules as the complete owned ruleset; UFW reset also removes unrelated UFW rules
    #[schemars(extend("default" = false))]
    pub exclusive: Option<bool>,
    /// allow default-deny or reject over SSH without a rule covering the connected peer
    #[schemars(extend("default" = false))]
    pub allow_lockout: Option<bool>,
    /// named firewall rules reconciled without deleting undeclared managed rules
    #[serde(default)]
    pub rules: Vec<FirewallRuleTomlConfig>,
}
