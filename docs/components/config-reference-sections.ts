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
