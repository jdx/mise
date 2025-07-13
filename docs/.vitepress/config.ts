import { defineConfig } from "vitepress";
import { Command, commands } from "./cli_commands";
import {
  groupIconMdPlugin,
  groupIconVitePlugin,
} from "vitepress-plugin-group-icons";
import { tabsMarkdownPlugin } from "vitepress-plugin-tabs";
import { withMermaid } from "vitepress-plugin-mermaid";

// https://vitepress.dev/reference/site-config
export default withMermaid(
  defineConfig({
    title: "mise-en-place",
    description: "mise-en-place documentation",
    lang: "en-US",
    lastUpdated: true,
    appearance: "dark",
    mermaid: {},
    sitemap: {
      hostname: "https://mise.jdx.dev",
    },
    themeConfig: {
      // https://vitepress.dev/reference/default-theme-config
      outline: "deep",
      nav: [
        { text: "Dev Tools", link: "/dev-tools/" },
        { text: "Environments", link: "/environments/" },
        { text: "Tasks", link: "/tasks/" },
      ],
      sidebar: [
        {
          text: "Guides",
          items: [
            { text: "Demo", link: "/demo" },
            { text: "Getting Started", link: "/getting-started" },
            { text: "Walkthrough", link: "/walkthrough" },
            { text: "Installing mise", link: "/installing-mise" },
            { text: "IDE Integration", link: "/ide-integration" },
            { text: "Continuous Integration", link: "/continuous-integration" },
          ],
        },
        {
          text: "Configuration",
          items: [
            { text: "mise.toml", link: "/configuration" },
            { text: "Settings", link: "/configuration/settings" },
            {
              text: "Configuration Environments",
              link: "/configuration/environments",
            },
          ],
        },
        {
          text: "Dev Tools",
          items: [
            { text: "Dev Tools Overview", link: "/dev-tools/" },
            {
              text: "Comparison to asdf",
              link: "/dev-tools/comparison-to-asdf",
            },
            { text: "Shims", link: "/dev-tools/shims" },
            { text: "Aliases", link: "/dev-tools/aliases" },
            { text: "Registry", link: "/registry" },
            { text: "mise.lock Lockfile", link: "/dev-tools/mise-lock" },
            {
              text: "Backend Architecture",
              link: "/dev-tools/backend_architecture",
            },
            {
              text: "Core tools",
              link: "/core-tools",
              collapsed: true,
              items: [
                { text: "Bun", link: "/lang/bun" },
                { text: "Deno", link: "/lang/deno" },
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
              text: "Backends",
              link: "/dev-tools/backends/",
              collapsed: true,
              items: [
                { text: "aqua", link: "/dev-tools/backends/aqua" },
                { text: "asdf", link: "/dev-tools/backends/asdf" },
                { text: "cargo", link: "/dev-tools/backends/cargo" },
                { text: "dotnet", link: "/dev-tools/backends/dotnet" },
                { text: "gem", link: "/dev-tools/backends/gem" },
                { text: "github", link: "/dev-tools/backends/github" },
                { text: "gitlab", link: "/dev-tools/backends/gitlab" },
                { text: "go", link: "/dev-tools/backends/go" },
                { text: "http", link: "/dev-tools/backends/http" },
                { text: "npm", link: "/dev-tools/backends/npm" },
                { text: "pipx", link: "/dev-tools/backends/pipx" },
                { text: "spm", link: "/dev-tools/backends/spm" },
                { text: "ubi", link: "/dev-tools/backends/ubi" },
                { text: "vfox", link: "/dev-tools/backends/vfox" },
              ],
            },
          ],
        },
        {
          text: "Environments",
          items: [
            { text: "Environment Variables", link: "/environments/" },
            { text: "Secrets", link: "/environments/secrets" },
            { text: "Hooks", link: "/hooks" },
            { text: "direnv", link: "/direnv" },
          ],
        },
        {
          text: "Tasks",
          items: [
            { text: "Task Overview", link: "/tasks/" },
            { text: "Task Architecture", link: "/tasks/architecture" },
            { text: "Running Tasks", link: "/tasks/running-tasks" },
            { text: "TOML Tasks", link: "/tasks/toml-tasks" },
            { text: "File Tasks", link: "/tasks/file-tasks" },
            { text: "Task Configuration", link: "/tasks/task-configuration" },
          ],
        },
        {
          text: "Plugins",
          items: [
            { text: "Plugin Overview", link: "/plugins" },
            { text: "Using Plugins", link: "/plugin-usage" },
            {
              text: "Backend Plugin Development",
              link: "/backend-plugin-development",
            },
            {
              text: "Tool Plugin Development",
              link: "/tool-plugin-development",
            },
            { text: "Plugin Lua Modules", link: "/plugin-lua-modules" },
            { text: "Plugin Publishing", link: "/plugin-publishing" },
            { text: "asdf (Legacy) Plugins", link: "/asdf-legacy-plugins" },
          ],
        },
        {
          text: "About",
          items: [
            { text: "About mise", link: "/about" },
            { text: "FAQs", link: "/faq" },
            { text: "Troubleshooting", link: "/troubleshooting" },
            { text: "Tips & Tricks", link: "/tips-and-tricks" },
            {
              text: "Cookbook",
              link: "/mise-cookbook/",
              collapsed: true,
              items: [
                { text: "C++", link: "/mise-cookbook/cpp" },
                { text: "Docker", link: "/mise-cookbook/docker" },
                { text: "Node", link: "/mise-cookbook/nodejs" },
                { text: "Ruby", link: "/mise-cookbook/ruby" },
                { text: "Terraform", link: "/mise-cookbook/terraform" },
                { text: "Python", link: "/mise-cookbook/python" },
                { text: "Presets", link: "/mise-cookbook/presets" },
                { text: "Shell tricks", link: "/mise-cookbook/shell-tricks" },
              ],
            },
            { text: "Team", link: "/team" },
            { text: "Roadmap", link: "/roadmap" },
            { text: "Contributing", link: "/contributing" },
            { text: "External Resources", link: "/external-resources" },
          ],
        },
        {
          text: "Advanced",
          items: [
            { text: "Architecture", link: "/architecture" },
            { text: "Paranoid", link: "/paranoid" },
            { text: "Templates", link: "/templates" },
            { text: "How I Use mise", link: "/how-i-use-mise" },
            { text: "Directory Structure", link: "/directories" },
            { text: "Cache Behavior", link: "/cache-behavior" },
          ],
        },
        {
          text: "CLI Reference",
          collapsed: true,
          items: [
            { text: "CLI Overview", link: "/cli/" },
            ...cliReference(commands),
          ],
        },
      ],

      socialLinks: [
        { icon: "github", link: "https://github.com/jdx/mise" },
        { icon: "discord", link: "https://discord.gg/UBa7pJUN7Z" },
      ],

      editLink: {
        pattern: "https://github.com/jdx/mise/edit/main/docs/:path",
      },
      search: {
        provider: "algolia",
        options: {
          indexName: "rtx",
          appId: "1452G4RPSJ",
          apiKey: "ad09b96a7d2a30eddc2771800da7a1cf",
          insights: true,
        },
      },
      footer: {
        message:
          'Licensed under the MIT License. Maintained by <a href="https://github.com/jdx">@jdx</a> and <a href="https://github.com/jdx/mise/graphs/contributors">friends</a>.',
        copyright: `Copyright © ${new Date().getFullYear()} <a href="https://github.com/jdx">@jdx</a>`,
      },
      carbonAds: {
        code: "CWYIPKQN",
        placement: "misejdxdev",
      },
    },
    markdown: {
      config(md) {
        md.use(groupIconMdPlugin);
        md.use(tabsMarkdownPlugin);
      },
    },
    vite: {
      plugins: [
        groupIconVitePlugin({
          customIcon: {
            ".toml": "vscode-icons:file-type-toml",
            brew: "logos:homebrew",
            python: "logos:python",
            node: "logos:nodejs",
            ruby: "logos:ruby",
          },
        }),
      ],
    },
    head: [
      [
        "script",
        {
          async: "",
          src: "https://www.googletagmanager.com/gtag/js?id=G-B69G389C8T",
        },
      ],
      [
        "script",
        {},
        `window.dataLayer = window.dataLayer || [];
      function gtag(){dataLayer.push(arguments);}
      gtag('js', new Date());
      gtag('config', 'G-B69G389C8T');`,
      ],
      [
        "script",
        {
          "data-goatcounter": "https://jdx.goatcounter.com/count",
          async: "",
          src: "//gc.zgo.at/count.js",
        },
      ],
    ],
    transformPageData(pageData) {
      const canonicalUrl = `https://mise.jdx.dev/${pageData.relativePath}`
        .replace(/index\.md$/, "")
        .replace(/\.md$/, ".html");

      pageData.frontmatter.head ??= [];
      pageData.frontmatter.head.push([
        "link",
        { rel: "canonical", href: canonicalUrl },
      ]);
      pageData.frontmatter.head.push([
        "link",
        {
          rel: "sitemap",
          href: "https://mise.jdx.dev/sitemap.xml",
          type: "application/xml",
          title: "Sitemap",
        },
      ]);
    },
  }),
);

function cliReference(commands: { [key: string]: Command }) {
  return Object.keys(commands)
    .map((name) => [name, commands[name]] as [string, Command])
    .filter(([_name, command]) => command.hide !== true)
    .map(([name, command]) => {
      const x: any = {
        text: `mise ${name}`,
        link: `/cli/${name}`,
      };
      if (command.subcommands) {
        x.collapsed = true;
        x.items = Object.keys(command.subcommands)
          .filter(
            (subcommand) => command.subcommands![subcommand].hide !== true,
          )
          .map((subcommand) => ({
            text: `mise ${name} ${subcommand}`,
            link: `/cli/${name}/${subcommand}`,
          }));
      }
      return x;
    });
}
