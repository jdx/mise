import "./banner.css";

interface BannerData {
  id: string;
  enabled: boolean;
  message: string;
  link?: string;
  linkText?: string;
}

const ENDPOINT = "https://jdx.dev/banner.json";
const STORAGE_KEY = "jdx-banner-dismissed";

export function initBanner(): void {
  if (typeof window === "undefined") return;
  fetch(ENDPOINT, { cache: "no-cache" })
    .then((r) => (r.ok ? (r.json() as Promise<BannerData>) : null))
    .then((b) => {
      if (!b || !b.enabled) return;
      if (localStorage.getItem(STORAGE_KEY) === b.id) return;
      render(b);
    })
    .catch(() => {});
}

function isHttpUrl(value: string): boolean {
  try {
    const u = new URL(value, window.location.href);
    return u.protocol === "http:" || u.protocol === "https:";
  } catch {
    return false;
  }
}

function render(b: BannerData): void {
  const el = document.createElement("div");
  el.className = "jdx-banner";
  el.setAttribute("role", "region");
  el.setAttribute("aria-label", "Site announcement");

  const msg = document.createElement("span");
  msg.textContent = b.message;
  el.appendChild(msg);

  if (b.link && isHttpUrl(b.link)) {
    const a = document.createElement("a");
    a.href = b.link;
    a.target = "_blank";
    a.rel = "noopener noreferrer";
    a.textContent = b.linkText || "Learn more";
    el.appendChild(a);
  }

  const btn = document.createElement("button");
  btn.type = "button";
  btn.setAttribute("aria-label", "Dismiss");
  btn.textContent = "\u00d7";
  btn.addEventListener("click", () => {
    localStorage.setItem(STORAGE_KEY, b.id);
    el.remove();
    document.documentElement.style.removeProperty("--vp-layout-top-height");
  });
  el.appendChild(btn);

  document.body.prepend(el);

  requestAnimationFrame(() => {
    document.documentElement.style.setProperty(
      "--vp-layout-top-height",
      `${el.offsetHeight}px`,
    );
  });
}
