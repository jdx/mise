// What the editor shows for a mise.toml: hover text, problems and completions, from a scan of
// the text and what the schema says of each key.

import { parse, TomlError } from "smol-toml";
import {
  type Entry,
  type Scan,
  type Segment,
  type Value,
  contextAt,
  scan,
  tableKeys,
} from "./scan.ts";
import { type KeyInfo, type MiseSchema, type Written } from "./schema.ts";

export type Problem = {
  from: number;
  to: number;
  severity: "error" | "warning" | "info";
  message: string;
};

export type Hover = { from: number; to: number; info: KeyInfo; value?: Value };

export type Completion = {
  label: string;
  /** What to insert; the label when absent. */
  apply?: string;
  detail?: string;
  info?: string;
  kind: "property" | "value" | "section";
};

const id = (path: string[]) => path.join("\u0000");

function written(value: Value | undefined): Written {
  switch (value?.kind) {
    case "string":
    case "datetime":
      return {
        type: "string",
        scalar: value.kind === "string" ? value.scalar : undefined,
      };
    case "integer":
      return { type: "integer", scalar: value.scalar };
    case "float":
      return { type: "number", scalar: value.scalar };
    case "boolean":
      return { type: "boolean", scalar: value.scalar };
    case "array":
      return { type: "array" };
    case "table":
      return { type: "object" };
    default:
      return {};
  }
}

/** A mise.toml read against the schema. */
export class Analysis {
  readonly text: string;
  readonly schema: MiseSchema;
  readonly scan: Scan;
  private readonly keys: Map<string, Set<string>>;
  private readonly values = new Map<string, Value>();
  private readonly cache = new Map<string, KeyInfo>();

  constructor(text: string, schema: MiseSchema) {
    this.text = text;
    this.schema = schema;
    this.scan = scan(text);
    this.keys = tableKeys(this.scan);
    for (const entry of this.scan.entries) {
      if (entry.value) this.values.set(id(entry.path), entry.value);
    }
  }

  /** The value written at `path`, as far as matching it to a shape needs. */
  written(path: string[]): Written {
    const value = this.values.get(id(path));
    const keys = [...(this.keys.get(id(path)) ?? [])];
    if (value && value.kind !== "table") return written(value);
    return keys.length || value || this.isTable(path)
      ? { type: "object", keys }
      : {};
  }

  private isTable(path: string[]): boolean {
    return this.scan.entries.some(
      (e) => e.header === "table" && id(e.path) === id(path),
    );
  }

  /** What the schema says of the key at `path`. */
  info(path: string[]): KeyInfo {
    const key = id(path);
    let info = this.cache.get(key);
    if (!info) {
      info = this.schema.key(
        path,
        this.written(path),
        this.written(path.slice(0, -1)),
      );
      this.cache.set(key, info);
    }
    return info;
  }

  // Whether every key above `path` is one the schema describes, so `path` can be judged at all.
  private reachable(path: string[]): boolean {
    for (let i = 1; i < path.length; i++) {
      const above = this.info(path.slice(0, i));
      if (!above.known || !above.described) return false;
    }
    return true;
  }

  problems(): Problem[] {
    const out: Problem[] = [];
    try {
      parse(this.text);
    } catch (error) {
      if (error instanceof TomlError) {
        const from = offset(this.text, error.line, error.column);
        out.push({
          from,
          to: Math.min(this.text.length, from + 1),
          severity: "error",
          message: error.message.split("\n")[0],
        });
      }
    }
    const seen = new Set<string>();
    const report = (segment: Segment, value?: Value) => {
      const key = `${id(segment.path)}@${segment.from}`;
      if (seen.has(key) || !this.reachable(segment.path)) return;
      seen.add(key);
      const info = this.info(segment.path);
      if (!info.known) {
        out.push({
          from: segment.from,
          to: segment.to,
          severity: "warning",
          message: info.suggestion
            ? `not a key mise reads; did you mean ${info.suggestion}?`
            : "not a key mise reads",
        });
        return;
      }
      if (info.deprecated) {
        out.push({
          from: segment.from,
          to: segment.to,
          severity: "info",
          message: `deprecated${info.description ? `: ${firstLine(info.description)}` : ""}`,
        });
      }
      if (value && info.described) {
        const problem = this.schema.problem(
          segment.path,
          written(value),
          this.written(segment.path.slice(0, -1)),
        );
        if (problem)
          out.push({
            from: value.from,
            to: value.to,
            severity: "error",
            message: problem,
          });
      }
    };
    for (const entry of this.scan.entries) {
      entry.segments.forEach((segment, i) =>
        report(
          segment,
          i === entry.segments.length - 1 ? entry.value : undefined,
        ),
      );
    }
    return out.sort((a, b) => a.from - b.from);
  }

  /** The key or value at `pos`, and what the schema says of its key. */
  hover(pos: number): Hover | undefined {
    for (const entry of this.scan.entries) {
      for (const segment of entry.segments) {
        if (segment.from <= pos && pos <= segment.to) {
          return {
            from: segment.from,
            to: segment.to,
            info: this.info(segment.path),
          };
        }
      }
    }
    // A scalar value: what its key says, which lists the choices a value can be.
    let found: { entry: Entry; value: Value } | undefined;
    for (const entry of this.scan.entries) {
      const value = entry.value;
      if (
        value &&
        value.kind !== "table" &&
        value.kind !== "array" &&
        value.from <= pos &&
        pos <= value.to
      ) {
        found = { entry, value };
      }
    }
    if (!found) return undefined;
    return {
      from: found.value.from,
      to: found.value.to,
      info: this.info(found.entry.path),
      value: found.value,
    };
  }

  completions(
    pos: number,
  ): { from: number; to: number; options: Completion[] } | undefined {
    const context = contextAt(this.text, this.scan, pos);
    if (context.mode === "header") {
      const parts = context.typed.split(".");
      const parent = parts.slice(0, -1).filter(Boolean);
      const from = pos - (parts[parts.length - 1] ?? "").length;
      const options = this.schema
        .keys(parent, this.written(parent))
        .filter((k) => !k.pattern && /table/.test(k.type))
        .map((k) => ({
          label: k.name,
          detail: k.type,
          info: k.description,
          kind: "section" as const,
        }));
      return { from, to: this.wordEnd(pos), options };
    }
    if (context.mode === "key") {
      const present = this.keys.get(id(context.table)) ?? new Set();
      const options = this.schema
        .keys(context.table, this.written(context.table))
        .filter((k) => !present.has(k.name) || k.name === context.typed)
        .map((k) => ({
          label: k.name,
          apply: k.pattern
            ? `"${k.name}`
            : /^[A-Za-z0-9_-]+$/.test(k.name)
              ? k.name
              : JSON.stringify(k.name),
          detail: k.type,
          info: k.description,
          kind: "property" as const,
        }));
      return { from: context.from, to: this.wordEnd(pos), options };
    }
    const options = this.schema
      .values(context.path, this.written(context.path.slice(0, -1)))
      .map((v) => ({ label: v, kind: "value" as const }));
    return options.length
      ? { from: context.from, to: this.valueEnd(pos, context.typed), options }
      : undefined;
  }

  // Where the word at `pos` ends, so a completion replaces all of it.
  private wordEnd(pos: number): number {
    return pos + /^[A-Za-z0-9_-]*/.exec(this.text.slice(pos))![0].length;
  }

  // Where the value at `pos` ends: the rest of the word, or in a quoted string, its closing
  // quote, so a completion does not leave the old quote behind.
  private valueEnd(pos: number, typed: string): number {
    const quote = typed[0];
    if (quote === '"' || quote === "'") {
      const close = this.text.indexOf(quote, pos);
      const eol = this.text.indexOf("\n", pos);
      if (close !== -1 && (eol === -1 || close < eol)) return close + 1;
    }
    return this.wordEnd(pos);
  }
}

/** The offset of a 1-based line and column. */
function offset(text: string, line: number, column: number): number {
  let at = 0;
  for (let l = 1; l < line; l++) {
    const next = text.indexOf("\n", at);
    if (next === -1) return text.length;
    at = next + 1;
  }
  return Math.min(text.length, at + Math.max(0, column - 1));
}

function firstLine(text: string): string {
  return text.split("\n")[0];
}

export function analyze(text: string, schema: MiseSchema): Analysis {
  return new Analysis(text, schema);
}
