// The landing-page films: editorial cuts of the source reel's sections,
// each played at its recorded reading pace on its own source clock. The
// source clock stays intact for the pacing, capture and sound-design checks;
// the delivered picture, effects and chapters use the film clock built
// here, and the music is arranged on it independently (film-music.ts).
//
// A shot is a whole section, or part of one where the source's own
// transition into a neighbour the film does not keep would otherwise play
// (the open's hand-over to the pitch's terminal, the switch's fold into
// packslip's card, the end card's long hold under the source bed's ending).
// Where the film puts two sections side by side that the source did not,
// film.ts joins them: a ticket takes down the stage of the section the film
// plays before it (bible.ts Joins), and any other cut leaves and enters on
// the kit's own exit and entrance (kit/motion.ts).
import { chaptersVtt, sec, type Chapter, type SectionId } from "./timeline";

export type Edition = "tour" | "overview";
export interface Cut {
  id: SectionId;
  /** Film seconds. */
  start: number;
  end: number;
  /** Source (reel) seconds. */
  from: number;
  to: number;
}

/** A whole section, or a span of one in its own local seconds. */
type Shot = SectionId | { id: SectionId; from?: number; to?: number };

/**
 * The open through the name card's hold and its leave (STORYBOARD.md Act
 * 0: `leave` 10.11–10.61, the chef an eighth behind the card), on the frame
 * grid, with a breath of empty stage before the next scene enters: the
 * pitch's terminal, which rises in at 11.11 in the source, belongs to the
 * pitch's own shot.
 */
export const OPEN_TO = 10.75;
/**
 * The switch to the end of its threads (STORYBOARD.md: `threads` 18.25–19,
 * then `fold` from 19.5, api/ growing into the card packslip carries on
 * with). The films play no packslip, so they cut before the fold; the
 * film's exit takes the diagram down instead (test/edit.test.ts holds this
 * to the storyboard's `fold`).
 */
export const SWITCH_TO = 19.5;
/**
 * The end card through its last move (+9 s, kit/namecard.ts END_CUES) and a
 * hold under the music's ending (film-music.ts), rather than the source's
 * 14 s under the bed's own.
 */
export const END_TO = 12;

const OPEN: Shot = { id: "open", to: OPEN_TO };
const SWITCH: Shot = { id: "switch", to: SWITCH_TO };
const END: Shot = { id: "end", to: END_TO };

const SHOTS: Record<Edition, readonly Shot[]> = {
  // The brand's open, project switching first, then the project workflow
  // and the acts in the source's order, the morph and the end card.
  tour: [
    OPEN,
    SWITCH,
    "pitch",
    "tools",
    "use",
    "registry",
    "env",
    "vars",
    "tasks",
    "depends",
    "skip",
    "dotfiles",
    "track",
    "machines",
    "lock",
    "new",
    "bootstrap",
    "breath",
    "clone",
    "morph",
    END,
  ],
  // The morph needs the clone's card before it; the overview goes straight
  // to the end card.
  overview: [OPEN, SWITCH, "use", "depends", END],
};

/** Map the shots onto film time, each at its source pace. */
export function cuts(edition: Edition): Cut[] {
  let at = 0;
  return SHOTS[edition].map((shot) => {
    const id = typeof shot === "string" ? shot : shot.id;
    const s = sec(id);
    const from = s.start + (typeof shot === "string" ? 0 : (shot.from ?? 0));
    const to =
      typeof shot === "string" || shot.to === undefined
        ? s.end
        : s.start + shot.to;
    const start = at;
    at += to - from;
    return { id, start, end: at, from, to };
  });
}

/**
 * Whether cut `b` carries straight on from cut `a` on the source clock:
 * the source's own bar line between them, which the handoff holds (no
 * join, no exit or entrance).
 */
export const continuous = (a: Cut, b: Cut): boolean => a.to === b.from;

/** Runtime of the film, in film seconds. */
export const filmDuration = (edition: Edition) => cuts(edition).at(-1)!.end;
/** Locate a film shot; hold the final shot after the film ends. */
export function cutAt(edition: Edition, time: number): Cut {
  const list = cuts(edition);
  return list.find((cut) => time < cut.end) ?? list[list.length - 1];
}

/**
 * The player's chapter labels, by act: the acts' own names (timeline.ts
 * ACTS, the tickets' titles), but for the switching chapter, which the
 * source has no act for, and the end card.
 */
const LABELS = {
  mise: "mise",
  install: "Install mise",
  versions: "Project switching",
  "dev-tools": "Dev tools",
  environments: "Environments",
  tasks: "Tasks",
  dotfiles: "Dotfiles",
  everywhere: "Everywhere",
  "new-machine": "A new machine",
} as const;

/** Group the cuts into contiguous chapters, one per act the film visits. */
export function filmChapters(edition: Edition): Chapter[] {
  const chapters: Chapter[] = [];
  for (const cut of cuts(edition)) {
    const id = sec(cut.id).act;
    const last = chapters.at(-1);
    if (last?.id === id) {
      last.end = cut.end;
      last.sections.push(cut.id);
    } else {
      chapters.push({
        id,
        numeral: "",
        label: LABELS[id],
        start: cut.start,
        end: cut.end,
        sections: [cut.id],
      });
    }
  }
  // The opening configuration demo belongs with project switching, rather
  // than introducing a second chapter called Overview.
  const pitch = chapters.findIndex((c) => c.sections.includes("pitch"));
  if (pitch > 0) {
    chapters[pitch - 1].end = chapters[pitch].end;
    chapters[pitch - 1].sections.push(...chapters[pitch].sections);
    chapters.splice(pitch, 1);
  }
  return chapters;
}

/** WebVTT chapter track on the selected edition's delivered clock. */
export const filmChaptersVtt = (edition: Edition = "tour") =>
  chaptersVtt(filmChapters(edition));
