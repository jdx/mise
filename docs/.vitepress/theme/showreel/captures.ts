// The capture set as the reel reads it: the facts a render is given (load.ts
// builds them in node from docs/.vitepress/showreel-capture/out), and the
// accessors a scene uses to put a capture's real screen on the frame at any
// moment of the take. Every terminal line a scene draws comes from here, as
// the capture recorded it; every version number comes from `versions`.
// Nothing here draws, and nothing reads the file system.

import { type ReelFacts, TERM } from "./bible";
import { run, type Run } from "./kit/term";

/** The takes the reel shows, in capture order. C4 (the registry count) and C16 (off camera) have no screen. */
export const CAPTURE_IDS = [
  "C1",
  "C2",
  "C3",
  "C5",
  "C6",
  "C7",
  "C8",
  "C9",
  "C10",
  "C11",
  "C12",
  "C13",
  "C14",
  "C15",
  "C17",
  "C18",
  "C19",
] as const;
export type CaptureId = (typeof CAPTURE_IDS)[number];

export interface CaptureStyle {
  fg: string;
  bg: string;
  bold: boolean;
  dim: boolean;
}

export interface CaptureFrame {
  /** Seconds from the take's start. */
  t: number;
  /** Indices into `rows`, one per screen row. */
  rows: readonly number[];
  /** Column and row. */
  cursor: readonly [number, number];
  hidden: boolean;
}

export interface Capture {
  id: CaptureId;
  width: number;
  height: number;
  styles: readonly CaptureStyle[];
  /** Unique rows as runs of [text, style index]. */
  rows: readonly (readonly (readonly [string, number])[])[];
  frames: readonly CaptureFrame[];
  /** Each step's mark: the moment its output went quiet. */
  marks: readonly { name: string; t: number }[];
  /** Every key typed, with its time. */
  keys: readonly (readonly [number, string])[];
}

/** The capture run's versions file (versions.json), as far as the reel reads it. */
export interface Versions {
  mise: { version: string; version_line: string };
  node: {
    lts_major: number;
    lts_version: string;
    other_major: number;
    other_version: string;
  };
  tools: Readonly<Record<string, string>>;
}

/** Everything a render is given: the facts, typed. */
export interface ReelData {
  schema: 1;
  /** Which recorded run (a, b, …) the takes come from. */
  run: string;
  /** Machine 2's variant: "systemd" (a Watcher tile) or "plain". */
  variant: string;
  versions: Versions;
  captures: Readonly<Partial<Record<CaptureId, Capture>>>;
  /**
   * Files as each take left them ("C17/work/api/mise.toml"), as it started
   * where the rig snapshots that ("C8/start/work/api/mise.toml": the
   * `hk = "<hk_old>"` pin before the upgrade), and what the rig saved off
   * camera for a take ("C15/offcam/history-after.txt",
   * "C8/offcam/packslip-hk-<v>.json": the packslip statement mise saved
   * with that install).
   */
  files: Readonly<Record<string, string>>;
  /** The fixture files the takes started from: "api/.env". */
  fixtures: Readonly<Record<string, string>>;
  /** C4: the short names the pinned mise's `mise registry --hide-aliased` lists, and how many. */
  registry: { count: number; names: readonly string[] };
  /** Takes the set lacks. */
  missing: readonly string[];
}

/** The facts as capture data, or null when a render has none. */
export function reelData(facts: ReelFacts | null): ReelData | null {
  return facts && (facts as { schema?: unknown }).schema === 1
    ? (facts as unknown as ReelData)
    : null;
}

/** A take, or null when the set lacks it. */
export const capture = (d: ReelData | null, id: CaptureId): Capture | null =>
  d?.captures[id] ?? null;

/** A file as take `id` left it, or null. */
export const fileOf = (
  d: ReelData | null,
  id: CaptureId,
  path: string,
): string | null => d?.files[`${id}/${path}`] ?? null;

/** A fixture file, or null. */
export const fixture = (d: ReelData | null, path: string): string | null =>
  d?.fixtures[path] ?? null;

// Colours: captured SGR stays true to the capture, in the terminal's palette.

const NAMES = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
];

const hex2 = (n: number) => n.toString(16).padStart(2, "0");

/** An xterm 256-colour index as a CSS colour; the first 16 are the terminal's own. */
export function xterm256(n: number): string {
  if (n < 16) return TERM.ansi[n];
  if (n < 232) {
    const levels = [0, 95, 135, 175, 215, 255];
    const k = n - 16;
    const c = [Math.floor(k / 36), Math.floor(k / 6) % 6, k % 6];
    return `#${c.map((i) => hex2(levels[i])).join("")}`;
  }
  const g = 8 + 10 * (n - 232);
  return `#${hex2(g)}${hex2(g)}${hex2(g)}`;
}

/**
 * A captured style's background, or null for the terminal's own: the
 * highlighted choice in a confirm prompt (`  Yes  ` on pink) is a
 * background colour, which rowRuns carries as the run's `bg`.
 */
export function bgOf(st: CaptureStyle): string | null {
  const bg = st.bg;
  if (bg === "default") return null;
  const bright = bg.startsWith("bright");
  const i = NAMES.indexOf(bright ? bg.slice(6) : bg);
  if (i >= 0) return TERM.ansi[i + (bright ? 8 : 0)];
  if (/^#[0-9a-f]{6}$/i.test(bg)) return bg;
  const m = /^256:(\d+)$/.exec(bg);
  return m ? xterm256(Number(m[1])) : null;
}

/** A captured style's foreground. */
export function fgOf(st: CaptureStyle): string {
  if (st.dim) return TERM.dim;
  const fg = st.fg;
  if (fg === "default") return TERM.text;
  const bright = fg.startsWith("bright");
  const i = NAMES.indexOf(bright ? fg.slice(6) : fg);
  if (i >= 0) return TERM.ansi[i + (bright ? 8 : 0)];
  if (/^#[0-9a-f]{6}$/i.test(fg)) return fg;
  const m = /^256:(\d+)$/.exec(fg);
  if (m) return xterm256(Number(m[1]));
  return TERM.text;
}

// Screens.

/** Row `r` of the take as plain text. */
export const rowText = (c: Capture, r: number): string =>
  c.rows[r].map(([text]) => text).join("");

/** Row `r` as coloured runs, with any background the capture set (a confirm prompt's highlighted choice). */
export const rowRuns = (c: Capture, r: number): Run[] =>
  c.rows[r].map(([text, s]) =>
    run(
      text,
      fgOf(c.styles[s]),
      c.styles[s].bold,
      bgOf(c.styles[s]) ?? undefined,
    ),
  );

/** The index of the frame on screen at take time `t`: the last one at or before it. */
export function frameAt(c: Capture, t: number): number {
  let lo = 0;
  let hi = c.frames.length - 1;
  if (t < c.frames[0].t) return 0;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (c.frames[mid].t <= t + 1e-9) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

export interface ScreenOptions {
  /** Only the first `top` rows (a viewport crop, as C1's help screen). */
  top?: number;
  /**
   * Start at the last row that matches, dropping everything above it: a
   * later command's output alone, without the scrollback before it.
   */
  from?: RegExp;
  /**
   * Join soft-wrapped rows to their logical line: a row right after one that
   * filled the take's whole width (80 columns) is the rest of that line, cut
   * mid-word, and a pane narrower than the take (the side panes' 56 columns,
   * faded off the right edge) crops it away with the line's own end, instead
   * of showing it bright as an orphan fragment (`g.toml)`, `8.6 on x86_64…`).
   * A viewport crop (ART.md §13 Honesty): nothing is re-wrapped or edited.
   * The cursor on a joined row stays on its line, past its 80th column.
   * A row that filled all 80 columns and then ended on a hard newline looks
   * the same (lock's platform rows), so a RegExp joins only the rows after
   * a filled row it matches (daemons' `/^• \[/`, Postgres's log lines).
   */
  joinWraps?: boolean | RegExp;
}

export interface Screen {
  lines: Run[][];
  /** Where the cursor is in `lines`, or null while the take hides it. */
  cursor: { line: number; col: number } | null;
  /** The frame shown. */
  frame: number;
}

/**
 * Frame `i`'s rows as a screen with crops `o` shows them: the row indices
 * kept, in order, and where the cursor stands among them (-1: hidden or
 * cropped away). screenAt's rows, without building their runs.
 */
export function screenRows(
  c: Capture,
  i: number,
  o: ScreenOptions = {},
): { rows: number[]; cursor: { line: number; col: number } | null } {
  const f = c.frames[i];
  let rows = [...f.rows];
  if (o.top !== undefined) rows = rows.slice(0, o.top);
  let first = 0;
  if (o.from) {
    for (let r = rows.length - 1; r >= 0; r--)
      if (o.from.test(rowText(c, rows[r]))) {
        first = r;
        break;
      }
  }
  const cursorRow = f.hidden ? -1 : f.cursor[1];
  let last = rows.length - 1;
  while (last >= first && !rowText(c, rows[last]).trim() && last > cursorRow)
    last--;
  const kept: number[] = [];
  let cursor: { line: number; col: number } | null = null;
  let wraps = 0;
  for (let r = first; r <= last; r++) {
    // A continuation of a continuation (a line over 160 columns) joins
    // the same logical line.
    const cont =
      r > first && o.joinWraps && joinsNext(c, rows[r - 1], o, wraps);
    wraps = cont ? wraps + 1 : 0;
    if (!cont) kept.push(rows[r]);
    if (r === cursorRow)
      cursor = { line: kept.length - 1, col: f.cursor[0] + wraps * c.width };
  }
  if (cursor && cursor.line < 0) cursor = null;
  return { rows: kept, cursor };
}

/** Whether the row after row `r` is its soft-wrapped rest (ScreenOptions.joinWraps), `wraps` rows into a logical line. */
function joinsNext(
  c: Capture,
  r: number,
  o: ScreenOptions,
  wraps: number,
): boolean {
  const text = rowText(c, r);
  if (Array.from(text).length < c.width) return false;
  return (
    !(o.joinWraps instanceof RegExp) || wraps > 0 || o.joinWraps.test(text)
  );
}

/** The take's screen at take time `t`, trailing blank rows trimmed. */
export function screenAt(c: Capture, t: number, o: ScreenOptions = {}): Screen {
  const i = frameAt(c, t);
  const { rows, cursor } = screenRows(c, i, o);
  return { lines: rows.map((r) => rowRuns(c, r)), cursor, frame: i };
}

// Where things happen in a take.

/** One step of a take, from the mark before it to its own. */
export interface Step {
  /** The mark before it. */
  start: number;
  /** The first key typed, a moment before it. */
  type: number;
  /** The last key typed (Enter). */
  enter: number;
  /** Its own mark. */
  end: number;
}

export function stepOf(c: Capture, name: string): Step {
  const k = c.marks.findIndex((x) => x.name === name);
  if (k < 0) throw new Error(`${c.id} has no mark "${name}"`);
  const end = c.marks[k].t;
  const start = k > 0 ? c.marks[k - 1].t : 0;
  const keys = c.keys.filter(([t]) => t > start && t <= end);
  const type = keys.length ? keys[0][0] - 0.1 : start;
  const enter = keys.length ? keys[keys.length - 1][0] : start;
  return { start, type: Math.max(start, type), enter, end };
}

/** When key `key` was pressed, the first time after `after`. */
export function keyAt(c: Capture, key: string, after = -Infinity): number {
  const k = c.keys.find(([t, ch]) => t > after && ch === key);
  if (!k)
    throw new Error(`${c.id} has no key ${JSON.stringify(key)} after ${after}`);
  return k[0];
}

/**
 * The time of the first frame at or after `after` with a new row matching
 * `re`, or null. A row already on screen at `after` (scrollback from an
 * earlier command) does not count.
 */
export function firstWith(
  c: Capture,
  re: RegExp,
  after = -Infinity,
): number | null {
  const base = new Set(
    after > -Infinity ? c.frames[frameAt(c, after)].rows : [],
  );
  for (let i = 0; i < c.frames.length; i++) {
    const f = c.frames[i];
    if (f.t < after) continue;
    if (f.rows.some((r) => !base.has(r) && re.test(rowText(c, r)))) return f.t;
  }
  return null;
}

/**
 * The last frame before the first one at or after `after` with a row
 * matching `re`: the frame a scene cuts on to leave out what follows. With
 * no match, `fallback`.
 */
export function lastBefore(
  c: Capture,
  re: RegExp,
  after: number,
  fallback: number,
): number {
  const t = firstWith(c, re, after);
  if (t === null) return fallback;
  const i = frameAt(c, t - 1e-6);
  return Math.min(c.frames[i].t, t - 1e-4);
}

/** The first frame after the moment `t`: the screen once a key has landed. */
export function frameAfter(c: Capture, t: number): number {
  const f = c.frames.find((x) => x.t > t);
  return f ? f.t : c.frames[c.frames.length - 1].t;
}

/**
 * Where a take's shot must end: the first frame showing an install summary,
 * a task run's `Finished in`, a `mise WARN`, or a transfer rate in a
 * progress row. A scene cuts on the frame before it (lastBefore), so none of
 * it is ever on screen (test/forbidden.test.ts).
 */
export const CUT_ON =
  /installed \d+ tools? in|Finished in|mise WARN|\d(?:\.\d+)?\s?[kKMG]i?B\/s/;

// Version tokens, as the storyboard writes them.

/** The storyboard's version tokens with this capture run's values. */
export function tokens(d: ReelData | null): Record<string, string> | null {
  if (!d) return null;
  const n = d.versions.node;
  return {
    "node.lts_major": String(n.lts_major),
    "node.lts_version": n.lts_version,
    "node.other_major": String(n.other_major),
    "node.other_version": n.other_version,
  };
}

/** Text with `{node.lts_major}`-style tokens filled in; `?` where the facts have none. */
export function fill(text: string, tok: Record<string, string> | null): string {
  return text.replace(/\{([a-z_.]+)\}/g, (_, k: string) => tok?.[k] ?? "?");
}
