---
description: "Change how mise installs tools, loads config and runs tasks, and see which value wins when a setting is set in several places."
outline: [2, 3]
---

# Settings

<script setup>
import Settings from '/components/settings.vue';
</script>

Settings change how mise itself behaves, such as how many jobs run at once, how
downloads are verified and how tasks print output. Set them with
`mise settings set` or under `[settings]` in a config file. Variables for your
own programs belong in [`[env]`](/environments/) instead.

## Change a setting

[`mise settings set`](/cli/settings/set.html) writes to the global config,
`~/.config/mise/config.toml`. Add `--local` to write to the project config
instead, normally `mise.toml` (see
[which file mise writes to](/configuration.html#target-file-for-write-operations)):

```sh
mise settings set jobs 4          # global config
mise settings set --local jobs 2  # project config
mise settings unset --local jobs  # remove the project value
mise settings add disable_hints python_multi  # append to a list setting
```

The same settings in TOML:

```toml
[settings]
jobs = 4

[settings.node]
compile = true # the node.compile setting
```

A dotted name such as `node.compile` is a table under `[settings]`. Each entry
in the [reference](#reference) lists the setting's environment variable,
usually `MISE_` followed by the name in upper case with dots replaced by
underscores, such as `MISE_NODE_COMPILE`.

To see what mise uses:

```sh
mise settings get jobs           # the value in effect here
mise settings ls                 # values set in config files, with each file
mise settings ls --all           # every setting, including defaults
```

## Which value wins

When a setting is set in more than one place, mise uses the first of these that
sets it:

1. A command-line flag, such as `--jobs` or `--yes` (see
   [global flags](/cli/#global-flags))
2. The setting's environment variable, such as `MISE_JOBS`
3. `[settings]` in project config, starting with the file nearest the current
   directory; `mise.local.toml` wins over `mise.toml` in the same directory
4. `[settings]` in the global config, `~/.config/mise/config.toml`
5. `[settings]` in the system config, `/etc/mise/config.toml`
6. The default

[`minimum_release_age_excludes`](/configuration/settings.html#minimum_release_age_excludes) is the
exception: it combines the lists from every config file, and its environment
variable replaces the combined list. In
[safe mode](/security.html#safe-mode), mise ignores `[settings]` in project
config.

## Settings with restricted locations

Most settings work in every place listed above. Three kinds do not, and the
reference marks each of them with a badge.

### Global-only settings

Settings with the <Badge type="info" text="global only" /> badge take effect
only in the global or system config, or in their environment variable where
they have one, so a repository you clone cannot set them. Most of them control
what mise runs, trusts, downloads or writes on your machine, or whether it asks
before acting, such as [`yes`](/configuration/settings.html#yes),
[`trusted_config_paths`](/configuration/settings.html#trusted_config_paths), [`paranoid`](/configuration/settings.html#paranoid),
[`github.credential_command`](/configuration/settings.html#github.credential_command),
[`self_update.auto`](/configuration/settings.html#self_update.auto) and [`shims_dir`](/configuration/settings.html#shims_dir). In
project config, mise ignores them and prints a warning:

```text
mise WARN  yes in non-global config /home/me/app/mise.toml is ignored for security reasons
```

`mise settings set --local` and `mise settings add --local` refuse these
settings. Drop `--local` to write them to the global config, or use the
environment variable.

### Settings read before config files {#early-initialization}

Settings with the <Badge type="info" text="miserc" /> badge decide which config
files mise loads, so mise reads them before any `mise.toml`: [`env`](/configuration/settings.html#env),
[`auto_env`](/configuration/settings.html#auto_env), [`env_conf_d`](/configuration/settings.html#env_conf_d),
[`ceiling_paths`](/configuration/settings.html#ceiling_paths),
[`ignored_config_paths`](/configuration/settings.html#ignored_config_paths),
[`override_config_filenames`](/configuration/settings.html#override_config_filenames) and
[`override_tool_versions_filenames`](/configuration/settings.html#override_tool_versions_filenames). Set
them in a [`.miserc.toml` file](/configuration.html#miserc), without a
`[settings]` table, or with their environment variables. `env` also has the
`-E`/`--env` flag.

```toml [.miserc.toml]
env = ["development"]
```

Under `[settings]` in `mise.toml` or the global config they have no effect.
`mise settings set` refuses them and names the `miserc.toml` file or variable to
use, so edit `.miserc.toml` yourself.

### Environment-only settings

Settings with the <Badge type="info" text="env only" /> badge name the config
files themselves, so they work only as environment variables:
[`default_config_filename`](/configuration/settings.html#default_config_filename),
[`default_tool_versions_filename`](/configuration/settings.html#default_tool_versions_filename),
[`global_config_file`](/configuration/settings.html#global_config_file),
[`global_config_root`](/configuration/settings.html#global_config_root) and
[`system_config_file`](/configuration/settings.html#system_config_file). `mise settings set` refuses them,
and mise ignores them, with a warning, in every config file, including the
global one. A `.miserc.toml` ignores them without a warning.

Environment variables that are not settings, such as `MISE_DATA_DIR`, are
listed in [`MISE_*` variables](/configuration/environment-variables.html).

## Reference

<Settings :level="3" index />
