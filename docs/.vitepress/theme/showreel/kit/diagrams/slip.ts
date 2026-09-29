// The slip (packslip, ART.md §12.6): an excerpt of hk's real packslip, the
// release statement mise saved when it installed each version (C8's
// off-camera files), on a file card tucked under the shortened terminal.
// Its rows are the statement's real keys, its values abridged: `version`
// (a tools chip), `resources` (the completion shells, then the skill
// names, each under its kind) and `identity` (the signing scheme, and the
// workflow that signed it with its middle abridged to `…`). A micro badge
// in the tab strip says so: "Excerpt of hk's packslip · real keys, values
// abridged".
//
// It comes down out from under the pane's bottom edge, tab strip first; when
// the card flips to the new version its version chip stamps (the old chip
// leaving up under the new one) and any value that differs cross-fades.
// When the drawn link takes its place it hands its skill and version chips
// on (slipChips gives the scene where they stand): the rest of it fades
// where it is. Every value comes from the statement files; a pure function
// of them and the section's beat.

import { PALETTE, PILLAR } from "../../bible";
import { type CaptureId, fileOf, type ReelData } from "../../captures";
import { rgba } from "../../color";
import { ring, roundedRect } from "../../fx";
import { clamp, lerp, progress } from "../../math";
import type { Rect } from "../motion";
import { run } from "../term";
import {
  COLOR,
  DUR,
  FILE_ART,
  GLOW,
  LAYOUT,
  MOTION,
  PILLAR_BAND,
  RADIUS,
  SLIP_ART,
  STAMP,
  STROKE,
  TYPE,
} from "../style";
import { fileCard, missingCard, mono, monoW, pill, span } from "./common";

/** The badge the slip carries. */
export const SLIP_BADGE =
  "Excerpt of hk's packslip · real keys, values abridged";

/** What the slip reads from a packslip statement's predicate. */
export interface Packslip {
  version: string;
  completions: string[];
  skills: string[];
  scheme: string;
  keyId: string;
}

interface Predicate {
  version?: string;
  resources?: { kind?: string; shell?: string; name?: string }[];
  identity?: { scheme?: string; key_id?: string };
}

/**
 * The packslip statement mise saved for `tool`@`version` when it installed
 * it (take `from`'s off-camera `packslip-<tool>-<version>.json`), or null.
 */
export function packslipOf(
  d: ReelData | null,
  tool: string,
  version: string,
  from: CaptureId = "C8",
): Packslip | null {
  const text = fileOf(d, from, `offcam/packslip-${tool}-${version}.json`);
  if (text === null) return null;
  let p: Predicate;
  try {
    p = (JSON.parse(text) as { predicate?: Predicate }).predicate ?? {};
  } catch {
    return null;
  }
  if (!p.version) return null;
  const of = (kind: string, key: "shell" | "name") =>
    (p.resources ?? [])
      .filter((r) => r.kind === kind && r[key])
      .map((r) => r[key] as string);
  return {
    version: p.version,
    completions: of("completion", "shell"),
    skills: of("skill", "name"),
    scheme: p.identity?.scheme ?? "",
    keyId: p.identity?.key_id ?? "",
  };
}

/**
 * A signing identity's key abridged to its repository and its workflow
 * file: `https://github.com/jdx/hk/.github/workflows/release.yml@refs/…`
 * reads `jdx/hk/…/release.yml`.
 */
export function abridgeKey(keyId: string): string {
  const path = keyId.replace(/^[a-z]+:\/\/[^/]+\//, "").split("@")[0];
  const segs = path.split("/").filter(Boolean);
  return segs.length > 3
    ? `${segs[0]}/${segs[1]}/…/${segs[segs.length - 1]}`
    : segs.join("/");
}

/** A row of the slip: a real key (or none, continuing the row above), the kind it lists, and its values. */
export interface SlipRow {
  key: string;
  kind?: string;
  values: string[];
  /** The version: a tools chip, and the one that stamps. */
  version?: boolean;
}

/** The slip's rows for a statement. */
export function slipRows(p: Packslip): SlipRow[] {
  const rows: SlipRow[] = [
    { key: "version", values: [p.version], version: true },
  ];
  if (p.completions.length)
    rows.push({ key: "resources", kind: "completion", values: p.completions });
  if (p.skills.length)
    rows.push({
      key: p.completions.length ? "" : "resources",
      kind: "skill",
      values: p.skills,
    });
  const id = [p.scheme, abridgeKey(p.keyId)].filter(Boolean);
  if (id.length) rows.push({ key: "identity", values: id });
  return rows;
}

/** What names a row for `hl` and `hand`: the kind its values list, or its key (`version`, `identity`). */
const rowId = (row: SlipRow): string => row.kind ?? row.key;

export interface SlipOptions {
  d: ReelData | null;
  /** The tool whose statements the slip reads (default hk). */
  tool?: string;
  /** The version before the flip, and after it (ReelData.versions.tools). */
  before: string;
  after?: string;
  /** The beat the card flips to `after`: the slip re-stamps. */
  flip?: number;
  /** Local beats. */
  b: number;
  /** It comes down out from under the pane, tab strip first (arrive 1/2). */
  enter: number;
  /** It falls away (leave 3/8). */
  leave?: number;
  /**
   * From this beat it hands the chips of the rows `handed` names on (the
   * drawn link's: the skills and the version): those are not drawn, for
   * the scene to fly them from where slipChips has them, and everything
   * else fades where it stands over DUR.exit (glide).
   */
  hand?: number;
  /** The rows handed on, by kind or key (default the skills and the version). */
  handed?: readonly string[];
  /** The pane's bottom edge it comes out from under (PANES.slip's). */
  under?: number;
  alpha?: number;
  rect?: Rect;
  /**
   * Rows pointed at, by the kind their values list ("completion",
   * "skill") or their key ("identity"): 0 to 1, a paper outline round each
   * of the row's chips in turn (a caption naming them); or one value, as
   * "kind:value" ("completion:zsh", the shell a Tab completes in).
   */
  hl?: Readonly<Record<string, number>>;
}

/** The slip's type (TYPE.slip): at the reading floor for mono, on chips this tall. */
const SIZE = TYPE.slip.size;
const CHIP_H = TYPE.slip.chipH;
const PITCH = TYPE.slip.lineH;
/** The resources' values start past their kind, this far in. */
const KIND_W = SLIP_ART.kindW;
/** A value's text inset either side on its chip. */
const PAD = 12;
/** A handed or flipped value's chip width. */
const chipW = (v: string): number => monoW(v, SIZE) + 2 * PAD;
/** How far the stamp's ring spreads past the version chip's edge, px: clear of the labels round it. */
const RING_SPREAD = 22;

/** A value's chip on the slip at rest: its row, its text, its pill's rect and its text's baseline. */
export interface SlipChip {
  /** The row's kind or key (`version`, `completion`, `skill`, `identity`). */
  id: string;
  value: string;
  rect: Rect;
  /** The text's left edge and baseline. */
  x: number;
  base: number;
  version: boolean;
}

/** Where the slip's rows and chips stand in `r`, as `rows` lays them out. */
function layout(
  r: Rect,
  body: Rect,
  rows: readonly SlipRow[],
): { row: SlipRow; base: number; chips: SlipChip[] }[] {
  const top = body.y + (body.h - rows.length * PITCH) / 2;
  return rows.map((row, i) => {
    const base = top + i * PITCH + PITCH / 2 + 0.36 * SIZE;
    let vx = r.x + SLIP_ART.valueX + (row.kind ? KIND_W : 0);
    const chips = row.values.map((v) => {
      const w = chipW(v);
      const chip: SlipChip = {
        id: rowId(row),
        value: v,
        rect: { x: vx, y: base - 0.36 * SIZE - CHIP_H / 2, w, h: CHIP_H },
        x: vx + PAD,
        base,
        version: !!row.version,
      };
      vx += w + SLIP_ART.chipGap;
      return chip;
    });
    return { row, base, chips };
  });
}

/** The slip's rect's body under its tab strip (fileCard's). */
const bodyOf = (r: Rect): Rect => ({
  x: r.x,
  y: r.y + FILE_ART.tabH,
  w: r.w,
  h: r.h - FILE_ART.tabH,
});

/**
 * The chips the slip stands at rest on beat `b` (the flipped statement's
 * once the card has flipped), where it hands them on: for the scene to fly
 * them. Empty when a statement is missing.
 */
export function slipChips(o: SlipOptions): SlipChip[] {
  const R = o.rect ?? LAYOUT.slip;
  const tool = o.tool ?? "hk";
  const flipped =
    o.flip !== undefined && o.after !== undefined && o.b >= o.flip;
  const p = packslipOf(o.d, tool, flipped ? o.after! : o.before);
  if (!p) return [];
  return layout(R, bodyOf(R), slipRows(p)).flatMap((l) => l.chips);
}

/** The slip on beat `b`. Returns its rect as drawn, or null when it is not up. */
export function slip(
  ctx: CanvasRenderingContext2D,
  o: SlipOptions,
): Rect | null {
  const R = o.rect ?? LAYOUT.slip;
  const b = o.b;
  const alpha = o.alpha ?? 1;
  const tool = o.tool ?? "hk";
  const inK = span(b, o.enter, DUR.enter, "arrive");
  const outK = o.leave === undefined ? 0 : span(b, o.leave, DUR.exit, "leave");
  const handK = o.hand === undefined ? 0 : span(b, o.hand, DUR.exit, "glide");
  if (alpha <= 0 || inK <= 0 || outK >= 1 || handK >= 1) return null;
  const handed = o.hand !== undefined && b >= o.hand;
  const gives = new Set(o.handed ?? ["skill", "version"]);
  const under = o.under ?? LAYOUT.slipPane.y + LAYOUT.slipPane.h;
  const flipped = o.flip !== undefined && o.after !== undefined && b >= o.flip;
  const before = packslipOf(o.d, tool, o.before);
  const after = o.after === undefined ? null : packslipOf(o.d, tool, o.after);
  ctx.save();
  ctx.globalAlpha *= alpha * (1 - outK);
  // Out from under the pane, tab strip first: revealed down from the
  // pane's bottom edge as it comes down its last 24 px into place.
  ctx.beginPath();
  ctx.rect(R.x - 80, under, R.w + 160, (R.y + R.h + 60 - under) * inK);
  ctx.clip();
  const dy = -MOTION.riseIn * (1 - inK) + MOTION.fallOut * outK;
  const r = { ...R, y: R.y + dy };
  if (!before || (o.after !== undefined && !after)) {
    missingCard(ctx, r, "C8 packslip");
    ctx.restore();
    return r;
  }
  // Handing on: the frame and every value it keeps fade where they stand.
  ctx.save();
  ctx.globalAlpha *= 1 - handK;
  const body = fileCard(ctx, r, "packslip", { tag: SLIP_BADGE });
  ctx.restore();
  const fromRows = slipRows(before);
  const toRows = after ? slipRows(after) : fromRows;
  // Values that differ cross-fade over a quarter beat from the flip.
  const x = o.flip === undefined ? 1 : span(b, o.flip, DUR.tick, "arrive");
  const cross = flipped ? x : 0;
  const rows = flipped ? toRows : fromRows;
  for (const { row, base, chips } of layout(r, body, rows)) {
    const id = rowId(row);
    const kept = !(handed && gives.has(id));
    ctx.save();
    ctx.globalAlpha *= 1 - handK;
    if (row.key)
      mono(ctx, row.key, r.x + SLIP_ART.keyX, base, SIZE, {
        color: PALETTE.text3,
      });
    if (row.kind)
      mono(ctx, row.kind, r.x + SLIP_ART.valueX, base, SIZE, {
        color: PALETTE.text3,
      });
    ctx.restore();
    const old = fromRows.find((f) => f.key === row.key && f.kind === row.kind);
    const hl = clamp(o.hl?.[id] ?? 0);
    chips.forEach((c, j) => {
      const v = c.value;
      const was = old?.values[j];
      const changed = flipped && was !== undefined && was !== v;
      if (kept) {
        ctx.save();
        ctx.globalAlpha *= 1 - handK;
        if (row.version && flipped && o.flip !== undefined)
          stampChip(ctx, R, was ?? null, c, b, o.flip);
        else if (changed && was !== undefined) {
          valuePill(ctx, row, was, c.rect.x, base, 1 - cross);
          valuePill(ctx, row, v, c.rect.x, base, cross);
        } else valuePill(ctx, row, v, c.rect.x, base, 1);
        ctx.restore();
      }
      // Pointed at: a paper outline round each chip in turn.
      const one = clamp(o.hl?.[`${id}:${v}`] ?? 0);
      const k = Math.max(one, clamp(hl * chips.length - j * 0.5, 0, 1));
      if (k > 0 && kept) {
        const cr = c.rect;
        ctx.save();
        ctx.globalAlpha *= k * (1 - handK);
        roundedRect(
          ctx,
          cr.x - 1,
          cr.y - 1,
          cr.w + 2,
          cr.h + 2,
          RADIUS.seat + 1,
        );
        ctx.lineWidth = STROKE.seat;
        ctx.strokeStyle = COLOR.mark;
        ctx.stroke();
        ctx.restore();
      }
    });
  }
  ctx.restore();
  return r;
}

/** A value's pill on the slip: a tools chip for the version (PILLAR_BAND, pink), elevated for the rest. */
function valuePill(
  ctx: CanvasRenderingContext2D,
  row: SlipRow,
  text: string,
  x: number,
  base: number,
  alpha: number,
  flash = 0,
): Rect {
  if (!row.version || flash <= 0)
    return pill(
      ctx,
      [run(text, row.version ? PILLAR.tools : PALETTE.text1)],
      x,
      base,
      SIZE,
      {
        fill: row.version ? PILLAR_BAND.tools : PALETTE.elevated,
        h: CHIP_H,
        radius: RADIUS.seat,
        pad: PAD,
        alpha,
      },
    );
  // Stamping: the glint's flash in the fill, under the text.
  const r = {
    x,
    y: base - 0.36 * SIZE - CHIP_H / 2,
    w: chipW(text),
    h: CHIP_H,
  };
  ctx.save();
  ctx.globalAlpha *= alpha;
  roundedRect(ctx, r.x, r.y, r.w, r.h, RADIUS.seat);
  ctx.fillStyle = PILLAR_BAND.tools;
  ctx.fill();
  ctx.fillStyle = rgba(COLOR.glint, flash);
  ctx.fill();
  mono(ctx, [run(text, PILLAR.tools)], x + PAD, base, SIZE);
  ctx.restore();
  return r;
}

/**
 * The version chip stamping on beat `at` (ART.md §12.6, §9): the old
 * version's chip leaves up 12 px under it, fading over 1/4 beat; the new one
 * comes down from 1.3× over DUR.stamp (stamp), opaque from its first frame
 * (never a blank pill), with the glint's flash in its fill (under its text)
 * decaying over DUR.flick from the landing, and one ring from its own edge,
 * RING_SPREAD px out and clipped to the slip, so it crosses no label.
 */
function stampChip(
  ctx: CanvasRenderingContext2D,
  slipRect: Rect,
  was: string | null,
  c: SlipChip,
  b: number,
  at: number,
): void {
  const row: SlipRow = { key: "version", values: [c.value], version: true };
  const gone = span(b, at, DUR.tick, "leave");
  if (was !== null && was !== c.value && gone < 1) {
    ctx.save();
    ctx.translate(0, -12 * gone);
    valuePill(
      ctx,
      row,
      was,
      c.rect.x,
      c.base,
      1 - span(b, at, DUR.tick, "glide"),
    );
    ctx.restore();
  }
  const k = span(b, at, DUR.stamp, "stamp");
  const s = lerp(MOTION.stampFrom, 1, k);
  const cx = c.rect.x + c.rect.w / 2;
  const cy = c.rect.y + c.rect.h / 2;
  const flash =
    b >= at + DUR.stamp
      ? STAMP.flash *
        (1 - progress(at + DUR.stamp, at + DUR.stamp + DUR.flick, b))
      : 0;
  ctx.save();
  ctx.translate(cx, cy);
  ctx.scale(s, s);
  ctx.translate(-cx, -cy);
  // Opaque from its first frame: it covers the old chip leaving under it.
  valuePill(ctx, row, c.value, c.rect.x, c.base, 1, flash);
  ctx.restore();
  const rp = progress(at + DUR.stamp, at + DUR.stamp + DUR.half, b);
  if (rp > 0 && rp < 1) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(slipRect.x, slipRect.y, slipRect.w, slipRect.h);
    ctx.clip();
    const edge = c.rect.w / 2 + 4;
    ring(
      ctx,
      cx,
      cy,
      edge + RING_SPREAD,
      rp,
      COLOR.mark,
      GLOW.ring.width,
      edge,
    );
    ctx.restore();
  }
}
