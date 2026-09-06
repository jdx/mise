# GitHub Tokens

Many tools in mise are hosted on GitHub. For public releases, mise uses [mise-versions](https://mise-versions.jdx.dev) by default as a shared cache for version lists, release metadata, and GitHub artifact attestations. This avoids most unauthenticated GitHub API calls during normal installs, including CI and Docker builds.

GitHub tokens are still useful when mise has to fall back to GitHub's API, when `MISE_USE_VERSIONS_HOST=0` is set, or when installing tools from private repositories, GitHub Enterprise, or custom GitHub API hosts. Unauthenticated requests are subject to low [rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api), which can cause `403 Forbidden` errors. This page explains how to configure GitHub authentication in mise.

## Start with token diagnostics

Check which source mise selects without printing the full credential:

```sh
mise token github
mise token github github.mycompany.com
```

The second command is for an Enterprise host; replace the hostname. A selected
token is not proof that it has access to a particular repository. If an install
fails, check its permissions and expiry as well as the source shown here.

If you already use `gh auth login` with a system keyring, configure a
[credential command](#git-credential-helpers). mise's direct `hosts.yml` reader
cannot retrieve a token stored only in that keyring.

## Token Priority

mise checks the following sources in order. The first available token wins; a
wrong or expired higher-priority token can hide a working lower-priority source.
`GH_TOKEN` is not a direct mise token source, although a configured `gh auth
token` credential command can use it.

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

For public release access, a classic personal access token does not need private
repository scopes. For private repositories, grant the token access to the
repository and the permissions the API requires; a fine-grained token needs
**Contents: read** to [download release assets](https://docs.github.com/en/rest/releases/assets#get-a-release-asset).
Create a [personal access token](https://github.com/settings/tokens) and make it
available to the process running mise:

```sh
export MISE_GITHUB_TOKEN="ghp_xxxxxxxxxxxx"
```

The value above is a placeholder. Prefer a secret manager or your CI secret
store for the actual value. An existing `GITHUB_TOKEN` also works when no
higher-priority source overrides it; see [GitHub Actions](/dev-tools/github-tokens.html#ci-github-actions).

## Token File (`github_tokens.toml`)

You can store per-host GitHub tokens in a mise-specific config file:

```toml
# ~/.config/mise/github_tokens.toml
[tokens."github.com"]
token = "ghp_xxxxxxxxxxxx"

[tokens."github.mycompany.com"]
token = "ghp_yyyyyyyyyyyy"
```

This file is checked after environment variables, `credential_command`, and
native OAuth, but before the gh CLI file. It is useful when:

- You don't use the gh CLI, or
- The gh CLI token has restricted scope (e.g., Coder-provisioned tokens scoped to specific orgs) and you need a broader token for mise, or
- You want mise-specific tokens that don't interfere with other tools.

The file location follows `MISE_CONFIG_DIR` (defaults to `~/.config/mise`).
No additional setting is required. The file contains plaintext credentials; keep
it outside shared project configuration and readable only by your user. On Unix:

```sh
chmod 600 "${MISE_CONFIG_DIR:-$HOME/.config/mise}/github_tokens.toml"
```

## gh CLI Integration

If you use the [GitHub CLI](https://cli.github.com/) (`gh`), mise can read tokens directly from its `hosts.yml` config file. This is enabled by default and is used when no higher-priority source resolves a token.

mise looks for `hosts.yml` in these locations (first match wins):

1. `$GH_CONFIG_DIR/hosts.yml`
2. `$XDG_CONFIG_HOME/gh/hosts.yml` (when that variable is set)
3. `~/Library/Application Support/gh/hosts.yml` (macOS only)
4. `%APPDATA%\GitHub CLI\hosts.yml` (Windows only — this is gh's default location there)
5. `~/.config/gh/hosts.yml`

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

Configure a custom command in your **global** settings to obtain a GitHub token.
`github.credential_command` is global-only; a project cannot choose a command to
read your credentials. For example:

```toml [~/.config/mise/config.toml]
[settings.github]
credential_command = "op read 'op://Private/GitHub Token/credential'"
```

mise executes this command with the configured default inline shell ([`unix_default_inline_shell_args`](/configuration/settings.html#unix_default_inline_shell_args) or [`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args)) and reads the token from stdout. The hostname is available as `MISE_CREDENTIAL_HOST`, and the provider name (`github`) is available as `MISE_CREDENTIAL_PROVIDER`. For compatibility, recognized sh-compatible shells (`ash`, `bash`, `dash`, `ksh`, `sh`, and `zsh`) also receive the hostname as `$1`/`${1}`. This is checked before `github_tokens.toml` and gh CLI tokens, so it takes priority over file-based sources.

:::: warning Planned deprecation
The legacy `$1`/`${1}` hostname argument is deprecated. Use `MISE_CREDENTIAL_HOST` instead. mise will start warning in `2026.11.0`, and `$1` compatibility will be removed in `2027.11.0`.
::::

### Using ghtkn

[ghtkn](https://github.com/suzuki-shunsuke/ghtkn) can generate short-lived GitHub App user access tokens and print them to stdout, which makes it compatible with `credential_command`.

Run `ghtkn get` once manually before relying on it from mise so any browser-based device flow happens intentionally. After that, ghtkn can reuse tokens from your OS secret manager until they need to be regenerated.

The credential command runs with mise shims removed from `PATH` to avoid recursive mise invocations. If you install `ghtkn` with mise, use `mise which` to find the real executable path and store that in `credential_command` instead of relying on the shim:

```sh
mise settings set github.credential_command="\"$(mise which ghtkn)\" get -m 1h"
```

Do not make the credential command run `mise x`, `mise exec`, or another command that may need GitHub access to resolve or install `ghtkn`, since that can loop while mise is trying to obtain the GitHub token.

If `ghtkn` is already available without relying on a mise shim, you can also set it directly:

```toml [~/.config/mise/config.toml]
[settings.github]
credential_command = "ghtkn get -m 1h"
```

Use `mise token github` to confirm mise can resolve the token:

```sh
mise token github
```

## Native GitHub OAuth

mise can create short-lived GitHub App user access tokens directly with GitHub's OAuth device flow. This does not require a personal access token, GitHub App private key, app client secret, `gh`, `ghtkn`, or any other external credential command.

The design was inspired by [ghtkn](https://github.com/suzuki-shunsuke/ghtkn) — if you'd rather run a separate process and have mise pick up its token via `credential_command`, see [Using ghtkn](#using-ghtkn) above.

Create a GitHub App with device flow enabled, then configure its client ID:

```sh
mise settings set github.oauth_client_id=Iv1.yourgithubappclientid
```

Authorize once:

```sh
mise token github --oauth
```

After that, mise reuses the cached token for its own GitHub API calls and refreshes it when GitHub returns a refresh token. While the cached token is valid, mise also exports it to your shell as `GITHUB_TOKEN` (via `mise activate` / `mise hook-env` / `mise env` / `mise exec`) so tools that read `GITHUB_TOKEN`, such as `gh`, can use it:

```sh
mise exec -- gh pr list
```

mise does not replace an existing value of the export variable. Git credential
helpers and Cargo registry authentication have their own configuration; exporting
`GITHUB_TOKEN` does not configure them automatically.

To use a different variable name (for example, `gh`'s preferred `GH_TOKEN`), set `github.oauth_export_env`. Setting it to an empty string disables the auto-export.

You can still print a raw token explicitly when you need to pipe it somewhere:

```sh
mise token github --oauth --raw
```

The raw form prints a secret. Use it only when another command needs the token
value; normal diagnostics are masked. Copying the value into `MISE_GITHUB_TOKEN`
also makes that environment value take precedence over future OAuth resolution.

After changing the GitHub App's permissions or installation access, request a
fresh token:

```sh
mise token github --oauth --refresh
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
- **macOS/Windows** where `gh auth login` stores the token in the system keyring (macOS Keychain,
  Windows Credential Manager) rather than in `hosts.yml`. In that case `hosts.yml` exists but has no
  `oauth_token` key, so reading it cannot help. Run `mise token github` to tell the two apart: if it
  prints `(none)` while `gh auth status` works, this setting — or a `github.credential_command`
  that shells out to gh — is what you want.

  With a single account, `credential_command = "gh auth token"` is enough. If you authenticate to
  more than one host (GitHub Enterprise), pass the host mise is asking about, because bare
  `gh auth token` returns the token for gh's _own_ active host. mise exports it as
  `MISE_CREDENTIAL_HOST`, and the command runs through the platform's inline shell, so the
  interpolation differs:

  On macOS and Linux:

  ```toml [~/.config/mise/config.toml]
  [settings.github]
  credential_command = 'gh auth token --hostname "$MISE_CREDENTIAL_HOST"'
  ```

  On Windows, `cmd` is the default inline shell and does not expand `$VAR`:

  ```toml [~/.config/mise/config.toml]
  [settings.github]
  credential_command = 'gh auth token --hostname %MISE_CREDENTIAL_HOST%'
  ```

- Any environment where git already has credentials configured

mise runs `git credential fill` with `GIT_TERMINAL_PROMPT=0` (to prevent interactive prompts) and caches the result per host for the session.

To enable this behavior:

```toml
[settings.github]
use_git_credentials = true
```

## Debugging Token Resolution

Use the masked output to distinguish configuration problems from API failures:

```sh
mise token github
mise token github github.mycompany.com
```

| Result or symptom                                      | What to check                                                                                                   |
| ------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------- |
| An unexpected environment variable is selected         | Remove or correct that override in the process environment; lower-priority sources are not consulted            |
| `(none)` but `gh auth status` works                    | gh may use a system keyring; configure a host-aware credential command or opt into git credential helpers       |
| A token is selected but the repository is inaccessible | Check repository access, token expiry, organization authorization, and the API host                             |
| `403` or `429` from GitHub                             | Inspect the response for rate-limit details; `403` can also be a permissions failure                            |
| OAuth refresh is rejected                              | Check the configured GitHub App client ID and request `mise token github --oauth --refresh` after correcting it |

`mise token github --unmask` and `--raw` reveal the credential. They are not needed
to identify its source and should not be included in shared diagnostic logs.

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

If different GHE instances require different tokens, one
`MISE_GITHUB_ENTERPRISE_TOKEN` value cannot represent them. Use `github_tokens.toml`, the gh CLI integration, `credential_command`, or git credential helpers instead:

```sh
gh auth login --hostname github.mycompany.com
gh auth login --hostname github.other-company.com
```

## Reducing API Requests with Lockfiles {#avoiding-tokens-entirely-with-lockfiles}

A [lockfile](/dev-tools/mise-lock.html) can avoid release-discovery requests when
it records the required artifact URLs and checksums:

```sh
mise lock
mise install
```

This reduces token requirements for public downloads. It does not make private
artifacts public or guarantee an offline install. Missing platform metadata,
provenance verification, and Packslip policy checks can still need network access
or authentication. Keep required credentials available in CI even when using a
lockfile.

## CI / GitHub Actions

GitHub provides a workflow token through the `secrets.GITHUB_TOKEN` and
`github.token` contexts. To make it available to a shell command running mise,
pass it as an environment variable:

```yaml
# Step after checkout and mise setup
- name: Install development tools
  run: mise install
  env:
    GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

[`jdx/mise-action`](https://github.com/jdx/mise-action) has its own token input
and install step. Check the action's configuration when using it rather than a
separate `run` step.

The workflow token's permissions and repository scope still apply. Access to a
private tool in another repository may require a GitHub App token or a personal
access token with access to that repository, stored as an Actions secret. See
[GitHub's workflow authentication guide](https://docs.github.com/en/actions/tutorials/authenticate-with-github_token).

## .netrc

mise also supports `.netrc` for HTTP Basic auth. Credentials from `.netrc` take precedence over token-based auth headers. See [URL Replacements](/url-replacements.html) for details.
