const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const desktopRoot = path.resolve(__dirname, "..");
const viewSource = fs.readFileSync(path.join(desktopRoot, "src", "views", "SystemView.tsx"), "utf8");
const metricSource = fs.readFileSync(path.join(desktopRoot, "src", "views", "system", "SystemTopMetricCard.tsx"), "utf8");
const telemetrySource = fs.readFileSync(path.join(desktopRoot, "src", "views", "system", "useSystemTelemetry.ts"), "utf8");
const summarySource = fs.readFileSync(path.join(desktopRoot, "src", "views", "system", "SystemResourceSummary.tsx"), "utf8");
const viewStyleSource = ["SystemView.css", "system/system-metrics.css", "system/system-instance-overview.css"]
  .map((filename) => fs.readFileSync(path.join(desktopRoot, "src", "views", filename), "utf8")).join("\n");
const dialSource = fs.readFileSync(path.join(desktopRoot, "src", "views", "system", "SystemCoreDial.tsx"), "utf8");
const fieldSource = fs.readFileSync(path.join(desktopRoot, "src", "views", "system", "SystemCoreField.tsx"), "utf8");
const capabilitySource = fs.readFileSync(path.join(desktopRoot, "src", "webgl-capabilities.ts"), "utf8");
const dialStyleSource = fs.readFileSync(path.join(desktopRoot, "src", "views", "system", "system-core-dial.css"), "utf8");
const packageJson = JSON.parse(fs.readFileSync(path.join(desktopRoot, "package.json"), "utf8"));

function positiveAngleDelta(start, end) {
  return (end - start + 360) % 360;
}

function extractCoreDialSectors() {
  const match = dialSource.match(/export const CORE_DIAL_SECTORS = \[([\s\S]*?)\] as const;/);
  assert.ok(match, "CORE_DIAL_SECTORS should define the four visible dial regions");
  return [...match[1].matchAll(/\{\s*start:\s*(\d+),\s*end:\s*(\d+)\s*\}/g)].map((entry) => ({
    start: Number(entry[1]),
    end: Number(entry[2])
  }));
}

test("system core is an isolated Motion-powered SVG instrument", () => {
  assert.match(viewSource, /from "\.\/system\/SystemCoreDial"/);
  assert.doesNotMatch(viewSource, /function SystemCoreDial\(/);
  assert.match(dialSource, /from "motion\/react"/);
  assert.match(dialSource, /<motion\.svg/);
  assert.match(dialSource, /<motion\.path/);
  assert.match(dialSource, /animate=\{\{ pathLength:/);
  assert.match(dialSource, /<strong>\{stateLabel\}<\/strong>/);
  assert.match(dialStyleSource, /\.system-core-band-progress/);
  assert.doesNotMatch(dialStyleSource, /system-core-band-copy/);
});

test("system core uses a data-linked WebGL2 field with a static reduced-motion mode", () => {
  assert.match(packageJson.dependencies.motion, /^\^13\./);
  assert.match(packageJson.dependencies["@react-three/fiber"], /^\^9\./);
  assert.equal(
    packageJson.dependencies.three,
    "0.185.1",
    "keep Three on the audited version accepted by the R3F 9 peer range"
  );
  assert.equal(packageJson.devDependencies["@types/three"], "0.185.4");
  assert.match(fieldSource, /from "\.\.\/\.\.\/webgl-capabilities"/);
  assert.match(capabilitySource, /getContext\("webgl2"/);
  assert.match(capabilitySource, /cachedWebGl2Support/);
  assert.match(fieldSource, /glslVersion=\{THREE\.GLSL3\}/);
  assert.match(fieldSource, /uAttention/);
  assert.doesNotMatch(fieldSource, /uHealth|healthScore/);
  assert.match(fieldSource, /uLoad/);
  assert.match(fieldSource, /frameloop=\{reducedMotion \|\| !documentVisible \? "demand" : "always"\}/);
  assert.match(fieldSource, /visibilitychange/);
  assert.match(fieldSource, /<CoreFieldWake active=\{!reducedMotion && documentVisible\} \/>/);
  assert.match(fieldSource, /return null;/);
  assert.match(fieldSource, /camera\.zoom = Math\.max\(1, Math\.min\(width, height\) \/ 2\)/);
  assert.match(fieldSource, /invalidate\(\);/);
  assert.match(fieldSource, /pointerX\.get\(\) \/ 3\.5/);
  assert.match(fieldSource, /float dialAngle = atan\(point\.x, point\.y\);/);
  assert.match(fieldSource, /linearToOutputTexel\(vec4\(color,/);
  assert.match(fieldSource, /size=\{1\.25\}/);
  assert.match(dialSource, /<SystemCoreField[\s\S]*?resourceState=\{operatingState\}[\s\S]*?load=\{peakLoad\}/);

  const numericSmoothsteps = [...fieldSource.matchAll(/smoothstep\(\s*(\d+(?:\.\d+)?)\s*,\s*(\d+(?:\.\d+)?)/g)];
  assert.deepEqual(
    numericSmoothsteps.filter((match) => Number(match[1]) > Number(match[2])).map((match) => match[0]),
    [],
    "GLSL smoothstep calls must keep edge0 <= edge1"
  );
});

test("system core dial keeps four equal sectors separated by four equal gaps", () => {
  const sectors = extractCoreDialSectors();
  assert.equal(sectors.length, 4);
  const sectorWidths = sectors.map((sector) => positiveAngleDelta(sector.start, sector.end));
  const gaps = sectors.map((sector, index) => positiveAngleDelta(sector.end, sectors[(index + 1) % sectors.length].start));
  assert.deepEqual(sectorWidths, [70, 70, 70, 70]);
  assert.deepEqual(gaps, [20, 20, 20, 20]);
  assert.match(telemetrySource, /\.\.\.CORE_DIAL_SECTORS\[index\]/);
});

test("system core shares keyboard-operable hover, focus, and pin state with telemetry cards", () => {
  assert.match(dialSource, /role="group"/);
  assert.match(dialSource, /aria-pressed=\{selected\}/);
  assert.match(dialSource, /export interface SystemCoreInteraction/);
  assert.match(dialSource, /const \{ activeTone, pinnedTone, onHoverChange, onFocusChange, onPinToggle \} = interaction/);
  assert.doesNotMatch(dialSource, /useState/);
  assert.match(viewSource, /const coreActiveTone = corePinnedTone \?\? coreFocusedTone \?\? coreHoveredTone/);
  assert.match(viewSource, /setCoreHoveredTone\(\(current\) => active \? tone : current === tone \? null : current\)/);
  assert.match(viewSource, /window\.addEventListener\("keydown", clearCorePin\)/);
  assert.match(viewSource, /window\.removeEventListener\("keydown", clearCorePin\)/);
  assert.match(viewSource, /event\.key === "Escape"[\s\S]*?setCorePinnedTone\(null\)/);
  assert.match(metricSource, /function TopMetricCard[\s\S]*?<button[\s\S]*?aria-pressed=\{selected\}/);
  assert.match(viewSource, /interaction=\{coreInteraction\}/);
  assert.match(dialSource, /event\.currentTarget\.matches\(":focus-visible"\)/);
  assert.match(metricSource, /event\.currentTarget\.matches\(":focus-visible"\)/);
  assert.match(dialSource, /MotionConfig reducedMotion="user"/);
  assert.match(dialSource, /useReducedMotion\(\)/);
  assert.match(dialSource, /transition=\{reduceMotion[\s\S]*?\? \{ duration: 0 \}[\s\S]*?type: "spring"/);
  assert.match(dialSource, /const rawId = useId\(\)/);
  assert.match(dialStyleSource, /@media \(prefers-reduced-motion: reduce\)/);
  assert.match(dialStyleSource, /\.system-core-metric:focus-visible/);
});

test("system core phase motion is localized and never uses a decorative full-turn scanner", () => {
  assert.match(dialSource, /className="system-core-phase-lock"/);
  assert.match(dialSource, /className="system-core-phase-rail"/);
  assert.match(dialStyleSource, /\.system-core-phase-rail/);
  assert.doesNotMatch(dialSource, /system-core-scan/);
  assert.doesNotMatch(dialStyleSource, /system-core-scan|rotate\(360deg\)/);
  assert.doesNotMatch(fieldSource, /scanner|scanAngle/);
  assert.match(fieldSource, /phaseSignal/);
});

test("system telemetry cards use real sample history, distinct visual grammar, and dial-aligned placement", () => {
  assert.match(telemetrySource, /function useTelemetryHistory/);
  assert.match(telemetrySource, /previousSample\.current !== sampledAt/);
  assert.match(telemetrySource, /current\[key\] === null \? \[\] : isNewSample \? \[\.\.\.previous\[key\], current\[key\]\]\.slice\(-30\)/);
  assert.doesNotMatch(telemetrySource, /buildSeries|NETWORK_ACTIVITY_CEILING_BPS/);
  assert.match(telemetrySource, /spark: receiveSpark, secondarySpark: transmitSpark/);
  assert.match(telemetrySource, /receive: resources\.network\.receiveBps/);
  assert.match(telemetrySource, /transmit: resources\.network\.transmitBps/);
  assert.match(telemetrySource, /Math\.max\(1, \.\.\.history\.receive, \.\.\.history\.transmit\)/);
  assert.match(telemetrySource, /history\.receive\.map\(\(value\) => value \/ networkHistoryPeak \* 100\)/);
  assert.match(telemetrySource, /history\.transmit\.map\(\(value\) => value \/ networkHistoryPeak \* 100\)/);
  assert.match(metricSource, /metric\.tone === "network"[\s\S]*?<MiniSparkline[\s\S]*?metric\.percent === null/);
  assert.match(metricSource, /selected \? `\. \$\{lockedLabel\}` : active \? `\. \$\{linkedLabel\}` : ""/);
  assert.match(metricSource, /metric\.tone === "memory"[\s\S]*?system-top-metric-composition/);
  assert.match(metricSource, /metric\.tone === "disk"[\s\S]*?system-top-metric-threshold/);
  assert.match(dialSource, /channelBands\.find\(\(candidate\) => candidate\.tone === item\.tone\)/);
  assert.match(viewStyleSource, /"core network disk"/);
  assert.match(viewStyleSource, /\.system-top-metric-load-rail/);
});

test("system core reports telemetry without redundant panel header states", () => {
  assert.match(viewSource, /title=\{copy\.coreTitle\}[\s\S]*?eyebrow=\{copy\.coreMeta\}\s*\/>/);
  assert.match(viewSource, /title=\{copy\.instance\}[\s\S]*?eyebrow=\{copy\.instanceMeta\}\s*aside=\{\s*<div className="system-instance-summary-bar">/);
  assert.doesNotMatch(viewSource, /system-state-chip|system-instance-status|system-link-rate/);
  assert.match(viewSource, /operatingState=\{resources\.state\}/);
  assert.match(telemetrySource, /telemetryState: metric\.state/);
  assert.match(telemetrySource, /state === "normal" \? sampled : resourceStateLabel\(locale, state\)/);
  assert.match(dialSource, /system-core-metric-live is-\$\{telemetryState\}/);
  assert.match(dialSource, /activeBand\?\.telemetryLabel \|\| sampleStateLabel/);
  assert.doesNotMatch(dialSource, /<i \/> LIVE/);
});

test("system dashboard fills the shell without a page scrollbar", () => {
  assert.match(
    viewStyleSource,
    /\.shell-content-scroll:has\(\.system-dashboard-page\)\s*\{[\s\S]*?overflow-y:\s*hidden[\s\S]*?scrollbar-gutter:\s*auto/
  );
  assert.match(
    viewStyleSource,
    /\.shell-content-body:has\(\.system-dashboard-page\)\s*\{[\s\S]*?height:\s*100%[\s\S]*?overflow:\s*hidden/
  );
  assert.match(viewStyleSource, /\.system-dashboard-page\s*\{[\s\S]*?height:\s*100%[\s\S]*?overflow:\s*hidden/);
  assert.match(viewStyleSource, /grid-template-rows:\s*114px 114px minmax\(0,\s*1fr\) auto/);
  assert.match(viewStyleSource, /\.system-core-panel\s*\{[\s\S]*?minmax\(0,\s*1fr\)/);
  assert.match(viewStyleSource, /\.system-instance-list\s*\{[\s\S]*?min-height:\s*0[\s\S]*?overflow-y:\s*auto/);
  assert.doesNotMatch(viewStyleSource, /\.system-instance-list\s*\{[^}]*max-height:/);
  assert.match(
    viewStyleSource,
    /@media \(max-width: 1020px\)[\s\S]*?\.shell-content-scroll:has\(\.system-dashboard-page\)\s*\{[\s\S]*?overflow-y:\s*auto/
  );
});

test("system dashboard reflows a narrow desktop without forcing horizontal scroll", () => {
  assert.match(viewStyleSource, /@media \(max-width: 1020px\)[\s\S]*?"cpu memory" "network disk" "instance instance" "core core" "logs logs"/);
  assert.doesNotMatch(viewStyleSource, /(?:^|\n)\s*(?:min-)?width:\s*1180px;/);
  assert.match(dialStyleSource, /\.system-core-visual\s*\{[\s\S]*?min-width:\s*0;/);
});

test("system dashboard promotes a navigable instance overview without a duplicate bottleneck panel", () => {
  assert.doesNotMatch(viewSource, /className="system-performance-panel"/);
  assert.doesNotMatch(viewStyleSource, /grid-area:\s*performance/);
  assert.match(viewSource, /instance:\s*"实例概览"/);
  assert.match(viewSource, /orderedInstances\.map\(\(instance\)/);
  assert.match(viewSource, /onClick=\{\(\) => props\.onOpenInstance\(instance\.id\)\}/);
  assert.match(viewStyleSource, /"core instance instance"/);
  assert.match(viewStyleSource, /\.system-instance-list\s*\{[\s\S]*?grid-template-columns:\s*repeat\(2,/);
});

test("system dashboard consolidates network history and leaves room for path actions", () => {
  assert.doesNotMatch(viewSource, /className="system-link-panel"|function LinkFlowChart/);
  assert.match(telemetrySource, /detail: `RX \$\{formatRate\(locale, resources\.network\.receiveBps\)\} \/ TX \$\{formatRate\(locale, resources\.network\.transmitBps\)\}/);
  assert.match(viewStyleSource, /"logs logs logs"/);
});

test("core instrument keeps numeric readings complete in its compact presentation", () => {
  for (const selector of ["system-core-metric-value", "system-core-center-value"]) {
    const rule = dialStyleSource.match(new RegExp(`\\.${selector} strong \\{([^}]+)\\}`));
    assert.ok(rule, `${selector} must have explicit value typography`);
    assert.doesNotMatch(rule[1], /text-overflow:\s*ellipsis|overflow:\s*hidden/);
    assert.match(rule[1], /--core-value-length/);
  }
  assert.match(dialStyleSource, /data-wide-unit="true"[\s\S]*?flex-basis:\s*100%/);
  assert.doesNotMatch(dialSource, /className="system-core-center-meta"/);
  assert.match(viewSource, /<SystemResourceSummary assessment=\{resources\} locale=\{locale\} refreshing=\{props\.systemRefreshing\} \/>/);
});

test("resource status preserves unknown measurements and reports independent headroom", () => {
  assert.doesNotMatch(viewSource + dialSource + fieldSource + telemetrySource, /healthScore|safeMargin|runningRatio|NETWORK_ACTIVITY_CEILING_BPS/);
  assert.match(dialSource, /item\.percent !== null \? <motion\.path/);
  assert.match(dialSource, /channel && channel\.percent !== null/);
  assert.match(metricSource, /if \(metric\.percent === null\) return null/);
  assert.match(summarySource, /assessment\.memoryAvailableBytes/);
  assert.match(summarySource, /assessment\.memoryCommitAvailableBytes/);
  assert.match(summarySource, /assessment\.volumes\.map/);
  assert.doesNotMatch(summarySource, /<details|<summary|Check the selected game|启动新实例仍需/);
  assert.doesNotMatch(viewStyleSource, /system-resource-headroom\[open\]/);
  assert.match(summarySource, /<time dateTime=/);
});
