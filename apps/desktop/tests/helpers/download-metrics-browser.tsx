import React, { act, StrictMode, useLayoutEffect } from "react";
import { createRoot } from "react-dom/client";
import type { DownloadSnapshot } from "../../src/download-rate";
import { useDownloadMetrics } from "../../src/hooks/useDownloadMetrics";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const fixture = document.getElementById("fixture");
if (!fixture) throw new Error("Missing fixture root");
const root = createRoot(fixture);
const originalNow = Object.getOwnPropertyDescriptor(performance, "now");
const originalSetInterval = window.setInterval;
const originalClearInterval = window.clearInterval;
let now = 0;
let nextInterval = 1;
const intervals = new Map<number, () => void>();
Object.defineProperty(performance, "now", { configurable: true, value: () => now });
window.setInterval = (callback, delay, ...args) => {
  if (typeof callback !== "function" || delay !== 1000) throw new Error("Unexpected fixture interval");
  const id = nextInterval++;
  intervals.set(id, () => callback(...args));
  return id;
};
window.clearInterval = (id) => { if (id !== undefined) intervals.delete(id); };

type Metrics = ReturnType<typeof useDownloadMetrics>;
const commits: Metrics[] = [];
let domChanges = 0;
const mutationObserver = new MutationObserver((records) => { domChanges += records.length; });
mutationObserver.observe(fixture, { subtree: true, childList: true, characterData: true });
let scenarios = 0;

function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}
function Harness({ snapshot, observedAt }: { snapshot: DownloadSnapshot; observedAt?: number }) {
  const metrics = useDownloadMetrics(snapshot, observedAt);
  // Layout effects observe every DOM commit, before a passive effect can hide
  // a transient value. Checking only the final act() result misses 1 -> 2 -> 1.
  useLayoutEffect(() => {
    check(fixture?.querySelector("output")?.textContent === String(metrics.elapsedSeconds), "Committed elapsed text differs from metrics");
    commits.push(metrics);
  });
  return <output aria-label="Elapsed seconds">{metrics.elapsedSeconds}</output>;
}
async function render(snapshot: DownloadSnapshot | null, observedAt?: number) {
  await act(async () => {
    root.render(<StrictMode>{snapshot && <Harness snapshot={snapshot} observedAt={observedAt} />}</StrictMode>);
  });
}
async function tick(time: number) {
  now = time;
  await act(async () => { for (const callback of [...intervals.values()]) callback(); });
}
function expectCommits(start: number, expected: Metrics, description: string) {
  const actual = commits.slice(start);
  check(actual.length > 0, `${description}: no DOM commit`);
  check(actual.every((value) => value.elapsedSeconds === expected.elapsedSeconds
    && value.extraSeconds === expected.extraSeconds && value.bytesPerSecond === expected.bytesPerSecond),
  `${description}: ${JSON.stringify({ expected, actual })}`);
}
function progress(elapsed_seconds: number, downloaded_bytes: number, operation_id = "ark-install"): DownloadSnapshot {
  return { operation_id, active: true, phase: "downloading", downloaded_bytes, total_bytes: 10000, elapsed_seconds };
}
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  await act(async () => { root.unmount(); });
  mutationObserver.disconnect();
  if (originalNow) Object.defineProperty(performance, "now", originalNow);
  else Reflect.deleteProperty(performance, "now");
  window.setInterval = originalSetInterval;
  window.clearInterval = originalClearInterval;
  return { browser_errors: errors, native_dialogs: 0, remaining_intervals: intervals.size };
} });

async function run() {
  await render(progress(0, 0), 0);
  check(intervals.size === 1, "StrictMode must leave exactly one observation timer");
  await tick(1000);
  let start = commits.length;
  now = 1056;
  await render(progress(1, 100), now);
  expectCommits(start, { elapsedSeconds: 1, extraSeconds: 0, bytesPerSecond: 100 }, "A fresh native snapshot must never display 2 before 1");
  scenarios++;

  await tick(4056);
  start = commits.length;
  await render(progress(1, 100), 1056);
  expectCommits(start, { elapsedSeconds: 4, extraSeconds: 3, bytesPerSecond: null }, "Cached progress must retain its clock and stale speed during failed polls");
  scenarios++;

  start = commits.length;
  await render(progress(4, 400), now);
  expectCommits(start, { elapsedSeconds: 4, extraSeconds: 0, bytesPerSecond: 100 }, "Resuming progress must replace the stale observation before commit");
  scenarios++;

  await tick(5056);
  start = commits.length;
  await render(progress(4, 400), now);
  expectCommits(start, { elapsedSeconds: 4, extraSeconds: 0, bytesPerSecond: 100 }, "A new observation anchor must take effect even when native fields are unchanged");
  scenarios++;

  await render(null);
  check(intervals.size === 0, "Unmount must release its local timer");
  now = 9056;
  start = commits.length;
  await render(progress(4, 400), 5056);
  expectCommits(start, { elapsedSeconds: 8, extraSeconds: 4, bytesPerSecond: null }, "A remounted detail must retain the shared observation anchor");
  scenarios++;

  start = commits.length;
  const completed = { ...progress(7, 400), active: false, phase: "ready" };
  await render(completed, 9056);
  check(intervals.size === 0, "A terminal snapshot must stop the timer");
  now = 15000;
  await render({ ...completed }, 9056);
  await render(null);
  await render(completed, 9056);
  expectCommits(start, { elapsedSeconds: 7, extraSeconds: 0, bytesPerSecond: null }, "Native terminal elapsed time must remain frozen across renders and remounts");
  scenarios++;

  await render(null);
  now = 20000;
  await render(progress(10, 1000, "steamcmd"));
  await tick(21000);
  start = commits.length;
  now = 21056;
  await render(progress(11, 1100, "steamcmd"));
  expectCommits(start, { elapsedSeconds: 11, extraSeconds: 0, bytesPerSecond: 100 }, "Downloads without a shared anchor must also discard the previous extra second before commit");
  scenarios++;

  await tick(25056);
  start = commits.length;
  await render(progress(11, 1100, "steamcmd"));
  expectCommits(start, { elapsedSeconds: 15, extraSeconds: 4, bytesPerSecond: null }, "Cloning a cached download must not restart its local clock");
  start = commits.length;
  await render(progress(0, 0, "another-download"));
  expectCommits(start, { elapsedSeconds: 0, extraSeconds: 0, bytesPerSecond: null }, "A new operation cannot inherit the previous operation's extra seconds");
  scenarios++;

  check(errors.length === 0, "Browser emitted unexpected errors");
  return { status: "passed", scenarios, commits: commits.length, dom_changes: domChanges, browser_errors: errors };
}

void run().catch((error) => ({ status: "failed", scenarios, error: String(error), browser_errors: errors }))
  .then((report) => {
    const nonce = new URLSearchParams(location.search).get("nonce");
    return fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) });
  });
