---
description: "Apply your bootstrap configuration to other machines over SSH, from an inventory or from ad-hoc hosts."
socialDescription: "Apply your bootstrap configuration to other machines over SSH."
---

# Remote hosts

`mise bootstrap remote` sends a bootstrap project to other machines over SSH,
stages mise on each one, and runs `mise bootstrap` there. Hosts come from an
inventory in your configuration or from `--host` on the command line.

## Requirements

- The machine you run from (Linux, macOS, or Windows) needs OpenSSH's `ssh` and
  `tar`.
- Each host needs a POSIX shell plus `cksum`, `mktemp`, `tar`, and `uname`.
  Minimal images may need these installed first.
- Windows hosts, which use native SSH with PowerShell, are not supported.

## First run

Use a project whose bootstrap configuration you have reviewed, and an SSH host
you can already reach. Replace `devbox` with your SSH host or alias:

```sh
ssh devbox uname -s
mise bootstrap remote --host devbox --dry-run
mise bootstrap remote --host devbox
```

`--source <DIR>` picks the project directory to send. It defaults to the current
directory, or to an inventory host's `source`. A remote dry run still connects,
sends the project, stages mise, and inspects the host. It skips only the
bootstrap's changes, so it is not an offline plan.

::: warning The host does not keep mise
By default, mise runs from a temporary staging directory and removes it
afterwards. The host keeps the tools it installed under `~/.local/share/mise`,
but not mise itself. Add `--install-mise` if the configuration writes shell
activation, runs user services, or should run again on the host. See
[Keep mise on the host](#leaving-mise-installed-on-the-host).
:::

Check what the project sends before you send a repository that holds local
secrets or generated data; see [What gets sent](#what-gets-sent).

## Inventory {#inventory-configuration}

Declare hosts under `[bootstrap.remote.hosts]` so you can select them by name or
tag:

```toml
[bootstrap.remote]
source = "."
exclude = [".env.local", "artifacts"]
mise_env = ["linux", "server"]

[bootstrap.remote.hosts.cache]
host = "cache.example.com"
user = "ubuntu"
tags = ["cache", "production"]
mise_env = ["linux", "cache"]
```

| Key                 | Set in                         | Meaning                                                                     |
| ------------------- | ------------------------------ | --------------------------------------------------------------------------- |
| `host`              | A host                         | SSH host name or alias (required)                                           |
| `user`              | A host                         | SSH user                                                                    |
| `port`              | A host                         | SSH port                                                                    |
| `identity_file`     | A host                         | SSH private key                                                             |
| `ssh_options`       | A host                         | OpenSSH `-o` options, such as `ServerAliveInterval=30`                      |
| `tags`              | A host                         | Labels that `--tag` selects                                                 |
| `source`            | `[bootstrap.remote]` or a host | Local project directory to send                                             |
| `exclude`           | `[bootstrap.remote]` or a host | Patterns to leave out of what is sent                                       |
| `copy_link`         | `[bootstrap.remote]` or a host | Symbolic links to send as their targets                                     |
| `copy_links`        | `[bootstrap.remote]` or a host | Send every symbolic link as its target                                      |
| `mise_env`          | `[bootstrap.remote]` or a host | [Config environments](/configuration/environments.html) to load on the host |
| `install_mise`      | `[bootstrap.remote]` or a host | Keep mise on the host: `true`, `false`, or an executable path               |
| `mise_bin`          | A host                         | Local mise executable to upload                                             |
| `remote_mise`       | A host                         | A mise already installed on the host                                        |
| `bootstrap_command` | A host                         | Shell command that installs mise on the host                                |

These rules decide each host's final settings:

- Relative `source`, `identity_file`, and `mise_bin` paths resolve from the
  config file that declares them.
- Top-level `source`, `mise_env`, `install_mise`, `copy_link`, and `copy_links`
  are defaults for the hosts declared in the same config file. A host's own
  value replaces the default, except `copy_link`, which adds to it.
  `install_mise = false` opts one host out of a default.
- Top-level `exclude` patterns from every loaded config file apply to every
  host, including `--host` targets, so a project can add secret patterns to an
  inventory that lives in global config. A host's `exclude` adds to them.
- When two config files declare the same host name, the more local one wins.
- `mise_env` is passed to the host's `mise bootstrap` as `MISE_ENV`.
  `--remote-env <ENV>` replaces it for every selected host.
- mise checks only the hosts you select, and it checks all of them, with
  command-line overrides applied, before it opens any connection. A stale entry
  you did not select does not block a run, and an invalid selected one cannot
  cause a partial run.

The `mise bootstrap` that runs on a host ignores `[bootstrap.remote]`, so a host
never bootstraps other hosts.

## Select hosts {#selecting-hosts}

A bare `mise bootstrap remote` selects nothing, so a mistyped command cannot
reach every server in an inventory. Name the hosts you want:

```sh
# one or more inventory hosts by name
mise bootstrap remote cache

# every inventory host, or the hosts with any of these tags
mise bootstrap remote --all
mise bootstrap remote --tag cache --tag canary

# a server that is not in the inventory
mise bootstrap remote --host ubuntu@cache.example.com \
  --identity-file ~/.ssh/mise-cache \
  --source ./infra/mise-cache
```

You can combine these. mise bootstraps one host at a time: named hosts first,
in command-line order, then the other hosts that `--all` or `--tag` select, in
inventory order, then `--host` destinations in command-line order. After a
failed host, mise continues and reports every failure at the end;
`--fail-fast` stops at the first one.

Command-line options apply to every selected host. `--port` and `-i` replace
the host's inventory value. `--ssh-option` adds to the host's `ssh_options` and
is passed after them; OpenSSH uses the first value it gets for an option, so it
cannot replace an option the inventory already sets.

| Option                    | Effect                                                                      |
| ------------------------- | --------------------------------------------------------------------------- |
| `--port <PORT>`           | SSH port                                                                    |
| `-i`, `--identity-file`   | SSH private key                                                             |
| `--ssh-option <OPTION>`   | One OpenSSH `-o` option, such as `ProxyJump=bastion`; repeat for more       |
| `--connect-timeout <SEC>` | OpenSSH `ConnectTimeout` for every connection, 10 seconds unless you set it |

mise passes its own `ConnectTimeout` before any of these options, so a
`ConnectTimeout` in `--ssh-option` or `ssh_options` has no effect; use
`--connect-timeout` instead.

mise also reads your normal `~/.ssh/config` and keeps OpenSSH's host-key checks.

## What gets sent

mise archives the source directory and sends it to each host, leaving out
`.git`, `target`, and `node_modules`. Add `exclude` entries or `--exclude`
flags for local secrets and generated files. `--keep-staging` keeps the staging
directory on the host for debugging and prints its path.

Symbolic links are sent as links. To send a link's target instead, list the
link, relative to the source, in `copy_link` or pass `--copy-link <PATH>`. A
directory link becomes a real directory while links inside it stay links, so
you can share selected modules or playbooks without changing other links. A
host's `copy_link` entries add to the top-level list, and command-line entries
add to both.

`copy_links = true` or `--copy-links` replaces every link with its target, like
`rsync --copy-links`. That can pull in large vendored, generated, or dependency
trees, and files from outside the source directory. It also makes mise ignore
`copy_link`.

## Get mise onto the host {#provisioning-mise-itself}

By default, mise gives each host the same mise version you are running, so the
host reads your configuration the way your machine does. If your executable
runs on the host, mise uploads it. Otherwise it downloads that version's
official release for the host's platform (Linux x64, arm64, or armv7 with glibc
or musl, or macOS x64 or arm64), checks its signature and checksum, and uploads
it. Hosts on the same platform share one download. Before bootstrapping, mise
runs `mise version` on the host to check that the executable works.

mise substitutes an official release only for an official release. A debug
build, a source build with local changes, or a mise packaged by a distribution
cannot be swapped, and neither can a platform outside that list or a Linux host
whose libc mise cannot identify. For those, choose one of these options:

| Config key, flag                           | What it does                                                                                      |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------- |
| `mise_bin`, `--mise-bin`                   | Upload a mise you built yourself, for example for an architecture with no official release        |
| `remote_mise`, `--remote-mise`             | Use a mise already installed on the host                                                          |
| `bootstrap_command`, `--bootstrap-command` | Run a shell command on the host that installs mise, such as a package manager or a site installer |

```toml
[bootstrap.remote.hosts.arm-lab]
host = "arm-lab.example.com"
mise_bin = "./artifacts/mise-linux-armv5"

[bootstrap.remote.hosts.nix-builder]
host = "builder.example.com"
bootstrap_command = "nix profile install nixpkgs#mise"
```

You can set only one of these per host, and a command-line option replaces the
one the inventory sets.

`remote_mise` is an executable name or path, not a shell expression. A bare
name resolves through the host's login `PATH`, a `~/` path uses the host's home
directory, and an absolute path is used as written. A relative path such as
`./bin/mise` resolves inside the staged project and cannot point outside it.

`bootstrap_command` runs in a login shell. mise then opens a fresh login shell
to find `mise`, preferring an executable that is new or changed since before
the command, so an older mise earlier on `PATH` cannot shadow the new one. If
it cannot tell which executable is new, it stops and asks you to set
`remote_mise`. A dry run never runs this command: it uses a mise already on the
host, or stops and asks you to set `remote_mise` or `mise_bin`.

::: details How mise decides whether your executable runs on the host
mise detects the host's operating system, architecture, and, on Linux, libc
family. On Linux it also reads your executable's ELF interpreter. A static
executable needs no libc check. A dynamically linked one needs the same
interpreter path and libc family on the host. For glibc, the host's loader must
provide the highest `GLIBC_*` symbol version the executable requires. For musl,
the host's loader must be at least as new as yours. `mise version` on the host
has the final say.

Before it downloads a release for another platform, mise checks that your
executable matches one of the signed checksums for its own official release.
It verifies the release's `SHASUMS256.txt` with its minisign signature and
mise's embedded release key, then the downloaded file's SHA-256 checksum.
:::

### Keep mise on the host {#leaving-mise-installed-on-the-host}

Set `install_mise` to keep the executable mise used on the host after the run:

```toml
[bootstrap.remote]
install_mise = true

[bootstrap.remote.hosts.cache]
host = "cache.example.com"
install_mise = "/usr/local/bin/mise"
```

`true` installs to `~/.local/bin/mise`, where [mise.run](https://mise.run)
installs it. A string names a different executable path. It must be absolute
or start with `~/`, and it cannot be an existing directory. The same choices
work on the command line:

```sh
mise bootstrap remote cache --install-mise
mise bootstrap remote cache --install-mise=/usr/local/bin/mise
mise bootstrap remote cache --no-install-mise
```

A path needs the `=`, so a bare `--install-mise` cannot swallow a host name.
`--install-mise` replaces a `remote_mise` or `bootstrap_command` that the
inventory sets. In config, `install_mise` works with `mise_bin` but not with
`remote_mise` or `bootstrap_command`, which already provide a mise on the host;
use `bootstrap_command` when the host should own the install through its
package manager.

mise installs the executable it would otherwise have staged, then runs the
bootstrap with it, so the host ends on the version you ran. Replacing a mise
that is running is safe, and an identical executable is not uploaded again. A
dry run reports the install path but uses a temporary copy.

::: warning Protect the install directory
mise does not use sudo to install itself, so the SSH user must be able to write
the install path; `/usr/local/bin/mise` needs a user who owns that directory.
Anyone else who can write to the directory can replace `mise` for that user, so
keep it writable only by that user. mise checks the installed file's digest,
but only when the host has `sha256sum` or `shasum`, and the check cannot cover
later changes. Without `install_mise`, mise stages into a private `mktemp -d`
directory instead.
:::

Installing mise does not put its directory on the host's `PATH`, and mise warns
when the login `PATH` lacks it. `~/.local/bin` is on `PATH` by default on many
Linux distributions, but not on macOS. Add the directory in the host's shell
startup files, for example with a
[`[dotfiles]` line entry](/bootstrap/shell.html#put-mise-on-path-first), before
you rely on [`[bootstrap.mise_shell_activate]`](/bootstrap/shell.html): its
blocks call `mise` by name.

## Options and secrets {#bootstrap-controls-and-secrets}

Remote runs accept the same `--dry-run`, `--yes`, `--update`, `--only`,
`--skip`, `--force-dotfiles`, and `--prompt-secrets` options as
[`mise bootstrap`](/bootstrap.html#choose-what-runs), plus `--remote-env` to
choose config environments on the host:

```sh
mise bootstrap remote cache --dry-run
mise bootstrap remote cache --yes --update
mise bootstrap remote cache --only packages,files,services,compose
mise bootstrap remote cache --skip tools,task
mise bootstrap remote cache --force-dotfiles
mise bootstrap remote cache --prompt-secrets
mise bootstrap remote cache --remote-env linux,server
```

mise does not copy your local environment to the host. A local `-E` only
changes which local config files mise reads; the host loads the environments in
`mise_env` or `--remote-env`. [Secret inputs](/bootstrap/secrets.html) set on
this machine are not available there either. Use `--prompt-secrets` for an
attended run, or make the values available on the host.

## Adopt a configuration repository on a host {#private-configuration-repositories}

`--adopt` installs a configuration repository on each host instead of sending a
project. It accepts the same two kinds of repository as
[local adoption](/bootstrap/from-repository.html):

```sh
mise bootstrap remote --host devbox --install-mise --adopt you/mise-config
```

`OWNER/REPO` expands to `https://github.com/OWNER/REPO.git`. HTTPS and SSH URLs,
and local paths on this machine, work too. Other transports and custom Git
helpers are rejected, and clones do not follow HTTP redirects, so use the
repository's canonical URL. `--adopt` cannot be combined with `--source`,
`--copy-link`, `--copy-links`, or `--exclude`. Hosts still come from `--host`
or the inventory, never from the adopted repository's inventory.

mise fetches the repository once on this machine, with this machine's Git
credentials, pins that commit for every selected host, and sends it to each one
as a Git bundle over SSH. It does not change this machine's global config or
copy its Git configuration, and a private repository needs no extra flags. Add
`--install-mise` when the host does not have mise yet.

You do not need the [GitHub relay](/bootstrap/github-relay.html) for this
transfer. Add `--github-relay-read-only --github-relay-repo OWNER/REPO` only
when the bootstrap that follows needs other private GitHub content, such as a
private `[bootstrap.repos]` entry or a private release, and repeat
`--github-relay-repo` for each repository.

### Global configuration

A repository of global mise config becomes a checkout in the host's global
config directory, with the original credential-free origin, branch, and
upstream. The checkout stays after the run. On the host:

- An existing checkout with the same origin is reused, and `--update`
  fast-forwards it.
- Uncommitted changes, a different origin, conflicting files, or repository
  files ending in `.local.toml` stop the run for you to fix.
- A non-empty directory that is not a Git checkout can be adopted after you
  confirm, and its existing files and local overrides are kept.

With `--dry-run`, mise does everything except change the host: it fetches,
connects, stages, runs every check, and reports whether it would clone,
fast-forward, or adopt, with the number of new files. If the host already has a
global config, the bootstrap that follows is previewed too.

### Setup repository

For a setup repository, one connected with `mise dot origin set`, the host
restores your tracked files instead of checking the repository out, then records
the origin. Track the mise config and template sources the bootstrap needs on
your first machine; a file that is only referenced is not included. If that
config declares the history watcher, bootstrap installs it like any other user
service. If a file already on the host differs, setup stops before bootstrap
runs.

```sh
mise bootstrap remote --host devbox --install-mise --adopt you/setup --dry-run
```

With `--dry-run`, the host shows the files it would write and any held for a
decision, and records nothing. A setup preview can stage decrypted tracked
configuration, so treat a directory kept with `--keep-staging` as sensitive.

If setup succeeds but the host cannot reach the repository on its own
afterwards, the bootstrap says so. Give the host its own credentials for
ongoing synchronization, for example by running `gh auth login` and
`gh auth setup-git` there, or by setting an SSH URL with
`mise dot origin set`.

## Borrow GitHub access

`mise bootstrap remote` accepts the same `--github-relay-*` flags as
[`mise ssh`](/cli/ssh.html), which let Git and mise on the host read private
GitHub repositories with this machine's credentials for the length of the run.
See [GitHub relay](/bootstrap/github-relay.html).

## How a remote run works {#transport-and-staging}

For each host, mise:

1. Opens an OpenSSH connection with your normal SSH config and host-key policy.
2. Creates a private `/tmp/mise-bootstrap.*` staging directory with `mktemp`.
3. Archives the source directory locally and extracts it into the staging
   directory, or sends the repository bundle for `--adopt`.
4. Provides mise, staged or installed as described above.
5. Runs `mise bootstrap` from the staged project or the adopted configuration.
6. Removes the staging directory, even after a failed bootstrap, unless you
   pass `--keep-staging`.

On Unix, every command for one host shares one OpenSSH control connection. When
nobody is at the terminal, mise sets `BatchMode=yes`, so a password prompt fails
instead of hanging. In an attended terminal, the bootstrap gets a TTY, so SSH,
sudo, confirmation, and `--prompt-secrets` prompts work. mise never relaxes
OpenSSH's host-key checks.
