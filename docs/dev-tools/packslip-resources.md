# Packslip Completions and Skills

Tools installed with the [packslip backend](/dev-tools/backends/packslip.html)
can provide completions and agent skills for the version active in your project.
The vendor declares these resources in its release manifest; mise fetches them
with the tool and exposes them to your shell or agent.

A tool installed through another backend does not acquire these resources just
because its publisher also ships a packslip. The installed version must have
Packslip metadata, and that manifest must declare the resource you want.

The examples below use `mytool` for an installed executable that declares
completions or skills. Replace it with your command's name.

## Completions

First, print the completion script to check that the active tool provides one:

```sh
mise completion zsh --tool mytool
```

Then install a stub for the command:

```sh
mise completion zsh --tool mytool --install
```

`--install` takes the executable name as typed, such as `mytool`, rather than
a backend identifier containing slashes. For a release with several commands,
choose the command you want to complete. Without `--tool`, `mise completion`
generates completions for mise itself.

The command writes a completion file but does not edit your shell configuration.
Follow any one-time setup line it prints, then load the completion file or start
a new shell. It preserves an existing file that mise did not create unless you
explicitly pass `--force`.

### Follow the active version

The installed stub asks mise for the script when you complete the command.
It does not generate a completion by running the vendor tool at shell startup.
After you change directories or select another version with `mise use`, the
next completion uses that directory's active version.

| Shell      | How the installed stub follows version changes                                                               |
| ---------- | ------------------------------------------------------------------------------------------------------------ |
| zsh        | Temporarily hands a completion to the vendor script, then restores mise's stub.                              |
| bash       | Hands completion to the vendor registration and restores the stub at the next prompt.                        |
| fish       | Loads the vendor script in a child shell for each completion, keeping registrations out of the parent shell. |
| PowerShell | Hands a completion to the vendor completer, then restores mise's completer.                                  |

These four are the shells `mise completion` knows, for `--tool` with and
without `--install`. A tool's packslip completions are not available in any
other shell.

### Static files, usage specs, and generated scripts

mise prefers a completion file supplied by the vendor, then a script derived
from a static [usage](https://usage.jdx.dev) CLI spec, then a vendor command
that generates a completion or CLI spec. Selection is scoped to the installed
artifact, executable, and shell.

Usage-derived completions require the `usage` command both to generate the
script and at shell runtime:

```sh
mise use -g usage
```

If the only usable source runs the vendor executable, mise runs it on demand
and caches successful output per installed version, executable, and shell.
Concurrent requests share that work. Empty output, a failure, or a timeout
does not become a cached script; mise tries the next source.

Only generation takes turns, and only generation writes: a completion the
release ships is read straight out of the install, so a read-only system or
shared install still completes.

Calling `mise completion ... --tool` directly can also trigger generation.
The [`packslip.exec`](/configuration/settings.html#packslip.exec) setting does
not gate on-demand completion generation. It controls resource commands run
during installation, such as generated skills.

## Skills

A skill is a directory containing `SKILL.md` and any supporting files. mise
fetches declared skills during tool installation by default, so different tool
versions can carry different skill content.

Inspect the installed skills of tools active in the current directory:

```sh
mise skills ls
mise skills ls --json
```

Link them where your agent reads skills:

```sh
mise skills sync --dir .agents/skills
```

By default, `mise skills sync` uses `.claude/skills` under the nearest mise
project root. `--dir` selects a directory for that invocation.
`--global` instead resolves the configured skill directory under your home
folder. With the default setting, that is `~/.claude/skills`.

Each link points into the active tool version's install directory. Run sync
after a version change to update those links. Only mise-owned links are replaced;
a user-created directory or unrelated link at the same name is preserved and
reported as skipped.

### Keep project links up to date

Configure your agent's directory and enable synchronization after `mise install`
and `mise use`:

```toml
[settings.skills]
dir = ".agents/skills"
auto_sync = true
prune = true
```

`prune = true` also removes mise-owned links for skills no longer active. Without
it, stale links remain. To prune on one manual run:

```sh
mise skills sync --dir .agents/skills --prune
```

Automatic sync requires a mise project root. It does not run merely because
you change directories, and it does not change an already running agent's
skill-loading behavior. Reload skills as your agent requires.

### Choose whether to fetch or generate skills

| Setting                                                             | Default          | Effect                                                                      |
| ------------------------------------------------------------------- | ---------------- | --------------------------------------------------------------------------- |
| [`skills.fetch`](/configuration/settings.html#skills.fetch)         | `true`           | Fetch declared skills while installing tools.                               |
| [`skills.dir`](/configuration/settings.html#skills.dir)             | `.claude/skills` | Choose the directory used by sync.                                          |
| [`skills.auto_sync`](/configuration/settings.html#skills.auto_sync) | `false`          | Sync after install and use within a mise project.                           |
| [`skills.prune`](/configuration/settings.html#skills.prune)         | `false`          | Remove stale mise-owned links during sync.                                  |
| [`packslip.exec`](/configuration/settings.html#packslip.exec)       | `false`          | Allow running the installed tool to produce a resource during installation. |

A skill can be fetched from the artifact, a separate signed asset, or the source
repository at the release commit. A skill offered only as an `exec` command is
generated during installation only when `packslip.exec` is enabled. That command
runs the newly installed vendor executable and must print `SKILL.md` content.
Fetched skills do not require executing the tool.

Turning off `skills.fetch` affects skill fetching during installation; it does
not delete previously fetched files or existing links. Run sync with pruning
when you want to remove links to inactive skills.

## Resource selection and command execution

Resources can target an exact artifact filename or a platform. mise selects
resources for the artifact it installed, preferring exact artifact scope and
then the most specific platform scope. Different executables, shells, and skill
names identify separate resources.

Within an identity, equally scoped sources are alternatives. mise prefers an
archive file, then a signed asset, then a source-repository file. Static CLI specs
follow the same ordering. Declaration order breaks ties within a source type.
A usable higher-priority skill source prevents fetching or running lower ones.
A directory counts as a skill only when `SKILL.md` is present.

Separate resource assets must match their signed digests; repository resources
are pinned to the release's source commit. A missing optional resource can leave
the tool installed without it. A digest mismatch is a verification failure,
not permission to choose an unverified alternative.

When mise runs a resource generator, it:

- Adds the installed executables to PATH and applies the manifest's environment
  variables, expanding `{shell}` for completion resources.
- Uses a temporary working directory, no stdin, and discarded stderr.
- Enforces a five-second deadline and a 4 MiB output limit, and cleans up child
  processes after completion, failure, timeout, or cancellation.

These limits bound generation; they do not make vendor code a sandboxed program.
The [backend trust checks](/dev-tools/backends/packslip.html#what-is-verified)
establish which publisher's executable is being used.

## Troubleshooting

| Symptom                                        | What to check                                                                                                                                                                 |
| ---------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Tool was not installed from a packslip         | Check the active tool's backend and version with `mise ls`.                                                                                                                   |
| No completion declared                         | Check whether the release declares a matching shell, command, or usage spec. For multiple executables, pass the command name.                                                 |
| `--install` rejects a tool identifier          | Use the executable name, not `packslip:host/owner/repo`.                                                                                                                      |
| A completion file already exists               | Inspect its source before choosing whether to replace it with `--force`.                                                                                                      |
| Script prints but tab completion does not work | Follow the shell setup printed by `--install`; check that the stub is loaded and `usage` is on PATH if required.                                                              |
| Completion generation fails                    | Check whether the vendor command emits nonempty output within the time and size limits.                                                                                       |
| No skills listed                               | Check the active installed version, its declared resources, `skills.fetch`, and whether `SKILL.md` is present. An exec-only skill also needs `packslip.exec` at installation. |
| A skill link is skipped                        | A user-owned file or directory may occupy its name; mise leaves it intact.                                                                                                    |
| Links still point to an old version            | Run `mise skills sync` after changing versions, or enable automatic sync.                                                                                                     |

For exact flags, see [`mise completion`](/cli/completion.html),
[`mise skills ls`](/cli/skills/ls.html), and [`mise skills sync`](/cli/skills/sync.html).
Publishers can follow the [Packslip resources guide](https://packslip.dev/docs/resources/)
to add these declarations to their releases.
