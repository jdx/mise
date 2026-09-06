# Settings

<script setup>
import Settings from '/components/settings.vue';
</script>

Settings control mise itself, such as installation concurrency and task output.
Application environment variables belong in [`[env]`](/environments/).

## Change a setting

The settings command writes to the global config by default. Use `--local` to
write to the project's selected config file:

```sh
mise settings set jobs 4          # personal default
mise settings set --local jobs 2  # project setting
mise settings unset --local jobs  # remove the project override
```

The equivalent TOML is:

```toml
[settings]
jobs = 4
```

Use `mise settings ls --all` to inspect effective settings, including defaults.
`mise settings ls --json-extended` includes source information for configured
values. A setting's reference below lists its type, default, and environment
variable when available. Some settings also have global CLI flags.

## Early initialization

Some settings control config discovery and are read before `mise.toml`. These
must be set through their environment variable or, where supported, a
[`.miserc.toml` file](/configuration/environments.html#setting-mise-env-in-miserc-toml).
Follow the individual setting's instructions; putting an early setting under
`[settings]` can be too late to affect discovery.

## Reference

<Settings :level="2" />
