---
description: "Apply short recipes for everyday mise commands, scripts, configuration, shell prompts, and Intel tools on Apple silicon."
socialDescription: "Apply short mise recipes for everyday commands, scripts, configuration and shell prompts."
---

# Tips and tricks

Short recipes for everyday mise use. Each one links to the full guide.

## Everyday commands

For running tasks, including the `mise <task>` shorthand and rerunning a task
when files change, see [Running tasks](/tasks/running-tasks.html).

### Open a shell with the project environment {#mise-en}

[`mise en`](/cli/en.html) starts a new shell with the project's tools and
environment variables loaded, without shell activation. Exit that shell to
return to your session. Changing directories inside it does not update the
environment, and the new shell's startup files can still activate mise. To skip
Bash's startup file:

```sh
mise en -s "bash --norc"
```

### Find where a tool or setting comes from {#mise-tool-tool}

<a id="mise-cfg"></a>

```sh
mise tool ripgrep      # backend, requested and active versions, config source
mise registry ripgrep  # the backends the short name can resolve to
mise which rg          # the executable this project runs
mise config ls         # config files in use and the tools each one sets
mise settings ls       # settings set in config files, and the file that set each
```

See [`mise tool`](/cli/tool.html), [`mise registry`](/cli/registry.html),
[`mise which`](/cli/which.html), [`mise config ls`](/cli/config/ls.html) and
[`mise settings ls`](/cli/settings/ls.html). For the order in which config files
apply, see [config file locations](/configuration.html#mise-toml). To trace an
environment variable, see [See which variables mise set](#see-which-variables-mise-set).

## Scripts and wrappers

### Pin a tool in a shebang {#shebang}

A script can name the tool and version it runs with, without a `mise.toml`:

```js [script.js]
#!/usr/bin/env -S mise exec node@24 -- node
console.log(`Running node: ${process.version}`);
```

`env -S` splits the rest of the line into separate arguments. Run
`chmod +x script.js`, then `./script.js`; [`mise exec`](/cli/exec.html)
installs Node.js 24 the first time if it is missing. This needs mise on `PATH`
and an `env` that supports `-S`, but not shell activation. Windows does not run
shebang lines. To commit one wrapper per tool with more install options, use
[tool stubs](/dev-tools/tool-stubs.html).

### Let contributors run tasks without installing mise {#bootstrap-script}

<a id="project-local-task-entrypoints"></a>

Commit a `bin/mise` wrapper that downloads a pinned mise on first use, plus one
script per task:

```sh
mise generate install-script --localize --write bin/mise --windows
mise generate task-stubs --mise-bin ./bin/mise
```

Commit `bin/` and add `.mise/` to `.gitignore`; the localized wrapper keeps
mise, its tools and its cache there. Contributors then run `./bin/test` from
the project root, or `.\bin\test.cmd` on Windows. `--windows` also writes
`bin/mise.cmd`, which checks the downloaded `mise.exe` against a checksum
recorded when you generated it. Regenerate the wrapper to move to a newer mise.

[`mise generate task-stubs`](/cli/generate/task-stubs.html) writes a stub for
every task mise loads except hidden tasks and tasks from your global config.
Stubs that an older mise wrote for those stay in `bin/` until you delete them.
On Windows the default `.cmd`
launchers can alter arguments that contain `& ^ | " %`; generate with
`--windows-launcher exe` on Windows when arguments must arrive unchanged. For
the wrapper's version pinning and directories, see
[CI bootstrapping](/continuous-integration.html#bootstrapping).

## Configuration

### Install tools when you enter a project {#auto-install-when-entering-a-project}

In a shell with [`mise activate`](/cli/activate.html), an
[`enter` hook](/hooks.html) can install missing tools when you `cd` into the
project:

```toml [mise.toml]
[hooks]
enter = "mise install --quiet"
```

The tools are on `PATH` from the next prompt. The hook downloads tools and runs
their install scripts whenever you enter the directory with something missing;
run `mise install` yourself if you prefer to choose when that happens.

### Read a version from another tool's file {#using-tera-to-read-unsupported-version-files}

mise reads many version files directly once you enable them as
[idiomatic version files](/dev-tools/versions.html#idiomatic-version-files). For
another format, read the file with a [template](/templates.html). For example,
the Hugo Version Manager writes `.hvm` with a version tag and an optional
edition, such as `v0.152.2/extended`:

```toml [mise.toml]
[tools]
hugo-extended = "{{ read_file(path=config_root ~ '/.hvm') | trim | split(pat='/') | first | trim_start(pat='v') }}"
```

This keeps `0.152.2`. mise installs Hugo's editions as separate tools (`hugo`,
`hugo-extended` and `hugo-extended-withdeploy`), so use the one that matches the
project's edition. `config_root` keeps the path relative to the config file
when you run mise from a subdirectory.

### Share tasks across repositories {#share-task-catalogs}

[`task_config.includes`](/tasks/task-discovery.html#include-task-files-and-directories)
chooses where tasks are loaded from: directories, `tasks.toml` files, or remote
git repositories. Setting it replaces the default task directories, so list any
default directory (such as `.mise/tasks`) you still use. Remote includes run
code from that repository, so pin a tag or commit you trust:

```toml [mise.toml]
[task_config]
includes = [
  "mise-tasks",
  "tasks.toml",
  "git::https://github.com/myorg/shared-tasks.git//tasks?ref=v1.0.0",
]
```

An included `tasks.toml` holds tasks written as under `[tasks]`, without the
`tasks.` prefix. See [remote git includes](/tasks/task-discovery.html#remote-git-includes)
for the URL syntax and caching.

### Reuse task settings with templates {#reuse-task-definitions-with-templates}

A [task template](/tasks/templates.html) holds tools, environment variables and
commands that several tasks share:

```toml [mise.toml]
[task_templates."node:test"]
tools = { node = "24", pnpm = "latest" }
run = "pnpm test"

[tasks.test]
extends = "node:test"

[tasks."test:watch"]
extends = "node:test"
run = "pnpm test --watch"
```

Tasks inherit the template's fields and override the ones they set.

### Redact secrets from task output {#redact-secrets-from-task-output}

To mask values in task output, list their variables in the top-level
`redactions` array, or mark one variable with `redact = true`, which also works
in a task's `env`:

```toml [mise.toml]
redactions = ["API_KEY", "SECRETS_*"]

[tasks.deploy]
env = { TOKEN = { value = "{{ env.DEPLOY_TOKEN }}", redact = true } }
run = "./deploy.sh"
```

mise prints `[redacted]` in place of matching values. Raw and interactive tasks
bypass redaction, and the commands still receive the real values. See
[redaction](/environments/secrets/#redaction) for what it covers.

### Use prebuilt binaries for `cargo:` tools {#cargo-binstall}

Install [cargo-binstall](https://github.com/cargo-bins/cargo-binstall), and mise
uses it for [`cargo:` tools](/dev-tools/backends/cargo.html), which downloads a
prebuilt binary when the crate publishes one instead of compiling it:

```sh
mise use -g cargo-binstall
```

When no prebuilt binary exists, mise falls back to `cargo install`. The
[`cargo.binstall`](/configuration/settings.html#cargo.binstall) setting turns
this off.

## Shell prompt {#shell-prompt}

mise has no prompt segment of its own. In a shell with
[`mise activate`](/cli/activate.html), it can print what it loads when you
change directories, and it exports `[env]` variables that your prompt can read.

### Print what changes when you enter a project {#print-what-changes-when-you-enter-a-project}

```sh
mise settings set status.show_tools true
mise settings set status.show_env true
```

These commands write to your global config. Entering a project then prints
lines such as:

```text
mise +node@24.21.0
mise +API_KEY +NODE_ENV
```

[`status.show_tools`](/configuration/settings.html#status.show_tools) lists the
tools that became active (`+`) or inactive (`-`).
[`status.show_env`](/configuration/settings.html#status.show_env) lists the
names of variables that mise added (`+`), changed (`~`) or removed (`-`), without
their values.

### Show the project and environment in your prompt {#show-the-project-and-environment-in-your-prompt}

Set variables for the prompt in the project's `mise.toml`. mise exports them
while you are in the project and removes them when you leave:

```toml [mise.toml]
[env]
PROMPT_PROJECT = "{{ config_root | basename }}"
PROMPT_MISE_ENV = "{{ mise_env | default(value=[]) | join(sep=',') }}"
```

`config_root` is the directory that holds the config file. `mise_env` lists the
active [config environments](/configuration/environments.html), whether they
come from `MISE_ENV` or `.miserc.toml`. Then read the variables in your shell
startup file. For Zsh, in `~/.zshrc`:

```zsh
setopt PROMPT_SUBST
PROMPT='${PROMPT_PROJECT:+[$PROMPT_PROJECT${PROMPT_MISE_ENV:+:$PROMPT_MISE_ENV}] }%~ %# '
```

For Bash, in `~/.bashrc`:

```sh
PS1='${PROMPT_PROJECT:+[$PROMPT_PROJECT${PROMPT_MISE_ENV:+:$PROMPT_MISE_ENV}] }\w \$ '
```

In a project named `web` with the `staging` environment, the prompt starts with
`[web:staging]`. Prompt themes that show an environment variable work the same
way. For [powerline-go](https://github.com/justjanne/powerline-go), add
`shell-var` to `-modules` and pass
`-shell-var PROMPT_MISE_ENV -shell-var-no-warn-empty`.

Read `PROMPT_MISE_ENV` rather than `MISE_ENV`. An environment selected in
`.miserc.toml` does not set the shell's `MISE_ENV`, and exporting `MISE_ENV`
yourself overrides the `.miserc.toml` selection in every project.

### See which variables mise set {#see-which-variables-mise-set}

```sh
mise env --json-extended
```

[`mise env --json-extended`](/cli/env.html) lists every variable mise sets in
the current directory with its value and the config file that set it as
`source`. Variables that a tool sets, such as Java's `JAVA_HOME`, also name the
`tool`. The output contains real values, including values marked for
redaction, so redact it before you share it. For other problems, start with
[`mise doctor`](/cli/doctor.html).

## Platforms

### Run Intel tools on Apple silicon {#macos-rosetta}

To install the Intel build of a tool on an Apple silicon Mac, set
[`arch`](/configuration/settings.html#arch) to `x64` with `MISE_ARCH`. Give those
installs their own data directory so they do not replace the native ones, and
use the same two variables when you run the tools:

```sh
export MISE_DATA_DIR="$HOME/.local/share/mise-x64"
export MISE_ARCH=x64
mise install node@24
mise exec node@24 -- node --version
```

Rosetta must be installed to run Intel binaries. Tools that mise compiles from
source also need an Intel toolchain and libraries; see the tool's
[language guide](/core-tools.html).

If a backend needs mise itself to run as an Intel process, install the Intel
build of mise next to the native one with the installer's
[`MISE_INSTALL_ARCH`](/installing-mise.html#installer-options) variable, and use
the same `MISE_DATA_DIR` with it:

```sh
curl -fsSL https://mise.run | MISE_INSTALL_PATH="$HOME/.local/bin/mise-x64" MISE_INSTALL_ARCH=x64 sh
"$HOME/.local/bin/mise-x64" --version
```
