// The mise.toml snippet the reference shows for a key. A hand-written example for the key
// (config-reference-examples.ts) is used when there is one. Otherwise the snippet is written
// from the key's type: where the key goes, with a placeholder value of its type. Names the
// reader picks (`<name>`, `<tool>`) become example names so the snippet reads like a real
// config.

import { exampleFor } from "./config-reference-examples.ts";
import { valueFor } from "./config-reference-values.ts";

export type SnippetKey = {
  path: string;
  type: string;
  choices: string[];
  default?: string;
  deprecated: boolean;
  // the alternative forms of its table that have this key; absent when every form does
  forms?: number[];
  children: SnippetKey[];
};

// Example names for the placeholders in a key's path, by the key above the placeholder.
const examples: Record<string, string> = {
  tasks: "build",
  task_templates: "ci",
  tools: "node",
  env: "NODE_ENV",
  vars: "region",
  alias: "node",
  tool_alias: "node",
  plugins: "my-plugin",
  shell_alias: "ll",
  wrappers: "cargo",
  daemons: "postgres",
  daemon_groups: "backend",
  daemon_providers: "postgres",
  dotfiles: "~/.gitconfig",
  dotfile_groups: "nvim",
  platforms: "linux-x64",
  platform: "linux-x64",
  install_env: "MAKEFLAGS",
  headers: "Authorization",
  headers_forward: "Authorization",
  reload: "~/.zshrc",
  labels: "org.opencontainers.image.source",
  input_groups: "lockfiles",
  path_aliases: "web",
  ports: "replication",
  environment: "PORT",
  metadata: "owner",
  options: "database",
  versions: "lts",
  entries: "init.lua",
  rename_exe: "tool-*",
  checks: "database",
  projects: "packages/web",
  task_defaults: "build",
};

// Example names that depend on where the placeholder is, by the path above it, for sections
// that use the same placeholder for different things.
const pathExamples: Record<string, string> = {
  "tools.<aqua:…>.vars": "flavor",
  "tool_alias.<tool>": "lts",
  "bootstrap.files": "/etc/motd",
  "bootstrap.directories": "/srv/app",
  "bootstrap.remote.hosts": "build",
  "bootstrap.services": "notes-sync",
  "bootstrap.compose": "app",
  "bootstrap.packages": "brew:ripgrep",
  "bootstrap.repos": "~/src/mise",
  "bootstrap.macos.defaults": "com.apple.dock",
  "bootstrap.macos.defaults.<key>": "autohide",
  "bootstrap.macos.launchd.agents": "backup",
  "bootstrap.linux.systemd.units": "sync-notes",
  "bootstrap.apt.sources": "docker",
  "bootstrap.dnf.repos": "ghostty",
  "bootstrap.brew.taps": "my-org",
  "bootstrap.secrets": "GITHUB_TOKEN",
  "bootstrap.groups": "deploy",
  "bootstrap.users": "deploy",
};

// `<github:…>` is `github:cli/cli`; the rest of the backends get a generic name.
const backendExamples: Record<string, string> = {
  github: "github:cli/cli",
  gitlab: "gitlab:gitlab-org/cli",
  forgejo: "forgejo:forgejo/runner",
  aqua: "aqua:BurntSushi/ripgrep",
  cargo: "cargo:ripgrep",
  npm: "npm:prettier",
  pypi: "pypi:ruff",
  pipx: "pipx:black",
  gem: "gem:rubocop",
  go: "go:golang.org/x/tools/gopls",
  http: "http:my-tool",
  s3: "s3:my-tool",
  ubi: "ubi:goreleaser/goreleaser",
  spm: "spm:nicklockwood/SwiftFormat",
  packslip: "packslip:github.com/jdx/hk",
  spinel: "spinel:tobi/try",
  conda: "conda:ffmpeg",
};

// Sections whose entries are usually one line (`node = "22"`), even where a table works too.
const shortForm = new Set([
  "tools",
  "tool_alias",
  "alias",
  "plugins",
  "shell_alias",
  "env",
  "vars",
]);

// Keys a table's snippet lists first, when it has them.
const preferred = [
  "run",
  "version",
  "source",
  "description",
  "depends",
  "dir",
  "env",
  "file",
  "path",
];

function exampleName(
  segment: string,
  parent: string | undefined,
  parentPath = "",
): string {
  const backend = segment.match(/^<([a-z0-9-]+):…>$/);
  if (backend) return backendExamples[backend[1]] ?? `${backend[1]}:example`;
  if (!/^<.*>$/.test(segment)) return segment;
  if (pathExamples[parentPath]) return pathExamples[parentPath];
  if (parent && examples[parent]) return examples[parent];
  if (segment === "<NAME>") return "EXAMPLE";
  return "example";
}

const bare = (key: string) => /^[A-Za-z0-9_-]+$/.test(key);
const tomlKey = (key: string) => (bare(key) ? key : JSON.stringify(key));

const segments = (path: string) => path.split(".");
const own = (key: SnippetKey) => segments(key.path).at(-1)!;
const unlist = (segment: string) => segment.replace(/\[\]$/, "");

// The alternatives of a type label, split at its top level: `(string | table)[] | string`
// is `(string | table)[]` and `string`.
function alternatives(type: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < type.length; i++) {
    const c = type[i];
    if (c === "(") depth++;
    else if (c === ")") depth--;
    else if (c === "|" && depth === 0) {
      out.push(type.slice(start, i).trim());
      start = i + 1;
    }
  }
  out.push(type.slice(start).trim());
  return out;
}

function toml(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(toml).join(", ")}]`;
  if (value && typeof value === "object") {
    const entries = Object.entries(value).map(
      ([k, v]) => `${tomlKey(k)} = ${toml(v)}`,
    );
    return entries.length > 0 ? `{ ${entries.join(", ")} }` : "{}";
  }
  return JSON.stringify(value);
}

// A placeholder value for one alternative of a type.
function placeholder(type: string): string {
  if (type.endsWith("[]")) {
    const item = type.slice(0, -2).replace(/^\((.*)\)$/, "$1");
    return `[${placeholder(alternatives(item)[0])}]`;
  }
  switch (type) {
    case "boolean":
    case "true":
      return "true";
    case "false":
      return "false";
    case "integer":
    case "number":
      return "1";
    case "table":
      return "{ … }";
    default:
      return /^".*"$/.test(type) ? type : '"…"';
  }
}

// Values for string keys that are named for what they hold, wherever they appear, so the
// generated snippet reads `timeout = "5m"` rather than `timeout = "…"`.
const stringHints: Record<string, string> = {
  description: "Run the tests",
  hint: "Run `mise install` first",
  message: "Continue?",
  dir: "{{ config_root }}/app",
  shell: "bash -c",
  timeout: "5m",
  file: "scripts/build.sh",
  path: "./bin",
  prefix: "v",
  source: "config/settings.toml",
  target: "~/.config/app",
  root: "config",
  url: "https://example.com/app.tar.gz",
  api_url: "https://github.example.com/api/v3",
  version: "latest",
  tag: "ghcr.io/my-org/app:latest",
  from: "debian:bookworm-slim",
  profile: "default",
  region: "us-east-1",
  branch: "main",
  backend: "aqua:owner/tool",
  checksum: "sha256:…",
  bin: "tool",
  bin_path: "bin",
  asset_pattern: "tool-*-linux-amd64.tar.gz",
  matching: "linux",
  command: "cargo",
  run: "echo hello",
  task: "build",
  skip_if: "test -f dist/app",
  usage: 'arg "<file>"',
  cwd: "{{ config_root }}",
};

// A placeholder for a string, from the key's name; `"…"` when it gives no hint.
function stringValue(name: string): string {
  const hint = stringHints[name];
  return hint === undefined ? '"…"' : JSON.stringify(hint);
}

function value(key: SnippetKey, name = own(key)): string {
  const written = valueFor(key.path);
  if (written !== undefined) return written;
  if (key.default !== undefined) {
    try {
      return toml(JSON.parse(key.default));
    } catch {
      return key.default;
    }
  }
  const types = alternatives(key.type);
  if (["boolean", "integer", "number", "true", "false"].includes(types[0])) {
    return placeholder(types[0]);
  }
  // `choices` are written as TOML already
  if (key.choices.length > 0) return key.choices[0];
  // a key that takes a list or a table often takes a plain string too, and that reads best
  const type = types.includes("string") ? "string" : types[0];
  const item = type.endsWith("[]")
    ? alternatives(type.slice(0, -2).replace(/^\((.*)\)$/, "$1"))[0]
    : type;
  if (item === "string") {
    const hint = stringValue(unlist(name));
    if (hint !== '"…"') return type.endsWith("[]") ? `[${hint}]` : hint;
  }
  return placeholder(type);
}

const isTableType = (t: string) => t === "table" || t === "table[]";
// whether `key` can be a table, and whether it can be written as `key = value` without one
const takesTable = (key: SnippetKey) =>
  alternatives(key.type).some(isTableType);
const scalarForm = (key: SnippetKey) =>
  alternatives(key.type).some((t) => !isTableType(t));
// the key below an open table that stands for the names the reader picks
const isOpen = (key: SnippetKey) => /<[^>]*>$/.test(key.path);
// `<github:…>`: options a backend adds, not a key anyone writes
const isPattern = (key: SnippetKey) => /<[a-z0-9-]+:…>$/.test(key.path);

function openEntry(key: SnippetKey): SnippetKey | undefined {
  return key.children.find((c) => isOpen(c) && !isPattern(c));
}

// `key` as one line inside its table: `run = "…"`, `_.file = "…"`, `platforms = { … }`.
// `prefix` is a dotted key already written out.
function entryLine(key: SnippetKey, name: string, prefix = ""): string {
  const written = prefix + tomlKey(name);
  if (scalarForm(key) || valueFor(key.path) !== undefined)
    return `${written} = ${value(key, name)}`;
  // `[env]`'s `_` holds directives, written `_.file = …`
  const first = key.children.find((c) => !isOpen(c) && !c.deprecated);
  if (name === "_" && first) return entryLine(first, own(first), `${written}.`);
  return `${written} = { … }`;
}

// The lines for `key`'s own children, as they would sit below its header.
function childLines(key: SnippetKey, limit = 4): string[] {
  const lines: string[] = [];
  const open = openEntry(key);
  if (open) {
    const segs = segments(open.path);
    lines.push(
      entryLine(
        open,
        exampleName(segs.at(-1)!, segs.at(-2), segs.slice(0, -1).join(".")),
      ),
    );
  }
  const rank = (c: SnippetKey) => {
    const i = preferred.indexOf(own(c));
    return i < 0 ? preferred.length : i;
  };
  const named = key.children
    .filter((c) => !isOpen(c) && !c.deprecated)
    .map((c, i) => ({ c, i }))
    .sort(
      (a, b) =>
        rank(a.c) - rank(b.c) ||
        Number(!scalarForm(a.c)) - Number(!scalarForm(b.c)) ||
        a.i - b.i,
    )
    .map(({ c }) => c);
  // keys of one form only: a `[[watch_files]]` entry takes `run` or `task`, not both
  const form = named.find((c) => c.forms)?.forms?.[0];
  const fits = named.filter(
    (c) => !c.forms || form === undefined || c.forms.includes(form),
  );
  for (const child of fits.slice(0, limit - lines.length)) {
    lines.push(entryLine(child, unlist(own(child))));
  }
  const total = key.children.filter(
    (c) => !isPattern(c) && !c.deprecated,
  ).length;
  if (total > lines.length || total === 0) lines.push("# …");
  return lines;
}

// The header for the table at `segs`, such as `[tasks.build]` or
// `[[bootstrap.macos.defaults_entries]]`, and the prefix the table's own lines need
// (`_.` in `[env]`).
function header(segs: string[]): { line?: string; prefix: string } {
  if (segs.length === 0) return { prefix: "" };
  // `env._.file` is written `_.file` inside `[env]`, as the docs do
  const underscore = segs.indexOf("_");
  if (underscore > 0) {
    const { line } = header(segs.slice(0, underscore));
    return { line, prefix: `${segs.slice(underscore).join(".")}.` };
  }
  const names = segs.map((s, i) =>
    tomlKey(
      exampleName(
        unlist(s),
        segs[i - 1] && unlist(segs[i - 1]),
        segs.slice(0, i).join("."),
      ),
    ),
  );
  const joined = names.join(".");
  return {
    line: segs.at(-1)!.endsWith("[]") ? `[[${joined}]]` : `[${joined}]`,
    prefix: "",
  };
}

// `key` written as a table of its own: `[tasks.build]` and some of its keys.
function tableForm(key: SnippetKey): string[] {
  const segs = segments(key.path);
  // an open table whose entries are mostly tables, such as `[tasks]`: show one entry
  const open = openEntry(key);
  const named = key.children.filter((c) => !isOpen(c));
  if (
    open &&
    named.length === 0 &&
    takesTable(open) &&
    !shortForm.has(segs[0])
  ) {
    return tableForm(open);
  }
  const list =
    !alternatives(key.type).includes("table") && key.type.includes("table[]");
  const { line, prefix } = header(
    list ? [...segs.slice(0, -1), `${own(key)}[]`] : segs,
  );
  const lines: string[] = line ? [line] : [];
  // a tool's table names its version, which `<github:…>` and the like list under `<tool>`
  if (
    segs[0] === "tools" &&
    segs.length === 2 &&
    !key.children.some((c) => own(c) === "version")
  ) {
    lines.push('version = "latest"');
  }
  lines.push(
    ...childLines(key).map((l) => (l.startsWith("#") ? l : prefix + l)),
  );
  return lines;
}

// `key = value` in the table above it.
function lineForm(key: SnippetKey): string[] {
  const segs = segments(key.path);
  const parent = segs.slice(0, -1);
  const { line, prefix } = header(parent);
  const lines: string[] = line ? [line] : [];
  // a tool's options sit beside its version
  if (segs[0] === "tools" && segs.length === 3 && own(key) !== "version") {
    lines.push('version = "latest"');
  }
  const name = exampleName(
    unlist(own(key)),
    parent.at(-1) && unlist(parent.at(-1)!),
    parent.join("."),
  );
  lines.push(prefix + entryLine(key, name));
  return lines;
}

// Whether the reference has a hand-written example for `path`.
export function hasExample(path: string): boolean {
  return exampleFor(path) !== undefined;
}

export function snippet(key: SnippetKey): string {
  const example = exampleFor(key.path);
  if (example !== undefined) return example.trimEnd();
  return generated(key);
}

// The snippet written from the key's type alone.
export function generated(key: SnippetKey): string {
  const segs = segments(key.path);
  if (!takesTable(key)) return lineForm(key).join("\n");
  if (!scalarForm(key)) return tableForm(key).join("\n");
  const entry = segs.length === 2 && isOpen(key);
  // The second form is shown as a comment: both written out would define the key twice.
  const commented = (lines: string[]) =>
    lines.map((line) => (line === "" ? "#" : `# ${line}`));
  // a task is usually a table; a tool is usually a version
  if (entry && !shortForm.has(segs[0])) {
    return [
      ...tableForm(key),
      "",
      "# or, in short:",
      ...commented(lineForm(key)),
    ].join("\n");
  }
  return [
    ...lineForm(key),
    "",
    "# or, as a table:",
    ...commented(tableForm(key)),
  ].join("\n");
}
