const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const root = path.resolve(__dirname, "..", "..", "..");
const outputPath = path.join(
  root,
  "apps",
  "desktop",
  "src-tauri",
  "THIRD_PARTY_LICENSES-RUST.txt"
);

test("Rust license inventory covers every crates.io Cargo.lock package", () => {
  const lock = fs.readFileSync(path.join(root, "Cargo.lock"), "utf8");
  const lockedRegistryCount = lock
    .split(/\r?\n\[\[package\]\]\r?\n/u)
    .slice(1)
    .filter((block) =>
      block.includes('source = "registry+https://github.com/rust-lang/crates.io-index"')
    ).length;
  const output = fs.readFileSync(outputPath, "utf8");

  assert.equal(output.match(/^Package \/ 软件包:/gmu)?.length, lockedRegistryCount);
  assert.match(
    output,
    new RegExp(`Locked external crates / 锁定外部 crate: ${lockedRegistryCount}`)
  );
  assert.match(output, /^Package \/ 软件包: tauri@/mu);
  assert.match(output, /^Package \/ 软件包: sqlx@/mu);
  assert.match(output, /^Package \/ 软件包: reqwest@/mu);
});

test("Tauri installs the Rust legal inventory as an explicit resource", () => {
  const config = JSON.parse(
    fs.readFileSync(
      path.join(root, "apps", "desktop", "src-tauri", "tauri.conf.json"),
      "utf8"
    )
  );

  assert.equal(
    config.bundle.resources?.["THIRD_PARTY_LICENSES-RUST.txt"],
    "THIRD_PARTY_LICENSES-RUST.txt"
  );
});

test("CI and public snapshot contracts retain Rust license verification inputs", () => {
  const workflow = fs.readFileSync(path.join(root, ".github", "workflows", "ci.yml"), "utf8");
  const snapshotPolicy = fs.readFileSync(
    path.join(root, "scripts", "export_public_snapshot.py"),
    "utf8"
  );

  assert.match(workflow, /cargo fetch --locked/);
  assert.match(
    workflow,
    /generate_rust_third_party_licenses\.py --check --offline/
  );
  for (const requiredPath of [
    "apps/desktop/src-tauri/THIRD_PARTY_LICENSES-RUST.txt",
    "scripts/generate_rust_third_party_licenses.py",
    "scripts/rust_license_policy.py"
  ]) {
    assert.ok(snapshotPolicy.includes(`"${requiredPath}"`), requiredPath);
  }
});
