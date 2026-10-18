// The mise.toml snippet the reference shows for a key: where the key goes, with a placeholder
// value of its type. Names the reader picks (`<name>`, `<tool>`) become example names so the
// snippet reads like a real config.

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
};

// `<github:…>` is `github:cli/cli`; the rest of the backends get a generic name.
const backendExamples: Record<string, string> = {
  github: "github:cli/cli",
  gitlab: "gitlab:gitlab-org/cli",
  forgejo: "forgejo:example/tool",
  aqua: "aqua:BurntSushi/ripgrep",
  cargo: "cargo:ripgrep",
  npm: "npm:prettier",
  pypi: "pypi:ruff",
  pipx: "pipx:black",
  gem: "gem:rubocop",
  go: "go:golang.org/x/tools/gopls",
  http: "http:my-tool",
  s3: "s3:my-tool",
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

function exampleName(segment: string, parent: string | undefined): string {
  const backend = segment.match(/^<([a-z0-9-]+):…>$/);
  if (backend) return backendExamples[backend[1]] ?? `${backend[1]}:example`;
  if (!/^<.*>$/.test(segment)) return segment;
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

function value(key: SnippetKey): string {
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
  return placeholder(types.includes("string") ? "string" : types[0]);
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
  if (scalarForm(key)) return `${written} = ${value(key)}`;
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
    lines.push(entryLine(open, exampleName(segs.at(-1)!, segs.at(-2))));
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
    tomlKey(exampleName(unlist(s), segs[i - 1] && unlist(segs[i - 1]))),
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
  );
  lines.push(prefix + entryLine(key, name));
  return lines;
}

export function snippet(key: SnippetKey): string {
  const segs = segments(key.path);
  if (!takesTable(key)) return lineForm(key).join("\n");
  if (!scalarForm(key)) return tableForm(key).join("\n");
  const entry = segs.length === 2 && isOpen(key);
  // a task is usually a table; a tool is usually a version
  if (entry && !shortForm.has(segs[0])) {
    return [...tableForm(key), "", "# or, in short:", ...lineForm(key)].join(
      "\n",
    );
  }
  return [...lineForm(key), "", "# or, as a table:", ...tableForm(key)].join(
    "\n",
  );
}

const escape = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

// The snippet as HTML with a span around each kind of token, for the page's own colors.
export function highlight(text: string): string {
  return text
    .split("\n")
    .map((line) => {
      if (line.startsWith("#"))
        return `<span class="t-comment">${escape(line)}</span>`;
      if (line.startsWith("["))
        return `<span class="t-header">${escape(line)}</span>`;
      const eq = line.indexOf(" = ");
      if (eq < 0) return escape(line);
      const valueHtml = escape(line.slice(eq + 3)).replace(
        /("(?:[^"\\]|\\.)*")|\b(true|false|\d+)\b/g,
        (m, str) =>
          str
            ? `<span class="t-string">${m}</span>`
            : `<span class="t-literal">${m}</span>`,
      );
      return `<span class="t-key">${escape(line.slice(0, eq))}</span> = ${valueHtml}`;
    })
    .join("\n");
}
