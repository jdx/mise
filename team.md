<script setup>
import { VPTeamPage, VPTeamPageTitle, VPTeamPageSection, VPTeamMembers } from 'vitepress/theme'

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

Jeff Dickey is the primary developer behind mise. He does the bulk
of development for the project.

<VPTeamMembers :members="members" />

## Advisory Board

The advisory board helps make important decisions about the project such as:

* What features should be on the roadmap
* When should functionality move from experimental to stable
* If/when/how features should be deprecated

<VPTeamMembers :members="board" />

## Contributors

mise is an open source project which welcomes [contributions](https://github.com/jdx/mise/graphs/contributors).
We're grateful for those that have volunteered their work for the project.
