const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const desktopRoot = path.resolve(__dirname, "..");
const headerSource = fs.readFileSync(path.join(desktopRoot, "src", "components", "AppHeader.tsx"), "utf8");
const headerCss = fs.readFileSync(path.join(desktopRoot, "src", "styles", "app-header.css"), "utf8");
const apiSource = fs.readFileSync(path.join(desktopRoot, "src", "api.ts"), "utf8");

test("header brand opens the official website through the shared external URL boundary", () => {
  assert.match(headerSource, /const LANGAME_WEBSITE_URL = "https:\/\/langame\.cn\/"/);
  assert.match(headerSource, /openExternalUrl\(LANGAME_WEBSITE_URL\)/);
  assert.match(headerSource, /className="shell-window-brand-link"/);
  assert.match(headerSource, /data-no-window-drag="true"/);
  assert.match(headerSource, /aria-label=\{t\("shell\.openOfficialWebsite"\)\}/);
  assert.doesNotMatch(headerSource, /window\.location/);
});

test("header brand remains keyboard-visible and LAN browser links stay client-side", () => {
  assert.match(headerCss, /\.shell-window-brand-link:focus-visible\s*\{/);
  assert.match(headerCss, /\.shell-window-brand-link[\s\S]{0,300}?cursor:\s*pointer/);
  assert.match(apiSource, /if \(!isTauri\(\)\) \{[\s\S]{0,120}?window\.open/);
  assert.match(apiSource, /window\.open\(target\.href, "_blank", "noopener,noreferrer"\)/);
});

test("header brand SVG includes the full wordmark and LGSM badge", () => {
  const logos = [
    ["langame-logo.svg", fs.readFileSync(path.join(desktopRoot, "src", "assets", "langame-logo.svg"), "utf8")],
    ["langame-logo-dark.svg", fs.readFileSync(path.join(desktopRoot, "src", "assets", "langame-logo-dark.svg"), "utf8")]
  ];

  for (const [label, svgSource] of logos) {
    const viewBoxMatch = svgSource.match(/viewBox="([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)"/);
    assert.ok(viewBoxMatch, `${label} declares a viewBox`);
    const rightEdge = Number(viewBoxMatch[1]) + Number(viewBoxMatch[3]);
    // LGSM badge rect: x=1668 width=216 stroke=5 → visual right ≈ 1886.5
    assert.ok(
      rightEdge >= 1887,
      `${label} viewBox right edge ${rightEdge} clips the LGSM badge`
    );
    assert.match(svgSource, /id="lgsm-badge"/);
    assert.match(svgSource, /aria-label="LGSM"/);
  }

  assert.match(headerSource, /width=\{1734\}/);
  assert.match(headerSource, /height=\{261\}/);
  assert.match(headerCss, /\.shell-window-brand-logo\s*\{[\s\S]*?overflow:\s*visible/);
});
