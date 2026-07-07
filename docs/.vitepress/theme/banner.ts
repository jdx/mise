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
const ID_KEY = "jdx-banner-id";
const HEIGHT_KEY = "jdx-banner-height";

export function initBanner(): void {
  if (typeof window === "undefined") return;
  fetch(ENDPOINT)
    .then((r) => (r.ok ? (r.json() as Promise<BannerData>) : null))
    .then((b) => {
      if (
        !b ||
        !b.enabled ||
        isExpired(b.expires) ||
        localStorage.getItem(STORAGE_KEY) === b.id
      ) {
        clearReserved();
        return;
      }
      render(b);
    })
    .catch(clearReserved);
}

function clearReserved(): void {
  document.documentElement.style.removeProperty("--vp-layout-top-height");
  try {
    localStorage.removeItem(ID_KEY);
    localStorage.removeItem(HEIGHT_KEY);
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
      localStorage.setItem(ID_KEY, b.id);
      localStorage.setItem(HEIGHT_KEY, `${el.offsetHeight}px`);
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
    localStorage.setItem(STORAGE_KEY, b.id);
    observer?.disconnect();
    el.remove();
    clearReserved();
  });
  el.appendChild(btn);

  document.body.prepend(el);

  requestAnimationFrame(syncHeight);
  observer?.observe(el);
}
