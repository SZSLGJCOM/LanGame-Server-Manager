const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const sourceRoot = path.resolve(__dirname, "..", "src");
const typographyPath = path.join(sourceRoot, "styles", "typography.css");
const typography = fs.readFileSync(typographyPath, "utf8");

function cssFiles(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = path.join(directory, entry.name);
    return entry.isDirectory() ? cssFiles(entryPath) : entry.name.endsWith(".css") ? [entryPath] : [];
  });
}

function usesBoundedDialTextRole(value, definedTokens) {
  const padded = /^min\(var\((--text-[\w-]+)\), calc\(\((\d+(?:\.\d+)?)cqw - (\d+(?:\.\d+)?)ch - (\d+(?:\.\d+)?)px\) \* (\d+(?:\.\d+)?) \/ var\(--core-value-length\)\)\)$/.exec(value);
  if (padded) return definedTokens.has(padded[1]) && padded.slice(2).every((part) => Number(part) > 0);
  const match = /^min\(var\((--text-[\w-]+)\), (?:(\d+(?:\.\d+)?)cqw, )?calc\((\d+(?:\.\d+)?)cqw \/ var\(--core-value-length\)\)\)$/.exec(value);
  return Boolean(match && definedTokens.has(match[1]) && Number(match[3]) > 0
    && (match[2] === undefined || Number(match[2]) > 0));
}

test("dial fitting can shrink a declared text role but cannot replace or enlarge its upper bound", () => {
  const roles = new Set(["--text-display"]);
  assert.equal(usesBoundedDialTextRole("min(var(--text-display), calc(140cqw / var(--core-value-length)))", roles), true);
  assert.equal(usesBoundedDialTextRole("min(var(--text-display), 26cqw, calc(105cqw / var(--core-value-length)))", roles), true);
  assert.equal(usesBoundedDialTextRole("min(var(--text-display), calc((100cqw - 5ch - 4px) * 1.8 / var(--core-value-length)))", roles), true);
  for (const value of [
    "max(var(--text-display), calc(140cqw / var(--core-value-length)))",
    "min(30px, calc(140cqw / var(--core-value-length)))",
    "min(var(--text-undefined), calc(140cqw / var(--core-value-length)))",
    "min(var(--text-display), 26vw, calc(140cqw / var(--core-value-length)))",
    "min(var(--text-display), calc(140cqw / var(--unrelated-length)))",
    "min(var(--text-display), calc(0cqw / var(--core-value-length)))",
    "max(var(--text-display), calc((100cqw - 5ch - 4px) * 1.8 / var(--core-value-length)))",
    "min(var(--text-undefined), calc((100cqw - 5ch - 4px) * 1.8 / var(--core-value-length)))",
    "min(var(--text-display), calc((100cqw - 5ch - 4px) * 0 / var(--core-value-length)))",
    "min(var(--text-display), calc((100vw - 5ch - 4px) * 1.8 / var(--core-value-length)))",
    "min(var(--text-display), calc((100cqw - 5ch - 4px) * 1.8 / var(--unrelated-length)))"
  ]) assert.equal(usesBoundedDialTextRole(value, roles), false, value);
});

test("desktop typography exposes stable logical-pixel roles and shared font families", () => {
  const expectedRoles = {
    "page-title": "1.375rem",
    "section-title": "1rem",
    body: "0.8125rem",
    secondary: "0.75rem",
    meta: "0.75rem",
    code: "0.8125rem",
  };
  for (const [role, size] of Object.entries(expectedRoles)) {
    assert.match(typography, new RegExp(`--text-${role}:\\s*${size.replace(".", "\\.")};`));
  }
  assert.match(typography, /:root\s*\{[^}]*font-size:\s*16px;/);
  assert.match(typography, /body\s*\{[^}]*font-size:\s*var\(--text-body\)/);
  assert.match(typography, /--font-sans:\s*"Inter",[^;]*"Segoe UI"[^;]*"Microsoft YaHei UI"/);
  assert.match(typography, /--font-mono:[^;]*"Cascadia Mono"[^;]*Consolas/);
  assert.match(typography, /pre,\s*code,\s*kbd,\s*samp\s*\{[^}]*font-family:\s*var\(--font-mono\)[^}]*font-size:\s*var\(--text-code\)/);
  assert.match(fs.readFileSync(path.join(sourceRoot, "app.css"), "utf8"), /^@import "\.\/styles\/typography\.css";/);
});

test("Inter variable fonts and their upstream license ship locally", () => {
  const faces = [...typography.matchAll(/@font-face\s*\{([^}]+)\}/g)];
  assert.equal(faces.length, 2, "Bundle real upright and italic faces");
  for (const [index, [, face]] of faces.entries()) {
    assert.match(face, /font-family:\s*"Inter";/);
    assert.match(face, /font-weight:\s*100 900;/);
    assert.match(face, /font-display:\s*swap;/);
    assert.match(face, new RegExp(`font-style:\\s*${index === 0 ? "normal" : "italic"};`));
    const source = /src:\s*url\("([^\"]+)"\) format\("woff2"\);/.exec(face)?.[1];
    assert.ok(source?.startsWith("../assets/fonts/"), "Fonts must resolve to bundled files, without CDN or installed-font substitution");
    const bytes = fs.readFileSync(path.resolve(path.dirname(typographyPath), source));
    assert.equal(bytes.toString("ascii", 0, 4), "wOF2", "Font asset must be a WOFF2 binary");
    assert.equal(bytes.readUInt32BE(8), bytes.length, "Font must not be truncated");
  }
  const license = fs.readFileSync(path.join(sourceRoot, "../public/fonts/inter/OFL.txt"), "utf8");
  assert.match(license, /Copyright \(c\) 2016 The Inter Project Authors/);
  assert.match(license, /SIL OPEN FONT LICENSE Version 1\.1/);
  assert.match(fs.readFileSync(path.join(sourceRoot, "../index.html"), "utf8"), /rel="preload"[^>]+InterVariable\.woff2[^>]+as="font"[^>]+crossorigin/);
});

test("component typography uses declared roles instead of viewport-scaled or local text sizes", () => {
  const definedTokens = new Set([...typography.matchAll(/(--(?:text|font|leading)-[\w-]+):/g)].map((match) => match[1]));
  const dialPath = path.join(sourceRoot, "views", "system", "system-core-dial.css");
  for (const file of cssFiles(sourceRoot)) {
    // Font declarations describe the upstream face, not a component text role.
    const source = fs.readFileSync(file, "utf8").replace(/@font-face\s*\{[^}]*\}/g, "");
    const relative = path.relative(sourceRoot, file);
    for (const declaration of source.matchAll(/(?<![-\w])(font-size|font-family|font-weight)\s*:\s*([^;\n}]+)/g)) {
      const [, property, value] = declaration;
      if (value === "inherit") continue;
      if (file === typographyPath && property === "font-size" && value === "16px") continue;
      // Dial figures fit their numeric length inside a fixed visual instrument.
      if (file === dialPath && property === "font-size" && usesBoundedDialTextRole(value, definedTokens)) continue;
      const token = /^var\((--[\w-]+)\)$/.exec(value)?.[1];
      assert.ok(token && definedTokens.has(token), `${relative}: ${property}: ${value} must use a declared typography role`);
    }
  }
});
