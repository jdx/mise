---
description: "Set GitHub, GitLab, and Forgejo tokens for private releases, self-hosted instances, and higher API rate limits."
socialDescription: "Set GitHub, GitLab, and Forgejo tokens for private releases and API rate limits."
---

# GitHub, GitLab, and Forgejo tokens

mise reads public GitHub release metadata through
[mise-versions](https://mise-versions.jdx.dev), so most installs need no token.
Set one when mise calls a forge's API itself: for private repositories, GitHub
Enterprise, self-managed GitLab and Forgejo hosts, or after GitHub's anonymous
rate limit returns `403 Forbidden`.

mise also calls the GitHub API directly when mise-versions does not have a
release's metadata yet, or when
[`use_versions_host`](/configuration/settings.html#use_versions_host) is
`false`. Without a token, those calls share GitHub's low
[anonymous rate limit](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api).

## Set a token

Export a token in the environment that runs mise:

```sh
export MISE_GITHUB_TOKEN="ghp_xxxxxxxxxxxx"     # GitHub
export MISE_GITLAB_TOKEN="glpat-xxxxxxxxxxxx"   # GitLab
export MISE_FORGEJO_TOKEN="xxxxxxxxxxxx"        # Forgejo and Codeberg
```

For GitHub, create a [personal access token](https://github.com/settings/tokens).
A classic token needs no scopes for public releases. For a private repository,
give the token access to that repository; a fine-grained token needs the
Contents: read permission to
[download release assets](https://docs.github.com/en/rest/releases/assets#get-a-release-asset).
An existing `GITHUB_TOKEN` also works unless a higher-priority source is set;
see [token priority](#token-priority).

Keep the real value in a secret manager or your CI's secret store. If you
already sign in with `gh`, `glab` or `fj`, mise can read the token they stored
and you may not need to set anything; see [CLI logins](#cli-logins).

## Check which token mise uses

[`mise token`](/cli/token.html) shows the token mise would send to a host and
where it came from, with the token masked:

```sh
mise token github
mise token gitlab gitlab.mycompany.com
mise token forgejo
```

```text
github.com: ghp_…1234 (source: GITHUB_TOKEN)
```

Without a hostname, the commands check `github.com`, `gitlab.com` and
`codeberg.org`. `(none)` means no source supplied a token for that host. A
selected token does not prove that it can reach a repository; if an install
still fails, check the token's access, expiry and organization authorization.

`--unmask`, and `--raw` for `mise token github`, print the secret itself. You do
not need them to find the source, so keep their output out of shared logs.

## Token sources

### Environment variables

Each provider reads the variables in rows 1 and 2 of the
[priority table](#token-priority), in that order. The Enterprise variables
apply only to hosts other than github.com, gitlab.com and codeberg.org; the
others apply to every host, including Enterprise hosts when the Enterprise
variable is unset. mise does not read `GH_TOKEN` itself, although a
`gh auth token` [credential command](#credential-command) can. A tool's
`install_env` can also set these variables while that tool installs.

### Token files

Per-host tokens that only mise reads go in `github_tokens.toml`,
`gitlab_tokens.toml` or `forgejo_tokens.toml` in the mise config directory
(`~/.config/mise`, or `MISE_CONFIG_DIR`):

```toml [~/.config/mise/github_tokens.toml]
[tokens."github.com"]
token = "ghp_xxxxxxxxxxxx"

[tokens."github.mycompany.com"]
token = "ghp_yyyyyyyyyyyy"
```

The GitLab and Forgejo files use the same layout, keyed by hosts such as
`gitlab.com` and `codeberg.org`. Use a token file when you do not use the
provider's CLI, when the CLI's token is scoped too narrowly for mise (for
example, an organization-scoped token in a cloud development environment), or
when you want tokens that only mise reads. The files hold plaintext
credentials, so keep them out of shared configuration and readable only by you:

```sh
chmod 600 "${MISE_CONFIG_DIR:-$HOME/.config/mise}/github_tokens.toml"
```

### CLI logins

mise reads the tokens that the forge CLIs store, without running them:

| CLI                                                    | File         | Turn off with                                                                           |
| ------------------------------------------------------ | ------------ | --------------------------------------------------------------------------------------- |
| [gh](https://cli.github.com/)                          | `hosts.yml`  | [`github.gh_cli_tokens = false`](/configuration/settings.html#github.gh_cli_tokens)     |
| [glab](https://gitlab.com/gitlab-org/cli)              | `config.yml` | [`gitlab.glab_cli_tokens = false`](/configuration/settings.html#gitlab.glab_cli_tokens) |
| [fj](https://codeberg.org/forgejo-contrib/forgejo-cli) | `keys.json`  | [`forgejo.fj_cli_tokens = false`](/configuration/settings.html#forgejo.fj_cli_tokens)   |

The CLIs store a token per host, so one `gh auth login --hostname <host>` per
GitHub Enterprise host is enough for mise to authenticate to all of them.

mise looks for each file where the CLI keeps it and uses the first one found:

- `gh`: `$GH_CONFIG_DIR/hosts.yml`, then `$XDG_CONFIG_HOME/gh/hosts.yml` when
  that variable is set, then `~/Library/Application Support/gh/hosts.yml` on
  macOS or `%APPDATA%\GitHub CLI\hosts.yml` on Windows, then
  `~/.config/gh/hosts.yml`.
- `glab`: `$GLAB_CONFIG_DIR/config.yml`, then `~/.config/glab-cli/config.yml`
  (glab's legacy location, which glab still prefers when it exists), then
  `$XDG_CONFIG_HOME/glab-cli/config.yml`, then
  `~/Library/Application Support/glab-cli/config.yml` on macOS or
  `%LOCALAPPDATA%\glab-cli\config.yml` on Windows.
- `fj`: `$XDG_DATA_HOME/forgejo-cli/keys.json` (by default
  `~/.local/share/forgejo-cli/keys.json`), then on macOS
  `~/Library/Application Support/forgejo-cli.forgejo-cli/keys.json` and the
  legacy `~/Library/Application Support/Cyborus.forgejo-cli/keys.json`.

#### If gh keeps its token in the system keyring {#gh-keyring}

`gh auth login` on macOS and Windows usually stores the token in the system
keyring (macOS Keychain, Windows Credential Manager), not in `hosts.yml`, so
mise cannot read it from the file. `mise token github` then prints `(none)`
while `gh auth status` succeeds. Have mise ask gh for the token with a
[credential command](#credential-command) in your global config:

```toml [~/.config/mise/config.toml]
[settings.github]
credential_command = 'gh auth token --hostname "$MISE_CREDENTIAL_HOST"'
```

On Windows, the default inline shell is `cmd`, which does not expand `$VAR`:

```toml [~/.config/mise/config.toml]
[settings.github]
credential_command = 'gh auth token --hostname %MISE_CREDENTIAL_HOST%'
```

Passing the host matters when you sign in to more than one host, because a bare
`gh auth token` returns the token for gh's own active host. With a single
github.com account, `credential_command = "gh auth token"` is enough.
[Git credential helpers](#git-credential-helpers) are the other option.

### Credential command

A credential command prints a token that mise reads from stdout, so mise can
take it from a password manager or another tool. Set it in your global config;
`credential_command` is global-only, so a project cannot choose a command that
reads your credentials:

```toml [~/.config/mise/config.toml]
[settings.github]
credential_command = "op read 'op://Private/GitHub Token/credential'"

[settings.gitlab]
credential_command = "op read 'op://Private/GitLab Token/credential'"
```

The Forgejo setting is
[`forgejo.credential_command`](/configuration/settings.html#forgejo.credential_command).
mise runs the command with your default inline shell
([`unix_default_inline_shell_args`](/configuration/settings.html#unix_default_inline_shell_args),
or [`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args)
on Windows) and uses its output, trimmed of surrounding whitespace, as the
token. The command receives `MISE_CREDENTIAL_HOST` (the hostname) and
`MISE_CREDENTIAL_PROVIDER` (`github`, `gitlab` or `forgejo`). It runs with
mise's shims removed from `PATH`, with `GIT_TERMINAL_PROMPT=0` and no stdin, and
at most once per host per mise command. If it exits with an error or prints
nothing, mise moves on to the next source.

::: warning Planned deprecation
sh-compatible shells (`ash`, `bash`, `dash`, `ksh`, `sh` and `zsh`) also receive
the hostname as `$1`. That argument is deprecated: use `MISE_CREDENTIAL_HOST`
instead. mise starts warning in 2026.11.0 and removes `$1` in 2027.11.0.
:::

#### Using ghtkn

[ghtkn](https://github.com/suzuki-shunsuke/ghtkn) creates short-lived GitHub App
user access tokens and prints them to stdout, so it works as a GitHub credential
command. Run `ghtkn get` once by hand so that its browser-based device flow
happens when you expect it. After that, ghtkn reuses tokens from your operating
system's secret store until they need to be renewed.

```toml [~/.config/mise/config.toml]
[settings.github]
credential_command = "ghtkn get -m 1h"
```

Because the command runs without mise's shims on `PATH`, a ghtkn installed by
mise needs its real path. Print it with `mise which ghtkn` and put that path in
the command:

```toml [~/.config/mise/config.toml]
[settings.github]
credential_command = "/path/printed/by/mise-which/ghtkn get -m 1h"
```

`mise which` returns a path inside one installed version, so update the setting
after upgrading ghtkn, or install ghtkn outside mise. Do not make the command run
`mise exec` or anything else that may need GitHub access to install ghtkn: that
can loop while mise is trying to get a token. Check the result with
`mise token github`.

### Native GitHub OAuth

mise can create short-lived GitHub App user access tokens itself with GitHub's
OAuth device flow, without a personal access token, an app private key or
secret, or an external command. Create a GitHub App with device flow enabled,
then configure its client ID and authorize once:

```sh
mise settings github.oauth_client_id=Iv1.yourgithubappclientid
mise token github --oauth
```

mise caches the token in its state directory, reuses it for its own GitHub API
calls, and refreshes it when GitHub issued a refresh token with it. After
changing the GitHub App's permissions or installation access, request a fresh
token:

```sh
mise token github --oauth --refresh
```

While a cached or refreshable token exists, mise also exports it as
`GITHUB_TOKEN` through `mise activate`, `mise hook-env`, `mise env` and
`mise exec`, so tools such as `gh` can use it:

```sh
mise exec -- gh pr list
```

Every program you run in that shell, including project tasks and scripts, can
read the exported token. A value set in mise's `[env]` takes precedence, but a
value already in your shell, such as a personal `GITHUB_TOKEN`, is replaced. Set
[`github.oauth_export_env`](/configuration/settings.html#github.oauth_export_env)
to another name, such as `GH_TOKEN`, or to `""` to keep your shell's value and
the token to mise itself. Exporting it does not configure Git credential helpers
or Cargo registry authentication.

`mise token github --oauth --raw` prints the token for a command that needs the
value. Copying it into `MISE_GITHUB_TOKEN` makes that variable win over OAuth
from then on.

The OAuth settings, with non-default examples:

```toml [~/.config/mise/config.toml]
[settings.github]
oauth_client_id = "Iv1.yourgithubappclientid"
oauth_open_browser = false       # do not open a browser; use the printed URL
oauth_copy_code = true          # copy the device code to the clipboard
oauth_export_env = "GH_TOKEN"    # or "" to turn off the export
```

Leave [`github.oauth_scopes`](/configuration/settings.html#github.oauth_scopes)
empty for a GitHub App: its user tokens get their access from the app's
permissions and installations.

### Git credential helpers

With `use_git_credentials` on, mise runs `git credential fill` as the last
source and caches the answer per host for the rest of the command. Use it in
devcontainers and other environments where Git already has credentials, such as
a keyring helper:

```toml [~/.config/mise/config.toml]
[settings.github]
use_git_credentials = true

[settings.gitlab]
use_git_credentials = true

[settings.forgejo]
use_git_credentials = true
```

mise runs Git with `GIT_TERMINAL_PROMPT=0`, so Git does not stop to ask for a
password in the terminal.

## Token priority

mise checks these sources in order and uses the first token it finds:

| Order | GitHub                                                            | GitLab                                                       | Forgejo                                                         |
| ----- | ----------------------------------------------------------------- | ------------------------------------------------------------ | --------------------------------------------------------------- |
| 1     | `MISE_GITHUB_ENTERPRISE_TOKEN` (hosts other than github.com)      | `MISE_GITLAB_ENTERPRISE_TOKEN` (hosts other than gitlab.com) | `MISE_FORGEJO_ENTERPRISE_TOKEN` (hosts other than codeberg.org) |
| 2     | `MISE_GITHUB_TOKEN`, then `GITHUB_API_TOKEN`, then `GITHUB_TOKEN` | `MISE_GITLAB_TOKEN`, then `GITLAB_TOKEN`                     | `MISE_FORGEJO_TOKEN`, then `FORGEJO_TOKEN`                      |
| 3     | `github.credential_command`                                       | `gitlab.credential_command`                                  | `forgejo.credential_command`                                    |
| 4     | Native OAuth, for the host of `github.oauth_api_url`              |                                                              |                                                                 |
| 5     | `github_tokens.toml`                                              | `gitlab_tokens.toml`                                         | `forgejo_tokens.toml`                                           |
| 6     | gh `hosts.yml`                                                    | glab `config.yml`                                            | fj `keys.json`                                                  |
| 7     | `git credential fill`, when enabled                               | `git credential fill`, when enabled                          | `git credential fill`, when enabled                             |

A wrong or expired token from a higher source hides a working one further down,
because mise stops at the first token it finds.

Tokens are looked up by host. `api.github.com` and `raw.githubusercontent.com`
use the token for `github.com`, so key token files and CLI logins by
`github.com`. GitLab and Forgejo tokens go only to the origin of the tool's
`api_url`.

## GitHub Enterprise and self-managed hosts {#github-enterprise}

Point the tool at the instance's API with the
[`api_url`](/dev-tools/backends/github.html#api-url) tool option:

```toml [mise.toml]
[tools]
"github:myorg/mytool" = { version = "latest", api_url = "https://github.mycompany.com/api/v3" }
"gitlab:myorg/mytool" = { version = "latest", api_url = "https://gitlab.mycompany.com/api/v4" }
"forgejo:myorg/mytool" = { version = "latest", api_url = "https://forgejo.mycompany.com/api/v1" }
```

mise looks up tokens for the host of `api_url`, here `github.mycompany.com`,
`gitlab.mycompany.com` and `forgejo.mycompany.com`. The Forgejo backend uses
Codeberg when `api_url` is not set. For GitHub Enterprise Cloud with data
residency (`api.<name>.ghe.com`), key tokens by `<name>.ghe.com`.

`MISE_GITHUB_TOKEN` and the other general variables also reach Enterprise hosts.
To use a different token for github.com and an Enterprise host, set
`MISE_GITHUB_ENTERPRISE_TOKEN`, or use a per-host source. One Enterprise
variable cannot serve several instances that need different tokens; use a token
file, the CLI logins, a credential command or Git credential helpers instead:

```sh
gh auth login --hostname github.mycompany.com
gh auth login --hostname github.other-company.com
```

For native OAuth with an Enterprise instance that supports device flow, also
set [`github.oauth_auth_url`](/configuration/settings.html#github.oauth_auth_url)
and [`github.oauth_api_url`](/configuration/settings.html#github.oauth_api_url).
See the [github](/dev-tools/backends/github.html),
[gitlab](/dev-tools/backends/gitlab.html) and
[forgejo](/dev-tools/backends/forgejo.html) backends for their other options.

## GitHub Actions and other CI {#ci-github-actions}

GitHub Actions provides a workflow token as `secrets.GITHUB_TOKEN`.
[`jdx/mise-action`](https://github.com/jdx/mise-action) passes it to mise by
default through its `github_token` input, and keeps it to the action unless you
set `persist_github_token`. For later steps that run mise, pass the token in the
environment:

```yaml
- uses: jdx/mise-action@v5
- name: Run tests
  run: mise run test
  env:
    GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

The workflow token's permissions and repository scope still apply. A private
tool in another repository needs a GitHub App token or a personal access token
with access to that repository, stored as an Actions secret; pass it to the
action as `github_token` and to later steps as `MISE_GITHUB_TOKEN`. See GitHub's
[workflow authentication guide](https://docs.github.com/en/actions/tutorials/authenticate-with-github_token).

On other CI systems, store the token as a secret and expose it as
`MISE_GITHUB_TOKEN`, `MISE_GITLAB_TOKEN` or `MISE_FORGEJO_TOKEN`.

## Troubleshooting

Start with [`mise token`](#check-which-token-mise-uses) to see which source mise
picks.

| Symptom                                                    | What to check                                                                                                          |
| ---------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| The source is an environment variable you did not expect   | Remove or fix it in the environment that runs mise; mise does not consult lower sources once one has a token           |
| `(none)` while `gh auth status` succeeds                   | gh keeps its token in the system keyring; see [the keyring section](#gh-keyring)                                       |
| A token is found but the repository is not accessible      | The token's repository access, expiry and organization authorization, and the API host                                 |
| `403` or `429` from GitHub                                 | The rate-limit details in the response; a `403` can also mean missing permissions. See [errors](/errors.html#http-403) |
| No token is used for a self-managed GitLab or Forgejo host | The tool's `api_url`; mise keys tokens by its host and sends them only to its origin                                   |
| OAuth refresh is rejected                                  | The GitHub App client ID; after fixing it, run `mise token github --oauth --refresh`                                   |

## Fewer API requests with lockfiles {#avoiding-tokens-entirely-with-lockfiles}

A [lockfile](/dev-tools/mise-lock.html) that records artifact URLs and checksums
lets later installs skip release discovery:

```sh
mise lock
mise install --locked
```

This reduces how often mise needs a token for public downloads. It does not make
private artifacts public or guarantee an offline install: missing platform
entries, provenance verification and packslip policy checks can still need the
network or a token. Keep the credentials CI needs available even with a
lockfile.

## netrc credentials {#netrc}

mise also reads `~/.netrc` (`%USERPROFILE%\_netrc` first on Windows) for HTTP
Basic authentication, for example for an Artifactory mirror. It is a fallback:
a token mise already attached to a request wins. The exception is a
[URL replacement](/url-replacements.html) that sends the request to a different
host. mise then drops the original host's credentials and uses matching netrc
credentials for the new host. Turn this off with
[`netrc`](/configuration/settings.html#netrc), or choose another file with
[`netrc_file`](/configuration/settings.html#netrc_file).
