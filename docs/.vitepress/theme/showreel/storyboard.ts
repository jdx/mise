// The storyboard as data: sections.json, written by plan v3's timing checker
// (board5.py), retimed to 60 BPM by the pacing planner (build/retime
// plan.ts, on kit/pace.ts), and the source of STORYBOARD.md. timeline.ts
// keeps its own table (it imports nothing), and test/storyboard.test.ts
// holds the two equal. The captions come from here verbatim, with the
// version tokens (`{node.lts_major}` and the rest) filled from the capture
// run's versions file, so no scene or caption writes a version number;
// each is anchored on the scene event it explains (`after`, PACE rule 5),
// and each section carries its event plan (`plan`).

import { BEAT, type SectionId, sec } from "./bible";
import { fill, type ReelData, tokens } from "./captures";
import BOARD from "./sections.json";
import { PACE } from "./kit/style";
import {
  type Caption,
  entrance,
  readingTime,
  settledTime,
  wordCount,
} from "./type";

export interface BoardCaption {
  /**
   * The anchor, global seconds: the caption starts to write in here, unless
   * the scene reports its `after` event (captionTimes).
   */
  at: number;
  lines: string[];
  /**
   * The scene event (kit/pace.ts Pace, greyScene's `events`) the caption
   * explains: it starts to write in PACE.gap after that event is on screen
   * and still (PACE rule 5), wherever the take puts it. Without the event,
   * on `at`.
   */
  after?: string;
  /**
   * How long it holds (captionsFor). Absent: the storyboard's rule, a
   * section's last caption holding until two beats before the section ends
   * (or until read, where that is later) and an earlier one until the next
   * caption's anchor when less than PACE.captionEvery would be left bare
   * between them. A number (global seconds): it
   * holds until then instead (at least its reading time, at most one beat
   * before the section ends). "read": it leaves once read, for a section
   * that ends on a deliberately uncaptioned hold.
   */
  until?: number | "read";
}

/** One section's event plan (STORYBOARD.md "Event plans"): what happens, in order, on the section's beats. */
export interface BoardEvent {
  /** Section beats: when it starts moving, and when it is still. */
  at: number;
  end: number;
  /** The event's name: a caption's `after`, a score cue, or the scene's own. */
  name: string;
  what: string;
}

export interface BoardSection {
  id: string;
  act: string;
  actLabel: string;
  beats: number;
  start: number;
  end: number;
  captions: BoardCaption[];
  captures: string[];
  visual: string;
  /** The event plan the scene follows (kit/pace.ts builds it). */
  plan?: BoardEvent[];
}

export const STORYBOARD: readonly BoardSection[] = BOARD as BoardSection[];

/** A section's storyboard entry. */
export function boardOf(id: SectionId): BoardSection {
  const b = STORYBOARD.find((x) => x.id === id);
  if (!b) throw new Error(`no storyboard entry for "${id}"`);
  return b;
}

/**
 * The second bootstrap caption without systemd on machine 2, where there is
 * no watcher to start (plan v3 §7).
 */
const PLAIN_BOOTSTRAP = "Then it installs your global tools.";

/** Up to the next eighth, quarter and half beat: the grids captions start, land and leave on. */
const eighth = (b: number) => Math.ceil(b * 8 - 1e-6) / 8;
const quarter = (b: number) => Math.ceil(b * 4 - 1e-6) / 4;
const half = (b: number) => Math.ceil(b * 2 - 1e-6) / 2;
/** Down to the half beat at or before `b`. */
const halfDown = (b: number) => Math.floor(b * 2 + 1e-6) / 2;

/**
 * A gap between two captions shorter than this many beats is not left
 * bare: the first holds to the next, so the two make one caption change
 * (PACE rule 5: at most one in PACE.captionEvery).
 */
const BARE = PACE.captionEvery / BEAT;

/** A scene's events, section beats (kit/pace.ts Pace.events): when each is on screen and still. */
export type SceneEvents = Readonly<Record<string, number | null | undefined>>;

/** One caption on its section's beats, as read (before it holds on). */
export interface CaptionTimes {
  /** Its anchor: its first word may start to rise from here. */
  at: number;
  /** Its first word starts to rise: the anchor, or up to a quarter beat after it, where the first line's landing falls on the grid. */
  start: number;
  lines: { in: number; text: string }[];
  /** Its last line has landed: the caption is whole. */
  landed: number;
  /** The half beat it may leave on, once read (PACE rule 5). */
  minOut: number;
}

/**
 * A section's captions as they are read, on its beats. Each starts to
 * write in on its anchor: PACE.gap after its `after` event when the scene
 * reports it (`events`), else its storyboard time; and never sooner than
 * PACE.captionEvery after the caption before it started, nor before that
 * one has been read (PACE rule 5). The first line's words rise one per
 * 1/32 note from the anchor, the second line's once the first has landed.
 * It may leave, on the half beat, once it has held the reading time of all
 * its words from its anchor (type.ts readingTime) and a third of a second a
 * word from the moment its last line landed (settledTime), and it is never
 * up for less than PACE.captionEvery.
 */
export function captionTimes(
  id: SectionId,
  d: ReelData | null,
  events?: SceneEvents | null,
): CaptionTimes[] {
  const s = sec(id);
  const tok = tokens(d);
  const every = PACE.captionEvery / BEAT;
  let prev: CaptionTimes | null = null;
  return boardOf(id).captions.map((c, k) => {
    let texts = c.lines.map((l) => fill(l, tok));
    if (id === "bootstrap" && k === 1 && d?.variant === "plain")
      texts = [PLAIN_BOOTSTRAP];
    const event = c.after !== undefined ? events?.[c.after] : undefined;
    let at =
      typeof event === "number" && Number.isFinite(event)
        ? eighth(event + PACE.gap)
        : Math.round(((c.at - s.start) / BEAT) * 8) / 8;
    if (prev) at = Math.max(at, eighth(prev.start + every), prev.minOut);
    let land = at;
    const lines = texts.map((text) => {
      // Words the line rises in: entrance() counts them as drawWords does.
      const rise = -entrance(text, 0) / BEAT;
      land = quarter(land + rise);
      return { in: land, text };
    });
    const total = texts.reduce((n, l) => n + wordCount(l), 0);
    const landed = Math.max(...lines.map((l) => l.in));
    // Its first word rises once the first line's landing is on the grid.
    const start = entrance(lines[0].text, lines[0].in * BEAT) / BEAT;
    // Read, and up long enough that its leaving is not a second caption
    // change inside PACE.captionEvery of its arrival.
    const hold = Math.max(
      start + readingTime(total) / BEAT,
      landed + settledTime(total) / BEAT,
      start + every,
    );
    prev = { at, start, lines, landed, minOut: half(hold) };
    return prev;
  });
}

/**
 * A section's captions on its own beats (captionTimes), each wiping away on
 * the half beat once read. Then it holds on (BoardCaption `until`): a
 * section's last caption until two beats before the section ends, so its
 * payoff is never a bare, top-heavy frame and its wipe is over before the
 * closing rest (PACE rule 6), or, where its reading time runs later, until
 * it has been read (never past one beat before the end); an earlier one
 * until the next caption's anchor when less than PACE.captionEvery would
 * be bare between them. test/captions.test.ts checks the reading rules.
 */
export function captionsFor(
  id: SectionId,
  d: ReelData | null,
  events?: SceneEvents | null,
): Caption[] {
  const s = sec(id);
  const board = boardOf(id).captions;
  const read = captionTimes(id, d, events);
  const latest = s.beats - 1;
  return read.map((c, k) => {
    const until = board[k].until;
    let out = c.minOut;
    if (until === "read") return { out, lines: c.lines };
    if (typeof until === "number") out = halfDown((until - s.start) / BEAT);
    else if (k === read.length - 1) out = s.beats - 2;
    else {
      const next = read[k + 1];
      const lands = Math.min(...next.lines.map((l) => l.in));
      const room = halfDown(Math.min(next.at, lands - 0.5));
      if (room - c.minOut < BARE) out = room;
    }
    return { out: Math.max(c.minOut, Math.min(out, latest)), lines: c.lines };
  });
}
