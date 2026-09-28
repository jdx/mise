// The CI panel (lock b3–b4, ART.md §12.4): the fixture's CI workflow as a
// file card and nothing more: no hosting chrome, no log, no status icons.
// Its YAML is set verbatim in SYNTAX colours at mono 28; the rows that
// matter light as seat marks and hold (`uses: jdx/mise-action@v4` and
// `install_args: --locked` as the mise.toml and mise.lock chips dock beside
// them, `run: mise run ci` 2.5 beats later: the scene's cues). It is as
// wide as its lines and those chips need (ciRect), centred, and pushes in
// slowly (1.00 → 1.04 over 6 beats, glide).
// Returns where each row stands, after the push, so the ledger's chip can
// dock by its row. A pure function of the file and the section's beat.

import type { Pt, Rect } from "../motion";
import { DUR, FILE_ART, LAYOUT, MOTION } from "../style";
import {
  fileCard,
  missingCard,
  mono,
  monoBaseline,
  monoW,
  seatMark,
  span,
  yamlRuns,
} from "./common";

/** The workflow's rows: mono 28 on a 40 px pitch, tightened to fit a longer file. */
const SIZE = 28;
const PITCH = 40;
const PUSH_BEATS = 6;
/** A lit row's band reaches this far past its text; a docked chip sits this far past that. */
const BAND_PAST = 18;
const DOCK_GAP = 16;

/** A row of the panel, in stage px after the push-in: its cell, baseline, and where its text ends. */
export interface CiRow {
  text: string;
  x: number;
  y: number;
  top: number;
  h: number;
  /** The x of its first non-space character, and just past its last. */
  start: number;
  end: number;
  /** The panel's push-in scale on this frame. */
  scale: number;
}

export interface CiPanelOptions {
  /** The workflow file (the fixture's .github/workflows/ci.yml); null draws the missing card. */
  yml: string | null;
  /** Local beats. */
  b: number;
  /** It rises in (24 px and a fade, arrive 1/2). */
  enter: number;
  /** The push-in starts. */
  push?: number;
  /** Its length, beats (default PUSH_BEATS). */
  pushFor?: number;
  /** Rows that light, by pattern, and from when; each holds once lit. */
  lights?: readonly { match: RegExp; at: number }[];
  /** Its tab's name (default the workflow's path in the repository). */
  title?: string;
  alpha?: number;
  rect?: Rect;
  /** The take or fixture named when the file is missing. */
  from?: string;
}

/**
 * The panel sized to its workflow (ART.md §12.4): as wide as its longest
 * line needs, or as a chip docked beside one of `docks`' rows needs (the
 * chip `w` px wide, past the row's lit band), centred where `r` is
 * (LAYOUT.ci), as tall as it is. A wide panel whose lines fill only its
 * left half leaves the other half an empty field.
 */
export function ciRect(
  yml: string,
  docks: readonly { match: RegExp; w: number }[] = [],
  r: Rect = LAYOUT.ci,
): Rect {
  let need = 0;
  for (const row of ciRows(yml, r)) {
    need = Math.max(need, row.end - r.x);
    for (const d of docks)
      if (d.match.test(row.text))
        need = Math.max(need, row.end - r.x + BAND_PAST + DOCK_GAP + d.w);
  }
  const w = Math.min(r.w, Math.ceil(need + FILE_ART.inset));
  return { x: Math.round(r.x + (r.w - w) / 2), y: r.y, w, h: r.h };
}

/** The panel's rows at rest (no push), for laying out before drawing. */
export function ciRows(yml: string, r: Rect = LAYOUT.ci): CiRow[] {
  const lines = yml.replace(/\s+$/, "").split("\n");
  const tabH = FILE_ART.tabH;
  const room = r.h - tabH - 2 * FILE_ART.top;
  const pitch = Math.min(PITCH, Math.floor(room / Math.max(1, lines.length)));
  const x = r.x + FILE_ART.inset;
  return lines.map((text, i) => {
    const top = r.y + tabH + FILE_ART.top + i * pitch;
    return {
      text,
      x,
      y: monoBaseline(top, pitch, SIZE),
      top,
      h: pitch,
      start: x + monoW(/^\s*/.exec(text)?.[0] ?? "", SIZE),
      end: x + monoW(text.trimEnd(), SIZE),
      scale: 1,
    };
  });
}

/** The CI panel on beat `b`. Returns its rows in stage px, pushed in. */
export function ciPanel(
  ctx: CanvasRenderingContext2D,
  o: CiPanelOptions,
): CiRow[] {
  const r = o.rect ?? LAYOUT.ci;
  const b = o.b;
  const alpha = o.alpha ?? 1;
  const k = span(b, o.enter, DUR.enter, "arrive");
  const push =
    o.push === undefined
      ? 0
      : span(b, o.push, o.pushFor ?? PUSH_BEATS, "glide");
  const s = 1 + MOTION.pushIn * push;
  const cx = r.x + r.w / 2;
  const cy = r.y + r.h / 2;
  const dy = MOTION.riseIn * (1 - k);
  // Stage px of a point on the panel at rest.
  const X = (x: number) => cx + (x - cx) * s;
  const Y = (y: number) => cy + (y - cy) * s + dy;
  const rows = o.yml === null ? [] : ciRows(o.yml, r);
  const staged = rows.map((row) => ({
    text: row.text,
    x: X(row.x),
    y: Y(row.y),
    top: Y(row.top),
    h: row.h * s,
    start: X(row.start),
    end: X(row.end),
    scale: s,
  }));
  if (alpha <= 0 || k <= 0) return staged;
  ctx.save();
  ctx.globalAlpha *= alpha * k;
  ctx.translate(0, dy);
  ctx.translate(cx, cy);
  ctx.scale(s, s);
  ctx.translate(-cx, -cy);
  if (o.yml === null) {
    missingCard(ctx, r, o.from ?? "fixtures");
    ctx.restore();
    return staged;
  }
  fileCard(ctx, r, o.title ?? ".github/workflows/ci.yml");
  for (const row of rows) {
    const lit = Math.max(
      0,
      ...(o.lights ?? [])
        .filter((l) => l.match.test(row.text))
        .map((l) => span(b, l.at, DUR.seat, "arrive")),
    );
    // The band hugs the lit line's text (the panel is wide and its lines
    // short, and indented), leaving room beside it for the lock chip.
    const x0 = Math.max(r.x + 14, row.start - BAND_PAST);
    seatMark(
      ctx,
      { x: x0, y: row.top + 2, w: row.end + BAND_PAST - x0, h: row.h - 4 },
      lit,
    );
    mono(ctx, yamlRuns(row.text), row.x, row.y, SIZE);
  }
  ctx.restore();
  return staged;
}

/**
 * Where a chip docks beside the first row matching `match` (its left edge,
 * at the row's mid-height), in the same stage px as the rows: pass the rows
 * `ciPanel` returned on this frame, so the dock follows the push-in.
 */
export function ciDock(rows: readonly CiRow[], match: RegExp): Pt | null {
  const row = rows.find((r) => match.test(r.text));
  if (!row) return null;
  return {
    x: row.end + (BAND_PAST + DOCK_GAP) * row.scale,
    y: row.top + row.h / 2,
  };
}
