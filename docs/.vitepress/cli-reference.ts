// Add mise-specific website navigation after usage generates docs/cli.
// Command prose, arguments, flags, and visibility still come from mise.usage.kdl.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

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
  activate: ["Shell activation", "/getting-started.html#activate-mise"],
  deactivate: ["Shell activation", "/getting-started.html#activate-mise"],
  en: ["Shell activation", "/getting-started.html#activate-mise"],
  shell: ["Shell activation", "/getting-started.html#activate-mise"],
  completion: ["Shell completions", "/dev-tools/packslip-resources.html"],
  "shell-alias": ["Shell aliases", "/shell-aliases.html"],
  env: ["Environment variables", "/environments/"],
  set: ["Environment variables", "/environments/"],
  unset: ["Environment variables", "/environments/"],
  exec: ["Running tools", "/dev-tools/"],
  backends: ["Choosing backends", "/dev-tools/backends/"],
  config: ["Configuration", "/configuration.html"],
  edit: ["Configuration", "/configuration.html"],
  fmt: ["Configuration", "/configuration.html"],
  settings: ["Settings reference", "/configuration/settings.html"],
  "tool-alias": ["Tool version aliases", "/dev-tools/aliases.html"],
  bootstrap: ["Bootstrap workflow", "/bootstrap.html"],
  "bootstrap accounts": ["Users and groups", "/bootstrap/accounts.html"],
  "bootstrap compose": ["Compose projects", "/bootstrap/compose.html"],
  "bootstrap dotfiles": ["Dotfile ownership and modes", "/dotfiles.html"],
  "bootstrap files": [
    "Privileged files and directories",
    "/bootstrap/files.html",
  ],
  "bootstrap firewall": ["Host firewall", "/bootstrap/firewall.html"],
  "bootstrap launchd": ["LaunchAgents", "/bootstrap/launchd.html"],
  "bootstrap linux": ["systemd user units", "/bootstrap/systemd.html"],
  "bootstrap macos": ["macOS defaults", "/bootstrap/macos-defaults.html"],
  "bootstrap macos launchd-agents": ["LaunchAgents", "/bootstrap/launchd.html"],
  "bootstrap macos-defaults": [
    "macOS defaults",
    "/bootstrap/macos-defaults.html",
  ],
  "bootstrap mise-shell-activate": ["Shell setup", "/bootstrap/shell.html"],
  "bootstrap packages": ["Host packages", "/bootstrap/packages/"],
  "bootstrap packages brew": [
    "Homebrew packages and taps",
    "/bootstrap/packages/brew.html",
  ],
  "bootstrap plugins": ["Package plugins", "/bootstrap/packages/plugins.html"],
  "bootstrap remote": ["Remote bootstrap", "/bootstrap/remote.html"],
  "bootstrap repos": ["Repository checkouts", "/bootstrap/repos.html"],
  "bootstrap secrets": ["Bootstrap secrets", "/bootstrap/secrets.html"],
  "bootstrap services": ["System services", "/bootstrap/services.html"],
  "bootstrap systemd": ["systemd user units", "/bootstrap/systemd.html"],
  "bootstrap user": ["Current-user settings", "/bootstrap/user.html"],
  cache: ["Cache behavior", "/cache-behavior.html"],
  "cache task": ["Task output caching", "/tasks/caching.html"],
  deps: ["Project dependencies", "/dev-tools/deps.html"],
  doctor: ["Troubleshooting", "/troubleshooting.html"],
  generate: ["Tasks and automation", "/tasks/"],
  "generate config": ["Configuration", "/configuration.html"],
  "generate devcontainer": ["IDE integration", "/ide-integration.html"],
  "generate github-action": [
    "Continuous integration",
    "/continuous-integration.html",
  ],
  "generate install-script": ["Project installation scripts", "/dev-tools/"],
  "generate tool-stub": ["Portable tool stubs", "/dev-tools/tool-stubs.html"],
  implode: ["Uninstalling mise", "/installing-mise.html"],
  "install-into": ["Development tools", "/dev-tools/"],
  install: ["Installing and selecting tools", "/dev-tools/"],
  latest: ["Version requests", "/dev-tools/"],
  link: ["Development tools", "/dev-tools/"],
  lock: ["Lockfiles and strict installation", "/dev-tools/mise-lock.html"],
  ls: ["Development tools", "/dev-tools/"],
  "ls-remote": ["Version requests", "/dev-tools/"],
  mcp: ["MCP integration", "/mcp.html"],
  oci: ["Building and running OCI images", "/dev-tools/mise-oci.html"],
  outdated: ["Upgrading tools", "/dev-tools/"],
  packslip: ["Signer verification", "/dev-tools/packslip-verification.html"],
  patrons: ["Supporting mise", "/about.html"],
  plugins: ["Plugin selection and maintenance", "/plugin-usage.html"],
  "plugins link": ["Developing tool plugins", "/tool-plugin-development.html"],
  prune: ["Development tools", "/dev-tools/"],
  registry: ["Registry and explicit backends", "/registry.html"],
  reshim: ["Shims", "/dev-tools/shims.html"],
  run: ["Running tasks", "/tasks/running-tasks.html"],
  search: ["Registry and explicit backends", "/registry.html"],
  "self-update": ["Installing and updating mise", "/installing-mise.html"],
  skills: [
    "Skills and other Packslip resources",
    "/dev-tools/packslip-resources.html",
  ],
  sponsors: ["Supporting mise", "/about.html"],
  ssh: ["Git provider authentication", "/dev-tools/github-tokens.html"],
  sync: ["Development tools", "/dev-tools/"],
  "sync node": ["Node.js", "/lang/node.html"],
  "sync python": ["Python", "/lang/python.html"],
  "sync ruby": ["Ruby", "/lang/ruby.html"],
  tasks: ["Task configuration", "/tasks/task-configuration.html"],
  "tasks add": ["TOML tasks", "/tasks/toml-tasks.html"],
  "tasks deps": ["Task dependency graph", "/tasks/architecture.html"],
  "tasks edit": ["File tasks", "/tasks/file-tasks.html"],
  "tasks graph": ["Monorepo projects", "/tasks/monorepo.html"],
  "tasks run": ["Running tasks", "/tasks/running-tasks.html"],
  "test-tool": [
    "Contributing and registry tests",
    "/contributing.html#tool-testing",
  ],
  token: ["Git provider authentication", "/dev-tools/github-tokens.html"],
  tool: ["Development tools", "/dev-tools/"],
  "bin-paths": ["Shims and executable lookup", "/dev-tools/shims.html"],
  "tool-stub": ["Portable tool stubs", "/dev-tools/tool-stubs.html"],
  trust: ["Configuration trust", "/security.html"],
  untrust: ["Configuration trust", "/security.html"],
  uninstall: ["Development tools", "/dev-tools/"],
  unuse: [
    "Configuration write targets",
    "/configuration.html#target-file-for-write-operations",
  ],
  upgrade: ["Development tools", "/dev-tools/"],
  use: ["Installing and selecting tools", "/dev-tools/"],
  version: ["Troubleshooting", "/troubleshooting.html"],
  watch: ["Watching tasks", "/tasks/running-tasks.html"],
  where: ["Development tools", "/dev-tools/"],
  which: ["Shims and executable lookup", "/dev-tools/shims.html"],
};

const categories = [
  [
    "Install and inspect tools",
    "use install install-into uninstall unuse upgrade outdated lock latest ls ls-remote tool where which bin-paths registry search backends link sync prune reshim tool-stub packslip",
  ],
  [
    "Shell and environment",
    "activate deactivate completion en env exec shell set unset shell-alias tool-alias ssh",
  ],
  ["Tasks and project automation", "run tasks watch deps generate"],
  ["Machine setup and images", "bootstrap oci"],
  [
    "Configuration and diagnostics",
    "config edit fmt settings trust untrust doctor cache version self-update implode",
  ],
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
    execFileSync("usage", ["generate", "json", "--file", "mise.usage.kdl"], {
      encoding: "utf8",
    }),
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
  for (const name of commands.keys()) {
    const file = resolve(
      docsDir,
      "cli",
      name ? `${name.replaceAll(" ", "/")}.md` : "index.md",
    );
    if (!existsSync(file)) continue; // usage excludes hidden commands' own pages.
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
      const hiddenParent = parts
        .slice(0, -1)
        .some((_, i) => commands.get(parts.slice(0, i + 1).join(" "))?.hide);
      let compatibility = "";
      if (hiddenParent) {
        const canonical = name
          .replace("bootstrap launchd", "bootstrap macos launchd-agents")
          .replace("bootstrap systemd", "bootstrap linux systemd-units")
          .replace("bootstrap macos-defaults", "bootstrap macos defaults");
        if (canonical !== name && commands.has(canonical))
          compatibility = `\nThis is a compatibility spelling. Use [\`mise ${commands.get(canonical)!.usage}\`](/cli/${canonical.replaceAll(" ", "/")}.html) in new scripts.\n`;
      }
      page += `\n\n${navigationMarker}\n${compatibility}\n## Related documentation\n\n- [${guide[0]}](${guide[1]}).\n- ${parentLink}.\n- [Global flags and argument syntax](/cli/#global-flags).\n`;
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
    writeFileSync(file, page.trimEnd() + "\n");
    count++;
  }
  console.log(`Added reference navigation to ${count} CLI pages`);
}

if (import.meta.main) main();
