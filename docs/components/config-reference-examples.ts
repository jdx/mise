// Hand-written mise.toml examples for the reference, keyed by a key's path as the reference
// writes it (`tasks.<name>.depends`, `tools.<github:…>`). The key explorer shows one of these
// for the selected key and writes a snippet from the key's type only where there is none.
//
// An example is a whole, realistic mise.toml excerpt, not a fragment of syntax: it shows the
// key the way a project would use it, with the neighbours that make it make sense. Every one
// is parsed and checked against schema/mise.json by config-reference-examples.test.ts, so an
// option that is renamed or removed fails the build instead of staying in the docs.

// `toml` drops the newline that starts each example.
const toml = (strings: TemplateStringsArray) => strings[0].replace(/^\n/, "");

export const examples: Record<string, string> = {
  // ---------------------------------------------------------------- tools
  tools: toml`
[tools]
node = "22"                       # the latest 22.x
python = ["3.13", "3.12"]         # both installed; the first is the default
terraform = "latest"
"npm:prettier" = "3"              # tools from any backend, not just the registry
"github:cli/cli" = "2"
`,
  "tools.<tool>": toml`
[tools]
go = "1.24"
python = ["3.13", "3.12"]         # the first version is the default

[tools.node]
version = "22"
postinstall = "corepack enable"

[tools.terraform]
version = "1.9"
os = ["linux", "macos"]
`,
  "tools.<tool>.version": toml`
[tools.node]
version = "22"                    # a prefix: the newest 22.x.y

[tools.python]
version = "latest"

[tools.ruby]
version = "ref:master"            # build a branch or tag from source
`,
  "tools.<tool>.os": toml`
# Only install on the systems that need it.
[tools.watchman]
version = "latest"
os = ["linux", "macos"]

[tools."aqua:microsoft/PowerShell"]
version = "latest"
os = "windows"
`,
  "tools.<tool>.postinstall": toml`
[tools.node]
version = "22"
postinstall = "corepack enable"

[tools.python]
version = "3.13"
postinstall = { run = "python -m pip install --upgrade pip", when = "always" }
`,
  "tools.<tool>.install_env": toml`
[tools.ruby]
version = "3.4"
install_env = { RUBY_CONFIGURE_OPTS = "--disable-install-doc" }

[tools.python]
version = "3.13"
install_env = { PYTHON_CONFIGURE_OPTS = "--enable-shared" }
`,
  "tools.<tool>.depends": toml`
# Install node first, then the CLI that is installed with npm.
[tools]
node = "22"

[tools."npm:typescript"]
version = "5"
depends = ["node"]
`,
  "tools.<tool>.lazy": toml`
# Installed the first time its shim runs, not by \`mise install\`
[tools.terraform]
version = "1.9"
lazy = true
`,
  "tools.<tool>.minimum_release_age": toml`
# Skip versions that were released less than a week ago.
[tools.node]
version = "latest"
minimum_release_age = "7d"
`,
  "tools.<tool>.auto_update": toml`
# In global config: update the tool before it runs, checking at most every 12 hours
[tools.node]
version = "22"
auto_update = "12h"
`,
  "tools.<tool>.platforms": toml`
[tools."http:my-cli"]
version = "1.2.3"

[tools."http:my-cli".platforms]
linux-x64 = { url = "https://example.com/my-cli-1.2.3-linux-amd64.tar.gz" }
macos-arm64 = { url = "https://example.com/my-cli-1.2.3-darwin-arm64.tar.gz" }
`,
  "tools.<github:…>": toml`
[tools]
"github:cli/cli" = "2"            # a release of any GitHub repository

[tools."github:BurntSushi/ripgrep"]
version = "14"
asset_pattern = "ripgrep-*-x86_64-unknown-linux-musl.tar.gz"
bin = "rg"
`,
  "tools.<github:…>.asset_pattern": toml`
[tools."github:BurntSushi/ripgrep"]
version = "14"
asset_pattern = "ripgrep-*-x86_64-unknown-linux-musl.tar.gz"
`,
  "tools.<github:…>.platforms": toml`
[tools."github:BurntSushi/ripgrep"]
version = "14"

[tools."github:BurntSushi/ripgrep".platforms]
linux-x64 = { asset_pattern = "ripgrep-*-x86_64-unknown-linux-musl.tar.gz" }
macos-arm64 = { asset_pattern = "ripgrep-*-aarch64-apple-darwin.tar.gz" }
`,
  "tools.<github:…>.bin": toml`
# The executable has a different name than the repository.
[tools."github:BurntSushi/ripgrep"]
version = "14"
bin = "rg"
`,
  "tools.<github:…>.version_prefix": toml`
# Tags look like "v1.2.3" or "release-1.2.3".
[tools."github:owner/tool"]
version = "1.2.3"
version_prefix = "release-"
`,
  "tools.<github:…>.checksum": toml`
[tools."github:owner/tool"]
version = "1.2.3"
checksum = "sha256:3b7a5e1f0c7d4a8e9f2b6c1d0e5a4f8b7c3d2e1a0f9b8c7d6e5f4a3b2c1d0e9f"
`,
  "tools.<github:…>.strip_components": toml`
# Skip the top-level directory inside the archive.
[tools."github:owner/tool"]
version = "1.2.3"
strip_components = 1
`,
  "tools.<github:…>.rename_exe": toml`
[tools."github:owner/tool"]
version = "1.2.3"
rename_exe = "tool-linux-amd64"
`,
  "tools.<gitlab:…>": toml`
[tools."gitlab:gitlab-org/cli"]
version = "1"
asset_pattern = "glab_*_linux_amd64.tar.gz"
`,
  "tools.<http:…>": toml`
[tools."http:my-cli"]
version = "1.2.3"
url = "https://example.com/my-cli-{{ version }}-linux-amd64.tar.gz"
checksum = "sha256:3b7a5e1f0c7d4a8e9f2b6c1d0e5a4f8b7c3d2e1a0f9b8c7d6e5f4a3b2c1d0e9f"
bin_path = "bin"
`,
  "tools.<http:…>.url": toml`
[tools."http:my-cli"]
version = "1.2.3"
url = "https://example.com/my-cli-{{ version }}-linux-amd64.tar.gz"
`,
  "tools.<http:…>.headers": toml`
[tools."http:internal-cli"]
version = "1.2.3"
url = "https://artifacts.example.com/internal-cli-{{ version }}.tar.gz"
headers = { Authorization = "Bearer {{ env.ARTIFACTS_TOKEN }}" }
`,
  "tools.<ubi:…>": toml`
[tools."ubi:goreleaser/goreleaser"]
version = "latest"
exe = "goreleaser"
`,
  "tools.<aqua:…>": toml`
[tools]
"aqua:BurntSushi/ripgrep" = "latest"

[tools."aqua:cli/cli"]
version = "2"
symlink_bins = true
`,
  "tools.<cargo:…>": toml`
[tools]
"cargo:ripgrep" = "latest"

[tools."cargo:cargo-edit"]
version = "latest"
features = ["vendored-libgit2"]
locked = true
`,
  "tools.<cargo:…>.features": toml`
[tools."cargo:cargo-edit"]
version = "latest"
features = ["vendored-libgit2"]
`,
  "tools.<npm:…>": toml`
[tools]
"npm:prettier" = "3"

[tools."npm:typescript"]
version = "5"
`,
  "tools.<pypi:…>": toml`
[tools]
"pipx:black" = "latest"

[tools."pipx:httpie"]
version = "latest"
extras = ["socks"]
`,
  "tools.<pypi:…>.extras": toml`
[tools."pipx:httpie"]
version = "latest"
extras = ["socks"]
`,
  "tools.<go:…>": toml`
[tools]
"go:golang.org/x/tools/gopls" = "latest"

[tools."go:github.com/golangci/golangci-lint/cmd/golangci-lint"]
version = "latest"
tags = ["osusergo"]
`,
  "tools.<gem:…>": toml`
[tools]
"gem:rubocop" = "latest"

[tools."gem:rails"]
version = "7"
`,
  "tools.<conda:…>": toml`
[tools."conda:ffmpeg"]
version = "latest"
channel = "conda-forge"
`,
  "tools.python": toml`
[tools.python]
version = "3.13"
patch_sysconfig = true
`,
  "tools.java": toml`
[tools.java]
version = "temurin-21"
release_type = "ga"
`,
  "tools.rust": toml`
[tools.rust]
version = "stable"
profile = "minimal"
components = ["clippy", "rustfmt"]
targets = ["wasm32-unknown-unknown"]
`,
  "tools.rust.components": toml`
[tools.rust]
version = "stable"
components = ["clippy", "rustfmt", "rust-analyzer"]
`,
  "tools.rust.targets": toml`
[tools.rust]
version = "stable"
targets = ["wasm32-unknown-unknown", "aarch64-unknown-linux-gnu"]
`,
  "tools.dotnet": toml`
[tools.dotnet]
version = "9"
runtime = "aspnetcore"
`,

  // ---------------------------------------------------------------- tool_alias, alias
  tool_alias: toml`
[tool_alias]
# \`tf\` is another name for terraform, and \`my-cli\` installs from a GitHub release.
tf = "terraform"
my-cli = "github:my-org/my-cli"
`,
  "tool_alias.<tool>": toml`
[tool_alias]
tf = "terraform"
"my-cli" = "github:my-org/my-cli"

[tool_alias.node.versions]
lts = "22"
stable = "24"
`,
  "tool_alias.<tool>.backend": toml`
[tool_alias.ripgrep]
backend = "aqua:BurntSushi/ripgrep"
`,
  "tool_alias.<tool>.versions": toml`
# \`mise use node@lts\` now means node 22.
[tool_alias.node.versions]
lts = "22"
stable = "24"
`,

  // ---------------------------------------------------------------- tasks
  tasks: toml`
[tasks.build]
description = "Compile the project"
run = "cargo build"

[tasks.test]
description = "Run the tests"
depends = ["build"]
run = "cargo test"

[tasks.lint]
run = ["cargo fmt --check", "cargo clippy"]
`,
  "tasks.<name>": toml`
# a task is a command...
[tasks.fmt]
run = "cargo fmt"

# ...or a table with options
[tasks.build]
description = "Compile the project"
depends = ["fmt"]
sources = ["src/**/*.rs", "Cargo.toml"]
outputs = ["target/debug/app"]
run = "cargo build"
`,
  "tasks.<name>.run": toml`
# One command
[tasks.build]
run = "cargo build"

# Several, run one after another
[tasks.ci]
run = [
  "cargo fmt --check",
  "cargo clippy",
  "cargo test",
]

# Other tasks, and commands, mixed
[tasks.release]
run = [
  { task = "ci" },
  "cargo publish",
]

# Start with a multi-line script
[tasks.setup]
run = """
#!/usr/bin/env bash
set -euo pipefail
npm ci
npm run build
"""
`,
  "tasks.<name>.run_windows": toml`
[tasks.clean]
run = "rm -rf dist"
run_windows = "rmdir /s /q dist"
`,
  "tasks.<name>.description": toml`
[tasks.build]
description = "Compile the project"
run = "cargo build"
`,
  "tasks.<name>.alias": toml`
[tasks.build]
alias = "b"
run = "cargo build"

[tasks.test]
alias = ["t", "check"]
run = "cargo test"
`,
  "tasks.<name>.depends": toml`
[tasks.build]
run = "cargo build"

[tasks.lint]
run = "cargo clippy"

# \`mise run test\` runs build and lint (in parallel) first
[tasks.test]
depends = ["build", "lint"]
run = "cargo test"

# arguments and environment can be passed to a dependency
[tasks.release]
depends = [{ task = "build", args = ["--release"], env = { CARGO_INCREMENTAL = "0" } }]
run = "cargo publish"
`,
  "tasks.<name>.depends_post": toml`
# \`mise run build\` also runs notify afterwards
[tasks.build]
run = "cargo build"
depends_post = ["notify"]

[tasks.notify]
run = "echo build finished"
`,
  "tasks.<name>.wait_for": toml`
# lint is not required, but if it is part of the run, build waits for it
[tasks.build]
run = "cargo build"
wait_for = ["lint"]

[tasks.lint]
run = "cargo clippy"
`,
  "tasks.<name>.env": toml`
[tasks.test]
run = "cargo test"

[tasks.test.env]
RUST_BACKTRACE = "1"
DATABASE_URL = "postgres://localhost/test"
`,
  "tasks.<name>.vars": toml`
[tasks.deploy]
run = "./deploy.sh {{ vars.environment }}"

[tasks.deploy.vars]
environment = "staging"
`,
  "tasks.<name>.dir": toml`
[tasks.build-web]
dir = "web"
run = "npm run build"

[tasks.test-api]
dir = "{{ config_root }}/services/api"
run = "go test ./..."
`,
  "tasks.<name>.sources": toml`
# Skip the task when no source is newer than the outputs
[tasks.build]
sources = ["src/**/*.rs", "Cargo.toml", "Cargo.lock"]
outputs = ["target/release/app"]
run = "cargo build --release"
`,
  "tasks.<name>.outputs": toml`
[tasks.build]
sources = ["src/**/*.ts", "package.json"]
outputs = ["dist/**/*"]
run = "npm run build"
`,
  "tasks.<name>.tools": toml`
# Tools only this task needs; they are installed the first time it runs
[tasks.docs]
description = "Build the docs"
run = "mdbook build"

[tasks.docs.tools]
mdbook = "latest"
node = "22"
`,
  "tasks.<name>.tools.<tool>": toml`
[tasks.docs]
run = "mdbook build"

[tasks.docs.tools]
mdbook = "latest"
`,
  "tasks.<name>.hide": toml`
# Left out of \`mise tasks\`, still runnable
[tasks.internal-setup]
hide = true
run = "./scripts/setup.sh"
`,
  "tasks.<name>.confirm": toml`
[tasks.deploy]
confirm = "Deploy to production?"
run = "./deploy.sh"

[tasks.reset-db]
confirm = { message = "This deletes all data. Continue?", default = "no" }
run = "dropdb app && createdb app"
`,
  "tasks.<name>.shell": toml`
[tasks.build]
shell = "bash -c"
run = "set -euo pipefail; make all"

[tasks.convert]
shell = "python -c"
run = "print('hello from python')"
`,
  "tasks.<name>.file": toml`
# Run a script instead of an inline command
[tasks.deploy]
file = "scripts/deploy.sh"
`,
  "tasks.<name>.usage": toml`
[tasks.greet]
usage = '''
arg "<name>" help="Who to greet"
flag "-l --loud" help="Shout"
'''
run = 'echo "Hello, \${usage_name}"'
`,
  "tasks.<name>.timeout": toml`
[tasks.integration]
timeout = "10m"
run = "cargo test --test integration"
`,
  "tasks.<name>.skip_if": toml`
# Do nothing when the output already exists
[tasks.generate]
skip_if = "test -f generated/schema.json"
run = "./scripts/generate-schema.sh"
`,
  "tasks.<name>.interactive": toml`
[tasks.login]
interactive = true
run = "aws sso login"
`,
  "tasks.<name>.output": toml`
[tasks.build]
output = "prefix"
run = "cargo build"

[tasks.watch]
output = "interleave"
run = "npm run watch"
`,
  "tasks.<name>.quiet": toml`
# Do not print the \`[task] $ command\` line
[tasks.version]
quiet = true
run = "cargo pkgid"
`,
  "tasks.<name>.silent": toml`
# Print nothing at all, or just hide one stream
[tasks.version]
silent = true
run = "cargo pkgid"

[tasks.probe]
silent = "stderr"
run = "./probe.sh"
`,
  "tasks.<name>.extends": toml`
[task_templates.ci]
tools = { node = "22" }
env = { CI = "1" }

[tasks.test]
extends = "ci"
run = "npm test"
`,
  "tasks.<name>.cache": toml`
[tasks.build]
run = "npm run build"
sources = ["src/**/*", "package.json"]
outputs = ["dist/**/*"]

[tasks.build.cache]
enabled = true
env = ["NODE_ENV"]
`,
  "tasks.<name>.watch": toml`
[tasks.dev]
sources = ["src/**/*"]
run = "npm run dev"

[tasks.dev.watch]
no_vcs_ignore = true
`,
  "tasks.<name>.daemons": toml`
[settings]
experimental = true

[daemons]
postgres = "18"

[tasks.dev]
daemons = "postgres"
run = "npm run dev"
`,
  "tasks.<name>.secrets": toml`
[tasks.deploy]
secrets = ["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"]
run = "./deploy.sh"
`,
  "tasks.<name>.deny_net": toml`
[tasks.build]
run = "npm run build"
deny_net = true
allow_write = ["./dist"]
`,
  "tasks.<name>.pass_through_env": toml`
[tasks.test]
run = "cargo test"
pass_through_env = ["HOME", "CARGO_HOME"]
`,

  // ---------------------------------------------------------------- task_templates, task_config
  task_templates: toml`
[task_templates.node-ci]
tools = { node = "22" }
env = { CI = "1" }
run = "npm ci && npm run {{ vars.script }}"

[tasks.lint]
extends = "node-ci"
vars = { script = "lint" }

[tasks.test]
extends = "node-ci"
vars = { script = "test" }
`,
  "task_templates.<name>": toml`
[task_templates.node-ci]
description = "Install dependencies, then run an npm script"
tools = { node = "22" }
run = "npm ci && npm run {{ vars.script }}"

[tasks.test]
extends = "node-ci"
vars = { script = "test" }
`,
  task_config: toml`
[task_config]
dir = "{{ config_root }}/scripts"   # where tasks run, unless they say otherwise
shell = "bash -c"
includes = ["tasks", "scripts/tasks"]   # directories of file tasks
`,
  "task_config.includes": toml`
[task_config]
includes = ["tasks", "scripts/tasks", "mise-tasks/*"]
`,
  "task_config.excludes": toml`
[task_config]
excludes = ["tasks/experimental"]
`,
  "task_config.dir": toml`
[task_config]
dir = "{{ config_root }}"
`,
  "task_config.shell": toml`
[task_config]
shell = "bash -c"
`,
  "task_config.cascade": toml`
# Tasks in subdirectories also see this directory's tasks
[task_config]
cascade = true
`,
  "task_config.cache": toml`
[task_config.cache]
enabled = true
audit = true
env = ["CI"]
`,

  // ---------------------------------------------------------------- env
  env: toml`
[env]
NODE_ENV = "development"
DATABASE_URL = "postgres://localhost/app_dev"
PORT = 3000                                  # numbers and booleans work too
APP_ROOT = "{{ config_root }}"               # templates
PATH_EXTRA = "{{ env.HOME }}/bin"
UNWANTED = false                             # unset a variable set elsewhere

[env._]
file = ".env"                                # load a dotenv file
path = ["node_modules/.bin", "./bin"]        # prepend to PATH
`,
  "env.<NAME>": toml`
[env]
NODE_ENV = "development"
PORT = 3000
DEBUG = true
APP_ROOT = "{{ config_root }}"
GOPATH = false                    # remove it from the environment
`,
  "env.<NAME>.value": toml`
[env.API_URL]
value = "https://staging.example.com"
tools = true                      # can read what installed tools set
`,
  "env.<NAME>.default": toml`
# Keep a value that is already set; otherwise use this
[env.LOG_LEVEL]
default = "info"
`,
  "env.<NAME>.required": toml`
# Fail with this message when it is not set
[env.DATABASE_URL]
required = "Set DATABASE_URL, for example in .env.local"
`,
  "env.<NAME>.redact": toml`
# Hidden in output and logs
[env.API_TOKEN]
value = "{{ env.EXTERNAL_API_TOKEN }}"
redact = true
`,
  "env.<NAME>.tools": toml`
# Evaluated after tools are on PATH
[env.GOBIN]
value = "{{ env.GOPATH }}/bin"
tools = true
`,
  "env._": toml`
[env._]
file = ".env"
path = ["node_modules/.bin"]
source = "scripts/env.sh"
`,
  "env._.file": toml`
[env._]
file = ".env"                                    # one file

# or several, with options
# file = [".env", { path = ".env.local", required = false }]
`,
  "env._.path": toml`
[env._]
path = ["node_modules/.bin", "{{ config_root }}/bin"]
`,
  "env._.source": toml`
# Run a script and keep the variables it exports
[env._]
source = "scripts/env.sh"
`,
  "env._.python.venv": toml`
[env._.python]
venv = { path = ".venv", create = true }
`,
  "env._.file.redact": toml`
[env._]
file = { path = ".env.secrets", redact = true }
`,
  "env._.file.required": toml`
[env._]
file = { path = ".env.local", required = false }
`,
  "env._.file.tools": toml`
[env._]
file = { path = ".env", tools = true }
`,
  "env._.file.expand": toml`
[env._]
file = { path = ".env", expand = true }
`,
  "env._.file.find_up": toml`
# Use the nearest .env, in this directory or above it
[env._]
file = { path = ".env", find_up = true }
`,

  // ---------------------------------------------------------------- vars
  vars: toml`
[vars]
region = "us-east-1"
image = "ghcr.io/me/app:{{ vars.region }}"

[tasks.deploy]
run = "deploy --region {{ vars.region }}"
`,
  "vars.<name>": toml`
[vars]
region = "us-east-1"
replicas = 3

[tasks.deploy]
run = "deploy --region {{ vars.region }} --replicas {{ vars.replicas }}"
`,
  "vars.<name>.prompt": toml`
# Asked once by \`mise bootstrap --prompt-vars\`, then remembered
[vars.region]
prompt = "Which region?"
default = "us-east-1"
`,
  "vars.<name>.default": toml`
[vars.region]
default = "us-east-1"
`,
  "vars.<name>.required": toml`
[vars.region]
required = "Set region in mise.local.toml"
`,
  "vars._.file": toml`
[vars._]
file = "vars.toml"
`,

  // ---------------------------------------------------------------- shell_alias, plugins, wrappers
  shell_alias: toml`
[shell_alias]
ll = "ls -la"
gs = "git status"
`,
  "shell_alias.<name>": toml`
[shell_alias]
ll = "ls -la"
`,
  plugins: toml`
[plugins]
my-tool = "https://github.com/my-org/asdf-my-tool"
`,
  wrappers: toml`
# \`cargo\` in this project runs through the wrapper, with these extras
[wrappers.cargo]
command = "cargo"
args = ["--locked"]
env = { CARGO_TERM_COLOR = "always" }
`,
  "wrappers.<command>": toml`
[wrappers.cargo]
command = "cargo"
args = ["--locked"]
env = { CARGO_TERM_COLOR = "always" }
`,

  // ---------------------------------------------------------------- hooks, watch_files
  hooks: toml`
[hooks]
enter = "echo 'welcome to {{ config_root }}'"
leave = "echo 'goodbye'"
postinstall = "npm install"

[[hooks.cd]]
run = "git fetch --quiet &"
`,
  "hooks.enter": toml`
[hooks]
enter = "echo 'entering the project'"

# or a list, with options
# [[hooks.enter]]
# shell = "bash"
# run = "echo 'entering the project'"
`,
  "hooks.leave": toml`
[hooks]
leave = "echo 'leaving the project'"
`,
  "hooks.cd": toml`
# Runs on every directory change inside the project
[hooks]
cd = "echo 'now in $PWD'"
`,
  "hooks.preinstall": toml`
[hooks]
preinstall = "echo 'about to install tools'"
`,
  "hooks.postinstall": toml`
[hooks]
postinstall = "npm install -g corepack"

# or run a task:
# [hooks.postinstall]
# task = "setup"
`,
  watch_files: toml`
[[watch_files]]
patterns = ["package.json", "package-lock.json"]
run = "npm install"

[[watch_files]]
patterns = ["Cargo.lock"]
task = "build"
`,

  // ---------------------------------------------------------------- config file
  settings: toml`
[settings]
experimental = true
jobs = 4
idiomatic_version_file_enable_tools = ["node", "python"]
`,
  include: toml`
include = ["mise.shared.toml", "config/*.toml"]
`,
  min_version: toml`
# Refuse to run on a mise older than 2024.11.0
min_version = "2024.11.0"
`,
  "min_version.hard": toml`
[min_version]
hard = "2024.11.0"   # mise refuses to run with an older version
`,
  "min_version.soft": toml`
[min_version]
soft = "2025.1.0"    # mise warns, then carries on
`,
  redactions: toml`
# Hidden in output wherever they appear
redactions = ["API_TOKEN", "*_SECRET"]
`,
  "secrets.fnox": toml`
[secrets.fnox]
profile = "production"
`,
  tool_config: toml`
[tool_config]
locked = true
`,
  "tool_config.locked": toml`
# Only install the versions pinned in mise.lock
[tool_config]
locked = true
`,
  _: toml`
# Free-form data for your own scripts and tools; mise ignores it
[_]
team = "platform"
owners = ["alice", "bob"]
`,

  // ---------------------------------------------------------------- monorepo
  monorepo_root: toml`
monorepo_root = true

[monorepo]
config_roots = ["projects/*"]
`,
  monorepo: toml`
monorepo_root = true

[monorepo]
config_roots = ["projects/frontend", "projects/backend"]

[monorepo.path_aliases]
web = "projects/frontend"
api = "projects/backend"
`,
  "monorepo.config_roots": toml`
monorepo_root = true

[monorepo]
config_roots = ["projects/frontend", "projects/backend", "packages/*"]
`,
  "monorepo.path_aliases": toml`
[monorepo.path_aliases]
web = "projects/frontend"
api = "projects/backend"
`,

  // ---------------------------------------------------------------- project services
  deps: toml`
[settings]
experimental = true

[tools]
node = "24"

[deps.npm]
auto = true

[deps.uv]
auto = true
`,
  "deps.npm": toml`
# Run \`npm install\` automatically when package-lock.json changes
[deps.npm]
auto = true
`,
  "deps.npm.auto": toml`
[deps.npm]
auto = true
`,
  "deps.<key>": toml`
# A provider of your own
[deps.protos]
sources = ["proto/**/*.proto"]
outputs = ["gen/**/*"]
run = "buf generate"
`,
  "deps.disable": toml`
[deps]
disable = ["npm"]
`,
  oci: toml`
[oci]
from = "debian:bookworm-slim"
tag = "ghcr.io/my-org/devenv:v1"
workdir = "/workspace"
cmd = ["bash"]

[oci.env]
EDITOR = "vim"

[oci.labels]
"org.opencontainers.image.source" = "https://github.com/my-org/my-app"
`,
  "oci.from": toml`
[oci]
from = "debian:bookworm-slim"
`,
  "oci.copy": toml`
[[oci.copy]]
host = "./config"
image = "/etc/my-app"
`,
  daemons: toml`
[settings]
experimental = true

[daemons]
postgres = "18"                  # a service preset

[daemons.api]
run = "exec npm run dev"         # a command of your own
port = 3000
`,
  "daemons.<name>": toml`
[daemons]
postgres = "18"

[daemons.api]
run = "exec npm run dev"
port = 3000
`,
  "daemons.<name>.run": toml`
[daemons.api]
run = "exec npm run dev"
`,
  "daemons.<name>.task": toml`
[tasks.serve]
run = "exec npm run dev"

[daemons.api]
task = "serve"
`,
  "daemons.<name>.preset": toml`
[daemons.analytics]
preset = "postgres"
version = "18"
port = 5433
`,
  "daemons.<name>.port": toml`
[daemons.api]
run = "exec npm run dev"
port = { auto = true, base = 3000 }   # a different port in each worktree
`,
  daemon_groups: toml`
[daemons]
postgres = "18"
redis = "7"

[daemon_groups]
default = ["postgres", "redis"]
`,
  "daemon_groups.<name>": toml`
[daemon_groups]
default = ["postgres", "redis"]
full = ["default", "worker"]
`,
  daemons_settings: toml`
[daemons_settings]
namespace = "my-app"
namespace_per_worktree = true
`,
  doctor: toml`
[doctor.checks.openssl]
description = "OpenSSL development files are discoverable"
run = "pkg-config --exists openssl"
hint = "Run \`mise bootstrap packages apply\` to install the build dependencies."
`,
  "doctor.checks.<key>": toml`
[doctor.checks.database]
description = "PostgreSQL accepts connections"
run = "pg_isready --quiet"
hint = "Start it with \`mise daemons start postgres\`."
timeout = "5s"
os = ["linux", "macos"]
`,

  // ---------------------------------------------------------------- machine setup
  dotfiles: toml`
[dotfiles]
"~/.gitconfig" = { source = "git/config", mode = "copy" }
"~/.config/nvim" = { source = "nvim" }      # a symlink, the default mode
"~/.zshrc" = { mode = "track" }             # leave it in place, keep its history
`,
  "dotfiles.<target>": toml`
[dotfiles]
"~/.gitconfig" = { source = "git/config", mode = "copy" }
"~/.config/nvim" = { source = "nvim" }
`,
  "dotfiles.<target>.source": toml`
[dotfiles]
"~/.config/nvim" = { source = "nvim" }
`,
  "dotfiles.<target>.mode": toml`
[dotfiles]
"~/.config/nvim" = { source = "nvim", mode = "symlink" }
"~/.gitconfig" = { source = "git/config", mode = "copy" }
"~/.zshrc" = { mode = "track" }
`,
  dotfile_groups: toml`
[dotfile_groups.nvim]
root = "nvim"
target = "~/.config/nvim"
mode = "symlink-each"
exclude = ["README.md"]
`,
  "dotfile_groups.<name>": toml`
[dotfile_groups.nvim]
root = "nvim"
target = "~/.config/nvim"
mode = "symlink-each"
`,

  // ---------------------------------------------------------------- more tools
  "tools.<forgejo:…>": toml`
[tools."forgejo:forgejo/runner"]
version = "latest"
api_url = "https://code.forgejo.org/api/v1"
bin = "forgejo-runner"
`,
  "tools.<s3:…>": toml`
[tools."s3:my-tool"]
version = "1.0.0"
url = "s3://my-bucket/tools/my-tool-v{{ version }}.tar.gz"
region = "us-east-1"
`,
  "tools.<pipx:…>": toml`
[tools]
"pipx:black" = "latest"

[tools."pipx:httpie"]
version = "latest"
extras = ["socks"]
`,
  "tools.<spm:…>": toml`
[tools]
"spm:nicklockwood/SwiftFormat" = "latest"

[tools."spm:patricklorran/ios-settings"]
version = "latest"
provider = "gitlab"
`,
  "tools.<packslip:…>": toml`
[tools]
"packslip:github.com/jdx/hk" = "latest"

[tools."packslip:github.com/example/tool"]
version = "latest"
variant = "fips"
`,
  "tools.<spinel:…>": toml`
[settings]
experimental = true

[tools."spinel:tobi/try"]
version = "1.10.1"
entrypoint = "try.rb"
bin = "try"
tag_prefix = "v"
`,
  "tools.dotnet-core": toml`
[tools.dotnet-core]
version = "9"
runtime = "aspnetcore"
`,
  "tools.<tool>.platforms.<key>": toml`
[tools."http:my-cli"]
version = "1.2.3"

[tools."http:my-cli".platforms.linux-x64]
url = "https://example.com/my-cli-1.2.3-linux-amd64.tar.gz"
`,
  "tools.<tool>.platform": toml`
# \`platform\` is another name for \`platforms\`
[tools."http:my-cli"]
version = "1.2.3"

[tools."http:my-cli".platform]
linux-x64 = { url = "https://example.com/my-cli-1.2.3-linux-amd64.tar.gz" }
macos-arm64 = { url = "https://example.com/my-cli-1.2.3-darwin-arm64.tar.gz" }
`,
  "tools.<tool>.platform.<key>": toml`
[tools."http:my-cli"]
version = "1.2.3"

[tools."http:my-cli".platform.macos-arm64]
url = "https://example.com/my-cli-1.2.3-darwin-arm64.tar.gz"
`,
  "tools.<github:…>.platforms.<key>": toml`
[tools."github:BurntSushi/ripgrep"]
version = "14"

[tools."github:BurntSushi/ripgrep".platforms.macos-arm64]
asset_pattern = "ripgrep-*-aarch64-apple-darwin.tar.gz"
`,

  // ---------------------------------------------------------------- env and vars, more
  "env._.python": toml`
[env._.python]
venv = { path = ".venv", create = true }
`,
  "env.<NAME>.age": toml`
# \`mise set --age-encrypt API_KEY=…\` writes this
[env]
API_KEY = { age = "YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IFgyNTUxOSBN..." }

# or, to set options on it
[env.DB_PASSWORD]
age = { value = "YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IFgyNTUxOSBN...", format = "zstd", redact = true }
`,
  "vars.<name>.age": toml`
[vars.api_key]
age = { value = "YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IFgyNTUxOSBN...", format = "zstd" }
`,
  "vars._.source": toml`
[vars._]
source = "scripts/vars.sh"
`,
  secrets: toml`
[secrets.fnox]
profile = "production"
`,
  "hooks.enter.script": toml`
# Evaluated in the shell, so it can change the shell itself
[[hooks.enter]]
shell = "bash"
script = "source .venv/bin/activate"
`,
  "hooks.enter.scripts": toml`
[[hooks.enter]]
shell = "bash"
scripts = ["source .venv/bin/activate", "export APP_ENV=development"]
`,

  // ---------------------------------------------------------------- bootstrap
  bootstrap: toml`
# A machine, described: ~/.config/mise/config.toml, applied by \`mise bootstrap\`
[bootstrap.mise_shell_activate]
zsh = true

[bootstrap.user]
login_shell = "zsh"

[bootstrap.packages]
"brew:git" = "latest"
"brew:ripgrep" = "latest"
"apt:build-essential" = "latest"

[bootstrap.repos."~/src/dotfiles"]
url = "https://github.com/me/dotfiles.git"

[tools]
node = "24"
`,
  "bootstrap.mise_shell_activate": toml`
[bootstrap.mise_shell_activate]
zsh = true                 # \`mise activate\` in ~/.zshrc, shims in ~/.zprofile
fish = "auto"              # activation at the prompt, shims in scripts
bash = { enabled = true, mode = "shims" }
`,
  "bootstrap.user": toml`
[bootstrap.user]
login_shell = "zsh"
`,
  "bootstrap.user.login_shell": toml`
[bootstrap.user]
login_shell = "zsh"
`,
  "bootstrap.packages": toml`
[bootstrap.packages]
"apt:libssl-dev" = "latest"
"brew:postgresql@16" = "16"
"brew-cask:visual-studio-code" = "latest"
"snap:code" = { channel = "latest/stable", classic = true }
`,
  "bootstrap.packages.<key>": toml`
[bootstrap.packages]
"brew:ripgrep" = "latest"

[bootstrap.packages."apt:postgresql-16"]
version = "16"
os = "linux"
`,
  "bootstrap.repos": toml`
[bootstrap.repos."~/src/mise"]
url = "https://github.com/jdx/mise.git"
ref = "main"

[bootstrap.repos."~/src/linux"]
url = "https://github.com/torvalds/linux.git"
depth = 1
filter = "blob:none"
`,
  "bootstrap.repos.<key>": toml`
[bootstrap.repos."~/src/mise"]
url = "https://github.com/jdx/mise.git"
ref = "main"
submodules = true
`,
  "bootstrap.plugins": toml`
[bootstrap.plugins]
brew = "https://github.com/my-org/mise-package-plugin"
`,
  "bootstrap.files": toml`
[bootstrap.files."/etc/ssh/sshd_config.d/10-hardening.conf"]
content = """
PermitRootLogin no
PasswordAuthentication no
"""
mode = "0644"
notify = ["sshd"]
os = "linux"
`,
  "bootstrap.files.<key>": toml`
[bootstrap.files."/etc/motd"]
source = "files/motd.tera"
template = true
owner = "root"
group = "root"
mode = "0644"
`,
  "bootstrap.directories": toml`
[bootstrap.directories."/srv/app"]
owner = "deploy"
group = "deploy"
mode = "0750"
`,
  "bootstrap.directories.<key>": toml`
[bootstrap.directories."/srv/app/cache"]
owner = "deploy"
mode = "0700"
phase = "post-packages"
`,
  "bootstrap.services": toml`
[bootstrap.services.sshd]
state = "running"
enabled = true

[bootstrap.services.notes-sync]
scope = "user"
command = "mise run sync-notes"
restart = "on-failure"
`,
  "bootstrap.services.<key>": toml`
[bootstrap.services.notes-sync]
scope = "user"
command = "mise run sync-notes"
description = "Sync notes"
working_directory = "~/notes"
environment = { RSYNC_RSH = "ssh" }
`,
  "bootstrap.compose": toml`
[bootstrap.compose.app]
project_dir = "/srv/app"
files = ["compose.yaml"]
depends_on = ["package:apt:docker.io", "service:docker"]
`,
  "bootstrap.compose.<key>": toml`
[bootstrap.compose.app]
project_dir = "/srv/app"
files = ["compose.yaml", "compose.prod.yaml"]
profiles = ["prod"]
oneshot = ["migrate"]
pull = "always"
`,
  "bootstrap.remote": toml`
[bootstrap.remote]
source = "."
install_mise = true
exclude = ["node_modules", "target"]

[bootstrap.remote.hosts.build]
host = "build.example.com"
user = "deploy"
tags = ["ci"]
`,
  "bootstrap.remote.hosts": toml`
[bootstrap.remote.hosts.build]
host = "build.example.com"
user = "deploy"

[bootstrap.remote.hosts.staging]
host = "10.0.4.12"
user = "deploy"
port = 2222
tags = ["staging"]
`,
  "bootstrap.remote.hosts.<key>": toml`
[bootstrap.remote.hosts.build]
host = "build.example.com"
user = "deploy"
identity_file = "~/.ssh/id_ed25519"
ssh_options = ["StrictHostKeyChecking=accept-new"]
tags = ["ci"]
`,
  "bootstrap.hooks": toml`
[bootstrap.hooks]
pre-packages = "echo starting"
post-tools = ["mise run build", "mise run test"]
final = { run = "echo finished" }
`,
  "bootstrap.secrets": toml`
[bootstrap.secrets]
GITHUB_TOKEN = "GITHUB_TOKEN"

[bootstrap.secrets.NPM_TOKEN]
env = "NPM_TOKEN"
description = "Token for the private registry"
`,
  "bootstrap.groups": toml`
[bootstrap.groups.deploy]
gid = 1500
`,
  "bootstrap.groups.<key>": toml`
[bootstrap.groups.deploy]
gid = 1500
system = false
`,
  "bootstrap.users": toml`
[bootstrap.users.deploy]
group = "deploy"
groups = ["docker", "sudo"]
shell = "/bin/zsh"
home = "/home/deploy"
create_home = true
`,
  "bootstrap.users.<key>": toml`
[bootstrap.users.deploy]
uid = 1500
group = "deploy"
groups = ["docker"]
comment = "Deploy user"
`,
  "bootstrap.apt": toml`
[bootstrap.apt.sources.docker]
uris = "https://download.docker.com/linux/ubuntu"
suites = "noble"
components = "stable"
key = "https://download.docker.com/linux/ubuntu/gpg"
fingerprint = "9DC858229FC7DD38854AE2D88D81803C0EBFCD88"

[bootstrap.packages]
"apt:docker-ce" = "latest"
`,
  "bootstrap.apt.sources": toml`
[bootstrap.apt.sources.git]
ppa = "git-core/ppa"
`,
  "bootstrap.apt.sources.<key>": toml`
[bootstrap.apt.sources.docker]
uris = "https://download.docker.com/linux/ubuntu"
suites = "noble"
components = "stable"
key = "https://download.docker.com/linux/ubuntu/gpg"
fingerprint = "9DC858229FC7DD38854AE2D88D81803C0EBFCD88"
`,
  "bootstrap.dnf": toml`
[bootstrap.dnf.repos.ghostty]
copr = "scottames/ghostty"

[bootstrap.packages]
"dnf:ghostty" = "latest"
`,
  "bootstrap.dnf.repos": toml`
[bootstrap.dnf.repos.ghostty]
copr = "scottames/ghostty"
`,
  "bootstrap.dnf.repos.<key>": toml`
[bootstrap.dnf.repos.ghostty]
copr = "scottames/ghostty"
`,
  "bootstrap.brew": toml`
[bootstrap.brew]
adopt = true

[bootstrap.brew.taps]
my-org = "https://github.com/my-org/homebrew-tools.git"
`,
  "bootstrap.brew.taps": toml`
[bootstrap.brew.taps]
my-org = "https://github.com/my-org/homebrew-tools.git"
`,
  "bootstrap.macos": toml`
[bootstrap.macos.dock]
autohide = true
orientation = "left"
tilesize = 48

[bootstrap.macos.finder]
show_pathbar = true
preferred_view_style = "column"

[bootstrap.macos.keyboard]
key_repeat = 2
initial_key_repeat = 15

[bootstrap.macos.trackpad]
tap_to_click = true
`,
  "bootstrap.macos.dock": toml`
[bootstrap.macos.dock]
apps = ["/Applications/Safari.app", "~/Applications/Zed.app"]
autohide = true
orientation = "left"
tilesize = 48
show_recents = false
`,
  "bootstrap.macos.finder": toml`
[bootstrap.macos.finder]
show_all_files = true
show_pathbar = true
show_status_bar = true
preferred_view_style = "column"
`,
  "bootstrap.macos.keyboard": toml`
[bootstrap.macos.keyboard]
key_repeat = 2
initial_key_repeat = 15
press_and_hold = false
automatic_capitalization = false
`,
  "bootstrap.macos.trackpad": toml`
[bootstrap.macos.trackpad]
tap_to_click = true
three_finger_drag = true
`,
  "bootstrap.macos.defaults": toml`
[bootstrap.macos.defaults."com.apple.screencapture"]
location = "~/Screenshots"
type = "png"

[bootstrap.macos.defaults.NSGlobalDomain]
AppleShowAllExtensions = true
NSAutomaticQuoteSubstitutionEnabled = false
`,
  "bootstrap.macos.defaults.<key>": toml`
[bootstrap.macos.defaults."com.apple.screencapture"]
location = "~/Screenshots"
type = "png"
disable-shadow = true
`,
  "bootstrap.macos.defaults.<key>.<key>.<key>": toml`
[bootstrap.macos.defaults."com.apple.finder".FXInfoPanesExpanded]
General = true
Preview = false
`,
  "bootstrap.macos.defaults.<key>.<key>.<key>.<key>": toml`
[bootstrap.macos.defaults."com.apple.symbolichotkeys".AppleSymbolicHotKeys."64"]
enabled = false
`,
  "bootstrap.macos.defaults.<key>.<key>.<key>.<key>.<key>": toml`
[bootstrap.macos.defaults."com.apple.symbolichotkeys".AppleSymbolicHotKeys."64".value]
parameters = [32, 49, 1048576]
type = "standard"
`,
  "bootstrap.macos.defaults_entries": toml`
[[bootstrap.macos.defaults_entries]]
domain = "com.apple.finder"
key = "FXPreferredViewStyle"
value = "clmv"

[[bootstrap.macos.defaults_entries]]
domain = "com.apple.dock"
key = "persistent-apps"
host = "current"
value = []
`,
  "bootstrap.macos.launchd": toml`
[bootstrap.macos.launchd.agents.backup]
program = "/usr/local/bin/backup"
args = ["--quiet"]
start_calendar_interval = { hour = 2, minute = 30 }
`,
  "bootstrap.macos.launchd.agents": toml`
[bootstrap.macos.launchd.agents.sync]
program = "/usr/local/bin/sync-notes"
run_at_load = true
start_interval = 3600
`,
  "bootstrap.macos.launchd.agents.<key>": toml`
[bootstrap.macos.launchd.agents.backup]
program = "/usr/local/bin/backup"
args = ["--quiet"]
start_calendar_interval = { hour = 2, minute = 30 }
stdout_path = "~/Library/Logs/backup.log"
stderr_path = "~/Library/Logs/backup.err"
`,
  "bootstrap.macos.launchd.agents.<key>.start_calendar_interval": toml`
# Every Monday at 02:30
[bootstrap.macos.launchd.agents.backup]
program = "/usr/local/bin/backup"

[bootstrap.macos.launchd.agents.backup.start_calendar_interval]
weekday = 1
hour = 2
minute = 30
`,
  "bootstrap.macos.login_items": toml`
[bootstrap.macos.login_items]
apps = ["/Applications/Rectangle.app", "~/Applications/Raycast.app"]
exclusive = false
`,
  "bootstrap.linux": toml`
[bootstrap.linux.firewall]
default_incoming = "deny"

[[bootstrap.linux.firewall.rules]]
name = "ssh"
port = 22
protocol = "tcp"

[bootstrap.linux.systemd.units.sync-notes]
exec_start = "/usr/bin/rsync -a %h/notes /mnt/backup"
on_calendar = "daily"
`,
  "bootstrap.linux.firewall": toml`
[bootstrap.linux.firewall]
backend = "ufw"
default_incoming = "deny"
default_outgoing = "allow"

[[bootstrap.linux.firewall.rules]]
name = "ssh"
port = 22
protocol = "tcp"
action = "limit"

[[bootstrap.linux.firewall.rules]]
name = "https"
port = 443
protocol = "tcp"
`,
  "bootstrap.linux.firewall.rules": toml`
[[bootstrap.linux.firewall.rules]]
name = "postgres-from-vpn"
port = 5432
protocol = "tcp"
source = "10.8.0.0/24"
`,
  "bootstrap.linux.systemd": toml`
[bootstrap.linux.systemd.units.sync-notes]
description = "Sync notes"
exec_start = "/usr/bin/rsync -a %h/notes /mnt/backup"
on_calendar = "daily"
persistent = true
`,
  "bootstrap.linux.systemd.units": toml`
[bootstrap.linux.systemd.units.sync-notes]
exec_start = "/usr/bin/rsync -a %h/notes /mnt/backup"
type = "oneshot"
on_calendar = "daily"
`,
  "bootstrap.linux.systemd.units.<key>": toml`
[bootstrap.linux.systemd.units.sync-notes]
description = "Sync notes"
type = "oneshot"
exec_start = "/usr/bin/rsync -a %h/notes /mnt/backup"
on_calendar = "daily"
persistent = true
`,

  // ---------------------------------------------------------------- depends and run, as tables
  "tasks.<name>.depends.task": toml`
[tasks.release]
depends = [{ task = "build", args = ["--release"] }]
run = "cargo publish"
`,
  "tasks.<name>.depends.args": toml`
[tasks.release]
depends = [{ task = "build", args = ["--release"] }]
run = "cargo publish"
`,
  "tasks.<name>.depends.env": toml`
[tasks.release]
depends = [{ task = "build", env = { CARGO_INCREMENTAL = "0" } }]
run = "cargo publish"
`,
  "tasks.<name>.depends.env.<NAME>": toml`
[tasks.release]
depends = [{ task = "build", env = { CARGO_INCREMENTAL = "0" } }]
run = "cargo publish"
`,
  "tasks.<name>.depends.optional": toml`
# Run lint first if the project defines it; no error if it does not
[tasks.test]
depends = [{ task = "lint", optional = true }]
run = "cargo test"
`,
  "tasks.<name>.run.task": toml`
[tasks.release]
run = [
  { task = "build", args = ["--release"] },
  "cargo publish",
]
`,
  "tasks.<name>.run.args": toml`
[tasks.ci]
run = [{ task = "lint", args = ["--fix"] }]
`,
  "tasks.<name>.run.env": toml`
[tasks.ci]
run = [{ task = "test", env = { CI = "1" } }]
`,
  "tasks.<name>.run.env.<NAME>": toml`
[tasks.ci]
run = [{ task = "test", env = { CI = "1" } }]
`,
  "tasks.<name>.run.tasks": toml`
# Run these tasks in parallel, then the next entry
[tasks.ci]
run = [{ tasks = ["lint", "test"] }, "echo done"]
`,
  "bootstrap.macos.defaults.<key>.<key>.<key>.<key>.<key>.<key>": toml`
[bootstrap.macos.defaults."com.apple.symbolichotkeys".AppleSymbolicHotKeys."64".value]
parameters = [32, 49, 1048576]
type = "standard"
`,
};

// Sections a task, and a task template, can set for itself, which read the same as the
// top-level section of that name.
const taskSections = { tasks: "build", task_templates: "ci" } as const;

// The example for `path`: its own, else, for a key below a task's `tools`, `env` or `vars`,
// the top-level key's example written under a task, so `tasks.<name>.tools.python` shows
// `[tasks.build.tools]` where `tools.python` shows `[tools]`.
export function exampleFor(path: string): string | undefined {
  if (path in examples) return examples[path];
  // a task template takes the keys of a task
  const template = path.match(/^task_templates\.<name>\.(.+)$/);
  if (template && `tasks.<name>.${template[1]}` in examples) {
    return examples[`tasks.<name>.${template[1]}`].replace(
      /^(\[\[?)tasks\./gm,
      "$1task_templates.",
    );
  }
  const task = path.match(
    /^(tasks|task_templates)\.<name>\.((?:tools|env|vars)\..+)$/,
  );
  const base = task && exampleFor(task[2]);
  if (task && base !== undefined) {
    const prefix = `${task[1]}.${taskSections[task[1] as keyof typeof taskSections]}`;
    return base.replace(/^(\[\[?)(tools|env|vars)\b/gm, `$1${prefix}.$2`);
  }
  // a deps provider takes the keys of npm's
  const provider = path.match(/^deps\.([a-z-]+)$/);
  if (
    provider &&
    path !== "deps.npm" &&
    path !== "deps.disable" &&
    "deps.npm" in examples
  ) {
    return examples["deps.npm"].replace(
      /^\[deps\.npm\]/m,
      `[deps.${provider[1]}]`,
    );
  }
  // a backend's platform tables take that backend's own options
  const platforms = path.match(
    /^tools\.<([a-z0-9-]+):…>\.(platforms?)(\.<key>)?$/,
  );
  const option = platforms && platformOptions[platforms[1]];
  if (platforms && option) {
    const name = `${platforms[1]}:${option.repository}`;
    const table = (platform: string, value: string) =>
      `[tools."${name}".${platforms[2]}.${platform}]\n${option.key} = ${value}\n`;
    const linux = table("linux-x64", option.linux);
    return `[tools."${name}"]\nversion = "1.2.3"\n\n${
      platforms[3]
        ? linux
        : `${linux}\n${table("macos-arm64", option.macos ?? option.linux)}`
    }`;
  }
  return undefined;
}

// A repository and an option a platform can set, for each backend with platform tables:
// its value on Linux, and on macOS when it differs.
const platformOptions: Record<
  string,
  { repository: string; key: string; linux: string; macos?: string }
> = {
  github: {
    repository: "owner/tool",
    key: "asset_pattern",
    linux: '"tool-*-linux-amd64.tar.gz"',
    macos: '"tool-*-darwin-arm64.tar.gz"',
  },
  gitlab: {
    repository: "group/tool",
    key: "asset_pattern",
    linux: '"tool-*-linux-amd64.tar.gz"',
    macos: '"tool-*-darwin-arm64.tar.gz"',
  },
  forgejo: {
    repository: "owner/tool",
    key: "asset_pattern",
    linux: '"tool-*-linux-amd64.tar.gz"',
    macos: '"tool-*-darwin-arm64.tar.gz"',
  },
  ubi: { repository: "owner/tool", key: "bin_path", linux: '"bin"' },
  http: {
    repository: "my-tool",
    key: "url",
    linux: '"https://example.com/tool-1.2.3-linux-amd64.tar.gz"',
    macos: '"https://example.com/tool-1.2.3-darwin-arm64.tar.gz"',
  },
  s3: {
    repository: "my-tool",
    key: "url",
    linux: '"s3://my-bucket/tools/tool-1.2.3-linux-amd64.tar.gz"',
    macos: '"s3://my-bucket/tools/tool-1.2.3-darwin-arm64.tar.gz"',
  },
  cargo: { repository: "cargo-edit", key: "bin", linux: '"cargo-add"' },
  spm: { repository: "owner/tool", key: "filter_bins", linux: '["tool"]' },
};
