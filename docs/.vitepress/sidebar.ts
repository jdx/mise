import { type Command, commands } from "./cli_commands.ts";

// Shared between the VitePress config and the llms.txt generator
// (docs/.vitepress/llms.ts), so both describe the same set of pages.
export type SidebarItem = {
  text: string;
  link?: string;
  collapsed?: boolean;
  items?: SidebarItem[];
};

export const sidebar: SidebarItem[] = [
  {
    text: "Start here",
    items: [
      { text: "Getting started", link: "/getting-started" },
      { text: "Installing mise", link: "/installing-mise" },
      { text: "Shell setup", link: "/shell-setup" },
      { text: "Existing projects", link: "/walkthrough" },
      { text: "Editors and IDEs", link: "/ide-integration" },
      { text: "Continuous integration", link: "/continuous-integration" },
      { text: "Docker", link: "/mise-cookbook/docker" },
      { text: "AI assistants (MCP)", link: "/mcp" },
    ],
  },
  {
    text: "Dev tools",
    collapsed: true,
    items: [
      { text: "Overview", link: "/dev-tools/" },
      {
        text: "Version requests and version files",
        link: "/dev-tools/versions",
      },
      { text: "Registry", link: "/registry" },
      {
        text: "Backends",
        link: "/dev-tools/backends/",
        collapsed: true,
        items: [
          { text: "aqua", link: "/dev-tools/backends/aqua" },
          { text: "asdf (legacy)", link: "/dev-tools/backends/asdf" },
          { text: "cargo", link: "/dev-tools/backends/cargo" },
          { text: "conda", link: "/dev-tools/backends/conda" },
          { text: "dotnet", link: "/dev-tools/backends/dotnet" },
          { text: "forgejo", link: "/dev-tools/backends/forgejo" },
          { text: "gem", link: "/dev-tools/backends/gem" },
          { text: "github", link: "/dev-tools/backends/github" },
          { text: "gitlab", link: "/dev-tools/backends/gitlab" },
          { text: "go", link: "/dev-tools/backends/go" },
          { text: "http", link: "/dev-tools/backends/http" },
          { text: "npm", link: "/dev-tools/backends/npm" },
          {
            text: "packslip",
            link: "/dev-tools/backends/packslip",
            collapsed: true,
            items: [
              {
                text: "Verification and policy",
                link: "/dev-tools/packslip-verification",
              },
              {
                text: "Man pages, completions, and skills",
                link: "/dev-tools/packslip-resources",
              },
            ],
          },
          { text: "pypi", link: "/dev-tools/backends/pypi" },
          { text: "s3", link: "/dev-tools/backends/s3" },
          { text: "spinel (experimental)", link: "/dev-tools/backends/spinel" },
          { text: "spm", link: "/dev-tools/backends/spm" },
          { text: "ubi (deprecated)", link: "/dev-tools/backends/ubi" },
          { text: "vfox", link: "/dev-tools/backends/vfox" },
        ],
      },
      { text: "Shims", link: "/dev-tools/shims" },
      { text: "Tool aliases", link: "/dev-tools/aliases" },
      { text: "Tool stubs", link: "/dev-tools/tool-stubs" },
      {
        text: "Lockfile (mise.lock)",
        link: "/dev-tools/mise-lock",
        collapsed: true,
        items: [
          {
            text: "mise.lock reference",
            link: "/dev-tools/mise-lock-reference",
          },
        ],
      },
      { text: "Git provider tokens", link: "/dev-tools/github-tokens" },
      { text: "System installs", link: "/dev-tools/system-installs" },
      { text: "OCI images (experimental)", link: "/dev-tools/mise-oci" },
      {
        text: "Identity install layout (experimental)",
        link: "/dev-tools/install-layout",
      },
      { text: "Migrating from asdf", link: "/dev-tools/comparison-to-asdf" },
    ],
  },
  {
    text: "Languages",
    collapsed: true,
    items: [
      { text: "Core tools overview", link: "/core-tools" },
      { text: "Bun", link: "/lang/bun" },
      { text: "Deno", link: "/lang/deno" },
      { text: ".NET", link: "/lang/dotnet" },
      { text: "Elixir", link: "/lang/elixir" },
      { text: "Erlang", link: "/lang/erlang" },
      { text: "Go", link: "/lang/go" },
      { text: "Java", link: "/lang/java" },
      { text: "Node.js", link: "/lang/node" },
      { text: "Python", link: "/lang/python" },
      { text: "Ruby", link: "/lang/ruby" },
      { text: "Rust", link: "/lang/rust" },
      { text: "Swift", link: "/lang/swift" },
      { text: "Zig", link: "/lang/zig" },
    ],
  },
  {
    text: "Environments",
    collapsed: true,
    items: [
      { text: "Environment variables", link: "/environments/" },
      {
        text: "Secrets",
        link: "/environments/secrets/",
        collapsed: true,
        items: [
          { text: "fnox (experimental)", link: "/environments/secrets/fnox" },
          { text: "SOPS files", link: "/environments/secrets/sops" },
          {
            text: "age values (experimental)",
            link: "/environments/secrets/age",
          },
        ],
      },
      { text: "Shell aliases", link: "/shell-aliases" },
      { text: "Hooks", link: "/hooks" },
      { text: "Migrating from direnv", link: "/direnv" },
    ],
  },
  {
    text: "Tasks",
    collapsed: true,
    items: [
      { text: "Overview", link: "/tasks/" },
      { text: "TOML tasks", link: "/tasks/toml-tasks" },
      { text: "File tasks", link: "/tasks/file-tasks" },
      { text: "Task arguments", link: "/tasks/task-arguments" },
      { text: "Running tasks", link: "/tasks/running-tasks" },
      { text: "Task discovery and precedence", link: "/tasks/task-discovery" },
      { text: "Dependencies and execution order", link: "/tasks/architecture" },
      { text: "Task templates", link: "/tasks/templates" },
      {
        text: "Task caching",
        link: "/tasks/caching",
        collapsed: true,
        items: [
          { text: "Remote task cache", link: "/tasks/remote-cache" },
          {
            text: "Remote cache protocol",
            link: "/tasks/remote-cache-protocol",
          },
        ],
      },
      {
        text: "Monorepo tasks",
        link: "/tasks/monorepo",
        collapsed: true,
        items: [
          {
            text: "Workspace project graph (experimental)",
            link: "/tasks/workspace-graph",
          },
        ],
      },
      { text: "Project dependencies (experimental)", link: "/dev-tools/deps" },
      { text: "OpenTelemetry (experimental)", link: "/tasks/opentelemetry" },
      {
        text: "Task configuration reference",
        link: "/tasks/task-configuration",
      },
    ],
  },
  {
    text: "Daemons (experimental)",
    collapsed: true,
    items: [
      { text: "Overview", link: "/daemons" },
      {
        text: "Set up a development stack",
        link: "/daemons/development-stack",
      },
      { text: "Service presets", link: "/daemons/presets" },
      { text: "Ports, URLs, and worktrees", link: "/daemons/worktrees" },
      { text: "Share daemons across projects", link: "/daemons/sharing" },
      { text: "Data and cleanup", link: "/daemons/data" },
    ],
  },
  {
    text: "Bootstrap",
    collapsed: true,
    items: [
      { text: "Overview", link: "/bootstrap" },
      { text: "Set up a machine", link: "/bootstrap/setup" },
      {
        text: "Bootstrap from a repository",
        link: "/bootstrap/from-repository",
      },
      { text: "Machine modules", link: "/bootstrap/modules" },
      {
        text: "Remote hosts",
        link: "/bootstrap/remote",
        collapsed: true,
        items: [
          { text: "GitHub relay (mise ssh)", link: "/bootstrap/github-relay" },
        ],
      },
      {
        text: "Packages",
        link: "/bootstrap/packages/",
        collapsed: true,
        items: [
          { text: "apk", link: "/bootstrap/packages/apk" },
          { text: "apt", link: "/bootstrap/packages/apt" },
          { text: "aur", link: "/bootstrap/packages/aur" },
          { text: "brew", link: "/bootstrap/packages/brew" },
          { text: "brew-cask", link: "/bootstrap/packages/brew-cask" },
          { text: "dnf", link: "/bootstrap/packages/dnf" },
          { text: "flatpak", link: "/bootstrap/packages/flatpak" },
          { text: "macos-app", link: "/bootstrap/packages/macos-app" },
          { text: "mas", link: "/bootstrap/packages/mas" },
          { text: "nix", link: "/bootstrap/packages/nix" },
          { text: "pacman", link: "/bootstrap/packages/pacman" },
          { text: "scoop", link: "/bootstrap/packages/scoop" },
          { text: "winget", link: "/bootstrap/packages/winget" },
          { text: "zypper", link: "/bootstrap/packages/zypper" },
          {
            text: "Package manager plugins",
            link: "/bootstrap/packages/plugins",
          },
        ],
      },
      {
        text: "Host setup",
        items: [
          { text: "Linux users and groups", link: "/bootstrap/accounts" },
          { text: "System files and directories", link: "/bootstrap/files" },
          { text: "Secret inputs", link: "/bootstrap/secrets" },
          { text: "Services", link: "/bootstrap/services" },
          { text: "Linux firewall", link: "/bootstrap/firewall" },
          { text: "Docker Compose projects", link: "/bootstrap/compose" },
        ],
      },
      {
        text: "Your environment",
        items: [
          { text: "Git repositories", link: "/bootstrap/repos" },
          {
            text: "Shell activation and login shell",
            link: "/bootstrap/shell",
          },
        ],
      },
      {
        text: "Platform-specific",
        items: [
          { text: "macOS defaults", link: "/bootstrap/macos-defaults" },
          { text: "macOS LaunchAgents", link: "/bootstrap/launchd" },
          { text: "systemd user units", link: "/bootstrap/systemd" },
        ],
      },
    ],
  },
  {
    text: "Dotfiles",
    collapsed: true,
    items: [
      { text: "Overview", link: "/dotfiles" },
      { text: "Managed files", link: "/dotfiles/managed" },
      { text: "Groups", link: "/dotfiles/groups" },
      { text: "Edit part of a file", link: "/dotfiles/edits" },
      {
        text: "History",
        link: "/dotfiles/history",
        collapsed: true,
        items: [
          { text: "Sync across machines", link: "/dotfiles/sync" },
          { text: "Encrypted files", link: "/dotfiles/encryption" },
        ],
      },
      { text: "Dotfiles reference", link: "/dotfiles/reference" },
    ],
  },
  {
    text: "Configuration",
    collapsed: true,
    items: [
      { text: "mise.toml", link: "/configuration" },
      { text: "Config environments", link: "/configuration/environments" },
      { text: "Settings", link: "/configuration/settings" },
      { text: "Config variables", link: "/configuration/vars" },
      { text: "Tera templates", link: "/templates" },
      {
        text: "MISE_* variables",
        link: "/configuration/environment-variables",
      },
      { text: "Directories", link: "/directories" },
      { text: "Caches", link: "/cache-behavior" },
      { text: "URL replacements", link: "/url-replacements" },
      {
        text: "Project diagnostics",
        link: "/configuration/project-diagnostics",
      },
    ],
  },
  {
    text: "Security",
    collapsed: true,
    items: [
      { text: "Security overview", link: "/security" },
      { text: "Paranoid mode", link: "/paranoid" },
      { text: "Sandboxing", link: "/sandboxing" },
    ],
  },
  {
    text: "Plugins",
    collapsed: true,
    items: [
      { text: "Plugins", link: "/plugins" },
      {
        text: "Writing plugins",
        items: [
          { text: "Tool plugins", link: "/tool-plugin-development" },
          { text: "Backend plugins", link: "/backend-plugin-development" },
          { text: "Environment plugins", link: "/env-plugin-development" },
          {
            text: "Package manager plugins",
            link: "/package-plugin-development",
          },
          { text: "Plugin Lua reference", link: "/plugin-lua-modules" },
          { text: "Publishing plugins", link: "/plugin-publishing" },
        ],
      },
      { text: "asdf plugins (legacy)", link: "/asdf-legacy-plugins" },
    ],
  },
  {
    text: "Help",
    collapsed: true,
    items: [
      { text: "Troubleshooting", link: "/troubleshooting" },
      { text: "Error messages", link: "/errors" },
      { text: "FAQ", link: "/faq" },
      { text: "Glossary", link: "/glossary" },
    ],
  },
  {
    text: "Cookbook",
    collapsed: true,
    items: [
      { text: "Cookbook overview", link: "/mise-cookbook/" },
      { text: "Tips and tricks", link: "/tips-and-tricks" },
      { text: "Bazel", link: "/mise-cookbook/bazel" },
      { text: "C++ and CMake", link: "/mise-cookbook/cpp" },
      { text: "Neovim", link: "/mise-cookbook/neovim" },
      { text: "Node.js", link: "/mise-cookbook/nodejs" },
      { text: "Python", link: "/mise-cookbook/python" },
      { text: "Ruby on Rails", link: "/mise-cookbook/ruby" },
      { text: "Scaffolding tasks", link: "/mise-cookbook/presets" },
      { text: "Terraform and OpenTofu", link: "/mise-cookbook/terraform" },
    ],
  },
  {
    text: "About",
    collapsed: true,
    items: [
      { text: "About mise", link: "/about" },
      { text: "Releases", link: "/releases" },
      { text: "Contact", link: "/contact" },
      { text: "Demo", link: "/demo" },
      { text: "mise run: the song", link: "/mise-en-place" },
      { text: "External resources", link: "/external-resources" },
    ],
  },
  {
    text: "Contributing",
    collapsed: true,
    items: [
      { text: "Contributing", link: "/contributing" },
      { text: "Adding tools to the registry", link: "/contributing/registry" },
      { text: "Codebase architecture", link: "/architecture" },
      { text: "Packaging mise", link: "/packaging" },
    ],
  },
  {
    text: "CLI reference",
    collapsed: true,
    items: [{ text: "CLI overview", link: "/cli/" }, ...cliReference(commands)],
  },
];

function cliReference(
  commands: { [key: string]: Command },
  parent: string[] = [],
): SidebarItem[] {
  return Object.keys(commands)
    .map((name) => [name, commands[name]] as [string, Command])
    .filter(([_name, command]) => command.hide !== true)
    .map(([name, command]) => {
      const path = [...parent, name];
      const item: SidebarItem = {
        text: `mise ${path.join(" ")}`,
        link: `/cli/${path.join("/")}`,
      };
      if (command.subcommands) {
        item.collapsed = true;
        item.items = cliReference(command.subcommands, path);
      }
      return item;
    });
}
