---
description: "Group optional machine setup into config environments, choose the modules each machine loads, and remove a module's resources."
socialDescription: "Group optional machine setup into config environments and pick them per machine."
---

# Machine modules

A module is a [config environment](/configuration/environments.html) file that
holds one piece of machine setup, such as SSH or GPG, with its packages,
dotfiles, and services together. Each machine selects the modules it needs, and
[`mise bootstrap unapply`](/cli/bootstrap/unapply.html) removes what a module
set up.

## Define a module

Keep shared setup in `~/.config/mise/config.toml` and put each optional piece
in a `config.<name>.toml` file beside it. This SSH module is for a Linux machine
with apt and a systemd user session. It installs the client, links an SSH config
from your dotfiles checkout, and runs an agent:

```toml [~/.config/mise/config.ssh.toml]
[bootstrap.packages]
"apt:openssh-client" = "latest"

[dotfiles]
"~/.ssh/config" = "~/src/dotfiles/ssh/config"

[bootstrap.services.ssh-agent]
scope = "user"
command = "ssh-agent -D -a %t/ssh-agent.socket"
```

Create `~/src/dotfiles/ssh/config` before you apply this module. To use the
agent from your shell, set `SSH_AUTH_SOCK` to
`$XDG_RUNTIME_DIR/ssh-agent.socket`.

In a [bootstrap project](/bootstrap/from-repository.html#a-bootstrap-project),
use `mise.toml` and `mise.ssh.toml` in the project directory instead.

## Select modules {#select-and-preview-modules}

Choose this machine's modules in `miserc.local.toml` in the global config
directory. For example, after defining `config.ssh.toml` and `config.gpg.toml`:

```toml [~/.config/mise/miserc.local.toml]
env = ["ssh", "gpg"]
```

mise reads [`miserc.local.toml`](/configuration.html#miserc) before any other
config. The file stays on this machine even when the config directory is a
checkout shared with
[`--adopt`](/bootstrap/from-repository.html#global-mise-configuration), as long
as you do not commit it. Its `env` list replaces any list in a shared
`miserc.toml`, so put the setup every machine needs in the base `config.toml`,
which always loads. Preview the combined setup, then apply it:

```sh
mise bootstrap --dry-run
mise bootstrap
```

To pick modules for one run, use `mise -E ssh,gpg bootstrap`. `-E` replaces the
selection from `miserc.local.toml` for that run instead of adding to it. For
[remote hosts](/bootstrap/remote.html), set each host's `mise_env` list in the
inventory, so one repository can describe machines with different combinations
of modules.

## Combine modules {#how-modules-combine}

Declarations with different keys all contribute to the same run. When two
modules in the same directory declare the same key, the one listed later in
`env` wins. With `env = ["ssh", "gpg"]`, a service declared in both files uses
the definition from `config.gpg.toml`.

`mise config` lists the files that loaded. `mise bootstrap plan --json`
includes `origin.config` and `origin.environment` for each managed file and
service, so you can trace a resource back to its declaration.

Setup that should always load belongs in the base config or in a
[`conf.d` fragment](/configuration.html#conf-d) without an environment
suffix, such as `conf.d/ssh.toml`. To keep a module together with its source
files, use a [`conf.d` folder](/configuration.html#conf-d-folders). Relative
dotfile sources resolve inside the folder, and `mise.<env>.toml` files in it
load only for that environment:

```text
~/.config/mise/conf.d/ssh/
├── mise.toml          # always loaded; "~/.ssh/config" = "ssh_config"
├── mise.linux.toml    # when the linux environment is active
└── ssh_config
```

## Remove a module's resources

Removing a module from `env` stops loading its declarations, but leaves its
resources on the machine. `mise bootstrap unapply` removes the module's managed
files, directories, user services, systemd user units, and dotfile entries and
edits.

First, remove the module from `env` in `miserc.local.toml` (or wherever you
selected it), so the next bootstrap does not apply it again. Keep the module's
config file on disk, then preview and confirm the removal:

```sh
mise bootstrap unapply ssh --dry-run
mise bootstrap unapply ssh
```

The command selects `ssh` for this run on top of your remaining environments, so
it can read the module's declarations. Name several modules to remove them in
one run:

```sh
mise bootstrap unapply ssh gpg --dry-run
mise bootstrap unapply ssh gpg
```

Unapply asks for confirmation before it removes anything. `--yes` approves the
removal without a prompt, as do the global `--yes` flag, `MISE_YES`, and the
[`yes`](/configuration/settings.html#yes) setting, which mise turns on in CI.

### How removal is planned

mise compares the current configuration with and without the named
environments. It uses the declarations still on disk, not a record of earlier
bootstrap runs, so keep those declarations until cleanup is complete. Deleting
a module's file first leaves mise without the information it needs to remove
its resources.

- A resource that the base config or another selected module still declares
  present is kept. A `state = "absent"` declaration elsewhere does not protect
  it.
- A target that no longer matches its declaration, for example a file edited by
  hand, is kept and reported with a reason. Review the output before you pass
  `--force` to remove changed targets.
- A directory is removed only if it is empty after the planned removals.
  Unreadable directories, and managed paths of an unexpected type, are kept even
  with `--force`.

Source files and config entries are kept. Unapply does not restore resources
that a `state = "absent"` declaration removed earlier.

### Resources that need separate cleanup

Unapply reports package, repository, and Compose declarations with guidance
instead of removing them:

- For packages, unapply suggests
  [`mise bootstrap packages prune`](/cli/bootstrap/packages/prune.html), which
  handles Homebrew formulae, casks that mise installed, and packages from
  plugins. Preview its plan first, because pruning is not limited to one
  module's packages. Remove packages from other managers with the manager
  itself.
- For a Compose project, set `state = "absent"` and apply the change while the
  module is still selected, for example with
  `mise -E ssh bootstrap --only compose`.
- For a repository, delete the checkout declared in
  [`[bootstrap.repos]`](/bootstrap/repos.html) when you no longer need it.

Other bootstrap sections, including system services, are outside unapply's
scope. Use the removal steps on each section's page.
