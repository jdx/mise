use eyre::{Result, bail};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::types::{ProjectLocalState, ProjectState, Resource, ResourceStatus, ProviderAccount};

const STATE_DIR: &str = ".projects";
const STATE_FILE: &str = "state.json";
const LOCAL_STATE_FILE: &str = "state.local.json";

/// Returns the .projects directory path relative to the given root.
pub fn projects_dir(root: &Path) -> PathBuf {
    root.join(STATE_DIR)
}

/// Check whether a project is initialized in the given directory.
pub fn is_initialized(root: &Path) -> bool {
    projects_dir(root).join(STATE_FILE).exists()
}

/// Load project state from disk.
pub fn load_state(root: &Path) -> Result<ProjectState> {
    let path = projects_dir(root).join(STATE_FILE);
    if !path.exists() {
        bail!(
            "No project initialized in {}. Run `mise sleeves init` first.",
            root.display()
        );
    }
    let data = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data)?)
}

/// Save project state to disk.
pub fn save_state(root: &Path, state: &ProjectState) -> Result<()> {
    let dir = projects_dir(root);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(STATE_FILE);
    let data = serde_json::to_string_pretty(state)?;
    std::fs::write(&path, data)?;
    Ok(())
}

/// Load local state (resource IDs, etc.)
pub fn load_local_state(root: &Path) -> Result<ProjectLocalState> {
    let path = projects_dir(root).join(LOCAL_STATE_FILE);
    if !path.exists() {
        return Ok(ProjectLocalState::default());
    }
    let data = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data)?)
}

/// Save local state
pub fn save_local_state(root: &Path, state: &ProjectLocalState) -> Result<()> {
    let dir = projects_dir(root);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(LOCAL_STATE_FILE);
    let data = serde_json::to_string_pretty(state)?;
    std::fs::write(&path, data)?;
    Ok(())
}

/// Initialize a new project
pub fn init_project(root: &Path, name: &str) -> Result<ProjectState> {
    if is_initialized(root) {
        bail!(
            "Project already initialized in {}. See `mise sleeves status`.",
            root.display()
        );
    }
    let state = ProjectState {
        name: name.to_string(),
        account_id: None,
        providers: vec![],
        resources: vec![],
    };
    save_state(root, &state)?;
    save_local_state(root, &ProjectLocalState::default())?;
    Ok(state)
}

/// Link a provider account to the project
pub fn link_provider(root: &Path, provider: &str) -> Result<ProviderAccount> {
    let mut state = load_state(root)?;
    if state.providers.iter().any(|p| p.provider == provider) {
        bail!("Provider '{}' is already linked to this project.", provider);
    }
    let account = ProviderAccount {
        provider: provider.to_string(),
        account_id: format!("acct_{provider}_{}", crate::rand::random_string(8)),
        display_name: format!("{provider} account"),
        linked_at: chrono::Utc::now().to_rfc3339(),
    };
    state.providers.push(account.clone());
    save_state(root, &state)?;
    Ok(account)
}

/// Add a service and provision a resource
pub fn add_service(root: &Path, provider: &str, service: &str) -> Result<Resource> {
    let mut state = load_state(root)?;

    // Auto-link provider if not already linked
    if !state.providers.iter().any(|p| p.provider == provider) {
        let account = ProviderAccount {
            provider: provider.to_string(),
            account_id: format!("acct_{provider}_{}", crate::rand::random_string(8)),
            display_name: format!("{provider} account"),
            linked_at: chrono::Utc::now().to_rfc3339(),
        };
        state.providers.push(account);
    }

    // Check for duplicate resource
    if state
        .resources
        .iter()
        .any(|r| r.provider == provider && r.service == service && r.status != ResourceStatus::Removed)
    {
        bail!(
            "Service '{}/{}' is already provisioned. Use `mise sleeves upgrade` to change its tier.",
            provider,
            service
        );
    }

    let env_vars = generate_env_vars(provider, service);
    let resource_id = format!("res_{}_{}", crate::rand::random_string(6), service);

    let resource = Resource {
        name: format!("{}-{}", provider, service),
        provider: provider.to_string(),
        service: service.to_string(),
        resource_id: resource_id.clone(),
        tier: "free".to_string(),
        status: ResourceStatus::Active,
        created_at: chrono::Utc::now().to_rfc3339(),
        env_vars: env_vars.clone(),
    };

    state.resources.push(resource.clone());
    save_state(root, &state)?;

    // Save resource ID to local state
    let mut local = load_local_state(root)?;
    local
        .resource_ids
        .insert(format!("{}/{}", provider, service), resource_id);
    save_local_state(root, &local)?;

    // Sync env vars to .env
    sync_env_to_dotenv(root, &state)?;

    Ok(resource)
}

/// Remove a service/resource
pub fn remove_service(root: &Path, identifier: &str) -> Result<String> {
    let mut state = load_state(root)?;

    let idx = find_resource_index(&state, identifier)?;
    let resource = &mut state.resources[idx];
    let name = resource.name.clone();
    resource.status = ResourceStatus::Removed;

    save_state(root, &state)?;
    sync_env_to_dotenv(root, &state)?;
    Ok(name)
}

/// Rotate credentials for a service
pub fn rotate_credentials(root: &Path, identifier: &str) -> Result<Resource> {
    let mut state = load_state(root)?;
    let idx = find_resource_index(&state, identifier)?;

    let resource = &mut state.resources[idx];
    resource.env_vars = generate_env_vars(&resource.provider, &resource.service);

    let updated = resource.clone();
    save_state(root, &state)?;
    sync_env_to_dotenv(root, &state)?;
    Ok(updated)
}

/// Upgrade the tier of a service
pub fn upgrade_service(root: &Path, identifier: &str, new_tier: &str) -> Result<Resource> {
    let mut state = load_state(root)?;
    let idx = find_resource_index(&state, identifier)?;

    state.resources[idx].tier = new_tier.to_string();
    let updated = state.resources[idx].clone();
    save_state(root, &state)?;
    Ok(updated)
}

/// Collect all env vars from active resources
pub fn collect_env_vars(state: &ProjectState) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    for r in &state.resources {
        if r.status == ResourceStatus::Active {
            vars.extend(r.env_vars.clone());
        }
    }
    vars
}

/// Sync env vars to the .env file (merge with existing)
pub fn sync_env_to_dotenv(root: &Path, state: &ProjectState) -> Result<()> {
    let env_path = root.join(".env");
    let mut existing = BTreeMap::new();

    if env_path.exists() {
        let content = std::fs::read_to_string(&env_path)?;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                existing.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }

    // Merge new vars (overwrite provider-managed vars)
    let new_vars = collect_env_vars(state);
    existing.extend(new_vars);

    let mut output = String::new();
    output.push_str("# Managed by mise sleeves — do not edit managed entries manually\n");
    for (k, v) in &existing {
        output.push_str(&format!("{}={}\n", k, v));
    }

    std::fs::write(&env_path, output)?;
    Ok(())
}

/// Generate placeholder env vars for a provider/service combo
fn generate_env_vars(provider: &str, service: &str) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    let prefix = provider.to_uppercase();
    let secret = crate::rand::random_string(32);

    match (provider, service) {
        ("vercel", "project") => {
            vars.insert(
                "VERCEL_PROJECT_ID".into(),
                format!("prj_{}", crate::rand::random_string(16)),
            );
            vars.insert("VERCEL_TOKEN".into(), format!("vt_{secret}"));
        }
        ("supabase", "database") => {
            vars.insert(
                "SUPABASE_DATABASE_URL".into(),
                format!(
                    "postgresql://postgres:{}@db.supabase.co:5432/postgres",
                    crate::rand::random_string(24)
                ),
            );
            vars.insert(
                "SUPABASE_ANON_KEY".into(),
                format!("eyJ_{}", crate::rand::random_string(40)),
            );
            vars.insert(
                "SUPABASE_SERVICE_ROLE_KEY".into(),
                format!("eyJ_{}", crate::rand::random_string(40)),
            );
        }
        ("supabase", "auth") => {
            vars.insert(
                "SUPABASE_AUTH_URL".into(),
                "https://auth.supabase.co".into(),
            );
        }
        ("neon", "database") => {
            vars.insert(
                "NEON_DATABASE_URL".into(),
                format!(
                    "postgresql://user:{}@ep-neon.us-east-2.aws.neon.tech/neondb",
                    crate::rand::random_string(24)
                ),
            );
        }
        ("planetscale", "database") => {
            vars.insert(
                "PLANETSCALE_DATABASE_URL".into(),
                format!(
                    "mysql://user:{}@aws.connect.psdb.cloud/mydb?sslaccept=strict",
                    crate::rand::random_string(24)
                ),
            );
        }
        ("turso", "database") => {
            vars.insert(
                "TURSO_DATABASE_URL".into(),
                format!("libsql://mydb-{}.turso.io", crate::rand::random_string(8)),
            );
            vars.insert(
                "TURSO_AUTH_TOKEN".into(),
                format!("eyJ_{}", crate::rand::random_string(40)),
            );
        }
        ("chroma", "database") => {
            vars.insert(
                "CHROMA_API_KEY".into(),
                format!("ck_{}", crate::rand::random_string(24)),
            );
            vars.insert("CHROMA_HOST".into(), "https://api.trychroma.com".into());
        }
        ("clerk", "auth") => {
            vars.insert(
                "CLERK_SECRET_KEY".into(),
                format!("sk_live_{secret}"),
            );
            vars.insert(
                "NEXT_PUBLIC_CLERK_PUBLISHABLE_KEY".into(),
                format!("pk_live_{}", crate::rand::random_string(32)),
            );
        }
        ("posthog", "analytics") => {
            vars.insert(
                "POSTHOG_PROJECT_API_KEY".into(),
                format!("phc_{}", crate::rand::random_string(32)),
            );
            vars.insert("POSTHOG_HOST".into(), "https://us.i.posthog.com".into());
        }
        ("railway", "project") => {
            vars.insert(
                "RAILWAY_PROJECT_ID".into(),
                format!("rw_{}", crate::rand::random_string(16)),
            );
            vars.insert(
                "RAILWAY_TOKEN".into(),
                format!("rw_tok_{secret}"),
            );
        }
        ("railway", "database") => {
            vars.insert(
                "RAILWAY_DATABASE_URL".into(),
                format!(
                    "postgresql://postgres:{}@containers.railway.app:5432/railway",
                    crate::rand::random_string(24)
                ),
            );
        }
        ("runloop", "sandbox") => {
            vars.insert(
                "RUNLOOP_API_KEY".into(),
                format!("rl_{secret}"),
            );
        }
        _ => {
            // Generic fallback
            let key = format!(
                "{}_{}_KEY",
                prefix,
                service.to_uppercase().replace('-', "_")
            );
            vars.insert(key, format!("key_{secret}"));
        }
    }
    vars
}

/// Find a resource index by "provider/service" or resource name
fn find_resource_index(state: &ProjectState, identifier: &str) -> Result<usize> {
    // Try "provider/service" format
    if let Some((provider, service)) = identifier.split_once('/') {
        if let Some(idx) = state.resources.iter().position(|r| {
            r.provider == provider && r.service == service && r.status != ResourceStatus::Removed
        }) {
            return Ok(idx);
        }
    }

    // Try by resource name
    if let Some(idx) = state.resources.iter().position(|r| {
        r.name == identifier && r.status != ResourceStatus::Removed
    }) {
        return Ok(idx);
    }

    bail!(
        "Resource '{}' not found. Run `mise sleeves status` to see current resources.",
        identifier
    );
}
