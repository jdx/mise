# Coming from rtx

mise was formerly called rtx. The project was renamed to avoid confusion with NVIDIA's
RTX graphics cards and to make the tool easier to find. Old articles, configuration files,
and shell setup may still use the rtx name.

## Migrate an existing setup

1. [Install mise](/installing-mise.html) using your preferred installation method.
2. Review your rtx configuration and move it to the corresponding mise location. Merge with
   existing mise configuration instead of overwriting it; see the mapping below.
3. Replace `rtx activate` in shell startup files with the appropriate
   [mise activation](/getting-started.html#activate-mise) command. Update scripts and any
   `RTX_*` environment variables to their supported `MISE_*` equivalents.
4. Run `mise install` from each project to install its configured tools. In an activated
   shell, run `mise doctor` and check a tool with `mise exec -- node --version` (or another
   tool declared by your project).
5. If you use shims, run `mise reshim` and replace the old rtx shim directory in `PATH`.
   Once the new setup works, remove the old rtx installation using its installation method.

| Old location                | mise location                |
| --------------------------- | ---------------------------- |
| `.rtx.toml`                 | `mise.toml`                  |
| `.rtx.local.toml`           | `mise.local.toml`            |
| `~/.config/rtx/config.toml` | `~/.config/mise/config.toml` |
| Project `.config/rtx/`      | Project `.config/mise/`      |
| Project `.rtx/`             | Project `.mise/`             |

Current mise still recognizes `.rtx.toml` and `.rtx.local.toml`, but use mise filenames for
new configuration. `RTX_*` environment variables are not mise settings; check the
[settings reference](/configuration/settings.html) when renaming an old override.

## Tool installations

Do not rely on current mise to move old rtx installation directories automatically.
Reinstall from configuration with `mise install`. Moving an installed runtime can leave
absolute paths pointing at the old location, particularly for Python environments and Ruby.
Keep the old installation until you have verified the replacement.

The standalone installer puts the mise executable at `~/.local/bin/mise` by default.
Installed tools live under the [data directory](/directories.html). These are different
locations; updating an executable path alone does not move tool installations or shims.

## CI and shared scripts

Replace `rtx-action` with [`jdx/mise-action@v3`](/continuous-integration.html#github-actions)
and review its inputs. Replace direct `rtx` commands in scripts with `mise`.
For repositories whose contributors may not have mise installed, commit a
[generated install wrapper](/continuous-integration.html#bootstrapping).

If migration changes behavior you depend on, include the old configuration and the command
that differs in a [discussion](/contact.html). A small reproduction helps distinguish the
rename from changes introduced since the rtx release you were using.
