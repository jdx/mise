<script setup>
import { VPTeamMembers } from 'vitepress/theme'

const members = [
  {
    avatar: 'https://www.github.com/jdx.png',
    name: 'Jeff Dickey',
    title: 'BDFL',
    links: [
      { icon: 'github', link: 'https://github.com/jdx' },
      { icon: 'twitter', link: 'https://twitter.com/jdxcode' },
      { icon: 'mastodon', link: 'https://fosstodon.org/@jdx' }
    ]
  }
]
const board = [
  {
    avatar: 'https://www.github.com/booniepepper.png',
    name: 'Justin "J.R." Hill',
    links: [
      { icon: 'github', link: 'https://github.com/booniepepper' },
    ]
  },
  {
    avatar: 'https://www.github.com/pepicrft.png',
    name: 'Pedro Piñera Buendía',
    links: [
      { icon: 'github', link: 'https://github.com/pepicrft' },
    ]
  },
  {
    avatar: 'https://www.github.com/chadac.png',
    name: 'Chad Crawford',
    links: [
      { icon: 'github', link: 'https://github.com/chadac' },
    ]
  }
]
</script>

# Team

mise is maintained by Jeff Dickey with help from the community. For questions, feedback,
and bug reports, use the channels on the [Contact](/contact.html) page.

<VPTeamMembers :members="members" />

## Advisory Board

The advisory board helps make important decisions about the project, such as:

- Which features should be on the roadmap
- When functionality should move from experimental to stable
- If, when, and how features should be deprecated

<VPTeamMembers :members="board" />

## Contributors

mise is an open-source project. See [everyone who has contributed](https://github.com/jdx/mise/graphs/contributors),
and read [Contributing](/contributing.html) to help with code, documentation, or testing.
A clear bug report or a correction to an example also helps improve the project.
