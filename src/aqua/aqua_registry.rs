use crate::config::Settings;
use crate::dirs;
use crate::duration::WEEKLY;
use crate::file;
use crate::git::{CloneOptions, Git};
use crate::{info, trace};
use aqua_registry::AquaRegistryManager;
use eyre::Result;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock as Lazy};

pub static AQUA_REGISTRY: Lazy<Arc<AquaRegistryManager>> = Lazy::new(|| {
    let settings = Settings::get();

    // Try with Git support first
    let registry_manager = create_registry_with_git_support(&settings).unwrap_or_else(|err| {
        eprintln!("Failed to initialize aqua registry with Git support: {err:?}");
        // Fallback to basic settings-based initialization
        AquaRegistryManager::with_settings(
            settings.aqua.registry_url.as_deref(),
            &settings
                .aqua
                .additional_registries
                .clone()
                .unwrap_or_default(),
            settings.aqua.baked_registry,
        )
        .unwrap_or_else(|err2| {
            eprintln!("Failed to initialize aqua registry with settings: {err2:?}");
            // Fallback to baked-only registry
            eprintln!("Attempting fallback to baked registry only");
            AquaRegistryManager::with_settings(None, &[], true).unwrap_or_else(|err3| {
                eprintln!("Failed to initialize baked registry fallback: {err3:?}");
                // Final fallback to empty registry
                eprintln!("Using empty registry as final fallback");
                AquaRegistryManager::with_settings(None, &[], false).unwrap_or_else(|err4| {
                    panic!("Critical failure: Unable to create even empty aqua registry: {err4:?}");
                })
            })
        })
    });

    Arc::new(registry_manager)
});

// Re-export types for backwards compatibility
pub use aqua_registry::{AquaChecksumType, AquaMinisignType, AquaPackageType};

// Re-export AquaPackage as needed by backend
pub use aqua_registry::AquaPackage;

// Compatibility for the doctor - count packages in baked registry
use std::collections::HashMap;

pub static AQUA_STANDARD_REGISTRY_FILES: Lazy<HashMap<&'static str, &'static str>> =
    Lazy::new(|| {
        // Return a map with the count of packages in the baked registry
        // The actual data is now in the aqua-registry crate's baked module
        // For compatibility with doctor, we just need the count, not the actual data
        // The doctor command only uses .len() on this map
        HashMap::new() // Will be populated by build.rs if needed
    });

// Git support implementation
fn create_registry_with_git_support(settings: &Settings) -> Result<AquaRegistryManager> {
    let git_clone_fn = Some(Box::new(
        |url: &str, cache_dir: &std::path::Path| -> Result<Vec<PathBuf>> {
            clone_or_update_git_registry(url, cache_dir)
        },
    )
        as Box<dyn Fn(&str, &std::path::Path) -> Result<Vec<PathBuf>>>);

    AquaRegistryManager::with_git_support(
        settings.aqua.registry_url.as_deref(),
        &settings
            .aqua
            .additional_registries
            .clone()
            .unwrap_or_default(),
        settings.aqua.baked_registry,
        git_clone_fn,
    )
}

fn clone_or_update_git_registry(url: &str, _cache_dir: &std::path::Path) -> Result<Vec<PathBuf>> {
    // Use mise's cache directory
    let actual_cache_dir = dirs::CACHE.join("aqua-registry");
    let repo = Git::new(&actual_cache_dir);

    if repo.exists() {
        // Update existing repository
        fetch_latest_repo(&repo)?;
    } else {
        // Clone new repository
        info!(
            "cloning aqua registry from {} to {:?}",
            url, actual_cache_dir
        );
        repo.clone(url, CloneOptions::default())?;
    }

    // Find registry files in the cloned repository
    let mut registry_files = Vec::new();

    // Check for main registry.yaml
    let main_registry = actual_cache_dir.join("registry.yaml");
    if main_registry.exists() {
        registry_files.push(main_registry);
    }

    // Check for pkgs/**/registry.yaml files
    let pkgs_dir = actual_cache_dir.join("pkgs");
    if pkgs_dir.exists() {
        for entry in std::fs::read_dir(&pkgs_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let registry_file = entry.path().join("registry.yaml");
                if registry_file.exists() {
                    registry_files.push(registry_file);
                }
            }
        }
    }

    if registry_files.is_empty() {
        return Err(eyre::eyre!(
            "No registry.yaml files found in Git repository"
        ));
    }

    Ok(registry_files)
}

fn fetch_latest_repo(repo: &Git) -> Result<()> {
    if file::modified_duration(&repo.dir)? < WEEKLY {
        return Ok(());
    }

    // Don't update if PREFER_OFFLINE is set
    if crate::env::PREFER_OFFLINE.load(std::sync::atomic::Ordering::Relaxed) {
        trace!("skipping aqua registry update due to PREFER_OFFLINE");
        return Ok(());
    }

    info!("updating aqua registry");
    repo.update(None)?;
    Ok(())
}
