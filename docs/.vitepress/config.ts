import { defineConfig } from 'vitepress'

// https://vitepress.dev/reference/site-config
export default defineConfig({
  title: "rtx",
  description: "rtx documentation",
  lastUpdated: true,
  themeConfig: {
    // https://vitepress.dev/reference/default-theme-config
    nav: [
      { text: 'Home', link: '/' },
      { text: 'Getting Started', link: '/getting-started' }
    ],

    sidebar: [
      { text: 'Getting Started', link: '/getting-started' },
      { text: 'Demo', link: '/demo' },
      { text: 'Configuration', link: '/configuration' },
      { text: 'Versioning', link: '/versioning' },
      { text: 'Directory Structure', link: '/directories' },
      { text: 'IDE Integration', link: '/ide-integration' },
      { text: 'Project Roadmap', link: '/project-roadmap' },
      { text: 'Cache behavior', link: '/cache-behavior' },
      { text: 'FAQs', link: '/faq' },
      {
        text: 'Dev Tools',
        link: '/dev-tools',
        items: [
          { text: 'Plugins', link: '/plugins' },
          { text: 'Shims', link: '/shims' },
          { text: 'Aliases', link: '/aliases' },
          { text: 'Comparison to asdf', link: '/comparison-to-asdf' },
          { text: 'macOS Rosetta', link: '/macos-rosetta.html' },
        ],
      },
      {
        text: 'Environments',
        link: '/environments',
        items: [
          { text: 'Profiles', link: '/profiles' },
          { text: 'Shebang', link: '/shebang' },
          { text: 'direnv', link: '/direnv' },
          { text: 'Templates', link: '/templates' },
          { text: 'CI/CD', link: '/ci-cd' },
        ],
      },
      {
        text: 'Tasks',
        link: '/tasks/',
        items: [
          {text: 'Script Tasks', link: '/tasks/script-tasks'},
          {text: 'TOML Tasks', link: '/tasks/toml-tasks'},
          {text: 'Running Tasks', link: '/tasks/running-tasks'},
        ],
      },
      {
        text: 'Languages',
        items: [
          { text: 'Bun', link: '/lang/bun' },
          { text: 'Deno', link: '/lang/deno' },
          { text: 'Go', link: '/lang/go' },
          { text: 'Java', link: '/lang/java' },
          { text: 'Node.js', link: '/lang/node' },
          { text: 'Python', link: '/lang/python' },
          { text: 'Ruby', link: '/lang/ruby' },
        ]
      },
      {
        text: 'CLI Reference',
        link: '/cli/',
        items: [
          { text: 'Global Flags', link: '/cli/global-flags' }
        ]
      },
    ],

    socialLinks: [
      { icon: 'github', link: 'https://github.com/jdx/rtx' }
    ],

    editLink: {
      pattern: 'https://github.com/jdx/rtx-docs/edit/main/docs/:path',
    },

    footer: {
      message: 'Licensed under the MIT License. Maintained by <a href="https://github.com/jdx">@jdx</a> and <a href="https://github.com/jdx/rtx/graphs/contributors">friends</a>.',
      copyright: 'Copyright © 2024 <a href="https://github.com/jdx">@jdx</a>',
    },
  },
})
