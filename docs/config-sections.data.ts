import reference, { type Key } from "./config-reference.data.ts";

// The sections of the mise.toml reference and the keys directly in each, without
// the keys below those. The page renders these at once and loads the rest of the
// reference (config-reference.data.ts) after it has shown.
export default {
  watch: reference.watch,
  async load(): Promise<Key[]> {
    const keys = await reference.load();
    return keys.map((section) => ({
      ...section,
      children: section.children.map((key) => ({ ...key, children: [] })),
    }));
  },
};
