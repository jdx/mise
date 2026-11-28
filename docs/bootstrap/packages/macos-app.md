---
description: "Install a macOS app from a vendor download or a Homebrew cask, verified by its checksum, without writing to the Homebrew prefix."
---

# macOS apps without Homebrew (macos-app)

The `macos-app` manager installs an app bundle into `/Applications` and keeps
its install records in mise's state directory. It never writes to the Homebrew
prefix, and needs no `brew` binary.

The download comes from one of two places:

- [an official Homebrew cask](#install-from-a-homebrew-cask), which supplies
  the current version, URL, and checksum;
- [a download you declare](#declare-a-download), for a vendor or internal app
  that has no cask.

Prefer [`brew-cask`](/bootstrap/packages/brew-cask.html) when you want
Homebrew to own the app. Use `macos-app` with a cask when the Homebrew prefix
belongs to another user, such as a second account on a shared Mac.

## Install from a Homebrew cask

Set `cask` to an official cask token:

```toml
[bootstrap.packages]
"macos-app:figma" = { cask = "figma", os = "macos" }
```

mise reads the cask's current version, download, and checksum from Homebrew's
cask index, verifies the download, and installs only the cask's app bundle. The
cask must:

- be an official cask; tap casks such as `owner/tap/name` are rejected;
- install only app bundles, with no pkg installers, command-line binaries,
  fonts, or install scripts;
- publish a checksum for its download, and depend on no other cask or formula.

mise refuses a cask that does not meet these rules, and names the reason; install
that one with `brew-cask` instead. Homebrew publishes only a cask's current
version, so `version` cannot be set with `cask`, and neither can `url`,
`sha256`, or `artifact`.

Updates work as they do for `brew-cask`. `mise bootstrap packages apply`
installs the cask's current version when the app is missing, and leaves an
installed app alone. `mise bootstrap packages upgrade` installs the cask's
current version when it differs from the installed one; an app that the cask
marks as updating itself is replaced only when the installed bundle is older
than the cask and not running. Uninstall and zap steps in the cask are
not run; see [Remove an app](#remove-an-app).

## Declare a download

Add a table with all four required fields. This example uses a placeholder URL
and checksum; replace both with the values for your app:

```toml
[bootstrap.packages."macos-app:example"]
version = "1.2.3"
url = "https://example.com/Example-{{version}}-arm64.dmg"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
artifact = "Example.app"
os = "macos/arm64"
```

| Field      | Meaning                                                                                                                                        |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `version`  | The release to install. `"latest"` is not accepted.                                                                                            |
| `url`      | The archive to download. <code v-pre>{{version}}</code> is replaced with `version`. A `.git` URL is rejected, because a clone has no checksum. |
| `sha256`   | The archive's SHA-256 checksum, as 64 hexadecimal characters. Homebrew's `no_check` is not accepted.                                           |
| `artifact` | The app bundle to install from the archive, such as `Example.app`.                                                                             |

The archive can be a `.dmg`, `.zip`, or tar archive containing the app bundle.
`macos-app` does not install pkg installers, command-line binaries, or fonts.

Choose the download for your Mac's architecture. The optional `os` selector in
the example limits the entry to Apple Silicon. mise warns about a URL that is
not HTTPS, and still verifies its checksum.

If an entry is missing a field or has an invalid one, mise warns and skips it,
on every platform, so a shared config reports the mistake wherever it is read.

## Install

Preview and install the declared app:

```sh
mise bootstrap packages apply macos-app:example --dry-run
mise bootstrap packages apply macos-app:example
```

mise downloads the archive, checks its checksum, and installs the app into
`/Applications`. To use another directory, set
[`MISE_BREW_CASK_OPT_APPDIR`](/bootstrap/packages/brew-cask.html#overriding-the-application-directory).
mise keeps its install records in its state directory, separate from Homebrew's
Caskroom.

Naming `macos-app:<name>` on the command line installs the declaration from your
config; the declaration must exist.

## Adopt an existing app

If the destination already holds an app that this entry did not install, mise
refuses to install over it. To take over an identical app without replacing its
bundle, add `adopt = true` to the declaration:

```toml
[bootstrap.packages."macos-app:example"]
version = "1.2.3"
url = "https://example.com/Example-{{version}}-arm64.dmg"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
artifact = "Example.app"
adopt = true
```

mise compares the installed bundle with the download. If they differ, adoption
fails and leaves the existing app untouched; remove the app first to install a
different build. Keeping the bundle in place avoids the
[loss of privacy permissions](/bootstrap/packages/brew-cask.html#macos-privacy-security-tcc)
that a replacement can cause. The `[bootstrap.brew] adopt` default does not
apply to `macos-app`.

After adoption, mise treats the app as its own and replaces it when you update
the declaration. If Homebrew or another tool also manages the app, stop
managing it there first, or both overwrite the same bundle.

An interrupted install that left an app without a completed record also needs
`adopt = true`. Changing `artifact` or the app directory makes mise check the
new location the same way. An app that appears at the destination while mise
is preparing its own copy is also refused and left untouched:

```text
macos-app: '/Applications/Example.app' was created by something else while this
app was being staged; it was left untouched
```

A dry run warns about an existing app it does not own, but cannot tell whether
adoption succeeds, because it has not downloaded the archive to compare.

## Update a declared download {#update-a-declared-app}

For each new release, change `version` and `sha256`, and change `url` if it
does not use <code v-pre>{{version}}</code> or the vendor changed its URL
format. Then run:

```sh
mise bootstrap packages apply macos-app:example
```

`mise bootstrap packages upgrade` also installs the newly declared version when
the app is already installed. With an unchanged declaration and a matching
installed version, there is nothing to update.

## Remove an app

`macos-app` does not support `state = "absent"` or `prune`. Deleting the entry
leaves the app installed; delete the app bundle yourself.
