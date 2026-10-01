const { parseSource, visitSyntax } = require("./typescript_source_tools.cjs");

const TRANSLATION_KEY_PROPERTY = /^(?:label|title|description|placeholder|help|summary|action|message|hint|error|warning|empty|status)Key$/;
const KEY_LITERAL = /^[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)+$/;

function stringLiterals(expression) {
  if (!expression) return [];
  if (expression.type === "StringLiteral") return [expression];
  if (expression.type === "ConditionalExpression") {
    return [...stringLiterals(expression.consequent), ...stringLiterals(expression.alternate)];
  }
  if (expression.type === "TemplateLiteral" && expression.expressions.length === 0) {
    return [{ value: expression.quasis[0].cooked ?? expression.quasis[0].raw, span: expression.span }];
  }
  if (["ParenthesisExpression", "TsAsExpression", "TsConstAssertion", "TsSatisfiesExpression"].includes(expression.type)) {
    return stringLiterals(expression.expression);
  }
  return [];
}

function collectTranslationReferences(source, filename) {
  const syntax = parseSource(source, filename);
  const messageNames = new Set();
  const keyParameters = new Map();
  const references = new Map();
  const bytes = Buffer.from(source, "utf8");
  const add = (expression, requireKeyShape = false) => {
    for (const literal of stringLiterals(expression)) {
      if (requireKeyShape && !KEY_LITERAL.test(literal.value)) continue;
      const line = bytes.subarray(0, literal.span.start - 1).toString("utf8").split(/\r?\n/).length;
      const location = `${filename}:${line}`;
      const locations = references.get(literal.value) ?? [];
      if (!locations.includes(location)) locations.push(location);
      references.set(literal.value, locations);
    }
  };

  visitSyntax(syntax, (node) => {
    if (node.type === "ImportDeclaration" && /(?:^|\/)app-ui(?:\.ts)?$/.test(node.source.value)) {
      for (const specifier of node.specifiers) {
        if (specifier.type === "ImportSpecifier" && (specifier.imported?.value ?? specifier.local.value) === "message") {
          messageNames.add(specifier.local.value);
        }
      }
    }
    if (node.type === "FunctionDeclaration" && node.identifier) {
      const positions = node.params.flatMap((parameter, index) =>
        parameter.pat?.type === "Identifier" && TRANSLATION_KEY_PROPERTY.test(parameter.pat.value) ? [index] : []);
      if (positions.length) keyParameters.set(node.identifier.value, positions);
    }
  });

  visitSyntax(syntax, (node) => {
    if (node.type === "CallExpression") {
      const calleeName = node.callee.type === "Identifier" ? node.callee.value : null;
      const isTranslation = calleeName === "t" || messageNames.has(calleeName)
        || (node.callee.type === "MemberExpression" && node.callee.property.type === "Identifier" && node.callee.property.value === "t");
      if (isTranslation) add(node.arguments[0]?.expression);
      for (const index of keyParameters.get(calleeName) ?? []) add(node.arguments[index]?.expression, true);
    }
    if (node.type === "KeyValueProperty" && TRANSLATION_KEY_PROPERTY.test(node.key.value ?? "")) {
      add(node.value, true);
    }
    if (node.type === "AssignmentExpression" && node.left.type === "Identifier" && /(?:Key|Label)$/.test(node.left.value)) {
      add(node.right, true);
    }
    if (node.type === "VariableDeclarator" && node.id.type === "Identifier" && /(?:Key|Label)$/.test(node.id.value)) {
      add(node.init, true);
    }
    if (node.type === "VariableDeclarator" && node.id.type === "Identifier" && /MessageKeys$/.test(node.id.value)
      && node.init?.type === "ObjectExpression") {
      for (const property of node.init.properties) {
        if (property.type === "KeyValueProperty") add(property.value, true);
      }
    }
  });
  return references;
}

function extractMessagePlaceholders(value) {
  return Array.from(value.matchAll(/\{\s*([\w.]+)\s*\}/g), (match) => match[1]).sort();
}

module.exports = { collectTranslationReferences, extractMessagePlaceholders };
