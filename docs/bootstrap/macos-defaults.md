# macOS Defaults <Badge type="warning" text="experimental" />

mise can declare macOS user defaults (preferences) in the
`[bootstrap.macos.defaults]` section of `mise.toml` and apply them with
`mise bootstrap macos-defaults apply`:

```toml
[bootstrap.macos.defaults]
NSGlobalDomain = { KeyRepeat = 2, InitialKeyRepeat = 15, ApplePressAndHoldEnabled = false }
"com.apple.dock" = { autohide = true, tilesize = 48, orientation = "left" }
"com.apple.finder" = { ShowPathbar = true, AppleShowAllFiles = true }
```

Each key under `[bootstrap.macos.defaults]` is a preferences domain. Quote
domains containing dots. Values map to the matching `defaults write` type:

| TOML value | written as         | example                |
| ---------- | ------------------ | ---------------------- |
| boolean    | `-bool true/false` | `autohide = true`      |
| integer    | `-int <n>`         | `tilesize = 48`        |
| float      | `-float <n>`       | `scale = 1.5`          |
| string     | `-string <s>`      | `orientation = "left"` |

Other plist shapes (arrays, dicts, dates, data) are not supported; entries
using them parse fine but are skipped with a warning, so configs written for
newer mise versions still work.

## Semantics

`[bootstrap.macos.defaults]` follows the same rules as
[`[bootstrap.packages]`](/bootstrap/packages/):

- **Declarative and additive** — (domain, key) pairs merge across the
  [config hierarchy](/configuration.html) (global → project) as a union; a
  more local config overrides the value of a pair the global config declared
  but cannot remove it. mise never deletes a default.
- **OS-filtered** — on anything other than macOS the section is inert:
  `mise bootstrap macos-defaults status` and `mise doctor` list the entries
  as skipped (so nothing is silently invisible) and
  `mise bootstrap macos-defaults apply` ignores them, so a shared config
  authored for both Linux and macOS just works.
- **Manual application only** — mise never writes defaults implicitly; only
  `mise bootstrap macos-defaults apply` does, after the usual confirmation
  prompt.
- **Strictly typed** — an existing value only counts as in sync when both
  the value and the plist type match: an integer `1` does not satisfy a
  configured `true`. `mise bootstrap macos-defaults apply` converges it to the
  typed value.

User defaults are per-user, so unlike system packages no sudo is ever
involved. Host-scoped preferences (`defaults -currentHost`) and `sudo
defaults` system domains are not supported.

## Commands

```sh
mise bootstrap macos-defaults status            # shows defaults drift
mise bootstrap macos-defaults status --missing  # exit 1 if anything is unset or differs

mise bootstrap macos-defaults apply           # writes unset/differing defaults
mise bootstrap macos-defaults apply --dry-run # print the `defaults write` commands
mise bootstrap macos-defaults apply --yes     # skip the confirmation prompt
```

`mise bootstrap macos-defaults status` reports each entry as `set` (matches),
`differs` (a value exists but doesn't match — the current value is shown), or
`unset`. `mise doctor` summarizes the same drift.

## App restarts

Some applications only pick up changed defaults after a relaunch — mise
prints a reminder after writing. The usual suspects:

```sh
killall Dock
killall Finder
killall SystemUIServer
```

mise deliberately does not kill applications itself.

## Finding keys

To discover a setting's domain and key, change it in System Settings and
diff the output of `defaults read` before and after, or read a domain
directly:

```sh
defaults read com.apple.dock
defaults read-type com.apple.dock tilesize
```
