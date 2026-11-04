// A scan of a mise.toml for the editor: where each key and value sits in the text, and what
// key path it has. It is not a TOML parser: smol-toml checks the syntax. This reads what it can
// of a half-written file, because the editor asks about one while it is being typed, and skips
// the rest of a line it cannot read.

export type ValueKind =
  | "string"
  | "integer"
  | "float"
  | "boolean"
  | "datetime"
  | "array"
  | "table"
  | "invalid";

/** One key of a dotted key, with the full path it names. */
export type Segment = {
  name: string;
  path: string[];
  from: number;
  to: number;
};

export type Value = {
  kind: ValueKind;
  from: number;
  to: number;
  /** The value of a string, number or boolean. */
  scalar?: string | number | boolean;
};

/** A key as written: `a.b = 1`, or a table header `[a.b]` or `[[a.b]]`. */
export type Entry = {
  path: string[];
  segments: Segment[];
  header?: "table" | "array";
  value?: Value;
};

/** An inline table `{ ... }`, whose keys belong to `path`, or an array `[ ... ]`, whose items
 * are values of `path`. */
export type Container = {
  kind: "table" | "array";
  path: string[];
  from: number;
  to: number;
};

export type Scan = {
  entries: Entry[];
  containers: Container[];
  /** Each header and the path its keys belong to, in order. */
  sections: { path: string[]; from: number }[];
};

const BARE = /[A-Za-z0-9_-]/;

export function scan(text: string): Scan {
  const entries: Entry[] = [];
  const containers: Container[] = [];
  const sections: { path: string[]; from: number }[] = [];
  // How many tables each array of tables has had, so `[[a]]` keys go to the latest element.
  const arrays = new Map<string, number>();
  let pos = 0;
  let table: string[] = [];

  const peek = (s: string) => text.startsWith(s, pos);
  const eol = () => {
    while (pos < text.length && text[pos] !== "\n") pos++;
  };
  const spaces = () => {
    while (pos < text.length && (text[pos] === " " || text[pos] === "\t"))
      pos++;
  };
  // Spaces, newlines and comments, as inside an array or an inline table.
  const blank = () => {
    for (;;) {
      spaces();
      if (text[pos] === "\n" || text[pos] === "\r") pos++;
      else if (text[pos] === "#") eol();
      else return;
    }
  };

  // A quoted string at `pos`, returning its value, or undefined if it never closes on the line.
  function quoted(): string | undefined {
    const quote = text[pos];
    const multi = text.startsWith(quote.repeat(3), pos);
    const close = multi ? quote.repeat(3) : quote;
    pos += close.length;
    // A newline right after the opening delimiter of a multi-line string is not part of it.
    if (multi && text.startsWith("\r\n", pos)) pos += 2;
    else if (multi && text[pos] === "\n") pos++;
    let out = "";
    while (pos < text.length) {
      if (text.startsWith(close, pos)) {
        // A multi-line string may end in up to two quotes of its own before the delimiter.
        let run = 0;
        while (multi && text[pos + run] === quote && run < 5) run++;
        const extra = multi ? run - 3 : 0;
        out += quote.repeat(extra);
        pos += close.length + extra;
        return out;
      }
      const c = text[pos];
      if (!multi && c === "\n") return undefined;
      if (quote === '"' && c === "\\") {
        const next = text[pos + 1];
        const escapes: Record<string, string> = {
          n: "\n",
          t: "\t",
          r: "\r",
          '"': '"',
          "\\": "\\",
          b: "\b",
          f: "\f",
        };
        if (next in escapes) {
          out += escapes[next];
          pos += 2;
        } else if (next === "u" || next === "U") {
          const width = next === "u" ? 4 : 8;
          // Only the hex digits that are there, so a half-typed escape does not run past the
          // closing quote.
          const hex = /^[0-9a-fA-F]*/.exec(
            text.slice(pos + 2, pos + 2 + width),
          )![0];
          const code = parseInt(hex, 16);
          // Anything but a Unicode scalar value is a syntax error, which smol-toml reports.
          const scalar =
            hex.length === width &&
            code <= 0x10ffff &&
            (code < 0xd800 || code > 0xdfff);
          out += scalar ? String.fromCodePoint(code) : "";
          pos += 2 + hex.length;
        } else if (multi && (next === "\n" || next === " " || next === "\r")) {
          // A line-ending backslash trims the newline and the whitespace after it.
          pos++;
          while (pos < text.length && /\s/.test(text[pos])) pos++;
        } else {
          pos += 2;
        }
        continue;
      }
      out += c;
      pos++;
    }
    return undefined;
  }

  // A dotted key below `base`, or undefined if there is none at `pos`.
  function key(base: string[]): Segment[] | undefined {
    const segments: Segment[] = [];
    for (;;) {
      spaces();
      const from = pos;
      let name: string | undefined;
      if (text[pos] === '"' || text[pos] === "'") {
        name = quoted();
      } else {
        while (pos < text.length && BARE.test(text[pos])) pos++;
        name = pos > from ? text.slice(from, pos) : undefined;
      }
      if (name === undefined) return segments.length ? segments : undefined;
      const parent = segments.length
        ? segments[segments.length - 1].path
        : base;
      segments.push({ name, path: [...parent, name], from, to: pos });
      spaces();
      if (text[pos] !== ".") return segments;
      pos++;
    }
  }

  function value(path: string[]): Value {
    const from = pos;
    const c = text[pos];
    if (c === '"' || c === "'") {
      const scalar = quoted();
      return scalar === undefined
        ? { kind: "invalid", from, to: pos }
        : { kind: "string", from, to: pos, scalar };
    }
    if (c === "[") {
      pos++;
      const container: Container = {
        kind: "array",
        path,
        from,
        to: text.length,
      };
      containers.push(container);
      let index = 0;
      for (;;) {
        blank();
        if (pos >= text.length) break;
        if (text[pos] === "]") {
          pos++;
          break;
        }
        const before = pos;
        value([...path, String(index++)]);
        blank();
        if (text[pos] === ",") pos++;
        // Something that is not an element: give up on the rest of the array.
        else if (text[pos] !== "]" && pos === before) break;
      }
      container.to = pos;
      return { kind: "array", from, to: pos };
    }
    if (c === "{") {
      pos++;
      const container: Container = {
        kind: "table",
        path,
        from,
        to: text.length,
      };
      containers.push(container);
      for (;;) {
        blank();
        if (pos >= text.length) break;
        if (text[pos] === "}") {
          pos++;
          break;
        }
        const before = pos;
        keyValue(path);
        blank();
        if (text[pos] === ",") pos++;
        else if (text[pos] !== "}" && pos === before) {
          // Not a key: skip what cannot be read and stop at the end of the line.
          eol();
          break;
        }
      }
      container.to = pos;
      return { kind: "table", from, to: pos };
    }
    const rest = text.slice(pos, pos + 64);
    const word = /^(true|false)(?![A-Za-z0-9_-])/.exec(rest);
    if (word) {
      pos += word[0].length;
      return { kind: "boolean", from, to: pos, scalar: word[0] === "true" };
    }
    const date =
      /^\d{4}-\d{2}-\d{2}([Tt ]\d{2}:\d{2}(:\d{2}(\.\d+)?)?([Zz]|[+-]\d{2}:\d{2})?)?|^\d{2}:\d{2}(:\d{2}(\.\d+)?)?/.exec(
        rest,
      );
    if (date) {
      pos += date[0].length;
      return { kind: "datetime", from, to: pos };
    }
    const number =
      /^[+-]?(0x[0-9a-fA-F_]+|0o[0-7_]+|0b[01_]+|inf|nan|\d[\d_]*(\.\d[\d_]*)?([eE][+-]?\d[\d_]*)?)(?![A-Za-z0-9_.-])/.exec(
        rest,
      );
    if (number) {
      pos += number[0].length;
      const literal = number[0].replace(/_/g, "");
      const float =
        /[.eE]|inf|nan/.test(literal) && !/^[+-]?0[xob]/.test(literal);
      return float
        ? { kind: "float", from, to: pos, scalar: Number(literal) }
        : { kind: "integer", from, to: pos, scalar: Number(literal) };
    }
    // Not a value TOML knows: up to the next separator.
    while (pos < text.length && !/[,\]}\n#]/.test(text[pos])) pos++;
    return { kind: "invalid", from, to: pos };
  }

  // `key = value` below `base`.
  function keyValue(base: string[]) {
    const segments = key(base);
    if (!segments) return;
    const path = segments[segments.length - 1].path;
    const entry: Entry = { path, segments };
    entries.push(entry);
    spaces();
    if (text[pos] !== "=") return;
    pos++;
    spaces();
    if (pos >= text.length || text[pos] === "\n" || text[pos] === "#") return;
    entry.value = value(path);
  }

  // The path of a header's key, with each array of tables it passes through indexed.
  function indexed(names: string[], self: boolean): string[] {
    const out: string[] = [];
    names.forEach((name, i) => {
      out.push(name);
      const count = arrays.get(names.slice(0, i + 1).join("\u0000"));
      const last = i === names.length - 1;
      if (count !== undefined && !(last && self)) out.push(String(count - 1));
    });
    return out;
  }

  function header() {
    const from = pos;
    const array = peek("[[");
    pos += array ? 2 : 1;
    const segments = key([]);
    spaces();
    if (!segments) {
      eol();
      return;
    }
    const names = segments.map((s) => s.name);
    if (array) {
      const id = names.join("\u0000");
      arrays.set(id, (arrays.get(id) ?? 0) + 1);
    }
    // The header's keys, each with the path it names: an array of tables it passes through is
    // indexed by its latest element, but not the key itself.
    const base = indexed(names, true);
    const named = segments.map((s, i) => ({
      ...s,
      path: indexed(names.slice(0, i + 1), true),
    }));
    table = array
      ? [...base, String((arrays.get(names.join("\u0000")) ?? 1) - 1)]
      : base;
    entries.push({
      path: base,
      segments: named,
      header: array ? "array" : "table",
    });
    sections.push({ path: table, from });
    pos += array ? (peek("]]") ? 2 : 0) : peek("]") ? 1 : 0;
  }

  sections.push({ path: [], from: 0 });
  while (pos < text.length) {
    blank();
    if (pos >= text.length) break;
    if (text[pos] === "[") header();
    else keyValue(table);
    // Whatever is left of the line is not something this scan reads.
    spaces();
    if (text[pos] === "#") eol();
    if (pos < text.length && text[pos] !== "\n") eol();
  }
  return { entries, containers, sections };
}

/** The keys set directly in each table, keyed by the table's path joined with NUL. */
export function tableKeys(result: Scan): Map<string, Set<string>> {
  const keys = new Map<string, Set<string>>();
  const add = (parent: string[], name: string) => {
    const id = parent.join("\u0000");
    if (!keys.has(id)) keys.set(id, new Set());
    keys.get(id)!.add(name);
  };
  // Each written key belongs to the table its path leads to. An array's index is in a path but
  // never a segment, so it is not taken for a key, while a key such as `404` is.
  for (const entry of result.entries) {
    for (const segment of entry.segments) {
      add(segment.path.slice(0, -1), segment.name);
    }
  }
  return keys;
}

/** What the text at `pos` is: a key being typed in a table, a value, or a table header. */
export type Context =
  | { mode: "key"; table: string[]; from: number; typed: string }
  | { mode: "value"; path: string[]; from: number; typed: string }
  | { mode: "header"; from: number; typed: string };

export function contextAt(text: string, result: Scan, pos: number): Context {
  // The innermost inline table or array around `pos`, if any.
  const container = result.containers
    .filter(
      (c) =>
        c.from < pos && (pos < c.to || (c.to === text.length && pos === c.to)),
    )
    .sort((a, b) => b.from - a.from)[0];
  const lineStart = text.lastIndexOf("\n", pos - 1) + 1;
  const start = container
    ? Math.max(container.from + 1, lastSeparator(text, container.from + 1, pos))
    : lineStart;
  const before = text.slice(start, pos);
  const word = /[A-Za-z0-9_."'-]*$/.exec(before)![0];
  const from = pos - word.length;
  if (!container && /^\s*\[\[?[^\]]*$/.test(text.slice(lineStart, pos))) {
    const typed = /[A-Za-z0-9_."'-]*$/.exec(text.slice(lineStart, pos))![0];
    return { mode: "header", from: pos - typed.length, typed };
  }
  // In an array, whatever is typed is an item: a value of the array's key.
  if (container?.kind === "array")
    return { mode: "value", path: [...container.path, "0"], from, typed: word };
  const table = container ? container.path : sectionAt(result, pos);
  const eq = indexOfUnquoted(before, "=");
  if (eq === -1)
    return {
      mode: "key",
      table: [...table, ...dotted(word).slice(0, -1)],
      from: from + lastDot(word),
      typed: word.slice(lastDot(word)),
    };
  const keyText = before.slice(0, eq).trim();
  return {
    mode: "value",
    path: [...table, ...dotted(keyText)],
    from,
    typed: word,
  };
}

function sectionAt(result: Scan, pos: number): string[] {
  let path: string[] = [];
  for (const section of result.sections) {
    if (section.from <= pos) path = section.path;
  }
  return path;
}

// Where the current key-value of an inline table starts: after the last `,` or `{` outside quotes
// and nested brackets.
function lastSeparator(text: string, from: number, to: number): number {
  let depth = 0;
  let quote: string | null = null;
  let last = from;
  for (let i = from; i < to; i++) {
    const c = text[i];
    if (quote) {
      if (c === "\\" && quote === '"') i++;
      else if (c === quote) quote = null;
    } else if (c === '"' || c === "'") quote = c;
    else if (c === "[" || c === "{") depth++;
    else if (c === "]" || c === "}") depth--;
    else if (c === "," && depth === 0) last = i + 1;
  }
  return last;
}

function indexOfUnquoted(text: string, needle: string): number {
  let quote: string | null = null;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quote) {
      if (c === "\\" && quote === '"') i++;
      else if (c === quote) quote = null;
    } else if (c === '"' || c === "'") quote = c;
    else if (c === needle) return i;
  }
  return -1;
}

/** The keys of a dotted key as typed, unquoted. */
export function dotted(text: string): string[] {
  const out: string[] = [];
  const re = /\s*(?:"((?:[^"\\]|\\.)*)"|'([^']*)'|([A-Za-z0-9_-]+))\s*(\.|$)/y;
  let m: RegExpExecArray | null;
  while (re.lastIndex < text.length && (m = re.exec(text))) {
    out.push(m[1] ?? m[2] ?? m[3]);
    if (m[4] !== ".") break;
  }
  if (text.endsWith(".")) out.push("");
  return out.length ? out : [""];
}

function lastDot(word: string): number {
  return word.lastIndexOf(".") + 1;
}
