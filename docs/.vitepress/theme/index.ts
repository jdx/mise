import type { Theme } from "vitepress";
import DefaultTheme from "vitepress/theme";
import { enhanceAppWithTabs } from "vitepress-plugin-tabs/client";
import "virtual:group-icons.css";
import "./custom.css";
import { onMounted } from "vue";
import { data as starsData } from "../stars.data";

export default {
  extends: DefaultTheme,
  enhanceApp({ app }) {
    enhanceAppWithTabs(app);
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
          starBadge.innerHTML = starsData.stars;
          starBadge.title = "GitHub Stars";
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
