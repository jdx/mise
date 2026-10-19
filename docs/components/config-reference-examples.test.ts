import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { parse } from "smol-toml";
import { analyze } from "../.vitepress/mise-toml/analyze.ts";
import { MiseSchema } from "../.vitepress/mise-toml/schema.ts";
import { referenceKeys, type Key } from "../config-reference.data.ts";
import { examples, exampleFor } from "./config-reference-examples.ts";

const schema = new MiseSchema(
  JSON.parse(
    readFileSync(new URL("../../schema/mise.json", import.meta.url), "utf-8"),
  ),
);

const keys = referenceKeys((s) => s);
const paths = new Set<string>();
const visit = (key: Key) => {
  paths.add(key.path);
  key.children.forEach(visit);
};
keys.forEach(visit);

test("every example is a key the reference lists", () => {
  const unknown = Object.keys(examples).filter((path) => !paths.has(path));
  assert.deepEqual(unknown, []);
});

// Each key's example: those written for it and those derived from another key's.
const shown = [...paths]
  .map((path) => [path, exampleFor(path)] as const)
  .filter(
    (entry): entry is readonly [string, string] => entry[1] !== undefined,
  );

for (const [path, text] of shown) {
  test(`the example for ${path} is valid mise.toml`, () => {
    assert.doesNotThrow(() => parse(text), "does not parse as TOML");
    const problems = analyze(text, schema)
      .problems()
      .map((p) => `${p.severity} ${text.slice(p.from, p.to)}: ${p.message}`);
    assert.deepEqual(problems, []);
    assert.ok(text.endsWith("\n"), "ends with a newline");
  });
}
