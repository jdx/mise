// backends (plan v3 §3, Act I; ART.md §7, §9, §12.10; STORYBOARD.md
// "Pacing"): the api card holds from the registry. One focal motion at a
// time, half a beat apart (kit/pace.ts), whatever is not the focus dimmed.
// A short terminal rises in at `~/work/api $` and, under it, `prettier` in
// big type; the take types `mise use ` at TYPE_RATE; then a slot opens in
// front of `prettier` and `npm:` types into it in tools pink, a key an
// eighth; the slot goes solid and fades, the word holds whole at full size
// for a beat, and `npm:prettier` shrinks into its own cells after `mise
// use `; the take cuts to the word typed as it lands there. A beat of anticipation,
// Enter, and its install rows (with the `↳ 1/1 pkgs` sub-row) move at 3×
// under the time-lapse badge and fade out while they still move, before
// any row finishes; the pane holds, badged, on the frame the command was
// entered on for the time-lapse's 1.5 s, then dims as `"npm:prettier" =
// "latest"` seats in the card, and the caption follows. A real Ctrl-L on
// the ⌃L keycap (the card folding up to make room for the shelf), and
// `prettier --check README.md .github` runs; its verdict lifts out of its
// row into a panel across the stage and holds, then a file chip comes up
// and ticks for each file the command named, and the footnote says whose
// node it ran on.
//
// Then the card, the shelf and the footnote go, and the terminal widens
// across the stage (centred in the actor band with the chips under it) as
// its title changes to `~/work/tools` (C6, gh installed beforehand): `mise
// use github:cli/cli` prints its tools line (the progress header it
// flashes for 0.2 s is cut, the line keeping its real time), and `gh
// --version` its two real lines, each held for its read; then text chips
// for the other backends land under the pane, the caption names them, and
// the second footnote names what some of them need. The pane narrows back
// into the left column with gh's lines on it, and the card slides back
// beside it, full height: both hold into the Versions ticket, which takes
// them down (kit/rest.ts backends|versions; STORYBOARD.md: the section
// before a ticket never clears its own stage).

import { BEAT, PALETTE, PILLAR, type ReelFacts } from "../bible";
import {
  type Capture,
  capture,
  type CaptureId,
  fileOf,
  firstWith,
  frameAfter,
  keyAt,
  type ReelData,
  reelData,
  stepOf,
} from "../captures";
import {
  backendChip,
  beatOf,
  bigline,
  card,
  chain,
  cut,
  fileChip,
  findRow,
  footnote,
  greyScene,
  keep,
  keycap,
  keycapOn,
  type Play,
  playLen,
  raised,
  rectOf,
  shot,
  slot,
  step,
  tick,
  TYPE_RATE,
  typed,
  win,
} from "../kit/grey";
import { bump, enter, exit, lerpRect, type Rect, span } from "../kit/motion";
import { focusOf, focusPlan, GAP, Pace, readsOf, unfocus } from "../kit/pace";
import { drawPaneWindow, HOLDS } from "../kit/rest";
import {
  CHIP_ART,
  DUR,
  EASE,
  LAYOUT,
  MOTION,
  PANE_DIM,
  PANES,
  RADIUS,
  SHADOW,
  TYPE,
  ZOOM_ART,
} from "../kit/style";
import {
  advance,
  drawTermLine,
  lerpPane,
  type Pane,
  termLit,
} from "../kit/term";
import { lerp, smoothstep } from "../math";
import type { SceneCues } from "../score/cues";
import {
  ARRIVE_BY,
  cellsOf,
  enteredInstall,
  KINETIC,
  perFacts,
  shrinkAt,
  takeIn,
  typedFrame,
  typedOf,
  wordFrame,
} from "./g2-tools/common";

const CARD = LAYOUT.card;
/** The card folded up to its rows while the bottom of the stage holds prettier's result. */
const CARD_UP = LAYOUT.envCard;
const TITLE = "~/work/api/mise.toml";

/** The terminal: short over the kinetic type and the result shelf (PANES.slip), then wide for the tools project. */
const SHORT: Pane = PANES.slip;
/** The other backends' chips: how far under the wide pane, and how tall (CHIP_ART.backendH). */
const CHIPS_GAP = 26;
const CHIPS_H = CHIP_ART.backendH;
/**
 * Across the stage, one row taller than PANES.wideShort so gh's lines and
 * the command above them all show, set so the pane and the chips under it
 * sit centred in the actor band (y 120–690), not in its top half.
 */
const WIDE: Pane = (() => {
  const w = PANES.wideShort;
  const h = w.h + w.lineH;
  const band = LAYOUT.left;
  const y = Math.round(band.y + (band.h - (h + CHIPS_GAP + CHIPS_H)) / 2);
  return {
    ...w,
    y,
    h,
    rows: w.rows + 1,
    baseline0: w.baseline0 + (y - w.y),
  };
})();

// `npm:` into its slot, and into the pane: the pane and `prettier` rise
// in, and the take types `mise use ` at TYPE_RATE (under a beat); half a
// beat after, a slot opens, `npm:` types into it a key an eighth (8 a
// second), the word holds whole at full size for a beat (rule 7), and
// shrinks into its cells after `mise use ` (one motion at a time: nothing
// types while the word holds).
const PANE_IN = 0;
const PRETTIER_IN = 0;
const SLOT_AT = 19 / 8;
/** `mise use ` is on the row by here, half a beat before the slot opens. */
const TYPED_BY = SLOT_AT - GAP;
const NPM_AT = SLOT_AT + 1 / 8;
const NPM_EACH = 1 / 8;
const SOLID = NPM_AT + 1 / 2;
/** Whole once its last key lands (NPM_AT + 3/8 + a sixteenth), held at full size a beat, then shrinking. */
const SHRINK = NPM_AT + 3 * NPM_EACH + DUR.stagger + 1 + DUR.stagger;
const LAND = SHRINK + DUR.bigShrink;
/** The word arrives on its cells (the take cuts to it typed there), fading by LAND. */
const ARRIVE = SHRINK + ARRIVE_BY * DUR.bigShrink;
/** The kinetic line's centre and baseline, in the left column under the short pane (use's slam stands there too). */
const KX = KINETIC.x;
const KY = KINETIC.y;
const KSIZE = TYPE.big.kinetic;

/** The shelf under the short pane: the verdict's panel, and the file chips under it. */
const SHELF_Y = SHORT.y + SHORT.h + 40;
const ZOOM_H = 104;
const FILES_Y = SHELF_Y + ZOOM_H + 18;
/** A file chip's tick, just after it, and the gap to the next chip. */
const TICK = TYPE.chip.file;
const TICK_GAP = 12;
const FILE_GAP = 24;

/** The ⌃L: the keycap coming up, pressed, let go; the card folds up for the shelf from the press. */
const CLEAR_DUR = DUR.keycapLead + DUR.keyDown + DUR.keyUp;
/**
 * To the tools project: the card and the shelf leave (a quarter beat),
 * then the pane widens into the card's column (DUR.move).
 */
const OFF = DUR.tick;
const WIDEN = DUR.move;
/**
 * The check's screen fades out over the widen's first 3/16 beat, and C6's
 * prompt fades in over the next 3/16, titling the widening window
 * `~/work/tools`: the title changes without the two screens ever showing
 * at once.
 */
const SWAP = 3 / 16;
const CHIPS_Y = WIDE.y + WIDE.h + CHIPS_GAP;
/** Back: the pane narrows to SHORT (the rest's pane), and the card comes back once it has the room. */
const NARROW = DUR.move;

/** The other backends, as text chips. */
const OTHERS = ["pypi:", "cargo:", "go:"] as const;

/** What prettier prints for each file it checks, and its verdict. */
const FILES = [/^README\.md$/, /^\.github\/workflows\/ci\.yml$/] as const;
const VERDICT = /^All matched files use Prettier code style!$/;

/** The fallback words without the take. */
const FALLBACK_WORD = "npm:prettier";

/**
 * `mise use npm:prettier` up to its Enter: `mise use ` typed at TYPE_RATE
 * once the pane is up, done half a beat before the slot opens, so it
 * waits on the row for the shrinking word to arrive in the cells after
 * it, where the take cuts to the word typed (the kinetic type stands in
 * for its typing: wordFrame). The install (Pace.lapse) carries on from
 * there.
 */
function typingPlays(c: Capture): Play[] {
  const u = stepOf(c, "use");
  const word = typedOf(c, "use")?.split(" ").pop() ?? FALLBACK_WORD;
  const pre = wordFrame(c, "use", word);
  const typing = (pre - u.type) / (BEAT * TYPE_RATE);
  return [
    cut(0, u.type),
    ...(pre > u.type
      ? chain(TYPED_BY - typing, [[u.type, pre, TYPE_RATE]])
      : []),
    cut(ARRIVE, typedFrame(c, "use")),
  ];
}

/**
 * C6's `mise use github:cli/cli` from beat `at`: typed at TYPE_RATE, a
 * beat of anticipation, Enter; the progress header it flashes is cut (the
 * entered command holds for its real time), and its tools line prints with
 * the prompt after it.
 */
function ghUsePlays(c: Capture, at: number): Play[] {
  const u = stepOf(c, "use");
  const tools = firstWith(c, /tools: github:cli\/cli@/, u.enter) ?? u.end;
  // The prompt comes back a frame after the tools line: they land as one.
  const prompt = firstWith(c, /^~\/work\/\w+ \$/, tools + 1e-6) ?? tools;
  const t = typed(at, u.type, u.enter, u.enter + 0.005);
  return [t, cut(at + playLen(t) + (tools - u.enter - 0.005) / BEAT, prompt)];
}

/**
 * When everything happens (kit/pace.ts): the section's focal motions one at
 * a time from the takes, the captions anchored on them, and the focus.
 * Without the takes, the plan's own beats (sections.json).
 */
const schedule = perFacts((d: ReelData | null) => {
  const c5 = takeIn(d, "C5");
  const c6 = takeIn(d, "C6");
  const p = new Pace("backends", { d, from: 0 });
  const none = { plays: [] as Play[] };
  const npm = p.move("npm", LAND);
  const install = c5
    ? p.lapse(
        "install",
        (at) => enteredInstall(c5, "use", at),
        (ps) => ps[1].at,
      )
    : { ...p.move("install", 1.875), ...none };
  const seat = p.move("seat", DUR.seatGap + DUR.seat);
  p.caption(0);
  const clear = p.move("clear", CLEAR_DUR, { after: p.termReady });
  const press = clear.at + DUR.keycapLead;
  const check = c5
    ? p.term("check", (at) => [step(c5, "check", at)], { lines: 2 })
    : { ...p.move("check", 4.02), ...none };
  const verdictS = p.move("verdict", DUR.zoom);
  const chipsS = p.move("chips", 3 * DUR.tick);
  const foot1 = p.move("foot1", DUR.enter);
  const toTools = p.move("toTools", OFF + WIDEN);
  const gh = c6
    ? p.term("gh", (at) => ghUsePlays(c6, at), { lines: 1 })
    : { ...p.move("gh", 2.7), ...none };
  const ghVersion = c6
    ? p.term(
        "ghVersion",
        (at) => [step(c6, "gh", at, { until: HOLDS.C6!(c6) })],
        { lines: 2 },
      )
    : { ...p.move("ghVersion", 1.98), ...none };
  const backendChips = p.move("backendChips", 3 * DUR.tick);
  p.caption(1);
  const foot2 = p.move("foot2", DUR.enter);
  const narrow = p.move("narrow", NARROW + DUR.enter);
  // The takes on those beats.
  const uses = c5 ? [...typingPlays(c5), ...install.plays] : [];
  const checks = c5
    ? [cut(press, frameAfter(c5, keyAt(c5, "\f"))), ...check.plays]
    : [];
  const ghs = c6
    ? [
        cut(toTools.at + OFF, stepOf(c6, "use").type),
        ...gh.plays,
        ...ghVersion.plays,
      ]
    : [];
  // The file chips come up in turn once the verdict has zoomed, each
  // ticking a quarter beat after it rises.
  const files = FILES.map((_, i) => ({
    up: chipsS.at + i * DUR.tick,
    tick: chipsS.at + (i + 1) * DUR.tick,
  }));
  let verdict = check.end;
  let named: string[] = [];
  if (c5) {
    const chk = stepOf(c5, "check");
    const v = firstWith(c5, VERDICT, chk.enter);
    if (v !== null) verdict = beatOf(checks, v);
    named = FILES.map((re) => {
      const t = firstWith(c5, re, chk.enter);
      return t === null ? "" : findName(c5, re, t);
    });
  }
  let ghAt = gh.end;
  if (c6) {
    const u = stepOf(c6, "use");
    const tools = firstWith(c6, /tools: github:cli\/cli@/, u.enter) ?? u.end;
    ghAt = beatOf(ghs, tools);
  }
  const leave = toTools.at;
  const paneOut = narrow.at;
  // The terminal has the focus while it acts (with `prettier` as `mise use `
  // types, the kinetic word alone from its slot); the card while a line seats
  // in it; the shelf while the verdict and its files show; the chips
  // while they land and the caption names them; the terminal stays lit
  // until each output has been read (rule 3). All lit on the bar line.
  const focus = focusPlan(
    [
      [npm.at, ["kinetic", "term"]],
      [npm.at + SLOT_AT, "kinetic"],
      [install.at, "term"],
      [seat.at, "card"],
      [clear.at, "term"],
      [verdictS.at, "shelf"],
      [toTools.at, "term"],
      [backendChips.at, "chips"],
      [narrow.at, "*"],
    ],
    readsOf(check, gh, ghVersion),
  );
  return {
    p,
    uses,
    checks,
    ghs,
    focus,
    install,
    cutAt: install.end,
    seat: seat.at,
    seated: seat.end,
    clear: clear.at,
    press,
    checkAt: check.at,
    files,
    named,
    verdict,
    zoom: verdictS.at,
    foot1: [foot1.at, leave + OFF] as const,
    leave,
    toolsUp: leave + OFF + SWAP,
    ghIn: leave + OFF + WIDEN,
    gh: ghAt,
    chips: OTHERS.map((_, i) => backendChips.at + i * DUR.tick),
    foot2: [foot2.at, paneOut + OFF] as const,
    paneOut,
    cardBack: paneOut + NARROW,
  };
});

/** The text of the row matching `re` on the take's screen at `t`. */
function findName(c: Capture, re: RegExp, t: number): string {
  const f = c.frames.find((x) => x.t >= t);
  if (!f) return "";
  for (const r of f.rows) {
    const text = c.rows[r].map(([s]) => s).join("");
    if (re.test(text)) return text;
  }
  return "";
}

/** The terminal's window at beat `b`: where it stands, and how present it is. */
function paneAt(
  b: number,
  S: ReturnType<typeof schedule>,
): { pane: Pane; alpha: number } {
  const widen = S.leave + OFF;
  if (b < widen) {
    const i = enter(b, PANE_IN, DUR.enter);
    return {
      pane: { ...SHORT, y: SHORT.y + i.dy, baseline0: SHORT.baseline0 + i.dy },
      alpha: i.alpha,
    };
  }
  // Wide for the tools project, then back to SHORT, where it rests.
  const n = span(b, S.paneOut, NARROW, "move");
  if (n >= 1) return { pane: SHORT, alpha: 1 };
  if (n > 0) return { pane: lerpPane(WIDE, SHORT, n), alpha: 1 };
  return {
    pane: lerpPane(SHORT, WIDE, span(b, widen, WIDEN, "move")),
    alpha: 1,
  };
}

/**
 * The card's rect and presence: at rest, folded up for the shelf from the
 * ⌃L, off to the right (gone before the pane widens into its column), and
 * back once the pane has narrowed.
 */
function cardAt(
  b: number,
  S: ReturnType<typeof schedule>,
): { rect: Rect; alpha: number } {
  if (b < S.cardBack) {
    const r = lerpRect(CARD, CARD_UP, span(b, S.press, DUR.move, "move"));
    const off = span(b, S.leave, OFF, "leave");
    const fade = span(b, S.leave, OFF, "glide");
    return { rect: { ...r, x: r.x + MOTION.slideIn * off }, alpha: 1 - fade };
  }
  const on = span(b, S.cardBack, DUR.enter, "arrive");
  return {
    rect: { ...CARD, x: CARD.x + MOTION.slideIn * (1 - on) },
    alpha: on,
  };
}

export const scene = greyScene(
  "backends",
  (g) => {
    const { ctx, b } = g;
    const c5 = capture(g.d, "C5");
    const S = schedule(g.d);
    // The card: C3's file until npm:prettier seats, then C5's; dimmed
    // while something else has the focus.
    const seating = b >= S.seat;
    const cd = cardAt(b, S);
    keep(g, () =>
      card(ctx, cd.rect, {
        title: TITLE,
        text: fileOf(g.d, seating ? "C5" : "C3", "work/api/mise.toml"),
        from: seating ? "C5" : "C3",
        open: ["[tools]"],
        lit: { "[tools]": 0.6 * bump(b, S.seated, DUR.ignite) },
        seat: seating
          ? [
              {
                re: /^"npm:prettier" = /,
                a: 1 - span(b, S.seated + DUR.seatHold, DUR.seatFade, "glide"),
                drop: span(b, S.seat, DUR.seatGap + DUR.seat, "linear"),
              },
            ]
          : undefined,
        alpha: cd.alpha * focusOf(S.focus, "card", b),
        // As tall as its rows (kit/rest.ts registry|backends, backends|versions).
        fit: true,
      }),
    );
    // The terminal's window: dimmed while the card, the shelf or the
    // chips have the focus.
    const pw = paneAt(b, S);
    const dim = unfocus(S.focus, "term", b);
    // Kept: it holds through the bar line into the Versions ticket.
    if (pw.alpha > 0)
      keep(g, () => {
        ctx.save();
        ctx.globalAlpha *= pw.alpha;
        drawPaneWindow(ctx, pw.pane, dim);
        ctx.restore();
      });
    const alphaOf = () => pw.alpha;
    // The install: its rows fade while they move, then the pane holds the
    // frame the command was entered on, dimmed and badged, to the ⌃L.
    const s1 = shot(g, "C5", pw.pane, -1, S.press, () => S.uses, {
      bare: true,
      fade: 0,
      dim: alphaOf,
      focus: () => dim,
      // The time-lapse badge holds with the entered command for its 1.5 s.
      tail: { badgeUntil: S.install.end },
    });
    // The check: the ⌃L cuts straight to its fresh prompt (a real clear,
    // no fade in), and it stays in the short pane's grid as the window
    // starts to widen, its text fading out over SWAP.
    ctx.save();
    ctx.globalAlpha *= 1 - smoothstep(S.toolsUp - SWAP, S.toolsUp, b);
    const s2 = shot(g, "C5", SHORT, S.press, S.toolsUp, () => S.checks, {
      bare: true,
      fade: 0,
      // The fresh prompt comes up from the dimmed title's level as the pane brightens.
      dim: () => lerp(PANE_DIM.title, 1, span(b, S.press, DUR.dim, "glide")),
      focus: () => dim,
    });
    ctx.restore();
    shot(g, "C6", pw.pane, S.toolsUp, Infinity, () => S.ghs, {
      bare: true,
      keep: true,
      fade: SWAP,
      dim: alphaOf,
      focus: () => dim,
    });
    const kc = keycapOn(SHORT, "ctrl-l");
    keycap(ctx, kc.x, kc.y, "ctrl-l", b, S.press);
    // `npm:` types into its slot in front of `prettier`, and the line shrinks into the pane.
    kinetic(ctx, b, c5, s1);
    // The shelf: the verdict, zoomed, the file chips ticking in turn, and
    // the footnote, leaving with the card.
    const shelfOut = (i: number) =>
      exit(b, S.leave + (i * DUR.stagger) / 2, OFF);
    const names = FILES.map(
      (_, i) => S.named[i] || ["README.md", ".github/workflows/ci.yml"][i],
    );
    let fx = LAYOUT.left.x;
    names.forEach((name, i) => {
      const f = S.files[i];
      const up = span(b, f.up, DUR.tick, "arrive");
      const o = shelfOut(i + 1);
      const a = up * o.alpha;
      const tickK = span(b, f.tick, DUR.tick, "arrive");
      // The chip at its own width, and the tick drawn on just after it, so
      // the chip never grows as prettier checks the file.
      const r = fileChip(ctx, name, fx, FILES_Y + o.dy, { alpha: 0 });
      if (a > 0) {
        ctx.save();
        const sc = lerp(0.9, 1, up);
        ctx.translate(r.x + r.w / 2, r.y + r.h / 2);
        ctx.scale(sc, sc);
        ctx.translate(-(r.x + r.w / 2), -(r.y + r.h / 2));
        fileChip(ctx, name, fx, FILES_Y + o.dy, { alpha: a });
        ctx.restore();
        ctx.save();
        ctx.globalAlpha *= o.alpha;
        tick(ctx, r.x + r.w + TICK_GAP + TICK / 2, r.y + r.h / 2, TICK, tickK);
        ctx.restore();
      }
      fx += r.w + TICK_GAP + TICK + FILE_GAP;
    });
    if (s2 && b >= S.zoom) verdictZoom(ctx, b, S.zoom, s2, shelfOut(0));
    footnote(
      ctx,
      "Runs on the project's `node`.",
      win(b, S.foot1[0], S.foot1[1]),
    );
    // The other backends, as text chips under the wide pane.
    let bx = LAYOUT.left.x;
    OTHERS.forEach((name, i) => {
      const p = span(b, S.chips[i], DUR.tick, "snap");
      const up = span(b, S.chips[i], DUR.tick, "arrive");
      // They leave as the pane narrows, the last as it starts.
      const o = exit(
        b,
        S.paneOut - (OTHERS.length - i) * DUR.stagger,
        DUR.exit,
      );
      const w = backendChip(ctx, name, bx, CHIPS_Y, 0);
      const a = up * o.alpha;
      if (a > 0) {
        ctx.save();
        const sc = lerp(0.9, 1, p);
        const cx = bx + w / 2;
        const cy = CHIPS_Y + 28;
        ctx.translate(cx, cy + o.dy);
        ctx.scale(sc, sc);
        ctx.translate(-cx, -cy);
        backendChip(ctx, name, bx, CHIPS_Y, a);
        ctx.restore();
      }
      bx += w + 16;
    });
    footnote(
      ctx,
      "Some backends need their ecosystem's tool, like `uv`.",
      win(b, S.foot2[0], S.foot2[1]),
    );
  },
  {
    // The terminal's window is the lit screen while it is up.
    lit: (b, d) => {
      const pw = paneAt(b, schedule(d));
      return pw.alpha > 0 ? termLit(rectOf(pw.pane), pw.alpha) : null;
    },
    events: (d) => schedule(d).p.events(),
  },
);

/**
 * The kinetic line (ART.md §12.10): `prettier` rises in, a slot opens left
 * of it as it makes room, `npm:` types into the slot in tools pink, the
 * outline goes solid and fades, and the whole word shrinks into its cells
 * in the pane's command row, where the take's line shows it typed.
 */
function kinetic(
  ctx: CanvasRenderingContext2D,
  b: number,
  c5: Capture | null,
  s: ReturnType<typeof shot>,
): void {
  if (b >= LAND) return;
  const cmd = typedOf(c5, "use");
  const word = cmd?.split(" ").pop() ?? FALLBACK_WORD;
  const colon = word.indexOf(":") + 1;
  const prefix = word.slice(0, colon);
  const name = word.slice(colon);
  const np = Array.from(prefix).length;
  const nn = Array.from(name).length;
  const trk = TYPE.big.tracking;
  // Opaque until it arrives on its cells, its colour turning to the terminal's text.
  const sh = shrinkAt(b, SHRINK, PALETTE.paper);
  const k = sh.k;
  // Where the word lands: its cells on the command row.
  const to = (cmd ? cellsOf(s, cmd, word) : null) ?? {
    x: KX,
    y: KY,
    size: SHORT.size,
  };
  const size = lerp(KSIZE, to.size, k);
  const adv = advance(size) + trk * size;
  const cx = lerp(KX, to.x, k);
  const y = lerp(KY, to.y, k);
  const all = (np + nn) * adv - trk * size;
  const left = cx - all / 2;
  const fade = sh.alpha;
  // Before the slot opens `prettier` stands centred alone; it makes room for the slot.
  const room = EASE.move(span(b, SLOT_AT, DUR.tick, "linear"));
  const aloneLeft = KX - (nn * adv - trk * size) / 2;
  const nameLeft =
    k > 0 ? left + np * adv : lerp(aloneLeft, left + np * adv, room);
  const inK = enter(b, PRETTIER_IN, DUR.tick);
  const partCentre = (x0: number, n: number) => x0 + (n * adv - trk * size) / 2;
  const toText = (from: string) => shrinkAt(b, SHRINK, from).color;
  bigline(
    ctx,
    name,
    y + inK.dy * (1 - k),
    inK.alpha * fade,
    size,
    toText(PALETTE.paper),
    partCentre(nameLeft, nn),
    {},
  );
  // The slot, npm:-wide, left of `prettier`: dashed as it opens, solid
  // once typed, then gone. Its right edge rides `prettier`'s left edge as
  // the word makes room, so the outline never cuts through it.
  const slotA =
    span(b, SLOT_AT, DUR.tick, "arrive") *
    (1 - span(b, SOLID + DUR.flick, DUR.tick, "glide"));
  if (slotA > 0) {
    const pad = 0.12 * KSIZE;
    slot(
      ctx,
      {
        x: left - pad,
        y: y - 0.8 * size - pad / 2,
        w: nameLeft - left + pad,
        h: size * 1.05 + pad,
      },
      slotA,
      span(b, SOLID, DUR.flick, "arrive"),
    );
  }
  // `npm:`, a key an eighth, in tools pink.
  Array.from(prefix).forEach((ch, i) => {
    const a = span(b, NPM_AT + i * NPM_EACH, DUR.stagger, "arrive");
    if (a <= 0) return;
    bigline(
      ctx,
      ch,
      y,
      a * fade,
      size,
      toText(PILLAR.tools),
      partCentre(left + i * adv, 1),
      {},
    );
  });
}

/** prettier's verdict, lifted out of its row into a panel across the stage (move 3/4), held, and gone with the shelf. */
function verdictZoom(
  ctx: CanvasRenderingContext2D,
  b: number,
  at: number,
  s: NonNullable<ReturnType<typeof shot>>,
  out: { alpha: number; dy: number },
): void {
  const row = findRow(s.screen, VERDICT);
  if (!row || out.alpha <= 0) return;
  const runs = s.screen.lines[row.line];
  const n = Array.from(row.text).length;
  const cell = s.layout.cell(row.line, 0);
  const k = span(b, at, DUR.zoom, "move");
  const size = lerp(s.pane.size, ZOOM_ART.size, k);
  const inset = lerp(16, ZOOM_ART.inset, k);
  const from: Rect = {
    x: cell.x - 16,
    y: cell.y,
    w: n * s.layout.advance + 32,
    h: cell.h,
  };
  const to: Rect = {
    x: LAYOUT.left.x,
    y: SHELF_Y,
    w: n * advance(ZOOM_ART.size) + 2 * ZOOM_ART.inset,
    h: ZOOM_H,
  };
  const r = lerpRect(from, to, k);
  const a = span(b, at, DUR.tick, "arrive") * out.alpha;
  ctx.save();
  ctx.translate(0, out.dy);
  raised(ctx, r, RADIUS.zoom, { shadow: SHADOW.float, alpha: a });
  ctx.globalAlpha *= a;
  drawTermLine(ctx, runs, r.x + inset, r.y + r.h / 2 + 0.36 * size, size);
  ctx.restore();
}

/** The score's cues (score/cues.ts), where the picture has them. */
export const cues: SceneCues<"backends"> = (facts: ReelFacts | null) => {
  const S = schedule(reelData(facts));
  return {
    npm: [0, 1, 2, 3].map((i) => NPM_AT + i * NPM_EACH + DUR.stagger),
    seat: S.seated,
    clear: S.press,
    // The bell: as the verdict lifts out of its row.
    verdict: S.zoom,
    chips: S.files.map((f) => f.tick),
    gh: S.gh,
    backends: S.chips,
  };
};

/** The section's schedule (kit/pace.ts Pace) and its takes' plays, for the pacing checks. */
export const pacing = (
  facts: ReelFacts | null,
): { pace: Pace; takes: Partial<Record<CaptureId, readonly Play[]>> } => {
  const S = schedule(reelData(facts));
  return { pace: S.p, takes: { C5: [...S.uses, ...S.checks], C6: S.ghs } };
};
