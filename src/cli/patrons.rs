use std::time::Duration;

use eyre::Result;
use serde::{Deserialize, Serialize};

use crate::file::modified_duration;
use crate::http::HTTP;
use crate::{dirs, duration, file};

/// Show the individuals supporting mise as Patron-tier members
///
/// Lists the individuals on the Patron tier from <https://en.dev/patrons.json>.
/// The list refreshes daily; supporting terminals will render each patron's
/// name as a clickable link via OSC 8 hyperlinks.
///
/// To appear here, become a patron at <https://en.dev/sponsor>.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Patrons {
    /// Output in JSON format
    #[clap(long, short = 'J')]
    json: bool,

    /// Bypass the local cache and re-fetch
    #[clap(long)]
    refresh: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct PatronsPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    schema: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generated_at: Option<String>,
    patrons: Vec<Patron>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Patron {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

const PATRONS_URL: &str = "https://en.dev/patrons.json";
const SPONSOR_URL: &str = "https://en.dev/sponsor";
const CACHE_TTL: Duration = duration::DAILY;

impl Patrons {
    pub async fn run(self) -> Result<()> {
        let payload = load_patrons(self.refresh).await?;

        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&payload)?);
        } else {
            render_human(&payload)?;
        }
        Ok(())
    }
}

async fn load_patrons(refresh: bool) -> Result<PatronsPayload> {
    let cache_path = dirs::CACHE.join("patrons.json");

    if !refresh
        && let Ok(age) = modified_duration(&cache_path)
        && age < CACHE_TTL
        && let Ok(body) = file::read_to_string(&cache_path)
        && let Ok(payload) = serde_json::from_str::<PatronsPayload>(&body)
    {
        return Ok(payload);
    }

    match HTTP.get_text(PATRONS_URL).await {
        Ok(body) => {
            let payload: PatronsPayload = serde_json::from_str(&body)?;
            let _ = file::create_dir_all(*dirs::CACHE);
            let _ = file::write(&cache_path, &body);
            Ok(payload)
        }
        Err(err) => {
            // Network failed — fall back to whatever we have cached, however old.
            if let Ok(body) = file::read_to_string(&cache_path)
                && let Ok(payload) = serde_json::from_str::<PatronsPayload>(&body)
            {
                debug!("failed to refresh patrons.json, using stale cache: {err:#?}");
                return Ok(payload);
            }
            Err(err)
        }
    }
}

fn render_human(payload: &PatronsPayload) -> Result<()> {
    if payload.patrons.is_empty() {
        miseprintln!(
            "No patrons yet — be the first: {}",
            hyperlink(SPONSOR_URL, SPONSOR_URL),
        );
        return Ok(());
    }
    miseprintln!("mise is supported by these patrons — thank you ❤\n");
    for p in &payload.patrons {
        let label = match &p.url {
            Some(url) => hyperlink(url, &p.name),
            None => p.name.clone(),
        };
        miseprintln!("  • {label}");
    }
    miseprintln!(
        "\nBecome a patron: {}",
        hyperlink(SPONSOR_URL, SPONSOR_URL),
    );
    Ok(())
}

fn hyperlink(url: &str, text: &str) -> String {
    if supports_hyperlinks::supports_hyperlinks() {
        // OSC 8 hyperlink. Many modern terminals (iTerm2, WezTerm, Kitty,
        // Windows Terminal, recent GNOME Terminal, etc.) render `text` as a
        // clickable link. Terminals that don't support OSC 8 simply ignore
        // the escapes and render `text` as-is.
        format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
    } else {
        text.to_string()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise patrons</bold>
    $ <bold>mise patrons -J</bold>
    $ <bold>mise patrons --refresh</bold>"#
);
