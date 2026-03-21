# GitHub Tokens

Many tools in mise are hosted on GitHub, and mise uses the GitHub API to list versions and download releases. Unauthenticated requests are subject to low [rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api), which can cause `403 Forbidden` errors — especially in CI. This page explains how to configure GitHub authentication in mise.

## Token Priority

mise checks the following sources in order. The first token found wins:

| Source                         | Applies to                   |
| ------------------------------ | ---------------------------- |
| `MISE_GITHUB_TOKEN` env var    | github.com                   |
| `GITHUB_API_TOKEN` env var     | github.com                   |
| `GITHUB_TOKEN` env var         | github.com                   |
| gh CLI token (from hosts.yml)  | github.com                   |
| `MISE_GITHUB_ENTERPRISE_TOKEN` | GitHub Enterprise            |
| `GITHUB_TOKEN` env var         | GitHub Enterprise (fallback) |
| gh CLI token (from hosts.yml)  | GitHub Enterprise            |

## Setting a Token via Environment Variable

Create a [personal access token](https://github.com/settings/tokens/new?description=MISE_GITHUB_TOKEN) (no scopes required) and set it:

```sh
export MISE_GITHUB_TOKEN="ghp_xxxxxxxxxxxx"
```

Or, if you already have `GITHUB_TOKEN` set (common in GitHub Actions), mise will use it automatically.

## gh CLI Integration

If you use the [GitHub CLI](https://cli.github.com/) (`gh`), mise can read tokens directly from its config file (`~/.config/gh/hosts.yml` or `$GH_CONFIG_DIR/hosts.yml`). This is enabled by default and kicks in when no token environment variable is set.

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
mise reads the config file directly — it does not shell out to `gh`. If your gh CLI uses a credential helper (e.g., macOS Keychain) instead of storing tokens in `hosts.yml`, the token won't be available to mise. In that case, set the token via an environment variable or in mise settings.
:::

To disable this behavior:

```toml
[settings.github]
gh_cli_tokens = false
```

## GitHub Enterprise

For self-hosted GitHub instances, set the `api_url` [tool option](/dev-tools/backends/github.html#api-url) on the tool:

```toml
[tools]
"github:myorg/mytool" = { version = "latest", api_url = "https://github.mycompany.com/api/v3" }
```

For authentication, mise checks (in order):

1. `MISE_GITHUB_ENTERPRISE_TOKEN` env var
2. `GITHUB_TOKEN` env var
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
