use eyre::Result;
use std::path::Path;

use super::catalog;
use super::state;
use super::types::*;

/// The Sleeves server handles all commands, operating on the local project state.
/// In a production deployment this would be a remote API; here it runs in-process.
pub struct SleevesServer<'a> {
    root: &'a Path,
}

impl<'a> SleevesServer<'a> {
    pub fn new(root: &'a Path) -> Self {
        Self { root }
    }

    /// Initialize a new project
    pub fn init(&self, name: &str) -> Result<ProjectState> {
        state::init_project(self.root, name)
    }

    /// Get project status
    pub fn status(&self) -> Result<ProjectState> {
        state::load_state(self.root)
    }

    /// Get health status for all resources
    pub fn health(&self) -> Result<Vec<HealthStatus>> {
        let st = state::load_state(self.root)?;
        Ok(st
            .resources
            .iter()
            .filter(|r| r.status != ResourceStatus::Removed)
            .map(|r| HealthStatus {
                resource_name: r.name.clone(),
                healthy: r.status == ResourceStatus::Active,
                message: format!("{} is {}", r.name, r.status),
            })
            .collect())
    }

    /// Browse the service catalog
    pub fn catalog(&self, filter: Option<&str>) -> Vec<ProviderInfo> {
        match filter {
            None => catalog::get_catalog(),
            Some(f) => {
                // Try provider name first
                if let Some(p) = catalog::get_provider(f) {
                    vec![p]
                } else {
                    // Try category
                    let by_cat = catalog::get_by_category(f);
                    if !by_cat.is_empty() {
                        by_cat
                    } else {
                        vec![]
                    }
                }
            }
        }
    }

    /// Link a provider account
    pub fn link(&self, provider: &str) -> Result<ProviderAccount> {
        // Validate provider exists in catalog
        if catalog::get_provider(provider).is_none() {
            eyre::bail!(
                "Unknown provider '{}'. Run `mise sleeves catalog` to see available providers.",
                provider
            );
        }
        state::link_provider(self.root, provider)
    }

    /// Add a service (provision a resource)
    pub fn add(&self, provider: &str, service: &str) -> Result<Resource> {
        // Validate service exists in catalog
        if catalog::find_service(provider, service).is_none() {
            eyre::bail!(
                "Unknown service '{}/{}'. Run `mise sleeves catalog {}` to see available services.",
                provider,
                service,
                provider
            );
        }
        state::add_service(self.root, provider, service)
    }

    /// Remove a service
    pub fn remove(&self, identifier: &str) -> Result<String> {
        state::remove_service(self.root, identifier)
    }

    /// Rotate credentials
    pub fn rotate(&self, identifier: &str) -> Result<Resource> {
        state::rotate_credentials(self.root, identifier)
    }

    /// Upgrade a service tier
    pub fn upgrade(&self, identifier: &str, new_tier: &str) -> Result<Resource> {
        state::upgrade_service(self.root, identifier, new_tier)
    }

    /// List environment variables
    pub fn env(&self) -> Result<Vec<EnvVar>> {
        let st = state::load_state(self.root)?;
        let mut vars = Vec::new();
        for r in &st.resources {
            if r.status != ResourceStatus::Removed {
                for (key, val) in &r.env_vars {
                    vars.push(EnvVar {
                        key: key.clone(),
                        provider: r.provider.clone(),
                        service: r.service.clone(),
                        masked_value: mask_value(val),
                    });
                }
            }
        }
        vars.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(vars)
    }

    /// Sync env vars to .env
    pub fn env_pull(&self) -> Result<usize> {
        let st = state::load_state(self.root)?;
        state::sync_env_to_dotenv(self.root, &st)?;
        Ok(state::collect_env_vars(&st).len())
    }

    /// Get billing info (stub)
    pub fn billing_show(&self) -> Result<Option<PaymentMethod>> {
        // In a real implementation this would query the account API
        Ok(None)
    }

    /// Add billing method (stub — would open browser in production)
    pub fn billing_add(&self) -> Result<PaymentMethod> {
        Ok(PaymentMethod {
            method_type: "card".into(),
            last_four: "4242".into(),
            expiry: "12/28".into(),
        })
    }

    /// Generate LLM context combining project + provider context
    pub fn llm_context(&self) -> Result<String> {
        let st = state::load_state(self.root)?;
        let mut ctx = String::new();
        ctx.push_str("# Project Context\n\n");
        ctx.push_str(&format!("Project: {}\n", st.name));
        ctx.push_str(&format!("Providers: {}\n", st.providers.iter().map(|p| p.provider.as_str()).collect::<Vec<_>>().join(", ")));
        ctx.push_str(&format!("Resources: {}\n\n", st.resources.iter().filter(|r| r.status == ResourceStatus::Active).count()));

        for r in &st.resources {
            if r.status == ResourceStatus::Active {
                ctx.push_str(&format!(
                    "## {}/{} (tier: {})\n",
                    r.provider, r.service, r.tier
                ));
                ctx.push_str("Environment variables:\n");
                for key in r.env_vars.keys() {
                    ctx.push_str(&format!("  - {}\n", key));
                }
                ctx.push('\n');
            }
        }

        // Write to file
        let ctx_path = state::projects_dir(self.root).join("llm-context.md");
        std::fs::write(&ctx_path, &ctx)?;

        Ok(ctx)
    }
}

fn mask_value(val: &str) -> String {
    if val.len() <= 8 {
        "••••••••".into()
    } else {
        let visible = &val[..4];
        format!("{}••••••••", visible)
    }
}
