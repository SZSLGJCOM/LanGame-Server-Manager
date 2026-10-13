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
    /uses: actions\/checkout@[a-f0-9]{40}\b/u
  );
  assert.match(
    workflow,
    /^\s*CARGO_AUDIT_VERSION: "\d+\.\d+\.\d+"\s*$/mu
  );
  assert.match(
    workflow,
    /cargo install cargo-audit --version "\$CARGO_AUDIT_VERSION" --locked --root "\$RUNNER_TEMP\/lgsm-rustsec"/u
  );
  assert.match(
    workflow,
    /test "\$\("\$RUNNER_TEMP\/lgsm-rustsec\/bin\/cargo-audit" --version\)" = "cargo-audit \$CARGO_AUDIT_VERSION"/u
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

test("RustSec tooling cache is versioned and only master may publish it", () => {
  const workflow = fs.readFileSync(workflowPath, "utf8");
  assert.match(workflow, /uses: actions\/cache\/restore@[a-f0-9]{40}\b/u);
  assert.match(workflow, /uses: actions\/cache\/save@[a-f0-9]{40}\b/u);
  assert.match(workflow, /key: rustsec-\$\{\{ runner\.os \}\}-\$\{\{ runner\.arch \}\}-\$\{\{ env\.CARGO_AUDIT_VERSION \}\}-\$\{\{ hashFiles\('rust-toolchain\.toml'\) \}\}/u);
  const saveStep = workflow.split(/\n\s+- name:/u).find((step) => step.includes("uses: actions/cache/save@"));
  assert.ok(saveStep, "the cache publication step must be present");
  assert.match(saveStep, /^\s*if: github\.ref == 'refs\/heads\/master' && steps\.scanner-cache\.outputs\.cache-hit != 'true'\s*$/mu);
  assert.match(saveStep, /path: \$\{\{ runner\.temp \}\}\/lgsm-rustsec/u);
  assert.match(saveStep, /key: \$\{\{ steps\.scanner-cache\.outputs\.cache-primary-key \}\}/u);
});
