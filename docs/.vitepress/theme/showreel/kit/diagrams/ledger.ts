// The ledger (lock, ART.md §12.3): mise.lock as the take wrote it, drawn as
// a ruled ledger on a file card. Only one tool's entry is open (`node`): its
// version and specifiers as the file writes them, then one row per platform
// (the platform's name, and its checksum clipped at the card's edge), and a
// pill for the platforms past the fourth. Every other tool folds to its
// header and an annotation: its platform count, and a signer or an aube
// sidecar where the lock records one; a micro "provenance" badge sits in
// the row of each entry whose lock records provenance.
//
// Over it, the request: a file card holding the project's `node = …` line,
// and a thread from that line, through the gutter, to the version the lock
// records. Then the ledger shrinks into a `mise.lock` file chip, and the
// request into a `mise.toml` one, each flying an arc to dock beside the CI
// panel row that reads it (ci-panel.ts): with a seat mark on that row and
// no tick, since no CI run was recorded (ART.md §12.3). Every value is read
// from the take's files; a pure function of them and the section's beat.

import { PALETTE, PILLAR } from "../../bible";
import { mix } from "../../color";
import { roundedRect } from "../../fx";
import { lerp, progress } from "../../math";
import { drawText, font, MONO } from "../../type";
import { arc, curveAt, exit, type Pt, type Rect } from "../motion";
import {
  card,
  type CardRows,
  fileCard as partsFileCard,
  fileFrame,
  gridRuns,
  missing,
  tomlRuns as partsToml,
} from "../parts";
import { run } from "../term";
import {
  CARD_ART,
  CHIP_ART,
  COLOR,
  DUR,
  EASE,
  FILE_ART,
  LAYOUT,
  LEDGER_ART,
  MOTION,
  RADIUS,
  SHADOW,
  STROKE,
  SYNTAX,
  THREAD,
  TYPE,
} from "../style";
import {
  annotation,
  badge,
  fileCard,
  missingCard,
  mono,
  monoBaseline,
  monoW,
  raised,
  scaled,
  seatMark,
  span,
  staggerOf,
  thread,
  tomlRuns,
} from "./common";

/** One `[[tools.<name>]]` entry of a lockfile. */
export interface LockEntry {
  name: string;
  /** Its header line, as the file writes it. */
  head: string;
  /** Its `version = …` and `specifiers = …` lines, as the file writes them. */
  version: string | null;
  specifiers: string | null;
  /** Its platforms, in the file's order, with each one's checksum value. */
  platforms: { name: string; checksum: string | null }[];
  signer: boolean;
  aube: boolean;
  provenance: boolean;
}

/** A lockfile's tool entries, in the file's order. */
export function lockEntries(lock: string): LockEntry[] {
  const out: LockEntry[] = [];
  let platform: LockEntry["platforms"][number] | null = null;
  for (const raw of lock.split("\n")) {
    const line = raw.trim();
    const head = /^\[\[tools\.(.+)\]\]$/.exec(line);
    if (head) {
      out.push({
        name: head[1].replace(/^"|"$/g, ""),
        head: line,
        version: null,
        specifiers: null,
        platforms: [],
        signer: false,
        aube: false,
        provenance: false,
      });
      platform = null;
      continue;
    }
    const e = out.at(-1);
    if (!e) continue;
    const plat = /^\[tools\..+\."platforms\.(.+)"\]$/.exec(line);
    if (plat) {
      platform = { name: plat[1], checksum: null };
      e.platforms.push(platform);
      continue;
    }
    const kv = /^([\w-]+)\s*=\s*(.*)$/.exec(line);
    if (!kv) continue;
    const [, key, value] = kv;
    if (platform) {
      if (key === "checksum") platform.checksum = value.replace(/^"|"$/g, "");
      if (key === "signer") e.signer = true;
      if (key === "provenance") e.provenance = true;
    } else {
      if (key === "version") e.version = line;
      if (key === "specifiers") e.specifiers = line;
      if (key === "aube") e.aube = true;
      if (key === "provenance") e.provenance = true;
    }
  }
  return out;
}

/** What a folded entry's annotation says: its platforms, a signer, an aube sidecar. */
export function foldNote(e: LockEntry): string {
  const n = e.platforms.length;
  return [
    n ? `${n} platform${n === 1 ? "" : "s"}` : "",
    e.signer ? "signer" : "",
    e.aube ? "aube sidecar" : "",
  ]
    .filter(Boolean)
    .join(" · ");
}

/** A row of the ledger. */
export type LedgerRow =
  | {
      kind: "head";
      entry: string;
      text: string;
      /** The annotation it carries while folded ("" for none). */
      note: string;
      provenance: boolean;
      /** The entry that opens. */
      open: boolean;
    }
  | { kind: "kv"; entry: string; text: string }
  | { kind: "sum"; entry: string; platform: string; checksum: string }
  | { kind: "more"; entry: string; text: string };

/**
 * The ledger's rows: every entry's header, and under `open`'s its version,
 * its specifiers, a row per platform up to `sums`, and a pill for the rest.
 */
export function ledgerRows(
  entries: readonly LockEntry[],
  open: string,
  sums: number = LEDGER_ART.sums,
): LedgerRow[] {
  const rows: LedgerRow[] = [];
  for (const e of entries) {
    const isOpen = e.name === open;
    rows.push({
      kind: "head",
      entry: e.name,
      text: e.head,
      note: foldNote(e),
      provenance: e.provenance,
      open: isOpen,
    });
    if (!isOpen) continue;
    for (const text of [e.version, e.specifiers])
      if (text) rows.push({ kind: "kv", entry: e.name, text });
    for (const p of e.platforms.slice(0, sums))
      if (p.checksum)
        rows.push({
          kind: "sum",
          entry: e.name,
          platform: p.name,
          checksum: p.checksum,
        });
    const more = e.platforms.length - Math.min(sums, e.platforms.length);
    if (more > 0)
      rows.push({
        kind: "more",
        entry: e.name,
        text: `${more} more platform${more === 1 ? "" : "s"}`,
      });
  }
  return rows;
}

export interface LedgerOptions {
  /** mise.lock as the take left it; null draws the missing card. */
  lock: string | null;
  /** The project's mise.toml, whose `<tool> = …` line the request card holds. */
  toml: string | null;
  /** The entry that opens (default `node`). */
  tool?: string;
  /** Local beats. */
  b: number;
  /** The ledger slides in from the right, its headers staggered (the request with it, unless it morphs). */
  enter: number;
  /** The tool's entry unfolds its version and specifiers. */
  unfold?: number;
  /** Its platform rows open, each checksum written on left to right (default with `unfold`). */
  sums?: number;
  /** The provenance badges pop in (default with the ledger's entrance). */
  badges?: number;
  /** The request's row takes its seat mark (default as it arrives). */
  seat?: number;
  /** The thread from the request to the version row draws on; the version row takes a seat mark as it arrives. */
  thread?: number;
  /**
   * The thread lets go, fading over DUR.threadFade (glide) from here, its
   * ends where they stood (default: faded by the first chip's
   * anticipation, so no chip ever leaves a thread hanging).
   */
  unthread?: number;
  /** The ledger shrinks into its `mise.lock` chip and flies to dock at `dock` (the chip's left edge, mid-height). */
  chip?: ChipFlight;
  /**
   * The request shrinks into its own `mise.toml` chip the same way and
   * flies to dock at `dock`. Without it the request falls and fades as
   * the ledger's shrink starts.
   */
  requestChip?: ChipFlight;
  /** The ledger's card as tall as its rows (they unfold, it grows), at most `rect`'s height. */
  fit?: boolean;
  /**
   * The request is the station card closing up: from `at` the card (as the
   * bar line holds it, drawn at rest before then) folds away every line but
   * the tool's and shrinks onto `via` if given, else onto the request's
   * rect (DUR.fold, move), keeping its title; from `via` it then travels
   * to the request's rect (DUR.move, move).
   */
  morph?: { at: number; card: MorphCard; via?: Rect };
  /** The request's tab (default `mise.toml`; the morph keeps the card's). */
  requestTitle?: string;
  /** A micro badge in the ledger's tab strip (what the card condenses). */
  tag?: string;
  /** The take named when a file is missing (default C17). */
  from?: string;
  alpha?: number;
  /** Where the ledger and the request stand (LAYOUT.ledger, LAYOUT.request). */
  rect?: Rect;
  request?: Rect;
}

/** A chip's flight: when its card starts to become it, and where it docks. */
export interface ChipFlight {
  at: number;
  /**
   * The beat its flight starts, the chip holding where it shrank until
   * then (default: as the shrink ends).
   */
  fly?: number;
  /** The chip's left edge, at its row's mid-height. */
  dock: Pt;
  /**
   * A cubic's two handles from the chip's start to its end (top-left
   * corners, the end a little above the dock), in place of the reel's
   * usual arc: a flight routed round a panel's lines, not over them.
   */
  handles?: (start: Pt, end: Pt) => readonly [Pt, Pt];
}

/** The station card a request morphs from, as the bar line draws it (kit/parts.ts card). */
export interface MorphCard {
  rect: Rect;
  title: string;
  text: string | null;
  from: string;
  open: readonly string[];
}

/** Where the ledger's parts are on a frame. */
export interface LedgerLayout {
  /** The request's tool row, and the ledger's version row (stage px, at rest), or null. */
  requestRow: Rect | null;
  versionRow: Rect | null;
  /** The mise.lock chip, once the ledger has become it. */
  chip: Rect | null;
  /** The mise.toml chip, once the request has become it. */
  requestChip: Rect | null;
}

const LOCK_NAME = "mise.lock";
const REQUEST_NAME = "mise.toml";

/** The chip's type size, its padding, and the gap between its glyph and its name. */
const CHIP = TYPE.chip.value;
const CHIP_PAD = 16;
const GLYPH_GAP = 12;
/** The anticipation before a card shrinks into its chip: a 2 % swell (ART.md §12.3). */
const SWELL = 0.02;
/** A chip arrives this far above its dock and drops the rest of the way. */
const DOCK_DROP = 10;

/**
 * A file chip's size (ART.md §9): a document glyph and the file's name in
 * mono 32. No room for a tick: nothing in the reel checked the files it
 * carries (ART.md §12.3).
 */
export function fileChipSize(name: string = LOCK_NAME): {
  w: number;
  h: number;
} {
  const g = CHIP_ART.docGlyph;
  return {
    w: CHIP_PAD + g.w + GLYPH_GAP + monoW(name, CHIP) + CHIP_PAD,
    h: Math.round(CHIP_ART.heightEm * CHIP),
  };
}

/** The mise.lock file chip's size. */
export const lockChipSize = (): { w: number; h: number } =>
  fileChipSize(LOCK_NAME);

/** The chip's phases from its `at`: anticipation, the shrink, the flight, the landing (ART.md §12.3). */
export const CHIP_TIMES = {
  anticipate: DUR.anticipate,
  shrink: DUR.move,
  fly: DUR.move,
  land: DUR.tick,
} as const;

/** The beat a chip touches down on its dock (the click), from its `at`. */
export const chipDocks = (at: number): number =>
  at + CHIP_TIMES.anticipate + CHIP_TIMES.shrink + CHIP_TIMES.fly;

/** Where a chip is in its phases on beat `b` (all 0 without one). */
interface ChipPhase {
  antic: number;
  /** The shrink, eased (move). */
  shrink: number;
  /** The flight, eased (travel). */
  fly: number;
  /** The drop onto the dock (drop), and the same landing unovershot (arrive). */
  land: number;
  settle: number;
  shrinkAt: number;
}

function chipPhase(b: number, at: number | undefined, fly?: number): ChipPhase {
  if (at === undefined)
    return {
      antic: 0,
      shrink: 0,
      fly: 0,
      land: 0,
      settle: 0,
      shrinkAt: Infinity,
    };
  const T = CHIP_TIMES;
  const shrinkAt = at + T.anticipate;
  const flightAt = Math.max(shrinkAt + T.shrink, fly ?? -Infinity);
  return {
    antic: progress(at, shrinkAt, b),
    shrink: span(b, shrinkAt, T.shrink, "move"),
    fly: span(b, flightAt, T.fly, "travel"),
    land: span(b, flightAt + T.fly, T.land, "drop"),
    settle: span(b, flightAt + T.fly, T.land, "arrive"),
    shrinkAt,
  };
}

const STILL = { alpha: 1, dy: 0 } as const;

/** The ledger on beat `b`. */
export function ledger(
  ctx: CanvasRenderingContext2D,
  o: LedgerOptions,
): LedgerLayout {
  const out: LedgerLayout = {
    requestRow: null,
    versionRow: null,
    chip: null,
    requestChip: null,
  };
  const alpha = o.alpha ?? 1;
  const b = o.b;
  const R = o.rect ?? LAYOUT.ledger;
  const Q = o.request ?? LAYOUT.request;
  const tool = o.tool ?? "node";
  const from = o.from ?? "C17";
  if (alpha <= 0 || (b < o.enter && !o.morph)) return out;
  ctx.save();
  ctx.globalAlpha *= alpha;
  const enterK = span(b, o.enter, DUR.enter, "arrive");
  const lockC = chipPhase(b, o.chip?.at, o.chip?.fly);
  const reqC = chipPhase(b, o.requestChip?.at, o.requestChip?.fly);
  // Without a chip of its own, the request falls and fades as the ledger's
  // shrink starts.
  const gone = o.requestChip ? STILL : exit(b, lockC.shrinkAt, DUR.exit);
  const firstChip = Math.min(
    o.chip?.at ?? Infinity,
    o.requestChip?.at ?? Infinity,
  );
  const unthread = o.unthread ?? firstChip - DUR.threadFade;
  const threadA = 1 - span(b, unthread, DUR.threadFade, "glide");

  // The ledger: sliding in, at rest, then shrinking into its chip.
  if (b >= o.enter) {
    if (o.lock === null) missingCard(ctx, R, from, enterK);
    else if (lockC.shrink < 1) {
      const rows = ledgerRows(lockEntries(o.lock), tool);
      const card = o.fit
        ? { ...R, h: Math.min(R.h, ledgerHeight(rowStates(rows, b, o))) }
        : R;
      // The version row takes a seat mark as the thread arrives, and keeps it.
      const versionSeat =
        o.thread === undefined
          ? 0
          : span(b, o.thread + DUR.thread, DUR.tick, "arrive");
      ctx.save();
      ctx.globalAlpha *= enterK;
      ctx.translate(MOTION.slideIn * (1 - enterK), 0);
      out.versionRow = intoChip(ctx, {
        card,
        name: LOCK_NAME,
        tab: fileCardTab(card),
        ph: lockC,
        draw: (blank) => drawLedger(ctx, card, rows, b, o, versionSeat, blank),
      });
      ctx.restore();
      if (lockC.shrink > 0) out.chip = chipAt(R, LOCK_NAME);
    }
  }

  // The request: the card closing up round its line (and travelling to
  // its place), or sliding in with the ledger; then shrinking into its chip.
  const m = o.morph;
  const closeK = m ? progress(m.at, m.at + DUR.fold, b) : 1;
  const travelK = m?.via ? span(b, m.at + DUR.fold, DUR.move, "move") : 1;
  const arrived = m
    ? m.at + DUR.fold + (m.via ? DUR.move : 0)
    : o.enter + DUR.enter;
  const title = m?.card.title ?? o.requestTitle ?? REQUEST_NAME;
  const line = o.toml === null ? undefined : toolLine(o.toml, tool);
  const restQ = m?.via ? lerpRect(m.via, Q, travelK) : Q;
  if (gone.alpha > 0 && reqC.shrink < 1) {
    ctx.save();
    ctx.globalAlpha *= gone.alpha;
    ctx.translate(0, gone.dy);
    if (!m) {
      ctx.globalAlpha *= enterK;
      ctx.translate(MOTION.slideIn * (1 - enterK), 0);
    }
    if (m && closeK < 1)
      out.requestRow = drawMorph(ctx, {
        rect: m.via ?? Q,
        card: m.card,
        k: closeK,
        line,
        tool,
        from,
      });
    else {
      const seat = span(b, o.seat ?? arrived, DUR.seat, "arrive");
      out.requestRow = intoChip(ctx, {
        card: restQ,
        name: REQUEST_NAME,
        tab: fileFrameTab(restQ, title, REQUEST_NAME),
        ph: reqC,
        draw: (blank) =>
          requestCard(ctx, restQ, {
            title,
            line,
            from,
            seat,
            blank,
            missing: o.toml === null,
          }),
      });
    }
    ctx.restore();
    if (reqC.shrink > 0) out.requestChip = chipAt(restQ, REQUEST_NAME);
  }

  // The thread from the ask to the record, through the gutter: out of the
  // request row's end nearer the gutter, into the ledger's margin.
  const threadK =
    o.thread === undefined ? 0 : span(b, o.thread, DUR.thread, "arrive");
  if (
    threadK > 0 &&
    threadA > 0 &&
    gone.alpha > 0 &&
    out.requestRow &&
    out.versionRow
  ) {
    const q = out.requestRow;
    const v = out.versionRow;
    const qy = q.y + q.h / 2 + gone.dy;
    const vy = v.y + v.h / 2;
    const left = q.x + q.w <= THREAD.gutterX;
    thread(
      ctx,
      [
        {
          x: left ? q.x + q.w - CARD_ART.anchorX : q.x + CARD_ART.anchorX,
          y: qy,
        },
        { x: THREAD.gutterX, y: qy },
        { x: THREAD.gutterX, y: vy },
        { x: R.x + LEDGER_ART.margin - 12, y: vy },
      ],
      threadK,
      enterK * gone.alpha * threadA,
    );
  }

  // The chips in flight, and docked.
  if (o.requestChip && reqC.shrink >= 1)
    out.requestChip = flyChip(
      ctx,
      chipAt(restQ, REQUEST_NAME),
      o.requestChip,
      reqC,
      REQUEST_NAME,
    );
  if (o.chip && lockC.shrink >= 1 && o.lock !== null)
    out.chip = flyChip(ctx, chipAt(R, LOCK_NAME), o.chip, lockC, LOCK_NAME);
  ctx.restore();
  return out;
}

/** Where a card's chip forms: over its tab, at its top-left. */
function chipAt(r: Rect, name: string): Rect {
  const s = fileChipSize(name);
  return { x: r.x, y: r.y, w: s.w, h: s.h };
}

const lerpRect = (a: Rect, b: Rect, k: number): Rect => ({
  x: lerp(a.x, b.x, k),
  y: lerp(a.y, b.y, k),
  w: lerp(a.w, b.w, k),
  h: lerp(a.h, b.h, k),
});

/** Where a card's tab sets its title, and the part of it before the chip's name. */
interface TabLabel {
  x: number;
  y: number;
  size: number;
  prefix: string;
  /** How the card sets it: kit/diagrams/common.ts fileCard (grid mono) or kit/parts.ts fileFrame (mono 500). */
  face: "grid" | "frame";
}

/** A title as kit/diagrams/common.ts fileCard sets it in its tab. */
const fileCardTab = (r: Rect): TabLabel => ({
  x: r.x + FILE_ART.tabPad,
  y: monoBaseline(r.y, FILE_ART.tabH, TYPE.file.title),
  size: TYPE.file.title,
  prefix: "",
  face: "grid",
});

/** A title as kit/parts.ts fileFrame sets it in its tab. */
const fileFrameTab = (r: Rect, title: string, name: string): TabLabel => {
  const ts = TYPE.file.title;
  return {
    x: r.x + FILE_ART.tabPad,
    y: r.y + Math.min(FILE_ART.tabH, r.h) / 2 + 0.36 * ts,
    size: ts,
    prefix: title.endsWith(name) ? title.slice(0, -name.length) : "",
    face: "frame",
  };
};

/** The mono 500 a file frame's tab sets its title in (kit/parts.ts fileFrame). */
function frameText(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  size: number,
  color: string,
): void {
  drawText(ctx, text, x, y, { font: font(size, 500, MONO), fill: color });
}

/** A title of `n` characters' width with nothing in it: the tab stays, its name is drawn apart. */
const blankOf = (title: string): string => " ".repeat(Array.from(title).length);

/**
 * A card becoming its file chip (ART.md §12.3), on its chip phase `ph`:
 * at rest until the anticipation, a 2 % swell (wind), then one scaled
 * layer, the card with its rows, shrinking toward the chip's corner and
 * fading as the chip's body forms under the card's own name, which glides
 * from the tab into the chip (the path before it leaving with the card),
 * so no frame shows an empty card or a blank tab. `draw` draws the card at
 * rest, its tab title blank when told; its return is passed back (the
 * card's rows, at rest).
 */
function intoChip<T>(
  ctx: CanvasRenderingContext2D,
  o: {
    card: Rect;
    name: string;
    tab: TabLabel;
    ph: ChipPhase;
    draw: (blank: boolean) => T;
  },
): T {
  const { card, ph } = o;
  const e = ph.shrink;
  const swell =
    1 + SWELL * (e > 0 ? 1 - e : Math.sin((Math.PI / 2) * ph.antic));
  const chip = chipAt(card, o.name);
  let got: T | undefined;
  scaled(ctx, card.x + card.w / 2, card.y + card.h / 2, swell, () => {
    if (e <= 0) {
      got = o.draw(false);
      return;
    }
    const tab = o.tab;
    // The card and its rows: one layer, shrinking toward the chip's corner.
    // It shrinks fast early (ease-out on the move) so it is small by the
    // time it has faded.
    const sEnd = Math.sqrt((chip.w / card.w) * (chip.h / card.h));
    const s = lerp(1, sEnd, 1 - (1 - e) ** 2);
    ctx.save();
    ctx.globalAlpha *= 1 - progress(0.1, 0.6, e);
    scaled(ctx, card.x, card.y, s, () => {
      got = o.draw(true);
      if (tab.prefix) {
        if (tab.face === "frame")
          frameText(ctx, tab.prefix, tab.x, tab.y, tab.size, PALETTE.text2);
        else
          mono(ctx, tab.prefix, tab.x, tab.y, tab.size, {
            color: PALETTE.text2,
          });
      }
    });
    ctx.restore();
    // The chip's body, forming round the name.
    drawChip(ctx, chip, {
      name: o.name,
      body: progress(0.3, 0.75, e),
      label: 0,
    });
    // The name, from the tab into the chip.
    const L = chipLabel(chip);
    const x = lerp(tab.x + monoW(tab.prefix, tab.size), L.x, e);
    const y = lerp(tab.y, L.y, e);
    const size = lerp(tab.size, CHIP, e);
    const color = mix(PALETTE.text2, PALETTE.text1, e);
    const grid = tab.face === "grid" ? 1 : progress(0.15, 0.7, e);
    if (grid < 1) {
      ctx.save();
      ctx.globalAlpha *= 1 - grid;
      frameText(ctx, o.name, x, y, size, color);
      ctx.restore();
    }
    ctx.save();
    ctx.globalAlpha *= grid;
    mono(ctx, o.name, x, y, size, { color });
    ctx.restore();
  });
  return got as T;
}

/** Where a chip sets its name: after its glyph, centred in its height. */
const chipLabel = (r: Rect): Pt => ({
  x: r.x + CHIP_PAD + CHIP_ART.docGlyph.w + GLYPH_GAP,
  y: monoBaseline(r.y, r.h, CHIP),
});

/**
 * A formed chip at `start` flying an arc (travel 3/4) to dock at `dock`
 * (its left edge, mid-height), arriving a little above it and dropping
 * onto it (drop 6 %): the float shadow in flight, the small one docked.
 */
function flyChip(
  ctx: CanvasRenderingContext2D,
  start: Rect,
  f: ChipFlight,
  ph: ChipPhase,
  name: string,
): Rect {
  const to = { x: f.dock.x, y: f.dock.y - start.h / 2 };
  const end = { x: to.x, y: to.y - DOCK_DROP };
  let p: Pt = { x: start.x, y: start.y };
  if (ph.fly > 0) {
    const h = f.handles?.(start, end);
    p = h
      ? cubicAt(start, h[0], h[1], end, ph.fly)
      : curveAt(arc(start, end, MOTION.arcLift), ph.fly);
  }
  if (ph.fly >= 1) p = { x: to.x, y: to.y - DOCK_DROP * (1 - ph.land) };
  const r = { x: p.x, y: p.y, w: start.w, h: start.h };
  drawChip(ctx, r, {
    name,
    floatK: ph.fly > 0 && ph.fly < 1 ? 1 : ph.fly >= 1 ? 1 - ph.settle : 0,
  });
  return r;
}

/** A cubic Bézier at `u`. */
function cubicAt(a: Pt, b: Pt, c: Pt, d: Pt, u: number): Pt {
  const v = 1 - u;
  const w0 = v * v * v;
  const w1 = 3 * v * v * u;
  const w2 = 3 * v * u * u;
  const w3 = u * u * u;
  return {
    x: w0 * a.x + w1 * b.x + w2 * c.x + w3 * d.x,
    y: w0 * a.y + w1 * b.y + w2 * c.y + w3 * d.y,
  };
}

/** The request's tool line in a mise.toml. */
function toolLine(toml: string, tool: string): string | undefined {
  const re = new RegExp(
    `^"?${tool.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}"?\\s*=`,
  );
  return toml.split("\n").find((l) => re.test(l));
}

/** A card's rows as kit/parts.ts card lays them out, measured without drawing. */
function cardRowsOf(ctx: CanvasRenderingContext2D, c: MorphCard): CardRows {
  ctx.save();
  ctx.beginPath();
  ctx.rect(0, 0, 0, 0);
  ctx.clip();
  const rows = card(ctx, c.rect, {
    title: c.title,
    text: c.text,
    from: c.from,
    open: c.open,
  });
  ctx.restore();
  return rows;
}

/**
 * The request card at rest: mise.toml's `<tool> = …` line alone, on a file
 * card (ART.md §8), its seat mark at `seat`; `blank` leaves its tab's title
 * out (a chip's name is taking it). Returns the line's row (stage px), or
 * null.
 */
function requestCard(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  o: {
    title: string;
    line: string | undefined;
    from: string;
    seat: number;
    blank?: boolean;
    /** The project's mise.toml is missing: the labelled box instead. */
    missing?: boolean;
  },
): Rect | null {
  const lh = TYPE.file.lineH;
  const size = TYPE.file.size;
  if (o.missing) {
    missing(ctx, r, o.from);
    return null;
  }
  const rows = partsFileCard(ctx, r, {
    title: o.blank ? blankOf(o.title) : o.title,
    lines: o.line === undefined ? [] : [o.line],
    from: o.from,
    hl: o.line === undefined ? [] : [{ re: /^/, a: o.seat }],
  });
  const row = rows[0];
  if (!row) return null;
  const endY = r.y + FILE_ART.tabH + FILE_ART.top + 0.8 * size;
  return {
    x: r.x,
    y: row.top ?? endY - 0.8 * size - (lh - size) / 2,
    w: r.w,
    h: lh,
  };
}

/**
 * The station card closing up round the request's line (`k` 0 to 1 of
 * DUR.fold): at 0 the card exactly as the bar line draws it; then the frame
 * shrinks from the card's rect, tab and radius onto `rect`'s (move), every
 * other row fades as the card slides up round the line, and the line
 * glides into the request's first row. Returns the line's row (stage px),
 * or null.
 */
function drawMorph(
  ctx: CanvasRenderingContext2D,
  o: {
    rect: Rect;
    card: MorphCard;
    k: number;
    line: string | undefined;
    tool: string;
    from: string;
  },
): Rect | null {
  const r = o.rect;
  const C = o.card;
  const lh = TYPE.file.lineH;
  const size = TYPE.file.size;
  const top = (y: number) => y - 0.8 * size - (lh - size) / 2;
  if (o.k <= 0) {
    // The card at rest, as the bar line has it.
    const rows = card(ctx, C.rect, {
      title: C.title,
      text: C.text,
      from: C.from,
      open: C.open,
    });
    const row = rows.find((x) => x.kind === "body" && toolLine(x.text, o.tool));
    return row ? { x: C.rect.x, y: top(row.y), w: C.rect.w, h: lh } : null;
  }
  const e = EASE.move(o.k);
  const F = lerpRect(C.rect, r, e);
  if (C.text === null || o.line === undefined) {
    missing(ctx, F, o.from);
    return null;
  }
  // The request's row, at rest.
  const endY = r.y + FILE_ART.tabH + FILE_ART.top + 0.8 * size;
  const endX = r.x + FILE_ART.inset;
  const start = cardRowsOf(ctx, C).find(
    (x) => x.kind === "body" && x.text === o.line,
  );
  const tabH = lerp(CARD_ART.tabH, FILE_ART.tabH, e);
  const body = fileFrame(ctx, F, C.title, {
    tabH,
    titleSize: lerp(TYPE.card.title, TYPE.file.title, e),
    radius: lerp(RADIUS.card, RADIUS.file, e),
  });
  const sx = start?.x ?? endX;
  const sy = start?.y ?? endY;
  const dx = (endX - sx) * e;
  const dy = (endY - sy) * e;
  ctx.save();
  ctx.beginPath();
  ctx.rect(F.x, body.y, F.w, Math.max(0, body.h - 2));
  ctx.clip();
  // Every other row: the card's own, sliding with the line, fading into
  // the frame's surface (the card drawn whole, so its shadow stays under
  // its fill, then the surface laid back over it), inside the frame's
  // edges and outside the line's band.
  const others = 1 - EASE.settle(progress(0, 0.45, o.k));
  if (others > 0) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(F.x + 2, body.y, F.w - 4, body.h);
    ctx.rect(F.x + 2, top(sy + dy), F.w - 4, lh);
    ctx.clip("evenodd");
    ctx.save();
    ctx.translate(dx, dy);
    card(ctx, C.rect, {
      title: C.title,
      text: C.text,
      from: C.from,
      open: C.open,
    });
    ctx.restore();
    if (others < 1) {
      const g = ctx.createLinearGradient(0, F.y, 0, F.y + F.h);
      g.addColorStop(0, COLOR.surfaceTop);
      g.addColorStop(1, COLOR.surfaceBottom);
      ctx.globalAlpha *= 1 - others;
      ctx.fillStyle = g;
      ctx.fillRect(F.x + 2, body.y, F.w - 4, body.h);
    }
    ctx.restore();
  }
  gridRuns(ctx, partsToml(o.line), sx + dx, sy + dy, size);
  ctx.restore();
  return { x: F.x, y: top(sy + dy), w: F.w, h: lh };
}

/** A ledger row on a frame: its height (opening), alpha, drop and a checksum's write-on. */
interface RowState {
  row: LedgerRow;
  h: number;
  a: number;
  dy: number;
  reveal: number;
}

/**
 * Each row's state on beat `b`. The unfold comes in two steps: the entry's
 * version and specifiers, then its platform rows. Each first parts the
 * rows under it (move 1/4, ART.md §7's seat), and only then do its own
 * rows write in, staggered, so a row never fades in over the one it is
 * pushing down.
 */
function rowStates(
  rows: readonly LedgerRow[],
  b: number,
  o: LedgerOptions,
): RowState[] {
  const lh = TYPE.ledger.lineH;
  const tool = o.tool ?? "node";
  const openAt = (row: LedgerRow): number | undefined =>
    row.kind === "kv" ? o.unfold : (o.sums ?? o.unfold);
  const heads = rows.filter((x) => x.kind === "head");
  // Each step's rows, counted for their stagger.
  const group = (row: LedgerRow) =>
    rows.filter(
      (z) => z.kind !== "head" && z.entry === tool && openAt(z) === openAt(row),
    );
  let head = -1;
  return rows.map((row) => {
    if (row.kind === "head") head++;
    // Headers stagger in after the card (1/16 beat each, capped).
    const enter = span(
      b,
      o.enter + staggerOf(head, heads.length),
      DUR.tick,
      "arrive",
    );
    let a = enter;
    let dy = 8 * (1 - enter);
    let h = lh;
    let reveal = 1;
    if (row.kind !== "head") {
      const at = openAt(row);
      if (at !== undefined) {
        h = lh * span(b, at, DUR.seatGap, "move");
        const g = group(row);
        const inAt = at + DUR.seatGap + staggerOf(g.indexOf(row), g.length);
        const f = span(b, inAt, DUR.tick, "arrive");
        a *= f;
        dy += 8 * (1 - f);
        // A checksum writes on left to right after its platform's name.
        if (row.kind === "sum")
          reveal = span(b, inAt + DUR.flick, DUR.half, "settle");
      }
    }
    return { row, h, a, dy, reveal };
  });
}

/** The ledger card's height for these rows: its tab, its rows as tall as they stand, and its insets. */
const ledgerHeight = (states: readonly RowState[]): number =>
  FILE_ART.tabH +
  2 * FILE_ART.top +
  states.reduce((n, st) => n + (st.h > 0.01 ? st.h : 0), 0);

/**
 * The ledger card at rest at `r`: its frame (its tab's title left blank
 * when `blank`, a chip's name taking it), and its rows. Returns the open
 * entry's version row.
 */
function drawLedger(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  rows: readonly LedgerRow[],
  b: number,
  o: LedgerOptions,
  versionSeat: number,
  blank: boolean,
): Rect | null {
  const L = LEDGER_ART;
  const size = TYPE.ledger.size;
  const lh = TYPE.ledger.lineH;
  const body = fileCard(ctx, r, blank ? blankOf(LOCK_NAME) : LOCK_NAME, {
    shadow: SHADOW.raised,
    tag: o.tag,
  });
  // The open entry's header lights over the unfold's first 1/2.
  const kvK =
    o.unfold === undefined ? 1 : span(b, o.unfold, DUR.fold, "arrive");
  let version: Rect | null = null;
  ctx.save();
  roundedRect(ctx, r.x, body.y, r.w, body.h, 0);
  ctx.clip();
  // The margin rule.
  ctx.fillStyle = COLOR.ledgerMargin;
  ctx.fillRect(r.x + L.margin, body.y, 1, body.h);
  let y = body.y + FILE_ART.top;
  const x = r.x + L.rowX;
  for (const { row, h, a, dy, reveal } of rowStates(rows, b, o)) {
    if (h <= 0.01) continue;
    const top = y;
    y += h;
    ctx.save();
    ctx.beginPath();
    ctx.rect(r.x, top, r.w, h);
    ctx.clip();
    ctx.fillStyle = COLOR.ledgerRule;
    ctx.fillRect(r.x + 12, top + lh - 1, r.w - 24, 1);
    ctx.globalAlpha *= a;
    const base = monoBaseline(top, lh, size) + dy;
    const mid = top + lh / 2 + dy;
    const fade = [r.x + r.w - 64, r.x + r.w - 12] as const;
    if (row.kind === "head") {
      const lit = row.open ? kvK : 0;
      const color = mix(SYNTAX.header, PILLAR.tools, lit);
      const w = mono(ctx, [run(row.text, color, true)], x, base, size);
      // A folded entry's note; the opening entry's leaves as it unfolds.
      const noteA = row.open ? 1 - kvK : 1;
      if (row.note && noteA > 0)
        annotation(ctx, row.note, x + w + L.annotationGap, mid, noteA);
      if (row.provenance) {
        const bs = TYPE.badgeMicro.size - 2;
        badge(
          ctx,
          r.x + r.w - 16,
          mid - Math.round(bs * 1.6) / 2,
          "provenance",
          {
            size: bs,
            align: "right",
            k:
              o.badges === undefined
                ? 1
                : span(b, o.badges, DUR.badge, "arrive"),
          },
        );
      }
    } else if (row.kind === "kv") {
      const isVersion = /^version\s*=/.test(row.text);
      if (isVersion && versionSeat > 0)
        seatMark(
          ctx,
          {
            x: r.x + L.margin + 4,
            y: top + 2 + dy,
            w: r.w - L.margin - 16,
            h: lh - 4,
          },
          versionSeat,
        );
      mono(ctx, tomlRuns(row.text), x, base, size, { fade });
      if (isVersion) version = { x: r.x, y: top, w: r.w, h: lh };
    } else if (row.kind === "sum") {
      const plat = row.platform.padEnd(L.platformCols);
      mono(ctx, plat, x, base, size, { color: PALETTE.text3 });
      const sx = x + monoW(plat, size);
      if (reveal > 0) {
        ctx.save();
        if (reveal < 1) {
          ctx.beginPath();
          ctx.rect(sx, top, (r.x + r.w - sx) * reveal, h);
          ctx.clip();
        }
        mono(ctx, row.checksum, sx, base, TYPE.ledger.sum, {
          color: PALETTE.text3,
          fade,
        });
        ctx.restore();
      }
    } else {
      annotation(ctx, row.text, x, mid);
    }
    ctx.restore();
  }
  ctx.restore();
  return version;
}

/**
 * A file chip (ART.md §9): surface, a document glyph and the file's name in
 * mono 32, no tick. `body` fades the surface and glyph in (the card it
 * forms from shrinking away under it), `label` the name; in flight it casts
 * the `float` shadow (`floatK`), easing back to the `small` one as it lands.
 */
function drawChip(
  ctx: CanvasRenderingContext2D,
  r: Rect,
  o: { name: string; body?: number; label?: number; floatK?: number },
): void {
  const body = o.body ?? 1;
  const label = o.label ?? 1;
  const floatK = o.floatK ?? 0;
  if (body > 0) {
    ctx.save();
    ctx.globalAlpha *= body;
    raised(ctx, r, RADIUS.chip, {
      shadow: floatK > 0.5 ? SHADOW.float : SHADOW.small,
      shadowAlpha: floatK > 0.5 ? floatK : 1 - floatK,
    });
    const g = CHIP_ART.docGlyph;
    const gx = r.x + CHIP_PAD;
    const gy = r.y + (r.h - g.h) / 2;
    ctx.beginPath();
    ctx.moveTo(gx, gy);
    ctx.lineTo(gx + g.w - g.fold, gy);
    ctx.lineTo(gx + g.w, gy + g.fold);
    ctx.lineTo(gx + g.w, gy + g.h);
    ctx.lineTo(gx, gy + g.h);
    ctx.closePath();
    ctx.moveTo(gx + g.w - g.fold, gy);
    ctx.lineTo(gx + g.w - g.fold, gy + g.fold);
    ctx.lineTo(gx + g.w, gy + g.fold);
    ctx.lineWidth = STROKE.badgeMicro;
    ctx.lineJoin = "round";
    ctx.strokeStyle = PALETTE.text3;
    ctx.stroke();
    ctx.restore();
  }
  if (label > 0) {
    const L = chipLabel(r);
    ctx.save();
    ctx.globalAlpha *= label;
    mono(ctx, o.name, L.x, L.y, CHIP, { color: PALETTE.text1 });
    ctx.restore();
  }
}
