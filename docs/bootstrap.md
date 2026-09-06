# Bootstrap

`mise bootstrap` applies the machine setup declared in your mise configuration:
packages, files, services, repositories, shell setup, tools, and a final task.
Use it for workstation or server setup that needs more than installing
`[tools]`. Run it explicitly when you want to apply that configuration.

Start with the parts your machine needs, preview them, and add more resources
as the configuration grows. Each section has its own status and apply commands.
For SSH targets, see [remote bootstrap](/bootstrap/remote.html).

## Example

This small `mise.toml` configures zsh activation, installs Node.js, and verifies
it in a final task. Choose the [shell entries](/bootstrap/shell.html) for the
shell you actually use:

```toml
[bootstrap.mise_shell_activate]
zprofile = "shims"
zshrc = "activate"

[tools]
node = "24"

[tasks.bootstrap]
run = "node --version"
```

Review the configuration before trusting it, then preview and apply it:

```sh
mise trust
mise bootstrap --dry-run
mise bootstrap
mise bootstrap status
```

`--yes` skips confirmation prompts for an unattended apply. A dry run inspects
state and prints proposed actions; hooks and the final task are not executed.
Open a new shell after activation files change.

## Starting from a repository

On a new machine, mise can clone the repository containing that configuration
before it starts:

```sh
mise -E work bootstrap --from git@github.com:example/dotfiles.git --yes
```

The checkout defaults to `$MISE_DATA_DIR/bootstrap-repo`; use `--from-dir` to choose
another location. The explicitly supplied checkout is trusted for this
invocation, and the active `-E` environments are forwarded to it, so files such
as `mise.home.toml` and `mise.work.toml` can select different profiles. Existing
checkouts must have the requested URL as their `origin`. They are reused as-is
unless `--update` is passed, in which case mise runs a fast-forward-only pull
before applying the bootstrap configuration. During `--dry-run`, a missing
checkout is reported but not cloned.

If the repository is the global mise configuration itself, use `--from-git`
instead:

```sh
mise -E work bootstrap --from-git example/mise-config --yes
```

This clones the repository into `$MISE_CONFIG_DIR` (normally
`~/.config/mise`). Files such as `config.toml`, `config.work.toml`, `conf.d/`,
and `tasks/` are therefore loaded as global configuration during the first
bootstrap and remain active for future mise invocations. If
`$MISE_GLOBAL_CONFIG_FILE` selects an individual global config file, the repo
is cloned into that file's parent directory instead and that file is loaded.
As with `--from`, an existing non-empty destination must be a git checkout
whose `origin` exactly matches the requested URL; pass `--update` to
fast-forward it before bootstrap.

## How it runs

`mise bootstrap` runs the steps below in order.

Before making changes, mise resolves any required
[`[bootstrap.secrets]`](/bootstrap/secrets.html) used by the files phase. This
preflight prevents a missing input from leaving a partially provisioned host.

1. `mise bootstrap accounts apply` converges
   [`[bootstrap.users]` and `[bootstrap.groups]`](/bootstrap/accounts.html).
2. `mise bootstrap plugins apply` installs package manager plugins declared in
   [`[bootstrap.plugins]`](/bootstrap/packages/plugins.html).
3. Built-in managers install missing [`[bootstrap.packages]`](/bootstrap/packages/).
4. `mise bootstrap files apply` converges
   [`[bootstrap.files]` and `[bootstrap.directories]`](/bootstrap/files.html).
5. `mise bootstrap services apply` converges existing systemd system units from
   [`[bootstrap.services]`](/bootstrap/services.html).
6. `mise bootstrap firewall apply` converges host firewall policy and rules from
   [`[bootstrap.linux.firewall]`](/bootstrap/firewall.html).
7. `mise bootstrap compose apply` converges
   [`[bootstrap.compose]`](/bootstrap/compose.html) projects.
8. `mise bootstrap repos apply` clones or updates
   [`[bootstrap.repos]`](/bootstrap/repos.html).
9. `mise bootstrap dotfiles apply` applies [`[dotfiles]`](/dotfiles.html).
10. `mise bootstrap mise-shell-activate apply` configures shell activation from
    [`[bootstrap.mise_shell_activate]`](/bootstrap/shell.html).
11. `mise bootstrap macos defaults apply` writes
    [`[bootstrap.macos.defaults]`](/bootstrap/macos-defaults.html).
12. `mise bootstrap macos launchd-agents apply` writes and loads
    [`[bootstrap.macos.launchd.agents]`](/bootstrap/launchd.html).
13. `mise bootstrap linux systemd-units apply` converges
    [`[bootstrap.linux.systemd.units]`](/bootstrap/systemd.html)
    by writing unit files, enabling/disabling them, and starting/stopping them
    as configured.
14. `mise bootstrap user apply` applies [`[bootstrap.user]`](/bootstrap/user.html).
15. `mise install` installs missing `[tools]`.
16. Plugin package managers apply after their host tools are available.
17. `mise run bootstrap` runs a task named `bootstrap`, if one exists.
18. `[bootstrap.hooks.final]` runs after the bootstrap task, if configured.

Every mutating run — the full `mise bootstrap`, each `mise bootstrap <part>
apply`, and the commands that change dotfiles or bootstrap config in place
(`dotfiles add`, `unapply`, `edit`, `packages use`, `import`, brew `tap`) —
records a pair of [history checkpoints](/history.html): the tracked files
before and after the run, plus a journal of what the run changed. Dry runs
record nothing.

Use `mise bootstrap --skip <part>` to skip specific parts. Supported parts are
`accounts`, `plugins`, `packages`, `files`, `services`, `firewall`, `compose`, `repos`, `dotfiles`, `mise-shell-activate`,
`macos-defaults`, `macos-launchd-agents`, `linux-systemd-units`, `user`, `tools`,
`task`, and `final-hook`. The old shorter names `shell`, `defaults`, `launchd`,
and `systemd` are still accepted as aliases. The flag can be repeated or
comma-separated, for example `mise bootstrap --skip tools,task`.

Use `mise bootstrap --only <part>` to run only specific parts. It supports the
same part names and can be repeated or comma-separated, for example
`mise bootstrap --only dotfiles,tools`. `--only` and `--skip` are mutually
exclusive.

Use `mise bootstrap --update` to refresh system package manager metadata
before installing packages (apk: `--update-cache`, apt: `apt-get update`) and
update declared repositories. Check the [repo update rules](/bootstrap/repos.html)
for clean-worktree and fast-forward requirements.

Hook phases can also run before and after the built-in steps:
`pre-packages`, `post-packages`, `pre-repos`, `post-repos`, `pre-dotfiles`,
`post-dotfiles`, `pre-defaults`, `post-defaults`, `pre-user`, `post-user`,
`pre-tools`, and `post-tools`. Hook commands support [Tera templates](/templates.html)
using the declaring config's context, including values such as
<code v-pre>{{ config_root }}</code>, <code v-pre>{{ xdg_config_home }}</code>,
and <code v-pre>{{ vars.name }}</code>.

The declarative steps compare the requested state with the host and apply
needed changes. Hooks and the `bootstrap` task run on every selected apply, so
make them safe to repeat. Bootstrap is a sequence, not a transaction: if a later
phase fails, earlier successful changes remain. Fix the reported failure and
run bootstrap again.

## Previewing changes

Use `mise bootstrap --dry-run` to preview the selected phases. To narrow an
apply while developing a configuration, for example:

```sh
mise bootstrap --only dotfiles,tools --dry-run
mise bootstrap --only dotfiles,tools
```

Select every prerequisite your changes need. `--only services` does not install
the packages or unit files that supply those services.

For a structured resource plan, use `mise bootstrap plan`. The provisioning
planner reports accounts, system packages, privileged files and directories,
system services, firewall policy and rules, and Compose projects in dependency
order. Other declarative bootstrap parts will join the same graph as they adopt
the resource model.

```sh
mise bootstrap plan
mise bootstrap plan --json
mise bootstrap plan --detailed-exitcode
```

With `--detailed-exitcode`, the command exits 0 when nothing would change, 2
when the plan contains changes, and 1 when planning fails or any resource has
an `unknown` state. Unknown resources do not count as changes, but they block a
successful convergence result. A package is unknown when its manager is
unavailable on the current platform or cannot install the requested version.
This matches apply behavior: unsupported pins remain visible for manual
resolution instead of being reported as changes mise would skip.

When `mise bootstrap` applies or would apply something that needs user
follow-up, it prints a final `bootstrap: follow-up` section after a successful
run. Dry runs use `bootstrap: follow-up if applied`. If a later bootstrap phase
fails after earlier phases already produced follow-up items, mise prints those
items before returning the error. The section is omitted when there is nothing
actionable to report.

By default, bootstrap refuses dotfile conflicts rather than replacing local
files. Use `mise bootstrap --force-dotfiles` when you explicitly want the
dotfiles phase to replace conflicting whole-file dotfile targets.

## Inspecting state

Use `mise bootstrap status` to inspect the declarative bootstrap state in one
place. It reports every declarative part — secrets, accounts, files and
directories, services, firewall, Compose projects, packages, repos, dotfiles,
shell activation, macOS defaults, LaunchAgents, systemd units, and login shell —
plus `[tools]` and any system dependencies that installed tools require:

```sh
mise bootstrap status
mise bootstrap status --json
mise bootstrap status --missing
mise bootstrap packages status
mise bootstrap repos status
mise bootstrap dotfiles status
mise bootstrap dotfiles apply --dry-run
mise bootstrap dotfiles apply --dry-run --verbose
mise bootstrap mise-shell-activate status
mise bootstrap macos defaults status
mise bootstrap macos launchd-agents status
mise bootstrap linux systemd-units status
mise bootstrap firewall status
mise bootstrap user status
```

Use `mise bootstrap dotfiles history` to see the checkpoints bootstrap has recorded — a pair per
mutating run, with the tracked files before and after. See [History](/history.html).

```sh
mise bootstrap dotfiles history
mise bootstrap dotfiles history show latest
mise bootstrap dotfiles history diff 11 12
```

`mise bootstrap status --missing` checks the whole declarative bootstrap
surface in one command. The narrower `mise bootstrap packages status --missing`
and `mise bootstrap dotfiles status --missing` commands are useful when you only
want to check one part without installing anything.

## What goes where

| Config                                                                  | Use for                                                       |
| ----------------------------------------------------------------------- | ------------------------------------------------------------- |
| [`[bootstrap.secrets]`](/bootstrap/secrets.html)                        | Names of secret inputs consumed by managed file templates     |
| [`[bootstrap.users]`, `[bootstrap.groups]`](/bootstrap/accounts.html)   | Linux service accounts and groups                             |
| [`[bootstrap.files]`, `[bootstrap.directories]`](/bootstrap/files.html) | Managed system paths, content, ownership, and permissions     |
| [`[bootstrap.services]`](/bootstrap/services.html)                      | Existing Linux systemd system units and file-change handlers  |
| [`[bootstrap.compose]`](/bootstrap/compose.html)                        | Docker Compose project lifecycle                              |
| [`[bootstrap.plugins]`](/bootstrap/packages/plugins.html)               | Package manager plugins                                       |
| [`[bootstrap.packages]`](/bootstrap/packages/)                          | OS packages from apk, apt, dnf, pacman, brew, flatpak, or mas |
| [`[bootstrap.repos]`](/bootstrap/repos.html)                            | Git repos cloned before dotfiles are applied                  |
| [`[dotfiles]`](/dotfiles.html)                                          | Whole-file dotfiles and small managed edits to existing files |
| [`[bootstrap.mise_shell_activate]`](/bootstrap/shell.html)              | mise activation snippets in shell startup files               |
| [`[bootstrap.macos.*]`](/bootstrap/macos-defaults.html)                 | Curated macOS preferences for Dock/Finder/keyboard/trackpad   |
| [`[bootstrap.macos.defaults]`](/bootstrap/macos-defaults.html)          | macOS user preferences written through `defaults write`       |
| [`[bootstrap.macos.launchd.agents]`](/bootstrap/launchd.html)           | macOS user LaunchAgents written and loaded with `launchctl`   |
| [`[bootstrap.linux.systemd.units]`](/bootstrap/systemd.html)            | Linux systemd user services managed with `systemctl --user`   |
| [`[bootstrap.linux.firewall]`](/bootstrap/firewall.html)                | Linux host firewall policy and managed rules                  |
| [`[bootstrap.user]`](/bootstrap/user.html)                              | Current-user settings such as `login_shell`                   |
| `[bootstrap.hooks]`                                                     | Commands that run at named bootstrap phases                   |
| `[tools]`                                                               | Versioned dev tools managed by mise                           |
| `[tasks.bootstrap]`                                                     | Anything custom that should run after tools are installed     |

Use declarative sections when mise can inspect and converge the state. Use
`[tasks.bootstrap]` for imperative setup that does not fit those sections,
such as checking authentication or seeding local data. The task runs again on
every bootstrap, so guard operations that should happen only once.

## Hooks

Hooks run only during explicit `mise bootstrap` invocations. A hook can be
specified as a command string, an array of command strings, or a table with a
`run` field. They use the same default inline shell setting as tasks, stop the
bootstrap if they fail, and print the command instead of running it during
`mise bootstrap --dry-run`. Hooks run in the current process environment; use
`mise exec -- ...` inside a hook, or use `[tasks.bootstrap]`, when the command
needs tools from `[tools]` on PATH.

The following hooks assume `node`, `python`, and `gh` are declared in `[tools]`.

```toml
[bootstrap.hooks.post-tools]
run = [
  "mise exec -- node --version",
  "mise exec -- python --version",
]

[bootstrap.hooks.final]
run = "mise exec -- gh auth status"
```

As shorthand, a hook phase can also be set directly:

```toml
[bootstrap.hooks]
post-defaults = "killall Dock || true"
```

Hooks merge across the config hierarchy from global to local, so shared config
can define broad machine setup while a project adds its own phase commands.
The `pre-dotfiles` and `post-dotfiles` phases also wrap
`mise bootstrap dotfiles apply`.

## Common workflows

### New machine

```sh
mise trust
mise bootstrap --yes
```

### Add a package

```sh
mise bootstrap packages use apk:zlib-dev apt:libssl-dev
```

This writes `[bootstrap.packages]` and installs what is missing.

### Capture an edited dotfile

```sh
$EDITOR ~/.zshrc
mise bootstrap dotfiles add ~/.zshrc
```

`mise bootstrap dotfiles add` stores the live file under `dotfiles.root` and writes an
explicit `[dotfiles]` entry with `mode`.

### Edit a managed dotfile

```sh
mise bootstrap dotfiles edit ~/.zshrc
mise bootstrap dotfiles apply ~/.zshrc
```

For symlinked dotfiles, `edit` opens the managed source, so it works with the
default `symlink` mode.

## Composing configuration roots

`[bootstrap].config_roots` composes declarative resources from independent
configuration roots into the current bootstrap operation:

```toml
[bootstrap]
config_roots = ["bundles/*"]
```

Entries are relative to the declaring config root and may use single-level `*`
globs. Each matched directory is loaded with the normal active configuration
environments. Relative resource sources and template `config_root` values remain
relative to the config that declared them. Variables declared by a selected root
are available to that root's dotfile templates without leaking into sibling
roots.

Composition includes `[dotfiles]`, `[bootstrap.files]`,
`[bootstrap.directories]`, `[bootstrap.services]`, and `[bootstrap.compose]`.
Equivalent declarations are deduplicated. Different declarations for the same
dotfile target, edit `(path, id)`, managed file, managed directory, service, or
Compose project are errors that identify both declaring configs. Independent
roots never acquire precedence from their order in `config_roots`.
Same-target `symlink-each` declarations are the exception: their source trees
compose when their leaf paths are disjoint, while overlapping leaves or
file/directory collisions are reported with both declaring configs.
Directory `copy` and `symlink-each` footprints are also checked against nested
dotfile declarations. Disjoint leaves may share directories, but two entries
cannot own the same leaf or place a file where another entry needs a directory.

Other configuration such as tools, tasks, packages, hooks, and repos is not
collected from these roots. Use their existing explicit workflows when those
resources need aggregate behavior.

Use `mise bootstrap config-roots` to inspect the active non-composed bootstrap
declarations before running those workflows:

```sh
mise bootstrap config-roots
mise bootstrap config-roots --json
```

The command reports package, repo, account, and hook declarations separately
for every matched configuration root. JSON output includes the declaring config
and its active configuration environment. Counts describe active TOML entries;
the command does not inspect the host, resolve resource state, or run bootstrap
hooks.

## Advanced: self-managing config

You can manage the dotfiles repository and the mise global config as
dotfiles:

```toml
[settings]
dotfiles.root = "~/.dotfiles"

[dotfiles]
"~/.dotfiles" = "~/src/dotfiles"
"~/.config/mise/config.toml" = "~/src/dotfiles/mise/config.toml"
```

The repo/source must exist before the first apply. Use the real repo path for
sources needed during the first run; `~/.dotfiles` does not exist until mise
creates that symlink. Replacing the active global config affects future mise
invocations, so use this pattern carefully.
