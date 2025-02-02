<template>
  <input
    class="filter"
    type="text"
    placeholder="Filter by Short or Full"
    v-model="filter"
  />
  <table class="filtered-table">
    <thead>
      <tr>
        <th>Short</th>
        <th>Full</th>
        <th>OS</th>
      </tr>
    </thead>
    <tbody>
      <tr v-for="(entry, index) in filteredData" :key="`backend-${index}`">
        <td v-html="highlightMatches(entry.short)"></td>
        <td>
          <span v-for="(backend, index) in entry.backends">
            <a
              :href="`${backend.url}`"
              v-html="highlightMatches(backend.name)"
            ></a>
            <span v-if="index < entry.backends.length - 1">,<br /></span>
          </span>
        </td>
        <td>
          <span v-for="(os, index) in entry.os">
            {{ os }}
            <span v-if="index < entry.os.length - 1">, </span>
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
      filter: "",
      data: data,
    };
  },
  computed: {
    filteredData() {
      if (this.filter.trim() === '') return this.data;
      return this.data.filter(entry => {
        const searchTerm = this.filter.toLowerCase();
        const short = entry.short.toString().toLowerCase();

        return (
          short.includes(searchTerm) ||
          entry.backends.some((b) => b.name.toLowerCase().includes(searchTerm))
        );
      });
    },
  },
  methods: {
    highlightMatches(text) {
      if (this.filter.trim() === '') return text;
      const matchExists = text.toLowerCase().includes(this.filter.toLowerCase());
      if (!matchExists) return text;

      const re = new RegExp(this.filter, "ig");
      return text.replace(
        re,
        (matchedText) => `<strong>${matchedText}</strong>`,
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
  border: 1px solid #ccc;
  border-radius: 5px;
}
.filtered-table {
  display: table;
  width: 100%;
}
</style>
