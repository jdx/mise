// Old documentation URLs keep working after a page or a section moves.
//
// Both maps are append-only. Published links, search results, and released mise
// binaries point at these URLs, so an entry stays after the move ships. Keys and
// targets are root-absolute published paths: `/page.html`, `/section/` for an
// index page, and `/page.html#id` for a section. Moved sections live in
// anchor-redirects.mjs, which the browser loads only when a hash misses.

/**
 * Pages that no longer exist. The build writes a stub at each key that sends
 * the reader to `to`. When the old page's sections went to different places,
 * `hashes` maps an old section id to its new home; any other hash is carried
 * over to `to`.
 *
 * @type {Record<string, { to: string, hashes?: Record<string, string> }>}
 */
export const pageRedirects = {
  "/bootstrap/user.html": {
    to: "/bootstrap/shell.html#set-your-login-shell",
    hashes: {
      semantics: "/bootstrap/shell.html#set-your-login-shell",
      commands: "/bootstrap/shell.html#set-your-login-shell",
    },
  },
  "/cli/bootstrap/launchd/apply.html": {
    to: "/cli/bootstrap/macos/launchd-agents/apply.html",
  },
  "/cli/bootstrap/launchd/status.html": {
    to: "/cli/bootstrap/macos/launchd-agents/status.html",
  },
  "/cli/bootstrap/macos-defaults/apply.html": {
    to: "/cli/bootstrap/macos/defaults/apply.html",
  },
  "/cli/bootstrap/macos-defaults/status.html": {
    to: "/cli/bootstrap/macos/defaults/status.html",
  },
  "/cli/bootstrap/systemd/apply.html": {
    to: "/cli/bootstrap/linux/systemd-units/apply.html",
  },
  "/cli/bootstrap/systemd/status.html": {
    to: "/cli/bootstrap/linux/systemd-units/status.html",
  },
  "/dev-tools/backend_architecture.html": {
    to: "/dev-tools/backends/",
    hashes: {
      "what-are-backends": "/dev-tools/backends/#identifiers",
      "the-backend-trait-system": "/architecture.html#backend-system",
      "backend-types": "/dev-tools/backends/#which-backend-to-use",
      "how-backend-selection-works":
        "/dev-tools/backends/#how-backend-selection-works",
      "environment-variable-overrides":
        "/dev-tools/backends/#environment-variable-overrides",
      "registry-system": "/dev-tools/backends/#choose-a-backend-yourself",
      "backend-capabilities-comparison":
        "/dev-tools/backends/#which-backend-to-use",
      "when-to-use-each-backend": "/dev-tools/backends/#which-backend-to-use",
      "backend-dependencies": "/dev-tools/#tool-dependencies",
      "configuration-and-overrides":
        "/dev-tools/backends/#choose-a-backend-yourself",
      "disable-backends": "/dev-tools/backends/#disable-backends",
      "force-backend-for-tool":
        "/dev-tools/backends/#choose-a-backend-yourself",
      "backend-specific-settings": "/dev-tools/backends/#identifiers",
      "troubleshooting-backend-issues": "/dev-tools/backends/#troubleshooting",
      "debug-backend-selection": "/dev-tools/backends/#troubleshooting",
    },
  },
  "/dev-tools/backends/pipx.html": {
    to: "/dev-tools/backends/pypi.html",
  },
  "/history.html": {
    to: "/dotfiles/history.html",
    hashes: {
      saving: "/dotfiles/history.html#saving",
      "automatic-saves": "/dotfiles/history.html#automatic-saves",
      comparing: "/dotfiles/history.html#comparing",
      "referring-to-checkpoints":
        "/dotfiles/history.html#referring-to-checkpoints",
      "rolling-back": "/dotfiles/history.html#rolling-back",
      "reload-an-application-after-restoring-files":
        "/dotfiles/history.html#reload-an-application-after-restoring-files",
      "sharing-across-machines": "/dotfiles/sync.html#sharing-across-machines",
      "choose-a-sync-mode": "/dotfiles/sync.html#choose-a-sync-mode",
      "sync-immediately": "/dotfiles/sync.html#sync-immediately",
      "resolve-a-conflict": "/dotfiles/sync.html#resolve-a-conflict",
      "conflict-notifications": "/dotfiles/sync.html#conflict-notifications",
      "repository-authentication":
        "/dotfiles/sync.html#repository-authentication",
      "how-shared-history-is-stored":
        "/dotfiles/sync.html#how-shared-history-is-stored",
      "identify-commits-by-machine":
        "/dotfiles/sync.html#identify-commits-by-machine",
      "resolve-unrelated-histories":
        "/dotfiles/sync.html#resolve-unrelated-histories",
      "capturing-an-external-command":
        "/dotfiles/history.html#capturing-an-external-command",
      "explicit-tracking-and-exclusions":
        "/dotfiles/history.html#explicit-tracking-and-exclusions",
      "preview-before-tracking":
        "/dotfiles/history.html#preview-before-tracking",
      "choose-which-files-a-directory-saves":
        "/dotfiles/history.html#choose-which-files-a-directory-saves",
      "capture-warnings": "/dotfiles/history.html#capture-warnings",
      "exclude-files-from-one-directory":
        "/dotfiles/history.html#exclude-files-from-one-directory",
      "exclude-files-across-tracked-entries":
        "/dotfiles/history.html#exclude-files-across-tracked-entries",
      "credential-filtering-and-omissions":
        "/dotfiles/history.html#credential-filtering-and-omissions",
      "stop-saving-a-path": "/dotfiles/history.html#stop-tracking-a-file",
      "encrypted-shared-files": "/dotfiles/encryption.html",
      "choose-recipients": "/dotfiles/encryption.html#choose-recipients",
      "use-an-existing-ssh-key":
        "/dotfiles/encryption.html#use-an-existing-ssh-key",
      "generate-a-dedicated-age-key":
        "/dotfiles/encryption.html#generate-a-dedicated-age-key",
      "add-a-recovery-recipient":
        "/dotfiles/encryption.html#add-a-recovery-recipient",
      "allow-plaintext-history":
        "/dotfiles/encryption.html#allow-plaintext-history",
      "remove-plaintext-from-history":
        "/dotfiles/encryption.html#remove-plaintext-from-history",
      health: "/dotfiles/history.html#health",
      "what-a-checkpoint-records":
        "/dotfiles/reference.html#what-a-checkpoint-records",
      "nested-repositories": "/dotfiles/history.html#nested-repositories",
      "descriptions-from-an-agent":
        "/dotfiles/history.html#descriptions-from-an-agent",
      "recovery-details": "/dotfiles/reference.html#recovery-details",
      "operation-checkpoints": "/dotfiles/reference.html#operation-checkpoints",
      "watcher-reference": "/dotfiles/reference.html#watcher-reference",
      "adaptive-scheduling": "/dotfiles/reference.html#adaptive-scheduling",
      "reconciliation-and-failures":
        "/dotfiles/reference.html#reconciliation-and-failures",
      retention: "/dotfiles/reference.html#retention",
      "requirements-and-settings":
        "/dotfiles/history.html#requirements-and-settings",
    },
  },
  "/mise-cookbook/shell-tricks.html": {
    to: "/tips-and-tricks.html#shell-prompt",
    hashes: {
      "prompt-colouring":
        "/tips-and-tricks.html#print-what-changes-when-you-enter-a-project",
      "current-configuration-environment-in-powerline-go-prompt":
        "/tips-and-tricks.html#show-the-project-and-environment-in-your-prompt",
      "inspect-what-changed-after-mise-hook":
        "/tips-and-tricks.html#see-which-variables-mise-set",
    },
  },
  "/plugin-usage.html": {
    to: "/plugins.html",
    hashes: {
      "what-are-plugins": "/plugins.html#choose-a-plugin-type",
      "backend-plugins": "/plugins.html#backend-plugins",
      "tool-plugins": "/plugins.html#tool-plugins",
      "installing-plugins": "/plugins.html#installing-plugins",
      "from-a-git-repository": "/plugins.html#from-a-git-repository",
      "from-zip-file": "/plugins.html#from-zip-file",
      "from-local-directory": "/plugins.html#from-local-directory",
      "using-plugins-advanced": "/plugins.html#using-plugins",
      "plugin-tool-format": "/plugins.html#backend-plugins",
      "managing-plugins": "/plugins.html#update-plugins",
      "list-installed-plugins": "/plugins.html#update-plugins",
      "update-plugins": "/plugins.html#update-plugins",
      "remove-plugins": "/plugins.html#remove-plugins",
      configuration: "/plugins.html#tool-options",
      "finding-plugins": "/plugins.html#when-you-need-a-plugin",
      "plugin-examples": "/backend-plugin-development.html#complete-example",
      "vfox-npm-example-plugin":
        "/backend-plugin-development.html#complete-example",
      "backend-plugins-advanced": "/backend-plugin-development.html",
      "tool-plugins-advanced": "/tool-plugin-development.html",
      "security-considerations": "/plugins.html#security-considerations",
      troubleshooting: "/plugins.html#troubleshooting",
      "plugin-installation-fails": "/plugins.html#troubleshooting",
      "tool-installation-fails": "/plugins.html#troubleshooting",
      "environment-issues": "/plugins.html#troubleshooting",
      "next-steps": "/plugins.html#choose-a-plugin-type",
    },
  },
  "/README.html": {
    to: "https://github.com/jdx/mise/blob/main/docs/README.md",
  },
  "/team.html": {
    to: "/about.html#who-makes-mise",
    hashes: {
      "advisory-board": "/about.html#advisory-board",
      contributors: "/about.html#contributors",
    },
  },
};

/**
 * Normalize a published path so `/a.html`, `/a`, `/dir/`, `/dir/index.html`
 * and `/dir` compare equal to their map keys.
 *
 * @param {string} path
 */
export function normalizePath(path) {
  const normalized = path
    .replace(/\.html$/, "")
    .replace(/\/index$/, "/")
    .replace(/\/+$/, "");
  return normalized === "" ? "/" : normalized;
}

/**
 * The file a page redirect's stub is written to, relative to the build output.
 *
 * @param {string} from
 */
export function stubFile(from) {
  if (from.endsWith("/")) return `${from.slice(1)}index.html`;
  if (from.endsWith(".html")) return from.slice(1);
  return `${from.slice(1)}.html`;
}

/**
 * Where a removed page sends a reader who arrived with `hash` (including `#`).
 *
 * @param {{ to: string, hashes?: Record<string, string> }} redirect
 * @param {string} hash
 */
export function pageRedirectTarget(redirect, hash) {
  const id = decodeHash(hash);
  if (id && redirect.hashes && Object.hasOwn(redirect.hashes, id)) {
    return redirect.hashes[id];
  }
  if (id && !redirect.to.includes("#")) return `${redirect.to}${hash}`;
  return redirect.to;
}

/**
 * Where a section that moved off `pathname` now lives, if it moved.
 *
 * @param {string} pathname
 * @param {string} hash including the leading `#`
 * @param {Record<string, string>} redirects
 */
export function anchorRedirectTarget(pathname, hash, redirects) {
  const id = decodeHash(hash);
  if (!id) return undefined;
  const page = normalizePath(pathname);
  for (const [from, to] of Object.entries(redirects)) {
    const [fromPath, fromId] = splitHash(from);
    if (fromId === id && normalizePath(fromPath) === page) return to;
  }
  return undefined;
}

/**
 * The static page written in place of a removed one. It works without
 * JavaScript (meta refresh, which drops the hash) and with it (the script also
 * maps the hash).
 *
 * @param {{ to: string, hashes?: Record<string, string> }} redirect
 * @param {string} siteUrl
 */
export function redirectStub(redirect, siteUrl) {
  const to = escapeHtml(redirect.to);
  const canonical = escapeHtml(new URL(redirect.to, siteUrl).toString());
  const script = `(function () {
  var to = ${scriptJson(redirect.to)};
  var hashes = ${scriptJson(redirect.hashes || {})};
  var id = location.hash.slice(1);
  try { id = decodeURIComponent(id); } catch (e) {}
  if (id && Object.prototype.hasOwnProperty.call(hashes, id)) to = hashes[id];
  else if (id && to.indexOf("#") < 0) to += location.hash;
  location.replace(to);
})();`;
  return `<!doctype html>
<html lang="en-US">
<head>
<meta charset="utf-8">
<title>Moved to ${to}</title>
<meta name="robots" content="noindex">
<link rel="canonical" href="${canonical}">
<script>${script}</script>
<meta http-equiv="refresh" content="0; url=${to}">
</head>
<body>
<p>This page moved to <a href="${to}">${to}</a>.</p>
</body>
</html>
`;
}

function decodeHash(hash) {
  const raw = hash.startsWith("#") ? hash.slice(1) : hash;
  try {
    return decodeURIComponent(raw);
  } catch {
    return raw;
  }
}

function splitHash(url) {
  const index = url.indexOf("#");
  return index === -1 ? [url, ""] : [url.slice(0, index), url.slice(index + 1)];
}

function escapeHtml(text) {
  return text
    .replace(/&/g, "&amp;")
    .replace(/"/g, "&quot;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

// JSON inside a <script> must not close the tag.
function scriptJson(value) {
  return JSON.stringify(value).replace(/</g, "\\u003c");
}
