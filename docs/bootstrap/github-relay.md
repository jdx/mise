---
description: "Lend a remote host read-only access to private GitHub repositories for one SSH session, without copying your token to it."
socialDescription: "Lend a remote host read-only GitHub access for one SSH session."
---

# GitHub relay (mise ssh)

[`mise ssh`](/cli/ssh.html) opens an SSH session like `ssh`. With
`--github-relay-read-only`, Git and mise on the remote host can read the private
GitHub repositories you name, using this machine's GitHub credentials, until the
session ends. The token never leaves this machine.

## Examples

```sh
# a shell on devbox that can read one private repository
mise ssh devbox --github-relay-read-only --github-relay-repo you/setup

# run one command instead of a shell
mise ssh devbox --github-relay-read-only --github-relay-repo you/setup \
  -- git clone https://github.com/you/setup.git

# allow every repository your local credentials can read
mise ssh devbox --github-relay-read-only --github-relay-all-repos

# plain OpenSSH, with no relay
mise ssh devbox -i ~/.ssh/devbox -p 2222 -o ServerAliveInterval=30 -- uname -a
```

`--github-relay-read-only` needs exactly one of `--github-relay-repo`, repeated
for each repository, or `--github-relay-all-repos`. The other `--github-relay-*`
flags work only with `--github-relay-read-only`. mise finds the GitHub token on
this machine the way it does for downloads (see
[Git provider tokens](/dev-tools/github-tokens.html)); it does not log in for
you or store new credentials.

Without relay flags, `mise ssh` runs OpenSSH and passes `-i`, `-p`, and `-o`
through. With the relay, mise also stages a temporary copy of itself on the
host for the session, the same way [remote bootstrap](/bootstrap/remote.html)
does, and removes it afterwards. Nothing is installed on the host.

[`mise bootstrap remote`](/bootstrap/remote.html) takes the same
`--github-relay-*` flags, and the borrowed access then covers the
`mise bootstrap` that runs on the host. Adopting a configuration repository on a
host does not need the relay; see
[Remote hosts](/bootstrap/remote.html#private-configuration-repositories).

## What the relay allows

Git's GitHub remotes, over HTTPS or SSH, and mise's own GitHub requests on the
host go through the relay for the session. For the repositories you name, it
allows:

- clone and fetch
- repository metadata, contents, and refs
- releases and release assets
- tar and zip source archives

It refuses pushes, API writes, GraphQL, and every other request. A download that
redirects to one of GitHub's download hosts is followed without your
credentials, and other redirects are refused. A resumed download keeps its range
header, and a refused download fails instead of being saved as a file.

## When access ends

Borrowed access ends when the session ends, including when it fails or
disconnects. mise installs no token and leaves no Git configuration on the
host, so afterwards the host needs its own credentials, or another relay
session, to fetch private repositories.

`--github-relay-max-duration` ends access sooner:

```sh
mise ssh devbox --github-relay-read-only --github-relay-repo you/setup \
  --github-relay-max-duration 1h
```

The limit starts when the relay starts. When it expires, mise revokes access
and cancels transfers in progress, and the command on the host ends once it
notices that the relay is gone.

## Limits

- A request body can be at most 8 MiB, and the relay accepts at most 32
  connections. Responses are streamed.
- [`github_relay.concurrency`](/configuration/settings.html#github_relay.concurrency)
  caps the requests that run at once; requests beyond it fail instead of
  waiting.
- [`github_relay.request_timeout`](/configuration/settings.html#github_relay.request_timeout)
  limits each request, including its streamed response.

## Logging

Request logging is off by default. Turn it on for one session with
`--github-relay-log-requests`, and choose `text` or `jsonl` output with
`--github-relay-log-format`:

```sh
mise bootstrap remote --host devbox --adopt you/setup \
  --github-relay-read-only --github-relay-repo you/private-tools \
  --github-relay-log-requests --github-relay-log-format jsonl
```

To keep a preference, set it in the global config of the machine you run the
command from. Project config cannot set these settings:

```toml [~/.config/mise/config.toml]
[settings.github_relay]
log_requests = true
log_format = "jsonl"
max_duration = "1h"
```

`--github-relay-no-log-requests` turns logging off for one session, and the
format and duration flags override their settings. The settings never turn the
relay on or choose repositories, so every session still needs the flags.

Events go to this machine's stderr, not to the remote command's output. Each
event shows the method, the repository, the operation, the status, and the time
to the response headers. A redirect to a GitHub download host shows the host
only. Query values, refs, filenames, headers, credentials, bodies, and signed
download URLs are left out, and a refused request appears as
`unapproved operation`. `jsonl` applies to relay events only; other mise
messages can still appear on stderr.

Every relay prints a summary when it ends, even with logging off: the requests
it received (not counting heartbeat probes), the requests it refused or could
not serve, the bytes it received from GitHub, and up to 128 repositories that
were requested. Redirects appear as separate log events but do not add to the
request count.

## Settings

<Settings child="github_relay" :level="3" />

## Security

::: warning Trust the host with what you authorize
A compromised host can read the authorized private content during the session.
Keeping the token on this machine limits what can leak; it does not make the
host trustworthy. Name only the repositories the session needs, rather than
using `--github-relay-all-repos`.
:::

## Platform support

Both machines must run Linux or macOS, and the host must be able to run the
staged mise. The relay does not support Windows, GitHub Enterprise, `gh` on the
remote host, writes, or access that outlasts the session.
