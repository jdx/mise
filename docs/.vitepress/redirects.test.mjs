import assert from "node:assert/strict";
import { test } from "node:test";
import { checkSite, internalTarget } from "./check-links.mjs";
import {
  anchorRedirectTarget,
  normalizePath,
  pageRedirectTarget,
  redirectStub,
  stubFile,
} from "./redirects.mjs";

test("normalizes published paths", () => {
  for (const [path, expected] of [
    ["/team.html", "/team"],
    ["/team", "/team"],
    ["/dev-tools/", "/dev-tools"],
    ["/dev-tools/index.html", "/dev-tools"],
    ["/dev-tools", "/dev-tools"],
    ["/", "/"],
    ["/index.html", "/"],
  ]) {
    assert.equal(normalizePath(path), expected, path);
  }
});

test("writes stubs where the old page was published", () => {
  assert.equal(stubFile("/team.html"), "team.html");
  assert.equal(
    stubFile("/dev-tools/backends/pipx.html"),
    "dev-tools/backends/pipx.html",
  );
  assert.equal(stubFile("/old-section/"), "old-section/index.html");
  assert.equal(stubFile("/history"), "history.html");
});

test("maps a removed page's hash to the section's new home", () => {
  const redirect = {
    to: "/dotfiles/history.html",
    hashes: { "sharing-across-machines": "/dotfiles/sync.html" },
  };
  assert.equal(
    pageRedirectTarget(redirect, "#sharing-across-machines"),
    "/dotfiles/sync.html",
  );
  assert.equal(
    pageRedirectTarget(redirect, "#rollback"),
    "/dotfiles/history.html#rollback",
  );
  assert.equal(pageRedirectTarget(redirect, ""), "/dotfiles/history.html");
  assert.equal(
    pageRedirectTarget({ to: "/about.html#who-makes-mise" }, "#jdx"),
    "/about.html#who-makes-mise",
  );
});

test("follows a section that moved off a surviving page", () => {
  const redirects = {
    "/configuration.html#scopes": "/dev-tools/versions.html#scopes",
  };
  for (const path of ["/configuration.html", "/configuration"]) {
    assert.equal(
      anchorRedirectTarget(path, "#scopes", redirects),
      "/dev-tools/versions.html#scopes",
    );
  }
  assert.equal(
    anchorRedirectTarget("/configuration.html", "#other", redirects),
    undefined,
  );
  assert.equal(
    anchorRedirectTarget("/faq.html", "#scopes", redirects),
    undefined,
  );
  assert.equal(
    anchorRedirectTarget("/configuration.html", "", redirects),
    undefined,
  );
});

test("renders a stub that works with and without JavaScript", () => {
  const html = redirectStub(
    { to: "/plugins.html", hashes: { x: "/plugins.html#</script>" } },
    "https://mise.jdx.dev",
  );
  assert.match(
    html,
    /<meta http-equiv="refresh" content="0; url=\/plugins.html">/,
  );
  assert.match(
    html,
    /<link rel="canonical" href="https:\/\/mise.jdx.dev\/plugins.html">/,
  );
  assert.match(html, /<meta name="robots" content="noindex">/);
  assert.match(html, /<a href="\/plugins.html">/);
  assert.doesNotMatch(html, /#<\/script>/);
  assert.ok(html.indexOf("<script>") < html.indexOf("http-equiv"));
});

test("resolves internal link targets", () => {
  assert.equal(
    internalTarget("/tasks/index.html", "./caching.html#sources"),
    "/tasks/caching.html#sources",
  );
  assert.equal(internalTarget("/faq.html", "#trust"), "/faq.html#trust");
  assert.equal(internalTarget("/faq.html", "/dev-tools/"), "/dev-tools/");
  assert.equal(
    internalTarget("/faq.html", "https://github.com/jdx/mise"),
    null,
  );
  assert.equal(internalTarget("/faq.html", "mailto:x@example.com"), null);
  assert.equal(internalTarget("/faq.html", "/schema/mise.json"), null);
  assert.equal(
    internalTarget("/faq.html", "/a.html#caf%C3%A9"),
    "/a.html#café",
  );
});

function site(pages) {
  return new Map(
    Object.entries(pages).map(([file, { ids = [], links = [] }]) => [
      file,
      { path: `/${file}`, ids: new Set(ids), links },
    ]),
  );
}

test("reports links to missing anchors, moved sections and moved pages", () => {
  const problems = checkSite(
    site({
      "index.html": {
        links: [
          "/faq.html#trust",
          "/faq.html#gone",
          "/faq.html#old",
          "/team.html",
          "/missing.html",
          "/releases.html#2026.10.4",
        ],
      },
      "faq.html": { ids: ["trust"] },
      "about.html": { ids: ["who-makes-mise"] },
      "releases.html": {},
      "team.html": {},
    }),
    {
      pages: { "/team.html": { to: "/about.html#who-makes-mise" } },
      anchors: { "/faq.html#old": "/about.html#who-makes-mise" },
      dynamicAnchors: new Set(["/releases"]),
    },
  );
  assert.deepEqual(problems, [
    "/index.html: links to missing anchor /faq.html#gone",
    "/index.html: links to moved section /faq.html#old; link /about.html#who-makes-mise",
    "/index.html: links to moved page /team.html; link /about.html#who-makes-mise",
    "/index.html: links to missing page /missing.html",
  ]);
});

test("reports broken redirect entries", () => {
  const problems = checkSite(
    site({
      "faq.html": { ids: ["trust"] },
      "about.html": {},
    }),
    {
      pages: { "/team.html": { to: "/about.html#who-makes-mise" } },
      anchors: {
        "/faq.html#trust": "/about.html",
        "/gone.html#x": "/about.html",
      },
      dynamicAnchors: new Set(),
    },
  );
  assert.deepEqual(problems, [
    "redirects.mjs: no stub was written for /team.html",
    "redirect /team.html: links to missing anchor /about.html#who-makes-mise",
    "redirects.mjs: /faq.html#trust still exists on its page, so the redirect never applies",
    "redirects.mjs: /gone.html#x is on a page that does not exist; use pageRedirects",
  ]);
});
