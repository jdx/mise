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

These commands do not install plugins. Run `mise bootstrap plugins apply` first,
or include `plugins` with `--only`, so a plugin declared in
[`[bootstrap.plugins]`](/bootstrap/packages/plugins.html) is there to apply its
section.

A dry run prints each value that would change, with its current value:

```text
mise would set desk: appearance.blur false -> true
mise would set desk: appearance.blur_strength 2 -> 4
```

`mise bootstrap settings status --missing` exits with status 1 when a value
differs from the config, for example after someone changes it in the program's
own settings, or when a plugin cannot run. A section for a plugin made for
another OS is listed as skipped but does not count, so one config can hold
every platform's settings. `mise bootstrap status` and `mise bootstrap plan` do not check
settings yet, so use this command to detect that drift.

`[bootstrap.settings.macos]` is built in rather than a plugin: it holds
[macOS preferences](/bootstrap/macos-defaults.html), which the
`macos-defaults` part applies. No plugin can be named `macos`.

Sections merge across config files key by key, and a more local file's value
wins, as for the rest of `[bootstrap]`. `desk_plugin` and `desk-plugin` name
the same plugin, so their sections merge too. `appearance = { blur = true }` and
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

To write a settings plugin, see the
[settings plugin guide](/settings-plugin-development.html).
