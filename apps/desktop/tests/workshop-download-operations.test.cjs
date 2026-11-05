const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".css"] = () => {};
require.extensions[".tsx"] = require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { WorkshopDownloadOperations, activeWorkshopDownloadJob } = require("../src/views/servers/workshop-download-operations.ts");
const request = (id) => ({ ids: [id], contentIds: [id], items: {}, locale: "zh-CN" });
const job = (target = "server-a", overrides = {}) => ({ id: `workshop-download:${target}:one`, target_id: target,
  kind: "DownloadWorkshop", status: "Running", progress_percent: 42, cancellable: false, cancel_requested: false,
  label: "Download", ...overrides });

test("accepted downloads survive subscribers leaving and keep instances isolated", () => {
  const store = new WorkshopDownloadOperations();
  const unsubscribe = store.subscribe(() => {});
  store.begin("server-a", request("123456")).trackJobs([]);
  const id = store.snapshot("server-a").requestId;
  store.recordJob("server-a", id, job());
  unsubscribe();
  assert.equal(store.snapshot("server-a").status, "running");
  assert.deepEqual(store.snapshot("server-a").contentIds, ["123456"]);
  assert.equal(store.snapshot("server-a").job.progress_percent, 42);
  assert.equal(store.snapshot("server-b").status, "idle");
  assert.equal(store.begin("server-a", request("654321")), null);
});

test("only the accepted request can settle or receive polled progress", () => {
  const store = new WorkshopDownloadOperations();
  const first = store.begin("server-a", request("123456"));
  const firstId = store.snapshot("server-a").requestId;
  first.fail("download rejected");
  assert.equal(store.snapshot("server-a").error, "download rejected");
  const second = store.begin("server-a", request("654321"));
  first.succeed();
  store.recordJob("server-a", firstId, job());
  assert.equal(store.snapshot("server-a").status, "running");
  assert.equal(store.snapshot("server-a").job, null);
  second.succeed();
  assert.equal(store.snapshot("server-a").status, "success");
  assert.equal(store.snapshot("server-a").error, null);
});

test("100 percent does not complete deployment and settings", () => {
  const store = new WorkshopDownloadOperations();
  const operation = store.begin("server-a", request("123456"));
  operation.trackJobs([]);
  store.recordJob("server-a", store.snapshot("server-a").requestId, job("server-a", { progress_percent: 100 }));
  assert.equal(store.snapshot("server-a").status, "running");
  operation.fail("deployment failed");
  assert.equal(store.snapshot("server-a").status, "error");
  assert.equal(store.snapshot("server-a").job, null);
});

test("startup Workshop updates, finished jobs and other instances cannot supply download progress", () => {
  const manual = job();
  assert.equal(activeWorkshopDownloadJob([
    job("server-a", { id: "instance-workshop-update:server-a:one" }),
    job("server-a", { status: "Completed" }), job("server-b"), manual
  ], "server-a"), manual);
});

test("queued downloads ignore existing jobs and keep the identity of their own attempt", () => {
  const store = new WorkshopDownloadOperations();
  const operation = store.begin("server-a", request("123456"));
  const first = job();
  const own = job("server-a", { id: "workshop-download:server-a:two" });
  store.recordJob("server-a", store.snapshot("server-a").requestId, first);
  assert.equal(store.snapshot("server-a").job, null, "No task is attributed before the native attempt");
  operation.trackJobs([first]);
  const attempt = store.snapshot("server-a").requestId;
  store.recordJob("server-a", attempt, first);
  assert.equal(store.snapshot("server-a").job, null);
  store.recordJob("server-a", attempt, own);
  store.recordJob("server-a", attempt, job("server-a", { id: "workshop-download:server-a:three" }));
  assert.equal(store.snapshot("server-a").job.id, own.id);
  operation.trackJobs([first, own]);
  store.recordJob("server-a", attempt, own);
  assert.equal(store.snapshot("server-a").job, null, "An account retry drops the prior attempt's progress");
});

test("bounded completed history never discards a running download", () => {
  const store = new WorkshopDownloadOperations();
  store.begin("pending", request("123456"));
  for (let i = 0; i < 40; i++) store.begin(`server-${i}`, request("654321")).succeed();
  assert.equal(store.snapshot("pending").status, "running");
  assert.equal(store.snapshot("server-0").status, "idle");
  assert.equal(store.snapshot("server-39").status, "success");
});
