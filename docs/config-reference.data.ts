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
export function typeLabel(root: Schema, nodes: Schema[]): string {
  const labels = new Set<string>();
  const add = (n: Schema) => {
    const types = Array.isArray(n.type) ? n.type : n.type ? [n.type] : [];
    for (const t of types) {
      if (t === "array") {
        const items = expand(root, n.items ?? {});
        const inner = typeLabel(root, items);
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

// The schemas a table gives the keys it does not name.
function placeholderChildren(root: Schema, nodes: Schema[]): Schema[] {
  const out: Schema[] = [];
  for (const n of nodes) {
    if (n.additionalProperties && typeof n.additionalProperties === "object") {
      out.push(...expand(root, n.additionalProperties));
    }
  }
  return out;
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

export function describeKey(
  root: Schema,
  name: string,
  schema: Schema,
  parent: string,
  render: (s: string) => string,
  depth = 0,
): Key {
  const nodes = expand(root, schema);
  const path = parent ? `${parent}.${name}` : name;
  const description: string =
    first(itself(root, schema), (n) => n.description) ??
    first(nodes, (n) => n.description) ??
    "";
  const experimental = /^\[?experimental\]?\b/i.test(description);
  const defaultValue = first(nodes, (n) => n.default);
  const key: Key = {
    path,
    type: typeLabel(root, nodes),
    description: render(description.replace(/^\[experimental\]\s*/i, "")),
    choices: choices(nodes),
    default:
      defaultValue === undefined ? undefined : JSON.stringify(defaultValue),
    deprecated: itself(root, schema).some((n) => n.deprecated === true),
    experimental,
    link: elsewhere[path],
    children: [],
  };
  if (depth >= MAX_DEPTH || key.link) return key;

  const seen = new Set<string>();
  for (const n of nodes) {
    for (const [child, childSchema] of Object.entries(n.properties ?? {})) {
      if (seen.has(child)) continue;
      seen.add(child);
      key.children.push(
        describeKey(
          root,
          child,
          childSchema as Schema,
          path,
          render,
          depth + 1,
        ),
      );
    }
  }
  if (key.children.length === 0) {
    // An open table: its keys are names the user picks.
    const value = placeholderChildren(root, nodes);
    if (value.some((n) => n.properties)) {
      const placeholder = placeholders[path] ?? "<key>";
      const entry = describeKey(
        root,
        placeholder,
        { allOf: value },
        path,
        render,
        depth + 1,
      );
      if (entry.children.length > 0) key.children.push(entry);
    }
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
      key.children.push(
        ...entry.children.map((c) => ({
          ...c,
          path: c.path.replace(".[].", "[]."),
        })),
      );
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
