use super::*;

impl Settings {
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
