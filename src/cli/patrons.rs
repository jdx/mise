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
    #[serde(default)]
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

    let (stage, err) = match HTTP.get_text(PATRONS_URL).await {
        Ok(body) => match serde_json::from_str::<PatronsPayload>(&body) {
            Ok(payload) => {
                let _ = file::create_dir_all(*dirs::CACHE);
                let _ = file::write(&cache_path, &body);
                return Ok(payload);
            }
            Err(err) => ("parse", eyre::Report::from(err)),
        },
        Err(err) => ("fetch", err),
    };
    // Either the fetch or the parse failed — fall back to whatever we have
    // cached, however old, so the command stays useful in either case.
    if let Ok(cached_body) = file::read_to_string(&cache_path)
        && let Ok(payload) = serde_json::from_str::<PatronsPayload>(&cached_body)
    {
        warn!("failed to {stage} patrons.json, using stale cache: {err:#}");
        return Ok(payload);
    }
    Err(err)
}

fn render_human(payload: &PatronsPayload) -> Result<()> {
    if payload.patrons.is_empty() {
        miseprintln!(
            "No patrons yet — be the first: {}",
            hyperlink(SPONSOR_URL, SPONSOR_URL),
        );
        return Ok(());
    }
    miseprintln!("mise is supported by these patrons — thank you ❤");
    miseprintln!("");
    for p in &payload.patrons {
        // Sanitize before either rendering branch — patron-supplied data
        // must never carry control bytes into the terminal, whether it
        // gets wrapped in an OSC 8 hyperlink or printed as-is.
        let name = strip_control(&p.name);
        let label = match &p.url {
            Some(url) => hyperlink(&strip_control(url), &name),
            None => name,
        };
        miseprintln!("  • {label}");
    }
    miseprintln!("");
    miseprintln!("Become a patron: {}", hyperlink(SPONSOR_URL, SPONSOR_URL));
    Ok(())
}

fn hyperlink(url: &str, text: &str) -> String {
    // Use `on(Stream::Stdout)` so we also verify stdout is a TTY — bare
    // `supports_hyperlinks()` only inspects env vars and would still emit
    // escapes into pipes like `mise patrons | cat`.
    //
    // Callers are responsible for stripping control bytes from `url` /
    // `text` if they come from untrusted sources; we don't strip here so
    // the same function works for trusted constants like SPONSOR_URL.
    if supports_hyperlinks::on(supports_hyperlinks::Stream::Stdout) {
        // OSC 8 hyperlink. Many modern terminals (iTerm2, WezTerm, Kitty,
        // Windows Terminal, recent GNOME Terminal, etc.) render `text` as
        // a clickable link. Terminals that don't support OSC 8 simply
        // ignore the escapes and render `text` as-is.
        format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
    } else {
        text.to_string()
    }
}

fn strip_control(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise patrons</bold>
    $ <bold>mise patrons -J</bold>
    $ <bold>mise patrons --refresh</bold>"#
);
