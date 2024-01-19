import { defineConfig } from 'vitepress'

// https://vitepress.dev/reference/site-config
export default defineConfig({
  title: "mise-en-place",
  description: "mise-en-place documentation",
  lastUpdated: true,
  sitemap: {
    hostname: 'https://mise.jdx.dev',
  },
  themeConfig: {
    // https://vitepress.dev/reference/default-theme-config
    outline: 'deep',
    nav: [
      { text: 'Dev Tools', link: '/dev-tools/' },
      { text: 'Environments', link: '/environments' },
      { text: 'Tasks', link: '/tasks/' },
    ],
    sidebar: [
      { text: 'About', link: '/about' },
      { text: 'Team', link: '/team' },
      { text: 'Getting Started', link: '/getting-started' },
      { text: 'Demo', link: '/demo' },
      { text: 'Configuration', link: '/configuration' },
      { text: 'Plugins', link: '/plugins' },
      { text: 'IDE Integration', link: '/ide-integration' },
      { text: 'Continuous Integration', link: '/continuous-integration' },
      { text: 'Coming from rtx', link: '/rtx' },
      { text: 'Tips & Tricks', link: '/tips-and-tricks' },
      { text: 'Paranoid', link: '/paranoid' },
      { text: 'FAQs', link: '/faq' },
      { text: 'How I Use mise', link: '/how-i-use-mise' },
      {
        text: 'Dev Tools',
        link: '/dev-tools/',
        items: [
          { text: 'Shims', link: '/dev-tools/shims' },
          { text: 'Aliases', link: '/dev-tools/aliases' },
          { text: 'Comparison to asdf', link: '/dev-tools/comparison-to-asdf' },
          {
            text: 'Backends',
            link: '/dev-tools/backends/',
            items: [
              { text: 'asdf', link: '/dev-tools/backends/asdf' },
              { text: 'cargo', link: '/dev-tools/backends/cargo' },
              { text: 'npm', link: '/dev-tools/backends/npm' },
            ]
          }
        ],
      },
      {
        text: 'Environments',
        link: '/environments',
        items: [
          { text: 'Profiles', link: '/profiles' },
          { text: 'direnv', link: '/direnv' },
          { text: 'Templates', link: '/templates' },
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
          { text: 'Rust', link: '/lang/rust' },
        ]
      },
      {
        text: 'Internals',
        items: [
          { text: 'Directory Structure', link: '/directories' },
          { text: 'Cache behavior', link: '/cache-behavior' },
          { text: 'Project Roadmap', link: '/project-roadmap' },
        ],
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
      { icon: 'github', link: 'https://github.com/jdx/mise' }
    ],

    editLink: {
      pattern: 'https://github.com/jdx/mise-docs/edit/main/:path',
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
      { async: '', src: 'https://www.googletagmanager.com/gtag/js?id=G-B69G389C8T' }
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
