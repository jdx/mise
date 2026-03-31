use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A provider account (e.g., Vercel, Supabase, Clerk)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderAccount {
    pub provider: String,
    pub account_id: String,
    pub display_name: String,
    pub linked_at: String,
}

/// A provisioned resource instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub name: String,
    pub provider: String,
    pub service: String,
    pub resource_id: String,
    pub tier: String,
    pub status: ResourceStatus,
    pub created_at: String,
    pub env_vars: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ResourceStatus {
    Active,
    Provisioning,
    Error,
    Removed,
}

impl std::fmt::Display for ResourceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Provisioning => write!(f, "provisioning"),
            Self::Error => write!(f, "error"),
            Self::Removed => write!(f, "removed"),
        }
    }
}

/// Project state persisted to .projects/state.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectState {
    pub name: String,
    pub account_id: Option<String>,
    pub providers: Vec<ProviderAccount>,
    pub resources: Vec<Resource>,
}

/// Local state persisted to .projects/state.local.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectLocalState {
    pub resource_ids: BTreeMap<String, String>,
}

/// Billing / payment method info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentMethod {
    pub method_type: String,
    pub last_four: String,
    pub expiry: String,
}

/// A service offering from a provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceOffering {
    pub provider: String,
    pub service: String,
    pub category: String,
    pub description: String,
    pub tiers: Vec<ServiceTier>,
}

/// A pricing tier for a service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceTier {
    pub name: String,
    pub price: String,
    pub features: Vec<String>,
}

/// Provider metadata for the catalog
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub name: String,
    pub categories: Vec<String>,
    pub services: Vec<ServiceOffering>,
}

/// Health status for a resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub resource_name: String,
    pub healthy: bool,
    pub message: String,
}

/// Env var entry for display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVar {
    pub key: String,
    pub provider: String,
    pub service: String,
    pub masked_value: String,
}

/// Structured JSON output wrapper
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonOutput<T: Serialize> {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T: Serialize> JsonOutput<T> {
    pub fn success(data: T) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    #[allow(dead_code)]
    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}
