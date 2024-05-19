#!/usr/bin/env node

const {execSync} = require("node:child_process");
const fs = require('node:fs');

const stdout = execSync("mise plugins --all --urls");
// Regular expression to match plugin name and repository URL
const regex = /^([\w-]+)\s+(https?:\/\/\S+)\s*$/gm;

let match;
let output = "---\neditLink: false\n---\n";

output +=
  "| Tool | Repository URL |\n| ----------- | --------------- |";
while ((match = regex.exec(stdout)) !== null) {
  const repoUrl = match[2].replace(/\.git$/, "");
  output += `\n| ${match[1]} | <${repoUrl}> |`;
}
output += "\n";

fs.writeFileSync("docs/registry.md", output);
