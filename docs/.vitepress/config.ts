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
      { text: 'Dev Tools', link: '/dev-tools' },
      { text: 'Environments', link: '/environments' },
      { text: 'Tasks', link: '/tasks' },
      { text: 'Shims', link: '/shims' },
      { text: 'direnv', link: '/direnv' },
      { text: 'macOS Rosetta', link: '/macos-rosetta.html' },
      { text: 'Demo', link: '/demo' },
      { text: 'Configuration', link: '/configuration' },
      { text: 'Shebang', link: '/shebang' },
      { text: 'Aliases', link: '/aliases' },
      { text: 'Plugins', link: '/plugins' },
      { text: 'Versioning', link: '/versioning' },
      { text: 'Directory Structure', link: '/directories' },
      { text: 'Templates', link: '/templates' },
      { text: 'Config Environments', link: '/config-environments' },
      { text: 'IDE Integration', link: '/ide-integration' },
      { text: 'Project Roadmap', link: '/project-roadmap' },
      { text: 'FAQs', link: '/faq' },
      { text: 'Comparison to asdf', link: '/comparison-to-asdf' },
      { text: 'CI/CD', link: '/ci-cd' },
      { text: 'Cache behavior', link: '/cache-behavior' },
      {
        text: 'Installation',
        link: '/installation/',
        items: [
          { text: 'Homebrew', link: '/installation/homebrew' }
        ]
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
