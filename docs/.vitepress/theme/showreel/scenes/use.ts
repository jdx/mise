// use (plan v3 §3, Act I; ART.md §7, §12.10; STORYBOARD.md "Pacing"):
// the stage the Dev tools ticket brought in, the api card and the short
// terminal (PANES.slip) at `~/work/api $`. One focal motion at a time,
// half a beat apart (kit/pace.ts Pace), whatever is not the focus dimmed:
// `jq .status resp.json` types at TYPE_RATE and is not found, and the
// line holds for its read. Then the command the take typed next, `mise
// use jq`, slams in as big type under the pane (whose text dims to half),
// holds at full size (rule 7), and shrinks into its own cells on the
// prompt row, where the take cuts to the command typed under it, handing
// over glyph for glyph (the slam stands in for the typing). A beat of
// anticipation, Enter, and its install rows move at 3× under the
// time-lapse badge and fade out while they still move, before any row
// finishes; the pane holds the frame the command was entered on, badged,
// for the time-lapse's 1.5 s. The terminal dims and `jq = "latest"` seats
// in the card (the rows below part for it, it drops in, its seat mark
// holds, [tools] bumps); the caption follows it. A real Ctrl-L: the ⌃L
// keycap is pressed on the key's time, the terminal brightens on the fresh
// prompt it cleared to, and `jq .status resp.json` prints "ok", held for
// its read. The terminal leaves; the card holds through both bar lines.

import { BEAT, PALETTE, type ReelFacts } from "../bible";
import {
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
  beatOf,
  bigline,
  card,
  chain,
  cut,
  greyScene,
  keep,
  keycap,
  keycapOn,
  type Play,
  rectOf,
  shot,
  step,
  TYPE_RATE,
} from "../kit/grey";
import { bump, exit, slam, span } from "../kit/motion";
import { focusOf, focusPlan, Pace, readsOf, unfocus } from "../kit/pace";
import { drawPaneWindow, OPENS } from "../kit/rest";
import { DUR, LAYOUT, PANE_DIM, PANES, TYPE } from "../kit/style";
import { type Pane, termLit } from "../kit/term";
import { lerp } from "../math";
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
const TITLE = "~/work/api/mise.toml";
/**
 * The terminal: short (six rows, every screen C3 shows), with the slam
 * under it, as backends has them (kit/rest.ts tools|use).
 */
const PANE: Pane = PANES.slip;

/** Kinetic type's whole motion: in, held at full size (rule 7), shrunk into its cells. */
const SLAM_DUR = DUR.slam + DUR.bigHold + DUR.bigShrink;
/** The ⌃L's motion: the keycap coming up, pressed, and let go. */
const CLEAR_DUR = DUR.keycapLead + DUR.keyDown + DUR.keyUp;
/** How far the pane's text dims while the slam holds (ART.md §12.10: to 50 %). */
const UNDER_SLAM = 0.5;

/** What the take typed to install jq (the slam's words), and the words without facts. */
const FALLBACK_COMMAND = "mise use jq";

/**
 * When everything happens (kit/pace.ts): the section's focal motions one at
 * a time from the take, the captions anchored on them, and the focus.
 * Without the take, the plan's own beats (sections.json).
 */
const schedule = perFacts((d: ReelData | null) => {
  const c = takeIn(d, "C3");
  const p = new Pace("use", { d });
  const nf = c
    ? p.term("notFound", (at) => [step(c, "not-found", at)], { lines: 1 })
    : { ...p.move("notFound", 2.68), plays: [] as Play[] };
  const sl = p.move("slam", SLAM_DUR);
  const install = c
    ? p.lapse(
        "install",
        (at) => enteredInstall(c, "use", at),
        (ps) => ps[1].at,
      )
    : { ...p.move("install", 1.875), plays: [] as Play[] };
  const seat = p.move("seat", DUR.seatGap + DUR.seat);
  p.caption(0);
  const clear = p.move("clear", CLEAR_DUR, { after: p.termReady });
  const press = clear.at + DUR.keycapLead;
  const ok = c
    ? p.term("ok", (at) => [step(c, "jq", at)], { lines: 1 })
    : { ...p.move("ok", 2.66), plays: [] as Play[] };
  p.readTerm();
  const leave = p.move("leave", DUR.exit);
  // The slam lands, holds, shrinks, and arrives on its cells, where the take cuts to the command typed.
  const land = sl.at + DUR.slam;
  const shrink = land + DUR.bigHold;
  const arrive = shrink + ARRIVE_BY * DUR.bigShrink;
  let first: Play[] = [];
  let second: Play[] = [];
  let notFound = nf.end;
  let okAt = ok.end;
  if (c) {
    const u = stepOf(c, "use");
    const word = typedOf(c, "use") ?? FALLBACK_COMMAND;
    const pre = wordFrame(c, "use", word);
    first = [
      cut(0, OPENS.C3!(c)),
      ...nf.plays,
      // What the take typed before the slammed word, at TYPE_RATE.
      ...(pre > u.type
        ? chain(arrive - (pre - u.type) / (BEAT * TYPE_RATE), [
            [u.type, pre, TYPE_RATE],
          ])
        : []),
      cut(arrive, typedFrame(c, "use")),
      ...install.plays,
    ];
    second = [cut(press, frameAfter(c, keyAt(c, "\f"))), ...ok.plays];
    const nfs = stepOf(c, "not-found");
    notFound = beatOf(
      first,
      firstWith(c, /command not found/, nfs.enter) ?? nfs.end,
    );
    const jq = stepOf(c, "jq");
    okAt = beatOf(second, firstWith(c, /^"ok"$/, jq.enter) ?? jq.end);
  }
  // The terminal has the focus while it acts, the card while jq seats and
  // the caption reads; both are lit again as the terminal leaves. The
  // terminal stays lit until each output has been read (rule 3).
  const focus = focusPlan(
    [
      [nf.at, "term"],
      [sl.at, "slam"],
      [install.at, "term"],
      [seat.at, "card"],
      [clear.at, "term"],
      [leave.at, "*"],
    ],
    readsOf(nf, ok),
  );
  return {
    p,
    first,
    second,
    focus,
    land,
    shrink,
    arrive,
    install,
    seat: seat.at,
    seated: seat.end,
    clear: clear.at,
    press,
    leave: leave.at,
    notFound,
    ok: okAt,
  };
});

/** The pane's presence as it leaves: its alpha and fall. */
const leaving = (b: number, d: ReelData | null) =>
  exit(b, schedule(d).leave, DUR.exit);

export const scene = greyScene(
  "use",
  (g) => {
    const { ctx, b } = g;
    const c = capture(g.d, "C3");
    const S = schedule(g.d);
    // The card: C2's file until jq seats, then C3's, the line dropping in;
    // dimmed while the terminal has the focus.
    const seating = b >= S.seat;
    keep(g, () =>
      card(ctx, CARD, {
        title: TITLE,
        text: fileOf(g.d, seating ? "C3" : "C2", "work/api/mise.toml"),
        from: seating ? "C3" : "C2",
        open: ["[tools]"],
        lit: { "[tools]": 0.6 * bump(b, S.seated, DUR.ignite) },
        seat: seating
          ? [
              {
                re: /^jq = /,
                a: 1 - span(b, S.seated + DUR.seatHold, DUR.seatFade, "glide"),
                drop: span(b, S.seat, DUR.seatGap + DUR.seat, "linear"),
              },
            ]
          : undefined,
        alpha: focusOf(S.focus, "card", b),
        // As tall as its rows, the seat's parting row growing it (kit/rest.ts tools|use).
        fit: true,
      }),
    );
    // The terminal: dimmed while the card has the focus (on the frame the
    // command was entered on, from the seat to the ⌃L), bright again as
    // the keycap comes up; then it leaves.
    const out = leaving(b, g.d);
    const dim = unfocus(S.focus, "term", b);
    ctx.save();
    ctx.translate(0, out.dy);
    ctx.globalAlpha *= out.alpha;
    keep(g, () => drawPaneWindow(ctx, PANE, dim));
    // While the slam holds the pane's text dims to half, and comes back as the command lands in it.
    const under =
      span(b, S.land - DUR.slam, DUR.slam, "wind") *
      (1 - span(b, S.shrink, DUR.bigShrink, "move"));
    const s = shot(g, "C3", PANE, -1, S.press, () => S.first, {
      bare: true,
      keep: true,
      fade: 0,
      dim: () => lerp(1, UNDER_SLAM, under),
      focus: () => dim,
      // The time-lapse badge holds with the entered command for its 1.5 s.
      tail: { badgeUntil: S.install.end },
    });
    shot(g, "C3", PANE, S.press, S.leave + DUR.exit, () => S.second, {
      bare: true,
      fade: 0,
      // The fresh prompt comes up from the dimmed title's level as the pane brightens.
      dim: () => lerp(PANE_DIM.title, 1, span(b, S.press, DUR.dim, "glide")),
    });
    ctx.restore();
    // `mise use jq` slams in under the pane, holds, and shrinks into its cells on the prompt row.
    const sl = slam(b, S.land);
    const ink = shrinkAt(b, S.shrink, PALETTE.paper);
    const k = ink.k;
    if (sl.alpha > 0 && b < S.shrink + DUR.bigShrink) {
      const text = typedOf(c, "use") ?? FALLBACK_COMMAND;
      const to = cellsOf(s, text, text) ?? {
        x: KINETIC.x,
        y: KINETIC.y,
        size: PANE.size,
      };
      bigline(
        ctx,
        text,
        lerp(KINETIC.y, to.y, k),
        sl.alpha * ink.alpha,
        lerp(TYPE.big.slam, to.size, k),
        ink.color,
        lerp(KINETIC.x, to.x, k),
        { scale: sl.scale, impact: sl.impact },
      );
    }
    const kc = keycapOn(PANE, "ctrl-l");
    keycap(ctx, kc.x, kc.y, "ctrl-l", b, S.press);
  },
  {
    // The terminal's window is the lit screen until it leaves.
    lit: (b, d) => {
      const out = leaving(b, d);
      return out.alpha > 0
        ? termLit({ ...rectOf(PANE), y: PANE.y + out.dy }, out.alpha)
        : null;
    },
    events: (d) => schedule(d).p.events(),
  },
);

/** The score's cues (score/cues.ts), where the picture has them. */
export const cues: SceneCues<"use"> = (facts: ReelFacts | null) => {
  const S = schedule(reelData(facts));
  return {
    notFound: S.notFound,
    slam: S.land,
    seat: S.seated,
    clear: S.press,
    ok: S.ok,
  };
};

/** The section's schedule (kit/pace.ts Pace) and its takes' plays, for the pacing checks. */
export const pacing = (
  facts: ReelFacts | null,
): { pace: Pace; takes: Partial<Record<CaptureId, readonly Play[]>> } => {
  const S = schedule(reelData(facts));
  return { pace: S.p, takes: { C3: [...S.first, ...S.second] } };
};
