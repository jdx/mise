import { defineConfig } from 'vitepress'

// https://vitepress.dev/reference/site-config
export default defineConfig({
  title: "rtx",
  description: "rtx documentation",
  themeConfig: {
    // https://vitepress.dev/reference/default-theme-config
    nav: [
      { text: 'Home', link: '/' },
      { text: 'Getting Started', link: '/getting-started.html' }
    ],

    sidebar: [
      { text: 'Getting Started', link: '/getting-started.html' },
      {
        text: 'Installation',
        items: [
          { text: 'Homebrew', link: '/installation/homebrew.html' }
        ]
      },
      { text: 'Shims', link: '/shims.html' },
      { text: 'Direnv', link: '/direnv.html' },
      {
        text: 'CLI Reference',
        items: [
          { text: 'Global Flags', link: '/cli/global-flags.html' }
        ]
      },
    ],

    socialLinks: [
      { icon: 'github', link: 'https://github.com/jdx/rtx' }
    ]
  }
})
