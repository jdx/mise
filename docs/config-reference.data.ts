import * as fs from "node:fs";
import * as path from "node:path";
import { createMarkdownRenderer } from "vitepress";
import {
  choices,
  expand,
  patternLabels,
  placeholders,
  typeLabel,
} from "./.vitepress/mise-toml/schema.ts";
import { grouped, guides } from "./components/config-reference-sections.ts";

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
  // how many keys are below this one, at any depth
  count: number;
  // another key with exactly the keys this one has, listed there instead of here
  sameAs?: string;
  // when the table this key is in has alternative forms (`run` or `task` in a
  // `[[watch_files]]` entry), the forms that have this key; absent when every form does
  forms?: number[];
  children: Key[];
};

// Marks a schema that stands for one key several alternative shapes each define.
const MERGED = "x-merged";

// Where the docs are published, for the links in the Markdown copy.
const SITE_URL = "https://mise.jdx.dev";

// Deep enough for a tool's options inside a task (`tasks.<name>.tools.<github:…>
// .platforms.<key>.rename_exe.<key>`); the bound also stops a cyclic `$ref`.
const MAX_DEPTH = 8;

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

// The keys each alternative form of a table names, when the table is one of several forms
// (`oneOf` or `anyOf`, reached through `$ref` and `allOf`, and the forms of those forms);
// forms with no keys of their own, such as a plain string, are left out. Empty when the
// table has fewer than two such forms.
function formKeys(root: Schema, schema: Schema): Set<string>[] {
  const leaves = (node: Schema, depth = 0): Schema[] => {
    const alternatives = itself(root, node).find(
      (n) => !n[MERGED] && (n.oneOf ?? n.anyOf),
    );
    if (!alternatives || depth > 8) return [node];
    return (alternatives.oneOf ?? alternatives.anyOf).flatMap((n: Schema) =>
      leaves(n, depth + 1),
    );
  };
  const forms = leaves(schema)
    .map(
      (form) =>
        new Set(
          itself(root, form).flatMap((n) => Object.keys(n.properties ?? {})),
        ),
    )
    .filter((keys) => keys.size > 0);
  return forms.length < 2 ? [] : forms;
}

// The schemas a table gives the keys it does not name, as written: expanding them
// here would make each alternative shape look like the key itself.
function placeholderChildren(nodes: Schema[]): Schema[] {
  return nodes
    .map((n) => n.additionalProperties)
    .filter((v) => v && typeof v === "object");
}

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
    count: 0,
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
  const forms = formKeys(root, schema);
  for (const [child, schemas] of named) {
    const entry = describeKey(
      root,
      child,
      schemas.length === 1 ? schemas[0] : { anyOf: schemas, [MERGED]: true },
      path,
      render,
      depth + 1,
    );
    const having = forms.flatMap((keys, i) => (keys.has(child) ? [i] : []));
    if (having.length > 0 && having.length < forms.length) entry.forms = having;
    key.children.push(entry);
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

// A key with this many keys below it, written the same as another's, names that
// key instead of listing them again: `tasks.<name>.tools.<github:…>` has the keys
// of `tools.<github:…>`. Smaller trees read better listed again.
const MIN_SHARED = 4;

// Sections made of other sections' keys: a key in one points at the section it
// comes from (`task_templates.<name>.run` at `tasks.<name>.run`), not the reverse.
const DERIVED = new Set(["task_templates", "monorepo"]);

// What `key` has below it, as text that is equal for two keys exactly when both
// have the same keys, with the same names, types and descriptions.
function shape(key: Key, cache: Map<Key, string>): string {
  let text = cache.get(key);
  if (text === undefined) {
    text = JSON.stringify(
      key.children.map((c) => [
        c.path.slice(key.path.length),
        c.type,
        c.description,
        c.choices,
        c.default,
        c.deprecated,
        c.experimental,
        c.forms,
        c.link,
        shape(c, cache),
      ]),
    );
    cache.set(key, text);
  }
  return text;
}

// Lists each set of keys once: a key whose keys another key also has, written the
// same way, points at it with `sameAs`. The key kept is one that is not
// deprecated, outside a derived section, nearest the top and then first in the
// schema, so `tasks.<name>.tools.<github:…>` points at `tools.<github:…>` and
// `platform` at `platforms`.
function shareRepeatedKeys(keys: Key[]) {
  const all: Key[] = [];
  const parents = new Map<Key, Key>();
  const visit = (key: Key) => {
    all.push(key);
    for (const child of key.children) {
      parents.set(child, key);
      visit(child);
    }
  };
  keys.forEach(visit);
  const above = (key: Key): Key[] => {
    const parent = parents.get(key);
    return parent ? [parent, ...above(parent)] : [];
  };
  const depth = (key: Key) => above(key).length;
  const deprecated = (key: Key) =>
    [key, ...above(key)].some((k) => k.deprecated);
  const derived = (key: Key) => DERIVED.has(key.path.split(".")[0]);
  const cache = new Map<Key, string>();
  const groups = new Map<string, Key[]>();
  for (const key of all) {
    if (key.count < MIN_SHARED) continue;
    const text = shape(key, cache);
    groups.set(text, [...(groups.get(text) ?? []), key]);
  }
  // From the top down, so a tree replaced here is not then kept for a smaller one.
  const ordered = [...groups.values()].sort(
    (a, b) => Math.min(...a.map(depth)) - Math.min(...b.map(depth)),
  );
  const replaced = (key: Key) => above(key).some((k) => k.sameAs);
  for (const group of ordered) {
    const present = group.filter((key) => !replaced(key));
    if (present.length < 2) continue;
    const [kept, ...others] = present.sort(
      (a, b) =>
        Number(deprecated(a)) - Number(deprecated(b)) ||
        Number(derived(a)) - Number(derived(b)) ||
        depth(a) - depth(b) ||
        all.indexOf(a) - all.indexOf(b),
    );
    for (const other of others) {
      other.sameAs = kept.path;
      other.children = [];
    }
  }
}

// Sets `count` on `key` and every key below it; run once the tree is final.
function countDescendants(key: Key): number {
  key.count = key.children.reduce((n, c) => n + 1 + countDescendants(c), 0);
  return key.count;
}

// Every key in schema/mise.json, sorted by section, with each description passed
// through `render`.
export function referenceKeys(render: (s: string) => string): Key[] {
  const root = JSON.parse(fs.readFileSync("./schema/mise.json", "utf-8"));
  const keys = Object.entries(root.properties as Record<string, Schema>)
    .map(([name, schema]) => describeKey(root, name, schema, "", render))
    .sort((a, b) => a.path.localeCompare(b.path));
  keys.forEach(countDescendants);
  shareRepeatedKeys(keys);
  keys.forEach(countDescendants);
  return keys;
}

// The reference as Markdown, for readers that do not run the page's scripts: an
// index at `configuration/reference.md` and a file for each section under
// `configuration/reference/`, every key written out with its full path. Returns
// path below the docs root -> content. `keys` keep their descriptions as Markdown.
export function referenceMarkdown(keys: Key[]): Map<string, string> {
  const file = (section: string) => `configuration/reference/${section}.md`;
  const sectionOf = new Map<string, string>();
  const visit = (key: Key, section: string) => {
    sectionOf.set(key.path, section);
    for (const child of key.children) visit(child, section);
  };
  for (const key of keys) visit(key, key.path);

  const about = (key: Key) =>
    [
      `\`${key.type}\``,
      key.deprecated && "deprecated",
      key.experimental && "experimental",
    ]
      .filter(Boolean)
      .join(", ");
  const details = (key: Key) =>
    [
      key.description,
      key.choices.length > 0 &&
        `Choices: ${key.choices.map((c) => `\`${c}\``).join(", ")}.`,
      key.default && `Default: \`${key.default}\`.`,
      key.link && `Documented in ${SITE_URL}${key.link}.`,
      key.sameAs &&
        `Same keys as \`${key.sameAs}\`${
          sectionOf.get(key.sameAs) === sectionOf.get(key.path)
            ? ""
            : `, in ${SITE_URL}/${file(sectionOf.get(key.sameAs)!)}`
        }.`,
    ]
      .filter(Boolean)
      .join(" ");
  const below = (key: Key): string[] =>
    key.children.flatMap((c) => {
      const text = details(c);
      return [
        `- \`${c.path}\` (${about(c)})${text ? `: ${text}` : ""}`,
        ...below(c),
      ];
    });
  const conventions = `Names in angle brackets, such as \`tasks.<name>\`, stand for a key you choose,
and \`<list>[].<key>\` is a key of each table in a list.`;

  const files = new Map<string, string>();
  for (const key of keys) {
    files.set(
      file(key.path),
      [
        `# \`${key.path}\` in mise.toml`,
        `Part of the mise.toml reference, ${SITE_URL}/configuration/reference.md, generated from ${SITE_URL}/schema/mise.json. ${conventions}`,
        `Type: ${about(key)}`,
        details(key),
        below(key).join("\n"),
      ]
        .filter(Boolean)
        .join("\n\n") + "\n",
    );
  }
  const entry = (key: Key) => {
    const count =
      key.count > 0 && `(${key.count === 1 ? "1 key" : `${key.count} keys`})`;
    const summary = [
      [
        key.description.split("\n")[0],
        key.sameAs && `same keys as \`${key.sameAs}\``,
      ]
        .filter(Boolean)
        .join("; "),
      count,
    ]
      .filter(Boolean)
      .join(" ");
    const guide = guides[key.path];
    const line = [
      `- [\`${key.path}\`](${SITE_URL}/${file(key.path)})`,
      summary && `: ${summary}`,
      guide && `. Guide: [${guide.text}](${SITE_URL}${guide.link})`,
    ]
      .filter(Boolean)
      .join("");
    return line;
  };
  const index = grouped(keys).map(
    (group) => `## ${group.title}\n\n${group.keys.map(entry).join("\n")}`,
  );
  files.set(
    "configuration/reference.md",
    `# mise.toml reference

Every key mise reads from a mise.toml, generated from ${SITE_URL}/schema/mise.json,
with a file for each top-level section. ${conventions} The same reference as a
page you can filter and browse: ${SITE_URL}/configuration/reference.html

${index.join("\n\n")}
`,
  );
  return files;
}

// Descriptions write placeholders such as `git::<url>//<path>`, which Markdown would read as
// HTML tags and drop. Outside code spans, a `<` is text unless it starts an autolink.
function literalAngles(s: string): string {
  return s
    .split(/(`[^`]*`)/)
    .map((part, i) =>
      i % 2 === 1 ? part : part.replace(/<(?!https?:\/\/)/g, "&lt;"),
    )
    .join("");
}

export default {
  watch: ["../schema/mise.json"],
  async load(): Promise<Key[]> {
    const md = await markdownRenderer();
    return referenceKeys((s) => md.renderInline(literalAngles(s), {}));
  },
};
