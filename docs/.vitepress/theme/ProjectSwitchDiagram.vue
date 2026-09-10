<script setup lang="ts">
import { ref } from "vue";

const projects = [
  { name: "api", node: "22" },
  { name: "dashboard", node: "24" },
];
const selected = ref(projects[0]);
</script>

<template>
  <figure
    class="project-switch-diagram"
    aria-label="Project configuration follows your directory"
  >
    <div class="projects">
      <button
        v-for="project in projects"
        :key="project.name"
        type="button"
        :aria-pressed="selected.name === project.name"
        :aria-label="`Show the shell environment in ${project.name}`"
        @click="selected = project"
      >
        <span class="project-path">{{ project.name }}/mise.toml</span>
        <code
          >[tools]<br />node = "{{
            project.node
          }}"<br /><br />[env]<br />APP_ENV = "{{ project.name }}"</code
        >
        <span class="project-command">$ cd ~/work/{{ project.name }}</span>
      </button>
    </div>
    <div class="project-arrows" aria-hidden="true">
      <span v-for="project in projects" :key="project.name">{{
        selected.name === project.name ? "↓" : ""
      }}</span>
    </div>
    <div class="project-shell" aria-live="polite" aria-atomic="true">
      <div class="project-shell-bar">
        <span>Active shell</span><span>~/work/{{ selected.name }}</span>
      </div>
      <dl>
        <div>
          <dt>Node</dt>
          <dd>{{ selected.node }}</dd>
        </div>
        <div>
          <dt>APP_ENV</dt>
          <dd>{{ selected.name }}</dd>
        </div>
      </dl>
    </div>
    <figcaption>
      Choose a project to see its environment. Shell activation and installed
      tool versions are required.
    </figcaption>
  </figure>
</template>

<style scoped>
.project-switch-diagram {
  margin: 0;
  min-width: 0;
}
.projects {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}
.projects button {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 18px;
  min-width: 0;
  padding: 18px;
  text-align: left;
  color: var(--vp-c-text-1);
  background: var(--vp-c-bg-soft);
  border: 1px solid var(--vp-c-divider);
  border-radius: 10px;
  cursor: pointer;
}
.projects button[aria-pressed="true"] {
  border-color: var(--vp-c-brand-1);
  box-shadow: inset 0 0 0 1px var(--vp-c-brand-1);
}
.projects button:focus-visible {
  outline: 2px solid var(--vp-c-brand-1);
  outline-offset: 4px;
}
.project-path {
  font-size: 0.8rem;
  font-weight: 600;
  overflow-wrap: anywhere;
}
.projects code {
  padding: 0;
  color: var(--vp-c-text-1);
  background: transparent;
  font-size: 0.8rem;
  line-height: 1.7;
  white-space: normal;
  overflow-wrap: anywhere;
}
.project-command {
  margin-top: auto;
  color: var(--vp-c-brand-1);
  font-family: var(--vp-font-family-mono);
  font-size: 0.8rem;
  overflow-wrap: anywhere;
}
.project-arrows {
  display: grid;
  grid-template-columns: 1fr 1fr;
  min-height: 40px;
  align-items: center;
  text-align: center;
  color: var(--vp-c-brand-1);
  font-size: 1.5rem;
}
.project-shell {
  border: 1px solid var(--vp-c-divider);
  border-radius: 10px;
  overflow: hidden;
  background: var(--vp-c-bg-soft);
}
.project-shell-bar {
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  gap: 8px;
  padding: 12px 18px;
  border-bottom: 1px solid var(--vp-c-divider);
  color: var(--vp-c-text-2);
  font-family: var(--vp-font-family-mono);
  font-size: 0.8rem;
}
dl {
  margin: 0;
  padding: 16px 18px;
}
dl > div {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 12px;
  padding: 4px 0;
}
dt {
  color: var(--vp-c-text-2);
  font-size: 0.85rem;
}
dd {
  margin: 0;
  font-family: var(--vp-font-family-mono);
  font-size: 0.85rem;
  overflow-wrap: anywhere;
}
figcaption {
  margin-top: 14px;
  color: var(--vp-c-text-2);
  font-size: 0.85rem;
  line-height: 1.6;
}
@media (max-width: 480px) {
  .projects button {
    padding: 12px;
  }
}
</style>
