# Remote bootstrap over SSH

`mise bootstrap remote` applies a bootstrap project to one or more machines
through the locally installed OpenSSH client. Targets can live in versioned
configuration or be supplied ad hoc from the command line.

Remote targets must provide a POSIX shell plus `cksum`, `mktemp`, `tar`, and `uname`.
Linux and macOS hosts satisfy these requirements by default. The orchestrating
machine needs local `ssh` and `tar` commands.

## Private configuration repositories

Install your configuration repository into the remote user's persistent global
mise configuration directory:

```sh
# Authenticate on this machine first (for example, using gh auth login).
mise bootstrap remote --host devbox --from-git jdx/dotfiles \
  --github-relay-read-only --github-relay-repo jdx/dotfiles
```

`OWNER/REPO` expands to `https://github.com/OWNER/REPO.git`. Explicit Git URLs,
SSH syntax, and local paths also work. `--from-git` conflicts with `--source`.
Targets still come from explicit `--host` or inventory selectors, never from the
downloaded repository's inventory.

mise fetches the repository once using **local Git authentication**, pins that
commit for every selected target, and transfers a Git bundle over SSH. It does
not modify the initiating machine's global configuration or copy its Git
configuration. The remote checkout retains the original credential-free origin,
its branch, and its upstream. Temporary staging is removed; the installed global
configuration is not. Use `--install-mise` to also install mise persistently when
the target does not already have it.

An existing matching checkout is reused unless `--update` requests a safe
fast-forward. Dirty checkouts, mismatched origins, conflicting files, and source
files ending in `.local.toml` require manual resolution. A nonempty non-Git
directory can be adopted after confirmation: existing files and local overrides
are preserved. `--dry-run` reports the required fetch/adoption work without
fetching the source or changing the target.

`--from-git` uses the repository instead of the inventory's archive source and
copy-link settings. Explicit `--source`, `--copy-link`, `--copy-links`, and
`--exclude` flags cannot be combined with it.

The relay is separate from this initial transfer. Enable it when bootstrap needs
additional private GitHub content, authorizing each required repository with a
repeated `--github-relay-repo`. Shorthand never enables or expands relay access.

## Borrowing GitHub access for one session

```sh
mise ssh devbox --github-relay-read-only --github-relay-repo jdx/dotfiles
mise ssh devbox --github-relay-read-only --github-relay-repo jdx/dotfiles \
  -- git clone https://github.com/jdx/dotfiles.git

# Deliberately allow every repository your local credential can read:
mise ssh devbox --github-relay-read-only --github-relay-all-repos

# Ordinary OpenSSH, without provisioning mise or starting a relay:
mise ssh devbox -i ~/.ssh/devbox -p 2222 -o ServerAliveInterval=30 -- uname -a
```

The same relay flags work with `mise bootstrap remote`. Enabling the relay
requires either a repository allowlist or explicit all-repository access, not
both. Credentials are resolved on the initiating machine using mise's existing
GitHub token resolution; there is no automatic login or new credential store.

The owned SSH connection forwards a private Unix socket. Remote mise requests
and Git's GitHub HTTPS/SSH transports use session-only adapters. The local broker
authorizes repository metadata, refs, contents, releases, assets, and smart-HTTP
clone/fetch. Pushes, API mutations, GraphQL, and other endpoints are denied.
Only approved GitHub asset redirects are followed, without authentication.
Requests are limited to 8 MiB and 32 accepted connections; responses are streamed.
The default concurrency is eight requests and the default total request timeout
is five minutes. Both limits can be configured on the initiating machine.

Borrowed access ends when the session ends, including failures and disconnects.
No GitHub token or persistent transport rewrite is installed on the target.
Future independent private-repository updates need that machine's own
credentials or another relay-enabled session. Without the relay, existing remote
authentication works as before.

::: warning Trust the target with the content you authorize
A compromised target can read authorized private content during the session.
Keeping credentials local limits credential exposure; it does not make the
target trustworthy. Use narrowly scoped repository allowlists.
:::

Relay support is initially limited to Linux/macOS clients and POSIX Linux/macOS
targets running a compatible mise. Windows, GitHub Enterprise, remote `gh`, write
operations, and unattended/persistent relay access are not supported.

Read-only access includes Git clone/fetch, repository metadata, contents, refs,
releases, release assets, and tar/zip source archives. Archive and asset redirects
are restricted to approved GitHub download hosts and never carry your credential.
Resume requests retain their range headers, and denied downloads fail rather than
being saved as artifacts.

### Observing and limiting borrowed access

```sh
mise ssh devbox --github-relay-read-only --github-relay-repo jdx/dotfiles \
  --github-relay-log-requests --github-relay-max-duration 1h

# Structured relay events for troubleshooting or auditing:
mise bootstrap remote --host devbox --from-git jdx/dotfiles \
  --github-relay-read-only --github-relay-repo jdx/dotfiles \
  --github-relay-log-requests --github-relay-log-format jsonl
```

Request logs are off by default. Enable them per invocation or save preferences
in your **local global** mise configuration:

```toml
[settings.github_relay]
log_requests = true
log_format = "text" # or "jsonl"
max_duration = "1h" # default "0s": until the session ends
request_timeout = "5m"
concurrency = 8 # 1-32; excess requests fail closed rather than queue
```

`--github-relay-no-log-requests` overrides a saved logging preference. The format
and duration flags also override their respective settings. These preferences
never enable the relay or authorize repositories: access and scope still require
explicit flags on every invocation.

Events go to the initiating machine's stderr, not the remote command's stdout.
They show the method, repository and fixed operation name, status, and time to
response headers. Approved download redirects are identified by host only. Query
values, refs, filenames, headers, credentials, bodies, and signed download URLs
are omitted; rejected paths appear as `unapproved operation`. JSONL applies to
relay events; other mise diagnostics can still appear on stderr.

Every relay prints a session-end summary, even when request logging is off:
requests received (excluding heartbeat probes), denied/unavailable requests,
upstream response bytes received, and up to 128 authorized repositories requested.
Redirects are separate log events but do not add to the incoming request count.

The duration limit starts when the local relay is created. Expiration immediately
revokes borrowed access and cancels active transfers; the remote adapter then
ends its command after detecting failed heartbeat probes. No credentials are
installed to extend access beyond this limit.

```toml
[bootstrap.remote]
source = "."
exclude = [".env.local", "artifacts"]
copy_link = ["modules/common", "playbooks/shared"]
mise_env = ["linux", "server"]

[bootstrap.remote.hosts.cache]
host = "cache.example.com"
user = "ubuntu"
port = 22
identity_file = "~/.ssh/mise-cache"
tags = ["cache", "production"]
ssh_options = ["ServerAliveInterval=30"]
mise_env = ["linux", "cache"]
```

`source` is the local project directory sent to the host. Relative `source`,
`identity_file`, and `mise_bin` paths are resolved from the config file that
declares them. A host-level `source` overrides `[bootstrap.remote].source`.
A host-level `mise_env` overrides `[bootstrap.remote].mise_env`; the ordered
values are passed as `MISE_ENV` to the staged `mise bootstrap` process. Use
`--remote-env <ENV>` to override the configured list for every selected host.
Higher-precedence config files win when the same inventory name is declared in
more than one layer. Top-level `exclude` patterns are unioned across every
loaded config layer and applied to every host, so a nearer project can add
secret patterns even when the inventory entry comes from global config.
This shared set also applies to ad-hoc `--host` targets; inventory host-level
excludes are additive. Only selected inventory entries are validated. mise
applies command-line overrides and validates the entire selected set before it
opens any SSH connection, so a stale unselected entry does not block an
unrelated target, and a selected invalid entry cannot cause a partial run.

Remote inventory is orchestration metadata. A `mise bootstrap` process running
inside the staged project does not recursively execute its
`[bootstrap.remote]` section.

## Selecting hosts

Target names are explicit by default, so an accidental bare command cannot
provision every server in an inventory:

```sh
# one or more named inventory entries
mise bootstrap remote cache

# every inventory host, or hosts matching any repeated tag
mise bootstrap remote --all
mise bootstrap remote --tag cache --tag canary

# a server that is not in inventory
mise bootstrap remote --host ubuntu@cache.example.com \
  --identity-file ~/.ssh/mise-cache \
  --source ./infra/mise-cache
```

Named and ad-hoc selectors may be combined. Explicit target names run first in
command-line order. `--all` and `--tag` then add remaining inventory hosts in
declaration order, followed by ad-hoc `--host` destinations in command-line
order. By default, mise continues after a failed target and reports every
failure at the end; `--fail-fast` stops at the first failure.

Command-line connection, source, and mise bootstrap options override every
selected host. `--ssh-option` maps directly to a separate OpenSSH `-o`
argument, so ProxyJump, custom host-key files, and other native OpenSSH
features remain available without mise inventing a second SSH configuration
language.

## Transport and staging

For each target, mise:

1. opens an OpenSSH connection using the user's normal SSH config and host-key
   policy;
2. creates a validated `/tmp/mise-bootstrap.*` directory;
3. archives the source directory locally and extracts it into the staging
   directory;
4. provisions the exact mise executable used for the remote run, staging it
   unless `install_mise` keeps it on the host;
5. executes `mise bootstrap` in the staged project; and
6. removes the staging directory, including after a failed bootstrap.

On Unix, all commands for one target reuse an OpenSSH control connection. In a
non-interactive caller, mise sets `BatchMode=yes` so password prompts fail
instead of hanging. In an attended terminal, the bootstrap command gets a TTY
so SSH, sudo, confirmation, and `--prompt-secrets` prompts remain usable.
OpenSSH's existing host-key verification is never weakened automatically.

`.git`, `target`, and `node_modules` are excluded from source archives by
default. Add repeatable `exclude` config entries or `--exclude` flags for
generated files and local secrets. Use `--keep-staging` only for debugging; it
prints the retained path instead of deleting the directory.

Symbolic links are archived as links by default. Use repeatable, source-relative
`copy_link` entries or `--copy-link <PATH>` flags to replace only named links
with their targets in the staged project. A selected directory link is copied
as a real directory while links nested inside its target remain links. This is
the safer choice for sharing selected modules or playbooks without changing
unrelated links in deep dependency trees. Host-level `copy_link` entries add to
the top-level list, and command-line entries add to both.

Set `copy_links = true` or pass `--copy-links` to dereference every symbolic
link encountered recursively. This matches tools such as `rsync --copy-links`,
but can unexpectedly expand small links deep in vendored, generated, or
dependency trees and can copy content outside the source directory. Explicit
`copy_link` selections are ignored when this global mode is enabled.

## Provisioning mise itself

By default, mise detects the remote OS, architecture, and Linux libc family. It
uploads the current local executable when that executable is compatible with
the target. This guarantees the remote process supports the same bootstrap
configuration as its orchestrator rather than silently using an older installed
version.

On Linux, mise also inspects the executable's ELF interpreter. Static binaries
can run without a target libc check. For dynamically linked binaries, the
remote host must provide the exact interpreter path and the same libc family.
For glibc binaries, mise extracts the highest required `GLIBC_*` symbol version
from the ELF and verifies that the remote loader provides at least that ABI
before upload. For musl binaries, mise compares the local and remote loader
versions and requires the remote loader to be at least as new. mise then runs
`mise version` remotely as the final authority for all other binary and host
requirements.

When the local executable cannot run on the target, mise automatically resolves
the raw executable for the same mise version from the official GitHub release.
This covers Linux x64, arm64, and armv7 on both glibc and musl, plus macOS x64
and arm64. mise downloads `SHASUMS256.txt` and its minisign signature, verifies
the manifest with mise's embedded release key, then verifies the selected
artifact's SHA-256 checksum before upload. The verified artifact is cached for
the duration of the command, so targets with the same platform share one
download.

Automatic substitution is deliberately limited to official release binaries.
Before downloading a different target, mise proves that the local executable
matches one of the signed checksums for the same official release. Debug builds,
source builds with local changes, and downstream-packaged binaries therefore
fail closed rather than silently changing code on the remote machine. Use an
explicit strategy below for those builds or for a platform outside the official
artifact matrix. Failure to identify a Linux libc family also requires an
explicit strategy.

Three explicit escape hatches cover other environments:

- `mise_bin` / `--mise-bin` uploads a user-built local executable. This is the
  primary path for architectures without an official precompiled binary.
- `remote_mise` / `--remote-mise` runs a known compatible command already on
  the host without uploading a binary.
- `bootstrap_command` / `--bootstrap-command` runs an explicit remote shell
  command in a login shell, then opens a fresh login shell to locate `mise`
  from the post-install profile, inherited `PATH`, or common user install
  directories. mise snapshots a content fingerprint and the reported version
  of each discoverable executable before installation and prefers a newly added
  or identity-changed path afterward, so an older executable earlier on `PATH`
  cannot shadow the installed one. Ambiguous
  unchanged candidates fail with instructions to select an explicit path. This
  supports source builds, installers that edit shell profiles, and site-specific
  installers. A dry run never executes this
  command; it uses an already-installed remote `mise` or fails with
  instructions to select `remote_mise` or `mise_bin`.

These strategies are mutually exclusive. Supplying one on the command line
replaces any provisioning strategy declared by the selected inventory host.
`remote_mise` is an executable name or path, not a shell expression. Bare names
are resolved to an absolute executable through the remote login `PATH`, `~/`
paths use the remote login home, absolute paths are used as written, and
relative paths such as `./bin/mise` resolve inside the staged project. Relative
paths that escape the staged project are rejected. Paths may contain whitespace
and are always passed as one executable argument. Use `bootstrap_command` when
shell evaluation is required.

```toml
[bootstrap.remote.hosts.arm-lab]
host = "arm-lab.example.com"
mise_bin = "./artifacts/mise-linux-armv5"

[bootstrap.remote.hosts.nix-builder]
host = "builder.example.com"
bootstrap_command = "nix profile install nixpkgs#mise"
```

mise verifies every uploaded or selected remote command by running
`mise version` before bootstrap.

### Leaving mise installed on the host

By default the provisioned executable lives in the staging directory and is
removed with it, so a target keeps the tools installed under
`~/.local/share/mise` but not the mise that installed them. Set `install_mise`
to keep mise on the machine:

```toml
[bootstrap.remote]
install_mise = true

[bootstrap.remote.hosts.cache]
host = "cache.example.com"
install_mise = "/usr/local/bin/mise"
```

`true` installs to `~/.local/bin/mise`, the same path used by
[mise.run](https://mise.run). A string installs to that path instead; it must be
absolute or start with `~/`, and it names the executable rather than a directory
— a path that already holds a directory is rejected instead of receiving the
executable as a child entry.
A host-level value replaces `[bootstrap.remote].install_mise`, so
`install_mise = false` opts one host out of a project-wide default.

```sh
mise bootstrap remote cache --install-mise
mise bootstrap remote cache --install-mise=/usr/local/bin/mise
mise bootstrap remote cache --no-install-mise
```

`--install-mise` requires `=` before a path so that a bare flag cannot consume a
target name. Like the other command-line provisioning options, it replaces a
`remote_mise` or `bootstrap_command` declared by a selected inventory host.

What gets installed is the same executable the default strategy would have
staged — the local binary or the checksum-verified official release artifact —
and it is what runs the bootstrap, so the host converges with the mise version
that orchestrated it. mise writes a temporary file beside the target and renames
it into place, so replacing a mise that is currently running cannot truncate it.
When the target already holds a byte-identical executable, nothing is uploaded.
A dry run never writes to the host: `--dry-run` reports the path it would
install to and stages the executable as usual.

`install_mise` composes with `mise_bin`, which installs a locally built
executable. It cannot be combined with `remote_mise` or `bootstrap_command`,
because both already provide mise on the host. `bootstrap_command` remains the
right choice when the host should own the install through a system package,
`nix profile install`, or a site-specific installer.

The SSH user must be able to write the install path; mise does not elevate for
it, so a path such as `/usr/local/bin/mise` needs a user who already owns that
directory. Keep that directory writable only by that user. After installing,
mise compares the target's digest with what it wrote and fails rather than
running something else, but that check is best-effort — it is skipped when the
host provides neither `sha256sum` nor `shasum`, and it cannot cover the window
between the check and the run. Anyone who can write the install directory
controls what that account runs as `mise` on every later invocation regardless,
so the directory's permissions are the real boundary. The staging directory used
without `install_mise` is created by `mktemp -d` and is private to the SSH
account.

Installing mise does not put it on the host's `PATH`. mise warns when the
install directory is missing from the login `PATH`, and the bootstrap project
can declare [`[bootstrap.mise_shell_activate]`](/bootstrap/shell.html) so the
same run writes activation or shims into the host's shell startup files.

## Bootstrap controls and secrets

Remote execution forwards the important convergence controls directly:

```sh
mise bootstrap remote cache --dry-run
mise bootstrap remote cache --yes --update
mise bootstrap remote cache --only packages,files,services,compose
mise bootstrap remote cache --skip tools,task
mise bootstrap remote cache --prompt-secrets
mise bootstrap remote cache --remote-env linux,server
```

Local environment variables are deliberately not copied to SSH hosts. An
explicitly configured `mise_env` is remote orchestration metadata rather than
an inherited local environment. Use `--prompt-secrets` for an attended run.
Provider-backed secret environment transport can be layered on separately
without putting values in config, archives, process arguments, plans, or logs.
