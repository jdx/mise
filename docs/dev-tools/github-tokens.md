# GitHub Tokens

Many tools in mise are hosted on GitHub, and mise uses the GitHub API to list versions and download releases. Unauthenticated requests are subject to low [rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api), which can cause `403 Forbidden` errors — especially in CI. This page explains how to configure GitHub authentication in mise.

## Token Priority

mise checks the following sources in order. The first token found wins:

**github.com:**

| Priority | Source                                      |
| -------- | ------------------------------------------- |
| 1        | `MISE_GITHUB_TOKEN` env var                 |
| 2        | `GITHUB_API_TOKEN` env var                  |
| 3        | `GITHUB_TOKEN` env var                      |
| 4        | gh CLI token (from `hosts.yml`)             |
| 5        | gh CLI token (from `gh auth token`, opt-in) |

**GitHub Enterprise hosts:**

| Priority | Source                                                             |
| -------- | ------------------------------------------------------------------ |
| 1        | `MISE_GITHUB_ENTERPRISE_TOKEN` env var                             |
| 2        | `MISE_GITHUB_TOKEN` / `GITHUB_API_TOKEN` / `GITHUB_TOKEN` env vars |
| 3        | gh CLI token (from `hosts.yml`, matched by hostname)               |
| 4        | gh CLI token (from `gh auth token --hostname <host>`, opt-in)      |

::: tip
The github.com env vars (`MISE_GITHUB_TOKEN`, etc.) are also used as a fallback for GHE when `MISE_GITHUB_ENTERPRISE_TOKEN` is not set. If you need different tokens for github.com and a GHE instance, set `MISE_GITHUB_ENTERPRISE_TOKEN` explicitly or use the gh CLI integration.
:::

## Setting a Token via Environment Variable

Create a [personal access token](https://github.com/settings/tokens/new?description=MISE_GITHUB_TOKEN) (no scopes required) and set it:

```sh
export MISE_GITHUB_TOKEN="ghp_xxxxxxxxxxxx"
```

Or, if you already have `GITHUB_TOKEN` set (common in GitHub Actions), mise will use it automatically.

## gh CLI Integration

If you use the [GitHub CLI](https://cli.github.com/) (`gh`), mise can read tokens from it automatically. This kicks in when no token environment variable is set.

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
mise reads the config file directly — it does not shell out to `gh` by default. If your gh CLI uses a credential helper (e.g., macOS Keychain or Windows Credential Manager) instead of storing tokens in `hosts.yml`, enable `gh_cli_token_from_cmd` (see below).
:::

To disable this behavior:

```toml
[settings.github]
gh_cli_tokens = false
```

### `gh auth token` (opt-in)

On some platforms (e.g. **Windows** or **macOS** with Keychain), the gh CLI stores credentials in a system credential helper rather than in `hosts.yml`. In that case you can opt in to having mise run `gh auth token` to retrieve the token:

```toml
[settings.github]
gh_cli_token_from_cmd = true
```

When enabled, mise runs `gh auth token` (or `gh auth token --hostname <host>` for GitHub Enterprise) after checking environment variables and `hosts.yml`.

```sh
# These are the commands mise may run when gh_cli_token_from_cmd = true:
gh auth token                            # for github.com
gh auth token --hostname <host>          # for GitHub Enterprise
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
3. gh CLI token for the API hostname

If you have **multiple** GHE instances, `MISE_GITHUB_ENTERPRISE_TOKEN` (a single value) won't work. Use the gh CLI integration instead:

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
