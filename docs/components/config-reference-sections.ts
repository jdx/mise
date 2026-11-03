// How the mise.toml reference groups its top-level sections, and the guide that explains
// each one. The page and its Markdown copy both read this; the keys themselves come from
// schema/mise.json. A section missing here is listed under "Other", so a new top-level key
// still shows up, but give it a group and a guide.

export type Guide = { text: string; link: string };

export type Group = { title: string; sections: string[] };

export const groups: Group[] = [
  {
    title: "Tools",
    sections: ["tools", "tool_alias", "tool_config", "plugins", "wrappers"],
  },
  {
    title: "Environment",
    sections: ["env", "vars", "redactions", "secrets"],
  },
  {
    title: "Tasks",
    sections: [
      "tasks",
      "task_config",
      "task_templates",
      "monorepo",
      "monorepo_root",
    ],
  },
  {
    title: "Hooks and shell",
    sections: ["hooks", "watch_files", "shell_alias"],
  },
  {
    title: "Config file",
    sections: ["settings", "include", "min_version", "doctor", "_"],
  },
  {
    title: "Project services",
    sections: [
      "deps",
      "oci",
      "daemons",
      "daemon_groups",
      "daemons_settings",
      "daemon_providers",
    ],
  },
  {
    title: "Machine setup",
    sections: ["bootstrap", "dotfiles", "dotfile_groups", "history"],
  },
];

// The page that explains a section, as listed in the table of sections on /configuration.html.
export const guides: Record<string, Guide> = {
  _: { text: "Free-form data", link: "/configuration.html#free-form-data" },
  alias: { text: "Tool aliases", link: "/dev-tools/aliases.html" },
  bootstrap: { text: "Bootstrap", link: "/bootstrap.html" },
  daemon_groups: { text: "Daemon groups", link: "/daemons.html#groups" },
  daemon_providers: {
    text: "Share daemons across projects",
    link: "/daemons/sharing.html",
  },
  daemons: { text: "Daemons", link: "/daemons.html" },
  daemons_settings: {
    text: "Ports, URLs, and worktrees",
    link: "/daemons/worktrees.html#namespaces",
  },
  deps: { text: "Project dependencies", link: "/dev-tools/deps.html" },
  doctor: {
    text: "Project diagnostics",
    link: "/configuration/project-diagnostics.html",
  },
  dotenv: { text: "Environment variables", link: "/environments/" },
  dotfile_groups: { text: "Dotfile groups", link: "/dotfiles/groups.html" },
  dotfiles: { text: "Dotfiles", link: "/dotfiles.html" },
  env: { text: "Environment variables", link: "/environments/" },
  env_file: { text: "Environment variables", link: "/environments/" },
  env_path: { text: "Environment variables", link: "/environments/" },
  experimental_monorepo_root: {
    text: "Monorepo tasks",
    link: "/tasks/monorepo.html",
  },
  history: { text: "Dotfiles history", link: "/dotfiles/history.html" },
  hooks: { text: "Hooks", link: "/hooks.html" },
  include: { text: "include", link: "/configuration.html#include" },
  min_version: {
    text: "Minimum mise version",
    link: "/configuration.html#minimum-mise-version",
  },
  monorepo: { text: "Monorepo tasks", link: "/tasks/monorepo.html" },
  monorepo_root: { text: "Monorepo tasks", link: "/tasks/monorepo.html" },
  oci: { text: "OCI images", link: "/dev-tools/mise-oci.html" },
  plugins: { text: "Plugins", link: "/configuration.html#plugins" },
  redactions: {
    text: "Redaction",
    link: "/environments/secrets/#redaction",
  },
  secrets: { text: "fnox", link: "/environments/secrets/fnox.html" },
  settings: { text: "Settings", link: "/configuration/settings.html" },
  shell_alias: { text: "Shell aliases", link: "/shell-aliases.html" },
  task_config: {
    text: "Task configuration",
    link: "/tasks/task-configuration.html#task-config-options",
  },
  task_templates: { text: "Task templates", link: "/tasks/templates.html" },
  tasks: { text: "Tasks", link: "/tasks/" },
  tool_alias: { text: "Tool aliases", link: "/dev-tools/aliases.html" },
  tool_config: {
    text: "Tool policy",
    link: "/configuration.html#tool-config",
  },
  tools: { text: "Dev tools", link: "/dev-tools/" },
  vars: { text: "Config variables", link: "/configuration/vars.html" },
  watch_files: { text: "Hooks", link: "/hooks.html" },
  wrappers: {
    text: "Command wrappers",
    link: "/dev-tools/shims.html#command-wrappers",
  },
};

type Named = { path: string; deprecated: boolean };

// `keys` in the groups above, with deprecated sections in a group of their own at the end
// and any section no group names under "Other".
export function grouped<K extends Named>(
  keys: K[],
): { title: string; keys: K[] }[] {
  const byPath = new Map(keys.map((k) => [k.path, k]));
  const placed = new Set<string>();
  const out: { title: string; keys: K[] }[] = [];
  for (const group of groups) {
    const members = group.sections
      .map((s) => byPath.get(s))
      .filter((k): k is K => k !== undefined && !k.deprecated);
    members.forEach((k) => placed.add(k.path));
    if (members.length > 0) out.push({ title: group.title, keys: members });
  }
  const other = keys.filter((k) => !placed.has(k.path) && !k.deprecated);
  if (other.length > 0) out.push({ title: "Other", keys: other });
  const deprecated = keys.filter((k) => k.deprecated);
  if (deprecated.length > 0)
    out.push({ title: "Deprecated", keys: deprecated });
  return out;
}

// Guides for a key more specific than its section's. A key without one uses the guide of
// the nearest key above it that has one, and then its section's.
const keyGuides: Record<string, Guide> = {
  "tools.<tool>": { text: "Tool options", link: "/dev-tools/#tool-options" },
  "tools.<tool>.version": {
    text: "Version requests",
    link: "/dev-tools/versions.html#request-syntax",
  },
  "tools.<tool>.os": {
    text: "OS-specific tools",
    link: "/dev-tools/#os-specific-tools",
  },
  "tools.<tool>.depends": {
    text: "Tool dependencies",
    link: "/dev-tools/#tool-dependencies",
  },
  "tools.<tool>.postinstall": {
    text: "Tool postinstall commands",
    link: "/dev-tools/#tool-postinstall-commands",
  },
  "tools.<tool>.install_env": {
    text: "Variables in tool options",
    link: "/dev-tools/#templates-in-tool-configuration",
  },
  "tools.<tool>.auto_update": {
    text: "Automatic tool updates",
    link: "/dev-tools/#automatic-tool-updates",
  },
  "tools.<tool>.lazy": {
    text: "Lazy tools",
    link: "/dev-tools/shims.html#lazy-tools",
  },
  "tools.<tool>.version_order": {
    text: "Version ordering",
    link: "/dev-tools/versions.html#version-ordering",
  },
  "tools.<github:…>": {
    text: "The github backend",
    link: "/dev-tools/backends/github.html#tool-options",
  },
  "tools.<github:…>.asset_pattern": {
    text: "Selecting GitHub assets",
    link: "/dev-tools/backends/github.html#selecting-assets",
  },
  "tools.<github:…>.platforms": {
    text: "Per-platform options",
    link: "/dev-tools/backends/github.html#per-platform-options",
  },
  "tools.<github:…>.version_prefix": {
    text: "Choosing versions",
    link: "/dev-tools/backends/github.html#choosing-versions",
  },
  "tools.<github:…>.bin": {
    text: "Extracting and naming executables",
    link: "/dev-tools/backends/github.html#extracting-and-naming-executables",
  },
  "tools.<gitlab:…>": {
    text: "The gitlab backend",
    link: "/dev-tools/backends/gitlab.html",
  },
  "tools.<forgejo:…>": {
    text: "The forgejo backend",
    link: "/dev-tools/backends/forgejo.html",
  },
  "tools.<ubi:…>": {
    text: "The ubi backend",
    link: "/dev-tools/backends/ubi.html",
  },
  "tools.<http:…>": {
    text: "The http backend",
    link: "/dev-tools/backends/http.html",
  },
  "tools.<s3:…>": {
    text: "The s3 backend",
    link: "/dev-tools/backends/s3.html",
  },
  "tools.<aqua:…>": {
    text: "The aqua backend",
    link: "/dev-tools/backends/aqua.html",
  },
  "tools.<cargo:…>": {
    text: "The cargo backend",
    link: "/dev-tools/backends/cargo.html",
  },
  "tools.<npm:…>": {
    text: "The npm backend",
    link: "/dev-tools/backends/npm.html",
  },
  "tools.<pypi:…>": {
    text: "The pypi backend",
    link: "/dev-tools/backends/pypi.html",
  },
  "tools.<pipx:…>": {
    text: "The pypi backend",
    link: "/dev-tools/backends/pypi.html",
  },
  "tools.<gem:…>": {
    text: "The gem backend",
    link: "/dev-tools/backends/gem.html",
  },
  "tools.<go:…>": {
    text: "The go backend",
    link: "/dev-tools/backends/go.html",
  },
  "tools.<conda:…>": {
    text: "The conda backend",
    link: "/dev-tools/backends/conda.html",
  },
  "tools.<spm:…>": {
    text: "The spm backend",
    link: "/dev-tools/backends/spm.html",
  },
  "tools.<packslip:…>": {
    text: "The packslip backend",
    link: "/dev-tools/backends/packslip.html",
  },
  "tools.<spinel:…>": {
    text: "The spinel backend",
    link: "/dev-tools/backends/spinel.html",
  },
  "tools.python": { text: "Python", link: "/lang/python.html" },
  "tools.java": { text: "Java", link: "/lang/java.html" },
  "tools.rust": { text: "Rust", link: "/lang/rust.html" },
  "tools.dotnet": { text: ".NET", link: "/dev-tools/backends/dotnet.html" },
  "env._": { text: "Env directives", link: "/environments/#env-directives" },
  "env._.file": { text: "env._.file", link: "/environments/#env-file" },
  "env._.path": { text: "env._.path", link: "/environments/#env-path" },
  "env._.source": { text: "env._.source", link: "/environments/#env-source" },
  "env._.python.venv": {
    text: "env._.python.venv",
    link: "/environments/#env-python-venv",
  },
  "env.<NAME>.default": {
    text: "Defaults",
    link: "/environments/#defaults",
  },
  "env.<NAME>.required": {
    text: "Required variables",
    link: "/environments/#required-variables",
  },
  "env.<NAME>.redact": {
    text: "Redaction",
    link: "/environments/secrets/#redaction",
  },
  "env.<NAME>.tools": {
    text: "Values that tools set",
    link: "/environments/#lazy-eval",
  },
  "vars.<name>.prompt": {
    text: "Ask once and remember",
    link: "/configuration/vars.html#prompt",
  },
  "hooks.enter": {
    text: "Directory hooks",
    link: "/hooks.html#directory-hooks",
  },
  "hooks.leave": {
    text: "Directory hooks",
    link: "/hooks.html#directory-hooks",
  },
  "hooks.cd": { text: "Directory hooks", link: "/hooks.html#directory-hooks" },
  "hooks.preinstall": {
    text: "Install hooks",
    link: "/hooks.html#preinstall-postinstall-hook",
  },
  "hooks.postinstall": {
    text: "Install hooks",
    link: "/hooks.html#preinstall-postinstall-hook",
  },
  watch_files: { text: "Watch files", link: "/hooks.html#watch-files-hook" },
  "tasks.<name>.run": {
    text: "Task commands",
    link: "/tasks/task-configuration.html#run",
  },
  "daemons.<name>.port": {
    text: "Ports, URLs, and worktrees",
    link: "/daemons/worktrees.html#automatic-ports",
  },
  "monorepo.path_aliases": {
    text: "Path aliases",
    link: "/tasks/monorepo.html#short-names-for-project-paths",
  },
  "monorepo.config_roots": {
    text: "Config roots",
    link: "/tasks/monorepo.html#explicit-config-roots",
  },
  "oci.copy": {
    text: "Copy files into the image",
    link: "/dev-tools/mise-oci.html#copy-files-into-the-image",
  },
};

// The ids of the headings in /tasks/task-configuration.html, by the property they describe.
const taskProperties: Record<string, string> = {
  run: "run",
  run_windows: "run-windows",
  file: "file",
  shell: "shell",
  dir: "dir",
  usage: "usage",
  raw_args: "raw-args",
  extends: "extends",
  description: "description",
  alias: "alias",
  hide: "hide",
  confirm: "confirm",
  depends: "depends",
  depends_post: "depends-post",
  wait_for: "wait-for",
  daemons: "daemons",
  env: "env",
  vars: "task-vars",
  secrets: "secrets",
  tools: "tools",
  skip_if: "skip-if",
  sources: "sources",
  outputs: "outputs",
  watch: "watch",
  cache: "cache",
  timeout: "timeout",
  raw: "raw",
  interactive: "interactive",
  output: "output",
  quiet: "quiet",
  silent: "silent",
};

function ruleGuide(path: string): Guide | undefined {
  const task = path.match(/^tasks\.<name>\.([a-z_]+)$/);
  const id = task && taskProperties[task[1]];
  if (id) {
    return {
      text: `The ${task[1]} property`,
      link: `/tasks/task-configuration.html#${id}`,
    };
  }
  const config = path.match(/^task_config\.([a-z_]+)$/);
  if (config && config[1] in taskConfigIds) {
    return {
      text: `task_config.${config[1]}`,
      link: `/tasks/task-configuration.html#task_config.${config[1]}`,
    };
  }
  return undefined;
}

const taskConfigIds: Record<string, true> = {
  cascade: true,
  dir: true,
  shell: true,
  includes: true,
  excludes: true,
  cache: true,
  global_inputs: true,
  input_groups: true,
  global_env: true,
  global_pass_through_env: true,
};

// The guide that explains `path`, a key at any depth, and failing that its section.
export function guideFor(path: string): Guide | undefined {
  // `watch_files[].run` is a key of `watch_files`, so a list marker does not change the guide
  const segments = path.split(".").map((s) => s.replace(/\[\]$/, ""));
  for (let n = segments.length; n > 0; n--) {
    const above = segments.slice(0, n).join(".");
    const guide = keyGuides[above] ?? ruleGuide(above);
    if (guide) return guide;
  }
  return guides[segments[0]];
}

// Every guide link the reference can show, for the test that they still lead somewhere.
export function allGuides(): Guide[] {
  const taskGuides = Object.keys(taskProperties).flatMap((p) => {
    const guide = ruleGuide(`tasks.<name>.${p}`);
    return guide ? [guide] : [];
  });
  const configGuides = Object.keys(taskConfigIds).flatMap((p) => {
    const guide = ruleGuide(`task_config.${p}`);
    return guide ? [guide] : [];
  });
  return [
    ...Object.values(guides),
    ...Object.values(keyGuides),
    ...taskGuides,
    ...configGuides,
  ];
}
