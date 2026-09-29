// Which Chromium the showreel's renderer, previewer and Chromium tests
// launch. In order:
//
//   1. SHOWREEL_CHROMIUM (or CHROME_PATH, as hk's renderer reads it): any
//      Chromium-based browser's executable;
//   2. Playwright's Chromium headless shell under PLAYWRIGHT_BROWSERS_PATH or
//      Playwright's own cache (~/.cache/ms-playwright on Linux,
//      ~/Library/Caches/ms-playwright on macOS, %LOCALAPPDATA%\ms-playwright
//      on Windows), at the revision the installed playwright-core
//      pins (`aube exec playwright-core install chromium-headless-shell`), or at
//      revision 1243 (playwright-core 1.63) when that one is not there;
//   3. otherwise undefined, and Playwright looks for its own.

import { existsSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

/** The headless shell's directory and executable, per platform. */
const SHELL = {
  linux: ["chrome-headless-shell-linux64", "chrome-headless-shell"],
  darwin: [
    process.arch === "arm64"
      ? "chrome-headless-shell-mac-arm64"
      : "chrome-headless-shell-mac-x64",
    "chrome-headless-shell",
  ],
  win32: ["chrome-headless-shell-win64", "chrome-headless-shell.exe"],
};

/** The headless shell revision the installed playwright-core pins, or null. */
function pinnedRevision() {
  try {
    // browsers.json is not in the package's exports; its package.json is.
    const require = createRequire(join(here, "../../package.json"));
    const pkg = dirname(require.resolve("playwright-core/package.json"));
    const browsers = JSON.parse(
      readFileSync(join(pkg, "browsers.json"), "utf8"),
    );
    return (
      browsers.browsers.find((b) => b.name === "chromium-headless-shell")
        ?.revision ?? null
    );
  } catch {
    return null;
  }
}

/** Where Playwright keeps its browsers by default on this platform. */
function playwrightCache() {
  if (process.platform === "darwin")
    return join(homedir(), "Library/Caches/ms-playwright");
  if (process.platform === "win32")
    return join(
      process.env.LOCALAPPDATA || join(homedir(), "AppData/Local"),
      "ms-playwright",
    );
  return join(homedir(), ".cache/ms-playwright");
}

/** The executable to pass as `chromium.launch({ executablePath })`, or undefined. */
export function chromiumPath() {
  const named = process.env.SHOWREEL_CHROMIUM || process.env.CHROME_PATH;
  if (named) return named;
  const shell = SHELL[process.platform];
  if (!shell) return undefined;
  const root = process.env.PLAYWRIGHT_BROWSERS_PATH || playwrightCache();
  for (const revision of new Set([pinnedRevision(), "1243"].filter(Boolean))) {
    const exe = join(root, `chromium_headless_shell-${revision}`, ...shell);
    if (existsSync(exe)) return exe;
  }
  return undefined;
}
