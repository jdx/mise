---
description: "Declare machine setup in mise configuration and apply it: packages, files, services, repositories, dotfiles, and tools."
socialDescription: "Declare machine setup in mise config and apply it with one command."
---

# Bootstrap

`mise bootstrap` sets up a machine from your mise configuration: host packages,
system files, services, Git repositories, dotfiles, shell activation, tools, and
your `bootstrap` task. Use it when a machine needs more than the tools in
`[tools]`.

mise never applies `[bootstrap]` on its own. It runs only when you run
`mise bootstrap` or one part's `apply` subcommand, such as
`mise bootstrap packages apply`. Parts that declare state skip whatever already
matches, so you can run it again after each configuration change.

## Quick start

Put this in your global config, `~/.config/mise/config.toml`, to make it this
machine's setup:

```toml
[bootstrap.mise_shell_activate]
zsh = true

[tools]
node = "24"

[tasks.bootstrap]
run = "node --version"
```

For Bash or fish, use `bash = true` or `fish = true` instead; see
[shell activation](/bootstrap/shell.html). Preview the run, apply it, then check
the result:

```sh
mise bootstrap --dry-run
mise bootstrap
mise bootstrap status
```

The dry run lists what each part would change and prints the `bootstrap` task
instead of running it:

```text
mise bootstrap: shell activation
edit ~/.zprofile (block:activate)
edit ~/.zshrc (block:activate)
mise bootstrap: tools
mise node@24.x.x  ⇢ would install
mise bootstrap: running `bootstrap` task
[bootstrap] $ node --version
```

The real run writes the activation blocks, installs Node.js, and runs the task.
Add `--yes` to apply without confirmation prompts, and open a new shell after
mise edits your shell startup files.

The same configuration also works in a project `mise.toml`, but a project file
needs [`mise trust`](/cli/trust.html) before mise reads it. The global config
does not.

## What you can declare {#what-goes-where}

Each section has its own page, and most have their own `status` and `apply`
subcommands.

| Config                                                                    | Use for                                                                                     |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| [`[bootstrap.packages]`](/bootstrap/packages/)                            | Host packages from apt, dnf, pacman, Homebrew, WinGet, and other package managers           |
| [`[bootstrap.plugins]`](/bootstrap/packages/plugins.html)                 | Package manager plugins that add more package sources                                       |
| [`[bootstrap.users]`, `[bootstrap.groups]`](/bootstrap/accounts.html)     | Linux users and groups                                                                      |
| [`[bootstrap.files]`, `[bootstrap.directories]`](/bootstrap/files.html)   | System files and directories, with their content, owner, and permissions                    |
| [`[bootstrap.secrets]`](/bootstrap/secrets.html)                          | Secret values that `[bootstrap.files]` and `[dotfiles]` templates read from the environment |
| [`[bootstrap.services]`](/bootstrap/services.html)                        | User services on Linux, macOS, and Windows, and existing Linux system services              |
| [`[bootstrap.linux.firewall]`](/bootstrap/firewall.html)                  | Linux host firewall policy and rules                                                        |
| [`[bootstrap.compose]`](/bootstrap/compose.html)                          | Docker Compose projects                                                                     |
| [`[bootstrap.repos]`](/bootstrap/repos.html)                              | Git repositories, cloned before dotfiles are applied                                        |
| [`[dotfiles]`](/dotfiles.html)                                            | Dotfiles: tracking files in place, creating files from sources, and editing blocks or lines |
| [`[dotfile_groups]`, `[bootstrap] dotfile_groups`](/dotfiles/groups.html) | Directory trees of dotfiles, and which of them a machine applies                            |
| [`[bootstrap.mise_shell_activate]`](/bootstrap/shell.html)                | mise activation in shell startup files                                                      |
| [`[bootstrap.user]`](/bootstrap/shell.html#set-your-login-shell)          | Your login shell                                                                            |
| [`[bootstrap.macos.defaults]`](/bootstrap/macos-defaults.html)            | macOS user preferences, written with `defaults write`                                       |
| [`[bootstrap.macos.*]`](/bootstrap/macos-defaults.html)                   | Common macOS preferences for the Dock, Finder, keyboard, and trackpad                       |
| [`[bootstrap.macos.launchd.agents]`](/bootstrap/launchd.html)             | macOS LaunchAgents, written and loaded with `launchctl`                                     |
| [`[bootstrap.linux.systemd.units]`](/bootstrap/systemd.html)              | systemd user units, managed with `systemctl --user`                                         |
| [`[tools]`](/dev-tools/)                                                  | Tools that mise installs and manages                                                        |
| [`[bootstrap.hooks]`](#hooks)                                             | Commands that run at named points in the run                                                |
| [`[tasks.bootstrap]`](#the-bootstrap-task)                                | Setup that the sections above do not cover                                                  |
| [`[bootstrap.remote]`](/bootstrap/remote.html)                            | SSH hosts for `mise bootstrap remote`                                                       |

## Start from a repository {#starting-from-a-repository}

To set up a machine from configuration you keep in Git, use the command that
matches what the repository holds:

| Repository holds                                       | Command                        | Where the files go                                       |
| ------------------------------------------------------ | ------------------------------ | -------------------------------------------------------- |
| A bootstrap project: `mise.toml` and the files it uses | `mise bootstrap --from <url>`  | A separate checkout, then wherever the project puts them |
| Global mise config: `config.toml`, `conf.d/`, `tasks/` | `mise bootstrap --adopt <url>` | Your global config directory, `~/.config/mise`           |
| A setup repository created with `mise dot origin set`  | `mise bootstrap --adopt <url>` | Each tracked file's path on this machine                 |

```sh
mise bootstrap --from https://github.com/you/workstation.git
mise bootstrap --adopt you/mise-config
mise bootstrap --adopt you/setup
```

`--adopt` also accepts `OWNER/REPO` for a GitHub repository. See
[Bootstrap from a repository](/bootstrap/from-repository.html) for refs,
existing checkouts, and `--update`, and [Set up a machine](/bootstrap/setup.html)
for a walkthrough that ends with adopting a setup repository.

## Run order {#how-it-runs}

Before it changes anything, mise resolves the
[`[bootstrap.secrets]`](/bootstrap/secrets.html) inputs used by
`[bootstrap.files]` templates and by `[dotfiles]` entries with
`mode = "template"`, and renders those templates. A missing value stops the run
before the first change.

`mise bootstrap` then runs these steps in order. The part name in the second
column works with `--only` and `--skip`, and a part's hooks run only when the
part runs.

| Step | Part (`--only`, `--skip`)          | Applies                                                                                                 | Hooks                           |
| ---- | ---------------------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------- |
| 1    | `accounts`                         | [`[bootstrap.users]` and `[bootstrap.groups]`](/bootstrap/accounts.html)                                |                                 |
| 2    | `plugins`                          | [`[bootstrap.plugins]`](/bootstrap/packages/plugins.html)                                               |                                 |
| 3    | `files`                            | Files and directories with [`phase = "pre-packages"`](/bootstrap/files.html)                            |                                 |
| 4    | `packages`                         | [`[bootstrap.packages]`](/bootstrap/packages/) from built-in package managers                           | `pre-packages`, `post-packages` |
| 5    | `files`                            | The remaining [`[bootstrap.files]` and `[bootstrap.directories]`](/bootstrap/files.html)                |                                 |
| 6    | `services`                         | [`[bootstrap.services]`](/bootstrap/services.html), except user services with `requires_tools = true`   |                                 |
| 7    | `firewall`                         | [`[bootstrap.linux.firewall]`](/bootstrap/firewall.html)                                                |                                 |
| 8    | `compose`                          | [`[bootstrap.compose]`](/bootstrap/compose.html)                                                        |                                 |
| 9    | `repos`                            | [`[bootstrap.repos]`](/bootstrap/repos.html)                                                            | `pre-repos`, `post-repos`       |
| 10   | `dotfiles`                         | [`[dotfiles]`](/dotfiles.html)                                                                          | `pre-dotfiles`, `post-dotfiles` |
| 11   | `mise-shell-activate` (`shell`)    | [`[bootstrap.mise_shell_activate]`](/bootstrap/shell.html)                                              |                                 |
| 12   | `macos-defaults` (`defaults`)      | [`[bootstrap.macos.defaults]`](/bootstrap/macos-defaults.html) and the other `[bootstrap.macos.*]` keys | `pre-defaults`, `post-defaults` |
| 13   | `macos-launchd-agents` (`launchd`) | [`[bootstrap.macos.launchd.agents]`](/bootstrap/launchd.html)                                           |                                 |
| 14   | `linux-systemd-units` (`systemd`)  | [`[bootstrap.linux.systemd.units]`](/bootstrap/systemd.html)                                            |                                 |
| 15   | `user`                             | [`[bootstrap.user]`](/bootstrap/shell.html#set-your-login-shell)                                        | `pre-user`, `post-user`         |
| 16   | `tools`                            | `[tools]`, as `mise install` would                                                                      | `pre-tools`, `post-tools`       |
| 17   | `packages`                         | Packages from [package manager plugins](/bootstrap/packages/plugins.html)                               |                                 |
| 18   | `services`                         | User services with `requires_tools = true`                                                              |                                 |
| 19   | `task`                             | The [`bootstrap` task](#the-bootstrap-task), if one exists                                              |                                 |
| 20   | `final-hook`                       |                                                                                                         | `final`                         |

The `post-packages` hook runs right after step 4, unless a package in
`[bootstrap.packages]` comes from a package manager plugin. Then it runs after
step 17 instead, once those packages are installed. Step 18 runs even with
`--skip tools`.

The declarative parts compare the configuration with the machine and change only
what differs. Hooks and the `bootstrap` task run every time, so make them safe
to repeat. A run is a sequence, not a transaction: if a later step fails,
earlier changes stay. Fix the reported failure and run `mise bootstrap` again.

Once at least one file is [tracked](/dotfiles/history.html), or history already
exists, every run that changes something records checkpoints of your tracked
files before and after it, so [`mise dot undo`](/cli/dotfiles/undo.html) can
reverse what the run wrote to them. That covers `mise bootstrap`, each
`mise bootstrap <part> apply`, and other commands that change files or bootstrap
config in place, such as `mise dot add`, `mise dot edit`,
`mise bootstrap packages use`, and `mise bootstrap unapply`. A bootstrap with
nothing tracked does not start a history, dry runs record nothing, and
[`history.enabled = false`](/configuration/settings.html#history.enabled) turns
recording off.

## Choose what runs

`--only` runs only the parts you name, and `--skip` runs everything except them.
Both take the part names from the [run order](#how-it-runs), repeated or
comma-separated, and you cannot combine them:

```sh
mise bootstrap --only dotfiles,tools --dry-run
mise bootstrap --only dotfiles,tools
mise bootstrap --skip tools,task
```

Select every part your change depends on. `--only services` does not install the
packages or unit files that supply those services, and a new account used as a
file owner needs both `accounts` and `files`.

These flags change what the selected parts do:

| Flag               | Effect                                                                                                         |
| ------------------ | -------------------------------------------------------------------------------------------------------------- |
| `--update`         | Refresh package metadata before installing packages, and update [declared repositories](/bootstrap/repos.html) |
| `--force-dotfiles` | Replace existing files that conflict with whole-file `[dotfiles]` entries instead of stopping                  |
| `--skip-dirty`     | Skip `[bootstrap.repos]` checkouts with local changes instead of failing                                       |
| `--prompt-secrets` | Prompt for [secret inputs](/bootstrap/secrets.html) missing from the environment                               |
| `-y`, `--yes`      | Apply without confirmation prompts                                                                             |

The metadata refresh depends on the manager: apk adds `--update-cache`; apt runs
`apt-get update`; dnf, nix, and AUR helpers add `--refresh`; pacman runs
`pacman -Sy`; zypper runs `zypper refresh`; scoop runs `scoop update`; and
winget runs `winget source update`. Homebrew, Flatpak, `macos-app`, and mas
have no separate refresh.

## Preview and inspect {#previewing-changes}

`mise bootstrap --dry-run` previews the selected parts. It lists the changes each
part would make, and it prints hook commands and the `bootstrap` task instead of
running them. It does not render `[dotfiles]` templates, but it does render
`[bootstrap.files]` content templates (`template = true`) to compare them with
the files on disk, so an `exec()` call in one of them runs during the dry run,
as it does for `mise bootstrap plan` and `mise bootstrap status`. See
[Templates that run commands](#templates-that-run-commands).

### Plan declarative resources

`mise bootstrap plan` lists the changes to declarative resources in dependency
order: accounts, packages (including packages from plugins), system files and
directories, system and user services, firewall policy and rules, and Compose
projects. It does not cover dotfiles, repositories, shell activation, macOS
defaults, LaunchAgents, systemd user units, the login shell, tools, hooks, or
the `bootstrap` task. Use `mise bootstrap --dry-run` for those.

```sh
mise bootstrap plan
mise bootstrap plan --json
mise bootstrap plan --detailed-exitcode
```

With `--detailed-exitcode`, the exit code reports what the plan found:

| Exit code | Meaning                                     |
| --------- | ------------------------------------------- |
| 0         | Nothing would change                        |
| 2         | The plan has changes                        |
| 1         | Planning failed, or a resource is `unknown` |

A resource is `unknown` when mise cannot reach its declared state on its own,
for example a package whose manager is unavailable on this platform or cannot
install the requested version. mise reports it instead of skipping it, so you
can resolve it by hand.

### Check state {#inspecting-state}

```sh
mise bootstrap status
mise bootstrap status --missing   # exit 1 if anything differs from the config
mise bootstrap status --json
```

`status` lists every configured part, the tools in `[tools]`, and the system
dependencies that installed tools need. `--missing` still lists everything and
only changes the exit code, which suits CI. Each part also has its own `status`
subcommand, such as `mise bootstrap packages status`, and
[`mise dot status`](/cli/dotfiles/status.html) covers dotfiles and their
history.

### Follow-up items

If a run leaves something for you to do, such as starting a new login session
after a login shell change or handling a part mise skipped on this platform,
`mise bootstrap` ends with a `bootstrap: follow-up` list. A dry run prints
`bootstrap: follow-up if applied` instead. mise prints the list even when a
later step fails.

## Hooks

Hooks run commands at named points in a run. Set a phase to a command string,
an array of commands, or a table with `run`:

```toml
[bootstrap.hooks]
post-defaults = "killall Dock || true"

[bootstrap.hooks.post-tools]
run = [
  "mise exec -- node --version",
  "mise exec -- python --version",
]

[bootstrap.hooks.final]
run = "mise exec -- gh auth status"
```

The phases are `pre-packages`, `post-packages`, `pre-repos`, `post-repos`,
`pre-dotfiles`, `post-dotfiles`, `pre-defaults`, `post-defaults`, `pre-user`,
`post-user`, `pre-tools`, `post-tools`, and `final`. The
[run order](#how-it-runs) shows where each one runs. Hooks from every config
file are combined, global config first, so a global config can set up the
machine while a project adds its own commands.

Hooks run during `mise bootstrap`. The `pre-dotfiles` and `post-dotfiles` hooks
also run around [`mise dot apply`](/cli/dotfiles/apply.html). Other part
subcommands, such as `mise bootstrap repos apply`, do not run hooks, and
`--no-hooks`, `MISE_NO_HOOKS=1`, and [safe mode](/security.html#safe-mode) skip
them all.

Each command runs with the shell from
[`unix_default_inline_shell_args`](/configuration/settings.html#unix_default_inline_shell_args)
(or
[`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args)),
as inline tasks do. A failing hook stops the run, and a dry run prints each
command instead of running it. Hooks run in mise's own environment, not a task
environment, so a command that needs tools from `[tools]` should go through
`mise exec --`, as above, or move to the [`bootstrap` task](#the-bootstrap-task).

Hook commands are [Tera templates](/templates.html) rendered with the declaring
config's context, such as <code v-pre>{{ config_root }}</code>,
<code v-pre>{{ xdg_config_home }}</code>, and <code v-pre>{{ vars.name }}</code>.

## The bootstrap task

After the other parts, `mise bootstrap` runs the task named `bootstrap` if one
exists, such as a `[tasks.bootstrap]` table. It runs like any other
[task](/tasks/), with the tools from `[tools]` available.

Use the declarative sections when mise can check and apply the state. Use the
task for setup they do not cover, such as checking authentication or seeding
local data:

```toml
[tasks.bootstrap]
run = [
  "install -d -m 700 ~/.ssh",
  "gh auth status",
]
```

The task runs on every `mise bootstrap`, so guard steps that should happen only
once. On machines that share a [setup repository](/bootstrap/setup.html),
adoption runs it once the files are restored. A shared change that
`mise dot pull` or the watcher restores does not run it; run `mise bootstrap` to
apply the change and run the task. `--skip task` leaves it out.

## Templates

Not every value under `[bootstrap]` is a [Tera template](/templates.html). These
are rendered:

| Where                                                         | What is rendered                                                      |
| ------------------------------------------------------------- | --------------------------------------------------------------------- |
| [`[bootstrap.linux.systemd.units]`](/bootstrap/systemd.html)  | Every string value in a unit                                          |
| [`[bootstrap.macos.launchd.agents]`](/bootstrap/launchd.html) | Every string value in an agent                                        |
| [`[bootstrap.compose]`](/bootstrap/compose.html)              | Text and list values in a Compose project, not fields such as `state` |
| [`[bootstrap.hooks]`](#hooks)                                 | The hook command                                                      |
| [`[bootstrap.files]`](/bootstrap/files.html)                  | File content, only with `template = true`                             |
| [`[dotfiles]`](/dotfiles.html)                                | File content, only with `mode = "template"` or `template = "tera"`    |

Everything else, including section keys, package specs, repository paths, and
macOS defaults, is used exactly as written.

Rendering uses the template context of the config file that declared the entry,
so <code v-pre>{{ config_root }}</code> does not depend on where you run
`mise bootstrap`. In units, agents, Compose projects, and hooks it is the
config's root: the project directory, or `MISE_GLOBAL_CONFIG_ROOT` (default
`$HOME`) for the global config. In `[bootstrap.files]` content templates it is
the directory that contains the declaring file; see
[render content as a template](/bootstrap/files.html#render-content-as-a-template).
A managed file's content template also gets <code v-pre>{{ target }}</code>. `[bootstrap.files]` templates and
`[dotfiles]` entries with `mode = "template"` can call
<code v-pre>{{ secret(name="...") }}</code>; block, line, and merge edits with
`template = "tera"` cannot.

Values without template syntax skip the renderer, so a literal `%h`, `%i`, or
`$HOME` in a unit, agent, or Compose project reaches the generated file
unchanged. Any `~` expansion a section documents still happens afterwards.

### Templates that run commands

<code v-pre>{{ exec(...) }}</code> works in `[bootstrap.hooks]` and in file
content templates, but not in unit, agent, or Compose values. Unit, agent, and
Compose values render the same way for `status`, `plan`, `--dry-run`, and
`apply`, so mise leaves `exec()` out of them to keep those commands from running
anything. File content templates do not have that protection: mise renders them
to compare their output with the file on disk, and their `exec()` calls run
then:

| Template                     | Read-only commands that render it                                              |
| ---------------------------- | ------------------------------------------------------------------------------ |
| `[bootstrap.files]` content  | `mise bootstrap status`, `mise bootstrap plan`, and `mise bootstrap --dry-run` |
| `[dotfiles]` content         | `mise bootstrap status` and `mise dot status`, but not a dry run               |
| `[bootstrap.hooks]` commands | A dry run, with `exec()` disabled, so a hook that calls it fails the dry run   |

## Next steps

- [Set up a machine](/bootstrap/setup.html) walks through tracking dotfiles,
  declaring tools and packages, and bringing a second machine to the same
  state.
- [Machine modules](/bootstrap/modules.html) groups optional setup into config
  environments that each machine selects, and removes a module's resources with
  `mise bootstrap unapply`.
- [Remote hosts](/bootstrap/remote.html) applies the same configuration to
  other machines over SSH.
- To change a dotfile that mise deploys, see
  [Adopt files and capture edits](/dotfiles/managed.html#capturing-changes). To
  add packages from the command line, see
  [`mise bootstrap packages use`](/cli/bootstrap/packages/use.html).
