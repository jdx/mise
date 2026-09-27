use super::*;

/// caches the result unless [`clear`] or [`store`] ran while it was loading.
pub type Loader = fn() -> Result<Arc<Settings>>;

static LOADER: OnceLock<Loader> = OnceLock::new();
static CURRENT: SettingsCache = SettingsCache::new();

/// The cached settings, plus a generation that every [`clear`] and [`store`]
/// bumps. A load that started before a bump read inputs (CLI overrides, env,
/// config files) that may since have changed, so its result is not cached.
struct SettingsCache {
    state: RwLock<CacheState>,
}

struct CacheState {
    generation: u64,
    settings: Option<Arc<Settings>>,
    /// The most recently cached settings. Survives [`clear`], and is only ever
    /// set to a value that was actually cached, never to a discarded stale load.
    last: Option<Arc<Settings>>,
}

impl SettingsCache {
    const fn new() -> Self {
        Self {
            state: RwLock::new(CacheState {
                generation: 0,
                settings: None,
                last: None,
            }),
        }
    }

    fn is_loaded(&self) -> bool {
        self.state.read().unwrap().settings.is_some()
    }

    fn last(&self) -> Option<Arc<Settings>> {
        self.state.read().unwrap().last.clone()
    }

    fn store(&self, settings: Arc<Settings>) {
        let mut state = self.state.write().unwrap();
        state.generation += 1;
        state.last = Some(settings.clone());
        state.settings = Some(settings);
    }

    fn clear(&self) {
        let mut state = self.state.write().unwrap();
        state.generation += 1;
        state.settings = None;
    }

    fn get_or_load(&self, loader: impl FnOnce() -> Result<Arc<Settings>>) -> Result<Arc<Settings>> {
        let generation = {
            let state = self.state.read().unwrap();
            if let Some(settings) = &state.settings {
                return Ok(settings.clone());
            }
            state.generation
        };
        let loaded = loader()?;
        let mut state = self.state.write().unwrap();
        if state.generation != generation {
            // Settings were cleared or replaced mid-load. The caller asked
            // before that happened, so it gets what it loaded, but the next
            // read must not see this stale snapshot.
            return Ok(loaded);
        }
        // Another thread in the same generation may have finished first;
        // keep its value so every reader shares one snapshot.
        if let Some(settings) = &state.settings {
            return Ok(settings.clone());
        }
        state.last = Some(loaded.clone());
        state.settings = Some(loaded.clone());
        Ok(loaded)
    }
}

/// Register the function [`Settings::try_get`] calls when nothing is cached.
///
/// mise calls this before anything reads settings. Only the first registration
/// takes effect.
pub fn set_loader(loader: Loader) {
    let _ = LOADER.set(loader);
}

/// Whether settings have been loaded since the last [`clear`].
pub fn is_loaded() -> bool {
    CURRENT.is_loaded()
}

/// The settings most recently cached, even if [`clear`] has run since. For
/// answers needed while settings are being reloaded; a load whose result was
/// discarded as stale never shows up here.
pub fn last_cached() -> Option<Arc<Settings>> {
    CURRENT.last()
}

/// Cache `settings` as the value [`Settings::get`] returns until the next [`clear`].
pub fn store(settings: Arc<Settings>) {
    CURRENT.store(settings);
}

/// Drop the cached settings so the next [`Settings::get`] runs the loader again.
pub fn clear() {
    CURRENT.clear();
}

/// A [`Loader`] that ignores config files and the environment: every setting
/// takes its `settings.toml` default. For the unit tests of crates below mise,
/// which have no config system to load from.
pub fn load_defaults() -> Result<Arc<Settings>> {
    Ok(Arc::new(Settings::builder().load()?))
}

impl Settings {
    pub fn get() -> Arc<Self> {
        Self::try_get().unwrap()
    }

    pub fn try_get() -> Result<Arc<Self>> {
        CURRENT.get_or_load(|| {
            let loader = LOADER
                .get()
                .expect("mise_settings::set_loader must be called before reading settings");
            loader()
        })
    }

    pub fn parse_default_package_line(package: &str) -> Option<String> {
        let package = package.split('#').next().unwrap_or_default().trim();
        (!package.is_empty()).then(|| package.to_string())
    }

    pub fn hidden_configs() -> &'static HashSet<&'static str> {
        static HIDDEN_CONFIGS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
            [
                "ci",
                "cd",
                "debug",
                "env_file",
                "install_before",
                "trace",
                "log_level",
            ]
            .into()
        });
        &HIDDEN_CONFIGS
    }

    pub fn lockfile_enabled(&self) -> bool {
        self.lockfile.unwrap_or(true)
    }

    pub fn generate_lockfiles(&self) -> bool {
        self.lockfile_mode.as_deref() == Some("generate")
    }

    pub fn validate_lockfile_mode(&self) -> Result<()> {
        validate_setting_enum_values(
            "lockfile_mode",
            self.lockfile_mode.as_deref(),
            &["merge", "generate"],
        )
    }

    pub fn lockfile_creation_enabled(&self) -> bool {
        self.lockfile == Some(true)
    }

    pub fn force_provenance_verify(&self) -> bool {
        self.locked_verify_provenance || self.paranoid
    }

    pub fn ensure_experimental(&self, what: &str) -> Result<()> {
        if !self.experimental {
            bail!("{what} is experimental. Enable it with `mise settings experimental=true`");
        }
        Ok(())
    }

    pub fn log_level(&self) -> log::LevelFilter {
        self.log_level.parse().unwrap_or(log::LevelFilter::Info)
    }

    pub fn disable_tools(&self) -> BTreeSet<String> {
        normalize_tool_names(&self.disable_tools)
    }

    pub fn enable_tools(&self) -> Option<BTreeSet<String>> {
        self.enable_tools.as_ref().map(normalize_tool_names)
    }

    pub fn partial_as_dict(partial: &SettingsPartial) -> eyre::Result<toml::Table> {
        let s = toml::to_string(partial)?;
        let mut table = toml::from_str(&s)?;
        remove_empty_nested_settings(&mut table, "");
        redact_settings_table(&mut table);
        Ok(table)
    }

    pub fn os(&self) -> &str {
        match self.os.as_deref().unwrap_or(OS) {
            "darwin" | "macos" => "macos",
            "linux" => "linux",
            "windows" => "windows",
            other => other,
        }
    }

    pub fn arch(&self) -> &str {
        match self.arch.as_deref().unwrap_or(ARCH) {
            "x86_64" | "amd64" => "x64",
            "aarch64" | "arm64" => "arm64",
            other => other,
        }
    }

    pub fn libc(&self) -> Option<&str> {
        match self.libc.as_deref()?.to_ascii_lowercase().as_str() {
            "glibc" | "gnu" => Some("gnu"),
            "musl" => Some("musl"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> Arc<Settings> {
        load_defaults().unwrap()
    }

    #[test]
    fn test_get_or_load_caches_result() {
        let cache = SettingsCache::new();
        let loaded = cache.get_or_load(|| Ok(defaults())).unwrap();
        assert!(cache.is_loaded());
        let cached = cache.get_or_load(|| panic!("loader rerun")).unwrap();
        assert!(Arc::ptr_eq(&loaded, &cached));

        cache.clear();
        assert!(!cache.is_loaded());
        assert!(Arc::ptr_eq(&loaded, &cache.last().unwrap()));
    }

    /// Thread A starts loading, thread B changes an input and clears, then A
    /// finishes. A's snapshot predates B's change, so it must not be cached.
    #[test]
    fn test_get_or_load_discards_load_cleared_midway() {
        let cache = SettingsCache::new();
        let stale = cache
            .get_or_load(|| {
                let stale = defaults();
                cache.clear();
                Ok(stale)
            })
            .unwrap();
        assert!(!cache.is_loaded());
        assert!(cache.last().is_none());

        let fresh = cache.get_or_load(|| Ok(defaults())).unwrap();
        assert!(!Arc::ptr_eq(&stale, &fresh));
        let cached = cache.get_or_load(|| panic!("loader rerun")).unwrap();
        assert!(Arc::ptr_eq(&fresh, &cached));
    }

    /// A load must not overwrite settings another thread stored mid-load.
    #[test]
    fn test_get_or_load_keeps_store_made_midway() {
        let cache = SettingsCache::new();
        let stored = defaults();
        let stale = cache
            .get_or_load(|| {
                cache.store(stored.clone());
                Ok(defaults())
            })
            .unwrap();
        let cached = cache.get_or_load(|| panic!("loader rerun")).unwrap();
        assert!(Arc::ptr_eq(&stored, &cached));
        assert!(!Arc::ptr_eq(&stale, &cached));
        assert!(Arc::ptr_eq(&stored, &cache.last().unwrap()));
    }

    /// Two loads in the same generation: the first to finish wins, and the
    /// second caller gets that same snapshot.
    #[test]
    fn test_get_or_load_concurrent_same_generation_shares_snapshot() {
        let cache = SettingsCache::new();
        let first = std::cell::OnceCell::new();
        let second = cache
            .get_or_load(|| {
                first
                    .set(cache.get_or_load(|| Ok(defaults())).unwrap())
                    .unwrap();
                Ok(defaults())
            })
            .unwrap();
        assert!(Arc::ptr_eq(first.get().unwrap(), &second));
    }
}
