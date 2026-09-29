// from jdx/hk@37937824 docs/.vitepress/theme/showreel/test/repo.ts
// The checkout the tests run in. The runner bundles each test into a
// temporary directory, so the path comes from the working directory: the
// checkout's root under `aube run`, or anywhere else inside it.

import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

export const REPO = (() => {
  for (let dir = resolve(process.cwd()); ; dir = dirname(dir)) {
    if (
      existsSync(join(dir, "settings.toml")) &&
      existsSync(join(dir, "docs/.vitepress/theme/showreel"))
    )
      return dir;
    if (dirname(dir) === dir)
      throw new Error(`no mise checkout at or above ${process.cwd()}`);
  }
})();

/** The showreel's source directory. */
export const SHOWREEL = join(REPO, "docs/.vitepress/theme/showreel");
