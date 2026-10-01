const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const source = fs.readFileSync(path.join(__dirname, "../src/styles/theme-system.css"), "utf8");

function themeTokens(theme) {
  const block = source.match(new RegExp(`:root\\[data-theme="${theme}"\\]\\s*\\{([^}]+)\\}`));
  assert.ok(block, `${theme} must declare a palette`);
  return Object.fromEntries([...block[1].matchAll(/(--[\w-]+):\s*([^;]+);/g)].map((match) => [match[1], match[2].trim()]));
}

function color(value, tokens) {
  const token = value.startsWith("--") ? value : value.match(/^var\((--[\w-]+)\)$/)?.[1];
  if (token) {
    assert.ok(tokens[token], `${token} must resolve to a color`);
    return color(tokens[token], tokens);
  }
  const hex = value.match(/^#([\da-f]{6})$/i);
  if (hex) {
    return [...hex[1].matchAll(/../g)].map(([channel]) => parseInt(channel, 16) / 255).concat(1);
  }
  const rgb = value.match(/^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)(?:\s*,\s*([\d.]+))?\s*\)$/);
  assert.ok(rgb, `Unsupported palette color: ${value}`);
  return [Number(rgb[1]) / 255, Number(rgb[2]) / 255, Number(rgb[3]) / 255, Number(rgb[4] ?? 1)];
}

function composite(foreground, background) {
  const alpha = foreground[3];
  return foreground.slice(0, 3).map((channel, index) => channel * alpha + background[index] * (1 - alpha)).concat(1);
}

function luminance(rgb) {
  const channels = rgb.slice(0, 3).map((channel) => channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4);
  return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
}

function assertContrast(tokens, foreground, background, minimum, label, backdrop) {
  const base = color(background, tokens);
  const surface = backdrop ? composite(base, color(backdrop, tokens)) : base;
  assert.equal(surface[3], 1, `${label}: the surface needs an opaque backdrop`);
  const text = composite(color(foreground, tokens), surface);
  const values = [luminance(text), luminance(surface)].sort((a, b) => b - a);
  const ratio = (values[0] + 0.05) / (values[1] + 0.05);
  assert.ok(ratio >= minimum, `${label}: ${foreground} on ${background} is ${ratio.toFixed(2)}:1; expected at least ${minimum}:1`);
}

for (const theme of ["light", "dark"]) {
  const tokens = themeTokens(theme);
  const surfaces = ["--bg", "--theme-card-surface", "--theme-card-elevated", "--theme-card-muted", "--theme-control-surface", "--theme-control-hover"];

  test(`${theme} shared structure and interaction colors remain neutral`, () => {
    for (const [token, value] of Object.entries(tokens)) {
      if (/success|warning|danger/.test(token) || !value.startsWith("#")) continue;
      const [red, green, blue] = color(value, tokens);
      assert.equal(red, green, `${token} must not tint neutral surfaces or controls`);
      assert.equal(green, blue, `${token} must not tint neutral surfaces or controls`);
    }
    const expected = theme === "dark"
      ? ["#101010", "#191919", "#232323", "#f5f5f5", "#adadad"]
      : ["#f6f6f6", "#ffffff", "#eeeeee", "#1a1a1a", "#606060"];
    ["--bg", "--theme-card-surface", "--theme-card-active-bg", "--shell-text", "--shell-muted"]
      .forEach((token, index) => assert.equal(tokens[token].toLowerCase(), expected[index], token));
  });

  test(`${theme} primary actions use the theme's contrasting light or dark fill`, () => {
    const action = luminance(color("--accent", tokens));
    const label = luminance(color("--theme-accent-contrast", tokens));
    assert.ok(theme === "dark" ? action > label : action < label);
  });

  test(`${theme} primary and secondary text stays readable across shared surfaces`, () => {
    for (const foreground of ["--shell-text", "--shell-muted"]) {
      for (const surface of surfaces) {
        assertContrast(tokens, foreground, surface, 4.5, theme);
      }
    }
  });

  test(`${theme} primary actions and selected controls meet normal-text contrast`, () => {
    for (const background of ["--accent", "--accent-strong"]) {
      assertContrast(tokens, "--theme-accent-contrast", background, 4.5, theme);
    }
    assertContrast(tokens, "--theme-card-active-text", "--theme-card-active-bg", 4.5, theme);
    assertContrast(tokens, "--theme-badge-text", "--theme-badge-bg", 4.5, theme);
  });

  test(`${theme} state messages retain readable neutral, success, warning and danger text`, () => {
    for (const state of ["status", "warning", "danger"]) {
      const background = state === "status" ? "--theme-status-surface" : `--theme-${state}-bg`;
      assertContrast(tokens, `--theme-${state}-text`, background, 4.5, theme);
    }
    const successBackground = source.match(/\.status-chip\.is-running,[\s\S]*?background:\s*([^;]+);/)?.[1].trim();
    assert.ok(successBackground, "Success status chips must declare their background");
    for (const surface of ["--bg", "--theme-card-surface", "--theme-card-elevated"]) {
      assertContrast(tokens, "--success", successBackground, 4.5, `${theme} success`, surface);
    }
  });

  test(`${theme} focus and selected-control boundaries remain distinct from shared surfaces`, () => {
    for (const surface of surfaces.concat("--theme-card-active-bg")) {
      assertContrast(tokens, "--shell-focus", surface, 3, theme);
      assertContrast(tokens, "--theme-card-active-border", surface, 3, theme);
    }
  });
}

test("shared shell accent resolves through the neutral action palette", () => {
  assert.match(source, /--shell-accent:\s*var\(--accent\);/);
});
