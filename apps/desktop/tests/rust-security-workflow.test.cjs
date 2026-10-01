const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const root = path.resolve(__dirname, "..", "..", "..");
const workflowPath = path.join(root, ".github", "workflows", "rust-security.yml");

test("Rust security workflow uses pinned tooling and scans dependency changes", () => {
  const workflow = fs.readFileSync(workflowPath, "utf8");

  assert.match(
    workflow,
    /actions\/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7\.0\.1/u
  );
  assert.match(
    workflow,
    /cargo install cargo-audit --version 0\.22\.2 --locked/u
  );
  assert.match(
    workflow,
    /^\s*run: cargo audit --deny unsound --deny yanked --ignore RUSTSEC-2024-0429\s*$/mu
  );
  assert.doesNotMatch(workflow, /cargo audit[^\n]*--deny warnings/u);
  assert.match(workflow, /^\s*schedule:\s*$/mu);
  assert.match(workflow, /^\s*pull_request:\s*$/mu);
  assert.match(workflow, /^\s*push:\s*$/mu);
  assert.match(workflow, /^\s*- Cargo\.lock\s*$/mu);
  assert.match(workflow, /^\s*contents: read\s*$/mu);
  assert.match(workflow, /^\s*persist-credentials: false\s*$/mu);
});
