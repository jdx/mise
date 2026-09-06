import assert from "node:assert/strict";
import { test } from "node:test";
import {
  commandIndex,
  replaceCommandIndex,
  type Command,
} from "./cli-reference";

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

test("replacing the command index preserves preceding and following sections", () => {
  const before = "# mise\n\n## Arguments\n\nTask arguments.\n\n";
  const after =
    "## Global Flags\n\nGlobal flags.\n\n## Examples\n\nmise --help\n";
  const old = "## Subcommands\n\n### Old group\n\n- old command\n\n";
  const index = commandIndex(
    command("", { subcommands: { use: command("use") } }),
  );
  assert.equal(
    replaceCommandIndex(before + old + after, index),
    before + index + "\n\n" + after,
  );
  assert.equal(replaceCommandIndex(before + old, index), before + index + "\n");
  assert.throws(
    () => replaceCommandIndex(before, index),
    /Missing Subcommands/,
  );
});
