---
description: "Install a macOS app from a vendor download URL, verified by its checksum, when no cask exists."
---

# Direct macOS app downloads (macos-app)

The `macos-app` manager installs an app bundle from a download you declare, for
a vendor or internal app that has no Homebrew cask. You give the URL, checksum,
and version yourself, and mise verifies the download before installing it into
`/Applications`.

Prefer [`brew-cask`](/bootstrap/packages/brew-cask.html) when a suitable cask
exists: the cask supplies the download details and tracks new releases for
you. `macos-app` cannot discover releases, so you update each declaration by
hand.

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

## Update a declared app

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
