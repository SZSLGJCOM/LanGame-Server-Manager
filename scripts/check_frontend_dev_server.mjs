import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const desktopRequire = createRequire(path.join(scriptDirectory, "..", "apps", "desktop", "package.json"));
const ts = desktopRequire("@typescript/typescript6");
const { parseSource, visitSyntax } = desktopRequire(path.join(
  scriptDirectory,
  "..",
  "apps",
  "desktop",
  "scripts",
  "typescript_source_tools.cjs",
));

const DEFAULT_ENTRY_PATHS = [
  "/",
  "/src/main.tsx",
  "/src/views/ServerWorkspaceView.tsx",
];

const UNRESOLVED_VITE_CLIENT_PLACEHOLDER =
  /__(?:MODE|BASE|SERVER_HOST|HMR_PROTOCOL|HMR_HOSTNAME|HMR_PORT|HMR_DIRECT_TARGET|HMR_BASE|HMR_TIMEOUT|HMR_ENABLE_OVERLAY|HMR_CONFIG_NAME|WS_TOKEN|SERVER_FORWARD_CONSOLE|DEFINES)__/;

function extractModuleSpecifiers(source, contentType) {
  const specifiers = new Set();
  if (contentType.includes("text/html")) {
    for (const scriptMatch of source.matchAll(/<script\b([^>]*)>/gi)) {
      const attributes = scriptMatch[1];
      if (!/\btype=["']module["']/i.test(attributes)) { continue; }
      const sourceMatch = attributes.match(/\bsrc=["']([^"']+)["']/i);
      if (sourceMatch) { specifiers.add(sourceMatch[1]); }
    }
    return specifiers;
  }

  const sourceFile = parseSource(source, "vite-module.js");
  visitSyntax(sourceFile, (node) => {
    if (
      (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) &&
      node.moduleSpecifier && ts.isStringLiteral(node.moduleSpecifier)
    ) {
      specifiers.add(node.moduleSpecifier.text);
    } else if (
      ts.isCallExpression(node) &&
      node.expression.kind === ts.SyntaxKind.ImportKeyword &&
      node.arguments[0] && ts.isStringLiteral(node.arguments[0])
    ) {
      specifiers.add(node.arguments[0].text);
    }
  });
  return specifiers;
}

function resolveLocalModuleUrl(specifier, parentUrl, expectedOrigin) {
  if (
    !specifier.startsWith("/") &&
    !specifier.startsWith("./") &&
    !specifier.startsWith("../") &&
    !specifier.startsWith("http://") &&
    !specifier.startsWith("https://")
  ) {
    return null;
  }

  const resolved = new URL(specifier, parentUrl);
  if (resolved.origin !== expectedOrigin) { return null; }
  resolved.hash = "";
  return resolved.href;
}

async function fetchModule(url, requestTimeoutMs, fetchImpl) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), requestTimeoutMs);
  try {
    const requestPath = new URL(url).pathname;
    const response = await fetchImpl(url, {
      cache: "no-store",
      headers: {
        accept: requestPath === "/"
          ? "text/html"
          : "text/javascript, application/javascript, application/ecmascript",
      },
      signal: controller.signal,
    });
    const body = await response.text();
    if (!response.ok) {
      const detail = body.replace(/\s+/g, " ").trim().slice(0, 240);
      throw new Error(`HTTP ${response.status} for ${url}${detail ? `: ${detail}` : ""}`);
    }
    const contentType = response.headers.get("content-type")?.toLowerCase() ?? "";
    if (requestPath === "/") {
      if (!contentType.includes("text/html") || !body.includes("/@vite/client")) {
        throw new Error(`Expected the Vite HTML entry at ${url}; received ${contentType || "unknown content"}.`);
      }
    } else if (!contentType.includes("javascript") && !contentType.includes("ecmascript")) {
      throw new Error(
        `Expected a JavaScript module at ${url}; received ${contentType || "unknown content"}.`,
      );
    }
    if (requestPath === "/@vite/client") {
      const unresolvedPlaceholder = body.match(UNRESOLVED_VITE_CLIENT_PLACEHOLDER)?.[0];
      if (unresolvedPlaceholder) {
        throw new Error(
          `Unresolved Vite client runtime placeholder ${unresolvedPlaceholder} at ${url}. ` +
          "The running dev server does not match the installed Vite client.",
        );
      }
    }
    return {
      body,
      contentType,
      url: response.url || url,
    };
  } finally {
    clearTimeout(timeout);
  }
}

export async function verifyViteModuleGraph({
  baseUrl,
  entryPaths = DEFAULT_ENTRY_PATHS,
  concurrency = 8,
  maxModules = 1200,
  requestTimeoutMs = 5000,
  fetchImpl = fetch,
}) {
  const base = new URL(baseUrl.endsWith("/") ? baseUrl : `${baseUrl}/`);
  const expectedOrigin = base.origin;
  const queue = [];
  const discovered = new Set();

  function enqueue(url) {
    if (discovered.has(url)) { return; }
    discovered.add(url);
    queue.push(url);
    if (discovered.size > maxModules) {
      throw new Error(`Vite module graph exceeded the ${maxModules} module safety limit.`);
    }
  }

  for (const entryPath of entryPaths) {
    enqueue(new URL(entryPath, base).href);
  }

  let cursor = 0;
  while (cursor < queue.length) {
    const batch = queue.slice(cursor, cursor + concurrency);
    cursor += batch.length;
    const modules = await Promise.all(
      batch.map((url) => fetchModule(url, requestTimeoutMs, fetchImpl)),
    );

    for (const module of modules) {
      for (const specifier of extractModuleSpecifiers(module.body, module.contentType)) {
        const resolved = resolveLocalModuleUrl(specifier, module.url, expectedOrigin);
        if (resolved) { enqueue(resolved); }
      }
    }
  }

  return { moduleCount: discovered.size };
}

export async function waitForViteModuleGraph({
  attempts = 4,
  retryDelayMs = 250,
  ...options
}) {
  if (!Number.isInteger(attempts) || attempts < 1) {
    throw new Error("Vite health-check attempts must be a positive integer.");
  }

  let lastError;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      const result = await verifyViteModuleGraph(options);
      return { ...result, attempt };
    } catch (error) {
      lastError = error;
      if (attempt < attempts) {
        await new Promise((resolve) => setTimeout(resolve, retryDelayMs));
      }
    }
  }
  throw lastError;
}

function readCliOptions(argv) {
  const options = { baseUrl: "", attempts: 4, retryDelayMs: 250 };
  for (let index = 0; index < argv.length; index += 1) {
    const value = argv[index];
    if (value === "--attempts") {
      options.attempts = Number.parseInt(argv[++index], 10);
    } else if (value === "--retry-delay-ms") {
      options.retryDelayMs = Number.parseInt(argv[++index], 10);
    } else if (!options.baseUrl) {
      options.baseUrl = value;
    } else {
      throw new Error(`Unexpected Vite health-check argument: ${value}`);
    }
  }
  if (!options.baseUrl) { throw new Error("Vite health check requires a base URL."); }
  return options;
}

async function main() {
  try {
    const result = await waitForViteModuleGraph(readCliOptions(process.argv.slice(2)));
    console.log(
      `[LanGame] Frontend module graph is ready (${result.moduleCount} modules, attempt ${result.attempt}).`,
    );
  } catch (error) {
    console.error(`[LanGame] Frontend module graph is not ready: ${error.message}`);
    process.exitCode = 1;
  }
}

const invokedPath = process.argv[1] ? path.resolve(process.argv[1]) : "";
if (invokedPath && fileURLToPath(import.meta.url) === invokedPath) {
  await main();
}
