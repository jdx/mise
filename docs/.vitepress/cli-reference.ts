// Add mise-specific website navigation after usage generates docs/cli.
// Command prose, arguments, flags, and visibility still come from mise.usage.kdl.
import { execFileSync } from "node:child_process";
import {
  existsSync,
  readdirSync,
  readFileSync,
  rmdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { pageDescription } from "./social-descriptions.mjs";

export interface Command {
  full_cmd: string[];
  usage: string;
  help?: string;
  hide: boolean;
  subcommands: Record<string, Command>;
}

const docsDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const navigationMarker = "<!-- generated reference navigation -->";

// Longest matching command path wins; descendants share their resource's guide.
const guides: Record<string, [string, string]> = {
  "bootstrap unapply": [
    "Removing a module",
    "/bootstrap/modules.html#remove-a-module-s-resources",
  ],
  "bootstrap status": [
    "Inspecting bootstrap state",
    "/bootstrap.html#inspecting-state",
  ],
  "bootstrap plan": [
    "Previewing bootstrap",
    "/bootstrap.html#plan-declarative-resources",
  ],
  "plugins ls": ["Installing plugins", "/plugins.html#installing-plugins"],
  "plugins uninstall": ["Removing plugins", "/plugins.html#remove-plugins"],
  "plugins update": ["Updating plugins", "/plugins.html#update-plugins"],
  "plugins install": ["Installing plugins", "/plugins.html#installing-plugins"],
  activate: ["Shell setup", "/shell-setup.html"],
  deactivate: ["Shell setup", "/shell-setup.html"],
  en: ["Shell setup", "/shell-setup.html"],
  shell: ["Shell setup", "/shell-setup.html"],
  completion: ["Shell completions", "/shell-setup.html#autocompletion"],
  "shell-alias": ["Shell aliases", "/shell-aliases.html"],
  vars: ["Config variables", "/configuration/vars.html"],
  env: ["Environment variables", "/environments/"],
  set: ["Environment variables", "/environments/"],
  unset: ["Environment variables", "/environments/"],
  exec: ["Running tools", "/dev-tools/"],
  installs: ["Identity install layout", "/dev-tools/install-layout.html"],
  backends: ["Choosing backends", "/dev-tools/backends/"],
  config: ["Configuration", "/configuration.html"],
  edit: ["Configuration", "/configuration.html"],
  fmt: ["Configuration", "/configuration.html"],
  settings: ["Settings reference", "/configuration/settings.html"],
  "tool-alias": ["Tool version aliases", "/dev-tools/aliases.html"],
  bootstrap: ["Bootstrap workflow", "/bootstrap.html"],
  "bootstrap accounts": ["Linux users and groups", "/bootstrap/accounts.html"],
  "bootstrap compose": ["Docker Compose projects", "/bootstrap/compose.html"],
  "bootstrap files": ["System files and directories", "/bootstrap/files.html"],
  "bootstrap firewall": ["Linux firewall", "/bootstrap/firewall.html"],
  "bootstrap linux": ["systemd user units", "/bootstrap/systemd.html"],
  "bootstrap macos": ["macOS defaults", "/bootstrap/macos-defaults.html"],
  "bootstrap macos launchd-agents": ["LaunchAgents", "/bootstrap/launchd.html"],
  "bootstrap mise-shell-activate": [
    "Shell activation and login shell",
    "/bootstrap/shell.html",
  ],
  "bootstrap packages": ["Bootstrap packages", "/bootstrap/packages/"],
  "bootstrap packages brew": [
    "Homebrew formulae and taps",
    "/bootstrap/packages/brew.html#third-party-taps",
  ],
  "bootstrap plugins": [
    "Package manager plugins",
    "/bootstrap/packages/plugins.html",
  ],
  daemons: ["Daemons", "/daemons.html"],
  "daemons providers": [
    "Share daemons across projects",
    "/daemons/sharing.html",
  ],
  dotfiles: ["Dotfiles", "/dotfiles.html"],
  "dotfiles history": ["Dotfiles history", "/dotfiles/history.html"],
  "dotfiles save": ["Dotfiles history", "/dotfiles/history.html"],
  "dotfiles rollback": ["Dotfiles history", "/dotfiles/history.html"],
  "dotfiles undo": ["Dotfiles history", "/dotfiles/history.html"],
  "dotfiles sync": ["Sync across machines", "/dotfiles/sync.html"],
  "dotfiles pull": ["Sync across machines", "/dotfiles/sync.html"],
  "dotfiles origin": ["Sync across machines", "/dotfiles/sync.html"],
  "dotfiles conflicts": ["Sync across machines", "/dotfiles/sync.html"],
  "dotfiles watch": ["Dotfiles history", "/dotfiles/history.html"],
  "dotfiles notify": ["Dotfiles history", "/dotfiles/history.html"],
  "dotfiles track": ["Dotfiles history", "/dotfiles/history.html"],
  "dotfiles untrack": ["Dotfiles history", "/dotfiles/history.html"],
  "dotfiles exclude": ["Dotfiles history", "/dotfiles/history.html"],
  "dotfiles include": ["Dotfiles history", "/dotfiles/history.html"],
  "dotfiles paths": ["Dotfiles history", "/dotfiles/history.html"],
  "dotfiles capture": ["Dotfiles history", "/dotfiles/history.html"],
  "dotfiles recover": ["Dotfiles history", "/dotfiles/history.html"],
  secrets: ["fnox secrets", "/environments/secrets/fnox.html"],
  "generate task-stubs": [
    "Project-local task entrypoints",
    "/tips-and-tricks.html#bootstrap-script",
  ],
  "bootstrap remote": ["Remote hosts", "/bootstrap/remote.html"],
  "bootstrap repos": ["Git repositories", "/bootstrap/repos.html"],
  "bootstrap secrets": ["Secret inputs", "/bootstrap/secrets.html"],
  "bootstrap services": ["Services", "/bootstrap/services.html"],
  "bootstrap user": [
    "Login shell",
    "/bootstrap/shell.html#set-your-login-shell",
  ],
  cache: ["Cache behavior", "/cache-behavior.html"],
  "cache task": ["Task caching", "/tasks/caching.html"],
  deps: ["Project dependencies", "/dev-tools/deps.html"],
  doctor: ["Troubleshooting", "/troubleshooting.html"],
  "doctor project": [
    "Project diagnostics",
    "/configuration/project-diagnostics.html",
  ],
  generate: ["Tasks and automation", "/tasks/"],
  "generate config": ["Configuration", "/configuration.html"],
  "generate devcontainer": [
    "Dev containers",
    "/ide-integration.html#dev-containers",
  ],
  "generate github-action": [
    "Continuous integration",
    "/continuous-integration.html#github-actions",
  ],
  "generate install-script": [
    "Project installation scripts",
    "/continuous-integration.html#bootstrapping",
  ],
  "generate tool-stub": ["Portable tool stubs", "/dev-tools/tool-stubs.html"],
  implode: ["Uninstalling mise", "/installing-mise.html#uninstalling"],
  "install-into": ["Development tools", "/dev-tools/"],
  install: ["Installing and selecting tools", "/dev-tools/"],
  latest: ["Version requests", "/dev-tools/versions.html"],
  link: ["Development tools", "/dev-tools/"],
  lock: ["Lockfiles and strict installation", "/dev-tools/mise-lock.html"],
  ls: ["Development tools", "/dev-tools/"],
  "ls-remote": ["Version requests", "/dev-tools/versions.html"],
  mcp: ["AI assistants (MCP)", "/mcp.html"],
  oci: ["OCI images", "/dev-tools/mise-oci.html"],
  outdated: ["Upgrading tools", "/dev-tools/#upgrade-tools"],
  packslip: ["Signer verification", "/dev-tools/packslip-verification.html"],
  patrons: ["Supporting mise", "/about.html#supporting-mise"],
  plugins: ["Plugins", "/plugins.html"],
  "plugins link": ["Tool plugins", "/tool-plugin-development.html"],
  prune: ["Removing tools", "/dev-tools/#remove-tools"],
  registry: ["Registry and explicit backends", "/registry.html"],
  reshim: ["Shims", "/dev-tools/shims.html"],
  run: ["Running tasks", "/tasks/running-tasks.html"],
  search: ["Registry and explicit backends", "/registry.html"],
  "self-update": ["Updating mise", "/installing-mise.html#updating"],
  skills: [
    "Skills and other Packslip resources",
    "/dev-tools/packslip-resources.html",
  ],
  sponsors: ["Supporting mise", "/about.html#supporting-mise"],
  ssh: ["GitHub relay", "/bootstrap/github-relay.html"],
  sync: ["Development tools", "/dev-tools/"],
  "sync node": [
    "Node.js",
    "/lang/node.html#migrating-from-nvm-nodenv-or-homebrew",
  ],
  "sync python": ["Python", "/lang/python.html#migrating-from-pyenv-or-uv"],
  "sync ruby": ["Ruby", "/lang/ruby.html#migrating-from-other-ruby-managers"],
  tasks: ["Task configuration reference", "/tasks/task-configuration.html"],
  "tasks add": ["TOML tasks", "/tasks/toml-tasks.html"],
  "tasks deps": [
    "Dependencies and execution order",
    "/tasks/architecture.html",
  ],
  "tasks edit": ["File tasks", "/tasks/file-tasks.html"],
  "tasks graph": ["Workspace project graph", "/tasks/workspace-graph.html"],
  "tasks run": ["Running tasks", "/tasks/running-tasks.html"],
  "test-tool": [
    "Testing registry tools",
    "/contributing/registry.html#tool-testing",
  ],
  token: ["Git provider authentication", "/dev-tools/github-tokens.html"],
  tool: ["Development tools", "/dev-tools/"],
  "bin-paths": ["Shims and executable lookup", "/dev-tools/shims.html"],
  "tool-stub": ["Portable tool stubs", "/dev-tools/tool-stubs.html"],
  trust: ["Configuration trust", "/security.html#configuration-trust"],
  untrust: ["Configuration trust", "/security.html#configuration-trust"],
  uninstall: ["Removing tools", "/dev-tools/#remove-tools"],
  unuse: ["Removing tools", "/dev-tools/#remove-tools"],
  upgrade: ["Upgrading tools", "/dev-tools/#upgrade-tools"],
  use: ["Installing and selecting tools", "/dev-tools/"],
  version: ["Troubleshooting", "/troubleshooting.html"],
  watch: [
    "Rerun tasks when files change",
    "/tasks/running-tasks.html#rerun-tasks-when-files-change",
  ],
  where: ["Development tools", "/dev-tools/"],
  which: ["Shims and executable lookup", "/dev-tools/shims.html"],
};

const categories = [
  [
    "Install and inspect tools",
    "use install install-into installs uninstall unuse upgrade outdated lock latest ls ls-remote tool where which bin-paths registry search backends link sync prune reshim tool-stub packslip",
  ],
  [
    "Shell and environment",
    "activate deactivate completion en env exec shell set unset vars shell-alias tool-alias secrets token ssh",
  ],
  ["Tasks and project automation", "run tasks watch deps daemons generate"],
  ["Machine setup and images", "bootstrap dotfiles oci"],
  [
    "Configuration and diagnostics",
    "config edit fmt settings trust untrust doctor cache version self-update implode",
  ],
  ["Plugins", "plugins test-tool"],
  ["Integrations and community", "mcp skills patrons sponsors"],
];

export function commandIndex(root: Command): string {
  const remaining = new Map(
    Object.entries(root.subcommands).filter(([, cmd]) => !cmd.hide),
  );
  const sections: string[] = [];
  for (const [heading, names] of [
    ...categories,
    ["Other commands", [...remaining.keys()].join(" ")],
  ]) {
    const rows: string[] = [];
    for (const name of names.split(" ")) {
      const cmd = remaining.get(name);
      if (!cmd) continue;
      remaining.delete(name);
      rows.push(
        `- [\`mise ${name}\`](/cli/${name}.html) — ${cmd.help ?? "Command reference"}`,
      );
    }
    if (rows.length) sections.push(`### ${heading}\n\n${rows.join("\n")}`);
  }
  return `## Subcommands\n\nChoose a command family below. Its page lists the available subcommands.\n\n${sections.join("\n\n")}`;
}

export function replaceCommandIndex(page: string, index: string): string {
  const heading = "## Subcommands";
  const start = page.search(/^## Subcommands$/m);
  if (start === -1)
    throw new Error("Missing Subcommands section in generated CLI index");
  const bodyStart = start + heading.length;
  const nextSection = page.slice(bodyStart).search(/^## /m);
  const suffix = nextSection === -1 ? "" : page.slice(bodyStart + nextSection);
  return (
    page.slice(0, start) +
    index.trimEnd() +
    "\n" +
    (suffix ? "\n" + suffix : "")
  );
}

export function withCommandDescription(page: string, command: Command): string {
  const description = pageDescription(
    {
      description: command.full_cmd.length
        ? command.help
        : "Explore mise commands for managing tools, environments, tasks, and machine setup.",
    },
    `CLI command ${command.full_cmd.join(" ") || "index"}`,
  );
  const body = page.replace(
    /^---\r?\ndescription: [^\r\n]*\r?\n---\r?\n\r?\n/,
    "",
  );
  return `---\ndescription: ${JSON.stringify(description)}\n---\n\n${body}`;
}

/**
 * Whether a command sits under a hidden command. usage still generates pages
 * for the subcommands of a hidden command; they are only reachable through the
 * hidden spelling, so the reference leaves them out.
 */
export function underHiddenCommand(
  name: string,
  commands: Map<string, Pick<Command, "hide">>,
): boolean {
  const parts = name ? name.split(" ") : [];
  return parts
    .slice(0, -1)
    .some(
      (_, i) => commands.get(parts.slice(0, i + 1).join(" "))?.hide === true,
    );
}

/** Remove directories left empty under `dir`. */
function removeEmptyDirs(dir: string) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const path = join(dir, entry.name);
    removeEmptyDirs(path);
    if (readdirSync(path).length === 0) rmdirSync(path);
  }
}

function sourcePath(url: string): string {
  const path = url.split("#")[0];
  return resolve(
    docsDir,
    path.slice(1).replace(/\.html$/, ".md") +
      (path.endsWith("/") ? "index.md" : ""),
  );
}

function main() {
  const root = JSON.parse(
    execFileSync(
      "mise",
      [
        "x",
        "usage",
        "--",
        "usage",
        "generate",
        "json",
        "--file",
        "mise.usage.kdl",
      ],
      { encoding: "utf8", maxBuffer: 10 * 1024 * 1024 },
    ),
  ).cmd as Command;
  const commands = new Map<string, Command>();
  function visit(cmd: Command) {
    commands.set(cmd.full_cmd.join(" "), cmd);
    Object.values(cmd.subcommands).forEach(visit);
  }
  visit(root);
  for (const [label, url] of Object.values(guides)) {
    if (!existsSync(sourcePath(url)))
      throw new Error(`Missing guide: ${label} (${url})`);
  }
  let count = 0;
  let removed = 0;
  for (const name of commands.keys()) {
    const file = resolve(
      docsDir,
      "cli",
      name ? `${name.replaceAll(" ", "/")}.md` : "index.md",
    );
    if (!existsSync(file)) continue; // usage excludes hidden commands' own pages.
    if (underHiddenCommand(name, commands)) {
      rmSync(file);
      removed++;
      continue;
    }
    let page = readFileSync(file, "utf8").split(navigationMarker)[0].trimEnd();
    if (name) {
      const parts = name.split(" ");
      let guide: [string, string] | undefined;
      for (let i = parts.length; i > 0 && !guide; i--)
        guide = guides[parts.slice(0, i).join(" ")];
      guide ??= ["Getting started", "/getting-started.html"];
      let parent = parts.slice(0, -1);
      while (
        parent.length &&
        !existsSync(resolve(docsDir, "cli", parent.join("/") + ".md"))
      )
        parent.pop();
      const parentUsage =
        commands.get(parent.join(" "))?.usage ?? parent.join(" ");
      const parentLink = parent.length
        ? `[\`mise ${parentUsage}\`](/cli/${parent.join("/")}.html)`
        : "[All commands](/cli/)";
      page += `\n\n${navigationMarker}\n\n## Related documentation\n\n- [${guide[0]}](${guide[1]}).\n- ${parentLink}.\n- [Global flags and argument syntax](/cli/#global-flags).\n`;
    } else {
      page = page.replace(
        /^(?:- )?\*\*Usage:\*\* `[^\n]+`/m,
        "**Usage:** `mise [FLAGS] [COMMAND | TASK] [ARGS]…`",
      );
      page = page.replace(/^- \*\*Usage:\*\*[^\n]+\n/m, "");
      page = replaceCommandIndex(page, commandIndex(root));
      page = page.replace(
        "## Arguments",
        "Use `mise COMMAND --help` for the help shipped with your installed version. Put mise\nflags before a task name; arguments after the name are passed to that task.\nSquare brackets mark optional input, angle brackets mark required input, and `…`\nmeans the argument can repeat. Do not type those notation characters.\n\n## Arguments",
      );
      page = page.replace(
        "## Global Flags",
        "## Global Flags\n\nThese flags provide shared context. A command can define its own flag with the same\nname, so consult that command's page for placement and meaning. Effect labels describe\nthe command's intended operation; configuration evaluation, caches, and required tool\ninstallation can still have side effects. They are not sandbox guarantees.\n",
      );
    }
    writeFileSync(
      file,
      withCommandDescription(page.trimEnd() + "\n", commands.get(name)!),
    );
    count++;
  }
  removeEmptyDirs(resolve(docsDir, "cli"));
  console.log(
    `Added reference navigation to ${count} CLI pages; removed ${removed} under hidden commands`,
  );
}

if (import.meta.main) main();
