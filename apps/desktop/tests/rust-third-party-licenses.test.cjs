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

test("the RAR decoder retains its custom terms separately from permissive fixture licenses", () => {
  const output = fs.readFileSync(outputPath, "utf8");
  const unrar = output.split("Package / 软件包: unrar-rs@0.15.3\n")[1]?.split("\n---")[0];
  assert.ok(unrar, "The locked RAR decoder must be present in the inventory.");
  assert.match(unrar, /Declared license \/ 声明许可证: license-file: LICENSE \(custom terms\)/u);
  assert.match(unrar, /Apache-2\.0 with the additional UnRAR restriction/u);
  assert.match(output, /cannot be\nused to develop RAR \(WinRAR\) compatible archiver/u);
  assert.match(output, /A binary distribution must include this file\./u);
  assert.match(output, /Copyright \(c\) 2018 Jack O'Connor/u);
});

test("the public snapshot retains Rust license verification inputs", () => {
  const snapshotPolicy = fs.readFileSync(
    path.join(root, "scripts", "export_public_snapshot.py"),
    "utf8"
  );

  for (const requiredPath of [
    "apps/desktop/src-tauri/THIRD_PARTY_LICENSES-RUST.txt",
    "scripts/generate_rust_third_party_licenses.py",
    "scripts/rust_license_policy.py",
    "scripts/third_party_license_sources/rust/blake2-simd-47714dc3-LICENSE"
  ]) {
    assert.ok(snapshotPolicy.includes(`"${requiredPath}"`), requiredPath);
  }
});
