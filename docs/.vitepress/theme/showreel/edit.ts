// Editorial cuts share the recorded scenes at their original reading pace.
// The source clock stays intact for scene/sound checks; the delivered films
// have their own clock. Picture, effects and chapters use this same edit;
// music is arranged independently in film-music.ts.
import { chaptersVtt, sec, type Chapter, type SectionId } from "./timeline.ts";

export type Edition = "tour" | "overview";
export interface Cut {
  id: SectionId;
  start: number;
  end: number;
  from: number;
  to: number;
  brand?: "intro" | "outro";
}

type Shot =
  | SectionId
  | { id: SectionId; from: number; to: number; brand: "intro" | "outro" };
const INTRO: Shot = { id: "open", from: 0, to: 6, brand: "intro" };
// Replace the extended source ending with a concise install card.
const OUTRO: Shot = { id: "end", from: 6, to: 14, brand: "outro" };
const SHOTS: Record<Edition, readonly Shot[]> = {
  tour: [
    INTRO,
    "switch",
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
    OUTRO,
  ],
  overview: [INTRO, "switch", "use", "depends", OUTRO],
};

export function cuts(edition: Edition): Cut[] {
  let at = 0;
  return SHOTS[edition].map((shot) => {
    const id = typeof shot === "string" ? shot : shot.id;
    const s = sec(id);
    const from = s.start + (typeof shot === "string" ? 0 : shot.from);
    const to = typeof shot === "string" ? s.end : s.start + shot.to;
    const start = at;
    at += to - from;
    return {
      id,
      start,
      end: at,
      from,
      to,
      ...(typeof shot === "string" ? {} : { brand: shot.brand }),
    };
  });
}

export const filmDuration = (edition: Edition) => cuts(edition).at(-1)!.end;
export function cutAt(edition: Edition, time: number): Cut {
  const list = cuts(edition);
  return list.find((cut) => time < cut.end) ?? list[list.length - 1];
}

export function filmChapters(edition: Edition): Chapter[] {
  const chapters: Chapter[] = [];
  for (const cut of cuts(edition)) {
    const s = sec(cut.id);
    const id =
      cut.brand === "intro"
        ? "mise"
        : cut.brand === "outro"
          ? "install"
          : s.act;
    const last = chapters.at(-1);
    if (last?.id === id) {
      last.end = cut.end;
      if (!cut.brand) last.sections.push(cut.id);
    } else {
      const labels = {
        mise: "Overview",
        install: "Install mise",
        versions: "Project switching",
        "dev-tools": "Dev tools",
        environments: "Environments",
        tasks: "Tasks",
        dotfiles: "Dotfiles",
        everywhere: "In CI",
        "new-machine": "A new machine",
      };
      chapters.push({
        id,
        numeral: "",
        label: labels[id],
        start: cut.start,
        end: cut.end,
        sections: cut.brand ? [] : [cut.id],
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

export const filmChaptersVtt = (edition: Edition = "tour") =>
  chaptersVtt(filmChapters(edition));
