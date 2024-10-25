#!/usr/bin/env node

const { execSync } = require("node:child_process");
const fs = require("node:fs");

process.env.MISE_ASDF = 1;
process.env.MISE_VFOX = 1;

const stdout = execSync("mise registry", { encoding: "utf-8" });
// Regular expression to match plugin name and repository URL
// e.g.: zprint asdf:carlduevel/asdf-zprint
const regex = /^(.+?) +(.+?):(.+?)(\[.+\])? *$/gm;

let match;
const output = [
  `---
editLink: false
---

# Registry
`,
];

output.push("| Short | Full |\n| ----------- | --------------- |");
while ((match = regex.exec(stdout)) !== null) {
  if (match[2] === "asdf" || match[2] === "vfox") {
    let repoUrl = match[3].replace(/\.git$/, "");
    if (!repoUrl.startsWith("http")) {
      repoUrl = `https://github.com/${repoUrl}`;
    }
    output.push(`| ${match[1]} | [${match[2]}:${match[3]}](${repoUrl}) |`);
  } else if (match[2] === "core") {
    output.push(
      `| ${match[1]} | [${match[2]}:${match[3]}](https://mise.jdx.dev/lang/${match[1]}.html) |`,
    );
  } else if (match[2] === "cargo") {
    output.push(
      `| ${match[1]} | [${match[2]}:${match[3]}](https://crates.io/crates/${match[3]}) |`,
    );
  } else if (match[2] === "npm") {
    output.push(
      `| ${match[1]} | [${match[2]}:${match[3]}](https://www.npmjs.com/package/${match[3]}) |`,
    );
  } else if (match[2] === "pipx") {
    output.push(
      `| ${match[1]} | [${match[2]}:${match[3]}](https://pypi.org/project/${match[3]}) |`,
    );
  } else if (match[2] === "ubi") {
    output.push(
      `| ${match[1]} | [${match[2]}:${match[3]}](https://github.com/${match[3]}) |`,
    );
  } else {
    output.push(`| ${match[1]} | ${match[2]}:${match[3]} |`);
  }
}
output.push("");

fs.writeFileSync("docs/registry.md", output.join("\n"));
