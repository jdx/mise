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

mise ships several releases a week. To update, run `mise self-update` or use the
package manager that installed mise; see
[Updating mise](/installing-mise.html#updating).

## Release timeline {#timeline}

Each bar is one release, oldest on the left. Click a bar or a release in the
list to read its notes.

<Releases :data="data" />

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

## What the counts include {#counts}

A change is one entry in the
[changelog](https://github.com/jdx/mise/blob/main/CHANGELOG.md): a feature, fix,
registry addition, or dependency update. The count leaves out the New
Contributors section and updates to the aqua registry that mise bundles.
