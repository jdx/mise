import assert from "node:assert/strict";
import { test } from "node:test";
import { contextAt, dotted, scan, tableKeys } from "./scan.ts";

const paths = (text: string) => scan(text).entries.map((e) => e.path.join("."));

test("keys get their full path under headers, dotted keys and inline tables", () => {
  const text = `min_version = "2026.1.0"
[tools]
node = "22"
"cargo:ripgrep" = { version = "14", features = ["a", "b"] }

[tasks.build]
run = "make"
env.FOO = 1
`;
  assert.deepEqual(paths(text), [
    "min_version",
    "tools",
    "tools.node",
    "tools.cargo:ripgrep",
    "tools.cargo:ripgrep.version",
    "tools.cargo:ripgrep.features",
    "tasks.build",
    "tasks.build.run",
    "tasks.build.env.FOO",
  ]);
});

test("each key of a dotted key and a header names its own path", () => {
  const { entries } = scan("[tasks.build]\nenv.FOO = 1\n");
  assert.deepEqual(
    entries[0].segments.map((s) => s.path.join(".")),
    ["tasks", "tasks.build"],
  );
  assert.deepEqual(
    entries[1].segments.map((s) => s.path.join(".")),
    ["tasks.build.env", "tasks.build.env.FOO"],
  );
  assert.equal(entries[1].value?.kind, "integer");
});

test("an array of tables indexes its elements", () => {
  const text = `[[watch_files]]
patterns = ["a"]
[[watch_files]]
patterns = ["b"]
run = "x"
`;
  assert.deepEqual(paths(text), [
    "watch_files",
    "watch_files.0.patterns",
    "watch_files",
    "watch_files.1.patterns",
    "watch_files.1.run",
  ]);
});

test("values are typed, and a value can span lines", () => {
  const text = `a = "x"
b = 'y'
c = """
multi
"""
d = [
  1, # a comment
  { e = true },
]
f = 1.5
g = 1979-05-27
h = { i = { j = false } }
`;
  const kinds = Object.fromEntries(
    scan(text).entries.map((e) => [e.path.join("."), e.value?.kind]),
  );
  assert.deepEqual(kinds, {
    a: "string",
    b: "string",
    c: "string",
    d: "array",
    "d.1.e": "boolean",
    f: "float",
    g: "datetime",
    h: "table",
    "h.i": "table",
    "h.i.j": "boolean",
  });
  const c = scan(text).entries.find((e) => e.path[0] === "c")!;
  assert.equal(c.value?.scalar, "multi\n");
});

test("a half-written file still scans", () => {
  assert.deepEqual(paths("[tools]\nnode = \nfoo"), [
    "tools",
    "tools.node",
    "tools.foo",
  ]);
  assert.deepEqual(paths('[env]\nA = "unterminated\nB = 1'), [
    "env",
    "env.A",
    "env.B",
  ]);
  assert.deepEqual(paths("x = { a = 1, "), ["x", "x.a"]);
});

test("the keys of each table are collected", () => {
  const keys = tableKeys(
    scan("[tasks.a]\nrun = 'x'\n[tasks.b]\ndepends = []\n"),
  );
  assert.deepEqual([...keys.get("tasks")!], ["a", "b"]);
  assert.deepEqual([...keys.get("tasks\u0000a")!], ["run"]);
});

test("the context of the cursor says what is being typed", () => {
  const at = (text: string) => {
    const pos = text.indexOf("|");
    const clean = text.replace("|", "");
    return contextAt(clean, scan(clean), pos);
  };
  assert.deepEqual(at("[tasks.build]\nde|"), {
    mode: "key",
    table: ["tasks", "build"],
    from: 14,
    typed: "de",
  });
  assert.deepEqual(at("[tasks.build]\nrun = |"), {
    mode: "value",
    path: ["tasks", "build", "run"],
    from: 20,
    typed: "",
  });
  assert.deepEqual(at("[settings]\nstatus.missing_t|"), {
    mode: "key",
    table: ["settings", "status"],
    from: 18,
    typed: "missing_t",
  });
  assert.deepEqual(at("[tools]\nnode = { version = '22', os|"), {
    mode: "key",
    table: ["tools", "node"],
    from: 33,
    typed: "os",
  });
  assert.deepEqual(at("[tas|"), { mode: "header", from: 1, typed: "tas" });
  // A new line in an array is an item, not a key of the table.
  assert.deepEqual(at('[tasks.build]\ndepends = [\n  "lint",\n  |\n]\n'), {
    mode: "value",
    path: ["tasks", "build", "depends", "0"],
    from: 38,
    typed: "",
  });
});

test("an escape that names no character does not stop the scan", () => {
  assert.deepEqual(paths('a = "\\U00110000"\nb = "\\uD800"\nc = 1\n'), [
    "a",
    "b",
    "c",
  ]);
  // A half-typed escape stops at the closing quote.
  assert.deepEqual(paths('a = "\\u"\nb = "\\U12"\nc = 1\n'), ["a", "b", "c"]);
});

test("a numeric key is a key, while an array index is not", () => {
  const keys = tableKeys(scan("[settings]\n404 = true\nd = [{ e = 1 }]\n"));
  assert.deepEqual([...keys.get("settings")!], ["404", "d"]);
  assert.deepEqual([...keys.get("settings\u0000d\u00000")!], ["e"]);
});

test("dotted keys are split as typed", () => {
  assert.deepEqual(dotted('tools."cargo:ripgrep".version'), [
    "tools",
    "cargo:ripgrep",
    "version",
  ]);
  assert.deepEqual(dotted("status."), ["status", ""]);
});
