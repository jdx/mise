// The drawn link (packslip, ART.md §12.7): what `mise skills sync` did,
// drawn from its own `linked … -> …` rows. One row per linked skill, 64 px
// apart: on the left the project's path (`.claude/skills/<skill>`) as a
// chip; on the right the install directory it points into, abridged with
// `…` around the version segment; between them a paper arrow drawn on. Only
// the active version's segment lights (tools pink on its band).
//
// It takes the slip's place (LAYOUT.slip). On its own it rises in and each
// segment lights as its arrow lands. With `form` it is built from the
// slip's own chips instead (packslip, ART.md §12.7): the scene flies the
// slip's skill names into the left nodes and its version chip into the
// first segment (linkLayout has where), the install paths fade in where
// they stand, each left node forms round its name as it lands, and the
// segments are lit from the version's landing, before the arrows draw on
// to them. Every string comes from the take's rows; a pure function of
// them and the section's beat.

import { PALETTE, PILLAR } from "../../bible";
import { roundedRect } from "../../fx";
import { arc, curveAt, curveTangent, type Pt, type Rect } from "../motion";
import { advance, run, type Run } from "../term";
import {
  DUR,
  LAYOUT,
  LINK_ART,
  MOTION,
  PILLAR_BAND,
  RADIUS,
  STROKE,
} from "../style";
import { mono, monoW, pill, runsText, span, strokeAlong } from "./common";

/** One link `mise skills sync` made: the project's path, and where it points. */
export interface Link {
  from: string;
  to: string;
}

/**
 * The links a screen's `linked <from> -> <to>` rows record. The terminal
 * wraps a row at `width` columns, so a row exactly that long carries on in
 * the next.
 */
export function linkedRows(
  lines: readonly (string | readonly Run[])[],
  width = 80,
): Link[] {
  const texts = lines.map((l) => (typeof l === "string" ? l : runsText(l)));
  const joined: string[] = [];
  for (let i = 0; i < texts.length; i++) {
    let t = texts[i];
    while (Array.from(texts[i]).length === width && i + 1 < texts.length)
      t += texts[++i];
    joined.push(t);
  }
  const out: Link[] = [];
  for (const t of joined) {
    const m = /^linked (\S+) -> (\S+)$/.exec(t.trim());
    if (m) out.push({ from: m[1], to: m[2] });
  }
  return out;
}

/** A link's project side: its path from `.claude/` on (the whole path if it has none). */
export function projectPath(p: string): string {
  const i = p.indexOf("/.claude/");
  return i >= 0 ? p.slice(i + 1) : p;
}

/**
 * A link's install side, abridged round its version segment: the two
 * segments before it and the version, between `…/` and `/…`
 * (`…/installs/hk/<version>/…`), with where the version sits in it.
 * Without the version in the path, its last three segments.
 */
export function installPath(
  p: string,
  version: string,
): { text: string; lit: readonly [number, number] | null } {
  const segs = p.split("/").filter(Boolean);
  const v = segs.indexOf(version);
  if (v < 0) {
    const text = `…/${segs.slice(-3).join("/")}`;
    return { text, lit: null };
  }
  const head = `…/${segs.slice(Math.max(0, v - 2), v).join("/")}/`;
  const text = `${head}${version}/…`;
  const start = Array.from(head).length;
  return { text, lit: [start, start + Array.from(version).length] };
}

export interface SkillsLinkOptions {
  links: readonly Link[];
  /** The version whose segment lights (ReelData.versions.tools). */
  active: string;
  /** Local beats. */
  b: number;
  /** It rises in (arrive 1/2); unused with `form`. */
  enter: number;
  /** The first arrow draws on; each next one half a beat later. */
  draw: number;
  alpha?: number;
  rect?: Rect;
  /** It folds away from this beat: falling and fading (DUR.exit, leave). */
  leave?: number;
  /**
   * Built from the slip's chips (packslip): the beat the scene has landed
   * the skill names in their left nodes and the version in the first
   * segment. Nothing is drawn before it; from it the left nodes form round
   * their names and the right ones round their versions over DUR.tick
   * (glide), and the segments are lit, the first on it and each next a
   * sixteenth later.
   */
  form?: number;
}

/** The nodes' type: the reading floor for mono. */
const SIZE = 26;
/** The version's band, inside its chip. */
const BAND_INSET = 4;

/** A node's chip rect: mono SIZE on a pill 1.5 em tall, half an em either side. */
const nodeRect = (text: string, x: number, base: number): Rect => {
  const h = Math.round(1.5 * SIZE);
  return { x, y: base - 0.36 * SIZE - h / 2, w: monoW(text, SIZE) + SIZE, h };
};

/** Where a row of the drawn link stands at rest. */
export interface LinkRow {
  left: string;
  inst: { text: string; lit: readonly [number, number] | null };
  from: Rect;
  to: Rect;
  mid: number;
  base: number;
  /** Where the skill's name sits in the left node (its text's left edge), and its length. */
  name: { x: number; text: string };
  /** The version segment's lit band, and its text's left edge; null without the version in the path. */
  band: Rect | null;
  segX: number;
}

/** The drawn link's rows at rest (`dy` 0), as skillsLink sets them in `o.rect`. */
export function linkLayout(
  o: Pick<SkillsLinkOptions, "links" | "active" | "rect">,
  dy = 0,
): LinkRow[] {
  const R = o.rect ?? LAYOUT.slip;
  const n = o.links.length;
  // Two columns: the project paths set flush right against their arrows,
  // the install paths flush left after them, so every arrow is the same.
  const nodes = o.links.map((l) => ({
    left: projectPath(l.from),
    inst: installPath(l.to, o.active),
  }));
  const wOf = (t: string) => monoW(t, SIZE) + SIZE;
  const leftW = Math.max(0, ...nodes.map((x) => wOf(x.left)));
  const rightW = Math.max(0, ...nodes.map((x) => wOf(x.inst.text)));
  const adv = advance(SIZE);
  return nodes.map(({ left, inst }, i) => {
    const mid = R.y + R.h / 2 + (i - (n - 1) / 2) * LINK_ART.pitch + dy;
    const base = mid + 0.36 * SIZE;
    const from = nodeRect(left, R.x + leftW - wOf(left), base);
    const to = nodeRect(inst.text, R.x + R.w - rightW, base);
    const slash = left.lastIndexOf("/") + 1;
    const name = {
      x: from.x + SIZE / 2 + slash * adv,
      text: left.slice(slash),
    };
    const segX = inst.lit ? to.x + SIZE / 2 + inst.lit[0] * adv : to.x;
    const band = inst.lit
      ? {
          x: segX - BAND_INSET,
          y: to.y + BAND_INSET,
          w: (inst.lit[1] - inst.lit[0]) * adv + 2 * BAND_INSET,
          h: to.h - 2 * BAND_INSET,
        }
      : null;
    return { left, inst, from, to, mid, base, name, band, segX };
  });
}

/** The drawn link on beat `b`. Returns each row's two chips. */
export function skillsLink(
  ctx: CanvasRenderingContext2D,
  o: SkillsLinkOptions,
): { from: Rect; to: Rect }[] {
  const b = o.b;
  const formed = o.form !== undefined;
  const k = span(b, o.enter, DUR.enter, "arrive");
  const out = o.leave === undefined ? 0 : span(b, o.leave, DUR.exit, "leave");
  const gone = o.leave === undefined ? 0 : span(b, o.leave, DUR.exit, "glide");
  const dy = (formed ? 0 : MOTION.riseIn * (1 - k)) + MOTION.fallOut * out;
  const rows = linkLayout(o, dy);
  const alpha = (o.alpha ?? 1) * (1 - gone);
  if (alpha <= 0 || (formed ? b < o.form! : k <= 0))
    return rows.map(({ from, to }) => ({ from, to }));
  // With `form`: the left nodes' pills and their paths round the names.
  const round = formed ? span(b, o.form!, DUR.tick, "glide") : 1;
  const landed = formed && b >= o.form!;
  ctx.save();
  ctx.globalAlpha *= alpha;
  rows.forEach((row, i) => {
    const draw = o.draw + i * DUR.half;
    const arrow = span(b, draw, DUR.half, "arrive");
    // With `form`, the first segment is lit as the version lands on it,
    // each next a sixteenth later; else each as its arrow lands.
    const lit = !formed
      ? span(b, draw + DUR.half, DUR.light, "arrive")
      : i === 0
        ? landed
          ? 1
          : 0
        : span(b, o.form! + i * DUR.stagger, DUR.light, "arrive");
    // The project path: whole at rest; forming round its name with `form`.
    if (!formed || round >= 1)
      pill(ctx, [run(row.left, PALETTE.text1)], row.from.x, row.base, SIZE, {
        radius: RADIUS.chip,
      });
    else if (landed) {
      const f = row.from;
      const head = row.left.slice(0, row.left.length - row.name.text.length);
      ctx.save();
      ctx.globalAlpha *= round;
      roundedRect(ctx, f.x, f.y, f.w, f.h, RADIUS.chip);
      ctx.fillStyle = PALETTE.elevated;
      ctx.fill();
      mono(ctx, [run(head, PALETTE.text1)], f.x + SIZE / 2, row.base, SIZE);
      ctx.restore();
      mono(
        ctx,
        [run(row.name.text, PALETTE.text1)],
        row.name.x,
        row.base,
        SIZE,
      );
    }
    // The install path: rising in with the link, or forming in place round
    // its version (the first as the slip's version chip lands in it, each
    // next a sixteenth later).
    ctx.save();
    ctx.globalAlpha *= formed
      ? span(b, o.form! + i * DUR.stagger, DUR.tick, "glide")
      : 1;
    pill(ctx, [run(row.inst.text, PALETTE.text1)], row.to.x, row.base, SIZE, {
      radius: RADIUS.chip,
    });
    ctx.restore();
    // Only the active version's segment lights.
    if (row.inst.lit && row.band && lit > 0) {
      const [s, e] = row.inst.lit;
      const chars = Array.from(row.inst.text);
      ctx.save();
      ctx.globalAlpha *= lit;
      roundedRect(
        ctx,
        row.band.x,
        row.band.y,
        row.band.w,
        row.band.h,
        RADIUS.seat - 2,
      );
      ctx.fillStyle = PILLAR_BAND.tools;
      ctx.fill();
      mono(
        ctx,
        [run(chars.slice(s, e).join(""), PILLAR.tools, true)],
        row.segX,
        row.base,
        SIZE,
      );
      ctx.restore();
    }
    drawArrow(
      ctx,
      { x: row.from.x + row.from.w + 14, y: row.mid },
      { x: row.to.x - 14, y: row.mid },
      arrow,
    );
  });
  ctx.restore();
  return rows.map(({ from, to }) => ({ from, to }));
}

/** A paper arrow from `a` to `b`, bowed up (LINK_ART.lift), drawn on to `k`; its head lands at the end. */
function drawArrow(
  ctx: CanvasRenderingContext2D,
  a: Pt,
  b: Pt,
  k: number,
): void {
  if (k <= 0) return;
  const curve = arc(a, b, LINK_ART.lift);
  const pts = Array.from({ length: 33 }, (_, i) => curveAt(curve, i / 32));
  const head = LINK_ART.head;
  const len = Math.max(1, Math.hypot(b.x - a.x, b.y - a.y));
  ctx.save();
  ctx.strokeStyle = PALETTE.paper;
  ctx.lineWidth = STROKE.thread;
  ctx.lineCap = "round";
  // The line stops under the head, so it never shows past the tip.
  strokeAlong(ctx, pts, k * (1 - (0.8 * head) / len));
  if (k >= 0.9) {
    const t = curveTangent(curve, 1);
    const s = (k - 0.9) / 0.1;
    ctx.translate(b.x, b.y);
    ctx.rotate(Math.atan2(t.y, t.x));
    ctx.scale(s, s);
    ctx.fillStyle = PALETTE.paper;
    ctx.beginPath();
    ctx.moveTo(0, 0);
    ctx.lineTo(-head, -head * 0.6);
    ctx.lineTo(-head, head * 0.6);
    ctx.closePath();
    ctx.fill();
  }
  ctx.restore();
}
