---
description: "Browse every mise release with its number of changes and the issues resolved since September 2026, and learn how version numbers work."
socialDescription: "Browse every mise release and learn how mise version numbers work."
editLink: false
---

# Releases

<script setup>
import Releases from '@jdxcode/docs-releases/Releases.vue';
import { data } from './releases.data';
</script>

mise ships several releases a week, numbered by year and month, such as
`2026.10.4`. To update, run `mise self-update` or use the package manager that
installed mise; see [Updating mise](/installing-mise.html#updating).

## Version numbers {#versioning}

mise uses calendar versions in the form `YEAR.MONTH.RELEASE`, such as
`2026.9.1`. The last number counts releases within the month, starting at 0;
it is not the day of the month.

The numbers say nothing about compatibility, so do not read the first number as
a SemVer major version. New features can arrive behind settings such as
`experimental = true`. A feature that is going away prints a deprecation
warning that names the mise version that removes it, normally 12 months after
the warning starts. The [release notes](https://github.com/jdx/mise/releases)
describe each change in behavior. To require a minimum mise version for a
project, set [`min_version`](/configuration.html#minimum-mise-version) in its
`mise.toml`.

## Release timeline {#timeline}

Each bar is one release, oldest on the left. Its height is the number of
entries in that release's
[changelog](https://github.com/jdx/mise/blob/main/CHANGELOG.md). Hover over or
focus a bar to see its details. Click a bar or a release in the list to open its
release notes, the same notes published on
[GitHub](https://github.com/jdx/mise/releases).

A change is one changelog entry: a feature, fix, registry addition, or
dependency update. The count leaves out the New Contributors section and updates
to the aqua registry that mise bundles. Releases since September 23, 2026, when
GitHub Issues were turned back on, also show how many issues they resolved.

<Releases :data="data" />
