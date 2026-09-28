// A part's view of its section's cues (score/cues.ts): the scene's own
// export where it has one, else the fallbacks, in global seconds. Kept out
// of score/index.ts so the parts it imports do not import it back.

import type { ReelFacts } from "../bible";
import { BEAT, type Section } from "../timeline";
import {
  type Cues,
  type CuedId,
  type CueName,
  resolveCues,
  type SceneModules,
} from "./cues";
import { SCENE_MODULES } from "./scenes";

let modules: SceneModules = SCENE_MODULES;

/** Section `s`'s cues, which must be section `id`'s. */
export function listen<S extends CuedId>(
  id: S,
  s: Section,
  facts: ReelFacts | null,
): Cues<CueName<S>> {
  if (s.id !== id) throw new Error(`listening for ${id}'s cues in ${s.id}`);
  return resolveCues(s as Section & { id: S }, facts, modules);
}

/**
 * Run `fn` with the score reading cues from `mods` instead of the scenes
 * (for tests: a scene's export moving a sound). Restores the scenes after.
 */
export function withSceneModules<T>(mods: SceneModules, fn: () => T): T {
  const prev = modules;
  modules = mods;
  try {
    return fn();
  } finally {
    modules = prev;
  }
}

/** Section `s`'s beat at global time `t`, rounded clear of float error. */
export const beatIn = (s: Section, t: number): number =>
  Math.round(((t - s.start) / BEAT) * 1e9) / 1e9;
