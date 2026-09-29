// vars (plan v3 §3, Act III; ART.md §12.12): the card (at [env]) and the
// terminal hold from the Environments ticket, and a shell-env strip over
// the terminal holds the value api's shell has (`APP_ENV=api`, as the
// pitch's `echo` printed it in the same directory); it comes up with the
// section's stage, still, before anything moves. Then one thing at a time
// (STORYBOARD.md "Pacing", kit/pace.ts): `cd ..` types (C9); the value
// folds off the strip; `echo APP_ENV=$APP_ENV` prints `APP_ENV=`, and
// caption 1 says why. `_.file = ".env"` seats in [env], which lights; the
// `.env` file card comes out of that row to its place under the card;
// `cd api` brings the values back onto the strip with the cwd hop (api's
// value and the `.env` file's `PORT=3000`, as mise loads them both there);
// `echo PORT=$PORT` prints `PORT=3000`, and caption 2 names `_.file`. Then
// the file card folds back into its row and the strip draws off, so the
// card ([env] lit) and the terminal's last screen hold into redact
// (kit/rest.ts vars|redact), which dissolves it into its prompt. While the
// terminal acts the card dims, and while the card does the terminal does
// (PACE.focusDim); everything is back up before the section's rest.
//
// Inferences, written down: the strip's values are the take's own lines,
// shown where the shell has them. api's `APP_ENV=api` is the one C2's
// `echo` printed in the same directory (C9 never echoes it there), and
// `PORT=3000` comes onto the strip on `cd api`, a beat before C9's `echo`
// prints it (a re-record echoing both would make them direct; the rig is
// unchanged this round).

import {
  type Capture,
  capture,
  fileOf,
  firstWith,
  fixture,
  type ReelData,
  reelData,
} from "../captures";
import {
  beatOf,
  card,
  cut,
  ENV_CARD,
  ENV_PANE,
  envStrip,
  greyScene,
  keep,
  type Play,
  rowMatch,
  shot,
  step,
  win,
} from "../kit/grey";
import { span } from "../kit/motion";
import { type FocusPlan, focusOf, Pace, unfocus } from "../kit/pace";
import { drawPaneWindow, OPENS } from "../kit/rest";
import { DUR, ENV_STRIP_ART, FILE_ART, LAYOUT, TYPE } from "../kit/style";
import { clamp } from "../math";
import type { SceneCues } from "../score/cues";
import type { SceneEvents } from "../storyboard";
import { bumpOver } from "../kit/diagrams/common";
import { fileBetween, outAndBack } from "./g3-versions-env/lift";
import { resultOf } from "./g3-versions-env/take";

/** The `.env` file card, under the card, one row tall (LAYOUT.envFile's top). */
const ENV_FILE = {
  x: LAYOUT.envFile.x,
  y: LAYOUT.envFile.y,
  w: LAYOUT.envFile.w,
  h: FILE_ART.tabH + 2 * FILE_ART.top + TYPE.file.lineH,
};

/** The file card's flights: out in its half-beat slot, back over 3/4 (DUR.std). */
const CARD_OUT = DUR.enter;
/** The strip draws off: its chips leave, an eighth later it shrinks away. */
const STRIP_OFF = DUR.flick + DUR.enter;

/** The take, when the set has it (a set of versions alone has no captures). */
const take = (d: ReelData | null): Capture | null =>
  d?.captures ? capture(d, "C9") : null;

/**
 * The section's schedule (kit/pace.ts): one focal motion at a time, in the
 * order STORYBOARD.md's event plan gives, each starting half a beat after
 * the last has stopped; C9's commands typed at TYPE_RATE, each output held
 * for its read. With no take, null: the captions keep their storyboard
 * anchors.
 */
function plan(d: ReelData | null, c: Capture) {
  const p = new Pace("vars", { d });
  const cdOut = p.term("cdOut", (at) => [step(c, "cd-out", at)], { lines: 0 });
  const strip = p.move("strip", DUR.fold);
  const envEmpty = p.term("envEmpty", (at) => [step(c, "env-empty", at)]);
  p.caption(0);
  const seatFile = p.move("seatFile", DUR.seatGap + DUR.seat);
  const envCard = p.move("envCard", CARD_OUT);
  const cdIn = p.term("cdIn", (at) => [step(c, "cd-in", at)], { lines: 0 });
  const port = p.term("port", (at) => [step(c, "port", at)]);
  p.caption(1);
  const envCardOut = p.move("envCardOut", DUR.std);
  const stripOut = p.move("stripOut", STRIP_OFF);
  const plays: Play[] = [
    cut(0, OPENS.C9!(c)),
    ...cdOut.plays,
    ...envEmpty.plays,
    ...cdIn.plays,
    ...port.plays,
  ];
  // Where each result really lands: the first frame after its Enter that
  // says something (the new prompt, the printed line).
  const land = (name: string) => beatOf(plays, resultOf(c, name));
  const focus: FocusPlan = [
    [cdOut.at, ["term", "strip"]],
    [seatFile.at, ["card", "file"]],
    [cdIn.at, ["term", "strip"]],
    [envCardOut.at, "*"],
  ];
  return {
    pace: p,
    plays,
    focus,
    events: p.events(),
    strip,
    seatFile,
    envCard,
    envCardOut,
    stripOut,
    at: {
      cdOut: land("cd-out"),
      empty: land("env-empty"),
      cdIn: land("cd-in"),
      port: land("port"),
    },
  };
}

type Plan = ReturnType<typeof plan>;
const PLANS = new WeakMap<Capture, Plan>();
/** The schedule for a take (a pure function of it and the facts' captions). */
export function schedule(d: ReelData | null): Plan | null {
  const c = take(d);
  if (!c) return null;
  let s = PLANS.get(c);
  if (!s) PLANS.set(c, (s = plan(d, c)));
  return s;
}

/** A take's row matching `re` once it has printed, as plain text (a chip's). */
function printedRow(c: Capture | null, re: RegExp): string | null {
  if (!c) return null;
  const t = firstWith(c, re);
  return t === null ? null : (rowMatch(c, t, re)?.[0] ?? null);
}

/** The value api's shell has: the pitch's own `echo APP_ENV=$APP_ENV` in ~/work/api (C2). */
const apiValue = (d: ReelData | null) =>
  printedRow(d?.captures ? capture(d, "C2") : null, /^APP_ENV=\S+$/);

/** Without the take: the storyboard's event plan (sections.json), which the missing card's layers follow. */
const FALLBACK = {
  strip: { at: 2.33, end: 2.83 },
  seatFile: { at: 8, end: 8.75 },
  envCard: { at: 9.25, end: 9.75 },
  envCardOut: { at: 16.25, end: 17 },
  stripOut: { at: 17.5, end: 18.125 },
  at: { cdOut: 1.4, empty: 5.67, cdIn: 11.24, port: 13.97 },
};

export const scene = greyScene(
  "vars",
  (g) => {
    const { ctx, b } = g;
    const c = take(g.d);
    const s = schedule(g.d);
    const P = s ?? FALLBACK;
    const { at } = P;
    const focus: FocusPlan = s?.focus ?? [];
    // The card: the file as C8 left it until `_.file` seats (C9's), [env]
    // lighting as it lands and lit into redact.
    const seatFrom = P.seatFile.at;
    const seatAt = P.seatFile.end;
    const seated = b >= seatFrom;
    const drop = clamp((b - seatFrom) / (seatAt - seatFrom));
    const seatMark =
      b < seatAt + DUR.seatHold
        ? 1
        : 1 - span(b, seatAt + DUR.seatHold, DUR.seatFade, "glide");
    // The row pulses as the file card folds back into it.
    const backAt = P.envCardOut.at;
    const pulse = win(b, backAt + DUR.std, backAt + DUR.std + DUR.half, 0.25);
    const rows = keep(g, () =>
      card(ctx, ENV_CARD, {
        title: "~/work/api/mise.toml",
        text: fileOf(g.d, seated ? "C9" : "C8", "work/api/mise.toml"),
        from: seated ? "C9" : "C8",
        open: ["[env]"],
        lit: { "[env]": span(b, seatAt, DUR.light, "arrive") },
        ignite: { "[env]": bumpOver(b, seatAt, DUR.ignite) },
        seat: seated
          ? [
              {
                re: /^_\.file = /,
                a: Math.max(seatMark, 0.8 * pulse),
                drop,
              },
            ]
          : [],
        alpha: focusOf(focus, "card", b),
      }),
    );
    const fileRow = rows.find((r) => /^_\.file = /.test(r.text));
    // The `.env` file card, out of its row and back into it.
    const env = fixture(g.d, "api/.env");
    if (fileRow && fileRow.top !== undefined) {
      // It comes out of (and goes back into) the row's end, clear of its text.
      const row = {
        // (the card at its smallest, a fifth of its width, just after the text)
        x: fileRow.x + (fileRow.w ?? 0) + TYPE.card.size / 2 + ENV_FILE.w / 10,
        y: fileRow.top,
        w: 0,
        h: fileRow.h ?? TYPE.card.lineH,
      };
      // Out from under the card's bottom edge, and back under it: it never
      // crosses the card's own rows or its fold pill.
      ctx.save();
      ctx.globalAlpha *= focusOf(focus, "file", b);
      fileBetween(
        ctx,
        row,
        ENV_FILE,
        outAndBack(b, P.envCard.at, backAt, { out: CARD_OUT }),
        {
          title: ".env",
          lines: env === null ? null : env.trim().split("\n"),
          from: "fixtures",
        },
        ENV_CARD,
      );
      ctx.restore();
    }
    // The terminal, from the ticket, its last screen held through the bar
    // line (kit/rest.ts HOLDS.C9), which redact dissolves into its prompt.
    const termDim = unfocus(focus, "term", b);
    keep(g, () => drawPaneWindow(ctx, ENV_PANE, termDim));
    shot(g, "C9", ENV_PANE, -1, Infinity, () => s?.plays ?? [], {
      bare: true,
      keep: true,
      focus: () => termDim,
    });
    // The shell-env strip over it: up with the section's stage (its edge
    // fade), and drawing off once the file card is home.
    const offAt = P.stripOut.at;
    const shrink = span(b, offAt + DUR.flick, DUR.enter, "leave");
    const sa = (1 - shrink) * focusOf(focus, "strip", b);
    if (sa > 0) {
      const R = LAYOUT.envStrip;
      const w = R.w * (1 - 0.3 * shrink);
      const app = apiValue(g.d);
      const port = printedRow(c, /^PORT=\S+$/);
      // api's value: folding off after `cd ..`, coming back on `cd api`
      // with the `.env` file's; both leave as the strip draws off.
      const leaveAll = (i: number) =>
        span(b, offAt + i * DUR.stagger, DUR.exit, "leave");
      const off = span(b, P.strip.at, DUR.fold, "leave");
      const back = span(b, at.cdIn, DUR.tick, "snap");
      const before = b < at.cdIn;
      const appK = before ? 1 - off : clamp(back);
      const appDy = before ? -ENV_STRIP_ART.flip * off : 6 * (1 - back);
      const chips = [
        ...(app
          ? [
              {
                text: app,
                alpha: appK * (1 - leaveAll(0)),
                dy: appDy - ENV_STRIP_ART.flip * leaveAll(0),
              },
            ]
          : []),
        ...(port
          ? [
              {
                text: port,
                alpha: before ? 0 : clamp(back) * (1 - leaveAll(1)),
                dy:
                  (before ? 0 : 6 * (1 - back)) -
                  ENV_STRIP_ART.flip * leaveAll(1),
              },
            ]
          : []),
      ];
      envStrip(ctx, { ...R, w }, chips, sa);
    }
  },
  { events: (d): SceneEvents | null => schedule(d)?.events ?? null },
);

/** The score's cues (score/cues.ts), where the picture has them. */
export const cues: SceneCues<"vars"> = (facts) => {
  const s = schedule(reelData(facts));
  if (!s) return {};
  return {
    cdOut: s.at.cdOut,
    empty: s.at.empty,
    seat: s.seatFile.end,
    cdIn: s.at.cdIn,
    port: s.at.port,
  };
};
