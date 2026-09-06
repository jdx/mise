# Plugin Publishing

Publish a plugin as a Git repository or release archive, with a tested installation command
and documentation for its actual interface. Users can install it directly by URL; adding a
mise registry shorthand is a separate process, and new asdf/vfox tool entries are not accepted.

This guide applies to Lua tool, backend, environment, and package plugins. Choose the
[plugin type](/plugins.html) before adapting a test or release workflow.

## Publishing Checklist

### Essential Files

Include `metadata.lua`, the hook files for your interface, and a README with installation,
configuration, prerequisites, supported platforms, and a working verification command.
Keep credentials and machine-specific configuration out of the repository.

### Optional but Recommended

Add a license, automated tests, a changelog or release notes, and formatting/lint tasks.
Document the minimum tested mise version, particularly for mise-specific hooks and modules.
Plugin metadata's version describes the plugin release, not the version of a managed tool.

## Repository Setup

### 1. Initialize Repository

Use the template matching your interface:

- [Tool plugin template](https://github.com/jdx/mise-tool-plugin-template).
- [Backend plugin template](https://github.com/jdx/mise-backend-plugin-template).
- [Environment plugin template](https://github.com/jdx/mise-env-plugin-template).

Create a new repository from the template, or initialize a fresh directory:

```sh
mkdir my-plugin
cd my-plugin
git init -b main
mkdir hooks
```

Add your implementation before testing. Empty metadata and hooks are not an installable plugin.

### 2. Basic Directory Structure

```text
my-plugin/
├── metadata.lua
├── README.md
├── LICENSE
├── hooks/
└── test/
```

The files inside `hooks/` identify the interface:

| Plugin type | Hook files |
| --- | --- |
| Tool | `available.lua`, `pre_install.lua`, `env_keys.lua`; optional lifecycle hooks |
| Backend | `backend_list_versions.lua`, `backend_install.lua`, `backend_exec_env.lua` |
| Environment | `mise_env.lua`, optional `mise_path.lua` |
| Package | `package_installed.lua`, `package_install.lua`; optional upgrade/uninstall hooks |

Backend hook implementations belong under `hooks/`, not in `metadata.lua`. Package plugins
also use `mise.plugin.toml` for manager capabilities; see [package development](/package-plugin-development.html).

### 3. Git Ignore Configuration

Ignore test output and local credentials using paths specific to your workflow. Do not
accidentally exclude files the plugin needs at runtime. Check the release tree with
`git ls-tree -r --name-only HEAD` and test the resulting archive or checkout.

## Versioning Strategy

### Semantic Versioning

SemVer is a useful convention for **plugin releases**: increase the major version for a
breaking configuration or behavior change, minor for compatible additions, and patch for
fixes. This does not imply that the **tools managed by the plugin** use SemVer.

### Version Management

Update `PLUGIN.version` and the release notes together:

```lua
PLUGIN = {
    name = "my-plugin",
    version = "1.2.3",
    description = "Manage Example Tool",
    author = "Plugin Author",
}
```

Use a Git tag for that release. A metadata version does not itself select a Git revision
when a user installs the repository.

## Testing Before Publication

### Automated Testing

Run tests with isolated mise directories so a local plugin cannot replace the developer's
installed plugin or edit their usual global configuration. This POSIX setup creates a
disposable test project. Save your plugin path before changing directories:

```sh
plugin_dir="$PWD"
test_dir="$(mktemp -d)"
export MISE_CONFIG_DIR="$test_dir/config"
export MISE_SYSTEM_CONFIG_DIR="$test_dir/system"
export MISE_GLOBAL_CONFIG_FILE="$test_dir/global.toml"
export MISE_DATA_DIR="$test_dir/data"
export MISE_CACHE_DIR="$test_dir/cache"
export MISE_STATE_DIR="$test_dir/state"
export MISE_ENV_CACHE=0
export MISE_YES=1
mkdir -p "$test_dir/project"
cd "$test_dir/project"
mise plugin link test-plugin "$plugin_dir"
```

Run this in a disposable shell/subshell and remove the temporary directory afterwards.
Clear any inherited `MISE_*` settings that affect your test, especially safe mode, forced
config paths, or disabled backends. These directories isolate mise's own state; package
plugins and external installers can still modify their host-managed state.

Then test the interface your plugin implements. The following names are placeholders:

| Type | Verification |
| --- | --- |
| Tool | `mise ls-remote test-plugin`, `mise use test-plugin@1.0.0`, `mise exec -- example --version` |
| Backend | `mise ls-remote test-plugin:example`, `mise use test-plugin:example@1.0.0`, `mise exec -- example --version` |
| Environment | Declare `_.test-plugin` under `[env]`, then assert the expected environment in a child process |
| Package | Use a disposable host profile or fake CLI; test status, selected batches, dry run, and ownership-aware prune |

The command after `mise exec --` must include the actual executable. Test a concrete tool
version rather than `latest` so an unrelated upstream release does not change the fixture.

### Manual Testing

Test the published Git ref or archive in a fresh test directory as well as a local symlink.
A symlink sees uncommitted and untracked files that may be absent from a release. Verify
paths containing spaces, required external programs, and every supported OS in CI. An
empty Linux container needs host prerequisites and an absolute mise executable path before
it can run your installer.

## Publishing Process

### 1. Prepare for Release

Run the plugin's documented checks, inspect the diff and release tree, update metadata and
notes, and commit only the intended release files. Verify that installation instructions
use your own repository URL and supported tool versions.

### 2. Create Release

From the reviewed release commit, create and push one tag:

```sh
git tag -a v1.2.3 -m "Release v1.2.3"
git push origin main
git push origin v1.2.3
```

Push the branch that actually contains the release if it is not `main`. Avoid `git push
--tags`, which can publish unrelated local tags.

### 3. GitHub Releases (Recommended)

Create a release for the existing tag with installation instructions, supported mise versions,
and behavior changes. Test the exact revision users will receive. Published signatures are
useful only when consumers verify them; do not imply that every plugin install validates
a Git tag signature.

### 4. Release Notes Template

````markdown
## v1.2.3

Describe the concrete behavior change, required mise version, and any migration steps.
List supported platforms and changed external prerequisites.

```sh
mise plugin install my-plugin 'https://github.com/your-org/my-plugin#v1.2.3'
```
````

## Distribution Methods

### 1. Direct Git Installation

```sh
mise plugin install my-plugin https://github.com/your-org/my-plugin
mise plugin install my-plugin 'https://github.com/your-org/my-plugin#v1.2.3'
```

Git refs use `#`, not the `@version` syntax used for tool requests. A tag or branch can move;
a commit ID identifies a fixed source revision. Existing installations require an explicit
update or replacement; sharing a new URL does not update users automatically.

### 2. Private Repository Access

Use the user's Git authentication setup, for example SSH:

```sh
mise plugin install my-plugin git@github.com:your-org/private-plugin.git
```

HTTPS repositories can use Git's credential helper. Do not put a token directly in a command
URL: it can be retained in shell history, configuration, process arguments, or Git remotes.
Verify access with `git ls-remote <repository-url>` before debugging plugin hooks.

### 3. Archive Distribution

Create an archive from the exact release ref, with a top-level directory:

```sh
git archive --format=zip --prefix=my-plugin/ --output=my-plugin-v1.2.3.zip v1.2.3
```

Publish it, then test its URL from a fresh mise data directory:

```sh
mise plugin install my-plugin https://github.com/your-org/my-plugin/releases/download/v1.2.3/my-plugin-v1.2.3.zip
```

An archive installation has no Git history. `mise plugin update` cannot fetch a new Git ref
for it; users must explicitly install the replacement archive.

## Maintenance and Updates

### 1. Update Workflow

Test changes, publish a new revision, and document how users update:

```sh
mise plugin update my-plugin#v1.3.0
```

Updating plugin code and upgrading installed tool versions are separate actions. Test both
new installations and existing installs when changing executable paths or environment hooks.

### 2. Backward Compatibility

Document renamed options, changed defaults, required external tools, and removed platforms.
Keep old configurations working when practical, and provide a complete replacement example
when a migration is necessary.

### 3. User Communication

Release notes should explain observable changes and the commands needed to adopt them.
State known limitations and how to report a reproducible failure without including secrets.

## Security Considerations

Review the code and dependencies you distribute. Treat tool names, paths, versions, and
configuration options as inputs when constructing commands. Keep downloads verified and
fail on missing required checksums. Do not print credentials or secret response bodies.

A plugin is executable code with the user's permissions. An archive, a tag, a tool lockfile,
and configuration trust each cover different things; avoid claiming that one secures all
of them. See [Security](/security.html) and [Using Plugins](/plugin-usage.html).

## Best Practices

Make the README sufficient to install, configure, run, update, and remove the integration.
Test from the release artifact, automate supported-platform checks, and keep fixtures
independent of personal settings and credentials.

## Troubleshooting

### Common Issues

If a plugin works as a local link but fails after publication, compare `git ls-tree` or
archive contents with the working directory. Check hook filenames and Lua syntax, and
confirm the published revision includes helper files.

If a version appears wrong, distinguish `PLUGIN.version`, the repository ref, and the
managed tool's version. Inspect `mise plugins ls --urls` and `mise ls --current` separately.
For authentication failures, first check repository access using Git itself.

## Next Steps

- [Backend Plugin Development](/backend-plugin-development.html).
- [Tool Plugin Development](/tool-plugin-development.html).
- [Environment Plugin Development](/env-plugin-development.html).
- [Package Plugin Development](/package-plugin-development.html).
- [Plugin Lua Modules](/plugin-lua-modules.html).

## Examples

### Simple Backend Plugin Release

Test `test-plugin:example` through all three backend hooks, then install the tagged repository
in a new data directory and repeat the same checks. This catches missing helper files and
incorrectly packaged hook directories.

### Tool Plugin with Hooks

Test version listing, artifact verification, extraction, `PostInstall` if present, and
`EnvKeys`. Include a version-file test in a project without a competing `[tools]` entry.
