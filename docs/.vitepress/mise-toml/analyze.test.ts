import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { analyze } from "./analyze.ts";
import { MiseSchema } from "./schema.ts";

const schema = new MiseSchema(
  JSON.parse(
    readFileSync(
      new URL("../../../schema/mise.json", import.meta.url),
      "utf-8",
    ),
  ),
);

// `text` with `|` marking a position, which is removed.
function at(text: string): [string, number] {
  const pos = text.indexOf("|");
  return [text.replace("|", ""), pos];
}

const problems = (text: string) =>
  analyze(text, schema)
    .problems()
    .map((p) => `${p.severity} ${text.slice(p.from, p.to)}: ${p.message}`);

test("hovering a key says what it is and where the reference describes it", () => {
  const [text, pos] = at('[tools]\nno|de = "22"\n');
  const hover = analyze(text, schema).hover(pos)!;
  assert.equal(
    hover.info.description,
    "a version of the tool, or a table with its version and options",
  );
  assert.equal(hover.info.reference, "tools.<tool>");
  assert.equal(text.slice(hover.from, hover.to), "node");

  const [task, taskPos] = at('[tasks.build]\nr|un = "make"\n');
  assert.equal(
    analyze(task, schema).hover(taskPos)!.info.reference,
    "tasks.<name>.run",
  );
  const [gh] = at('[tools]\n"github:cli/cli" = "2"\n');
  assert.equal(
    analyze(gh, schema).info(["tools", "github:cli/cli"]).reference,
    "tools.<github:…>",
  );
});

test("hovering a value shows its key and the choices it has", () => {
  const [text, pos] = at("[settings]\nexperimental = tr|ue\n");
  const hover = analyze(text, schema).hover(pos)!;
  assert.deepEqual(hover.info.path, ["settings", "experimental"]);
  assert.equal(hover.value?.kind, "boolean");
});

test("an unknown key is reported with the key it was likely meant to be", () => {
  assert.deepEqual(problems("[settings]\nexperimantal = true\n"), [
    "warning experimantal: not a key mise reads; did you mean experimental?",
  ]);
  // Below an unknown key nothing more is reported.
  assert.deepEqual(problems("[nope]\na = 1\n"), [
    "warning nope: not a key mise reads",
  ]);
});

test("a key is known only in the shape its table is written in", () => {
  assert.deepEqual(
    problems('[daemons.shared]\nproject = "../infra"\nport = 5432\n'),
    ["warning port: not a key mise reads"],
  );
  assert.deepEqual(
    problems(
      '[daemons.db]\npreset = "postgres"\nversion = "16"\nport = 5432\n',
    ),
    [],
  );
});

test("a value of the wrong kind is reported", () => {
  assert.deepEqual(problems('[tasks.build]\nrun = "make"\nhide = "yes"\n'), [
    'error "yes": expected boolean',
  ]);
  assert.deepEqual(
    problems('[settings]\nstatus.missing_tools = "sometimes"\n').length,
    1,
  );
});

test("a numeric key is checked like any other", () => {
  assert.deepEqual(problems("[settings]\n404 = true\n"), [
    "warning 404: not a key mise reads",
  ]);
});

test("a bad escape is a syntax error, and the rest is still read", () => {
  const found = problems(
    '[settings]\njobs = "\\U00110000"\nexperimantal = true\n',
  );
  assert.equal(found.length, 3);
  assert.match(found[0], /^error/);
  assert.match(found[2], /did you mean experimental/);
});

test("a deprecated key is noted", () => {
  const found = problems('[hooks]\nenter = { script = "echo hi" }\n');
  assert.equal(found.length, 1);
  assert.match(found[0], /^info script: deprecated/);
});

test("a TOML syntax error is reported", () => {
  const found = analyze("[tools]\nnode = \n", schema).problems();
  assert.equal(found[0].severity, "error");
});

test("a valid config has no problems", () => {
  assert.deepEqual(
    problems(`min_version = "2026.1.0"

[tools]
node = "22"
"cargo:ripgrep" = { version = "14", features = ["a"] }

[env]
NODE_ENV = "production"
_.path = ["./bin"]

[tasks.build]
description = "build it"
run = "make"
depends = ["lint"]

[[watch_files]]
patterns = ["*.rs"]
run = "cargo check"
`),
    [],
  );
});

test("completion offers the keys a table may take, without those it has", () => {
  const [text, pos] = at('[tasks.build]\nrun = "make"\nde|');
  const result = analyze(text, schema).completions(pos)!;
  const labels = result.options.map((o) => o.label);
  assert.ok(labels.includes("depends"));
  assert.ok(labels.includes("description"));
  assert.ok(!labels.includes("run"));
  assert.equal(result.from, text.length - 2);
});

test("completion offers sections in a header, and choices for a value", () => {
  const [header, headerPos] = at("[ta|");
  const sections = analyze(header, schema)
    .completions(headerPos)!
    .options.map((o) => o.label);
  assert.ok(sections.includes("tasks"));
  assert.ok(sections.includes("tools"));

  const [value, valuePos] = at("[settings]\nexperimental = |");
  const values = analyze(value, schema)
    .completions(valuePos)!
    .options.map((o) => o.label);
  assert.deepEqual(values.sort(), ["false", "true"]);

  // A choice replaces the whole quoted value, closing quote included.
  const [quoted, quotedPos] = at('[settings]\nstatus.missing_tools = "ne|"\n');
  const result = analyze(quoted, schema).completions(quotedPos)!;
  assert.equal(quoted.slice(result.from, result.to), '"ne"');
  assert.ok(result.options.some((o) => o.label === '"never"'));

  const [tools, toolsPos] = at("[tools]\n|");
  const backends = analyze(tools, schema).completions(toolsPos)!.options;
  const github = backends.find((o) => o.label === "github:")!;
  assert.equal(github.apply, '"github:');
});
