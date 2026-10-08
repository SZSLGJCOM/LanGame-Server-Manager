const path = require("node:path");
const { spawnSync } = require("node:child_process");
// TypeScript 7 owns type-checking; Microsoft's current compiler-API package
// provides syntax inspection and fixture emission until its native API is stable.
const ts = require("@typescript/typescript6");

function parseSource(source, filename = "module.ts") {
  const syntax = ts.createSourceFile(filename, source, ts.ScriptTarget.Latest, true);
  if (syntax.parseDiagnostics.length) {
    throw new SyntaxError(syntax.parseDiagnostics.map((diagnostic) =>
      ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")).join("\n"));
  }
  return syntax;
}

function transpileTypeScript(source, filename = "module.ts", { development = true } = {}) {
  // Node fixtures do not run through Vite. Supply the same compile-time mode
  // constant explicitly; transport regressions also exercise production mode.
  const resolvedSource = source.replaceAll("import.meta.env.DEV", String(development));
  const result = ts.transpileModule(resolvedSource, {
    fileName: filename,
    reportDiagnostics: true,
    compilerOptions: {
      target: ts.ScriptTarget.ES2020,
      module: ts.ModuleKind.CommonJS,
      jsx: ts.JsxEmit.ReactJSX,
      esModuleInterop: true,
      sourceMap: false,
    },
  });
  const errors = result.diagnostics?.filter((diagnostic) => diagnostic.category === ts.DiagnosticCategory.Error) ?? [];
  if (errors.length) {
    throw new SyntaxError(errors.map((diagnostic) =>
      ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")).join("\n"));
  }
  return result.outputText;
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
  ts.forEachChild(node, (child) => { visitSyntax(child, visitor); });
}

function sourceText(source, node) {
  if (!node || typeof node.getStart !== "function" || !Number.isInteger(node.end)) {
    throw new TypeError("TypeScript syntax node does not expose a valid source range");
  }
  return source.slice(node.getStart(), node.end);
}

module.exports = {
  parseSource,
  sourceText,
  transpileTypeScript,
  runTypeScriptCli,
  visitSyntax,
};
