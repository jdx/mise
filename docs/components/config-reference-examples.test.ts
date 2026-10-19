import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { parse } from "smol-toml";
import { analyze } from "../.vitepress/mise-toml/analyze.ts";
import { MiseSchema } from "../.vitepress/mise-toml/schema.ts";
import { referenceKeys, type Key } from "../config-reference.data.ts";
import { examples, exampleFor } from "./config-reference-examples.ts";
import { generated, snippet } from "./config-reference-toml.ts";
import { candidates, values } from "./config-reference-values.ts";

const schema = new MiseSchema(
  JSON.parse(
    readFileSync(new URL("../../schema/mise.json", import.meta.url), "utf-8"),
  ),
);

const keys = referenceKeys((s) => s);
const paths = new Set<string>();
const current: Key[] = [];
const visit = (key: Key, deprecated: boolean) => {
  paths.add(key.path);
  if (!deprecated && !key.deprecated) current.push(key);
  key.children.forEach((child) => visit(child, deprecated || key.deprecated));
};
keys.forEach((key) => visit(key, false));

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

test("every value is for a key the reference lists", () => {
  const reachable = new Set([...paths].flatMap(candidates));
  assert.deepEqual(
    Object.keys(values).filter((path) => !reachable.has(path)),
    [],
  );
});

// What the key explorer shows for each key that is not deprecated: a snippet with a real
// value, so a new key comes with an example (in config-reference-examples.ts) or a value
// (in config-reference-values.ts), and a key that is renamed shows up here.
const problemsWith = (text: string) =>
  analyze(text, schema)
    .problems()
    .map((p) => `${p.severity} ${text.slice(p.from, p.to)}: ${p.message}`);

for (const key of current) {
  test(`the snippet for ${key.path} is real, valid mise.toml`, () => {
    const text = `${snippet(key)}\n`;
    // a comment may say "…" for what is left out, but no value is a placeholder
    const placeholders = text
      .split("\n")
      .filter((line) => !line.startsWith("#") && line.includes("…"));
    assert.deepEqual(placeholders, []);
    assert.doesNotThrow(() => parse(text), "does not parse as TOML");
    assert.deepEqual(problemsWith(text), []);
  });
}

test("a snippet is written from the key's type when nothing is written for it", () => {
  const key = current.find((k) => k.path === "tasks.<name>.timeout")!;
  assert.match(generated(key), /^\[tasks\.build\]\ntimeout = /);
});
