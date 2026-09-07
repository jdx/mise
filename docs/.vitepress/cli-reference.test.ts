import assert from "node:assert/strict";
import { test } from "node:test";
import {
  commandIndex,
  replaceCommandIndex,
  withCommandDescription,
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

test("generated descriptions use command help, escape YAML, and replace prior descriptions", () => {
  const cmd = command("use", {
    full_cmd: ["use"],
    help: 'Install tools: "use" them',
  });
  const page = "<!-- generated -->\n# mise use\n";
  const result = withCommandDescription(page, cmd);
  assert.ok(
    result.startsWith(
      '---\ndescription: "Install tools: \\"use\\" them"\n---\n\n',
    ),
  );
  assert.ok(result.endsWith(page));
  assert.equal(withCommandDescription(result, cmd), result);
  assert.match(
    withCommandDescription(page, command("")),
    /Explore mise commands/,
  );
  assert.throws(
    () => withCommandDescription(page, command("use", { full_cmd: ["use"] })),
    /CLI command use/,
  );
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
