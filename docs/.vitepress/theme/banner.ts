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

function getDismissedId(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

export function initBanner(): void {
  if (typeof window === "undefined") return;
  const controller = new AbortController();
  const timeout = window.setTimeout(() => controller.abort(), 5000);
  fetch(ENDPOINT, { signal: controller.signal })
    .then((r) => (r.ok ? (r.json() as Promise<BannerData>) : null))
    .then((b) => {
      if (
        !b ||
        !b.enabled ||
        isExpired(b.expires) ||
        getDismissedId() === b.id
      ) {
        clearReserved();
        return;
      }
      render(b);
    })
    .catch(clearCurrentReservation)
    .finally(() => window.clearTimeout(timeout));
}

function clearCurrentReservation(): void {
  document.documentElement.style.removeProperty("--vp-layout-top-height");
}

function clearReserved(): void {
  clearCurrentReservation();
  try {
    localStorage.removeItem(CACHE_KEY);
  } catch {
    // localStorage unavailable — nothing cached to clear.
  }
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
      localStorage.setItem(
        CACHE_KEY,
        JSON.stringify({
          id: b.id,
          height: `${el.offsetHeight}px`,
          width: window.innerWidth,
          expires: b.expires ?? null,
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
    observer?.disconnect();
    el.remove();
    clearReserved();
  });
  el.appendChild(btn);

  document.body.prepend(el);

  requestAnimationFrame(syncHeight);
  observer?.observe(el);
}
