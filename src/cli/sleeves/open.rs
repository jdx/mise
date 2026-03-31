use eyre::Result;

/// Open a provider's dashboard in your default browser
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct SleevesOpen {
    /// Provider name (e.g., vercel, supabase, clerk)
    provider: String,
}

impl SleevesOpen {
    pub async fn run(self) -> Result<()> {
        let url = match self.provider.to_lowercase().as_str() {
            "vercel" => "https://vercel.com/dashboard",
            "supabase" => "https://supabase.com/dashboard",
            "neon" => "https://console.neon.tech",
            "planetscale" => "https://app.planetscale.com",
            "turso" => "https://turso.tech/app",
            "chroma" => "https://cloud.trychroma.com",
            "clerk" => "https://dashboard.clerk.com",
            "posthog" => "https://us.posthog.com",
            "railway" => "https://railway.app/dashboard",
            "runloop" => "https://runloop.ai/dashboard",
            other => {
                miseprintln!("Unknown provider '{}'. Opening generic search.", other);
                &format!("https://{}.com", other)
                    // Can't return a reference to a local, so handle below
            }
        };

        // For unknown providers, build URL differently
        let url = if self.provider.to_lowercase().as_str() == "vercel"
            || self.provider.to_lowercase().as_str() == "supabase"
            || self.provider.to_lowercase().as_str() == "neon"
            || self.provider.to_lowercase().as_str() == "planetscale"
            || self.provider.to_lowercase().as_str() == "turso"
            || self.provider.to_lowercase().as_str() == "chroma"
            || self.provider.to_lowercase().as_str() == "clerk"
            || self.provider.to_lowercase().as_str() == "posthog"
            || self.provider.to_lowercase().as_str() == "railway"
            || self.provider.to_lowercase().as_str() == "runloop"
        {
            url.to_string()
        } else {
            format!("https://{}.com", self.provider.to_lowercase())
        };

        miseprintln!("Opening {} dashboard: {}", self.provider, url);
        open_url(&url)?;
        Ok(())
    }
}

fn open_url(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", url])
            .spawn()?;
    }
    Ok(())
}
