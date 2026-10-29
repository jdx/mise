// What schema/mise.json says about a key of a mise.toml, for the editor and the reference page.
//
// The rules that pick a key's shape are those of `mise config explain` (src/cli/config/explain.rs):
// a table is matched to the shapes its own keys fit, a key is known when one of those shapes names
// it, and its description and deprecation come from the shape its value fits. Keep the two in
// step. Nothing here reads files, so the editor can run it in the browser.

export type Schema = Record<string, any>;

/** A value as far as matching it to a shape needs, as in explain.rs's `Written`. */
export type Written = {
  type?: "object" | "array" | "string" | "integer" | "number" | "boolean";
  scalar?: string | number | boolean;
  keys?: string[];
};

const MAX_DEPTH = 32;

/** `node` and every schema it stands for through `$ref` and the combinators. */
export function expand(root: Schema, node: Schema, depth = 0): Schema[] {
  if (!node || typeof node !== "object" || depth > MAX_DEPTH) return [];
  const out = [node];
  const target = pointer(root, node.$ref);
  if (target) out.push(...expand(root, target, depth + 1));
  for (const combinator of ["allOf", "oneOf", "anyOf"]) {
    for (const variant of node[combinator] ?? []) {
      out.push(...expand(root, variant, depth + 1));
    }
  }
  return out;
}

function pointer(root: Schema, ref: unknown): Schema | undefined {
  if (typeof ref !== "string" || !ref.startsWith("#/")) return undefined;
  return ref
    .slice(2)
    .split("/")
    .reduce(
      (n: any, k) => n?.[k.replaceAll("~1", "/").replaceAll("~0", "~")],
      root,
    );
}

/** `node` with any `$ref` followed. */
function resolve(root: Schema, node: Schema): Schema {
  let n = node;
  for (let i = 0; i < MAX_DEPTH; i++) {
    const target = pointer(root, n?.$ref);
    if (!target) break;
    n = target;
  }
  return n;
}

/** A short type such as `string | string[]`, `table` or `boolean`. */
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

/** The values an `enum` among `nodes` allows, as written in TOML. */
export function choices(nodes: Schema[]): string[] {
  const values = new Set<string>();
  for (const n of nodes) {
    for (const v of n.enum ?? []) values.add(JSON.stringify(v));
  }
  return [...values];
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

/** The names a section's open table has, as the reference writes them. */
export const placeholders: Record<string, string> = {
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

const isIndex = (key: string) => /^\d+$/.test(key);

function patternProperty(
  node: Schema,
  key: string,
): [string, Schema] | undefined {
  for (const [pattern, schema] of Object.entries(
    node.patternProperties ?? {},
  )) {
    try {
      if (new RegExp(pattern).test(key)) return [pattern, schema as Schema];
    } catch {
      // A pattern JavaScript cannot read names no key here.
    }
  }
  return undefined;
}

/** The schema `node` gives `key`: a named property, a matching pattern, its open table's
 * schema, or for an index, its items. */
export function directChild(node: Schema, key: string): Schema | undefined {
  if (node.properties && key in node.properties) return node.properties[key];
  const pattern = patternProperty(node, key);
  if (pattern) return pattern[1];
  if (
    node.additionalProperties &&
    typeof node.additionalProperties === "object"
  ) {
    return node.additionalProperties;
  }
  if (isIndex(key) && node.items && typeof node.items === "object")
    return node.items;
  return undefined;
}

function hasType(value: Written, t: string): boolean {
  return t === value.type || (t === "number" && value.type === "integer");
}

/** Whether `value` fits `node`'s `const`, `enum`, `type` and alternatives, and with `closed`
 * also whether a closed table shape names all of its keys. */
export function fits(
  root: Schema,
  node: Schema,
  value: Written,
  closed: boolean,
  depth = 0,
): boolean {
  if (depth > 16) return true;
  const n = resolve(root, node);
  if (!n || typeof n !== "object") return true;
  if (n.const !== undefined && value.scalar !== n.const) return false;
  if (
    Array.isArray(n.enum) &&
    !(value.scalar !== undefined && n.enum.includes(value.scalar))
  ) {
    return false;
  }
  const types =
    typeof n.type === "string"
      ? [n.type]
      : Array.isArray(n.type)
        ? n.type
        : null;
  if (types && !types.some((t: string) => hasType(value, t))) return false;
  const isClosed =
    n.additionalProperties === false || n.unevaluatedProperties === false;
  if (
    value.type === "object" &&
    closed &&
    isClosed &&
    ["allOf", "oneOf", "anyOf", "patternProperties"].every(
      (k) => n[k] === undefined,
    ) &&
    n.properties &&
    !(value.keys ?? []).every((k) => k in n.properties)
  ) {
    return false;
  }
  if (
    value.type === "object" &&
    Array.isArray(n.required) &&
    !n.required.every((k: string) => (value.keys ?? []).includes(k))
  ) {
    return false;
  }
  const alternatives = n.oneOf ?? n.anyOf;
  if (
    Array.isArray(alternatives) &&
    !alternatives.some((v: Schema) => fits(root, v, value, closed, depth + 1))
  ) {
    return false;
  }
  return (
    !Array.isArray(n.allOf) ||
    n.allOf.every((p: Schema) => fits(root, p, value, closed, depth + 1))
  );
}

/** Whether `value` is of a kind `node` allows: its `const`, `enum` and `type`, through its
 * alternatives. A table's keys are left to the checks of each key. */
function kindFits(
  root: Schema,
  node: Schema,
  value: Written,
  depth = 0,
): boolean {
  if (depth > 16) return true;
  const n = resolve(root, node);
  if (!n || typeof n !== "object") return true;
  if (n.const !== undefined && value.scalar !== n.const) return false;
  if (
    Array.isArray(n.enum) &&
    !(value.scalar !== undefined && n.enum.includes(value.scalar))
  ) {
    return false;
  }
  const types =
    typeof n.type === "string"
      ? [n.type]
      : Array.isArray(n.type)
        ? n.type
        : null;
  if (types && !types.some((t: string) => hasType(value, t))) return false;
  const alternatives = n.oneOf ?? n.anyOf;
  if (
    Array.isArray(alternatives) &&
    !alternatives.some((v: Schema) => kindFits(root, v, value, depth + 1))
  ) {
    return false;
  }
  return (
    !Array.isArray(n.allOf) ||
    n.allOf.every((p: Schema) => kindFits(root, p, value, depth + 1))
  );
}

/** The description on `node` or on a schema it points at with `$ref`. */
function ownDescription(root: Schema, node: Schema): string | undefined {
  let n: Schema | undefined = node;
  for (let i = 0; i < MAX_DEPTH && n; i++) {
    if (typeof n.description === "string") return n.description;
    n = pointer(root, n.$ref);
  }
  return undefined;
}

/** The description of the first shape `value` fits, from `node` down through its alternatives. */
function describe(
  root: Schema,
  node: Schema,
  value: Written,
  depth = 0,
): string | undefined {
  if (depth > 16 || !fits(root, node, value, true, depth)) return undefined;
  const own = ownDescription(root, node);
  if (own) return own;
  const n = resolve(root, node);
  for (const combinator of ["oneOf", "anyOf", "allOf"]) {
    for (const variant of n[combinator] ?? []) {
      const found = describe(root, variant, value, depth + 1);
      if (found) return found;
    }
  }
  return value.type === "array" && n.items
    ? ownDescription(root, n.items)
    : undefined;
}

/** Whether `node`, or what its `$ref` points at, is deprecated (not one of its alternatives). */
function deprecatedItself(root: Schema, node: Schema): boolean {
  let n: Schema | undefined = node;
  for (let i = 0; i < MAX_DEPTH && n; i++) {
    if (n.deprecated === true) return true;
    n = pointer(root, n.$ref);
  }
  return false;
}

function isDeprecated(root: Schema, node: Schema): boolean {
  return expand(root, node).some((n) => n.deprecated === true);
}

/** Whether a table with these shapes accepts keys it does not name. */
function acceptsAnyKey(nodes: Schema[]): boolean {
  return nodes.some(
    (n) =>
      n.additionalProperties === true ||
      n.unevaluatedProperties === true ||
      (n.unevaluatedProperties &&
        typeof n.unevaluatedProperties === "object") ||
      (n.type === "object" &&
        [
          "properties",
          "patternProperties",
          "additionalProperties",
          "unevaluatedProperties",
          "propertyNames",
          "allOf",
          "oneOf",
          "anyOf",
          "$ref",
        ].every((k) => n[k] === undefined)),
  );
}

/** The name closest to `typo` at most two edits away, as mise's `fuzzy::nearest`. */
export function nearest(
  typo: string,
  names: Iterable<string>,
): string | undefined {
  let best: [number, string] | undefined;
  const a = typo.toLowerCase();
  for (const name of names) {
    const d = osa(a, name.toLowerCase());
    if (d <= 2 && (!best || d < best[0])) best = [d, name];
  }
  return best?.[1];
}

function osa(a: string, b: string): number {
  const d = Array.from({ length: a.length + 1 }, (_, i) => [
    i,
    ...Array(b.length).fill(0),
  ]);
  for (let j = 0; j <= b.length; j++) d[0][j] = j;
  for (let i = 1; i <= a.length; i++) {
    for (let j = 1; j <= b.length; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      d[i][j] = Math.min(
        d[i - 1][j] + 1,
        d[i][j - 1] + 1,
        d[i - 1][j - 1] + cost,
      );
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) {
        d[i][j] = Math.min(d[i][j], d[i - 2][j - 2] + 1);
      }
    }
  }
  return d[a.length][b.length];
}

export type KeyInfo = {
  path: string[];
  known: boolean;
  /** Whether the schema describes the key, so the keys under it can be described too. */
  described: boolean;
  suggestion?: string;
  description?: string;
  type: string;
  choices: string[];
  default?: string;
  deprecated: boolean;
  experimental: boolean;
  /** The id of the key on the mise.toml reference page. */
  reference?: string;
};

/** One key that may be written in a table, for completion. */
export type KeyOption = {
  name: string;
  description?: string;
  type: string;
  pattern?: boolean;
};

export class MiseSchema {
  readonly root: Schema;

  constructor(root: Schema) {
    this.root = root;
  }

  /** The schemas the table at `path` has, from the top of a mise.toml. */
  at(path: string[]): Schema[] {
    let nodes = expand(this.root, this.root);
    for (const segment of path) {
      const next: Schema[] = [];
      for (const n of nodes) {
        const child = directChild(n, segment);
        if (child) next.push(...expand(this.root, child));
      }
      nodes = next;
    }
    return nodes;
  }

  // The shapes of a parent its keys fit by type, `const` and `required`, or every shape when
  // none fits; and whether one did.
  private forms(
    parent: string[],
    written: Written,
  ): { forms: Schema[]; fitted: boolean } {
    const nodes = this.at(parent);
    const forms = nodes.filter((n) => fits(this.root, n, written, false));
    return forms.length
      ? { forms, fitted: true }
      : { forms: nodes, fitted: false };
  }

  /** What the schema says of the key at `path`, set to `value`, in a table holding `parent`. */
  key(path: string[], value: Written, parent: Written): KeyInfo {
    const name = path[path.length - 1];
    const parentPath = path.slice(0, -1);
    const nodes = this.at(path);
    const { forms, fitted } = this.forms(parentPath, parent);
    const named = forms.some((n) => directChild(n, name) !== undefined);
    const known = named || (fitted && acceptsAnyKey(forms));
    let suggestion: string | undefined;
    if (!known) {
      const keys = forms.flatMap((n) =>
        Object.entries(n.properties ?? {})
          .filter(
            ([k, s]) => k !== name && !isDeprecated(this.root, s as Schema),
          )
          .map(([k]) => k),
      );
      suggestion = nearest(name, keys);
    }
    const parentNodes = this.at(parentPath);
    const fitting = parentNodes.filter((n) => fits(this.root, n, parent, true));
    const candidates = (fitting.length ? fitting : parentNodes)
      .map((n) => directChild(n, name))
      .filter((c): c is Schema => c !== undefined);
    let description: string | undefined;
    for (const c of candidates) {
      description = describe(this.root, c, value);
      if (description) break;
    }
    description ??= candidates
      .map((c) => ownDescription(this.root, c))
      .find(Boolean);
    const shape =
      candidates.find((c) => fits(this.root, c, value, true)) ?? candidates[0];
    const expanded = candidates.flatMap((c) => expand(this.root, c));
    const defaultValue = expanded.find((n) => n.default !== undefined)?.default;
    return {
      path,
      known,
      described: named && nodes.length > 0,
      suggestion,
      description: description?.replace(/^\[experimental\]\s*/i, ""),
      type: candidates.length ? typeLabel(this.root, expanded) : "any",
      choices: choices(expanded),
      default:
        defaultValue === undefined ? undefined : JSON.stringify(defaultValue),
      deprecated: shape ? deprecatedItself(this.root, shape) : false,
      experimental: /^\[experimental\]/i.test(description ?? ""),
      reference: known ? this.reference(path) : undefined,
    };
  }

  /** Why `value` does not fit the key at `path`, or undefined when it does. Only scalars and
   * the kind of a value are checked; a table's keys are checked one by one. */
  problem(path: string[], value: Written, parent: Written): string | undefined {
    if (!value.type) return undefined;
    const name = path[path.length - 1];
    const parentNodes = this.at(path.slice(0, -1));
    const fitting = parentNodes.filter((n) => fits(this.root, n, parent, true));
    const candidates = (fitting.length ? fitting : parentNodes)
      .map((n) => directChild(n, name))
      .filter((c): c is Schema => c !== undefined);
    if (
      !candidates.length ||
      candidates.some((c) => kindFits(this.root, c, value))
    )
      return undefined;
    const expanded = candidates.flatMap((c) => expand(this.root, c));
    const allowed = choices(expanded);
    if (
      allowed.length &&
      (value.type === "string" || value.type === "integer")
    ) {
      return `not one of ${allowed.join(", ")}`;
    }
    return `expected ${typeLabel(this.root, expanded)}`;
  }

  /** The keys a table at `path` holding `written` may take, for completion. */
  keys(path: string[], written: Written): KeyOption[] {
    const { forms } = this.forms(path, written);
    const seen = new Map<string, KeyOption>();
    for (const n of forms) {
      for (const [name, schema] of Object.entries(n.properties ?? {})) {
        if (seen.has(name) || isDeprecated(this.root, schema as Schema))
          continue;
        const nodes = expand(this.root, schema as Schema);
        seen.set(name, {
          name,
          description:
            ownDescription(this.root, schema as Schema) ??
            nodes.map((x) => x.description).find(Boolean),
          type: typeLabel(this.root, nodes),
        });
      }
      for (const [pattern, schema] of Object.entries(
        n.patternProperties ?? {},
      )) {
        // A backend's prefix, such as `github:`, as something to start a key with.
        const prefix = /^\^\(?([a-z0-9|]+)\)?:$/.exec(pattern);
        for (const p of prefix?.[1].split("|") ?? []) {
          const name = `${p}:`;
          if (seen.has(name)) continue;
          seen.set(name, {
            name,
            description: ownDescription(this.root, schema as Schema),
            type: "table",
            pattern: true,
          });
        }
      }
    }
    return [...seen.values()];
  }

  /** The values the key at `path` may take that can be listed: its choices and booleans. */
  values(path: string[], parent: Written): string[] {
    const name = path[path.length - 1];
    const parentNodes = this.at(path.slice(0, -1));
    const fitting = parentNodes.filter((n) => fits(this.root, n, parent, true));
    const expanded = (fitting.length ? fitting : parentNodes)
      .map((n) => directChild(n, name))
      .filter((c): c is Schema => c !== undefined)
      .flatMap((c) => expand(this.root, c));
    const out = new Set(choices(expanded));
    for (const n of expanded) {
      if (n.const !== undefined) out.add(JSON.stringify(n.const));
      if (
        n.type === "boolean" ||
        (Array.isArray(n.type) && n.type.includes("boolean"))
      ) {
        out.add("true");
        out.add("false");
      }
    }
    return [...out];
  }

  /** The id of the key at `path` on the reference page: `tasks.<name>.run`, `tools.<github:…>`. */
  reference(path: string[]): string | undefined {
    let nodes = expand(this.root, this.root);
    let id = "";
    for (const segment of path) {
      let label: string | undefined;
      const next: Schema[] = [];
      for (const n of nodes) {
        if (n.properties && segment in n.properties) {
          label ??= segment;
          next.push(...expand(this.root, n.properties[segment]));
          continue;
        }
        const pattern = patternProperty(n, segment);
        if (pattern) {
          const labels = patternLabels(pattern[0]);
          label ??=
            labels.find((l) =>
              segment.startsWith(l.replace(/^</, "").replace(/…>$/, "")),
            ) ?? labels[0];
          next.push(...expand(this.root, pattern[1]));
          continue;
        }
        if (isIndex(segment) && n.items && typeof n.items === "object") {
          label ??= "[]";
          next.push(...expand(this.root, n.items));
          continue;
        }
        if (
          n.additionalProperties &&
          typeof n.additionalProperties === "object"
        ) {
          const parentName = id.split(".").pop() ?? "";
          label ??= placeholders[id] ?? placeholders[parentName] ?? "<key>";
          next.push(...expand(this.root, n.additionalProperties));
        }
      }
      if (label === undefined) return undefined;
      id = label === "[]" ? `${id}[]` : id ? `${id}.${label}` : label;
      nodes = next;
    }
    return id;
  }
}
