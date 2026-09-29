// The folder diagram (switch, ART.md §12.8): `~/work/` over a tree whose
// branches run into two folder cards, `api/` and `dashboard/`, each a card
// with a folder tab holding its name, and inside it the project's
// mise.toml `node = …` line as the take left the file. The cwd's folder is
// lit (a paper edge, its name and its branch in paper) and a paper cwd dot
// with a soft glow sits on the trunk at its branch. On a `cd` the dot hops
// along the trunk to the other branch (an arc 20 px wide, snap, its
// overshoot held to 5 px) as the edges swap. When `node --version` prints,
// the version the terminal printed lifts out of its row onto its folder as
// a 48 px value chip: the chip forms round a copy of the row in place, rises,
// runs right past the pane's text at the row's own height, and comes into
// its slot from the left at the slot's height, so it crosses no glyph, no
// tab and no cwd dot on the way.
//
// Every string comes from the take (the folders' names, their files, the
// versions' runs); a pure function of them and the section's beat, at rest
// before its first cue and after its last. With `leave` it folds away: the
// chips shrink out, then the cards fall and fade in turn, the tree and the
// root with the last of them; with `become`, one folder stays and grows
// into the rect the station card stands in (switch into packslip), for the
// scene to bring the card in over it (becomeRect).

import { PALETTE, TERM } from "../../bible";
import { mix } from "../../color";
import { glow } from "../../fx";
import { clamp, lerp, TAU } from "../../math";
import { exit, lerpRect, type Presence, type Pt, type Rect } from "../motion";
import type { Run } from "../term";
import { DUR, EASE, FOLDERS_ART, RADIUS, STROKE, TYPE } from "../style";
import {
  chipRect,
  mono,
  monoBaseline,
  raised,
  roundCorners,
  runsText,
  span,
  tomlRuns,
  valueChip,
} from "./common";

/** A value the terminal printed, and where it printed it (its first column's left edge, its baseline, its size). */
export interface Printed {
  runs: readonly Run[];
  /** The beat it printed: it lifts from then. */
  at: number;
  from?: { x: number; y: number; size: number };
  /**
   * An x past the end of every row on the pane while the chip flies: it
   * runs right at its row's height to here before it turns for its slot
   * (default halfway to the slot).
   */
  clear?: number;
}

/** A folder: its name, its mise.toml as the take left it, and the version its project printed. */
export interface Folder {
  name: string;
  toml: string | null;
  version?: Printed;
}

/**
 * A value's lift onto its folder, beats (ART.md §9): the chip forms round a
 * copy of the row in place, rises, flies to its slot, and settles onto it.
 */
export interface Lift {
  form: number;
  rise: number;
  fly: number;
  settle: number;
}

/** ART.md §9's lift: 1/8 + 1/8 + 3/4 + 1/4 beat. */
const LIFT: Lift = {
  form: DUR.flick,
  rise: DUR.flick,
  fly: DUR.lift,
  settle: DUR.tick,
};

/** The folder that stays as the diagram folds away, growing into another rect (switch → packslip's station card). */
export interface Become {
  /** Its index in `folders`. */
  index: number;
  /** The rect it grows into, from `leave` + 1/8 over DUR.move (move). */
  rect: Rect;
  /**
   * 0 to 1: how far the card taking its place has come in over it (the
   * scene draws it, at becomeRect): its shadow gives way to the card's,
   * and at 1 the folder is gone under it.
   */
  cover?: number;
  /** Its own alpha, 0 to 1: when nothing takes its place on the bar line, it fades as it grows. */
  alpha?: number;
}

export interface FoldersOptions {
  folders: readonly Folder[];
  /** Local beats. */
  b: number;
  /** The folder the shell is in first, and each `cd` after it: its beat and the folder it lands in. */
  cwd: number;
  cds?: readonly { at: number; to: number }[];
  /** The folders' parent (`~/work/`). */
  root: string;
  alpha?: number;
  /** The take named when a file is missing (default C7). */
  from?: string;
  /** How the versions lift onto their folders (default LIFT). */
  lift?: Lift;
  /** The beat the diagram starts to fold away; gone DUR.flick + DUR.exit (plus a stagger) later. */
  leave?: number;
  /** One folder that stays through the fold and grows into another rect. */
  become?: Become;
  /** 0 to 1: the cwd dot's glow swelling (a hold breathing on the beat). */
  pulse?: number;
}

/** Where the folder diagram's parts are on a frame. */
export interface FoldersLayout {
  /** Each folder's card, and its version chip once it has landed. */
  cards: Rect[];
  chips: (Rect | null)[];
  /** The cwd dot. */
  dot: Pt;
}

/**
 * The version chips: mono 48, 20 px in from the card's bottom-left. 48,
 * not FOLDERS_ART.chip's 40: at 40 the two versions, the section's payoff,
 * were 8 px tall at phone width.
 */
const CHIP = { size: 48, inset: 20 } as const;

/** Inside a folder's body: "mise.toml" and the node line's baselines, clear of the 48 px chip under them. */
const LABEL_Y = 32;
const NODE_Y = 70;

/** The body of a folder card under its tab. */
const bodyOf = (r: Rect): Rect => ({
  x: r.x,
  y: r.y + FOLDERS_ART.tab.h,
  w: r.w,
  h: r.h - FOLDERS_ART.tab.h,
});

/** A branch's y: the middle of its folder's tab. */
const branchY = (r: Rect): number => r.y + FOLDERS_ART.tab.h / 2;

/** The corner where a branch leaves the trunk, px. */
const BRANCH_R = 12;
/** The dot's hop: an arc this wide beside the trunk. */
const HOP = 20;
/** The most the dot overshoots its branch along the trunk, px (snap's 12 % of a 258 px hop was 31). */
const HOP_OVER = 5;
/** The corners of a version's flight, px. */
const FLIGHT_R = 90;

/** A folder's outline: its tab on its top-left, merging into the card. */
function folderShape(ctx: CanvasRenderingContext2D, r: Rect): () => void {
  const T = FOLDERS_ART.tab;
  const tr = RADIUS.tab;
  const cr = RADIUS.file;
  const yb = r.y + T.h;
  return () => {
    ctx.beginPath();
    ctx.moveTo(r.x, r.y + tr);
    ctx.arcTo(r.x, r.y, r.x + tr, r.y, tr);
    ctx.lineTo(r.x + T.w - tr, r.y);
    ctx.arcTo(r.x + T.w, r.y, r.x + T.w, r.y + tr, tr);
    ctx.lineTo(r.x + T.w, yb - tr);
    ctx.arcTo(r.x + T.w, yb, r.x + T.w + tr, yb, tr);
    ctx.lineTo(r.x + r.w - cr, yb);
    ctx.arcTo(r.x + r.w, yb, r.x + r.w, yb + cr, cr);
    ctx.lineTo(r.x + r.w, r.y + r.h - cr);
    ctx.arcTo(r.x + r.w, r.y + r.h, r.x + r.w - cr, r.y + r.h, cr);
    ctx.lineTo(r.x + cr, r.y + r.h);
    ctx.arcTo(r.x, r.y + r.h, r.x, r.y + r.h - cr, cr);
    ctx.closePath();
  };
}

/**
 * How current each folder is on beat `b` (0 to 1: its edge, name and
 * branch in paper), and where the dot is along the trunk. The hop runs
 * over DUR.half on snap, but its overshoot past the branch is held to
 * HOP_OVER px; its sideways arc follows how far it has come (unovershot),
 * so it bows out mid-hop and is back on the trunk as it reaches the branch.
 */
export function cwdState(
  o: Pick<FoldersOptions, "folders" | "b" | "cwd" | "cds">,
): { current: number[]; dotY: number; dotX: number } {
  const F = FOLDERS_ART;
  const ys = F.folders.map(branchY);
  let from = o.cwd;
  let to = o.cwd;
  let at = -Infinity;
  for (const cd of o.cds ?? [])
    if (cd.at <= o.b) {
      from = to;
      to = cd.to;
      at = cd.at;
    }
  const swap = at === -Infinity ? 1 : span(o.b, at, DUR.tick, "arrive");
  const current = o.folders.map((_, i) =>
    i === to && i === from ? 1 : i === to ? swap : i === from ? 1 - swap : 0,
  );
  const y0 = ys[from] ?? ys[0];
  const y1 = ys[to] ?? ys[0];
  let e = at === -Infinity ? 1 : span(o.b, at, DUR.half, "snap");
  const dy = Math.abs(y1 - y0);
  if (e > 1 && dy > 0) e = 1 + (e - 1) * Math.min(1, HOP_OVER / (0.12 * dy));
  const u = clamp(e);
  return {
    current,
    dotY: lerp(y0, y1, e),
    dotX: F.trunkX - HOP * 4 * u * (1 - u),
  };
}

/** How far a becoming folder has grown on beat `b` (0 to 1), from `leave` + 1/8 over DUR.move. */
const growOf = (o: Pick<FoldersOptions, "b" | "leave">): number =>
  o.leave === undefined ? 0 : span(o.b, o.leave + DUR.flick, DUR.move, "move");

/**
 * Where the becoming folder (FoldersOptions.become) stands on beat `b`:
 * its folder's rect growing into `become.rect`, for the scene to draw the
 * card taking its place in. Null without `become`.
 */
export function becomeRect(
  o: Pick<FoldersOptions, "b" | "leave" | "become">,
): Rect | null {
  if (!o.become) return null;
  const r = FOLDERS_ART.folders[o.become.index];
  return lerpRect(r, o.become.rect, growOf(o));
}

/** The folder diagram on beat `b`. */
export function folders(
  ctx: CanvasRenderingContext2D,
  o: FoldersOptions,
): FoldersLayout {
  const F = FOLDERS_ART;
  const cards = F.folders.slice(0, o.folders.length);
  const state = cwdState(o);
  const out: FoldersLayout = {
    cards: [...cards],
    chips: o.folders.map(() => null),
    dot: { x: state.dotX, y: state.dotY },
  };
  const alpha = o.alpha ?? 1;
  const gone = foldedAway(o, cards.length);
  const grown = becomeRect(o);
  if (o.become && grown) out.cards[o.become.index] = grown;
  if (alpha <= 0 || !cards.length || gone.done) return out;
  ctx.save();
  ctx.globalAlpha *= alpha;
  // The root and the tree go with the last card to leave.
  const last = gone.tree;
  if (last.alpha > 0) {
    ctx.save();
    ctx.globalAlpha *= last.alpha;
    ctx.translate(0, last.dy);
    // The root, and the tree: every branch in the divider's colour, the
    // current one's path from the root over it in paper.
    mono(ctx, o.root, F.root.x, F.root.y, F.root.size, {
      color: PALETTE.text2,
    });
    const branch = (r: Rect) => {
      const by = branchY(r);
      ctx.beginPath();
      ctx.moveTo(F.trunkX, F.trunkTop);
      ctx.lineTo(F.trunkX, by - BRANCH_R);
      ctx.arcTo(F.trunkX, by, F.trunkX + BRANCH_R, by, BRANCH_R);
      ctx.lineTo(r.x, by);
      ctx.stroke();
    };
    ctx.save();
    ctx.lineWidth = STROKE.tree;
    ctx.lineCap = "round";
    ctx.strokeStyle = PALETTE.divider;
    cards.forEach(branch);
    cards.forEach((r, i) => {
      const k = state.current[i];
      if (k <= 0) return;
      ctx.save();
      ctx.globalAlpha *= k;
      ctx.strokeStyle = PALETTE.paper;
      branch(r);
      ctx.restore();
    });
    ctx.restore();
    ctx.restore();
  }
  // The cards: the becoming one last, over the others, once it grows
  // (before that in their own order, as a bar line's rest draws them).
  const order = cards.map((_, i) => i);
  if (o.become && growOf(o) > 0) {
    order.splice(order.indexOf(o.become.index), 1);
    order.push(o.become.index);
  }
  for (const i of order) {
    const becoming = o.become?.index === i && grown !== null;
    const cover = becoming ? clamp(o.become?.cover ?? 0) : 0;
    if (cover >= 1) continue;
    const r = becoming ? grown : cards[i];
    const f = o.folders[i];
    const k = state.current[i];
    const p = becoming
      ? { alpha: clamp(o.become?.alpha ?? 1), dy: 0 }
      : gone.cards[i];
    if (p.alpha <= 0) continue;
    // A becoming folder's own contents give way as it starts to grow.
    const ink = becoming
      ? 1 - span(o.b, (o.leave ?? 0) + DUR.flick, DUR.tick, "glide")
      : 1;
    ctx.save();
    ctx.globalAlpha *= p.alpha;
    ctx.translate(0, p.dy);
    raised(
      ctx,
      r,
      RADIUS.file,
      {
        edge: mix(PALETTE.divider, PALETTE.paper, k * ink),
        edgeWidth: lerp(STROKE.hairline, STROKE.lit, k * ink),
        shadowAlpha: 1 - cover,
      },
      folderShape(ctx, r),
    );
    if (ink > 0) {
      ctx.save();
      ctx.globalAlpha *= ink;
      const T = F.tab;
      mono(ctx, f.name, r.x + 18, monoBaseline(r.y, T.h, T.size), T.size, {
        color: mix(PALETTE.text2, PALETTE.text1, k),
        bold: true,
      });
      const body = bodyOf(r);
      mono(ctx, "mise.toml", body.x + 24, body.y + LABEL_Y, TYPE.file.title, {
        color: PALETTE.text3,
      });
      const line = f.toml?.split("\n").find((l) => /^node\s*=/.test(l));
      if (f.toml === null)
        mono(
          ctx,
          `CAPTURE MISSING: ${o.from ?? "C7"}`,
          body.x + 24,
          body.y + NODE_Y,
          TYPE.file.size,
          { color: TERM.ansi[1] },
        );
      else if (line)
        mono(ctx, tomlRuns(line), body.x + 24, body.y + NODE_Y, F.row.size);
      ctx.restore();
    }
    ctx.restore();
  }
  // The cwd dot, on the trunk at the current folder's branch.
  if (last.alpha > 0) {
    ctx.save();
    ctx.globalAlpha *= last.alpha;
    ctx.translate(0, last.dy);
    const swell = clamp(o.pulse ?? 0);
    glow(
      ctx,
      state.dotX,
      state.dotY,
      40 + 24 * swell,
      PALETTE.paper,
      0.35 + 0.3 * swell,
    );
    ctx.fillStyle = PALETTE.paper;
    ctx.beginPath();
    ctx.arc(state.dotX, state.dotY, F.cwd.r, 0, TAU);
    ctx.fill();
    ctx.restore();
  }
  // The versions, lifting out of the terminal onto their folders, and
  // shrinking out first when the diagram folds away.
  cards.forEach((r, i) => {
    const v = o.folders[i].version;
    if (!v || gone.chip <= 0) return;
    ctx.save();
    ctx.globalAlpha *= gone.chip;
    out.chips[i] = drawVersion(
      ctx,
      v,
      bodyOf(r),
      o.b,
      o.lift ?? LIFT,
      0.9 + 0.1 * gone.chip,
    );
    ctx.restore();
  });
  ctx.restore();
  return out;
}

/**
 * How far the diagram has folded away at `o.b`: the chips shrink out over
 * a quarter beat from `leave` (their fade on glide, so none ends in a pop),
 * then each card that leaves falls and fades (exit) a sixteenth after the
 * one above it, from an eighth later, the tree and the root with the last
 * of them; `done` once all are gone (a becoming folder is the scene's to
 * end). All at rest (1, 0) before `leave`.
 */
function foldedAway(
  o: Pick<FoldersOptions, "b" | "leave" | "become">,
  n: number,
): { chip: number; cards: Presence[]; tree: Presence; done: boolean } {
  const still: Presence = { alpha: 1, dy: 0 };
  if (o.leave === undefined || o.b < o.leave)
    return {
      chip: 1,
      cards: Array.from({ length: n }, () => still),
      tree: still,
      done: false,
    };
  const at = o.leave;
  const chip = 1 - span(o.b, at, DUR.tick, "glide");
  const leaving = Array.from({ length: n }, (_, i) => i).filter(
    (i) => i !== o.become?.index,
  );
  const cards = Array.from({ length: n }, (_, i) => {
    const j = leaving.indexOf(i);
    return j < 0
      ? still
      : exit(o.b, at + DUR.flick + j * DUR.stagger, DUR.exit);
  });
  const tree = leaving.length ? cards[leaving[leaving.length - 1]] : still;
  return {
    chip,
    cards,
    tree,
    done: !o.become && cards.every((p) => p.alpha <= 0),
  };
}

/** The point `u` (0 to 1) of the way along polyline `pts` by length. */
export function along(pts: readonly Pt[], u: number): Pt {
  const lens = [0];
  for (let i = 1; i < pts.length; i++)
    lens.push(
      lens[i - 1] +
        Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y),
    );
  const d = clamp(u) * lens[lens.length - 1];
  for (let i = 1; i < pts.length; i++) {
    if (lens[i] < d) continue;
    const seg = lens[i] - lens[i - 1];
    const k = seg > 0 ? (d - lens[i - 1]) / seg : 0;
    return {
      x: lerp(pts[i - 1].x, pts[i].x, k),
      y: lerp(pts[i - 1].y, pts[i].y, k),
    };
  }
  return pts[pts.length - 1];
}

/** Where a folder's version chip lands: its left edge and its text's baseline. */
function slotOf(body: Rect): Pt {
  const h = Math.round(1.5 * CHIP.size);
  const bottom = body.y + body.h - CHIP.inset;
  return { x: body.x + CHIP.inset, y: bottom - h / 2 + 0.36 * CHIP.size };
}

/**
 * A printed value lifting onto its folder (ART.md §9), over `lift`'s beats:
 * the chip forms round a copy of the row in place (its fill covering the
 * pane's own glyphs from the first frame, the copy on top of them, so the
 * row never reads doubled), rises 6 px, flies (travel), growing to 48 px:
 * right along its row's height to `clear`, round a corner, and into its
 * slot from the left at the slot's height; and settles onto it with snap.
 * Without a source it pops in where it lands. Returns the chip's rect once
 * it has landed.
 */
function drawVersion(
  ctx: CanvasRenderingContext2D,
  v: Printed,
  body: Rect,
  b: number,
  lift: Lift,
  scale = 1,
): Rect | null {
  const n = Array.from(runsText(v.runs)).length;
  const land = slotOf(body);
  if (b < v.at) return null;
  if (scale !== 1) {
    // Folding away: the landed chip shrinks about its own centre.
    const r = chipRect(n, land.x, land.y, CHIP.size);
    ctx.save();
    ctx.translate(r.x + r.w / 2, r.y + r.h / 2);
    ctx.scale(scale, scale);
    ctx.translate(-(r.x + r.w / 2), -(r.y + r.h / 2));
    const out = drawVersion(ctx, v, body, b, lift);
    ctx.restore();
    return out;
  }
  if (!v.from) {
    const k = span(b, v.at, lift.settle, "snap");
    ctx.save();
    ctx.globalAlpha *= clamp(k);
    const r = chipRect(n, land.x, land.y, CHIP.size);
    ctx.translate(r.x + r.w / 2, r.y + r.h / 2);
    ctx.scale(0.9 + 0.1 * k, 0.9 + 0.1 * k);
    ctx.translate(-(r.x + r.w / 2), -(r.y + r.h / 2));
    valueChip(ctx, v.runs, land.x, land.y, CHIP.size);
    ctx.restore();
    return b >= v.at + lift.settle ? r : null;
  }
  const s = v.from;
  const form = span(b, v.at, lift.form, "arrive");
  const rise = span(b, v.at + lift.form, lift.rise, "settle");
  const flyAt = v.at + lift.form + lift.rise;
  const fly = span(b, flyAt, lift.fly, "travel");
  const settle = span(b, flyAt + lift.fly, lift.settle, "snap");
  // The chip's text origin: from the printed row, right along its height,
  // and into the slot from the left (12 px short of it, then the settle).
  const start: Pt = { x: s.x, y: s.y - 6 * rise };
  const end: Pt = { x: land.x + CHIP.size / 2 - 12, y: land.y };
  const clear = v.clear ?? (start.x + end.x) / 2;
  const route = roundCorners(
    [start, { x: clear, y: start.y }, { x: clear, y: end.y }, end],
    FLIGHT_R,
  );
  const p = fly > 0 ? along(route, fly) : start;
  // It keeps its row's size along the row (a 48 px chip is taller than the
  // rows' pitch), and grows once it is past their text.
  const total = route.reduce(
    (n, q, i) =>
      i ? n + Math.hypot(q.x - route[i - 1].x, q.y - route[i - 1].y) : 0,
    0,
  );
  const u0 = total > 0 ? (0.8 * Math.abs(clear - start.x)) / total : 0;
  const size = lerp(
    s.size,
    CHIP.size,
    EASE.move(clamp((fly - u0) / Math.max(1e-6, 1 - u0))),
  );
  const dx = fly >= 1 ? 12 * settle : 0;
  const x = p.x + dx - size / 2;
  const y = p.y;
  ctx.save();
  // The chip, forming round the copy: its fill over the pane's own glyphs.
  valueChip(ctx, v.runs, x, y, size, { alpha: form });
  // The copy, on top, exactly over the row it came from while it forms.
  if (form < 1) mono(ctx, v.runs, x + size / 2, y, size);
  ctx.restore();
  // At rest by the clock (snap overshoots past 1 on its way there).
  return b >= flyAt + lift.fly + lift.settle
    ? chipRect(n, land.x, land.y, CHIP.size)
    : null;
}
