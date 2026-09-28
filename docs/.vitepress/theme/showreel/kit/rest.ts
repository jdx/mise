// What stays on the stage across each bar line: the persistent layers
// (the station card, the terminal's window and its screen, the chef, the
// folder diagram, and the file cards and rail a neighbour carries over), at
// rest, as the frame on the bar line draws them (handoff.ts) and as the
// scenes on either side draw them on their first and last frames. The
// whip is the one bar line that keeps nothing; everything else two
// neighbours share holds still through the line, so the reel never blinks
// to an empty stage between sections.
//
// A ticket clears the stage (STORYBOARD.md "Tickets hand the stage on"):
// the bar line into a ticket holds the outgoing section's layers at rest,
// the ticket takes them down under its printing paper and brings the next
// section's stage in under its lifting one (kit/grey.ts ticketStage), and
// the bar line out of it holds that next stage.
//
// A take's screen on a bar line is the one the incoming scene opens on
// (its prompt before the first key: OPENS), the one the outgoing scene
// holds last (HOLDS), or, where one shell carries on across the line
// (bootstrap into breath into clone), the very frame both show. The take
// times live here so a scene cuts to exactly the frame its bar line holds.
//
// Everything is placed on the layout grid (kit/style.ts LAYOUT, PANES,
// FOLDERS_ART, CHEF_ART), and every badge a screen carries rides its
// window's chrome bar (ART.md §6), never above it.

import { BEAT, type LitRect } from "../bible";
import { lerp, progress } from "../math";
import {
  type Capture,
  type CaptureId,
  CUT_ON,
  capture,
  fileOf,
  firstWith,
  fixture,
  frameAt,
  lastBefore,
  type ReelData,
  rowText,
  type ScreenOptions,
  screenAt,
  screenRows,
  stepOf,
} from "../captures";
import type { BoundaryId } from "../handoff";
import { chefLit, drawChefRest } from "./chef";
import { type Folder, folders } from "./diagrams/folders";
import type { Rect } from "./motion";
import { endCardRects } from "./namecard";
import {
  badgePop,
  CARD,
  card,
  ENV_CARD,
  ENV_PANE,
  fileCard,
  footnote,
  LEFT,
  missing,
  rail,
  rectOf,
} from "./parts";
import { CHEF_ART, DUR, EASE, LAYOUT, PANES } from "./style";
import {
  type ChromeBadge,
  drawTerm,
  drawWindow,
  FIT_MIN,
  lineText,
  type Pane,
  paneHeight,
  paneRows,
  termLayout,
  termLit,
} from "./term";

// The take times the bar lines hold.

/** Where each take's first shot opens: its prompt, just before the first key. */
export const OPENS: Partial<Record<CaptureId, (c: Capture) => number>> = {
  C2: (c) => stepOf(c, "cd").type,
  C3: (c) => stepOf(c, "not-found").type,
  C7: (c) => stepOf(c, "cd-dashboard").type,
  C9: (c) => stepOf(c, "cd-out").type,
  C11: (c) => stepOf(c, "ci").type,
  // skip's second run, from its own prompt (SKIP_SCREEN crops the first run's output away).
  C12: (c) => stepOf(c, "ci-skip").type,
  C13: (c) => stepOf(c, "help").type,
  C14: (c) => stepOf(c, "cd").type,
  C15: (c) => stepOf(c, "confirm").type,
  C17: (c) => stepOf(c, "lock").type,
  C18: (c) => stepOf(c, "confirm-1").type,
  // C19 opens on the screen its Ctrl-L cleared.
  C19: (c) => c.frames[0].t,
};

/**
 * The last frame of step `name` a shot may hold: its own mark (the moment
 * its output went quiet), or the frame before anything CUT_ON names (an
 * install summary, `Finished in`, a `mise WARN`, a transfer rate).
 */
export const holdOf = (c: Capture, name: string): number => {
  const s = stepOf(c, name);
  return lastBefore(c, CUT_ON, s.type, s.end);
};

/**
 * Where each take's last shot holds, when the scene's outgoing bar line
 * keeps its screen up for the next section (a ticket then takes it down).
 */
export const HOLDS: Partial<Record<CaptureId, (c: Capture) => number>> = {
  // pitch: `mise run ci`, on its last frame before `Finished in`.
  C2: (c) => holdOf(c, "ci"),
  // backends: `gh --version`'s two lines, at the prompt.
  C6: (c) => holdOf(c, "gh"),
  // switch: back in api, `node --version`'s line and the prompt after it.
  C7: (c) => holdOf(c, "node-api"),
  // packslip: `mise skills sync` and its `linked` rows, at the prompt.
  C8: (c) => holdOf(c, "sync"),
  // vars: `echo PORT=$PORT` -> `PORT=3000`, at the prompt.
  C9: (c) => holdOf(c, "port"),
  // redact: the deploy task's line with `[redacted]`, at the prompt.
  C10: (c) => holdOf(c, "deploy"),
  // daemons: the server's version under `[db] $ psql …`, at the prompt.
  C14: (c) => holdOf(c, "db"),
  // track: `mise dot rollback`'s last line, at the prompt.
  C15: (c) => holdOf(c, "rollback"),
};

/**
 * C12's screen as skip shows it: from the second run's prompt, the first
 * run's output (and its `Finished in`) cropped away above it.
 */
export const SKIP_SCREEN: ScreenOptions = { from: /^~\/work\/api \$/ };

/** The capture rig's terminal width: a row this long wrapped onto the next (a soft wrap). */
const COLS = 80;

/**
 * A lineAlpha that dims soft-wrapped continuation rows to `level`: a row
 * right after one that filled all 80 columns is the rest of that logical
 * line, cut mid-word, whose start the 56-column side pane has already
 * faded off its right edge, so shown bright it reads as an orphan
 * fragment (`g.toml)`, `8.6 on x86_64…`). Rows `also` names dim with it.
 */
export const dimWraps =
  (level: number, also?: RegExp) =>
  (i: number, lines: readonly string[]): number =>
    (i > 0 && Array.from(lines[i - 1]).length >= COLS) ||
    (also !== undefined && also.test(lines[i]))
      ? level
      : 1;

/**
 * Postgres's own log lines (`• [shop/postgres] … LOG: …`) and every
 * soft-wrapped continuation row, dimmed to 25 %: texture under pitchfork's
 * spinner, the started line and the task's output, so no fragment of a
 * log line (the `8.6` of `PostgreSQL 18.6`) reads beside the payoff.
 */
export const LOG_DIM = dimWraps(0.25, /^• \[/);

/**
 * Where bootstrap's shot holds: the last frame before the tools step's
 * first progress row, with `mise bootstrap: tools` its last line. The take
 * installs Node off camera from there; its rows move for 0.2 s before a
 * transfer rate appears, too short to play.
 */
export const BOOTSTRAP_HOLD = (c: Capture): number => {
  const done = stepOf(c, "done");
  return lastBefore(c, /^mise by @jdx/, done.type, done.end);
};

/**
 * Bootstrap's prose paragraphs, dimmed to texture (22 %): every row but the
 * command, the repository line, the plan table and mise's own log lines
 * (`mise bootstrap: tools`, `mise files: …`). A prose row that happens to
 * start with "mise " (`mise bootstrap` or renders templates…, a wrap of
 * the paragraph) is prose, not a log line.
 */
export const BOOTSTRAP_DIM = (i: number, lines: readonly string[]): number =>
  /^(~ \$|https:\/\/|Path |~\/|Set this|Wrote|mise [a-z][a-z ]*: |user services)/.test(
    lines[i],
  ) || !lines[i].trim()
    ? 1
    : 0.22;

// Badges in a window's chrome bar.

/**
 * The badges a terminal's chrome bar carries (ART.md §6), in the order
 * they stand from its right end: while install rows move faster than real
 * time, while a row names a placeholder repository, and on the fresh
 * machine.
 */
export const BADGES = {
  lapse: "time-lapse",
  illustration: "illustration",
  fresh: "fresh machine · mise installed",
} as const;

const ORDER: readonly string[] = Object.values(BADGES);

/** Badges in their chrome-bar order, right to left (ART.md §6); any other after them. */
export function orderBadges(
  list: readonly (string | ChromeBadge)[],
): (string | ChromeBadge)[] {
  const rank = (b: string | ChromeBadge) => {
    const i = ORDER.indexOf(typeof b === "string" ? b : b.text);
    return i < 0 ? ORDER.length : i;
  };
  return [...list].sort((a, b) => rank(a) - rank(b));
}

/** The fresh machine's badge, in the terminal's chrome bar from `new` into bootstrap. */
export const FRESH = BADGES.fresh;

/** A placeholder repository in a row: the illustration badge must be up. */
export const PLACEHOLDER = /\byou\/(api|setup)\b/;

/**
 * A placeholder repository as it is being typed: a row ending in `you/`
 * and the start of `api` or `setup`. The illustration badge comes up on
 * the first frame that shows one, not once the name is whole, so
 * `git clone https://github.com/you/a` is badged too. Only at the row's
 * end, where the cursor types it: `/home/you/…` paths (packslip's skills
 * links) are the fixture user's home, not a placeholder.
 */
export const PLACEHOLDER_TYPED = /\byou\/(?:a|ap|s|se|set|setu)?$/;

/** Whether a row shows a placeholder repository, whole or being typed. */
export const namesPlaceholder = (row: string): boolean =>
  PLACEHOLDER.test(row) || PLACEHOLDER_TYPED.test(row.trimEnd());

/**
 * When a take shows a placeholder, take seconds: each span from the first
 * frame showing a `you/` to the next Ctrl-L that clears the screen (or
 * Infinity), and when the take first printed a whole one (or null).
 */
const placeholders = new WeakMap<
  Capture,
  { spans: { from: number; to: number }[]; t0: number | null }
>();

function placeholderSpans(c: Capture): {
  spans: { from: number; to: number }[];
  t0: number | null;
} {
  let got = placeholders.get(c);
  if (got) return got;
  const clears = c.keys.filter(([, k]) => k === "\f").map(([t]) => t);
  const spans: { from: number; to: number }[] = [];
  let open: number | null = null;
  for (const f of c.frames) {
    if (open !== null) {
      const clear = clears.find((t) => t > open! && t <= f.t);
      if (clear === undefined) continue;
      spans.push({ from: open, to: clear });
      open = null;
    }
    if (f.rows.some((r) => namesPlaceholder(rowText(c, r)))) open = f.t;
  }
  if (open !== null) spans.push({ from: open, to: Infinity });
  got = { spans, t0: firstWith(c, PLACEHOLDER) };
  placeholders.set(c, got);
  return got;
}

/**
 * The badge a screen's chrome bar carries for what its rows say:
 * "illustration" while a shown row names a placeholder repository, and
 * (sticky) from the first frame that shows one being typed until the
 * screen is cleared, so it never blinks off while the name scrolls away
 * and back (ART.md §6). It pops in (badgePop) where the span starts and
 * fades out over DUR.badge from the Ctrl-L that clears it, by take time
 * `ct`.
 */
export function screenBadges(
  c: Capture,
  ct: number,
  shown: readonly string[],
): ChromeBadge[] {
  const { spans, t0 } = placeholderSpans(c);
  const span = spans.find((s) => s.from <= ct + 1e-9 && ct < s.to);
  if (span) {
    const pop = badgePop((ct - span.from) / BEAT);
    return [{ text: BADGES.illustration, ...pop }];
  }
  if (shown.some((l) => PLACEHOLDER.test(l))) {
    const pop = badgePop(t0 === null ? 1 : (ct - t0) / BEAT);
    return [{ text: BADGES.illustration, ...pop }];
  }
  // Cleared: it fades out over DUR.badge (ART.md §9: leaves over 1/4).
  const gone = spans.find((s) => ct >= s.to && ct < s.to + DUR.badge * BEAT);
  if (gone)
    return [
      {
        text: BADGES.illustration,
        alpha: 1 - (ct - gone.to) / (DUR.badge * BEAT),
      },
    ];
  return [];
}

// Scrolling: a terminal never pans back up.

/** Per take and pane height and screen crop: each frame's first line shown, never scrolling back. */
const scrolls = new WeakMap<Capture, Map<string, Int32Array>>();

/** How many lines screenAt gives frame `i` (trailing blanks trimmed, the cursor's row kept). */
const linesAt = (c: Capture, i: number, o: ScreenOptions): number =>
  screenRows(c, i, o).rows.length;

/**
 * The first line a pane `rows` rows tall shows of take `c`'s screen at take
 * time `ct`, as a real terminal scrolls: its bottom rows, but never back
 * up. When a prompt widget collapses (a confirm's Yes/No and hint rows
 * folding to one `? Yes` line), the screen gets shorter; a real terminal
 * leaves blank rows at the bottom, where the next output prints, instead of
 * panning the lines that scrolled off back down. A clear (the screen's
 * last line above the first shown) starts afresh. A pure function of take
 * time: each frame's answer is precomputed once per take, pane height and
 * crop.
 */
export function takeScroll(
  c: Capture,
  ct: number,
  rows: number,
  o: ScreenOptions = {},
): number {
  let byKey = scrolls.get(c);
  if (!byKey) scrolls.set(c, (byKey = new Map()));
  const key = `${rows}|${o.top ?? ""}|${o.from?.source ?? ""}|${String(o.joinWraps ?? "")}`;
  let firsts = byKey.get(key);
  if (!firsts) {
    firsts = new Int32Array(c.frames.length);
    let m = 0;
    for (let i = 0; i < c.frames.length; i++) {
      const n = linesAt(c, i, o);
      const own = Math.max(0, n - rows);
      m = m > n - 1 ? own : Math.max(m, own);
      firsts[i] = m;
    }
    byKey.set(key, firsts);
  }
  return firsts[frameAt(c, ct)];
}

/**
 * How a take's screen scrolls in a pane: "monotonic" (takeScroll: never
 * back up; the default for a take's screen), "plain" (kit/term.ts
 * firstLine: always its bottom rows), or a fixed
 * first line.
 */
export type TakeScroll = "monotonic" | "plain" | number;

// Fitting: a pane as tall as its take needs.

/** Per take, pane, floor and crop: each frame's rows needed so far, and when that last grew. */
const fits = new WeakMap<
  Capture,
  Map<string, { need: Int16Array; from: Int16Array; at: Float64Array }>
>();

/**
 * Pane `p` sized to take `c`'s screen at take time `ct` (kit/term.ts
 * fitPane): as many rows as the take has needed so far, at least `min`
 * (FIT_MIN) and at most the preset's, the same top-left corner. When
 * output needs another row, the bottom edge grows to it over DUR.tick (of
 * take time) on EASE.move, and the new row shows as the edge clears it; it
 * never shrinks on its own (a scene shrinks a pane with lerpPane). A pure
 * function of take time. The lit rect (restLit) stays the preset's.
 */
export function fittedPane(
  c: Capture,
  ct: number,
  p: Pane,
  o: { min?: number; screen?: ScreenOptions } = {},
): Pane {
  const min = Math.min(o.min ?? FIT_MIN, p.rows);
  let byKey = fits.get(c);
  if (!byKey) fits.set(c, (byKey = new Map()));
  const sc = o.screen ?? {};
  const key = `${p.rows}|${min}|${sc.top ?? ""}|${sc.from?.source ?? ""}|${String(sc.joinWraps ?? "")}`;
  let f = byKey.get(key);
  if (!f) {
    const n = c.frames.length;
    f = {
      need: new Int16Array(n),
      from: new Int16Array(n),
      at: new Float64Array(n),
    };
    let need = min;
    let from = min;
    let at = -Infinity;
    for (let i = 0; i < n; i++) {
      const want = Math.min(p.rows, Math.max(need, linesAt(c, i, sc)));
      if (want > need) {
        from = need;
        need = want;
        at = c.frames[i].t;
      }
      f.need[i] = need;
      f.from[i] = from;
      f.at[i] = at;
    }
    byKey.set(key, f);
  }
  const i = frameAt(c, ct);
  const k = EASE.move(progress(f.at[i], f.at[i] + DUR.tick * BEAT, ct));
  const rows = f.need[i];
  const h = lerp(paneHeight(p, f.from[i]), paneHeight(p, rows), k);
  return { ...p, h: Math.min(p.h, h), rows, fitRows: rows };
}

/** The first line shown for take `c` at `ct` in pane `p`, under `scroll`; undefined for firstLine's own. */
export function scrollOf(
  c: Capture,
  ct: number,
  p: Pane,
  scroll: TakeScroll = "monotonic",
  o: ScreenOptions = {},
): number | undefined {
  if (typeof scroll === "number") return scroll;
  if (p.anchor === "top") return undefined;
  const rows = p.fitRows ?? Math.floor(paneRows(p) + 1e-9);
  if (scroll === "plain")
    return p.fitRows === undefined
      ? undefined
      : Math.max(0, screenAt(c, ct, o).lines.length - rows);
  return takeScroll(c, ct, rows, o);
}

/** The rows of a screen of these lines that pane `p` shows (scrolled to `scroll`, default its bottom rows). */
export function shownRows(
  texts: readonly string[],
  p: Pane,
  scroll?: number,
): string[] {
  const first = Math.max(
    0,
    Math.floor(termLayout(p, texts.length, scroll).first),
  );
  return texts.slice(first, first + Math.ceil(paneRows(p)));
}

/**
 * A take's screen at take time `ct` in pane `p`, text only (the window is
 * drawn apart), its chrome bar's title and badges with it: `badges` (the
 * scene's: time-lapse, the fresh machine) and the illustration badge its
 * rows call for, in their order (orderBadges).
 */
export function drawScreen(
  ctx: CanvasRenderingContext2D,
  p: Pane,
  c: Capture,
  ct: number,
  t: number,
  o: {
    screen?: ScreenOptions;
    alpha?: number;
    lineAlpha?: (i: number, lines: readonly string[]) => number;
    /** 0 to 1: dimmed (PANE_DIM), text and title. */
    dim?: number;
    badges?: readonly (string | ChromeBadge)[];
    /** How the screen scrolls (default "monotonic": never back up; scrollOf). */
    scroll?: TakeScroll;
  } = {},
): ReturnType<typeof drawTerm> {
  const sc = screenAt(c, ct, o.screen ?? {});
  const texts = sc.lines.map(lineText);
  const la = o.lineAlpha;
  const scroll = scrollOf(c, ct, p, o.scroll, o.screen);
  return drawTerm(ctx, { ...p, window: false }, sc.lines, {
    t,
    alpha: o.alpha ?? 1,
    dim: o.dim,
    scroll,
    cursor: sc.cursor ?? undefined,
    lineAlpha: la ? (i) => la(i, texts) : undefined,
    badges: orderBadges([
      ...(o.badges ?? []),
      ...screenBadges(c, ct, shownRows(texts, p, scroll)),
    ]),
  });
}

// The rests.

interface CardRest {
  kind: "card";
  rect: Rect;
  title: string;
  from: CaptureId;
  path: string;
  open: readonly string[];
  hide?: RegExp;
  lit?: Readonly<Record<string, number>>;
  size?: number;
  /**
   * Size the card to its rows (kit/parts.ts CardOptions.fit, cardFit):
   * both scenes must draw it with the same `fit` on their side.
   */
  fit?: boolean | { min?: number };
}

interface PaneRest {
  kind: "pane";
  pane: Pane;
  /** A take's screen in the window, or none: an empty window between two takes. */
  take?: CaptureId;
  at?: (c: Capture) => number;
  screen?: ScreenOptions;
  lineAlpha?: (i: number, lines: readonly string[]) => number;
  /** 0 to 1: the pane dimmed (PANE_DIM): body, text and title. */
  dim?: number;
  /** Badges its chrome bar carries at rest (the fresh machine's). */
  badges?: readonly string[];
  /** How its screen scrolls (drawScreen; default "monotonic"). */
  scroll?: TakeScroll;
  /**
   * Size the window to the take (fittedPane at the rest's take time), at
   * least this many rows (true: FIT_MIN). Both scenes must draw the same
   * fit on their side (shot's `fit`, or fittedPane).
   */
  fit?: boolean | number;
}

interface FileRest {
  kind: "file";
  rect: Rect;
  title: string;
  lines: (d: ReelData | null) => string[] | null;
  from: string;
  size?: number;
}

/** A checkpoint on the rail: its id and trigger, and which checkpoint a rollback restored it from. */
export interface Checkpoint {
  id: string;
  label: string;
  from?: number;
}

interface RailRest {
  kind: "rail";
  rect: Rect;
  /** The checkpoints on it, all made; none for an empty rail. */
  dots?: (d: ReelData | null) => readonly Checkpoint[];
}

export type Layer =
  | CardRest
  | PaneRest
  | FileRest
  | RailRest
  /** switch's folder diagram before its first `cd`: api current, no versions yet. */
  | { kind: "folders" }
  /**
   * A static footnote (kit/parts.ts footnote) held through a bar line with
   * what it annotates: redact's, while `[redacted]` is on screen, into the
   * Tasks ticket, which takes it down with the rest of the stage.
   */
  | { kind: "footnote"; text: string }
  | { kind: "chef" };

/** The api project's card, as take `from` left it. */
const apiCard = (
  from: CaptureId,
  open: readonly string[],
  o: Partial<CardRest> = {},
): CardRest => ({
  kind: "card",
  rect: CARD,
  title: "~/work/api/mise.toml",
  from,
  path: "work/api/mise.toml",
  open,
  ...o,
});

/** A pane opening on take `take`'s first prompt (OPENS), or an empty window. */
const pane = (
  p: Pane,
  take?: CaptureId,
  o: Partial<PaneRest> = {},
): PaneRest => ({
  kind: "pane",
  pane: p,
  take,
  at: take ? OPENS[take] : undefined,
  ...o,
});

/** A pane holding take `take`'s last shot (HOLDS). */
const held = (
  p: Pane,
  take: CaptureId,
  o: Partial<PaneRest> = {},
): PaneRest => ({
  kind: "pane",
  pane: p,
  take,
  at: HOLDS[take],
  ...o,
});

/** switch's folders: api/ and dashboard/, each with its mise.toml as C7 left it. */
export const switchFolders = (d: ReelData | null): Folder[] => [
  { name: "api/", toml: fileOf(d, "C7", "work/api/mise.toml") },
  { name: "dashboard/", toml: fileOf(d, "C7", "work/dashboard/mise.toml") },
];

/** The folders' parent, over the tree. */
export const SWITCH_ROOT = "~/work/";

/** The `~/.zshrc` card and the checkpoint rail of the Dotfiles act (LAYOUT.zshrc, LAYOUT.rail). */
export const ZSHRC: Rect = LAYOUT.zshrc;
export const RAIL: Rect = LAYOUT.rail;
export const zshrcLines = (d: ReelData | null): string[] | null =>
  fixture(d, "home/.zshrc")?.trim().split("\n") ?? null;

/**
 * The checkpoints of ~/.zshrc after track's rollback, oldest first: its id
 * and trigger from C15's history listing after the rollback (off camera),
 * the last one restored from the first. The rail shows the first three.
 */
export function checkpoints(d: ReelData | null): Checkpoint[] {
  const text = fileOf(d, "C15", "offcam/history-after.txt");
  if (!text) return [];
  const all = text
    .split("\n")
    .map((l) => /^(\d+)\s+\S+ \S+\s+(\S+)\s+.*~\/\.zshrc/.exec(l))
    .filter((m): m is RegExpExecArray => m !== null)
    .map((m) => ({ id: m[1], label: m[2] }))
    .sort((a, b) => Number(a.id) - Number(b.id))
    .slice(0, 3);
  return all.map((cp, i) => (i === 2 ? { ...cp, from: 0 } : cp));
}

/** The climax's card: folded to its three headers, clear of the big type (LAYOUT.compact). */
export const COMPACT: Rect = LAYOUT.compact;

/** The tables of the cloned project's card open on breath|clone: what the clone installs and sets, before the card folds to its headers. */
export const CLONE_OPEN = ["[tools]", "[env]"] as const;

/**
 * The card breath and clone show: the cloned project's own file (C19's
 * `api/mise.toml`, at `~/api` on the fresh machine), `open` tables open
 * (CLONE_OPEN on breath|clone, so clone's "the card folds to its three
 * headers" is a real fold; none on clone|morph).
 */
export const CLONE_CARD = (
  lit: Readonly<Record<string, number>> = {},
  rect: Rect = CARD,
  open: readonly string[] = [],
): CardRest => ({
  kind: "card",
  rect,
  title: "~/api/mise.toml",
  from: "C19",
  path: "api/mise.toml",
  open,
  lit,
});

/** redact's footnote: what redaction does, and does not do. */
export const REDACT_NOTE =
  "Masks captured task output. Does not encrypt the file.";

/** Every table of the pitch's card lit: the pitch teaches the palette. */
const ALL_LIT = { "[tools]": 1, "[env]": 1, "[tasks": 1 } as const;

/**
 * Every bar line's persistent layers, drawn in this order. The whip's
 * keeps none. A bar line into a ticket keeps what the outgoing section
 * ends on; the ticket takes it down (kit/grey.ts ticketStage).
 */
export const RESTS: Record<BoundaryId, readonly Layer[]> = {
  // The pitch's terminal is as tall as its take needs (PaneRest.fit): six
  // rows at `~/work $`, growing as `mise run ci` prints (pitch's shot fits
  // the same way), so it never waits as one row in a 570 px window.
  "open|pitch": [pane(LEFT, "C2", { fit: true })],
  // Act 0 and Act I's card is as tall as its rows (CardRest.fit), so a
  // file of four to nine rows never stands in a 570 px box that is half
  // empty; pitch, use, registry and backends draw it with the same fit.
  "pitch|tools": [
    apiCard("C2", ["[tools]", "[env]", "[tasks.ci]"], {
      hide: /^run = /,
      lit: ALL_LIT,
      fit: true,
    }),
    held(LEFT, "C2", { fit: true }),
  ],
  // use's short terminal (six rows, every screen C3 shows), with its slam
  // under it as backends has them; the Dev tools ticket brings it in.
  "tools|use": [
    apiCard("C2", ["[tools]"], { fit: true }),
    pane(PANES.slip, "C3"),
  ],
  "use|registry": [apiCard("C3", ["[tools]"], { fit: true })],
  "registry|backends": [apiCard("C3", ["[tools]"], { fit: true })],
  // backends holds gh's pane, narrowed back into the left column, beside
  // the card: the Versions ticket takes both down (the section before a
  // ticket never clears its own stage).
  "backends|versions": [
    apiCard("C5", ["[tools]"], { fit: true }),
    held(PANES.slip, "C6"),
  ],
  "versions|switch": [{ kind: "folders" }, pane(LEFT, "C7")],
  // switch's last screen holds through its bar lines (a titled screen,
  // never an empty window); the next scene dissolves it into its own
  // opening prompt over its first quarter beat (restScreen). The station
  // card switch's api/ folder became holds with it, C7's file at [tools],
  // so it never dips out across the line (scenes/g3-versions-env/rests.ts
  // STATION). It is as tall as its rows (CardRest.fit) here and on
  // packslip|env, as Act 0 and Act I's is: a few [tools] rows never stand
  // in a 570 px box, and packslip's rows seating grow it on both sides.
  "switch|packslip": [
    held(LEFT, "C7"),
    apiCard("C7", ["[tools]"], { fit: true }),
  ],
  "packslip|env": [
    apiCard("C8", ["[tools]"], { fit: true }),
    held(PANES.slip, "C8", { dim: 1 }),
  ],
  "env|vars": [
    apiCard("C8", ["[env]"], { rect: ENV_CARD }),
    pane(ENV_PANE, "C9"),
  ],
  "vars|redact": [
    apiCard("C9", ["[env]"], { rect: ENV_CARD, lit: { "[env]": 1 } }),
    held(ENV_PANE, "C9"),
  ],
  "redact|tasks": [
    apiCard("C10", [], { rect: ENV_CARD }),
    held(ENV_PANE, "C10"),
    // For as long as `[redacted]` shows (STORYBOARD.md redact).
    { kind: "footnote", text: REDACT_NOTE },
  ],
  "tasks|depends": [apiCard("C11", ["[tasks.ci]"]), pane(LEFT, "C11")],
  // Act IV's seams rest on the next take's own prompt, never an empty,
  // untitled window: the outgoing scene brings it up (restScreen), the
  // incoming one opens on it.
  "depends|skip": [
    apiCard("C11", ["[tasks.ci]"], { lit: { "[tasks": 1 } }),
    pane(LEFT, "C12", { screen: SKIP_SCREEN }),
  ],
  // skip's card holds into args, which slides it out as its script card
  // comes in (ART.md §7 "Title change").
  "skip|args": [
    apiCard("C12", ["[tasks.ci]", "[tasks.build]"], {
      lit: { "[tasks": 1 },
    }),
    pane(LEFT, "C13"),
  ],
  "args|daemons": [pane(LEFT, "C14")],
  // daemons holds its terminal (the server's version) into the ticket,
  // which takes it down with the card (STORYBOARD.md: the section before
  // a ticket never clears its own stage).
  "daemons|dotfiles": [
    {
      kind: "card",
      rect: CARD,
      title: "~/work/shop/mise.toml",
      from: "C14",
      path: "work/shop/mise.toml",
      open: ["[daemons]", "[tasks.db]"],
      lit: { "[daemons]": 1 },
    },
    held(LEFT, "C14", { lineAlpha: LOG_DIM }),
  ],
  "dotfiles|track": [
    {
      kind: "file",
      rect: ZSHRC,
      title: "~/.zshrc",
      lines: zshrcLines,
      from: "fixtures",
    },
    { kind: "rail", rect: RAIL },
    pane(LEFT, "C15"),
  ],
  "track|machines": [
    {
      kind: "file",
      rect: ZSHRC,
      title: "~/.zshrc",
      lines: zshrcLines,
      from: "fixtures",
    },
    { kind: "rail", rect: RAIL, dots: checkpoints },
    held(LEFT, "C15"),
  ],
  "machines|lock": [apiCard("C17", ["[tools]"]), pane(LEFT, "C17")],
  "lock|new": [],
  "new|bootstrap": [pane(LEFT, "C18", { badges: [FRESH] })],
  "bootstrap|breath": [
    {
      kind: "pane",
      pane: LEFT,
      take: "C18",
      at: BOOTSTRAP_HOLD,
      lineAlpha: BOOTSTRAP_DIM,
    },
  ],
  "breath|clone": [CLONE_CARD({}, CARD, CLONE_OPEN), pane(LEFT, "C19")],
  "clone|morph": [CLONE_CARD(ALL_LIT, COMPACT)],
  "morph|end": [{ kind: "chef" }],
};

/**
 * The chef at rest on morph|end at GLOBAL time `t`: at CHEF_ART.place with
 * its halo and its motes, kept clear of the end card's lines. It is the end
 * card's first frame (namecard.ts drawEndCard at 0) and the morph's last
 * (chef.ts drawMorphChef with `avoid: endCardRects(ctx)`).
 */
export const chefRest = (ctx: CanvasRenderingContext2D, t: number): void =>
  drawChefRest(ctx, t, CHEF_ART.place, { avoid: endCardRects(ctx) });

/** A terminal's window alone: the pane a scene keeps up across a bar line, dimmed by `dim` (PANE_DIM). */
export const drawPaneWindow = (
  ctx: CanvasRenderingContext2D,
  p: Pane,
  dim = 0,
): void => drawWindow(ctx, rectOf(p), p.chrome, { dim });

/**
 * A pane layer's screen alone (no window): its take at its rest time, with
 * its title and badges, at `alpha`; the missing card when the set lacks the
 * take. What restScreen draws, and drawLayer over the window.
 */
export function drawPaneScreen(
  ctx: CanvasRenderingContext2D,
  l: Extract<Layer, { kind: "pane" }>,
  d: ReelData | null,
  t: number,
  alpha = 1,
  pane: Pane = l.pane,
): void {
  if (!l.take || alpha <= 0) return;
  const c = capture(d, l.take);
  if (!c) {
    missing(ctx, rectOf(l.pane), l.take, alpha);
    return;
  }
  drawScreen(ctx, pane, c, l.at!(c), t, {
    alpha,
    screen: l.screen,
    lineAlpha: l.lineAlpha,
    dim: l.dim,
    badges: l.badges,
    scroll: l.scroll,
  });
}

/** Draw one persistent layer at rest, at GLOBAL time `t`. */
export function drawLayer(
  ctx: CanvasRenderingContext2D,
  l: Layer,
  d: ReelData | null,
  t: number,
): void {
  switch (l.kind) {
    case "card":
      card(ctx, l.rect, {
        title: l.title,
        text: fileOf(d, l.from, l.path),
        from: l.from,
        open: l.open,
        hide: l.hide,
        lit: l.lit,
        size: l.size,
        fit: l.fit,
      });
      return;
    case "file":
      fileCard(ctx, l.rect, {
        title: l.title,
        lines: l.lines(d),
        from: l.from,
        size: l.size,
      });
      return;
    case "pane": {
      const c = l.take ? capture(d, l.take) : null;
      const ct = c ? l.at!(c) : 0;
      const p =
        c && l.fit
          ? fittedPane(c, ct, l.pane, {
              min: typeof l.fit === "number" ? l.fit : undefined,
              screen: l.screen,
            })
          : l.pane;
      drawPaneWindow(ctx, p, l.dim);
      drawPaneScreen(ctx, l, d, t, 1, p);
      return;
    }
    case "rail": {
      const dots = (l.dots?.(d) ?? []).map((cp) => ({ ...cp, on: 1 }));
      rail(ctx, l.rect, dots, dots.length - 1, 1);
      return;
    }
    case "folders":
      folders(ctx, {
        folders: switchFolders(d),
        b: 0,
        cwd: 0,
        root: SWITCH_ROOT,
      });
      return;
    case "chef":
      chefRest(ctx, t);
      return;
    case "footnote":
      footnote(ctx, l.text, 1);
      return;
  }
}

/** Boundary `id`'s persistent layers, at rest, as the frame on its bar line has them. */
export function drawRest(
  ctx: CanvasRenderingContext2D,
  id: BoundaryId,
  d: ReelData | null,
  t: number,
): void {
  for (const l of RESTS[id]) drawLayer(ctx, l, d, t);
}

/** Whether a bar line keeps anything on the stage. */
export const keepsStage = (id: BoundaryId): boolean => RESTS[id].length > 0;

/** For a scene that draws a rest's own layer: the bar line's layers, by kind. */
export const restOf = (id: BoundaryId): readonly Layer[] => RESTS[id];

/**
 * The lit screen on a bar line (Scene.lit, which the vignette spares): the
 * terminal's window it keeps up, or the chef on morph|end; null for a
 * stage with neither. Both scenes return it on their side of the line.
 */
export function restLit(id: BoundaryId): LitRect | null {
  for (const l of RESTS[id]) {
    if (l.kind === "pane") return termLit(rectOf(l.pane));
    if (l.kind === "chef") return chefLit(CHEF_ART.place);
  }
  return null;
}
