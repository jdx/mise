// The score's cue API: when the picture's events happen, so the sound
// lands where the picture does.
//
// Each section's score module listens for the named events below. A scene
// module (scenes/<id>.ts) tells it when they happen by exporting `cues`:
//
//   import type { SceneCues } from "../score/cues";
//   export const cues: SceneCues<"use"> = { clear: CLEAR, seat: SEAT };
//
// or, when the times come from a take, a function of the facts the scene
// draws with (the same facts the score is given):
//
//   export const cues: SceneCues<"use"> = (facts) => {
//     const c = capture(reelData(facts), "C3");
//     return { clear: CLEAR, ok: c ? beatOf(plays(c), okAt(c)) : null };
//   };
//
// Every time is in the section's own beats (beat 0 is its first
// downbeat; 0.25 is a sixteenth), the scene's `b`. A list is the same
// event more than once (each line a lane prints, each `linked` row); null
// says the event does not happen in this take (no Watcher tile without
// systemd). A name the scene leaves out keeps the fallback here: the
// kit's own constant where the event is the kit's (a ticket's tear, the
// morph's landings), else the beat the scene drew it on when the score was
// written, or the storyboard's caption anchor or grid beat
// (sections.json), so the score is whole before a scene exports anything.
// A fallback of null waits for the scene: its time is the take's, and a
// guess would sound where nothing happens. SceneCues is typed from this table, so a misspelt name or a
// section the score does not listen to fails `aube run typecheck:showreel`,
// and test/score.test.ts checks every exported time sits inside its
// section.

import type { ReelFacts } from "../bible";
import { reelData } from "../captures";
import { MORPH_CUE } from "../kit/chef";
import { TICKET } from "../kit/style";
import type { Section, SectionId } from "../timeline";

/** A cue: section beats, several times, or null for an event this take does not have. */
export type CueValue = number | readonly number[] | null;

/** A fallback: a cue, or one that depends on the facts (the take's variant). */
type Fallback = CueValue | ((facts: ReelFacts | null) => CueValue);

/** A ticket's two beats (ART.md §10, kit/style.ts TICKET.cue). */
const TICKET_CUES = {
  /** The feed starts: twelve printer steps to the tear. */
  print: 0,
  /** The tear: the ticket drops and hangs. */
  tear: TICKET.cue.tear,
  /** It lifts off the rail. */
  lift: TICKET.cue.liftAt,
} as const;

/**
 * Every event the score listens for, section by section, with its
 * fallback in section beats. The comment on each is what the sound marks.
 */
export const LISTEN = {
  open: {
    /** The block chef's cells lift off the help screen: tiny bells glint up to it. */
    lift: 3.75,
    /** The vector chef resolves and the name card lands: the service bell. */
    resolve: 5.25,
    /**
     * "mise", "-en", "-place" write on (scenes/g1-brand/open-card.ts
     * NAME_AT): C D♭ C on the celesta, as the end card writes it on again.
     */
    name: [5.75, 6, 6.25],
    /**
     * The name card and the chef leave. No sound of its own: an air under
     * it sat 12 to 18 dB below the synthesized band that carried the exit.
     * Listened for so the scene's export stays typed.
     */
    leave: 10,
  },
  pitch: {
    /** [tools], [env] and [tasks] light in turn. */
    tables: [1.5, 1.75, 2],
    /** `cd api` runs: the cwd dot hops (a tick; the switch's `cd`s are pitched). */
    cd: 5.75,
    /** `mise run ci` prints its tasks' lines. */
    ci: 18.9375,
    /** `v…` and `APP_ENV=api` land on the card rows that asked for them. */
    lands: [10.0625, 14.5625],
  },
  tools: TICKET_CUES,
  use: {
    /** `zsh: command not found: jq`. */
    notFound: 2.75,
    /** `mise use jq` slams in as big type. */
    slam: 3.875,
    /** `jq = "latest"` seats in the card. */
    seat: 9.25,
    /** The ⌃L keycap is pressed. */
    clear: 12.5,
    /** `jq .status resp.json` prints `"ok"`. */
    ok: 15.625,
  },
  registry: {
    /** `node` and `terraform` light as the caption names them. */
    lit: [2.5, 3],
  },
  backends: {
    /** `n`, `p`, `m`, `:` type into the slot in front of `prettier`. */
    npm: [2.563, 2.688, 2.813, 2.938],
    /** `"npm:prettier" = "latest"` seats. */
    seat: 8.11,
    /** ⌃L. */
    clear: 11,
    /** prettier's verdict prints and zooms: a pass. */
    verdict: 16.399,
    /** The file chips tick. */
    chips: [17.899, 18.149],
    /** `mise use github:cli/cli` prints its tools line. */
    gh: 24.101,
    /** The `pypi:`, `cargo:` and `go:` chips land. */
    backends: [27.079, 27.329, 27.579],
  },
  versions: TICKET_CUES,
  switch: {
    /** `cd ../dashboard` runs: a marimba run up the bed's chord there. */
    cdDashboard: 2.3125,
    /** dashboard's `node --version` prints. */
    dashboard: 6.1875,
    /** `cd ../api` runs: a marimba run up the bed's chord there. */
    cdApi: 11.5,
    /** api's `node --version` prints. */
    api: 15.375,
  },
  packslip: {
    /** `hk = "…"` lights in [tools]. */
    light: 1.25,
    /** The slip slides up under the terminal. */
    slip: 2.5,
    /** Tab on `hk check --j`: the old version's list. */
    tab: 7.247,
    /** The card flips to the new version and the slip re-stamps. */
    flip: 11.976,
    /** Tab again: the new list, `--junit-xml` lit. */
    junit: 16.821,
    /** `mise skills sync` prints its `linked` rows. */
    linked: 23.446,
    /** The drawn link lights the new version: the celesta's G G G F. */
    link: 25.361,
  },
  env: TICKET_CUES,
  vars: {
    /** `cd ..` runs. */
    cdOut: 1.375,
    /** `APP_ENV=` prints empty: the env's values fold off. */
    empty: 5.6875,
    /** `_.file = ".env"` seats. */
    seat: 8.75,
    /** `cd api` runs. */
    cdIn: 11.25,
    /** `PORT=3000` prints: the values come back. */
    port: 14,
  },
  redact: {
    /** The `_.file` line lifts out of the card. */
    zoom: 1,
    /** It is rewritten in place. */
    rewrite: 2.25,
    /** The file cards arrive: `.env.deploy`, then the deploy script. */
    cards: [4.5, 4.75],
    /** The run prints its line with `[redacted]`. */
    redacted: 8.5,
    /**
     * The stamp lands round `[redacted]`: its approach's end, not its start
     * (kit/parts.ts STAMP_LAND), where the thunk's weight is.
     */
    stamp: 9.9375,
    /** The file cards fold back into the card. */
    fold: 13.5,
  },
  tasks: TICKET_CUES,
  depends: {
    /** Each lane appears as its first line prints, in the take's order. */
    build: 5.146,
    lint: 5.271,
    test: 5.021,
    /** ci's lane: the dependencies resolve. */
    ci: 5.521,
    /** The lanes settle into [tasks], which lights. */
    settle: 16.713,
  },
  skip: {
    /** `sources` and `outputs` seat on the build task. */
    seat: 1,
    /** `[build] sources up-to-date, skipping` prints. */
    skipped: 3.08,
    /** The lanes refill, build's greyed. */
    build: 5.774,
    test: 5.899,
    lint: 6.024,
    ci: 6.274,
    /** The "skipped" stamp lands on the build lane (its approach's end, STAMP_LAND). */
    stamp: 7.649,
  },
  args: {
    /** The `#USAGE` lines light on the deploy card. */
    usage: 1.875,
    /** `--help` prints. */
    help: 5.317,
    /** `mise ERROR` prints for `prod`: the impact. */
    error: 12.291,
    /** The thread ties `prod` to the card's `choices`. */
    tie: 13.891,
  },
  daemons: {
    /** `cd ../shop` runs. */
    cd: 1.362,
    /**
     * `mise run db` runs: pitchfork waits on pg_isready. No sound of its
     * own: the synthesized band rested over the wait, and the bed plays
     * through it. Listened for so the scene's export stays typed.
     */
    run: 7.442,
    /** `✔ [shop/postgres] started on port 5432`: the pilot light. */
    started: 7.989,
    /** The thread ties the server's version to `postgres = "18"`. */
    tie: 10.91,
  },
  dotfiles: TICKET_CUES,
  track: {
    /** Checkpoints join the rail: the baseline, the watcher's save, the rollback's. */
    checkpoints: [6.895, 17.444, 31.795],
    /** `mise dot rollback` rolls back. */
    rollback: 31.795,
    /**
     * The alias's copy seats in the ~/.zshrc card (zshrc.ts aliasLift's
     * landing). The take's time, so null until the scene exports it.
     */
    seat: null,
  },
  machines: TICKET_CUES,
  lock: {
    /** The ledger slides in. */
    ledger: 4.022,
    /** The thread runs from the request to the version the lock records. */
    thread: 7.397,
    /** The chips dock: mise.toml by `uses: jdx/mise-action@v4`, then mise.lock by `install_args: --locked`, each row lighting. */
    dock: [12.375, 12.625],
    /** `run: mise run ci` lights. */
    run: 13.625,
  },
  new: TICKET_CUES,
  bootstrap: {
    /** The `~/.zshrc` card flies in off its create row. */
    card: 5.049,
    /** `Wrote 2 file(s)`: the Dotfiles and Config tiles tick. */
    wrote: 10.531,
    /** `applied mise-history`: the Watcher tile ticks (none without systemd). */
    watcher: (facts) => (reelData(facts)?.variant === "plain" ? null : 15.054),
    /** `mise bootstrap: tools`: the Tools tile shows the step started. */
    tools: 15.054,
  },
  breath: {
    /** ⌃L clears the shell. */
    clear: 1.75,
  },
  clone: {
    /** `mise run ci` lifts into big type. */
    slam: 6.7,
    /** git's first line prints. */
    git: 8.826,
    /** [tools] lights: its table's voice from the pitch. */
    tools: 11.638,
    /** Each lane appears, in the take's order. */
    test: 15,
    lint: 15.625,
    build: 15.313,
    ci: 16.25,
    /** [tasks] lights: its table's voice from the pitch. */
    tasks: 20.125,
    /** The `[ci] api ready` line lands as big type: the bell. */
    ready: 22.75,
    /** [env] lights: its table's voice from the pitch. */
    env: 23.875,
    /** The card folds to its three headers once the lanes have settled into [tasks]. */
    fold: 21.125,
    /**
     * The climax line's pieces land home and ignite their headers, in
     * reading order: `[ci]` in [tasks], the node version in [tools],
     * `APP_ENV=…` in [env]: clone.ts's homeLand, which clone's cues
     * export (`[ci]` leaves first, the other two together an eighth later).
     */
    home: [27.563, 27.688, 27.75],
  },
  morph: {
    /** The card's three lit headers lift off toward the toque. */
    lift: 0.125,
    /** Each band settles over its lobe, tools, env, tasks (kit/chef.ts MORPH_CUE.lands). */
    lands: MORPH_CUE.lands,
    /** The toque starts its drop, on bar 2's downbeat: a soft kick. */
    drop: 4,
    /** The toque touches the head and the chef blooms (MORPH_CUE.land). */
    land: MORPH_CUE.land,
  },
} as const satisfies { [S in SectionId]?: Readonly<Record<string, Fallback>> };

/** The sections the score listens to. */
export type CuedId = keyof typeof LISTEN;
/** The names section `S` listens for. */
export type CueName<S extends CuedId> = keyof (typeof LISTEN)[S] & string;

/** What a scene may export as `cues`: some or all of its section's names, in its beats. */
export type SceneCues<S extends CuedId> =
  | Partial<Record<CueName<S>, CueValue>>
  | ((facts: ReelFacts | null) => Partial<Record<CueName<S>, CueValue>>);

/** A section's cues, resolved: global seconds. */
export interface Cues<K extends string> {
  /** Every time of `name`, global seconds; empty when it does not happen. */
  all(name: K): number[];
  /** Its first time, or null when it does not happen. */
  at(name: K): number | null;
  /** The first time, or `or` (section beats) when it does not happen. */
  or(name: K, or: number): number;
  /** Where each name's times came from: the scene's export, or the fallback here. */
  readonly source: Readonly<Record<K, "scene" | "fallback">>;
}

/** Scene modules, by section: anything a module may export `cues` on. */
export type SceneModules = Partial<Record<SectionId, object>>;

const beatsOf = (v: CueValue): number[] =>
  v === null
    ? []
    : (typeof v === "number" ? [v] : [...v]).filter((b) => Number.isFinite(b));

/** A scene module's own cue table, if it exports one. */
export function exported(
  mod: object | undefined,
  facts: ReelFacts | null,
): Readonly<Record<string, CueValue>> | null {
  if (!mod || !("cues" in mod)) return null;
  const c = (mod as { cues: unknown }).cues;
  const table =
    typeof c === "function"
      ? (c as (f: ReelFacts | null) => unknown)(facts)
      : c;
  return table && typeof table === "object"
    ? (table as Record<string, CueValue>)
    : null;
}

/**
 * Section `s`'s cues, for the facts the picture draws: each name from the
 * scene module's export where it gives one, else the fallback.
 */
export function resolveCues<S extends CuedId>(
  s: Section & { id: S },
  facts: ReelFacts | null,
  modules: SceneModules,
): Cues<CueName<S>> {
  const fallbacks = LISTEN[s.id] as Readonly<Record<string, Fallback>>;
  const own = exported(modules[s.id], facts);
  const times = new Map<string, number[]>();
  const source: Record<string, "scene" | "fallback"> = {};
  for (const [name, fb] of Object.entries(fallbacks)) {
    const mine = own && name in own && own[name] !== undefined;
    const v: CueValue = mine
      ? own[name]
      : typeof fb === "function"
        ? fb(facts)
        : fb;
    source[name] = mine ? "scene" : "fallback";
    times.set(
      name,
      beatsOf(v).map((b) => s.beat(b)),
    );
  }
  const all = (name: string): number[] => times.get(name) ?? [];
  return {
    all,
    at: (name) => all(name)[0] ?? null,
    or: (name, or) => all(name)[0] ?? s.beat(or),
    source: source as Record<CueName<S>, "scene" | "fallback">,
  };
}
