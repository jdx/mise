import type { Theme } from "vitepress";
import DefaultTheme from "vitepress/theme";
import { enhanceAppWithTabs } from "vitepress-plugin-tabs/client";
import "virtual:group-icons.css";
import "./custom.css";
import NavBarExtra from "./NavBarExtra.vue";
import { h } from "vue";

export default {
  extends: DefaultTheme,
  Layout() {
    return h(DefaultTheme.Layout, null, {
      "nav-bar-content-before": () => h(NavBarExtra),
    });
  },
  enhanceApp({ app }) {
    enhanceAppWithTabs(app);
  },
} satisfies Theme;
