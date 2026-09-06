import assert from "node:assert/strict";
import { test } from "node:test";
import { commandIndex, type Command } from "./cli-reference";

function command(usage: string, overrides: Partial<Command> = {}): Command {
  return {
    full_cmd: [],
    usage,
    hide: false,
    subcommands: {},
    ...overrides,
  };
}

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
