import { createHash } from "node:crypto";
import { readdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const DESKTOP_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const LOCK_PATH = join(DESKTOP_ROOT, "package-lock.json");
const OUTPUT_PATH = join(DESKTOP_ROOT, "public", "THIRD_PARTY_LICENSES.txt");
const OVERRIDE_ROOT = join(DESKTOP_ROOT, "third-party-license-sources", "npm");
const LICENSE_FILENAME = /^(?:licen[cs]e|copying|notice)(?:[._-].*)?$/i;
const UTF8_DECODER = new TextDecoder("utf-8", { fatal: true });

const REVIEWED_OVERRIDES = new Map([
  [
    "@react-three/fiber@9.8.1",
    {
      declaredLicense: "MIT",
      files: [
        {
          name: "react-three-fiber-9.8.1-LICENSE",
          sha256: "9c35b5de7b7493a707fffe4eb23bd2f7f449153c1911f7c6eefb4e591fd5349a",
          upstream:
            "https://github.com/pmndrs/react-three-fiber/blob/v9.8.1/LICENSE"
        }
      ]
    }
  ]
]);

function fail(message) {
  throw new Error(message);
}

function sha256(payload) {
  return createHash("sha256").update(payload).digest("hex");
}

function decodeLicense(payload, source) {
  if (payload.includes(0)) {
    fail(`${source} is not a text license file`);
  }
  try {
    return UTF8_DECODER.decode(payload);
  } catch (error) {
    fail(`${source} is not valid UTF-8: ${error.message}`);
  }
}

function packageNameFromLockPath(lockPath) {
  const marker = "node_modules/";
  const markerIndex = lockPath.lastIndexOf(marker);
  if (markerIndex < 0) {
    fail(`unsupported package-lock path: ${lockPath}`);
  }
  const name = lockPath.slice(markerIndex + marker.length);
  if (!name || name.includes("/node_modules/")) {
    fail(`cannot derive package name from package-lock path: ${lockPath}`);
  }
  return name;
}

function displayPath(path) {
  return relative(DESKTOP_ROOT, path).split(sep).join("/");
}

async function readJson(path) {
  let payload;
  try {
    payload = await readFile(path, "utf8");
  } catch (error) {
    fail(`cannot read ${displayPath(path)}: ${error.message}`);
  }
  try {
    return JSON.parse(payload);
  } catch (error) {
    fail(`cannot parse ${displayPath(path)}: ${error.message}`);
  }
}

async function localLicenseFiles(packageDirectory) {
  let entries;
  try {
    entries = await readdir(packageDirectory, { withFileTypes: true });
  } catch (error) {
    fail(`cannot inspect ${displayPath(packageDirectory)}: ${error.message}`);
  }
  return entries
    .filter((entry) => entry.isFile() && LICENSE_FILENAME.test(entry.name))
    .map((entry) => entry.name)
    .sort((left, right) => left.localeCompare(right, "en"));
}

async function readReviewedOverride(packageId, declaredLicense) {
  const override = REVIEWED_OVERRIDES.get(packageId);
  if (!override) {
    fail(
      `${packageId} has no license file in its npm package and no reviewed exact-version override`
    );
  }
  if (override.declaredLicense !== declaredLicense) {
    fail(
      `${packageId} changed its declared license from ${override.declaredLicense} to ${declaredLicense}`
    );
  }

  const sources = [];
  for (const file of override.files) {
    const path = join(OVERRIDE_ROOT, file.name);
    let payload;
    try {
      payload = await readFile(path);
    } catch (error) {
      fail(`cannot read reviewed license source ${displayPath(path)}: ${error.message}`);
    }
    const actualHash = sha256(payload);
    if (actualHash !== file.sha256) {
      fail(
        `reviewed license source ${displayPath(path)} has SHA-256 ${actualHash}; expected ${file.sha256}`
      );
    }
    sources.push({
      label: `reviewed upstream source: ${file.upstream}`,
      name: file.name,
      payload,
      text: decodeLicense(payload, displayPath(path))
    });
  }
  return sources;
}

async function collectProductionPackages() {
  const lock = await readJson(LOCK_PATH);
  if (lock.lockfileVersion !== 3 || typeof lock.packages !== "object") {
    fail("package-lock.json must use lockfileVersion 3 with a packages inventory");
  }

  const packages = [];
  const usedOverrides = new Set();
  for (const [lockPath, lockPackage] of Object.entries(lock.packages)) {
    if (!lockPath.startsWith("node_modules/") || lockPackage.dev === true) {
      continue;
    }
    const name = packageNameFromLockPath(lockPath);
    const version = lockPackage.version;
    const declaredLicense = lockPackage.license;
    if (typeof version !== "string" || typeof declaredLicense !== "string") {
      fail(`${lockPath} is missing a version or SPDX license declaration`);
    }

    const packageDirectory = join(DESKTOP_ROOT, ...lockPath.split("/"));
    const packageJson = await readJson(join(packageDirectory, "package.json"));
    if (packageJson.name !== name || packageJson.version !== version) {
      fail(
        `${lockPath}/package.json does not match locked package ${name}@${version}; run npm ci`
      );
    }
    if (packageJson.license !== declaredLicense) {
      fail(
        `${name}@${version} package metadata declares ${packageJson.license}; lockfile declares ${declaredLicense}`
      );
    }

    const localFiles = await localLicenseFiles(packageDirectory);
    let sources;
    if (localFiles.length > 0) {
      sources = await Promise.all(
        localFiles.map(async (file) => {
          const path = join(packageDirectory, file);
          const payload = await readFile(path);
          return {
            label: `npm package file: ${file}`,
            name: file,
            payload,
            text: decodeLicense(payload, `${name}@${version}/${file}`)
          };
        })
      );
    } else {
      sources = await readReviewedOverride(`${name}@${version}`, declaredLicense);
      usedOverrides.add(`${name}@${version}`);
    }

    packages.push({
      declaredLicense,
      name,
      packageId: `${name}@${version}`,
      resolved: lockPackage.resolved ?? "not recorded",
      sources,
      version
    });
  }

  for (const packageId of REVIEWED_OVERRIDES.keys()) {
    if (!usedOverrides.has(packageId)) {
      fail(
        `reviewed override ${packageId} is no longer used; remove it only after reviewing the new package contents`
      );
    }
  }
  return packages.sort((left, right) => left.packageId.localeCompare(right.packageId, "en"));
}

function normalizedLicenseText(text) {
  return text.replace(/\r\n?/g, "\n").replace(/\n+$/u, "");
}

function renderLicenseDocument(packages) {
  const lines = [
    "LanGame Server Manager — npm Production Dependency Licenses",
    "LanGame Server Manager — npm 生产依赖许可证",
    "",
    "This deterministic file is generated from apps/desktop/package-lock.json and the",
    "license texts shipped in the corresponding installed npm packages. Exact-version",
    "reviewed upstream sources are used only when an npm package omits its license file.",
    "Do not edit this file manually.",
    "",
    "本文件根据 apps/desktop/package-lock.json、已安装 npm 包随附的许可证正文以及",
    "必要的精确版本上游审计来源确定性生成。请勿手动修改本文件。",
    "",
    `Production package count / 生产包数量: ${packages.length}`,
    "",
    "Each upstream license text below is reproduced without editorial changes apart from",
    "line-ending normalization. Project-authored headings are not part of those licenses.",
    "以下上游许可证正文除统一换行符外未作编辑；项目添加的标题不属于上游许可证正文。",
    ""
  ];

  for (const dependency of packages) {
    lines.push("=".repeat(80));
    lines.push(`Package / 软件包: ${dependency.packageId}`);
    lines.push(`Declared license / 声明许可证: ${dependency.declaredLicense}`);
    lines.push(`Locked source / 锁定来源: ${dependency.resolved}`);
    for (const source of dependency.sources) {
      lines.push(`License source / 许可证来源: ${source.label}`);
      lines.push(`SHA-256: ${sha256(source.payload)}`);
      lines.push("-".repeat(80));
      lines.push(normalizedLicenseText(source.text));
      lines.push("");
    }
  }
  return `${lines.join("\n").replace(/\n+$/u, "")}\n`;
}

async function generateLicenseDocument() {
  const packages = await collectProductionPackages();
  return { document: renderLicenseDocument(packages), packages };
}

async function checkOutput(document) {
  let current;
  try {
    current = await readFile(OUTPUT_PATH, "utf8");
  } catch (error) {
    fail(`cannot read ${displayPath(OUTPUT_PATH)}: ${error.message}`);
  }
  if (current !== document) {
    fail(
      `${displayPath(OUTPUT_PATH)} is stale; run node scripts/generate_npm_third_party_licenses.mjs`
    );
  }
}

async function main(cliArguments = process.argv.slice(2)) {
  const check = cliArguments.length === 1 && cliArguments[0] === "--check";
  if (cliArguments.length > 0 && !check) {
    fail("usage: node scripts/generate_npm_third_party_licenses.mjs [--check]");
  }
  const { document, packages } = await generateLicenseDocument();
  if (check) {
    await checkOutput(document);
    console.log(
      `${displayPath(OUTPUT_PATH)} is current (${packages.length} production packages)`
    );
    return;
  }
  await writeFile(OUTPUT_PATH, document, "utf8");
  console.log(
    `generated ${displayPath(OUTPUT_PATH)} (${packages.length} production packages)`
  );
}

const invokedPath = process.argv[1] ? pathToFileURL(resolve(process.argv[1])).href : "";
if (invokedPath === import.meta.url) {
  main().catch((error) => {
    console.error(`npm license generation failed: ${error.message}`);
    process.exitCode = 1;
  });
}

export {
  OUTPUT_PATH,
  collectProductionPackages,
  generateLicenseDocument,
  renderLicenseDocument
};
