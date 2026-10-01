import { invokeOrMock, shouldUseLanApi } from "../../src/api-transport";

const nonce = new URLSearchParams(location.search).get("nonce");
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
let checks = 0;

function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
  checks++;
}

async function expectFailure(operation: Promise<unknown>, message: string) {
  try { await operation; } catch (error) {
    check(error instanceof Error && error.message === message, "Transport returned an unexpected failure");
    return;
  }
  throw new Error("Failed transport substituted a successful result");
}

async function verifyContentPolicy() {
  const blob = new Blob(["onmessage = ({ data }) => postMessage({ echoed: data })"], { type: "text/javascript" });
  const url = URL.createObjectURL(blob);
  const worker = new Worker(url);
  try {
    const reply = await new Promise<unknown>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("Blob worker did not reply")), 3000);
      worker.onmessage = (event) => { clearTimeout(timer); resolve(event.data); };
      worker.onerror = (event) => { clearTimeout(timer); reject(new Error(event.message)); };
      worker.postMessage("fixture-worker-ready");
    });
    check(JSON.stringify(reply) === JSON.stringify({ echoed: "fixture-worker-ready" }), "Blob worker result changed");
  } finally {
    worker.terminate();
    URL.revokeObjectURL(url);
  }
  const blocked = new Promise<SecurityPolicyViolationEvent>((resolve, reject) => {
    const timer = setTimeout(() => { removeEventListener("securitypolicyviolation", listener); reject(new Error("Inline script was not blocked")); }, 3000);
    function listener(event: SecurityPolicyViolationEvent) {
      if (event.blockedURI !== "inline") return;
      clearTimeout(timer);
      removeEventListener("securitypolicyviolation", listener);
      resolve(event);
    }
    addEventListener("securitypolicyviolation", listener);
  });
  const inline = document.createElement("script");
  inline.textContent = "document.documentElement.dataset.inlineExecuted = 'yes'";
  document.head.append(inline);
  const violation = await blocked;
  inline.remove();
  check(violation.effectiveDirective === "script-src-elem" && violation.disposition === "enforce", "Inline policy was only reported, not enforced");
  check(document.documentElement.dataset.inlineExecuted === undefined, "Untrusted inline script executed");
}

async function run() {
  sessionStorage.clear();
  document.cookie = "fixture_cookie=must-not-be-sent; SameSite=Strict; path=/";
  check(!import.meta.env.DEV, "Fixture must exercise the compiled production transport branch");
  check(shouldUseLanApi(), "Production loopback must use the management API without a token");
  await expectFailure(invokeOrMock("bootstrap"), "fixture authentication required");
  check(sessionStorage.getItem("langameLanToken") === null, "Unauthenticated request invented a token");

  const browserFixtureToken = "synthetic-test-only";
  history.replaceState(null, "", `${location.pathname}${location.search}#langameToken=${browserFixtureToken}`);
  const result = await invokeOrMock<{ source: string }>("read_instance_details_from_storage", { instanceId: "fixture-instance" });
  check(result.source === "isolated-http-fixture", "Transport substituted synthetic preview data");
  check(location.hash === "", "The management token remained in the address bar");
  check(sessionStorage.getItem("langameLanToken") === browserFixtureToken, "Fragment token was not retained for this browser session");
  await expectFailure(invokeOrMock("start_instance_process", { instanceId: "fixture-instance" }), "fixture backend unavailable");
  await verifyContentPolicy();
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, production_transport: true, blob_worker_replied: true,
    inline_script_blocked: true, browser_errors: errors };
}

void run().catch((error: unknown) => ({ status: "failed", error: String(error), checks, browser_errors: errors }))
  .then(async (report) => {
    await fetch(`/__reliability_result/${nonce}`, {
      method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(report),
    });
  });
