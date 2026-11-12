---
description: "[experimental] Apply desktop and app settings from mise.toml through bootstrap settings plugins, such as one a Linux distro ships."
socialDescription: "Apply desktop and app settings from mise.toml through bootstrap settings plugins."
---

# Bootstrap settings plugins <Badge type="warning" text="experimental" />

A bootstrap settings plugin applies the settings of one program or desktop,
such as a Linux distro's appearance settings, from a
`[bootstrap.settings.<plugin>]` section. The plugin knows where those settings
live. mise compares the values you declare with the values the plugin reports
and asks the plugin to write only the ones that differ.

This is experimental: set
[`experimental = true`](/configuration/settings.html#experimental) or
`MISE_EXPERIMENTAL=1` to use it.

## Declare settings

Name the plugin in the section, then write its settings as TOML. Which keys a
plugin accepts is up to the plugin. For a plugin named `desk`:

```toml
[bootstrap.settings.desk.appearance]
blur = true
blur_strength = 4
```

Preview and apply them with the rest of the bootstrap, or on their own:

```sh
mise bootstrap --only settings --dry-run
mise bootstrap --only settings
mise bootstrap settings status
mise bootstrap settings apply
```

A dry run prints each value that would change, with its current value:

```text
mise would set desk: appearance.blur false -> true
mise would set desk: appearance.blur_strength 2 -> 4
```

`mise bootstrap settings status --missing` exits with status 1 when a value
differs from the config, for example after someone changes it in the program's
own settings.

Sections merge across config files key by key, and a more local file's value
wins, as for the rest of `[bootstrap]`. `appearance = { blur = true }` and
`"appearance.blur" = true` set the same key. When `mise bootstrap` reads several
independent config roots, they must not set a key to different values; mise
refuses to apply settings when they do.

A section for a plugin that is not installed, or that cannot run on this
machine, is skipped and listed in the `bootstrap: follow-up` output, so one
config can hold sections for several platforms. `mise bootstrap` applies
settings after it installs `[tools]`, so a program the plugin `requires` can be
one of your global tools.

## Where plugins come from

A distro or app can ship a settings plugin in
[`/usr/share/mise/plugins`](/directories.html#system-plugins), so every user on
the machine can use it without installing anything. You can also install one
yourself, under any name, through
[`[bootstrap.plugins]`](/bootstrap/packages/plugins.html) or
`mise plugins link`. A plugin you install overrides a system plugin of the same
name.

## Write a plugin {#write-a-plugin}

A bootstrap settings plugin is a Lua plugin with two hooks:

```text
mise-desk/
├── metadata.lua
├── mise.plugin.toml
└── hooks/
    ├── settings_read.lua    # report current values
    └── settings_write.lua   # write changed values
```

`mise.plugin.toml` needs a `[bootstrap-settings]` table, which marks the
directory as a bootstrap settings plugin. It accepts the same keys as
[`[package-manager]`](/package-plugin-development.html#mise-plugin-toml) except
`supports_version_pins`:

```toml
[bootstrap-settings]
requires = ["hyprctl"]  # programs the hooks run
os = ["linux"]          # platforms the plugin runs on
```

mise flattens the section into dotted keys. `[bootstrap.settings.desk.appearance]`
with `blur = true` becomes the key `appearance.blur` with the value `true`. A
list stays one value.

### SettingsRead

`ctx.keys` holds every declared key. Return the current value of each one you
know, in `values`. Leave a key out when it is unset.

```lua [hooks/settings_read.lua]
function PLUGIN:SettingsRead(ctx)
  return { values = { ["appearance.blur"] = false, ["appearance.blur_strength"] = 2 } }
end
```

mise calls this hook for status, dry runs, and before and after every write.
Keep it free of side effects and prompts.

### SettingsWrite

`ctx.changes` holds only the keys whose value differs, each a table with
`key`, `from` (`nil` when unset), and `to`. Write each one. Return nothing, or
`follow_up`, a list of steps the user must take for the change to show:

```lua [hooks/settings_write.lua]
function PLUGIN:SettingsWrite(ctx)
  for _, change in ipairs(ctx.changes) do
    -- write change.to for change.key
  end
  return { follow_up = { "log out and back in to apply the new theme" } }
end
```

After `SettingsWrite`, mise calls `SettingsRead` again and fails if a key still
differs, listing the `follow_up` steps in the error. mise compares numbers by
value, so `4` and `4.0` match; other values must have the same type, so `4` and
`"4"` differ.

### Rules {#rules}

- Never run `sudo`. A settings plugin changes the current user's settings.
- Change only the keys in `ctx.changes`. A key that is not declared is not a
  request to reset it.
- List every program the hooks run in `requires`.

The hooks can use the `cmd`, `json`, `file`, and other modules in the
[Plugin Lua reference](/plugin-lua-modules.html).
