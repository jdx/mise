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
      { text: 'Getting Started', link: '/getting-started.html' }
    ],

    sidebar: [
      { text: 'Getting Started', link: '/getting-started.html' },
      { text: 'Dev Tools', link: '/dev-tools' },
      { text: 'Environments', link: '/environments' },
      { text: 'Tasks', link: '/tasks' },
      { text: 'Shims', link: '/shims.html' },
      { text: 'Direnv', link: '/direnv.html' },
      { text: 'macOS Rosetta', link: '/macos-rosetta.html' },
      {
        text: 'Installation',
        items: [
          { text: 'Homebrew', link: '/installation/homebrew.html' }
        ]
      },
      {
        text: 'CLI Reference',
        items: [
          { text: 'Global Flags', link: '/cli/global-flags.html' }
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
