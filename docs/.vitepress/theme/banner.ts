import "./banner.css";

interface BannerData {
  id: string;
  enabled: boolean;
  message: string;
  link?: string;
  linkText?: string;
  expires?: string;
}

const ENDPOINT = "https://jdx.dev/banner.json";
const STORAGE_KEY = "jdx-banner-dismissed";
// Cached by the inline head script (config.ts) to reserve the banner's
// space before first paint so the header doesn't jump when it arrives.
const CACHE_KEY = "jdx-banner-cache";
let activeBanner:
  { element: HTMLElement; observer: ResizeObserver | null } | undefined;

function getDismissedId(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

export function initBanner(): void {
  if (typeof window === "undefined") return;
  const cachedBanner = readCachedBanner();
  if (cachedBanner) render(cachedBanner, false);

  const controller = new AbortController();
  const timeout = window.setTimeout(() => controller.abort(), 5000);
  fetch(ENDPOINT, { signal: controller.signal })
    .then((r) => {
      if (!r.ok) throw new Error(`banner request failed: ${r.status}`);
      return r.json() as Promise<BannerData>;
    })
    .then((b) => {
      if (
        !b ||
        !b.enabled ||
        isExpired(b.expires) ||
        getDismissedId() === b.id
      ) {
        removeActiveBanner();
        clearReserved();
        return;
      }
      render(b);
    })
    .catch(() => {
      clearCachedReservation();
      if (!activeBanner) clearCurrentReservation();
    })
    .finally(() => window.clearTimeout(timeout));
}

function readCachedBanner(): BannerData | null {
  if (
    !document.documentElement.style.getPropertyValue("--vp-layout-top-height")
  ) {
    return null;
  }
  try {
    const cached = JSON.parse(localStorage.getItem(CACHE_KEY) ?? "null");
    const b = cached?.banner;
    return b &&
      typeof b.id === "string" &&
      typeof b.enabled === "boolean" &&
      typeof b.message === "string" &&
      (!b.link || typeof b.link === "string") &&
      (!b.linkText || typeof b.linkText === "string") &&
      (!b.expires || typeof b.expires === "string")
      ? b
      : null;
  } catch {
    return null;
  }
}

function clearCachedReservation(): void {
  try {
    localStorage.removeItem(CACHE_KEY);
  } catch {
    // localStorage unavailable — nothing cached to clear.
  }
}

function clearCurrentReservation(): void {
  document.documentElement.style.removeProperty("--vp-layout-top-height");
}

function clearReserved(): void {
  clearCurrentReservation();
  clearCachedReservation();
}

function isExpired(expires: string | undefined): boolean {
  if (!expires) return false;
  const t = Date.parse(expires);
  if (Number.isNaN(t)) return false;
  return Date.now() >= t;
}

function isHttpUrl(value: string): boolean {
  try {
    const u = new URL(value, window.location.href);
    return u.protocol === "http:" || u.protocol === "https:";
  } catch {
    return false;
  }
}

function removeActiveBanner(): void {
  activeBanner?.observer?.disconnect();
  activeBanner?.element.remove();
  activeBanner = undefined;
}

function render(b: BannerData, persist = true): void {
  removeActiveBanner();
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
    a.rel = "noopener";
    a.textContent = b.linkText || "Learn more";
    el.appendChild(a);
  }

  const syncHeight = () => {
    document.documentElement.style.setProperty(
      "--vp-layout-top-height",
      `${el.offsetHeight}px`,
    );
    try {
      if (!persist) return;
      localStorage.setItem(
        CACHE_KEY,
        JSON.stringify({
          id: b.id,
          height: `${el.offsetHeight}px`,
          width: window.innerWidth,
          fontSize: getComputedStyle(document.documentElement).fontSize,
          pixelRatio: window.devicePixelRatio,
          cachedAt: Date.now(),
          expires: b.expires ?? null,
          banner: b,
        }),
      );
    } catch {
      // localStorage unavailable — skip caching; next load just pops in.
    }
  };

  const observer =
    typeof ResizeObserver !== "undefined"
      ? new ResizeObserver(syncHeight)
      : null;

  const btn = document.createElement("button");
  btn.type = "button";
  btn.setAttribute("aria-label", "Dismiss");
  btn.textContent = "\u00d7";
  btn.addEventListener("click", () => {
    try {
      localStorage.setItem(STORAGE_KEY, b.id);
    } catch {
      // Dismiss for this page even when localStorage is unavailable.
    }
    removeActiveBanner();
    clearReserved();
  });
  el.appendChild(btn);

  document.body.prepend(el);
  activeBanner = { element: el, observer };

  requestAnimationFrame(syncHeight);
  observer?.observe(el);
}
