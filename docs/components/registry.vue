<template>
  <input
    class="filter"
    type="text"
    placeholder="Filter by Short or Full"
    v-model="filter"
    autofocus="autofocus"
  />
  <label class="verified-filter">
    <input type="checkbox" v-model="verifiedOnly" />
    Only show tools with signature or provenance verification
  </label>
  <table class="full-width">
    <thead>
      <tr>
        <th>Short</th>
        <th>Full</th>
        <th>OS</th>
      </tr>
    </thead>
    <tbody>
      <tr v-if="filteredData.length === 0">
        <td colspan="3" class="no-matches">No matches found</td>
      </tr>
      <tr
        v-else
        v-for="(entry, index) in filteredData"
        :key="`backend-${index}`"
      >
        <td>
          <a
            v-if="entry.url"
            :href="entry.url"
            v-html="highlightMatches(entry.short)"
          ></a>
          <span v-else v-html="highlightMatches(entry.short)"></span>
          <a
            class="mise-versions"
            :href="`https://mise-versions.jdx.dev/tools/${encodeURIComponent(entry.short)}`"
            title="Versions and security info on mise-versions"
            >details ↗</a
          >
        </td>
        <td>
          <span v-for="(backend, index) in entry.backends">
            <a
              :href="`${backend.url}`"
              v-html="highlightMatches(backend.name)"
            ></a>
            <span
              v-for="feature in backend.verification"
              :key="feature"
              class="verification"
              :title="verificationLabels[feature].title"
              >{{ verificationLabels[feature].label }}</span
            >
            <span v-if="index < entry.backends.length - 1"><br /></span>
          </span>
        </td>
        <td>
          <span v-for="(os, index) in entry.os"
            >{{ os }}<span v-if="index < entry.os.length - 1">, </span>
          </span>
        </td>
      </tr>
    </tbody>
  </table>
</template>

<script>
import { data } from "/registry.data.ts";

export default {
  data() {
    return {
      filter:
        new URLSearchParams(globalThis?.location?.search).get("filter") || "",
      verifiedOnly:
        new URLSearchParams(globalThis?.location?.search).get("verified") ===
        "1",
      data: data,
      verificationLabels: {
        packslip: {
          label: "packslip",
          title: "Signed packslip release manifest",
        },
        "github-attestations": {
          label: "attestations",
          title: "GitHub artifact attestations",
        },
        slsa: { label: "SLSA", title: "SLSA provenance" },
        cosign: { label: "cosign", title: "Cosign signature" },
        minisign: { label: "minisign", title: "Minisign signature" },
      },
    };
  },
  computed: {
    filteredData() {
      const data = this.verifiedOnly
        ? this.data.filter((entry) =>
            entry.backends.some((b) => b.verification.length > 0),
          )
        : this.data;
      if (this.filter.trim() === "") return data;
      return data.filter((entry) => {
        const searchTerm = this.filter.toLowerCase();
        const short = entry.short.toString().toLowerCase();

        return (
          short.includes(searchTerm) ||
          entry.backends.some((b) => b.name.toLowerCase().includes(searchTerm))
        );
      });
    },
  },
  watch: {
    filter(newFilter = "") {
      const url = new URL(window.location);
      url.hash = "tools";
      if (newFilter.trim() === "") {
        url.searchParams.delete("filter");
      } else {
        url.searchParams.set("filter", newFilter);
      }
      window.history.pushState({}, "", url);
    },
    verifiedOnly(verifiedOnly) {
      const url = new URL(window.location);
      url.hash = "tools";
      if (verifiedOnly) {
        url.searchParams.set("verified", "1");
      } else {
        url.searchParams.delete("verified");
      }
      window.history.pushState({}, "", url);
    },
  },
  methods: {
    highlightMatches(text) {
      if (this.filter.trim() === "") return text;
      const matchExists = text
        .toLowerCase()
        .includes(this.filter.toLowerCase());
      if (!matchExists) return text;

      const re = new RegExp(this.filter, "ig");
      return text.replace(
        re,
        (matchedText) =>
          `<span style="background-color: rgba(173, 216, 230, 0.2)">${matchedText}</span>`,
      );
    },
  },
};
</script>

<style scoped>
.filter {
  width: 100%;
  padding: 10px;
  margin-bottom: 10px;
  border-radius: 10px;
  background: var(--vp-c-bg-soft);
  font-size: 15px;
  color: var(--vp-c-text-2);
}

.full-width {
  width: 100%;
  table-layout: fixed;
  min-height: 500px;
}

.full-width th:nth-child(1),
.full-width td:nth-child(1) {
  min-width: 25%;
  width: 25%;
}

.full-width th:nth-child(2),
.full-width td:nth-child(2) {
  min-width: 55%;
  width: 55%;
}

.full-width th:nth-child(3),
.full-width td:nth-child(3) {
  min-width: 20%;
}

.full-width th,
.full-width td {
  word-wrap: break-word; /* Allows text to wrap within cells */
}

.verified-filter {
  display: block;
  margin-bottom: 10px;
  font-size: 14px;
  color: var(--vp-c-text-2);
}

.mise-versions {
  display: block;
  font-size: 12px;
  color: var(--vp-c-text-2);
}

.verification {
  display: inline-block;
  margin-left: 6px;
  padding: 0 6px;
  border-radius: 8px;
  background: var(--vp-c-green-soft);
  color: var(--vp-c-green-1);
  font-size: 12px;
  line-height: 18px;
  white-space: nowrap;
}

.no-matches {
  text-align: center;
  font-style: italic;
  color: var(--vp-c-text-2);
}
</style>
