import { defineConfig } from 'vitepress'
import { Command, commands } from './cli_commands'

// https://vitepress.dev/reference/site-config
export default defineConfig({
  title: "mise-en-place",
  description: "mise-en-place documentation",
  lang: 'en-US',
  lastUpdated: true,
  appearance: 'dark',
  sitemap: {
    hostname: 'https://mise.jdx.dev',
  },
  themeConfig: {
    // https://vitepress.dev/reference/default-theme-config
    outline: 'deep',
    nav: [
      {text: 'Dev Tools', link: '/dev-tools/'},
      {text: 'Environments', link: '/environments'},
      {text: 'Tasks', link: '/tasks/'},
    ],
    sidebar: [
      {text: 'Getting Started', link: '/getting-started'},
      {text: 'About', link: '/about'},
      {text: 'Configuration', link: '/configuration'},
      {text: 'Continuous Integration', link: '/continuous-integration'},
      {text: 'Demo', link: '/demo'},
      {text: 'FAQs', link: '/faq'},
      {text: 'How I Use mise', link: '/how-i-use-mise'},
      {text: 'IDE Integration', link: '/ide-integration'},
      {text: 'Paranoid', link: '/paranoid'},
      {text: 'Registry', link: '/registry'},
      {text: 'Settings', link: '/settings'},
      {text: 'Plugins', link: '/plugins'},
      {text: 'Coming from rtx', link: '/rtx'},
      {text: 'Team', link: '/team'},
      {text: 'Contributing', link: '/contributing'},
      {text: 'Tips & Tricks', link: '/tips-and-tricks'},
      {
        text: 'Dev Tools',
        link: '/dev-tools/',
        items: [
          {text: 'Aliases', link: '/dev-tools/aliases'},
          {text: 'Comparison to asdf', link: '/dev-tools/comparison-to-asdf'},
          {text: 'Shims', link: '/dev-tools/shims'},
          {
            text: 'Backends',
            link: '/dev-tools/backends/',
            items: [
              {text: 'asdf', link: '/dev-tools/backends/asdf'},
              {text: 'cargo', link: '/dev-tools/backends/cargo'},
              {text: 'go', link: '/dev-tools/backends/go'},
              {text: 'npm', link: '/dev-tools/backends/npm'},
              {text: 'pipx', link: '/dev-tools/backends/pipx'},
              {text: 'spm', link: '/dev-tools/backends/spm'},
              {text: 'ubi', link: '/dev-tools/backends/ubi'},
              {text: 'vfox', link: '/dev-tools/backends/vfox'},
            ]
          }
        ],
      },
      {
        text: 'Environments',
        link: '/environments',
        items: [
          {text: 'direnv', link: '/direnv'},
          {text: 'Profiles', link: '/profiles'},
          {text: 'Templates', link: '/templates'},
        ],
      },
      {
        text: 'Tasks',
        link: '/tasks/',
        items: [
          {text: 'Running Tasks', link: '/tasks/running-tasks'},
          {text: 'File Tasks', link: '/tasks/file-tasks'},
          {text: 'TOML Tasks', link: '/tasks/toml-tasks'},
        ],
      },
      {
        text: 'Languages',
        items: [
          {text: 'Bun', link: '/lang/bun'},
          {text: 'Deno', link: '/lang/deno'},
          {text: 'Erlang', link: '/lang/erlang'},
          {text: 'Go', link: '/lang/go'},
          {text: 'Java', link: '/lang/java'},
          {text: 'Node.js', link: '/lang/node'},
          {text: 'Python', link: '/lang/python'},
          {text: 'Ruby', link: '/lang/ruby'},
          {text: 'Rust', link: '/lang/rust'},
        ]
      },
      {
        text: 'Internals',
        items: [
          {text: 'Cache Behavior', link: '/cache-behavior'},
          {text: 'Directory Structure', link: '/directories'},
          {text: 'Project Roadmap', link: '/project-roadmap'},
        ],
      },
      {
        text: 'CLI Reference',
        link: '/cli/',
        items: [
          {text: 'Global Flags', link: '/cli/global-flags'},
          ...cliReference(commands),
        ]
      },
    ],

    socialLinks: [
      {icon: 'github', link: 'https://github.com/jdx/mise'}
    ],

    editLink: {
      pattern: 'https://github.com/jdx/mise/edit/main/docs/:path',
    },
    search: {
      provider: 'algolia',
      options: {
        indexName: 'rtx',
        appId: '1452G4RPSJ',
        apiKey: 'ad09b96a7d2a30eddc2771800da7a1cf',
        insights: true,
      }
    },
    footer: {
      message: 'Licensed under the MIT License. Maintained by <a href="https://github.com/jdx">@jdx</a> and <a href="https://github.com/jdx/mise/graphs/contributors">friends</a>.',
      copyright: 'Copyright © 2024 <a href="https://github.com/jdx">@jdx</a>',
    },
    carbonAds: {
      code: 'CWYIPKQN',
      placement: 'misejdxdev',
    },
  },
  markdown: {
    // languages: [
    //   "elisp"
    // ]
  },
  head: [
    [
      'script',
      {async: '', src: 'https://www.googletagmanager.com/gtag/js?id=G-B69G389C8T'}
    ],
    [
      'script',
      {},
      `window.dataLayer = window.dataLayer || [];
      function gtag(){dataLayer.push(arguments);}
      gtag('js', new Date());
      gtag('config', 'G-B69G389C8T');`
    ]
  ],
})

function cliReference(commands: { [key: string]: Command }) {
  return Object.keys(commands)
    .map((name) => [name, commands[name]] as [string, Command])
    .filter(([name, command]) => command.hide !== true)
    .map(([name, command]) => {
      const x: any = {
        text: name,
      };
      if (command.subcommands) {
        x.collapsed = true;
        x.items = Object.keys(command.subcommands).map((subcommand) => ({
          text: subcommand,
          link: `/cli/${name}/${subcommand}`,
        }));
      } else {
        x.link = `/cli/${name}`;
      }
      return x;
    })
}
