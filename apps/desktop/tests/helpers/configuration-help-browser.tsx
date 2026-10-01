import React, { act, StrictMode, useState, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { ConfigurationHelp } from "../../src/views/settings/ConfigurationFieldHelp";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100dvh;padding:24px;display:grid;align-content:start;gap:20px";
const root = createRoot(fixture);
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const helpText = "设置服务器名称，方便玩家在列表中识别。";
const pathText = `D:\\Servers\\${"very-long-directory-name\\".repeat(42)}world.json\n${"路径保持完整，可在气泡内继续阅读。\n".repeat(16)}`.trim();
let changePathDescription: (description: string) => void;
let checks = 0;
let pointerTarget: HTMLElement | null = null;

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const target = document.querySelector<T>(selector);
  check(target, `Missing ${selector}`);
  return target;
}
async function settleUntil(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 4000;
  while (!predicate()) {
    check(performance.now() < deadline, description);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
function visible() { return [...document.querySelectorAll<HTMLElement>(".configuration-field-help-tooltip.is-visible")]; }
async function opened(text: string) {
  await settleUntil(() => visible().length === 1 && visible()[0].textContent === text
    && getComputedStyle(visible()[0]).opacity === "1", `Expected one fully visible tooltip: ${text.slice(0, 45)}`);
  return visible()[0];
}
async function closed() {
  await settleUntil(() => !document.querySelector(".configuration-field-help-tooltip"), "Tooltip did not dismiss");
}
async function point(target: HTMLElement) {
  await act(async () => {
    const previous = pointerTarget;
    previous?.dispatchEvent(new PointerEvent("pointerout", { bubbles: true, relatedTarget: target, pointerType: "mouse" }));
    target.dispatchEvent(new PointerEvent("pointerover", { bubbles: true, relatedTarget: previous, pointerType: "mouse" }));
    pointerTarget = target;
  });
}
async function focus(target: HTMLElement) { await act(async () => { target.focus(); }); }
async function escape() {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/Escape`, { method: "POST" });
    check(response.ok, "Native Escape dispatch failed");
  });
}
async function remainsOpen(text: string) {
  // This observation exceeds both the pointer transfer grace and exit animation.
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 320)); });
  check(visible().length === 1 && visible()[0].textContent === text, "Readable help disappeared while still in use");
}
function fits(target: HTMLElement) {
  const box = target.getBoundingClientRect();
  check(box.width > 0 && box.height > 0 && box.left >= 7 && box.right <= innerWidth - 7
    && box.top >= 7 && box.bottom <= innerHeight - 7,
  `Tooltip must remain within the viewport gutter: ${JSON.stringify(box.toJSON())}`);
  check(target.scrollWidth <= target.clientWidth + 1, "Tooltip text must not overflow horizontally");
}
function Field(props: { id: string; label: string; description: string; children?: ReactNode }) {
  return <ConfigurationHelp description={props.description}>{(help) => (
    <div id={props.id} className="configuration-field settings-schema-field" ref={help.anchorRef} {...help.interactionProps}>
      <label className="detail-label settings-field-label" htmlFor={`${props.id}-input`}>{props.label}</label>
      <div className="configuration-field-control">
        <input id={`${props.id}-input`} className="settings-schema-input" defaultValue={props.label}
          aria-describedby={help.descriptionId} />
      </div>
      {props.children}
    </div>
  )}</ConfigurationHelp>;
}
function PathField() {
  const [description, setDescription] = useState("显示完整存档目录。");
  changePathDescription = setDescription;
  return <ConfigurationHelp description={description}>{(help) => (
    <div id="path-anchor" ref={help.anchorRef} {...help.interactionProps} tabIndex={0} aria-describedby={help.descriptionId}
      style={{ position: "fixed", right: 16, bottom: 20, width: 220 }} className="instance-isolation-path">
      <span>存档目录</span><code>D:\Servers\…\world.json</code>
    </div>
  )}</ConfigurationHelp>;
}

Object.assign(window, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => { root.unmount(); });
  check(!document.querySelector(".configuration-field-help-tooltip"), "Unmount must remove all tooltip portals");
  return { browser_errors: errors, native_dialogs: 0 };
} });

async function run() {
  await act(async () => { root.render(<StrictMode>
    <header><h2>配置与维护 · 气泡交互验收</h2><p className="form-note">真实帮助组件和产品样式，使用合成字段。</p></header>
    <button id="outside" className="secondary-button" style={{ justifySelf: "start" }}>其他操作</button>
    <div className="settings-schema-section" style={{ width: "min(620px,100%)" }}>
      <section id="category"><Field id="regular" label="服务器名称" description={helpText} /></section>
      <Field id="parent" label="外层设置" description="外层设置说明。">
        <Field id="child" label="子项设置" description="仅显示子项自己的说明。" />
      </Field>
    </div>
    <PathField />
  </StrictMode>); });
  await document.fonts.ready;
  const outside = element("#outside");
  const regular = element("#regular-input");
  await focus(outside);
  await point(regular);
  let bubble = await opened(helpText);
  check(document.activeElement === outside, "Pointer help must not steal keyboard focus");
  const accessible = document.getElementById(regular.getAttribute("aria-describedby")!);
  check(accessible?.getAttribute("role") === "tooltip" && accessible.textContent === helpText,
    "Focused controls must retain the full accessible description");
  fits(bubble);
  await escape();
  await closed();
  check(document.activeElement === outside, "Global Escape must preserve focus on the other control");
  checks++;

  await point(outside);
  await point(regular);
  bubble = await opened(helpText);
  await point(bubble);
  await remainsOpen(helpText);
  check(getComputedStyle(bubble).pointerEvents === "auto", "Pointer must be able to enter the readable bubble");
  await point(outside);
  await closed();
  checks++;

  await focus(regular);
  await opened(helpText);
  await focus(outside);
  await closed();
  checks++;

  await point(regular);
  await opened(helpText);
  await act(async () => { element("#category").hidden = true; });
  await closed();
  await act(async () => { element("#category").hidden = false; });
  await point(outside);
  checks++;

  await point(element("#parent-input"));
  await opened("外层设置说明。");
  await point(element("#child-input"));
  await opened("仅显示子项自己的说明。");
  await remainsOpen("仅显示子项自己的说明。");
  await point(outside);
  await closed();
  await focus(element("#child-input"));
  await opened("仅显示子项自己的说明。");
  await focus(outside);
  await closed();
  checks++;

  await point(element("#path-anchor"));
  await opened("显示完整存档目录。");
  await act(async () => { changePathDescription(pathText); });
  bubble = await opened(pathText);
  await settleUntil(() => {
    const bounds = bubble.getBoundingClientRect();
    return bounds.left >= 7 && bounds.right <= innerWidth - 7 && bounds.top >= 7 && bounds.bottom <= innerHeight - 7;
  }, "Changing help text must reposition the enlarged bubble inside the viewport").catch((cause) => {
    throw new Error(`${String(cause)}; bounds=${JSON.stringify(bubble.getBoundingClientRect().toJSON())}; style=${bubble.style.cssText}`);
  });
  fits(bubble);
  check(getComputedStyle(bubble).whiteSpace === "pre-wrap", "Path help must preserve its explicit newlines");
  check(bubble.scrollHeight > bubble.clientHeight, "Long help must provide local vertical scrolling");
  await point(bubble);
  await act(async () => { bubble.scrollTop = bubble.scrollHeight; });
  check(bubble.scrollTop > 0, "All lines must remain reachable inside the bubble");
  await remainsOpen(pathText);
  fits(bubble);
  await escape();
  await closed();
  await point(outside);
  checks++;

  document.documentElement.dataset.theme = "light";
  await point(regular);
  bubble = await opened(helpText);
  fits(bubble);
  const style = getComputedStyle(bubble);
  check(style.fontSize === "12px" && style.borderRadius === "8px" && style.lineHeight === "18px",
    "Help must use the shared secondary type role and corner radius in both themes");
  await point(outside);
  await closed();
  document.documentElement.dataset.theme = "dark";
  checks++;

  await focus(regular);
  await opened(helpText);
  await escape();
  await closed();
  check(document.activeElement === regular, "Escape must dismiss focused help without moving focus");
  await focus(outside);
  await point(regular);
  await opened(helpText);
  checks++;
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors,
    pointer_events: "DOM PointerEvent dispatch", keyboard_events: "native CDP Escape" };
}

void run().catch((error) => ({ status: "failed", checks,
  error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
    return fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) });
  });
