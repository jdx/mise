---
description: "Set macOS user preferences, such as Dock, Finder, keyboard, and trackpad settings, from mise.toml as the defaults command would."
socialDescription: "Set macOS user preferences from mise.toml as the defaults command would."
---

# macOS defaults

Declare macOS preferences in `mise.toml`, and
[`mise bootstrap`](/bootstrap.html), or
[`mise bootstrap macos defaults apply`](/cli/bootstrap/macos/defaults/apply.html),
writes them the way the `defaults` command does. Common Dock, Finder, keyboard,
and trackpad settings have named keys; anything else goes in
`[bootstrap.macos.defaults]` by domain and key.

## Example

```toml
[bootstrap.macos.dock]
autohide = true
orientation = "left"
tilesize = 48
show_recents = false

[bootstrap.macos.finder]
show_all_files = true
show_pathbar = true
preferred_view_style = "list"

[bootstrap.macos.keyboard]
key_repeat = 2
initial_key_repeat = 15
press_and_hold = false

[bootstrap.macos.trackpad]
tap_to_click = true

[bootstrap.macos.defaults]
"com.apple.screencapture" = { type = "png", "disable-shadow" = true }
```

Apply as the user whose preferences should change:

```sh
mise bootstrap macos defaults apply --dry-run
mise bootstrap macos defaults apply
```

mise writes each preference that is missing or different and leaves the rest
alone. Dock, Finder, and the menu bar read many preferences only when they
start, so [restart them](#app-restarts) to see the change.

## Named settings {#friendly-sections}

Each named key writes one preference, or two for the trackpad keys. An unknown
key, a value of the wrong type, or a value outside the listed choices is
ignored with a warning.

`[bootstrap.macos.dock]` writes to the `com.apple.dock` domain:

| Key                      | Preference key           | Value                                                                      |
| ------------------------ | ------------------------ | -------------------------------------------------------------------------- |
| `autohide`               | `autohide`               | Boolean: hide the Dock until the pointer reaches it                        |
| `autohide_delay`         | `autohide-delay`         | Number of seconds before a hidden Dock appears, such as `0`                |
| `autohide_time_modifier` | `autohide-time-modifier` | Number of seconds the show and hide animation takes, such as `0.5`         |
| `orientation`            | `orientation`            | `"bottom"`, `"left"`, or `"right"`                                         |
| `tilesize`               | `tilesize`               | Integer icon size                                                          |
| `magnification`          | `magnification`          | Boolean: enlarge icons under the pointer                                   |
| `largesize`              | `largesize`              | Integer size of enlarged icons                                             |
| `show_recents`           | `show-recents`           | Boolean: show recent apps in the Dock                                      |
| `mru_spaces`             | `mru-spaces`             | Boolean: rearrange Spaces by most recent use                               |
| `apps`                   | `persistent-apps`        | List of application paths; see [Pin Dock applications](#dock-applications) |

`[bootstrap.macos.finder]` writes to `com.apple.finder`, except one key:

| Key                           | Domain             | Preference key                      | Value                                                          |
| ----------------------------- | ------------------ | ----------------------------------- | -------------------------------------------------------------- |
| `show_all_files`              | `com.apple.finder` | `AppleShowAllFiles`                 | Boolean: show hidden files                                     |
| `show_pathbar`                | `com.apple.finder` | `ShowPathbar`                       | Boolean: show the path bar                                     |
| `show_status_bar`             | `com.apple.finder` | `ShowStatusBar`                     | Boolean: show the status bar                                   |
| `show_extensions_warning`     | `com.apple.finder` | `FXEnableExtensionChangeWarning`    | Boolean: warn before changing a file extension                 |
| `sort_folders_first`          | `com.apple.finder` | `_FXSortFoldersFirst`               | Boolean: keep folders on top when sorting by name              |
| `preferred_view_style`        | `com.apple.finder` | `FXPreferredViewStyle`              | `"icon"`, `"list"`, `"column"`, or `"gallery"`                 |
| `save_new_documents_to_cloud` | `NSGlobalDomain`   | `NSDocumentSaveNewDocumentsToCloud` | Boolean: save new documents to iCloud by default, in every app |

`[bootstrap.macos.keyboard]` writes to `NSGlobalDomain`:

| Key                             | Preference key                         | Value                                                           |
| ------------------------------- | -------------------------------------- | --------------------------------------------------------------- |
| `key_repeat`                    | `KeyRepeat`                            | Integer: interval between repeated keys; lower is faster        |
| `initial_key_repeat`            | `InitialKeyRepeat`                     | Integer: delay before a held key repeats; lower is shorter      |
| `press_and_hold`                | `ApplePressAndHoldEnabled`             | Boolean: `false` repeats a held key instead of offering accents |
| `automatic_capitalization`      | `NSAutomaticCapitalizationEnabled`     | Boolean                                                         |
| `automatic_spelling_correction` | `NSAutomaticSpellingCorrectionEnabled` | Boolean                                                         |
| `fn_state`                      | `com.apple.keyboard.fnState`           | Boolean: `true` makes F1 to F12 act as standard function keys   |

`[bootstrap.macos.trackpad]` writes each key to both
`com.apple.AppleMultitouchTrackpad` and
`com.apple.driver.AppleBluetoothMultitouch.trackpad`, for built-in and
Bluetooth trackpads:

| Key                 | Preference key            | Value                            |
| ------------------- | ------------------------- | -------------------------------- |
| `tap_to_click`      | `Clicking`                | Boolean: tap to click            |
| `three_finger_drag` | `TrackpadThreeFingerDrag` | Boolean: drag with three fingers |

## Pin Dock applications {#dock-applications}

`apps` declares the applications pinned in the Dock, in order:

```toml
[bootstrap.macos.dock]
apps = [
  "/System/Applications/Utilities/Terminal.app",
  "/Applications/Firefox.app",
  "~/Applications/Example.app",
]
```

Paths must be absolute or start with `~/`, end in `.app`, and contain no `..`.
Duplicates, including symlinks to the same application, are rejected. Every
application must exist before mise applies a changed layout. Leave out `apps` to
leave the layout alone; `apps = []` removes every application tile mise
recognizes.

mise adds, removes, and reorders the applications in `com.apple.dock`'s
`persistent-apps` preference, and keeps each existing tile's bookmark and
metadata. It resolves symlinks to match applications, and new tiles keep the
path you declared. Other kinds of tiles and tiles mise does not recognize stay
as they are, and `persistent-others`, which holds the Dock's folders and
files, is not touched. Running apps that are not pinned, and the recent apps
section, are outside this list.

Status compares paths and order, so metadata that Dock adds does not count as
a change. Moving a pinned app by hand does, and the next apply restores the
declared order. mise has no command to capture your current Dock into the
config.

## Raw defaults

Each key under `[bootstrap.macos.defaults]` is a preferences domain, and each
key inside it a preference. Quote a domain that contains dots. TOML values map
to property list types:

| TOML value | Property list type | Example                        |
| ---------- | ------------------ | ------------------------------ |
| boolean    | boolean            | `autohide = true`              |
| integer    | integer            | `tilesize = 48`                |
| float      | real               | `scale = 1.5`                  |
| string     | string             | `orientation = "left"`         |
| array      | array              | `favorite-spaces = [1, 2, 3]`  |
| table      | dictionary         | `options = { enabled = true }` |

Arrays and tables are converted recursively, so nested values keep their types.
For example, a Dock entry can be an array of dictionaries:

```toml
[bootstrap.macos.defaults."com.apple.dock"]
"persistent-apps" = [
  { "tile-type" = "file-tile", "tile-data" = { "file-label" = "Terminal" } },
]
```

A declared value replaces the whole preference; mise does not merge arrays or
dictionaries element by element. To change one value inside a dictionary, see
[Change one value in a dictionary](#targeted-dictionary-updates). TOML dates
and times are skipped with a warning, and binary data has no TOML type, so
neither is supported.

### Find a preference's domain and key {#finding-keys}

Change the setting in System Settings and compare the output of
`defaults read` before and after, or read a domain directly:

```sh
defaults read com.apple.dock
defaults read-type com.apple.dock tilesize
```

## Explicit entries

Use `[[bootstrap.macos.defaults_entries]]` when a preference needs a host scope
or a path inside a dictionary. Each entry takes:

| Field    | Value                                                                                  |
| -------- | -------------------------------------------------------------------------------------- |
| `domain` | Preferences domain, such as `"NSGlobalDomain"`; `-g` and `-globalDomain` mean the same |
| `key`    | Preference key                                                                         |
| `value`  | Any type in the table above                                                            |
| `host`   | `"any"` (the default) or `"current"`                                                   |
| `path`   | List of dictionary keys inside the preference                                          |

### Current-host preferences

Preferences you would write with `defaults -currentHost` need
`host = "current"`:

```toml
[[bootstrap.macos.defaults_entries]]
domain = "NSGlobalDomain"
key = "com.apple.mouse.tapBehavior"
host = "current"
value = 1
```

mise then reads, writes, and synchronizes the preference for this machine
only. The same domain and key can be managed separately in each scope. Status
labels these domains with `(current host)`, JSON entries include `host`, and
the dry run of a single boolean, number, or string value shows `-currentHost`.

### Change one value in a dictionary {#targeted-dictionary-updates}

Add `path` to change a value inside a shared dictionary without replacing the
values around it. This turns off one keyboard shortcut and keeps its other
settings and every other shortcut:

```toml
[[bootstrap.macos.defaults_entries]]
domain = "com.apple.symbolichotkeys"
key = "AppleSymbolicHotKeys"
path = ["64", "enabled"]
value = false
```

Each component of `path` is a literal dictionary key, so a dot in a component
is not a separator. A path needs at least one component, and array indexes are
not supported. `value` replaces the selected value in full, even when it is an
array or dictionary.

mise reads the existing preference in the selected host scope, changes the
selected value, and writes the result. Other values keep their types, including
data and dates. Missing dictionaries along the path are created; an existing
value along the path that is not a dictionary is an error. Status compares only
the selected value, and the dry run shows its path. Quit the app first if it
rewrites this preference while it runs.

Two entries for the same preference and host where one path contains the other,
or a path entry and a whole-value declaration for the same preference, are
rejected before anything is written.

## Sandboxed apps

Sandboxed apps such as Safari keep their preferences in
`~/Library/Containers/<domain>/Data/Library/Preferences/<domain>.plist`, not in
`~/Library/Preferences`. When `~/Library/Containers/<domain>` exists, mise
reads and writes the container's file, and current-host entries use the
container's `ByHost` folder. These are the same files `defaults` uses.

An app that has never been launched has no container yet, so launch it once
before you apply its preferences. macOS protects other apps' containers, so
your terminal may need Full Disk Access (System Settings > Privacy & Security)
before `apply` can write them.

## How configs combine {#semantics}

Preferences merge by domain, key, host, and path across the
[config hierarchy](/configuration.html#configuration-hierarchy). A more local
config can change a value that a less local one declared, but it cannot remove
it, and mise never deletes a preference.

Named settings and raw defaults write the same preferences. Within one config
file, a `[bootstrap.macos.defaults]` value wins over a named setting for the
same domain and key, and a `[[bootstrap.macos.defaults_entries]]` entry wins
over both. A raw `persistent-apps` value therefore replaces `apps` in the same
file and is compared as a whole value. Across files, the more local config wins
as usual, so a project's named setting overrides a raw default from your global
config.

A value counts as set only when its type matches too: an integer `1` does not
satisfy `true`, and apply rewrites it as a boolean. Preferences belong to your
user, so mise never uses `sudo` for them, and system-wide domains written with
`sudo defaults` are not supported.

## Restart affected apps {#app-restarts}

Dock, Finder, and the menu bar read many preferences only when they start, so a
stored value can match while the app still shows the old behavior. mise does
not restart them. It prints a reminder after it writes preferences, and
`mise bootstrap` repeats the reminder in its follow-up summary. Relaunch them
yourself:

```sh
killall Dock Finder SystemUIServer
```

To restart Dock on every bootstrap, add a [hook](/bootstrap.html#hooks). Hooks
run on every bootstrap that includes the defaults step, even when nothing
changed:

```toml
[bootstrap.hooks]
post-defaults = "killall Dock || true"
```

## Undo a preference

mise never deletes a preference, so removing a declaration leaves the current
value in place. To return a preference to the system default, remove it from
your config and delete it:

```sh
defaults delete com.apple.dock autohide
```

## Preview and apply

```sh
mise bootstrap macos defaults status            # state of each preference
mise bootstrap macos defaults status --json     # the same, as JSON
mise bootstrap macos defaults status --missing  # exit 1 if any preference is unset or differs
mise bootstrap macos defaults apply --dry-run   # print the planned writes
mise bootstrap macos defaults apply             # apply after a confirmation prompt
mise bootstrap macos defaults apply --yes       # apply without prompting
```

Status reports each preference as `set` (it matches), `differs` (a value exists
but does not match, and status shows it), or `unset`. `mise doctor` summarizes
the same drift.

## On Linux and Windows

The sections are ignored on other platforms, so one config can serve several
machines.
[`mise bootstrap macos defaults status`](/cli/bootstrap/macos/defaults/status.html)
and `mise doctor` list the entries as skipped, and `apply` does nothing.

## See also

- [Bootstrap](/bootstrap.html#how-it-runs) for where defaults fall in the run
  order.
- [macOS LaunchAgents](/bootstrap/launchd.html) for programs that run at login.
- [Homebrew casks](/bootstrap/packages/brew-cask.html) for the applications you
  pin in the Dock.
