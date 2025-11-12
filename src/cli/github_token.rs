use crate::Result;
use crate::config::config_file::ConfigFile;
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::global_config_path;
use crate::env;
use crate::file::display_path;
use crate::http::HTTP;
use demand::Input;
use eyre::{bail, eyre};
use reqwest::header::{HeaderMap, HeaderValue};
use std::path::Path;

/// Set up a GitHub token to increase API rate limits
///
/// This command helps you set up a GitHub personal access token for use with mise.
/// It will open your browser to the token creation page and guide you through
/// creating a token with no scopes (which is sufficient for API rate limits).
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct GithubToken;

impl GithubToken {
    pub async fn run(self) -> Result<()> {
        let token = self.get_token_manually().await?;
        self.validate_token_scopes(&token).await?;
        self.save_token(&token).await?;

        let config_path = global_config_path();
        info!("GitHub token saved to {}", display_path(&config_path));

        if env::is_activated() {
            info!("Token is now available in your current shell session.");
        } else {
            info!("Restart your shell or run `mise activate` to use the token.");
        }

        Ok(())
    }

    async fn validate_token_scopes(&self, token: &str) -> Result<()> {
        info!("Validating token...");

        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_str(format!("token {token}").as_str())
                .map_err(|e| eyre!("Invalid token format: {}", e))?,
        );
        headers.insert(
            "x-github-api-version",
            HeaderValue::from_static("2022-11-28"),
        );
        headers.insert("user-agent", HeaderValue::from_static("mise"));

        let (_, resp_headers) = HTTP
            .json_headers_with_headers::<serde_json::Value, _>(
                "https://api.github.com/user",
                &headers,
            )
            .await
            .map_err(|e| {
                eyre!(
                    "Failed to validate token: {}. Make sure the token is valid.",
                    e
                )
            })?;

        let scopes_header = resp_headers.get("x-oauth-scopes");
        let scopes = scopes_header
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();

        if !scopes.is_empty() {
            bail!(
                "Token has scopes: {}. For security, only tokens with no scopes can be stored in the config file.\n\
                Please create a new token with no scopes at https://github.com/settings/tokens/new?description=MISE_GITHUB_TOKEN&scopes=",
                scopes.join(", ")
            );
        }

        info!("Token validated successfully (no scopes).");
        Ok(())
    }

    async fn get_token_manually(&self) -> Result<String> {
        let url = "https://github.com/settings/tokens/new?description=MISE_GITHUB_TOKEN&scopes=";

        info!("Opening GitHub token creation page in your browser...");
        if let Err(e) = webbrowser::open(url) {
            warn!("Failed to open browser: {}. Please visit: {}", e, url);
        } else {
            info!("Browser opened. Please create a token (no scopes needed).");
        }

        let token = Input::new("Paste your GitHub token here")
            .description("The token will be saved to your global mise config file")
            .password(true)
            .run()
            .map_err(|e| eyre!("Failed to read token: {}", e))?;

        let token = token.trim().to_string();
        if token.is_empty() {
            bail!("Token cannot be empty.");
        }

        Ok(token)
    }

    async fn save_token(&self, token: &str) -> Result<()> {
        let config_path = global_config_path();
        self.ensure_toml_config(&config_path)?;

        let mut mise_toml = if config_path.exists() {
            MiseToml::from_file(&config_path)?
        } else {
            MiseToml::init(&config_path)
        };

        mise_toml.update_env("GITHUB_TOKEN", token)?;
        mise_toml.ensure_env_comment_suffix("GITHUB_TOKEN", TOKEN_COMMENT_SUFFIX)?;
        mise_toml.save()
    }

    fn ensure_toml_config(&self, path: &Path) -> Result<()> {
        let is_toml = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"));

        if is_toml {
            return Ok(());
        }

        bail!(
            "Global config ({}) is not a TOML file. Run `mise config generate --global` or set \
             MISE_GLOBAL_CONFIG_FILE to a `.toml` path before storing secrets.",
            display_path(path)
        );
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise github-token</bold>
    # Opens browser to create a token, then prompts you to paste it

<bold><underline>How it works:</underline></bold>

This command helps you set up a GitHub personal access token to increase your API rate limits.
It will open your browser to the GitHub token creation page where you can create a token
with no scopes (which is sufficient for API rate limits). After creating the token,
paste it into the prompt and it will be saved to your global mise config file.

The token will be available when mise is activated in your shell.
"#
);

const TOKEN_COMMENT_SUFFIX: &str =
    " # GitHub token for API rate limits (no scopes required - safe to store)";
