import type { Theme } from "vitepress";
import DefaultTheme from "vitepress/theme";
import { enhanceAppWithTabs } from "vitepress-plugin-tabs/client";
import { initBanner } from "./banner";
import "virtual:group-icons.css";
import "./custom.css";
import Layout from "./Layout.vue";
import { onMounted } from "vue";
import { data as starsData } from "../stars.data";

export default {
  extends: DefaultTheme,
  Layout,
  enhanceApp({ app, router }) {
    enhanceAppWithTabs(app);
    initBanner();

    const onBeforeRouteChange = router.onBeforeRouteChange;
    router.onBeforeRouteChange = async (to) => {
      if (typeof window !== "undefined") {
        const url = new URL(to, window.location.origin);
        if (
          url.origin === window.location.origin &&
          url.pathname.startsWith("/tools/")
        ) {
          const toolPath = url.pathname.replace(/\.html$/, "");
          window.location.assign(
            `https://mise-versions.jdx.dev${toolPath}${url.search}${url.hash}`,
          );
          return false;
        }
      }
      return onBeforeRouteChange?.(to);
    };
  },
  setup() {
    onMounted(() => {
      // Add star count to GitHub social link
      const addStarCount = () => {
        const githubLink = document.querySelector(
          '.VPSocialLinks a[href*="github.com/jdx/mise"]',
        );
        if (githubLink && !githubLink.querySelector(".star-count")) {
          const starBadge = document.createElement("span");
          starBadge.className = "star-count";
          starBadge.title = "GitHub Stars";
          const glyph = document.createElement("span");
          glyph.className = "star-glyph";
          glyph.textContent = "★";
          glyph.setAttribute("aria-hidden", "true");
          starBadge.append(glyph, starsData.stars);
          githubLink.appendChild(starBadge);
        }
      };

      // Try immediately and after a short delay to ensure DOM is ready
      addStarCount();
      setTimeout(addStarCount, 100);

      // Also watch for route changes
      const observer = new MutationObserver(addStarCount);
      observer.observe(document.body, { childList: true, subtree: true });
    });
  },
} satisfies Theme;
