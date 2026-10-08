---
description: Use the man pages, shell completions, and agent skills a packslip release declares, matched to the tool version active in each project.
socialDescription: Version-matched man pages, completions, and agent skills from packslip releases.
---

# Man pages, completions, and skills

Tools installed with the [packslip backend](/dev-tools/backends/packslip.html)
can ship man pages, shell completions, and agent skills that follow the tool
version active in each project. The publisher declares them in the release
manifest. Install the tool with a `packslip:` identifier to get them; an
installation from another backend does not have them.

## Man pages

When an installed tool's manifest declares a static `man` resource, mise adds
the tool's man pages to `MANPATH` while that tool version is active. This works
in an activated shell and in the environments that `mise exec`, `mise run`, and
`mise env` create. Switching projects selects the matching version's pages.

mise keeps these pages under `.mise-packslip/man` in the tool's installation and
prepends that directory to an existing `MANPATH`. When `MANPATH` was unset, mise
keeps the operating system's default manual-page locations. If you installed
the tool with mise 2026.9.3 or earlier, reinstall it once
(`mise install --force TOOL@VERSION`) so its man pages are set up.

Man pages can come from the release archive, a separately signed release asset,
or the source repository at the release commit. A file such as `tool.1` is
placed under `man1`, and compressed pages such as `tool.5.gz` also work. mise
does not install man pages that would need a generator command (`exec`) or that
exist only as a `cli-spec`.

## Completions

Completions work in zsh, bash, fish, and PowerShell, and they follow the tool
version active in each project, so you never reinstall them after switching
versions.

### Use completions

With [mise activated](/shell-setup.html), a tool's completions are available
whenever the tool is active in your project:

```sh
mise use packslip:github.com/jdx/hk
mise exec -- hk --version
```

Then type `hk` and press Tab. hk ships native completion scripts, so no extra
setup or `usage` install is needed. mise registers a loader in the shell and
reads the publisher's script only when you complete a command. Switching
projects or tool versions selects the matching completion, and leaving the
project removes its registration.

### Manual setup without shell activation

Without shell activation, mise can install a completion file for a tool whose
manifest declares completions. Replace `TOOL` with the executable's name:

| Shell      | Command                                            |
| ---------- | -------------------------------------------------- |
| zsh        | `mise completion zsh --tool TOOL --install`        |
| bash       | `mise completion bash --tool TOOL --install`       |
| fish       | `mise completion fish --tool TOOL --install`       |
| PowerShell | `mise completion powershell --tool TOOL --install` |

Follow any one-time setup the command prints, then load the completion file or
start a new shell. mise writes the completion file but does not edit your shell
configuration, and it does not overwrite an existing file it did not create
unless you pass `--force`. The installed file calls mise when you complete a
command, so after you change directories or run `mise use`, the next completion
uses that directory's active version.

To print a completion script without installing it, leave out `--install`:

```sh
mise completion zsh --tool TOOL
```

`--tool` accepts the executable name (`hk`) or the tool identifier
(`packslip:github.com/jdx/hk`). With `--install`, pass the executable name,
because the installed file is named after it. If a release contains several
commands, name the one you want to complete. Without `--tool`,
[`mise completion`](/cli/completion.html) generates completions for mise itself.

<span id="static-files-usage-specs-and-generated-scripts"></span>

### Generated completions

A publisher can provide a completion file, a static usage CLI specification, or
a command that generates either one. mise prefers a completion file, then a
static specification, then the command. A completion derived from a usage
specification uses the engine built into mise, so you do not need to install
`usage`.

If a completion needs the publisher's generator command, mise runs it on demand
and caches successful output for the installed version, executable, and shell.
That can happen during tab completion or a direct `mise completion --tool` run,
but not at shell startup. Static completion files are read in place, without
writing a cache, so they also work from read-only or shared installs.
[`packslip.exec`](/configuration/settings.html#packslip.exec) applies only
during installation; it does not stop on-demand completion generation.

## Skills

A skill is a directory containing `SKILL.md` and supporting files. mise fetches
the skills a release declares when it installs the tool, so each version carries
its own. See the skills from the tools active in your project:

```sh
mise skills ls
mise skills ls --json
```

Link them where your agent reads skills:

```sh
mise skills sync --dir .agents/skills
```

These are symlinks into the active tool version's install directory, not
portable copies. Keep the generated links out of version control and have each
developer run sync on their own machine. mise records the links it owns in
`.mise-skills.json` in the same directory; ignore that file too. The directory
can also hold handwritten skills: mise leaves user-created directories and
unrelated links alone and reports any conflicting names as skipped. Run sync
again after a version change to update the links.

### Choose a skill directory

Without `--dir`, sync uses the [`skills.dir`](/configuration/settings.html#skills.dir)
setting under the nearest mise project root. Set it to the directory your agent
reads; `--dir` overrides it for one run.

`mise skills sync --global` uses the same setting under your home directory
instead. An absolute directory is used as written.

### Keep project links up to date

Add this to the project's `mise.toml`, or to your global config to apply it
everywhere:

```toml
[settings.skills]
dir = ".agents/skills"
auto_sync = true
prune = true
```

With `auto_sync`, mise syncs after `mise install` and after `mise use` changes a
version. `prune = true` removes links mise made for skills that are no longer
active; without it, stale links stay. To prune on one manual run:

```sh
mise skills sync --dir .agents/skills --prune
```

Automatic sync needs a mise project root, and changing directories does not
trigger it. Reload skills in your agent if it does not notice the change.

### Skill settings {#choose-whether-to-fetch-or-generate-skills}

| Setting                                                             | Default          | Effect                                                             |
| ------------------------------------------------------------------- | ---------------- | ------------------------------------------------------------------ |
| [`skills.fetch`](/configuration/settings.html#skills.fetch)         | `true`           | Fetch declared skills during tool installation.                    |
| [`skills.dir`](/configuration/settings.html#skills.dir)             | `.claude/skills` | Choose the directory sync links into.                              |
| [`skills.auto_sync`](/configuration/settings.html#skills.auto_sync) | `false`          | Sync after install and use within a mise project.                  |
| [`skills.prune`](/configuration/settings.html#skills.prune)         | `false`          | Remove stale mise-owned links during sync.                         |
| [`packslip.exec`](/configuration/settings.html#packslip.exec)       | `false`          | Run the installed tool to generate a resource during installation. |

A skill can come from the artifact, a separate signed asset, or the source
repository at the release commit, and fetching it does not run the tool. A
separate asset must match its signed digest, and a mismatch is a verification
failure. A skill offered only as an `exec` command is generated during
installation when `packslip.exec` is on; that command runs the newly installed
executable and must print `SKILL.md` content.

Turning off `skills.fetch` skips fetching from then on. It does not delete
skills already fetched or existing links; run sync with `--prune` to remove
links to skills that are no longer active.

### When a declared skill is not installed

`mise skills ls` and `mise skills sync` name each skill a release declares that
is missing from the install, with the reason: `skills.fetch` is off, the skill
is generated by an `exec` command and `packslip.exec` is off, or the download
failed at install time, in which case reinstalling the tool fetches it.

## When mise runs a publisher's command {#resource-selection-and-command-execution}

If a completion or skill is available only as a command, mise runs the installed
executable with its bin directory on `PATH` and the manifest's environment
variables, in a temporary directory with no stdin. Each run has a five-second
limit and a 4 MiB output limit, and mise cleans up child processes when it ends.
Empty output, a failure, or a timeout is not cached, and mise tries the next
source. The run is not sandboxed: the
[verification checks](/dev-tools/backends/packslip.html#what-is-verified)
establish whose executable it is.

When a release offers several sources for one resource, mise prefers one
declared for the exact artifact it installed over one declared for a platform.
Among equally specific sources, it prefers a file inside the artifact, then a
separate signed asset, then a file from the source repository. The
[packslip resources guide](https://packslip.dev/docs/resources/) describes the
full selection rules for publishers.

## Troubleshooting

| Symptom                                        | Next step                                                                                                                                                                              |
| ---------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Tool was installed through another backend     | Check `mise ls`, then install with an explicit `packslip:` identifier from a release that declares the resource.                                                                       |
| No completion declared                         | Confirm the release supports your shell and executable. If it does not, the publisher must add a completion or CLI spec.                                                               |
| `--install` rejects a tool identifier          | Pass the executable name, such as `hk`, instead of `packslip:github.com/jdx/hk`.                                                                                                       |
| A completion file already exists               | Inspect the existing file before replacing it with `--force`.                                                                                                                          |
| Script prints but tab completion does not work | Check that mise is activated and the tool is active in this project. With manual setup, follow the instructions printed by `--install`. mise handles usage-derived completions itself. |
| Completion generation fails                    | Check that the publisher's command produces nonempty output within the time and size limits. Report a failing generator to the publisher.                                              |
| Man page not found                             | Check that the tool is active (`mise ls --current`) and that its release declares a man page. Reinstall a tool installed with mise 2026.9.3 or earlier.                                |
| No skills listed                               | Check `mise skills ls`, the active version, and whether its manifest declares skills. Check `skills.fetch`; an exec-only skill also needs `packslip.exec` during installation.         |
| A skill link is skipped                        | Inspect the conflicting path; mise does not touch user-owned files and directories.                                                                                                    |
| Links point to an old version                  | Run `mise skills sync`, or enable `skills.auto_sync` for future installs.                                                                                                              |

For every flag, see [`mise completion`](/cli/completion.html),
[`mise skills ls`](/cli/skills/ls.html), and
[`mise skills sync`](/cli/skills/sync.html). Publishers can follow the
[packslip resources guide](https://packslip.dev/docs/resources/) to declare
these resources.
