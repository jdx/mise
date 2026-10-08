---
description: "Install mise, then give a project its own Node.js version, environment variable, and task."
---

# Getting started

Install mise, then give a project its own Node.js version, an environment
variable and a task. Steps 1 to 3 work without changing your shell
configuration; step 4 adds optional shell activation.

Joining a project that already has a `mise.toml`? Install mise (step 1), read
the project's `mise.toml` (see [trust](#trust)), then run `mise install` in the
project directory and `mise tasks ls` to list its tasks.
[Use mise in an existing project](/walkthrough.html) covers adding mise to a
project that does not use it yet.

## 1. Install mise {#installing-mise-cli}

On macOS or Linux, run the installer:

```sh
curl -fsSL https://mise.run | sh
~/.local/bin/mise --version
# 2026.x.x linux-x64 (2026-xx-xx)
```

The installer puts the `mise` executable at `~/.local/bin/mise`. If your shell
cannot find `mise` in the next steps, run it as `~/.local/bin/mise` until you
[activate mise](#activate-mise) in step 4, which adds it to `PATH`.

On Windows, run `winget install jdx.mise`. For Homebrew, apt, dnf and other
package managers, see [Installing mise](/installing-mise.html).

## 2. Run a tool {#mise-exec-run}

[`mise exec`](/cli/exec.html) runs a command with a specific tool version:

```sh
mise exec node@24 -- node --version
# v24.x.x
```

mise downloads Node.js 24 if it is not installed, then runs the command after
`--`. This does not change any config file or your current shell's environment.

## 3. Set up a project {#set-up-a-project}

Create a directory and select Node.js 24 for it:

```sh
mkdir my-project
cd my-project
mise use node@24
```

[`mise use`](/cli/use.html) installs Node.js 24 if needed and records the
request in a new `mise.toml`:

```toml [mise.toml]
[tools]
node = "24"
```

### Set an environment variable {#environment-variables}

Run [`mise set`](/cli/set.html) to add an `[env]` section:

```sh
mise set NODE_ENV=development
```

`mise.toml` now contains:

```toml [mise.toml]
[tools]
node = "24"

[env]
NODE_ENV = "development"
```

Commands run through mise get both the tool and the variable:

```sh
mise exec -- node -p process.env.NODE_ENV
# development
```

You can also [load variables from a `.env` file](/environments/#env-directives).

### Run a task {#run-a-task}

Add a task to the same `mise.toml`:

```toml [mise.toml]
[tasks.hello]
description = "Print the Node.js version and NODE_ENV"
run = 'node -e "console.log(process.version, process.env.NODE_ENV)"'
```

```sh
mise run hello
```

```text
[hello] $ node -e "console.log(process.version, process.env.NODE_ENV)"
v24.x.x development
```

[`mise run`](/cli/run.html) installs missing tools, sets the project's
environment, then runs the task.

### Check what mise selected {#confirm-what-is-active}

From the project directory:

```sh
mise config ls     # config files in use
mise ls --current  # selected tool versions
mise tasks ls      # available tasks
```

### Trust project configuration {#trust}

A `mise.toml` with `[env]`, hooks, templates or settings can run code or change
your environment, so mise loads it only once it is trusted: files you write with
`mise use` or `mise set` are trusted, and so is a project where you run
`mise install`, `mise exec` or `mise run`. For a project from someone else, read
its config, tasks included, before running those commands or
[`mise trust`](/cli/trust.html); see
[Configuration trust](/security.html#configuration-trust) for the rules.

### Share the project {#share-the-project}

Commit `mise.toml`. Teammates and CI run `mise install` in the project to get
the same tools. `"24"` is a prefix, not an exact version, so two machines can
resolve it to different Node.js 24 releases; to record exact versions, add a
[lockfile](/dev-tools/mise-lock.html) with `mise lock`.

## 4. Activate mise <Badge text="optional" /> {#activate-mise}

Activate mise in your interactive shell so that entering a project directory
puts its tools on `PATH` and sets its environment variables. Activation adds a
hook that runs at each prompt and directory change and updates the environment
for the current directory. Scripts and CI do not need activation; keep using
`mise exec` and `mise run` there.

Run the command for your shell once. It appends the activation line to your
shell's startup file:

::: code-group

```sh [bash]
echo 'eval "$(~/.local/bin/mise activate bash)"' >> ~/.bashrc
```

```sh [zsh]
echo 'eval "$(~/.local/bin/mise activate zsh)"' >> "${ZDOTDIR:-$HOME}/.zshrc"
```

```fish [fish]
mkdir -p ~/.config/fish
echo '~/.local/bin/mise activate fish | source' >> ~/.config/fish/config.fish
```

```powershell [PowerShell]
if (-not (Test-Path $PROFILE)) { New-Item -ItemType File -Path $PROFILE -Force | Out-Null }
Add-Content $PROFILE '(&mise activate pwsh) | Out-String | Invoke-Expression'
```

:::

If a package manager installed mise, write `mise` in place of
`~/.local/bin/mise`. With Homebrew, fish activates mise without this step.
[Shell setup](/shell-setup.html) covers Nushell, Xonsh, Elvish, Git Bash and
cmd.exe, and has versions of these lines that do not add a duplicate when run
twice.

Open a new terminal, then compare a directory inside the project with one
outside it:

```sh
cd my-project
node --version
# v24.x.x
cd ..
node --version
# another Node.js version, or "command not found"
```

To use a version outside any project, set a personal default with
`mise use --global node@24`. It writes `~/.config/mise/config.toml`, and
project configuration overrides it.

Run [`mise doctor`](/cli/doctor.html) to check the setup. Editors and other
programs that do not read your shell's startup files can use
[shims](/dev-tools/shims.html) instead, which have
[some limits](/dev-tools/shims.html#shims-vs-path).

## 5. Find more tools {#tool-backends}

Use the [registry](/registry.html) to find tool names such as `node`, `python`,
`jq` and `ripgrep`. Usually the name is all you need. Run these from the
`my-project` directory; after the step 4 demo, `cd my-project` first:

```sh
mise use ripgrep
mise exec -- rg --version
```

This adds `ripgrep` to the project's `mise.toml`. Outside a project,
`mise use` creates a `mise.toml` in the current directory, or writes the global
config when that directory is your home directory.

A backend tells mise where to get a tool and how to install it. You can name one
explicitly, including for tools without a registry entry:

```sh
mise exec github:BurntSushi/ripgrep -- rg --version
```

Some backends need another tool or package manager, such as Node.js for `npm:`.
Check the [backend guide](/dev-tools/backends/) before using a new ecosystem.

## If something does not work {#if-something-doesn-t-work}

Run `mise doctor` to check your setup. If a tool works through `mise exec` but
not as a plain command, check [shell activation](#activate-mise) and open a new
terminal. See [Troubleshooting](/troubleshooting.html) for other problems.

### GitHub API rate limits {#github-api-rate-limiting}

If an error reports a GitHub API rate limit, give mise a
[GitHub token](/dev-tools/github-tokens.html).

## Next steps {#next-steps}

- [Use mise in an existing project](/walkthrough.html)
- [Tasks](/tasks/): build, test and other project commands
- [Editors](/ide-integration.html) and [continuous integration](/continuous-integration.html)
- [Shell completions](/shell-setup.html#autocompletion)
- [Bootstrap](/bootstrap.html): system packages, dotfiles and services for a whole machine
