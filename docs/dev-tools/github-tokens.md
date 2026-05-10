# GitHub Tokens

Many tools in mise are hosted on GitHub, and mise uses the GitHub API to list versions and download releases. Unauthenticated requests are subject to low [rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api), which can cause `403 Forbidden` errors — especially in CI. This page explains how to configure GitHub authentication in mise.

## Token Priority

mise checks the following sources in order. The first token found wins:

**github.com:**

| Priority | Source                              |
| -------- | ----------------------------------- |
| 1        | `MISE_GITHUB_TOKEN` env var         |
| 2        | `GITHUB_API_TOKEN` env var          |
| 3        | `GITHUB_TOKEN` env var              |
| 4        | `credential_command` (if set)       |
| 5        | native GitHub OAuth (if configured) |
| 6        | `github_tokens.toml` (per-host)     |
| 7        | gh CLI token (from `hosts.yml`)     |
| 8        | `git credential fill` (if enabled)  |

**GitHub Enterprise hosts:**

| Priority | Source                                                             |
| -------- | ------------------------------------------------------------------ |
| 1        | `MISE_GITHUB_ENTERPRISE_TOKEN` env var                             |
| 2        | `MISE_GITHUB_TOKEN` / `GITHUB_API_TOKEN` / `GITHUB_TOKEN` env vars |
| 3        | `credential_command` (if set)                                      |
| 4        | native GitHub OAuth (if configured)                                |
| 5        | `github_tokens.toml` (per-host)                                    |
| 6        | gh CLI token (from `hosts.yml`, matched by hostname)               |
| 7        | `git credential fill` (if enabled)                                 |

::: tip
The github.com env vars (`MISE_GITHUB_TOKEN`, etc.) are also used as a fallback for GHE when `MISE_GITHUB_ENTERPRISE_TOKEN` is not set. If you need different tokens for github.com and a GHE instance, set `MISE_GITHUB_ENTERPRISE_TOKEN` explicitly or use the gh CLI integration.
:::

## Setting a Token via Environment Variable

Create a [personal access token](https://github.com/settings/tokens/new?description=MISE_GITHUB_TOKEN) (no scopes required) and set it:

```sh
export MISE_GITHUB_TOKEN="ghp_xxxxxxxxxxxx"
```

Or, if you already have `GITHUB_TOKEN` set (common in GitHub Actions), mise will use it automatically.

## Token File (`github_tokens.toml`)

You can store per-host GitHub tokens in a mise-specific config file:

```toml
# ~/.config/mise/github_tokens.toml
[tokens."github.com"]
token = "ghp_xxxxxxxxxxxx"

[tokens."github.mycompany.com"]
token = "ghp_yyyyyyyyyyyy"
```

This file is checked after environment variables and `credential_command` but before the gh CLI's `hosts.yml`, making it useful when:

- You don't use the gh CLI, or
- The gh CLI token has restricted scope (e.g., Coder-provisioned tokens scoped to specific orgs) and you need a broader token for mise, or
- You want mise-specific tokens that don't interfere with other tools.

The file location follows `MISE_CONFIG_DIR` (defaults to `~/.config/mise`).
No additional settings are required — mise auto-discovers the file if it exists.

## gh CLI Integration

If you use the [GitHub CLI](https://cli.github.com/) (`gh`), mise can read tokens directly from its `hosts.yml` config file. This is enabled by default and kicks in when no token environment variable is set.

mise looks for `hosts.yml` in these locations (first match wins):

1. `$GH_CONFIG_DIR/hosts.yml`
2. `$XDG_CONFIG_HOME/gh/hosts.yml` (defaults to `~/.config/gh/hosts.yml`)
3. `~/Library/Application Support/gh/hosts.yml` (macOS only)

This is especially useful for **GitHub Enterprise** — the gh CLI stores per-host tokens, so mise can authenticate to multiple GHE instances without juggling environment variables:

```yaml
# ~/.config/gh/hosts.yml (managed by `gh auth login`)
github.com:
  oauth_token: ghp_xxxxxxxxxxxx
  user: you
github.mycompany.com:
  oauth_token: ghp_yyyyyyyyyyyy
  user: you
```

::: info
mise reads the config file directly — it does not shell out to `gh`. If your gh CLI uses a credential helper (e.g., macOS Keychain) instead of storing tokens in `hosts.yml`, the token won't be available via this method. However, mise also supports `git credential fill` (see below), which can retrieve tokens from system keyrings.
:::

To disable this behavior:

```toml
[settings.github]
gh_cli_tokens = false
```

## Credential Command

You can configure a custom shell command that mise runs to obtain a GitHub token. This is useful when you want a credential source that only mise uses, without affecting git:

```toml
[settings.github]
credential_command = "op read 'op://Private/GitHub Token/credential'"
```

mise executes this command via `sh -c` and reads the token from stdout. The hostname is passed as `$1`, so the command can return different tokens for different hosts (e.g., `github.com` vs a GHE instance). This is checked before `github_tokens.toml` and gh CLI tokens, so it takes priority over file-based sources. Results are cached per host per session.

### Using ghtkn

[ghtkn](https://github.com/suzuki-shunsuke/ghtkn) can generate short-lived GitHub App user access tokens and print them to stdout, which makes it compatible with `credential_command`.

Run `ghtkn get` once manually before relying on it from mise so any browser-based device flow happens intentionally. After that, ghtkn can reuse tokens from your OS secret manager until they need to be regenerated.

The credential command runs with mise shims removed from `PATH` to avoid recursive mise invocations. If you install `ghtkn` with mise, use `mise which` to find the real executable path and store that in `credential_command` instead of relying on the shim:

```sh
mise settings set github.credential_command "$(mise which ghtkn) get -m 1h"
```

Do not make the credential command run `mise x`, `mise exec`, or another command that may need GitHub access to resolve or install `ghtkn`, since that can loop while mise is trying to obtain the GitHub token.

If `ghtkn` is already available without relying on a mise shim, you can also set it directly:

```toml
[settings.github]
credential_command = "ghtkn get -m 1h"
```

Use `mise token github` to confirm mise can resolve the token:

```sh
mise token github
```

## Native GitHub OAuth <Badge type="warning" text="experimental" />

mise can create short-lived GitHub App user access tokens directly with GitHub's OAuth device flow. This does not require a personal access token, GitHub App private key, app client secret, `gh`, `ghtkn`, or any other external credential command.

The design was inspired by [ghtkn](https://github.com/suzuki-shunsuke/ghtkn) — if you'd rather run a separate process and have mise pick up its token via `credential_command`, see [Using ghtkn](#using-ghtkn) above.

::: warning
This feature is experimental. Enable it with `mise settings experimental=true` (or `MISE_EXPERIMENTAL=1`) before using it. Behavior, settings, and token cache format may change in future releases.
:::

Create a GitHub App with device flow enabled, then configure its client ID:

```sh
mise settings set experimental true
mise settings set github.oauth_client_id Iv1.yourgithubappclientid
```

Authorize once:

```sh
mise token github --oauth
```

After that, mise reuses the cached token for its own GitHub API calls and refreshes it when GitHub returns a refresh token. While the cached token is valid, mise also exports it to your shell as `GITHUB_TOKEN` (via `mise activate` / `mise hook-env` / `mise env` / `mise exec`) so tools like `gh`, `git`, and `cargo publish` see it without any extra wiring:

```sh
gh pr list # uses the OAuth token automatically
```

To use a different variable name (for example, `gh`'s preferred `GH_TOKEN`), set `github.oauth_export_env`. Setting it to an empty string disables the auto-export.

You can still print a raw token explicitly when you need to pipe it somewhere:

```sh
export MISE_GITHUB_TOKEN="$(mise token github --oauth --raw)"
```

Optional settings:

```toml
[settings.github]
oauth_client_id = "Iv1.yourgithubappclientid"
oauth_scopes = "" # usually empty for GitHub App user access tokens
oauth_open_browser = true
oauth_export_env = "GITHUB_TOKEN" # set to "" to disable automatic export
```

## Git Credential Helpers

mise can use your existing git credential helpers to obtain GitHub tokens. This is **opt-in** and acts as a last-resort fallback after all other token sources.

This is especially useful for:

- **Devcontainer environments** where tokens are provided via git credential helpers
- **macOS/Windows** where `gh auth login` stores tokens in the system keyring rather than `hosts.yml`
- Any environment where git already has credentials configured

mise runs `git credential fill` with `GIT_TERMINAL_PROMPT=0` (to prevent interactive prompts) and caches the result per host for the session.

To enable this behavior:

```toml
[settings.github]
use_git_credentials = true
```

## Debugging Token Resolution

Use `mise token github` to see which token mise would use for a given host:

```sh
mise token github                           # check github.com (masked)
mise token github --unmask                  # show full token
mise token github github.mycompany.com      # check a GHE host
```

## GitHub Enterprise

For self-hosted GitHub instances, set the `api_url` [tool option](/dev-tools/backends/github.html#api-url) on the tool:

```toml
[tools]
"github:myorg/mytool" = { version = "latest", api_url = "https://github.mycompany.com/api/v3" }
```

For authentication, mise checks (in order):

1. `MISE_GITHUB_ENTERPRISE_TOKEN` env var
2. `MISE_GITHUB_TOKEN` / `GITHUB_API_TOKEN` / `GITHUB_TOKEN` env vars
3. `credential_command` for the API hostname
4. native GitHub OAuth for the configured API hostname
5. `github_tokens.toml` for the API hostname
6. gh CLI token for the API hostname
7. `git credential fill` for the API hostname

If you have **multiple** GHE instances, `MISE_GITHUB_ENTERPRISE_TOKEN` (a single value) won't work. Use `github_tokens.toml`, the gh CLI integration, `credential_command`, or git credential helpers instead:

```sh
gh auth login --hostname github.mycompany.com
gh auth login --hostname github.other-company.com
```

## Avoiding Tokens Entirely with Lockfiles

If you use [`mise.lock`](/dev-tools/mise-lock.html), mise stores exact download URLs and checksums. Future installs use the lockfile directly — no GitHub API calls needed:

```sh
mise settings lockfile=true
mise lock
```

This is the best approach for CI where you want deterministic builds without configuring tokens. See [mise.lock Lockfile](/dev-tools/mise-lock.html) for details.

## CI / GitHub Actions

In GitHub Actions, `GITHUB_TOKEN` is automatically available. mise picks it up with no extra configuration:

```yaml
- uses: jdx/mise-action@v2
  env:
    GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

For private repos or higher rate limits, use a [fine-grained personal access token](https://github.com/settings/tokens?type=beta) stored as a repository secret.

## .netrc

mise also supports `.netrc` for HTTP Basic auth. Credentials from `.netrc` take precedence over token-based auth headers. See [URL Replacements](/url-replacements.html) for details.
