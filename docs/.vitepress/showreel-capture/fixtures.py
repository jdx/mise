#!/usr/bin/env python3
"""Write the showreel's fixture files from versions.json.

usage: fixtures.py VERSIONS.json OUT_DIR

These are the files a person would have written by hand. Nothing here is a
mise config file that trust would apply to: every project config starts as
the file `mise use` writes (steps.json runs those commands), and the pieces
below are then added to it, the way someone edits the file afterwards.
MiseToml::save trusts the root it writes, so no trust prompt or warning can
print later (v2 captures report, section 3).

{{name}} placeholders are filled from versions.json "placeholders". Scenes
must read the same values from versions.json, never hard-code them.

OUT_DIR/manifest.json lists every file with its sha256.

Every file gets the same modification time, FIXTURE_DATE, and record.py
copies it with that time; the off-camera git commits (C17's push, C16's
publish) are dated FIXTURE_DATE too. So two runs start from byte- and
time-identical trees, and what mise's sources/outputs check or git sees does
not depend on when a run happens.
"""
import calendar
import hashlib
import json
import os
import re
import sys
import time

# The date the person wrote the fixture files. Arbitrary, and fixed.
FIXTURE_DATE = "2026-09-01T09:00:00Z"
FIXTURE_EPOCH = calendar.timegm(time.strptime(FIXTURE_DATE, "%Y-%m-%dT%H:%M:%SZ"))

FILES = {
    # ~/.zshrc on machine 1: the prompt, completion, and mise. Nothing else,
    # because this file is shown on screen when it is tracked.
    "home/.zshrc": """\
PROMPT='%~ $ '
autoload -Uz compinit && compinit
eval "$(mise activate zsh)"
""",
    # appended to the mise.toml that `mise use node@{{lts_major}}` wrote
    "api/mise.toml.tail": """\

[env]
APP_ENV = "api"

[tasks.lint]
run = "node --check src/server.js"

[tasks.test]
run = "node --test --test-reporter=dot"

[tasks.build]
run = "mkdir -p dist && cp src/server.js dist/server.js"

[tasks.ci]
depends = ["lint", "test", "build"]
run = "echo \\"api ready: node $(node --version), APP_ENV=$APP_ENV\\""
""",
    "api/src/server.js": """\
module.exports = function greet(name) {
  return `hello, ${name}`;
};
""",
    "api/test/greet.test.js": """\
const test = require("node:test");
const assert = require("node:assert");
const greet = require("../src/server.js");

test("greets", () => assert.strictEqual(greet("api"), "hello, api"));
""",
    "api/resp.json": """\
{ "status": "ok", "items": 3 }
""",
    "api/README.md": """\
# api

A small Node service.

Run `mise run ci` to lint, test and build it.
""",
    "api/.github/workflows/ci.yml": """\
name: ci
on:
  pull_request:
  push:
    branches: [main]
jobs:
  ci:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
      - uses: jdx/mise-action@v4
        with:
          install_args: --locked
      - run: mise run ci
""",
    # .claude/skills holds `mise skills sync` links into this machine's
    # installs, which the packslip docs say to keep out of version control
    "api/.gitignore": """\
dist/
node_modules/
.env.deploy
.claude/skills/
""",
    # committed dotenv file, loaded with _.file (C9)
    "api/.env": """\
PORT=3000
""",
    # gitignored secrets, loaded with redact = true (C10). A placeholder
    # token, not a real credential format.
    "api/.env.deploy": """\
DEPLOY_TOKEN=tok_demo_4f9a2c71e8
""",
    # a file task with a usage spec (C10, C13)
    "api/mise-tasks/deploy": """\
#!/usr/bin/env bash
#MISE description="Deploy the api"
#USAGE arg "<env>" help="Where to deploy" {
#USAGE   choices "staging" "production"
#USAGE }
echo "deploying to $usage_env with token $DEPLOY_TOKEN"
""",
    # appended to the mise.toml that `mise use pitchfork@...` wrote (C14).
    # No [settings]: experimental is enabled off camera, in the environment.
    "shop/mise.toml.tail": """\

[daemons]
postgres = "{{postgres_major}}"

[daemons_settings]
namespace = "shop"

[tasks.db]
daemons = "postgres"
run = 'psql -Atc "show server_version"'
""",
    # appended to the global config before it is tracked (C16), only when
    # machine 2 has a service manager to start the watcher with
    "home/config.toml.services": """\

[bootstrap.services.mise-history]
builtin = "history-watch"
""",
}

EXECUTABLE = {"api/mise-tasks/deploy"}

PLACEHOLDER = re.compile(r"\{\{([a-z0-9_]+)\}\}")


def fill(text, values):
    def sub(m):
        if m.group(1) not in values:
            raise KeyError(f"unknown placeholder {{{{{m.group(1)}}}}}")
        return values[m.group(1)]
    return PLACEHOLDER.sub(sub, text)


def main():
    versions = json.load(open(sys.argv[1]))
    out = sys.argv[2]
    values = versions["placeholders"]
    manifest = {}
    for rel, tmpl in FILES.items():
        body = fill(tmpl, values)
        path = os.path.join(out, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w") as f:
            f.write(body)
        if rel in EXECUTABLE:
            os.chmod(path, 0o755)
        os.utime(path, (FIXTURE_EPOCH, FIXTURE_EPOCH))
        manifest[rel] = hashlib.sha256(body.encode()).hexdigest()
    with open(os.path.join(out, "manifest.json"), "w") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")


if __name__ == "__main__":
    main()
