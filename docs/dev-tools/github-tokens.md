# GitHub Tokens

Many tools in mise are hosted on GitHub, and mise uses the GitHub API to list versions and download releases. Unauthenticated requests are subject to low [rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api), which can cause `403 Forbidden` errors — especially in CI. This page explains how to configure GitHub authentication in mise.

## Token Priority

mise checks the following sources in order. The first token found wins:

**github.com:**

| Priority | Source                             |
| -------- | ---------------------------------- |
| 1        | `MISE_GITHUB_TOKEN` env var        |
| 2        | `GITHUB_API_TOKEN` env var         |
| 3        | `GITHUB_TOKEN` env var             |
| 4        | `credential_command` (if set)      |
| 5        | `github_tokens.toml` (per-host)    |
| 6        | gh CLI token (from `hosts.yml`)    |
| 7        | `git credential fill` (if enabled) |

**GitHub Enterprise hosts:**

| Priority | Source                                                             |
| -------- | ------------------------------------------------------------------ |
| 1        | `MISE_GITHUB_ENTERPRISE_TOKEN` env var                             |
| 2        | `MISE_GITHUB_TOKEN` / `GITHUB_API_TOKEN` / `GITHUB_TOKEN` env vars |
| 3        | `credential_command` (if set)                                      |
| 4        | `github_tokens.toml` (per-host)                                    |
| 5        | gh CLI token (from `hosts.yml`, matched by hostname)               |
| 6        | `git credential fill` (if enabled)                                 |

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

Use `mise github token` to see which token mise would use for a given host:

```sh
mise github token                           # check github.com (masked)
mise github token --unmask                  # show full token
mise github token github.mycompany.com      # check a GHE host
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
4. `github_tokens.toml` for the API hostname
5. gh CLI token for the API hostname
6. `git credential fill` for the API hostname

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
