---
description: "[experimental] Write a Lua plugin that applies bootstrap settings, such as the appearance settings of a Linux desktop."
socialDescription: "Write a Lua plugin that applies bootstrap settings, such as the appearance settings of a Linux desktop."
---

# Writing settings plugins <Badge type="warning" text="experimental" />

A settings plugin applies the settings of one program or desktop, such as a
Linux distro's appearance settings, from a `[bootstrap.settings.<plugin>]`
section. The plugin reports current values and writes changed ones; mise
compares them with the config and decides what to write. Users set up a plugin
as described in the bootstrap guide to
[settings plugins](/bootstrap/settings-plugins.html).

## Layout {#layout}

A settings plugin is a Lua plugin with two hooks:

```text
mise-desk/
├── metadata.lua
├── mise.plugin.toml
└── hooks/
    ├── settings_read.lua    # report current values
    └── settings_write.lua   # write changed values
```

mise treats a directory with `metadata.lua` and both `hooks/settings_read.lua`
and `hooks/settings_write.lua` as a settings plugin. A directory that also has
`hooks/backend_install.lua` is a backend plugin instead, so keep a settings
plugin in its own repository.

Link your working copy under the name users write in the section. mise runs the
plugin only for a section that names it, so declare one with a setting that is
safe to change, and turn on
[`experimental`](/configuration/settings.html#experimental):

```toml [mise.toml]
[settings]
experimental = true

[bootstrap.settings.desk.appearance]
blur = true
```

Pick a value that differs from the current one: mise calls `SettingsWrite` only
for keys that differ, so the dry run must list a change. Then apply and check
that the value stuck. Name the plugin so other settings sections are left
alone:

```sh
mise plugins link desk ./mise-desk
mise bootstrap settings status desk
mise bootstrap settings apply --dry-run desk
mise bootstrap settings apply desk
mise bootstrap settings status --missing desk
```

## mise.plugin.toml {#mise-plugin-toml}

`mise.plugin.toml` is optional. Its `[bootstrap-settings]` table describes the
plugin's needs; it does not decide the plugin's type, which comes from its
hooks. The table accepts the same keys as
[`[package-manager]`](/package-plugin-development.html#mise-plugin-toml) except
`supports_version_pins`:

```toml [mise.plugin.toml]
[bootstrap-settings]
requires = ["hyprctl"]  # programs the hooks run
os = ["linux"]          # platforms the plugin runs on
```

## Keys {#keys}

mise flattens the section into dotted keys. `[bootstrap.settings.desk.appearance]`
with `blur = true` becomes the key `appearance.blur` with the value `true`. A
list stays one value.

## SettingsRead {#settingsread}

`ctx.keys` holds the keys mise asks about: every declared key, except after a
write, when it holds only the keys that were written. Return the current value
of each one in `ctx.keys` you know, in `values`. Leave a key out when it is
unset.

```lua [hooks/settings_read.lua]
function PLUGIN:SettingsRead(ctx)
  return { values = { ["appearance.blur"] = false, ["appearance.blur_strength"] = 2 } }
end
```

mise calls this hook for status, dry runs, and before and after every write.
Keep it free of side effects and prompts.

## SettingsWrite {#settingswrite}

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
`"4"` differ. Lua holds integers exactly only up to 2^53, so mise refuses to
write a larger one.

## Rules {#rules}

- Never run `sudo`. A settings plugin changes the current user's settings.
- Change only the keys in `ctx.changes`. A key that is not declared is not a
  request to reset it.
- List every program the hooks run in `requires`.

The hooks can use the `cmd`, `json`, `file`, and other modules in the
[Plugin Lua reference](/plugin-lua-modules.html).

## Shipping with a distro {#system-plugins}

A distro or app can install the plugin in
[`/usr/share/mise/plugins`](/directories.html#system-plugins), so every user on
the machine can use it without installing anything. To publish it for users to
install themselves, see [publishing plugins](/plugin-publishing.html).
