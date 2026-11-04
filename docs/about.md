---
description: "Learn what the name mise-en-place means, who makes mise, and how to support its development."
---

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

# About mise

mise is pronounced "meez". The name is short for _mise-en-place_, the French
kitchen practice of setting out every ingredient and utensil before you start
cooking.

mise installs a project's tools, sets its environment variables, and runs its
tasks from one `mise.toml` that works in your shell, editor, and CI. With
`mise bootstrap`, it can also set up a whole machine: packages, dotfiles, and
services. To start using it, follow [Getting started](/getting-started.html).

## Who makes mise {#who-makes-mise}

[Jeff Dickey](https://jdx.dev/) created mise and maintains it.

<VPTeamMembers :members="members" />

### Advisory board

The advisory board helps decide which features go on the roadmap, when a
feature moves from experimental to stable, and whether, when, and how a feature
is deprecated.

<VPTeamMembers :members="board" />

### Contributors

See [everyone who has contributed](https://github.com/jdx/mise/graphs/contributors).
To help with code, docs, or tests, read [Contributing](/contributing.html).

## Support mise {#supporting-mise}

mise is free and MIT licensed. Sponsors fund the development of mise and the
other jdx.dev open source tools: companies such as
[Entire](https://entire.io) and the
[Omacom Foundation](https://omarchy.org/patrons/), and individuals on the
Patron tier. To become a sponsor or a patron, see
[jdx.dev/sponsors](https://jdx.dev/sponsors.html).

[`mise sponsors`](/cli/sponsors.html) lists the sponsoring companies, and
[`mise patrons`](/cli/patrons.html) lists the patrons.
[Namespace](https://namespace.so) provides CI services for mise.

## Contact

To ask a question, report a bug, or reach Jeff, see [Contact](/contact.html).
