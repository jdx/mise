<template>
  <input
    class="filter"
    type="text"
    placeholder="Filter by Short or Full"
    v-model="filter"
    autofocus="autofocus"
  />
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
        <td v-html="highlightMatches(entry.short)"></td>
        <td>
          <span v-for="(backend, index) in entry.backends">
            <a
              v-if="backend.url"
              :href="`${backend.url}`"
              v-html="highlightMatches(backend.name)"
            ></a>
            <span
              v-else
              v-html="highlightMatches(backend.name)"
            ></span>
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
      data: data,
    };
  },
  computed: {
    filteredData() {
      if (this.filter.trim() === "") return this.data;
      return this.data.filter((entry) => {
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
  min-width: 40%;
  width: 50%;
}

.full-width th:nth-child(2),
.full-width td:nth-child(2) {
  min-width: 40%;
  width: 50%;
}

.full-width th:nth-child(3),
.full-width td:nth-child(3) {
  min-width: 20%;
}

.full-width th,
.full-width td {
  word-wrap: break-word; /* Allows text to wrap within cells */
}

.no-matches {
  text-align: center;
  font-style: italic;
  color: var(--vp-c-text-2);
}
</style>
