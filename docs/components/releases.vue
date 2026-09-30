<script setup lang="ts">
import { computed, ref } from "vue";
import { data } from "../releases.data";

type Release = (typeof data)[number];

const releases = data;
const newestFirst = [...releases].reverse();

const totals = computed(() => {
  let changes = 0;
  let issues = 0;
  for (const r of releases) {
    changes += r.changes;
    issues += r.issues ?? 0;
  }
  return { releases: releases.length, changes, issues };
});

const maxChanges = Math.max(...releases.map((r) => r.changes));
const maxIssues = Math.max(...releases.map((r) => r.issues ?? 0));

// The releases on a chart's x axis are equally spaced, so a label sits at the
// first release of each month that starts a quarter.
const ticks = computed(() => {
  const out: { index: number; label: string }[] = [];
  let last = "";
  releases.forEach((r, index) => {
    const month = r.date.slice(0, 7);
    if (month === last) return;
    last = month;
    const m = Number(month.slice(5));
    if ((m - 1) % 3 === 0) {
      out.push({
        index,
        label: m === 1 ? month.slice(0, 4) : MONTHS[m - 1],
      });
    }
  });
  return out;
});

const MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
];

function monthTitle(month: string) {
  return `${MONTHS[Number(month.slice(5)) - 1]} ${month.slice(0, 4)}`;
}

function describe(r: Release) {
  const parts = [`${r.changes} change${r.changes === 1 ? "" : "s"}`];
  if (r.issues !== null) {
    parts.push(`${r.issues} issue${r.issues === 1 ? "" : "s"} resolved`);
  }
  return parts.join(", ");
}

const releaseUrl = (r: Release) =>
  `https://github.com/jdx/mise/releases/tag/v${r.version}`;

// The release under the pointer or keyboard focus; the newest when none is.
const active = ref<Release | null>(null);
const shown = computed(() => active.value ?? releases[releases.length - 1]);

// Months are listed newest first. Older ones sit behind a button; the charts
// above always cover everything.
const months = computed(() => {
  const groups: {
    month: string;
    releases: Release[];
    changes: number;
    issues: number;
  }[] = [];
  for (const r of newestFirst) {
    const month = r.date.slice(0, 7);
    let group = groups[groups.length - 1];
    if (group?.month !== month) {
      group = { month, releases: [], changes: 0, issues: 0 };
      groups.push(group);
    }
    group.releases.push(r);
    group.changes += r.changes;
    group.issues += r.issues ?? 0;
  }
  return groups;
});
const showAll = ref(false);
const visibleMonths = computed(() =>
  showAll.value ? months.value : months.value.slice(0, 3),
);

const chartHeight = 140;
const barHeight = (value: number, max: number) =>
  value === 0 ? 0 : Math.max(2, (value / max) * chartHeight);
</script>

<template>
  <div class="releases">
    <dl class="totals">
      <div>
        <dd>{{ totals.releases.toLocaleString("en") }}</dd>
        <dt>releases</dt>
      </div>
      <div>
        <dd>{{ totals.changes.toLocaleString("en") }}</dd>
        <dt>changes</dt>
      </div>
      <div>
        <dd>{{ totals.issues.toLocaleString("en") }}</dd>
        <dt>issues resolved</dt>
      </div>
    </dl>

    <p class="readout" aria-live="polite">
      <a :href="releaseUrl(shown)">{{ shown.version }}</a>
      <span class="date">{{ shown.date }}</span>
      <span>{{ describe(shown) }}</span>
      <span class="breakdown">
        {{ shown.categories.features }} features ·
        {{ shown.categories.fixes }} fixes ·
        {{ shown.categories.registry }} registry ·
        {{ shown.categories.other }} other
      </span>
    </p>

    <figure class="chart changes">
      <figcaption>Changes per release</figcaption>
      <div class="plot" :style="{ height: chartHeight + 'px' }">
        <span class="axis max">{{ maxChanges }}</span>
        <span class="axis zero">0</span>
        <button
          v-for="r in releases"
          :key="r.version"
          class="bar"
          type="button"
          :aria-label="`${r.version}, ${r.date}: ${describe(r)}`"
          :style="{ height: barHeight(r.changes, maxChanges) + 'px' }"
          :class="{ on: shown === r }"
          @mouseenter="active = r"
          @focus="active = r"
          @blur="active = null"
          @mouseleave="active = null"
          @click="active = r"
        ></button>
      </div>
      <div class="ticks" aria-hidden="true">
        <span
          v-for="t in ticks"
          :key="t.index"
          :class="{ year: /^\d/.test(t.label) }"
          :style="{ left: (t.index / releases.length) * 100 + '%' }"
          >{{ t.label }}</span
        >
      </div>
    </figure>

    <figure class="chart issues">
      <figcaption>Issues resolved per release</figcaption>
      <div class="plot" :style="{ height: chartHeight * 0.6 + 'px' }">
        <span class="axis max">{{ maxIssues }}</span>
        <span class="axis zero">0</span>
        <button
          v-for="r in releases"
          :key="r.version"
          class="bar"
          type="button"
          tabindex="-1"
          aria-hidden="true"
          :style="{
            height: barHeight(r.issues ?? 0, maxIssues) * 0.6 + 'px',
          }"
          :class="{ on: shown === r }"
          @mouseenter="active = r"
          @mouseleave="active = null"
          @click="active = r"
        ></button>
      </div>
      <div class="ticks" aria-hidden="true">
        <span
          v-for="t in ticks"
          :key="t.index"
          :class="{ year: /^\d/.test(t.label) }"
          :style="{ left: (t.index / releases.length) * 100 + '%' }"
          >{{ t.label }}</span
        >
      </div>
    </figure>

    <p class="note">
      An issue counts when a pull request in the release closed it. The issue
      tracker was turned off for a long stretch before September 2026, and
      reports from that time came in as Discussions, which a pull request cannot
      close, so the counts there are far too low to compare.
    </p>

    <div class="columns" aria-hidden="true">
      <span>Release</span>
      <span class="size-label">Changes</span>
      <span class="count">Total</span>
      <span class="count issue-count">Issues</span>
    </div>

    <section v-for="group in visibleMonths" :key="group.month" class="month">
      <h3>
        {{ monthTitle(group.month) }}
        <span class="month-totals">
          {{ group.releases.length }} releases · {{ group.changes }} changes ·
          {{ group.issues }} issues
        </span>
      </h3>
      <ul>
        <li v-for="r in group.releases" :key="r.version">
          <a :href="releaseUrl(r)" class="version">{{ r.version }}</a>
          <span class="date">{{ r.date }}</span>
          <span class="size" aria-hidden="true">
            <span
              :style="{ width: (r.changes / maxChanges) * 100 + '%' }"
            ></span>
          </span>
          <span class="count">{{ r.changes }}</span>
          <span class="count issue-count">{{ r.issues ?? "–" }}</span>
        </li>
      </ul>
    </section>
    <button
      v-if="!showAll && months.length > visibleMonths.length"
      type="button"
      class="more"
      @click="showAll = true"
    >
      Show {{ months.length - visibleMonths.length }} older months
    </button>
  </div>
</template>

<style scoped>
.releases {
  --changes: var(--vp-c-brand-1);
  --issues: #2f7ea1;
  --grid: var(--vp-c-divider);
}
.dark .releases {
  --changes: #d8709b;
  --issues: #5cb3d6;
}

.totals {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 40px;
  margin: 24px 0;
}
.totals dd {
  margin: 0;
  font-size: 32px;
  font-weight: 600;
  line-height: 1.1;
  font-variant-numeric: tabular-nums;
}
.totals dt {
  color: var(--vp-c-text-2);
  font-size: 14px;
}

.readout {
  display: flex;
  flex-wrap: wrap;
  gap: 2px 12px;
  margin: 0 0 12px;
  min-height: 3em;
  font-size: 14px;
  font-variant-numeric: tabular-nums;
}
.readout a {
  font-weight: 600;
}
.readout .date,
.readout .breakdown {
  color: var(--vp-c-text-2);
}

.chart {
  margin: 0 0 8px;
}
.chart figcaption {
  margin-bottom: 4px;
  color: var(--vp-c-text-2);
  font-size: 13px;
}
.plot {
  position: relative;
  display: flex;
  align-items: flex-end;
  gap: 1px;
  margin-left: 28px;
  border-bottom: 1px solid var(--grid);
  background: linear-gradient(var(--grid), var(--grid)) top / 100% 1px no-repeat;
}
.axis {
  position: absolute;
  left: -28px;
  width: 24px;
  color: var(--vp-c-text-3);
  font-size: 11px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}
.axis.max {
  top: -6px;
}
.axis.zero {
  bottom: -6px;
}
.bar {
  flex: 1 1 0;
  min-width: 0;
  padding: 0;
  border: 0;
  border-radius: 2px 2px 0 0;
  background: var(--changes);
  opacity: 0.75;
  cursor: pointer;
}
.issues .bar {
  background: var(--issues);
}
.bar.on,
.bar:hover,
.bar:focus-visible {
  opacity: 1;
}
.bar:focus-visible {
  outline: 2px solid var(--vp-c-text-1);
  outline-offset: 1px;
}
.ticks {
  position: relative;
  height: 18px;
  margin: 4px 0 0 28px;
  color: var(--vp-c-text-3);
  font-size: 11px;
}
.ticks span {
  position: absolute;
}

.note {
  margin: 8px 0 24px;
  color: var(--vp-c-text-2);
  font-size: 13px;
}

.columns {
  display: grid;
  grid-template-columns: 6.5em 6.5em 1fr 3em 3em;
  gap: 8px;
  color: var(--vp-c-text-2);
  font-size: 12px;
}
.columns .size-label {
  grid-column: 2 / 4;
}
.month h3 {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 0 16px;
  margin: 28px 0 8px;
  font-size: 18px;
}
.month-totals {
  color: var(--vp-c-text-2);
  font-size: 13px;
  font-weight: 400;
}
.month ul {
  margin: 0;
  padding: 0;
  list-style: none;
}
.month li {
  display: grid;
  grid-template-columns: 6.5em 6.5em 1fr 3em 3em;
  align-items: center;
  gap: 8px;
  padding: 3px 0;
  border-top: 1px solid var(--grid);
  font-size: 14px;
  font-variant-numeric: tabular-nums;
}
.month li .date {
  color: var(--vp-c-text-2);
}
.size {
  height: 8px;
}
.size span {
  display: block;
  height: 100%;
  min-width: 2px;
  border-radius: 0 4px 4px 0;
  background: var(--changes);
  opacity: 0.75;
}
.count {
  text-align: right;
}
.issue-count {
  color: var(--issues);
}
.more {
  margin-top: 20px;
  padding: 6px 14px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  background: var(--vp-c-bg-soft);
  color: var(--vp-c-text-1);
  font-size: 14px;
  cursor: pointer;
}

/* 414 bars need about 1.5px each. Below that the gap would leave them no width,
   so they touch and the chart reads as a filled outline, and only years are
   labeled. */
@media (max-width: 800px) {
  .plot {
    gap: 0;
  }
  .bar {
    border-radius: 0;
  }
  .ticks span:not(.year) {
    display: none;
  }
}

@media (max-width: 560px) {
  .month li {
    grid-template-columns: 5.5em 1fr 2.5em 2.5em;
  }
  .month li .date {
    display: none;
  }
  .columns {
    grid-template-columns: 5.5em 1fr 2.5em 2.5em;
  }
  .columns .size-label {
    grid-column: 2;
  }
}
</style>
