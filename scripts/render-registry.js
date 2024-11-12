#!/usr/bin/env node

const { execSync } = require("node:child_process");
const fs = require("node:fs");

process.env.MISE_ASDF = 1;
process.env.MISE_VFOX = 1;
process.env.MISE_EXPERIMENTAL = 1;

const stdout = execSync("mise registry", { encoding: "utf-8" });

const output = [
  `---
editLink: false
---

# Registry
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
