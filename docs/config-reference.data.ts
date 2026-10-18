import * as fs from "node:fs";
import * as path from "node:path";
import { createMarkdownRenderer } from "vitepress";

// The mise.toml reference, read from schema/mise.json. The schema is generated
// from mise-config's declaration types and their doc comments, so this page and
// the editor hints both come from the same text.

type Schema = Record<string, any>;

export type Key = {
  // the key's path below its section, such as `tasks.<name>.depends`
  path: string;
  type: string;
  description: string;
  choices: string[];
  default?: string;
  deprecated: boolean;
  experimental: boolean;
  // a page that documents this key's children instead
  link?: string;
  children: Key[];
};

// Marks a schema that stands for one key several alternative shapes each define.
const MERGED = "x-merged";

// Deep enough for `[bootstrap]`; the bound also stops a cyclic `$ref`.
const MAX_DEPTH = 6;

// Sections documented on a page of their own instead of key by key here.
const elsewhere: Record<string, string> = {
  settings: "/configuration/settings.html",
};

async function markdownRenderer() {
  const config = (globalThis as any).VITEPRESS_CONFIG;
  if (config) {
    return createMarkdownRenderer(
      config.srcDir,
      config.markdown,
      config.site.base,
      config.logger,
    );
  }
  return createMarkdownRenderer(path.resolve("docs"));
}

// `node` and every schema it stands for through `$ref` and the combinators.
export function expand(root: Schema, node: Schema, depth = 0): Schema[] {
  if (!node || typeof node !== "object" || depth > 32) return [];
  const out = [node];
  const ref = node.$ref;
  if (typeof ref === "string" && ref.startsWith("#/")) {
    const target = ref
      .slice(2)
      .split("/")
      .reduce(
        (n: any, k) => n?.[k.replaceAll("~1", "/").replaceAll("~0", "~")],
        root,
      );
    out.push(...expand(root, target, depth + 1));
  }
  for (const combinator of ["allOf", "oneOf", "anyOf"]) {
    for (const variant of node[combinator] ?? []) {
      out.push(...expand(root, variant, depth + 1));
    }
  }
  return out;
}

// The schemas `node` is, through `$ref` and `allOf` but not the alternatives of `oneOf` or
// `anyOf`: a deprecated alternative does not make the key deprecated.
function itself(root: Schema, node: Schema, depth = 0): Schema[] {
  if (!node || typeof node !== "object" || depth > 32) return [];
  const out = [node];
  const ref = node.$ref;
  if (typeof ref === "string" && ref.startsWith("#/")) {
    const target = ref
      .slice(2)
      .split("/")
      .reduce(
        (n: any, k) => n?.[k.replaceAll("~1", "/").replaceAll("~0", "~")],
        root,
      );
    out.push(...itself(root, target, depth + 1));
  }
  for (const part of node.allOf ?? [])
    out.push(...itself(root, part, depth + 1));
  return out;
}

function first<T>(nodes: Schema[], pick: (n: Schema) => T | undefined) {
  for (const n of nodes) {
    const v = pick(n);
    if (v !== undefined) return v;
  }
  return undefined;
}

// A short type such as `string | string[]`, `table` or `boolean`.
export function typeLabel(root: Schema, nodes: Schema[], depth = 0): string {
  const labels = new Set<string>();
  const add = (n: Schema) => {
    const types = Array.isArray(n.type) ? n.type : n.type ? [n.type] : [];
    for (const t of types) {
      if (t === "array") {
        const items = expand(root, n.items ?? {});
        // a list of lists stops naming its items after a few levels
        const inner = depth > 3 ? "any" : typeLabel(root, items, depth + 1);
        labels.add(
          inner === "any"
            ? "array"
            : `${inner.includes(" ") ? `(${inner})` : inner}[]`,
        );
      } else if (t === "object") {
        labels.add("table");
      } else if (t === "null") {
        // TOML has no null
      } else {
        labels.add(t);
      }
    }
    if (n.const !== undefined) labels.add(JSON.stringify(n.const));
  };
  for (const n of nodes) {
    // a branch of a combinator speaks for its own type
    if (n.type || n.const !== undefined) add(n);
  }
  return labels.size ? [...labels].join(" | ") : "any";
}

function choices(nodes: Schema[]): string[] {
  const values = new Set<string>();
  for (const n of nodes) {
    for (const v of n.enum ?? []) values.add(JSON.stringify(v));
  }
  return [...values];
}

// The schemas a table gives the keys it does not name, as written: expanding them
// here would make each alternative shape look like the key itself.
function placeholderChildren(nodes: Schema[]): Schema[] {
  return nodes
    .map((n) => n.additionalProperties)
    .filter((v) => v && typeof v === "object");
}

// Readable names for a `patternProperties` key: `^github:` is `<github:…>`,
// `^(pypi|pipx):` is `<pypi:…>` and `<pipx:…>`, and `^(core:)?python(\[.*\])?$` is
// `python`.
export function patternLabels(pattern: string): string[] {
  const s = pattern
    .replace(/^\^/, "")
    .replace(/\$$/, "")
    .replace("(\\[.*\\])?", "")
    .replace("(core:)?", "");
  const prefixes = s.match(/^\((.*)\):$/);
  if (prefixes) return prefixes[1].split("|").map((p) => `<${p}:…>`);
  if (s.endsWith(":")) return [`<${s}…>`];
  const names = s.match(/^\((.*)\)$/);
  if (names) return names[1].split("|").filter((n) => !n.startsWith("core:"));
  return [s];
}

const placeholders: Record<string, string> = {
  tasks: "<name>",
  task_templates: "<name>",
  tools: "<tool>",
  env: "<NAME>",
  vars: "<name>",
  alias: "<tool>",
  tool_alias: "<tool>",
  plugins: "<name>",
  shell_alias: "<name>",
  wrappers: "<command>",
  daemons: "<name>",
  daemon_groups: "<name>",
  daemon_providers: "<name>",
  dotfiles: "<target>",
  dotfile_groups: "<name>",
};

// Every key below `key`, at any depth.
function descendants(key: Key): Key[] {
  return key.children.flatMap((c) => [c, ...descendants(c)]);
}

// `key`'s path below `base`, such as `platforms.<key>.bin`.
function relative(base: Key, key: Key): string {
  return key.path.slice(base.path.length + 1);
}

// The keys of `children` that `shared` does not already list, keeping a shared key whose
// own children add something (`platforms`, where Cargo's adds `bin`).
function additions(base: Key, children: Key[], shared: Set<string>): Key[] {
  return children.flatMap((c) => {
    const added = additions(base, c.children, shared);
    if (shared.has(relative(base, c)) && added.length === 0) return [];
    return [{ ...c, children: added }];
  });
}

export function describeKey(
  root: Schema,
  name: string,
  schema: Schema,
  parent: string,
  render: (s: string) => string,
  depth = 0,
): Key {
  const nodes = expand(root, schema);
  // one item of a list is `<list>[]`, so its keys read `<list>[].<key>` at every depth
  const path = !parent
    ? name
    : name === "[]"
      ? `${parent}[]`
      : `${parent}.${name}`;
  // The key's own description, else the one its alternative shapes agree on: one of
  // several differing alternatives would misdescribe the others.
  const alternatives = new Set(
    nodes.map((n) => n.description).filter((d) => d !== undefined),
  );
  // A key several shapes define has each of their descriptions.
  const merged: Schema[] = schema[MERGED] ? schema.anyOf : [];
  const mergedDescriptions = [
    ...new Set(
      merged
        .map((m) => first(itself(root, m), (n) => n.description))
        .filter((d) => d !== undefined),
    ),
  ];
  const description: string =
    first(itself(root, schema), (n) => n.description) ??
    (mergedDescriptions.length > 0
      ? mergedDescriptions.join("; or ")
      : alternatives.size === 1
        ? [...alternatives][0]
        : "");
  const experimental = /^\[?experimental\]?\b/i.test(description);
  const defaultValue = first(nodes, (n) => n.default);
  const key: Key = {
    path,
    type: typeLabel(root, nodes),
    description: render(description.replace(/^\[experimental\]\s*/i, "")),
    choices: choices(nodes),
    default:
      defaultValue === undefined ? undefined : JSON.stringify(defaultValue),
    // deprecated only when every shape that defines it says so
    deprecated:
      merged.length > 0
        ? merged.every((m) =>
            itself(root, m).some((n) => n.deprecated === true),
          )
        : itself(root, schema).some((n) => n.deprecated === true),
    experimental,
    link: elsewhere[path],
    children: [],
  };
  if (depth >= MAX_DEPTH || key.link) return key;

  // A key the shape itself forbids (`hide = false` beside a task template's `$ref` to the
  // task keys) is not one of its keys, whatever the shapes it is built from allow.
  const forbidden = new Set(
    itself(root, schema).flatMap((n) =>
      Object.entries(n.properties ?? {})
        .filter(([, v]) => v === false)
        .map(([k]) => k),
    ),
  );
  // A key the alternative shapes each define is every one of those definitions at once.
  const named = new Map<string, Schema[]>();
  for (const n of nodes) {
    for (const [child, childSchema] of Object.entries(n.properties ?? {})) {
      if (forbidden.has(child) || typeof childSchema !== "object") continue;
      const schemas = named.get(child) ?? [];
      if (!schemas.includes(childSchema)) schemas.push(childSchema as Schema);
      named.set(child, schemas);
    }
  }
  for (const [child, schemas] of named) {
    key.children.push(
      describeKey(
        root,
        child,
        schemas.length === 1 ? schemas[0] : { anyOf: schemas, [MERGED]: true },
        path,
        render,
        depth + 1,
      ),
    );
  }
  // An open table: its other keys are names the user picks.
  const value = placeholderChildren(nodes);
  const hasNamed = key.children.length > 0;
  let shared = new Set<string>();
  if (value.length > 0) {
    // a section's open table has the same names wherever it is nested (`tasks.<name>.env`)
    const placeholder = placeholders[path] ?? placeholders[name] ?? "<key>";
    const entry = describeKey(
      root,
      placeholder,
      value.length === 1 ? value[0] : { allOf: value },
      path,
      render,
      depth + 1,
    );
    shared = new Set(descendants(entry).map((c) => relative(entry, c)));
    key.children.push(entry);
  }
  // Keys matched by pattern, such as the backend-specific options of `[tools]`: each lists
  // only what it adds to the open-table entry above.
  for (const n of nodes) {
    for (const [pattern, patternSchema] of Object.entries(
      n.patternProperties ?? {},
    )) {
      for (const label of patternLabels(pattern)) {
        const entry = describeKey(
          root,
          label,
          patternSchema as Schema,
          path,
          render,
          depth + 1,
        );
        entry.children = additions(entry, entry.children, shared);
        if (entry.children.length > 0) key.children.push(entry);
      }
    }
  }
  if (!hasNamed) {
    // A list of tables: describe one item.
    const items = nodes.flatMap((n) => (n.items ? expand(root, n.items) : []));
    if (items.some((n) => n.properties)) {
      const entry = describeKey(
        root,
        "[]",
        { allOf: items },
        path,
        render,
        depth + 1,
      );
      key.children.push(...entry.children);
    }
  }
  return key;
}

export default {
  watch: ["../schema/mise.json"],
  async load(): Promise<Key[]> {
    const md = await markdownRenderer();
    const render = (s: string) => md.renderInline(s, {});
    const root = JSON.parse(fs.readFileSync("./schema/mise.json", "utf-8"));
    return Object.entries(root.properties as Record<string, Schema>)
      .map(([name, schema]) => describeKey(root, name, schema, "", render))
      .sort((a, b) => a.path.localeCompare(b.path));
  },
};
