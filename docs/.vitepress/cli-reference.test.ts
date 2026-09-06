import assert from "node:assert/strict";
import { test } from "node:test";
import MarkdownIt from "markdown-it";
import {
  commandIndex,
  fenceCodeBlocks,
  synopsis,
  type Command,
} from "./cli-reference";

function command(usage: string, overrides: Partial<Command> = {}): Command {
  return {
    full_cmd: [],
    usage,
    hide: false,
    subcommands: {},
    mounts: [],
    ...overrides,
  };
}

test("synopses distinguish optional subcommands and dynamic task arguments", () => {
  const config = command("config <SUBCOMMAND>", {
    subcommands: { ls: command("config ls") },
  });
  assert.equal(synopsis(config), "mise config [SUBCOMMAND]");
  assert.equal(
    synopsis({ ...config, subcommand_required: true }),
    "mise config <SUBCOMMAND>",
  );
  assert.equal(
    synopsis(
      command("run [FLAGS]", { mounts: [{ run: "mise tasks --usage" }] }),
    ),
    "mise run [FLAGS] [TASK] [ARGS]…",
  );
  assert.equal(synopsis(command("use <TOOL>…")), "mise use <TOOL>…");
});

test("fencing preserves nested lists and fenced TOML 1.1 and JSON examples", () => {
  const source = [
    "- First phase",
    "  - Nested description",
    "    continuation of the description",
    "",
    "```toml",
    "node = {",
    '  version = "22", # TOML 1.1 comment and trailing comma',
    "}",
    "```",
    "",
    "```json",
    "{",
    '  "tags": [',
    '    {"kind": "path"}',
    "  ]",
    "}",
    "```",
    "",
    "    mise run build",
    "    mise exec -- node -v",
    "",
  ].join("\n");
  const output = fenceCodeBlocks(source);
  assert.equal(
    new MarkdownIt().render(output),
    new MarkdownIt().render(source),
  );
  assert.ok(output.includes("```\nmise run build\nmise exec -- node -v\n```"));
  assert.equal(fenceCodeBlocks(output), output);
});

test("code nested in a list and literal backtick fences remain code", () => {
  const source =
    "- Example:\n\n      literal ``` fence\n      {{ template }}\n\n- Next item\n";
  const output = fenceCodeBlocks(source);
  assert.equal(
    new MarkdownIt().render(output),
    new MarkdownIt().render(source),
  );
  assert.ok(output.includes("````"));
});

test("command index hides compatibility commands and includes uncategorized additions", () => {
  const root = command("", {
    subcommands: {
      use: command("use <TOOL>", { help: "Install and select tools" }),
      legacy: command("legacy", { hide: true }),
      future: command("future", { help: "A new command" }),
    },
  });
  const output = commandIndex(root);
  assert.ok(output.includes("### Install and inspect tools"));
  assert.ok(output.includes("### Other commands"));
  assert.ok(output.includes("/cli/future.html"));
  assert.ok(!output.includes("legacy"));
  assert.equal((output.match(/\/cli\/use.html/g) ?? []).length, 1);
});
