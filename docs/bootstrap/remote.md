# Remote bootstrap over SSH

`mise bootstrap remote` applies a bootstrap project to one or more machines
through the locally installed OpenSSH client. Targets can live in versioned
configuration or be supplied ad hoc from the command line.

Remote targets must provide a POSIX shell plus `cksum`, `mktemp`, `tar`, and `uname`.
Linux and macOS hosts satisfy these requirements by default. The orchestrating
machine needs local `ssh` and `tar` commands.

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
