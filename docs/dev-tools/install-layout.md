---
description: Name each tool installation by what it is, so shorthands share one install and variants of one version coexist.
socialDescription: Name each installation by what it is, so variants of one version coexist.
---

# Identity install layout <Badge type="warning" text="experimental" />

By default, mise installs a tool into `installs/<tool>/<version>`, the legacy
layout. That path cannot say which backend produced the files, which platform
build they are, or which install options were used. So `age` and
`aqua:FiloSottile/age` are two separate downloads, and two variants of one
version overwrite each other.

The identity layout gives each installation its own directory,
`installs/<label>-<hash>/`, named by what was installed. The familiar
`installs/<tool>/<version>` path stays, as a link to that directory.

::: warning Experimental
The identity layout needs `experimental = true` and
`install_layout = "identity"`; `experimental` alone does not turn it on.
Directory names, receipts, and the catalog format can change between releases
while the layout is experimental. Try it where reinstalling tools is cheap, and
read [Turn it off or downgrade](#downgrading-and-compatibility) first.
:::

## Quick start

Turn the layout on for a project in `mise.toml`:

```toml [mise.toml]
[settings]
experimental = true
install_layout = "identity"

[tools]
age = "1.2.1"
```

To turn it on everywhere, run `mise settings experimental=true` and
`mise settings install_layout=identity`, or export `MISE_EXPERIMENTAL=1` and
`MISE_INSTALL_LAYOUT=identity`. See
[`install_layout`](/configuration/settings.html#install_layout).

Install the tool and see where it went:

```sh
mise install
mise where age
# ~/.local/share/mise/installs/age-hlencrst
readlink ~/.local/share/mise/installs/age/1.2.1
# ../age-hlencrst
```

The directory name is a readable label followed by a short hash, so
`ls installs/` still says what each directory is. The hash includes the
platform, so yours differs from the example on another OS or architecture.

## What changes

With the identity layout, a new installation looks like this:

```text
installs/
  age/                          the tool directory, as before
    1.2.1 -> ../age-hlencrst    version link
    1.2 -> ./1.2.1              runtime alias
    1 -> ./1.2.1                runtime alias
    latest -> ./1.2.1           runtime alias
  age-hlencrst/                 the installation itself
    .mise-install.toml          receipt
    age/age                     the tool's files
  .mise/                        catalog
```

### Installation directories

Each installation lives in `installs/<label>-<hash>/`. The directory that holds
them is the install store: the installs directory itself, except on Windows,
where it is a shorter sibling directory (see [Windows](#windows)).

The label is the last part of the backend's project name, lowercased and cut to
24 characters: `aqua:FiloSottile/age` gives `age`, `aqua:yarnpkg/berry` gives
`berry`, and `go:github.com/oapi-codegen/oapi-codegen/v2/cmd/oapi-codegen` gives
`oapi-codegen`. It comes from the backend, not the registry shorthand, so a
registry change never moves an install.

The hash is the first eight characters of a base32 digest of the installation's
identity. If that name is already taken by a different identity, or by a
directory mise did not create, the new installation extends its hash by two
characters (ten, then twelve, and so on). mise never renames an existing
installation to make room.

### Receipts

Each installation directory holds a `.mise-install.toml` receipt that records
what the installation answers to. This one is trimmed:

```toml
digest = "hlencrstldyjkqnpak2zyr453avqxfjf6so4vopt7au5nsxvzska"
dir = "age-hlencrst"
requested_as = "age"

[identity]
mode = "fallback"
backend = "aqua:FiloSottile/age"
version = "1.2.1"
platform = "linux-x64"
```

Alongside the identity, the receipt keeps the digest of the artifact mise
acquired when the backend reports one, the spelling the tool was requested with,
and the mise version that wrote it. mise writes the receipt last, so a directory
without one is an incomplete installation. A receipt describes how the
installation was requested. It does not prove the files are still the original
bytes, and a receipt copied next to a cached tool does not make that tool
trustworthy.

The `mode` says how much the identity pins down. `fallback` identifies the
request: backend, version, platform, and options. `resolved` also names a pinned
artifact checksum from a lockfile (see
[Lockfiles and selections](#lockfiles-and-selections)). A `fallback`
installation cannot promise that installing the same request again produces the
same bytes.

### The catalog

`installs/.mise/` records which directory each identity was assigned and which
installation each unlocked request selected. It is durable metadata, not a
cache. When you copy or cache the installs directory, keep the catalog with it,
together with the install store when that is a separate directory (on Windows,
`i` beside `installs`).

### Version links and aliases

`installs/<tool>/<version>` links to the installation, so paths you wrote down,
such as an IDE SDK entry, keep working. A link can point at only one variant:
when two variants of one version exist, the link names the one installed most
recently. mise never uses the link to decide which variant a request means, so
a hardcoded `installs/<tool>/<version>` path sees only the most recent variant.
On Linux and macOS the link is relative (`../age-hlencrst`).

Runtime aliases such as `latest`, `1`, or `1.2` keep their current format in
the tool directory (`1 -> ./1.2.1`). They chain to the version link and from
there to the installation.

## What stays the same {#what-does-not-change}

- mise does not move or migrate `installs/<tool>/<version>` directories that
  exist when you turn the layout on. Relocating them would break paths embedded
  in shebangs, virtual environments, and package-manager installs. mise keeps
  using a legacy installation in place when the backend recorded for it matches
  the request; otherwise it installs the version again in the identity layout
  instead of reinterpreting another backend's files. To move them yourself, see
  [Moving legacy installations](#moving-legacy-installations).
- Both layouts can share one installs directory. mise tells hashed directories
  from tool directories by their receipts and catalog reservations, not by their
  names.
- mise does not reclaim legacy installations on its own. Nothing removes or
  relocates a legacy directory because a hashed counterpart exists, and `prune`
  treats legacy installations as before. Only `mise installs migrate` moves them.
- Tool options, backends, and the registry work as before; only where files land
  and how installations are found changes. `mise backends switch` installs the
  new backend's installation of each switched version, which is a different
  installation from the old backend's, and points the version link at it. The
  old installation stays until it is pruned.

## How identity is decided

An installation's identity is a hash of the request it answers, not of the files
on disk. Installed tools can modify themselves, add packages, or generate files,
and none of that renames the directory or invalidates the installation. The
identity covers:

- The canonical backend, never the shorthand. `age` and `aqua:FiloSottile/age`
  resolve to the same backend, so they share one installation, whichever you ask
  for first.
- The concrete version, treated as an opaque string.
- The platform, such as `linux-x64` or `linux-x64-musl`, so glibc and musl
  builds of one version coexist.
- Options that change what gets installed. The backend decides which of a
  request's options these are. For example, `matching` on a `github:` tool picks
  which release asset to install, so different values give different
  installations. Options that only affect how versions are listed or verified,
  or that apply only while installing, such as `prerelease`, `depends`,
  `minimum_release_age`, `version_order`, and `auto_update`, do not split
  installs. `install_env` can change what a build produces, so mise hashes it
  into the identity and never records its values. A `postinstall` hook runs
  after the install and does not split installs, so editing it never reinstalls
  a tool.
- Pinned inputs: the artifact checksum, when the request comes from a lockfile
  that pins one for this platform, and the dependency graph a lockfile records
  for `npm:` and `pypi:` installs that have one.

Because options are part of the identity, tools that share a backend and a
version but select different release assets get separate installations. The
registry entries `restate-server` and `restatectl` both use
`github:restatedev/restate` and differ only in their `matching` option. With
this configuration, mise creates two directories that share a label, and each
tool directory links to its own:

```toml [mise.toml]
[tools]
restate-server = "1.4.0"
restatectl = "1.4.0"
```

```text
installs/
  restate-hof7qzg3/                    restatectl 1.4.0
  restate-kcyl6hcz/                    restate-server 1.4.0
  restatectl/1.4.0 -> ../restate-hof7qzg3
  restate-server/1.4.0 -> ../restate-kcyl6hcz
```

Neither counts as satisfying the other, and installing one never replaces the
other. The same applies to any two requests that differ in an install-affecting
option.

## Finding an installation: `mise where` and `mise which`

- `mise where age` prints the installation directory selected for the request,
  such as `~/.local/share/mise/installs/age-hlencrst`. It is the real directory,
  never a version link, so it is correct even when a link points at another
  variant.
- `mise which age` prints the executable. For an unlocked request it goes
  through the version link, such as
  `~/.local/share/mise/installs/age/1.2.1/age/age`, when that link names the
  selected installation, and through the installation directory otherwise.

`mise activate`, `mise env`, and `mise exec` follow the same rule for `PATH`. An
unlocked request puts the link on `PATH` (a version link such as
`installs/age/1.2.1`, or an alias such as `installs/node/20`) when that link
resolves to the installation the request selects, so with `node = "20"`,
activation puts `installs/node/20/bin` on `PATH` as before. If another variant
of the version was installed later and the link now points at it, they use the
selected installation's own directory. A request that comes from a lockfile
entry always uses the installation directory.

Use these commands, or `mise exec`, instead of building a path by hand. A hashed
name does not contain the version; read the version from the version link, the
receipt, or `mise ls`.

## Lockfiles and selections

Without a lockfile entry, `node = "20"` or `age = "latest"` first resolves to a
concrete version. mise then remembers which installation satisfied that
concrete request. That selection is shared by every project on the machine that
asks for the same backend, version, platform, and options. It is not tied to a
project directory or to the shorthand spelling.

The selection is sticky. Running tools, activating, and installing again reuse
the remembered installation, and mise does not look upstream for a re-released
artifact. `mise install --force`, or updating a rolling version such as a
`nightly` tag, reinstalls into the same directory, so every project that makes
the same request sees the refreshed files. A different version or different
options is a different request: upgrading one project to a new version does not
change what other projects pinned to the old version select.

When a [lockfile](/dev-tools/mise-lock.html) pins an artifact checksum for your
platform, the checksum becomes part of the identity. mise looks for an
installation with that identity. It also adopts an unlocked installation of the
same version when the checksum recorded for the artifact it acquired is the
pinned one, without downloading or reinstalling anything, so running
`mise install --locked` after an unlocked install of the same artifact reuses
it. Otherwise mise creates a separate installation, and different pinned
artifacts of one version coexist.

A pin that adopted an unlocked installation keeps it. If you later force a
refresh of the unlocked request (`mise install --force age@1.2.1`, run outside
that lockfile's project), mise installs into a new directory and moves the
shared selection to it instead of replacing the files the lockfile adopted.

A lockfile entry with no checksum for your platform does not pin anything; its
request uses the same installation an unlocked request would. A locked install
never changes an unlocked selection, so installing or updating one project does
not retarget what another project's unlocked request selects.

Unlocked selections are local to your machine. To carry a particular choice to
other machines or teammates, commit a lockfile.

## Choosing an installation: `mise installs`

Several installations can answer one unlocked request: a forced refresh that
could not replace a lockfile's installation in place, or installations that
lockfiles in other projects made. [`mise installs ls`](/cli/installs/ls.html)
lists every installation and says which one each request uses:

```sh
mise installs ls jq
# Installation  Tool  Version  Platform   Status
# jq-hm3qa4vb   jq    1.7.1    linux-x64  selected
# jq-ezjqmxa4   jq    1.7.1    linux-x64  pinned
```

`selected` is the installation that requests without a lockfile use, `pinned`
means a lockfile adopted it, and `shared` means it is in a read-only shared
installs directory. Add `--json` for the full identity, including options and
the artifact checksum.

[`mise installs select`](/cli/installs/select.html) makes another installation
the selected one and points the version link (`installs/jq/1.7.1`) at it:

```sh
mise installs select jq-ezjqmxa4
```

The selection applies to every project on the machine that asks for the same
tool, version, platform, and options without a lockfile. Projects whose lockfile
pins an artifact keep that artifact's installation. To select an installation in
a shared installs directory, pass its path; the selection is kept in your own
installs directory, and nothing is written to the shared one.

The first unlocked install of a request selects the installation it made. If
the selection is lost (the catalog was rebuilt from receipts, for example), or
the request was only ever installed by lockfiles, mise looks at the
installations that answer it. With exactly one, mise uses and selects it. With
several, mise stops instead of guessing, and lists them:

```text
mise ERROR jq@1.7.1 matches several installations and none is selected:
  jq-hm3qa4vb
  jq-ezjqmxa4 (a lockfile pins it)
Choose one with `mise installs select <dir>`, or install a fresh one with `mise install --force jq@1.7.1`
```

If the selected installation has since been pruned, installing restores it in
the same directory instead of choosing another one.

## Moving legacy installations

[`mise installs migrate`](/cli/installs/migrate.html) moves installations made
before the layout was turned on into it. It does not copy files: it reinstalls
each version from its backend into its own `<label>-<hash>` directory, so paths
the tool records about itself are written for the new location. Then it removes
the old `installs/<tool>/<version>` directory and puts the version link in its
place, so a path that pointed into the old directory, such as a virtual
environment's interpreter, still resolves.

```sh
mise installs migrate --dry-run   # list what would move
mise installs migrate             # move every legacy installation
mise installs migrate node python@3.12.1
```

The old directory is moved aside while its replacement installs, and put back
if the install fails.

A version that cannot be reinstalled is not an error. That happens when its
release was withdrawn or is signed by a different identity than mise accepted
before, when the network is unavailable, or when its installer fails. mise then
moves the existing directory, as it is, into its own `<label>-<hash>`
directory, writes the same receipt an install would, and leaves the version
link at the old path. Paths the tool recorded about itself, such as virtual
environment shebangs and `node_modules/.bin` links, keep resolving through that
link. Nothing in the directory is rewritten, and the identity holds only what
the old directory can say: the backend, version, platform and the options of
the request. A digest that comes from a lockfile, such as an artifact checksum
or the dependency graph of an `npm:` or `pipx:` install, is not reconstructed.

```text
relocated aube@2.2.4 to ~/.local/share/mise/installs/aube-4h2kfq7a
  aube@2.2.4 could not be reinstalled (github.com/aubepkg/aube has no release 2.2.4 ...); moved as it is
170 migrated, 12 relocated, 0 kept legacy, 0 failed
```

If it cannot be moved either, for example because it is on a different file
system than the install store, it stays in the legacy layout, untouched, and
still works:

```text
skipped aube@2.2.4 (kept legacy layout): <why it was not reinstalled>; not moved either: <why>
```

A later `mise installs migrate` tries it again. The command exits non-zero only
when a migration itself broke, for example when the old directory could not be
put back. A move is recorded like a reinstall, so the next run puts back an
interrupted one.

Each migration is recorded in `installs/.mise/migrations/` before anything
moves, so if a run is interrupted, the next
`mise installs migrate` either removes the old directory (the replacement had
finished) or puts it back and withdraws the unfinished replacement. Run it while
nothing is using the tools being moved.

Versions whose recorded backend is not the one their tool resolves to now are
left alone; mise is not using them, so `mise uninstall` them if nothing needs
them. Tools that keep the legacy layout are also left alone (see
[Known limits](#known-limits)).

## Pruning and uninstalling

Both work on one installation directory at a time.

- `mise uninstall age@1.2.1` removes the installation directory, and the version
  links that name it from every tool directory that has one. It does not follow
  a link to decide what to delete, and it refuses to remove a path directly under
  the install store that has neither a receipt nor a reservation in the catalog.
  Removing one variant leaves the others in place. When more than one
  installation of that version exists, it stops and asks for `--all`.
- Removing an installation keeps its catalog record, so installing the same
  identity again lands in the same directory. `mise uninstall` also forgets the
  selection that named it, so the requests it answered choose again; a pruned
  installation keeps its selection and is restored in place.
- `mise prune` keeps an installation while a tracked config needs it. An
  unlocked request needs the installation it selects. A tracked lockfile entry
  needs the installations of its backend and version, narrowed to the pinned
  artifact when the entry has a checksum for your platform. Legacy installations
  are pruned as before.
- `mise plugins uninstall --purge` also removes the plugin's installations in
  the identity layout.
- `mise ls` and `mise prune` list each installation separately. When several
  installations share a version, `mise ls` shows the directory of each one
  (`1.2.1 [age-hlencrst]`), and `mise prune` removes only the ones nothing
  needs. Directories with a receipt, and `.mise`, are never treated as installed
  tools.

### Templated versions and `mise prune`

A tool version written as a template, such as
<span v-pre>`node = "{{ vars.node }}"`</span>, depends on the vars, env,
`MISE_ENV`, `--no-env`, settings, and dotenv files where the project runs.
`mise prune` cannot reproduce that from another directory, so whenever a command
resolves all of a config's tools, mise saves what its templated versions
rendered to under `installs/.mise/snapshots/`. A template in a tool's options
makes the tool templated too.

- `mise prune`, `mise ls --prunable`, and the cleanup after `mise upgrade` read
  the snapshot instead of rendering templates. A command that resolves fewer
  tools than the config sets, such as `mise exec node@22`, records nothing.
- A config with no snapshot, or whose files changed since the snapshot, keeps
  every installation of its templated tools until you run a mise command in that
  project again. A change outside the config files, such as a shell variable, is
  noticed at the next such command, not before.
- A snapshot covers one context: the `MISE_ENV` and the set of loaded config
  files. A newer snapshot of the same context replaces it, so a version the
  project stopped using becomes prunable. A snapshot stores the requested
  version and the version the command settled on, so project aliases are
  followed.
- Snapshots are written with owner-only permissions. Tools with backend options
  or `install_env` are not snapshotted, because those can hold credentials under
  any name; prune keeps every installation of them.
- A snapshot whose config file is gone is ignored, and `mise prune --configs`
  removes it.

To keep a version that a snapshot no longer lists, reference it in a tracked
config or lockfile. With the legacy layout, `mise prune` renders these versions
from where it runs and can fail.

## Windows

The identity layout works the same way on Windows, with these differences:

- The version link is a directory junction with an absolute target, which IDE
  SDK selectors and other programs can follow. mise never substitutes a text
  file for a link. Junctions do not need administrator rights. A directory on a
  UNC path gets a symlink, which can need administrator rights or Developer
  Mode.
- If mise cannot create the link, the installation still succeeds and works
  through mise, and mise warns that the link is unavailable. Run `mise where` to
  get the real directory.
- Because junction targets are absolute, an installs directory copied to another
  location keeps pointing at the old one. mise finds installations through the
  catalog, relative to the installs directory it is using, so this affects
  programs that follow the link, not mise.
- Installations go into `%LOCALAPPDATA%\mise\i\` instead of `installs\`, seven
  characters shorter, because the real path is where installers extract files
  and what counts toward the 260-character limit. The version links, runtime
  aliases, and catalog stay in `installs\`, so `installs\java\21` keeps working
  in an IDE:

  ```text
  %LOCALAPPDATA%\mise\
    installs\
      jq\1.7.1 -> %LOCALAPPDATA%\mise\i\jq-ezjqmxa4   junction
      .mise\                                           catalog
    i\
      jq-ezjqmxa4\                                     the installation
  ```

  Setting `MISE_INSTALLS_DIR` keeps installations in that directory, as on other
  platforms. `MISE_INSTALL_STORE_DIR` chooses where installations go, on any
  platform, without moving the links.

- An installation's directory name has a bounded length: a label of at most 24
  characters, a dash, and eight hash characters, longer only after a collision.
  It sits directly under the install store, whatever the version string looks
  like. The data directory and the paths inside tools still count toward the
  limit, so a short `MISE_INSTALL_STORE_DIR` (such as `C:\m`) is the main lever
  left.

## Turn it off or downgrade {#downgrading-and-compatibility}

To turn the identity layout off, remove `install_layout = "identity"`. mise then
installs new versions into `installs/<tool>/<version>` again and leaves hashed
directories where they are. While it is off:

- `mise where` and `mise exec` still reach a hashed installation through its
  version link, and `mise ls` shows it as a symlinked version.
- `mise uninstall` removes only that link and leaves the hashed directory.
- Turning the layout back on finds the installation again through the catalog.

Older mise versions do not read receipts or the catalog. Like a current mise with
the layout off, they see an installation only through its version link and
runtime aliases. A link can point at only one variant, so when a version has
several variants an older mise cannot guarantee that it picks the one your
configuration means. If you need to go back to an older mise, expect to reinstall
tools that have more than one variant of a version.

## Known limits

- Some installs keep the legacy layout. `http:` installs (which link into a
  shared extraction cache), `rust`, and `dotnet` are not given hashed
  directories. Neither are `mise install --system`, `--shared`, and
  `mise install-into`, whose destinations are explicit, nor versions you
  `mise link` or reference with `path:`. mise still finds legacy installs in
  system and shared installs directories and never writes to them as part of
  this layout.
- Files that tools generate while installing, such as virtual environments,
  shebangs, and package-manager installs, record the real installation path,
  which contains the hash. They stay valid for that installation but do not
  carry over to another one. Installing the same identity again lands in the
  same directory.
- Versions named on the command line carry only the options a config gives
  them. In a project, `mise where tool@1.0`, `mise exec tool@1.0`, and similar
  commands use the install options the project config sets for that tool, such
  as `matching`. Outside a project, a bare version uses its only installation
  regardless of options. If there are several, `mise where` lists them and other
  commands treat the version as not installed. Name the options, as in
  `mise where 'tool[matching=server]@1.0'`, or run
  `mise install --force tool@1.0`.
- Legacy installs, hashed installs, and multiple variants of one version each
  take their own disk space until you uninstall them.
