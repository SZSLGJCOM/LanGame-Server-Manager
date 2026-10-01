const path = require("node:path");
const { spawnSync } = require("node:child_process");
const swc = require("@swc/core");

const TYPESCRIPT_EXTENSIONS = new Set([".ts", ".tsx", ".mts", ".cts"]);

function parserOptions(filename, syntax) {
  const extension = path.extname(filename).toLowerCase();
  return syntax === "typescript"
    ? {
        syntax: "typescript",
        tsx: extension === ".tsx",
        decorators: false,
      }
    : {
        syntax: "ecmascript",
        jsx: extension === ".jsx",
      };
}

function inferSyntax(filename) {
  return TYPESCRIPT_EXTENSIONS.has(path.extname(filename).toLowerCase())
    ? "typescript"
    : "ecmascript";
}

function parseSource(source, filename = "module.ts") {
  return swc.parseSync(source, parserOptions(filename, inferSyntax(filename)));
}

function transpileTypeScript(source, filename = "module.ts", { development = true } = {}) {
  // Node fixtures do not run through Vite. Supply the same compile-time mode
  // constant explicitly; transport regressions also exercise production mode.
  const resolvedSource = source.replaceAll("import.meta.env.DEV", String(development));
  const result = swc.transformSync(resolvedSource, {
    filename,
    sourceMaps: false,
    jsc: {
      parser: parserOptions(filename, "typescript"),
      target: "es2020",
      transform: {
        react: {
          runtime: "automatic",
        },
      },
    },
    module: {
      type: "commonjs",
    },
    isModule: "unknown",
  });
  return result.code;
}

function runTypeScriptCli({ cwd, arguments = [] }) {
  const packagePath = require.resolve("typescript/package.json", { paths: [cwd] });
  const packageMetadata = require(packagePath);
  const tscPath = path.resolve(path.dirname(packagePath), packageMetadata.bin.tsc);
  const completed = spawnSync(process.execPath, [tscPath, "--pretty", "false", ...arguments], {
    cwd,
    encoding: "utf8",
    windowsHide: true,
  });
  if (completed.error) {
    throw completed.error;
  }
  return {
    status: completed.status,
    output: `${completed.stdout ?? ""}${completed.stderr ?? ""}`.trim(),
  };
}

function visitSyntax(node, visitor) {
  if (!node || typeof node !== "object") {
    return;
  }
  if (visitor(node) === false) {
    return;
  }
  for (const [key, value] of Object.entries(node)) {
    if (key === "span") {
      continue;
    }
    if (Array.isArray(value)) {
      for (const child of value) {
        visitSyntax(child, visitor);
      }
    } else {
      visitSyntax(value, visitor);
    }
  }
}

function sourceText(source, node) {
  if (!node?.span || !Number.isInteger(node.span.start) || !Number.isInteger(node.span.end)) {
    throw new TypeError("SWC syntax node does not expose a valid source span");
  }
  return Buffer.from(source, "utf8")
    .subarray(node.span.start - 1, node.span.end - 1)
    .toString("utf8");
}

module.exports = {
  parseSource,
  sourceText,
  transpileTypeScript,
  runTypeScriptCli,
  visitSyntax,
};
