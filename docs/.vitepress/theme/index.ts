import type { Theme } from "vitepress";
import DefaultTheme from "vitepress/theme";
import { enhanceAppWithTabs } from "vitepress-plugin-tabs/client";
import { initBanner } from "./banner";
import "virtual:group-icons.css";
import "./custom.css";
import "./landing.css";
import Layout from "./Layout.vue";
import { onMounted, onUnmounted } from "vue";
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
    let observer: MutationObserver | undefined;
    onMounted(() => {
      // The nav bar renders the GitHub link twice: inline, and inside the "..."
      // overflow menu that replaces it below 1280px. Both need their own badge.
      const addStarCount = () => {
        if (!starsData.stars) return false;

        const githubLinks = document.querySelectorAll(
          '.VPSocialLinks a[href*="github.com/jdx/mise"]',
        );
        githubLinks.forEach((githubLink) => {
          if (githubLink.querySelector(".star-count")) return;
          const starBadge = document.createElement("span");
          starBadge.className = "star-count";
          starBadge.title = "GitHub Stars";
          const glyph = document.createElement("span");
          glyph.className = "star-glyph";
          glyph.textContent = "★";
          glyph.setAttribute("aria-hidden", "true");
          starBadge.append(glyph, starsData.stars);
          githubLink.appendChild(starBadge);
        });
        return (
          githubLinks.length > 0 &&
          Array.from(githubLinks).every((link) =>
            link.querySelector(".star-count"),
          )
        );
      };

      if (addStarCount()) return;

      // The nav mounts after the theme, so wait for it rather than re-running on
      // every mutation the page makes for the rest of its life.
      observer = new MutationObserver(() => {
        if (addStarCount()) observer?.disconnect();
      });
      observer.observe(document.querySelector(".VPNav") || document.body, {
        childList: true,
        subtree: true,
      });
    });
    onUnmounted(() => observer?.disconnect());
  },
} satisfies Theme;
