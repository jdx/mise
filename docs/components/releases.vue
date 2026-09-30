<script setup lang="ts">
import { computed, nextTick, onMounted, reactive, ref, shallowRef } from "vue";
import { withBase } from "vitepress";
import { ISSUES_SINCE } from "../.vitepress/releases.mjs";
import { data } from "../releases.data";

type Release = (typeof data)[number];

const releases = data;
const newestFirst = [...releases].reverse();

// Issue counts start when GitHub Issues came back; see ISSUES_SINCE.
const hasIssueCount = (r: Release) => r.date >= ISSUES_SINCE;
const issueReleases = releases.filter(hasIssueCount);

const totals = computed(() => {
  let changes = 0;
  for (const r of releases) changes += r.changes;
  let issues = 0;
  let uncounted = 0;
  for (const r of issueReleases) {
    issues += r.issues ?? 0;
    if (r.issues === null) uncounted++;
  }
  return { releases: releases.length, changes, issues, uncounted };
});

const maxChanges = Math.max(...releases.map((r) => r.changes));
// A release whose issues were not counted adds nothing to a total, so a total
// that is missing some reads as a lower bound.
const issueTotal = (issues: number, uncounted: number) =>
  uncounted ? `${issues.toLocaleString("en")}+` : issues.toLocaleString("en");

const maxIssues = Math.max(1, ...issueReleases.map((r) => r.issues ?? 0));

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

const dayNumber = (date: string) => Date.parse(date) / 86_400_000;

// The releases on a chart's x axis are equally spaced. A long chart labels the
// first release of each quarter; a short one (the issues chart starts small)
// labels about every week.
function ticksFor(list: Release[]) {
  const out: { index: number; label: string; year?: boolean }[] = [];
  if (!list.length) return out;
  const span = dayNumber(list[list.length - 1].date) - dayNumber(list[0].date);
  if (span > 120) {
    let last = "";
    list.forEach((r, index) => {
      const month = r.date.slice(0, 7);
      if (month === last) return;
      last = month;
      const m = Number(month.slice(5));
      if ((m - 1) % 3 === 0) {
        out.push({
          index,
          label: m === 1 ? month.slice(0, 4) : MONTHS[m - 1],
          year: m === 1,
        });
      }
    });
    return out;
  }
  let lastDay = -Infinity;
  list.forEach((r, index) => {
    const day = dayNumber(r.date);
    if (day - lastDay < 7) return;
    lastDay = day;
    out.push({
      index,
      label: `${MONTHS[Number(r.date.slice(5, 7)) - 1]} ${Number(r.date.slice(8))}`,
      year: true,
    });
  });
  return out;
}
const changeTicks = ticksFor(releases);
const issueTicks = ticksFor(issueReleases);

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
// The readout shows the release under the pointer or keyboard focus, then the
// one last clicked, then the newest. Keeping the clicked one lets the pointer
// leave a bar and reach the readout's link without it changing. These are
// shallow refs so the releases stay the same objects the template loops over,
// which the `shown === r` checks rely on.
const hovered = shallowRef<Release | null>(null);
const selected = shallowRef<Release | null>(null);
const shown = computed(
  () => hovered.value ?? selected.value ?? releases[releases.length - 1],
);

// Months are listed newest first. Older ones sit behind a button; the charts
// above always cover everything.
const months = computed(() => {
  const groups: {
    month: string;
    releases: Release[];
    changes: number;
    issues: number;
    uncounted: number;
    counted: boolean;
  }[] = [];
  for (const r of newestFirst) {
    const month = r.date.slice(0, 7);
    let group = groups[groups.length - 1];
    if (group?.month !== month) {
      group = {
        month,
        releases: [],
        changes: 0,
        issues: 0,
        uncounted: 0,
        counted: false,
      };
      groups.push(group);
    }
    group.releases.push(r);
    group.changes += r.changes;
    if (hasIssueCount(r)) {
      group.counted = true;
      group.issues += r.issues ?? 0;
      if (r.issues === null) group.uncounted++;
    }
  }
  return groups;
});
const showAll = ref(false);
const visibleMonths = computed(() =>
  showAll.value ? months.value : months.value.slice(0, 3),
);

// Releases whose notes are open, and the notes themselves. A release's notes are
// fetched when it is first opened (the notes of all of them are too big to put
// in the page), as HTML the docs build rendered from its GitHub release.
type Notes =
  | { state: "loading" }
  | { state: "failed" }
  | { state: "ready"; title: string; html: string };
const opened = reactive(new Set<string>());
const notes = reactive(new Map<string, Notes>());

async function loadNotes(r: Release) {
  if (!r.notes || notes.has(r.version)) return;
  notes.set(r.version, { state: "loading" });
  try {
    const res = await fetch(withBase(`/release-notes/${r.version}.json`));
    if (!res.ok) throw new Error(res.statusText);
    const { title, html } = await res.json();
    notes.set(r.version, { state: "ready", title, html });
  } catch {
    notes.set(r.version, { state: "failed" });
  }
}

function onToggle(r: Release, event: Event) {
  if ((event.target as HTMLDetailsElement).open) {
    opened.add(r.version);
    loadNotes(r);
  } else {
    opened.delete(r.version);
  }
}

// Open a release's notes and bring its row into view, expanding the list first
// when its month is one of the older ones.
async function openRelease(r: Release) {
  const month = r.date.slice(0, 7);
  if (!visibleMonths.value.some((g) => g.month === month)) showAll.value = true;
  selected.value = r;
  opened.add(r.version);
  loadNotes(r);
  await nextTick();
  document
    .getElementById(`release-${r.version}`)
    ?.scrollIntoView({ block: "center" });
  history.replaceState(null, "", `#${r.version}`);
}
onMounted(() => {
  const version = decodeURIComponent(location.hash.slice(1));
  const r = releases.find((r) => r.version === version);
  if (r) openRelease(r);
});

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
        <dd>
          {{ issueTotal(totals.issues, totals.uncounted) }}
        </dd>
        <dt>issues resolved since Sep 23, 2026</dt>
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
          @mouseenter="hovered = r"
          @focus="hovered = r"
          @blur="hovered = null"
          @mouseleave="hovered = null"
          @click="openRelease(r)"
        ></button>
      </div>
      <div class="ticks" aria-hidden="true">
        <span
          v-for="t in changeTicks"
          :key="t.index"
          :class="{ year: t.year }"
          :style="{ left: (t.index / releases.length) * 100 + '%' }"
          >{{ t.label }}</span
        >
      </div>
    </figure>

    <figure v-if="issueReleases.length" class="chart issues">
      <figcaption>Issues resolved per release, since Sep 23, 2026</figcaption>
      <div class="plot" :style="{ height: chartHeight * 0.6 + 'px' }">
        <span class="axis max">{{ maxIssues }}</span>
        <span class="axis zero">0</span>
        <button
          v-for="r in issueReleases"
          :key="r.version"
          class="bar"
          type="button"
          tabindex="-1"
          aria-hidden="true"
          :style="{
            height: barHeight(r.issues ?? 0, maxIssues) * 0.6 + 'px',
          }"
          :class="{ on: shown === r }"
          @mouseenter="hovered = r"
          @mouseleave="hovered = null"
          @click="openRelease(r)"
        ></button>
      </div>
      <div class="ticks" aria-hidden="true">
        <span
          v-for="t in issueTicks"
          :key="t.index"
          :class="{ year: t.year }"
          :style="{ left: (t.index / issueReleases.length) * 100 + '%' }"
          >{{ t.label }}</span
        >
      </div>
    </figure>

    <p class="note">
      An issue counts when a pull request in the release closed it. Counting
      starts on Sep 23, 2026, when GitHub Issues were turned back on. Before
      that, reports came in as Discussions, which a pull request cannot close,
      so earlier releases have no count.
    </p>

    <p v-if="totals.uncounted" class="note">
      A total marked + is a lower bound: {{ totals.uncounted }}
      {{ totals.uncounted === 1 ? "release has" : "releases have" }} no issue
      count yet, and the next release fills it in.
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
          {{ group.releases.length }} releases · {{ group.changes }} changes
          <template v-if="group.counted">
            · {{ issueTotal(group.issues, group.uncounted) }} issues
          </template>
        </span>
      </h3>
      <ul>
        <li v-for="r in group.releases" :key="r.version">
          <details
            :id="`release-${r.version}`"
            :open="opened.has(r.version)"
            @toggle="onToggle(r, $event)"
          >
            <summary class="row">
              <span class="version">{{ r.version }}</span>
              <span class="date">{{ r.date }}</span>
              <span class="size" aria-hidden="true">
                <span
                  :style="{ width: (r.changes / maxChanges) * 100 + '%' }"
                ></span>
              </span>
              <span class="count">{{ r.changes }}</span>
              <span class="count issue-count">{{
                hasIssueCount(r) ? (r.issues ?? "–") : "–"
              }}</span>
            </summary>
            <div v-if="opened.has(r.version)" class="notes">
              <p class="notes-head">
                <strong v-if="notes.get(r.version)?.state === 'ready'">{{
                  (notes.get(r.version) as any).title
                }}</strong>
                <a :href="releaseUrl(r)">{{ r.version }} on GitHub ↗</a>
              </p>
              <p v-if="!r.notes" class="notes-status">
                This release has no notes on GitHub.
              </p>
              <p
                v-else-if="notes.get(r.version)?.state === 'failed'"
                class="notes-status"
              >
                The notes could not be loaded. They are on GitHub.
              </p>
              <p
                v-else-if="notes.get(r.version)?.state !== 'ready'"
                class="notes-status"
              >
                Loading…
              </p>
              <div
                v-else
                class="rendered"
                v-html="(notes.get(r.version) as any).html"
              ></div>
            </div>
          </details>
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
.month ul li + li {
  margin-top: 0;
}
.month > ul > li {
  border-top: 1px solid var(--grid);
  font-size: 14px;
  font-variant-numeric: tabular-nums;
}
.row {
  display: grid;
  grid-template-columns: 6.5em 6.5em 1fr 3em 3em;
  align-items: center;
  gap: 8px;
  padding: 3px 0;
  cursor: pointer;
  list-style: none;
}
.row::-webkit-details-marker {
  display: none;
}
.row:hover .version,
.row:focus-visible .version {
  text-decoration: underline;
}
.row:focus-visible {
  outline: 2px solid var(--vp-c-brand-1);
  outline-offset: 2px;
}
.version {
  color: var(--vp-c-brand-1);
  font-weight: 500;
}
.row .date {
  color: var(--vp-c-text-2);
}
details[open] > .row .version::before {
  content: "▾ ";
}
details:not([open]) > .row .version::before {
  content: "▸ ";
  color: var(--vp-c-text-3);
}
.notes {
  margin: 4px 0 14px;
  padding: 4px 0 4px 16px;
  border-left: 2px solid var(--grid);
  font-size: 14px;
  font-variant-numeric: normal;
}
.notes-head {
  margin: 0 0 4px;
  font-size: 13px;
}
.notes-head strong {
  display: block;
  margin-bottom: 2px;
  font-size: 15px;
}
.notes-status {
  margin: 4px 0;
  color: var(--vp-c-text-2);
}
/* The notes are HTML from the docs build, so the page's own markdown styles
   apply; these only keep a release's headings in proportion to the list. */
.rendered :deep(h1),
.rendered :deep(h2),
.rendered :deep(h3) {
  margin: 16px 0 6px;
  padding-top: 0;
  border-top: 0;
  font-size: 15px;
  letter-spacing: 0;
}
.rendered :deep(p),
.rendered :deep(ul),
.rendered :deep(ol) {
  margin: 6px 0;
  line-height: 1.6;
}
.rendered :deep(div[class*="language-"]) {
  margin: 8px 0;
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
  .row {
    grid-template-columns: 5.5em 1fr 2.5em 2.5em;
  }
  .row .date {
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
