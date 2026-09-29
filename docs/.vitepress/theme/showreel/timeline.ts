// from jdx/hk@37937824 docs/.vitepress/theme/showreel/timeline.ts
// The reel's clock: the tempo, the acts, and the timeline of sections. It
// imports nothing, so the landing page can read the chapters without the
// drawing code. bible.ts re-exports all of it.
//
// Adapted for mise: 60 BPM, sections counted in beats (whole 4/4 bars, and
// a closing 2/4 bar where a whole bar would add dead time, or one 2/4 bar
// for a ticket), and one chapter per act rather than per section.

/**
 * 60 BPM makes a beat 1 s and a 4/4 bar 4 s (the retime of 2026-09-28: the
 * 3:26 cut at 75 BPM went "WAY too fast"; STORYBOARD.md "Pacing"). Every
 * bar line and beat lands on a frame at 30, 60 and 120 fps, and so does
 * every eighth (0.5 s); a sixteenth (0.25 s) lands on one at 60 and 120,
 * and a 32nd (0.125 s) only at 120. Times are computed as
 * `beats * 60 / BPM`, one correctly rounded division, so a bar line and the
 * frame `i / fps` on it are the same double.
 */
export const BPM = 60;
export const beat = (n: number): number => (n * 60) / BPM;
export const bar = (n: number): number => beat(n * 4);
export const BEAT = beat(1);
export const BAR = bar(1);
/** A ticket: the single 2/4 bar that opens an act (2 s). */
export const TICKET_BEATS = 2;

/**
 * The acts, in order. Each is one chapter in the player's menu and the
 * page's chapter list, so a label never carries a number (a test checks),
 * and the chapters track stays true when sections move. `pillar` is the
 * colour the act's ticket and accents take (bible.ts PILLAR), or null for
 * the brand's own acts at either end.
 */
export const ACTS = [
  { id: "mise", numeral: "0", label: "mise", pillar: null },
  { id: "dev-tools", numeral: "I", label: "Dev tools", pillar: "tools" },
  { id: "versions", numeral: "II", label: "Versions", pillar: "tools" },
  { id: "environments", numeral: "III", label: "Environments", pillar: "env" },
  { id: "tasks", numeral: "IV", label: "Tasks", pillar: "tasks" },
  { id: "dotfiles", numeral: "V", label: "Dotfiles", pillar: "machine" },
  { id: "everywhere", numeral: "VI", label: "Everywhere", pillar: "machine" },
  {
    id: "new-machine",
    numeral: "VII",
    label: "A new machine",
    pillar: "machine",
  },
  { id: "install", numeral: "VIII", label: "Install mise", pillar: null },
] as const satisfies readonly {
  id: string;
  numeral: string;
  label: string;
  pillar: string | null;
}[];

export type ActId = (typeof ACTS)[number]["id"];
export type Act = (typeof ACTS)[number];
export type Pillar = NonNullable<Act["pillar"]>;

/**
 * The timeline: every section in order, in beats. Every section is whole
 * 4/4 bars, or whole bars and a closing 2/4 bar where a whole bar would only
 * add dead time; a ticket is exactly one 2/4 bar (timeline.test.ts holds
 * both). The reel's length, the chapters, each scene's span, and where the
 * score places its cues all come from here, so lengthening or inserting a
 * section moves everything after it. Labels name the section in the page's
 * screen-reader list, nested under its act.
 *
 * The retime of 2026-09-28 (STORYBOARD.md "Pacing"): 27 sections, 436
 * beats at 60 BPM, 7:16, each section as long as its event plan needs
 * (sections.json `plan`, from kit/pace.ts Pace) with its rest; `morph` 8
 * beats and `end` 14, exactly the sung line, the rest and the button with
 * the ring-out clear of the last beat (jdx's decision 5). sections.json is
 * the source; this file imports nothing, so it keeps its own copy, and
 * test/storyboard.test.ts holds the two equal.
 */
export const SECTIONS = [
  { id: "open", act: "mise", label: "The mise command", beats: 14 },
  { id: "pitch", act: "mise", label: "One mise.toml per project", beats: 28 },
  { id: "tools", act: "dev-tools", label: "Dev tools", beats: 2, ticket: true },
  { id: "use", act: "dev-tools", label: "mise use", beats: 20 },
  { id: "registry", act: "dev-tools", label: "Tools by short name", beats: 12 },
  { id: "backends", act: "dev-tools", label: "Backends", beats: 34 },
  {
    id: "versions",
    act: "versions",
    label: "Versions",
    beats: 2,
    ticket: true,
  },
  { id: "switch", act: "versions", label: "Versions per project", beats: 22 },
  { id: "packslip", act: "versions", label: "Packslip releases", beats: 36 },
  {
    id: "env",
    act: "environments",
    label: "Environments",
    beats: 2,
    ticket: true,
  },
  {
    id: "vars",
    act: "environments",
    label: "Env vars and .env files",
    beats: 20,
  },
  { id: "redact", act: "environments", label: "Redacted output", beats: 18 },
  { id: "tasks", act: "tasks", label: "Tasks", beats: 2, ticket: true },
  { id: "depends", act: "tasks", label: "Task dependencies", beats: 20 },
  {
    id: "skip",
    act: "tasks",
    label: "Up-to-date tasks are skipped",
    beats: 16,
  },
  { id: "args", act: "tasks", label: "Task arguments", beats: 22 },
  { id: "daemons", act: "tasks", label: "Daemons for tasks", beats: 18 },
  {
    id: "dotfiles",
    act: "dotfiles",
    label: "Dotfiles",
    beats: 2,
    ticket: true,
  },
  { id: "track", act: "dotfiles", label: "Dotfile history", beats: 38 },
  {
    id: "machines",
    act: "everywhere",
    label: "Everywhere",
    beats: 2,
    ticket: true,
  },
  { id: "lock", act: "everywhere", label: "mise.lock", beats: 22 },
  {
    id: "new",
    act: "new-machine",
    label: "A new machine",
    beats: 2,
    ticket: true,
  },
  { id: "bootstrap", act: "new-machine", label: "mise bootstrap", beats: 24 },
  { id: "breath", act: "new-machine", label: "A fresh shell", beats: 6 },
  { id: "clone", act: "new-machine", label: "A fresh clone", beats: 30 },
  { id: "morph", act: "install", label: "The chef", beats: 8 },
  { id: "end", act: "install", label: "Install mise", beats: 14 },
] as const satisfies readonly {
  id: string;
  act: ActId;
  label: string;
  beats: number;
  ticket?: true;
}[];

export type SectionId = (typeof SECTIONS)[number]["id"];

/** One section on the reel's clock. Times are global seconds. */
export interface Section {
  id: SectionId;
  act: ActId;
  label: string;
  /** Length in beats: a multiple of 4 (or of 2, ending on a 2/4 bar), or TICKET_BEATS for a ticket. */
  beats: number;
  /** The 2/4 bar that opens an act. */
  ticket: boolean;
  /** The section's place in its act: 0 for the act's first. */
  index: number;
  start: number;
  /** The frame at `end` belongs to the next section. */
  end: number;
  /** Length in seconds. */
  len: number;
  /** Global time of local time `lt`, seconds into the section. */
  at(lt: number): number;
  /** Global time of beat `n` of the section; beat 0 is its first downbeat. */
  beat(n: number): number;
  /** Global time of bar `n` of the section (4/4 bars from its first downbeat). */
  bar(n: number): number;
}

const TIMELINE = new Map<SectionId, Section>();
{
  // Counted in whole beats from the top, so every boundary is exact.
  let first = 0;
  let index = 0;
  let act: ActId | null = null;
  for (const s of SECTIONS) {
    const b0 = first;
    index = s.act === act ? index + 1 : 0;
    act = s.act;
    TIMELINE.set(s.id, {
      id: s.id,
      act: s.act,
      label: s.label,
      beats: s.beats,
      ticket: "ticket" in s && s.ticket,
      index,
      start: beat(b0),
      end: beat(b0 + s.beats),
      len: beat(s.beats),
      at: (lt) => beat(b0) + lt,
      beat: (n) => beat(b0 + n),
      bar: (n) => beat(b0 + 4 * n),
    });
    first += s.beats;
  }
}

/** Where section `id` sits on the reel's clock. */
export function sec(id: SectionId): Section {
  const s = TIMELINE.get(id);
  if (!s) throw new Error(`no section "${id}"`);
  return s;
}

/** The whole reel's length in beats. */
export const BEATS = SECTIONS.reduce((n, s) => n + s.beats, 0);

/** The whole reel: every section, end to end. */
export const DURATION = beat(BEATS);

/** One chapter per act, for players and the page's chapter list, with the sections it holds. */
export interface Chapter {
  id: ActId;
  numeral: string;
  label: string;
  start: number;
  end: number;
  sections: SectionId[];
}

export const CHAPTERS: Chapter[] = ACTS.map(({ id, numeral, label }) => {
  const sections = SECTIONS.filter((s) => s.act === id).map((s) => s.id);
  if (!sections.length) throw new Error(`act "${id}" has no sections`);
  return {
    id,
    numeral,
    label,
    start: sec(sections[0]).start,
    end: sec(sections[sections.length - 1]).end,
    sections,
  };
});

/** The act a section belongs to. */
export function actOf(id: SectionId): Act {
  const a = ACTS.find((x) => x.id === sec(id).act);
  if (!a) throw new Error(`no act for "${id}"`);
  return a;
}

/** `hh:mm:ss.fff`, a WebVTT timestamp. Section bounds are whole milliseconds. */
function vttTime(s: number): string {
  const ms = Math.round(s * 1000);
  const pad = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${pad(Math.floor(ms / 3_600_000))}:${pad(Math.floor(ms / 60_000) % 60)}:${pad(Math.floor(ms / 1000) % 60)}.${pad(ms % 1000, 3)}`;
}

/**
 * The chapters as a WebVTT chapters track, one cue per act, identified by
 * its id. docs/public/showreel-chapters.vtt is this text; a test keeps it
 * current.
 */
export function chaptersVtt(): string {
  const cues = CHAPTERS.map(
    (c) => `${c.id}\n${vttTime(c.start)} --> ${vttTime(c.end)}\n${c.label}\n`,
  );
  return ["WEBVTT\n", ...cues].join("\n");
}
