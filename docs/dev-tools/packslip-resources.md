# Packslip Completions and Skills

Tools installed with the [Packslip backend](/dev-tools/backends/packslip.html)
can provide shell completions and agent skills that match the version active in
your project. The publisher declares these resources in the release manifest.

Install the tool through `packslip:` to use its declared resources. An existing
installation from another backend does not acquire them automatically.

## Completions

mise supports tool completions for zsh, bash, fish, and PowerShell. The installed
completion file follows the tool version active in each project, so you do not
need to reinstall completions after changing versions.

### Use completions

With [mise activated](/getting-started.html#activate-mise), completions become
available when an installed tool is active in your project. For example:

```sh
mise use hk
```

Type `hk` and press Tab. hk publishes native completion scripts for bash, zsh,
fish, and PowerShell, so no extra setup command or `usage` installation is
needed. mise registers a loader in the shell; it reads the publisher's script
only when you complete a command. Switching projects or tool versions selects
the matching completion, and leaving the project removes its registration.

### Manual setup without shell activation

For tools that declare completions in their Packslip manifest, mise can install
a completion file that loads those resources. Replace `TOOL` below with the
executable's name:

| Shell      | Command                                            |
| ---------- | -------------------------------------------------- |
| zsh        | `mise completion zsh --tool TOOL --install`        |
| bash       | `mise completion bash --tool TOOL --install`       |
| fish       | `mise completion fish --tool TOOL --install`       |
| PowerShell | `mise completion powershell --tool TOOL --install` |

Follow any one-time setup instructions printed by the command, then load the
completion file or start a new shell. mise writes the completion file but does
not edit your shell configuration. It preserves an existing file it did not
create unless you pass `--force`.

To print a completion script without installing it, omit `--install`:

```sh
mise completion zsh --tool TOOL
```

`--install` takes the command name, not a backend identifier such as
`packslip:github.com/jdx/hk`.
If a release contains several commands, choose the one you want to complete.
Without `--tool`, `mise completion` generates completions for mise itself.

### Generated completions

A publisher can provide a completion file, a static usage CLI specification,
or a command that generates either one. mise prefers static sources. A
usage-derived completion uses the engine embedded in mise; you do not need to
install `usage` separately.

If a completion needs a publisher's generator command, mise runs it on demand
and caches successful output for the installed version, executable, and shell.
This can happen during tab completion or a direct `mise completion --tool`
invocation. It does not run the publisher's tool merely at shell startup.

::: info
[`packslip.exec`](/configuration/settings.html#packslip.exec) controls resource
generation **during installation**. Setting it to `false` does not disable
**on-demand completion generation**.
:::

## Skills

A skill is a directory containing `SKILL.md` and any supporting files. mise
fetches declared skills during tool installation by default, so different tool
versions can carry different skill content. Only tools whose manifests declare
skills appear in the following commands.

Inspect skills from the tools active in your project:

```sh
mise skills ls
mise skills ls --json
```

Link them where your agent reads skills:

```sh
mise skills sync --dir .agents/skills
```

Each link points into the active tool version's install directory. Run sync
after a version change to update the links. mise preserves user-created
directories and unrelated links, reporting any conflicting names as skipped.

### Choose a skill directory

Without `--dir`, sync uses `.claude/skills` under the nearest mise project root.
Set [`skills.dir`](/configuration/settings.html#skills.dir) to use your agent's
preferred directory. `--dir` overrides the setting for one invocation.

`mise skills sync --global` resolves the configured directory under your home
folder instead; with the default setting, that is `~/.claude/skills`.
An absolute directory is used as written.

### Keep project links up to date

Configure the skill directory and enable synchronization after `mise install`
and `mise use`:

```toml
[settings.skills]
dir = ".agents/skills"
auto_sync = true
prune = true
```

`prune = true` removes mise-owned links for skills no longer active. Without it,
stale links remain. To prune on one manual run:

```sh
mise skills sync --dir .agents/skills --prune
```

Automatic sync requires a mise project root. It does not run just because you
change directories. Reload skills in your agent if it does not detect changes
automatically.

### Choose whether to fetch or generate skills

| Setting                                                             | Default          | Effect                                                             |
| ------------------------------------------------------------------- | ---------------- | ------------------------------------------------------------------ |
| [`skills.fetch`](/configuration/settings.html#skills.fetch)         | `true`           | Fetch declared skills during tool installation.                    |
| [`skills.dir`](/configuration/settings.html#skills.dir)             | `.claude/skills` | Choose the directory used by sync.                                 |
| [`skills.auto_sync`](/configuration/settings.html#skills.auto_sync) | `false`          | Sync after install and use within a mise project.                  |
| [`skills.prune`](/configuration/settings.html#skills.prune)         | `false`          | Remove stale mise-owned links during sync.                         |
| [`packslip.exec`](/configuration/settings.html#packslip.exec)       | `false`          | Run the installed tool to generate a resource during installation. |

A skill can come from the artifact, a separate signed asset, or the source
repository at the release commit. Fetching these files does not execute the tool.
A skill provided only through an `exec` command is generated during installation
when `packslip.exec` is enabled. That command runs the newly installed executable
and must print `SKILL.md` content.

Turning off `skills.fetch` skips future fetching; it does not delete previously
fetched files or existing links. Use sync with pruning to remove links to skills
that are no longer active.

## Resource selection and command execution

The following details apply when a release offers multiple sources for a resource.

### Source selection

Resources can target an exact artifact filename or a platform. mise selects
resources for the artifact it installed, preferring an exact artifact match,
then the most specific platform match. Different executables, shells, and skill
names identify separate resources.

For equally specific alternatives, mise prefers a file inside the artifact,
then a separate signed asset, then a source-repository file. Static CLI specs
follow the same ordering. Declaration order breaks ties within a source type.
A usable higher-priority skill source prevents fetching or running lower ones.
A directory counts as a skill only when `SKILL.md` is present.

For completions, mise first tries a supplied completion file, then a static usage
CLI spec, then a command that generates a completion or CLI spec. Selection is
specific to the installed artifact, executable, and shell.

Separate resource assets must match their signed digests; repository resources
are pinned to the release's source commit. A missing optional resource can leave
the tool installed without it. A digest mismatch is a verification failure.

### Generation and caching

Concurrent requests for the same generated completion share the work. Empty
output, a failure, or a timeout is not cached; mise tries the next source.
Static completion files are read directly from the installation without writing
a cache, so they also work from read-only or shared installs.

When mise runs a resource generator, it:

- Adds the installed executables to PATH and applies the manifest's environment
  variables, expanding `{shell}` for completion resources.
- Uses a temporary working directory, no stdin, and discarded stderr.
- Enforces a five-second deadline and a 4 MiB output limit, and cleans up child
  processes after completion, failure, timeout, or cancellation.

These limits do not sandbox the executable. The
[backend verification checks](/dev-tools/backends/packslip.html#what-is-verified)
establish which publisher's executable is being used.

### How completion files follow the active version {#follow-the-active-version}

The installed completion file delegates to mise when you complete a command.
After you change directories or select another version with `mise use`, the
next completion uses that directory's active version.

| Shell      | Implementation                                                                                                    |
| ---------- | ----------------------------------------------------------------------------------------------------------------- |
| zsh        | Temporarily delegates to the publisher's script, then restores mise's completion function.                        |
| bash       | Delegates to the publisher's registration and restores mise's function at the next prompt.                        |
| fish       | Loads the publisher's script in a child shell for each completion, keeping registrations out of the parent shell. |
| PowerShell | Temporarily delegates to the publisher's completer, then restores mise's completer.                               |

<span id="static-files-usage-specs-and-generated-scripts"></span>

## Troubleshooting

| Symptom                                        | Next step                                                                                                                                                                              |
| ---------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Tool was installed through another backend     | Check `mise ls`, then use an explicit `packslip:` identifier to install a release that declares the resource.                                                                          |
| No completion declared                         | Confirm the release supports your shell and executable. If it does not, the publisher must add a completion or CLI spec.                                                               |
| `--install` rejects a tool identifier          | Pass the executable name, such as `hk`, rather than `packslip:github.com/jdx/hk`.                                                                                                      |
| A completion file already exists               | Inspect the existing file before choosing to replace it with `--force`.                                                                                                                |
| Script prints but tab completion does not work | Check that mise is activated and the tool is active in this project. With manual setup, follow the instructions printed by `--install`. Mise handles usage-derived completions itself. |
| Completion generation fails                    | Check that the publisher's command produces nonempty output within the time and size limits. Report a failing generator to the publisher.                                              |
| No skills listed                               | Check `mise skills ls`, the active version, and whether its manifest declares skills. Check `skills.fetch`; an exec-only skill also needs `packslip.exec` enabled during installation. |
| A skill link is skipped                        | Inspect the conflicting path; mise preserves user-owned files and directories.                                                                                                         |
| Links point to an old version                  | Run `mise skills sync`, or enable `skills.auto_sync` for future install/use operations.                                                                                                |

For exact flags, see [`mise completion`](/cli/completion.html),
[`mise skills ls`](/cli/skills/ls.html), and [`mise skills sync`](/cli/skills/sync.html).
Publishers can follow the [Packslip resources guide](https://packslip.dev/docs/resources/)
to add these declarations to their releases.
