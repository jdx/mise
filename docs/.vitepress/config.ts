import { defineConfig, type TransfomContext } from "vitepress";
import { Command, commands } from "./cli_commands";

// https://vitepress.dev/reference/site-config
export default defineConfig({
  title: "mise-en-place",
  description: "mise-en-place documentation",
  lang: "en-US",
  lastUpdated: true,
  appearance: "dark",
  sitemap: {
    hostname: "https://mise.jdx.dev",
  },
  themeConfig: {
    // https://vitepress.dev/reference/default-theme-config
    outline: "deep",
    nav: [
      { text: "Dev Tools", link: "/dev-tools/" },
      { text: "Environments", link: "/environments" },
      { text: "Tasks", link: "/tasks/" },
    ],
    sidebar: [
      {
        text: "Guides",
        items: [
          { text: "Getting Started", link: "/getting-started" },
          {
            text: "Walkthrough",
            link: "/walkthrough",
            collapsed: true,
            items: [{ text: "Demo", link: "/demo" }],
          },
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
          { text: "Dev tools overview", link: "/dev-tools/" },
          { text: "Comparison to asdf", link: "/dev-tools/comparison-to-asdf" },
          { text: "Shims", link: "/dev-tools/shims" },
          { text: "Aliases", link: "/dev-tools/aliases" },
          { text: "Registry", link: "/registry" },
          {
            text: "Core tools",
            link: "/core-tools",
            collapsed: true,
            items: [
              { text: "Bun", link: "/lang/bun" },
              { text: "Deno", link: "/lang/deno" },
              { text: "Erlang", link: "/lang/erlang" },
              { text: "Go", link: "/lang/go" },
              { text: "Java", link: "/lang/java" },
              { text: "Node.js", link: "/lang/node" },
              { text: "Python", link: "/lang/python" },
              { text: "Ruby", link: "/lang/ruby" },
              { text: "Rust", link: "/lang/rust" },
              { text: "Swift", link: "/lang/swift" },
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
              { text: "go", link: "/dev-tools/backends/go" },
              { text: "npm", link: "/dev-tools/backends/npm" },
              { text: "pipx", link: "/dev-tools/backends/pipx" },
              { text: "spm", link: "/dev-tools/backends/spm" },
              { text: "ubi", link: "/dev-tools/backends/ubi" },
              { text: "vfox", link: "/dev-tools/backends/vfox" },
            ],
          },
          {
            text: "Plugins",
            link: "/plugins",
          },
        ],
      },
      {
        text: "Environments",
        items: [
          { text: "Environment variables", link: "/environments/" },
          { text: "Hooks", link: "/hooks" },
          { text: "direnv", link: "/direnv" },
        ],
      },
      {
        text: "Tasks",
        items: [
          { text: "Task overview", link: "/tasks/" },
          { text: "Running Tasks", link: "/tasks/running-tasks" },
          { text: "TOML Tasks", link: "/tasks/toml-tasks" },
          { text: "File Tasks", link: "/tasks/file-tasks" },
        ],
      },
      { text: "FAQs", link: "/faq" },
      { text: "Troubleshooting", link: "/troubleshooting" },
      { text: "Tips & Tricks", link: "/tips-and-tricks" },
      {
        text: "About",
        items: [
          { text: "About mise", link: "/about" },
          { text: "Team", link: "/team" },
          { text: "Project Roadmap", link: "/project-roadmap" },
          { text: "Contributing", link: "/contributing" },
        ],
      },
      {
        text: "Advanced",
        items: [
          { text: "Paranoid", link: "/paranoid" },
          { text: "Templates", link: "/templates" },
          { text: "Coming from rtx", link: "/rtx" },
          { text: "How I Use mise", link: "/how-i-use-mise" },
          { text: "Directory Structure", link: "/directories" },
          { text: "Cache Behavior", link: "/cache-behavior" },
        ],
      },
      {
        text: "CLI Reference",
        collapsed: true,
        items: [
          { text: "CLI overview", link: "/cli/" },
          ...cliReference(commands),
        ],
      },
    ],

    socialLinks: [{ icon: "github", link: "https://github.com/jdx/mise" }],

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
      copyright: 'Copyright © 2024 <a href="https://github.com/jdx">@jdx</a>',
    },
    carbonAds: {
      code: "CWYIPKQN",
      placement: "misejdxdev",
    },
  },
  markdown: {
    // languages: [
    //   "elisp"
    // ]
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
  },
});

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
