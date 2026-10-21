const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { createHash } = require("node:crypto");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const evidencePath = process.env.LANGAME_DST_MAP_EXPERIENCE_EVIDENCE_PATH;
const evidenceHash = process.env.LANGAME_DST_MAP_EXPERIENCE_EVIDENCE_SHA256;
const desktopRoot = path.resolve(__dirname, "..");
const sourceNames = [
  "tests/dst-map-experience-native-render-browser.test.cjs", "tests/helpers/dst-map-experience-native-render-browser.tsx",
  "tests/helpers/dst-map-experience-native-render-browser.html", "tests/helpers/runtime-browser-process.cjs",
  "tests/helpers/browser-locale-catalogs.ts", "src/dst-world-map.ts", "src/dst-map-view.ts", "src/dst-map-landmarks.ts",
  "tests/helpers/dst-map-layout-checks.ts", "src/views/servers/GMToolsWorkbench.tsx", "src/views/servers/GmToolsNavigation.tsx",
  "src/views/settings/ConfigurationSectionNavigation.tsx", "src/views/settings/ConfigurationFieldHelp.tsx",
  "src/views/settings/InstanceSettingsSaveContext.tsx", "src/views/settings/instance-settings-save-coordinator.ts",
  "src/styles/configuration-workspace.css", "src/styles/configuration-field-help.css", "src/views/servers/workbench/operations/gm-tools.css",
  "src/dst-map-cache.ts", "src/dst-map-session.ts", "src/dst-map-preferences.ts", "src/dst-map-errors.ts", "src/dst-map-renderer.ts",
  "src/dst-map-edge-renderer.ts", "src/dst-map-edge-shapes.ts", "src/api-dst-map.ts", "src/api-transport.ts",
  "src/views/servers/DstWorldMapWorkbench.tsx", "src/views/servers/DstWorldMapCanvas.tsx", "src/views/servers/DstMapLandmarks.tsx",
  "src/views/servers/DstMapZoomControls.tsx", "src/views/servers/DstMapRoster.tsx", "src/views/servers/DstMapArtworkStatus.tsx",
  "src/views/servers/DstMapCollectionStatus.tsx", "src/views/servers/useDstMapSession.ts", "src/views/servers/useDstMapFollow.ts",
  "src/api-dst-world-regeneration.ts", "src/views/servers/DstWorldRegeneration.tsx", "src/views/servers/DstWorldToolsPanel.tsx",
  "src/views/servers/dst-world-map.css", "src/views/servers/workbench.css", "src/views/servers/dst-gm-item-catalog.ts",
  "src/views/servers/gm-catalog-i18n.ts", "src/app.css",
];
const sha = bytes => createHash("sha256").update(bytes).digest("hex").toUpperCase();

async function readBounded(filename, maximum) {
  assert.ok(path.isAbsolute(filename), "Evidence input paths must be absolute");
  for (let cursor = filename; ; cursor = path.dirname(cursor)) {
    assert.equal((await fs.lstat(cursor)).isSymbolicLink(), false, "Evidence paths must not traverse symbolic links or junctions");
    if (path.dirname(cursor) === cursor) break;
  }
  const file = await fs.open(filename, "r");
  try {
    const stat = await file.stat();
    assert.ok(stat.isFile() && stat.size > 0 && stat.size <= maximum, "Evidence file size is outside its bounded contract");
    const buffer = Buffer.alloc(maximum + 1);
    let count = 0;
    while (count < buffer.length) {
      const { bytesRead } = await file.read(buffer, count, buffer.length - count, count);
      if (!bytesRead) break;
      count += bytesRead;
    }
    assert.ok(count > 0 && count <= maximum, "Evidence file exceeded its bounded read limit");
    const bytes = buffer.subarray(0, count);
    return { bytes, sha256: sha(bytes) };
  } finally { await file.close(); }
}

async function inputs() {
  assert.match(evidenceHash ?? "", /^[A-Fa-f0-9]{64}$/, "A separately supplied completed-evidence SHA-256 is required");
  const completedFile = await readBounded(evidencePath, 1024 * 1024);
  assert.equal(completedFile.sha256, evidenceHash.toUpperCase());
  const completed = JSON.parse(completedFile.bytes.toString("utf8"));
  assert.equal(completed.schema, "lgsm-popular-map-completed-native-case-v1");
  assert.equal(completed.nativeActualExit, 0);
  assert.equal(completed.nativeEvidenceAndNewEdgeStrictPassed, true);
  const strictFile = await readBounded(completed.strictReport, 4 * 1024 * 1024);
  assert.equal(strictFile.sha256, completed.inputSha256[completed.strictReport]);
  const strict = JSON.parse(strictFile.bytes.toString("utf8"));
  assert.equal(strict.passed, true);
  assert.equal(strict.status, "passed");
  assert.equal(strict.root, completed.nativeRoot);
  assert.deepEqual(strict.expectedShards, ["master", "caves"]);
  const snapshots = {};
  for (const shard of ["master", "caves"]) {
    const filename = path.join(completed.nativeRoot, `world-map-connected-${shard}.json`);
    const input = strict.inputFiles.find(item => item.path === filename);
    assert.ok(input, "The strict report must bind each actual connected snapshot");
    const source = await readBounded(filename, 16 * 1024 * 1024);
    assert.equal(source.sha256, completed.inputSha256[filename]);
    assert.equal(source.sha256, input.sha256.toUpperCase());
    assert.equal(source.bytes.length, input.bytes);
    assert.ok(strict.strictFullMaps.some(item => item.file === path.basename(filename) && item.passed), "The selected full map must have passed strict acceptance");
    const snapshot = JSON.parse(source.bytes.toString("utf8"));
    assert.equal(snapshot.shard, shard);
    assert.equal(snapshot.instanceId, strict.instanceId);
    assert.equal(snapshot.runId, strict.runId);
    assert.ok(snapshot.players.length > 0 && snapshot.tiles.length === snapshot.width * snapshot.height);
    assert.ok(Date.now() - snapshot.capturedAtUnixMs > 6000, "Offline replay must use an actually historical timestamp");
    assert.deepEqual(snapshot.art.issues, []);
    // Only omit terrain payloads according to the production position DTO.
    // Identity, geometry dimensions, player values and the capture clock are untouched.
    const positions = { ...snapshot, tiles: null, tileNames: {}, tileKinds: {}, tileTextures: {}, tileBaseTextures: {},
      tileColors: {}, terrainLayers: [], landmarks: null, roads: null, art: null };
    snapshots[shard] = { ...source, filename, snapshot, positions: Buffer.from(JSON.stringify(positions)) };
  }
  assert.equal(snapshots.master.snapshot.instanceId, snapshots.caves.snapshot.instanceId);
  assert.equal(snapshots.master.snapshot.runId, snapshots.caves.snapshot.runId);
  return { completed, completedFile, strictFile, snapshots };
}

async function sourceBindings() {
  return Promise.all(sourceNames.map(async relative => {
    const filename = path.join(desktopRoot, relative), input = await readBounded(filename, 2 * 1024 * 1024);
    return { path: filename, bytes: input.bytes.length, sha256: input.sha256 };
  }));
}

for (const [shard, mode] of [["master", "fit"], ["master", "camera"], ["caves", "fit"]]) {
  test(`production map Workbench with byte-bound native snapshots: ${shard} ${mode}`, {
    timeout: 150000, skip: !evidencePath ? "LANGAME_DST_MAP_EXPERIENCE_EVIDENCE_PATH is required; captured-native replay is opt-in" : false,
  }, async () => {
    const input = await inputs(), sources = await sourceBindings();
    const outputRoot = process.env.LANGAME_TOOLS_SCREENSHOT_DIR;
    assert.ok(outputRoot && path.isAbsolute(outputRoot), "A task-owned absolute screenshot output directory is required");
    const outside = path.relative(path.resolve(desktopRoot, "../.."), outputRoot);
    assert.ok(path.isAbsolute(outside) || outside.startsWith(`..${path.sep}`), "Replay artifacts must stay outside the repository");
    await fs.mkdir(outputRoot, { recursive: true });
    const screenshot = path.join(outputRoot, `dst-workbench-${shard}-${mode}.png`);
    await assert.rejects(fs.access(screenshot), { code: "ENOENT" }, "A replay screenshot must never overwrite previous evidence");
    const options = Buffer.from(JSON.stringify({ shard, mode,
      instanceId: input.snapshots.master.snapshot.instanceId, runId: input.snapshots.master.snapshot.runId,
      case: input.completed.case, sources: Object.fromEntries(Object.entries(input.snapshots).map(([key, value]) =>
        [key, { sha256: value.sha256, capturedAtUnixMs: value.snapshot.capturedAtUnixMs }])),
      arrangedStructures: input.completed.naturalBuildingGenerationClaimed === false,
      noCustomTerrainClaimed: input.completed.customTerrainRequirementNotApplicable === true }));
    const reads = [];
    const report = await runBrowserFixture({ fixturePath: "dst-map-experience-native-render-browser.html",
      viewport: { width: 1600, height: 900 }, screenshotPath: screenshot, fixtureTimeoutMs: 90000, bundleWithMiddleware: true,
      fixtureMiddleware(request, response, next) {
        if (request.url === "/native-dst-experience-options.json") {
          if (request.method !== "GET") { response.writeHead(405).end(); return; }
          response.writeHead(200, { "Content-Type": "application/json", "Cache-Control": "no-store", "Content-Length": options.length });
          response.end(options); return;
        }
        const match = /^\/native-dst-experience-(master|caves)\.json\?terrain=([01])$/.exec(request.url ?? "");
        if (!match) return next();
        if (request.method !== "GET") { response.writeHead(405).end(); return; }
        const source = input.snapshots[match[1]], bytes = match[2] === "1" ? source.bytes : source.positions;
        reads.push({ shard: match[1], terrain: match[2] === "1" });
        response.writeHead(200, { "Content-Type": "application/json", "Cache-Control": "no-store", "Content-Length": bytes.length,
          "X-Lgsm-Native-Source": source.sha256 });
        response.end(bytes);
      },
    });
    assert.equal(report.status, "passed", report.error);
    assert.equal(report.offline_native_replay, true);
    assert.equal(report.production_workbench, true);
    assert.equal(report.timestamp_unchanged, true);
    assert.equal(report.shard, shard);
    assert.ok(report.checks >= 16);
    if (mode === "camera") assert.equal(report.camera_restored_from_indexed_db, true);
    assert.ok(reads.some(item => item.shard === shard && !item.terrain), "Production polling must exercise a real position-only projection");
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    assert.deepEqual(await sourceBindings(), sources, "Screenshot source files must stay byte-identical throughout rendering");
    const png = await readBounded(screenshot, 16 * 1024 * 1024);
    assert.equal(png.bytes.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
    assert.equal(png.bytes.readUInt32BE(16), 1600);
    assert.equal(png.bytes.readUInt32BE(20), 900);
    const summary = { schema: "lgsm-native-production-workbench-replay-v1", report, screenshot, screenshotSha256: png.sha256,
      evidence: { path: evidencePath, sha256: input.completedFile.sha256 }, strictSha256: input.strictFile.sha256,
      snapshots: Object.fromEntries(Object.entries(input.snapshots).map(([key, value]) => [key,
        { path: value.filename, sha256: value.sha256, capturedAtUnixMs: value.snapshot.capturedAtUnixMs }])), sources,
      provenance: "Production Workbench rendering byte-bound historical native data. Offline replay; not a native game GPU capture or a new live server test." };
    await fs.writeFile(path.join(outputRoot, `dst-workbench-${shard}-${mode}.summary.json`), JSON.stringify(summary, null, 2), { flag: "wx" });
    console.log(`DST_NATIVE_MAP_EXPERIENCE ${JSON.stringify({ ...report, screenshot, screenshot_sha256: png.sha256 })}`);
  });
}
