#!/usr/bin/env node

const { execSync } = require("node:child_process");
const fs = require("node:fs");

process.env.MISE_ASDF = 1;
process.env.MISE_VFOX = 1;
process.env.MISE_EXPERIMENTAL = 1;

const stdout = execSync("mise registry --hide-aliased", { encoding: "utf-8" });

const output = [
  `---
editLink: false
---

# Registry

In general, the preferred backend to use for new tools is the following:

- [aqua](./dev-tools/backends/aqua.html) - offers the most features and security while not requiring plugins
- [ubi](./dev-tools/backends/ubi.html) - very simple to use
- [pipx](./dev-tools/backends/pipx.html) - only for python tools, requires python to be installed but this generally would always be the case for python tools
- [npm](./dev-tools/backends/npm.html) - only for node tools, requires node to be installed but this generally would always be the case for node tools
- [vfox](./dev-tools/backends/vfox.html) - only for tools that have unique installation requirements or need to modify env vars
- [asdf](./dev-tools/backends/asdf.html) - only for tools that have unique installation requirements or need to modify env vars, doesn't support windows
- [go](./dev-tools/backends/go.html) - only for go tools, requires go to be installed to compile. Because go tools can be distributed as a single binary, aqua/ubi are definitely preferred.
- [cargo](./dev-tools/backends/cargo.html) - only for rust tools, requires rust to be installed to compile. Because rust tools can be distributed as a single binary, aqua/ubi are definitely preferred.

However, each tool can define its own priority if it has more than 1 backend it supports. You can disable a backend with \`mise settings disable_backends=asdf\`.
And it will be skipped. See [Aliases](/dev-tools/aliases.html) for a way to set a default backend for a tool.

You can also specify the full name for a tool using \`mise use aqua:1password/cli\` if you want to use a specific backend.
`,
];

output.push("| Short | Full |\n| ----------- | --------------- |");
for (const match of stdout.split("\n")) {
  // e.g.: asdf:carlduevel/asdf-zprint
  const [short, ...fulls] = match.split(" ");
  const full = fulls
    .filter((x) => x !== "")
    .map((full) => {
      const match = full.match(/^(.+?):(.+?)(\[.+])?$/);
      if (match[1] === "asdf" || match[1] === "vfox") {
        let repoUrl = match[2].replace(/\.git$/, "");
        if (!repoUrl.startsWith("http")) {
          repoUrl = `https://github.com/${repoUrl}`;
        }
        return `[${match[1]}:${match[2]}](${repoUrl})`;
      } else if (match[1] === "core") {
        return `[${match[1]}:${match[2]}](https://mise.jdx.dev/lang/${match[2]}.html)`;
      } else if (match[1] === "cargo") {
        return `[${match[1]}:${match[2]}](https://crates.io/crates/${match[2]})`;
      } else if (match[1] === "npm") {
        return `[${match[1]}:${match[2]}](https://www.npmjs.com/package/${match[2]})`;
      } else if (match[1] === "pipx") {
        return `[${match[1]}:${match[2]}](https://pypi.org/project/${match[2]})`;
      } else if (match[1] === "go") {
        return `[${match[1]}:${match[2]}](https://pkg.go.dev/${match[2]})`;
      } else if (match[1] === "spm") {
        return `[${match[1]}:${match[2]}](https://github.com/${match[2]})`;
      } else if (match[1] === "ubi") {
        return `[${match[1]}:${match[2]}](https://github.com/${match[2]})`;
      } else if (match[1] === "aqua") {
        // TODO: handle non-github repos
        return `[${match[1]}:${match[2]}](https://github.com/${match[2]})`;
      } else {
        throw new Error(`Unknown registry: ${full}`);
      }
    })
    .join(" ");
  if (full !== "") output.push(`| ${short} | ${full} |`);
}
output.push("");

fs.writeFileSync("docs/registry.md", output.join("\n"));
