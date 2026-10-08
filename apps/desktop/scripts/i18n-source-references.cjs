const { parseSource, visitSyntax } = require("./typescript_source_tools.cjs");
const ts = require("@typescript/typescript6");

const TRANSLATION_KEY_PROPERTY = /^(?:label|title|description|placeholder|help|summary|action|message|hint|error|warning|empty|status)Key$/;
const KEY_LITERAL = /^[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)+$/;

function stringLiterals(expression) {
  if (!expression) return [];
  if (ts.isStringLiteralLike(expression)) return [expression];
  if (ts.isConditionalExpression(expression)) {
    return [...stringLiterals(expression.whenTrue), ...stringLiterals(expression.whenFalse)];
  }
  if (ts.isParenthesizedExpression(expression) || ts.isAsExpression(expression)
    || ts.isTypeAssertionExpression(expression) || ts.isSatisfiesExpression(expression)) {
    return stringLiterals(expression.expression);
  }
  return [];
}

function collectTranslationReferences(source, filename) {
  const syntax = parseSource(source, filename);
  const messageNames = new Set();
  const keyParameters = new Map();
  const references = new Map();
  const add = (expression, requireKeyShape = false) => {
    for (const literal of stringLiterals(expression)) {
      if (requireKeyShape && !KEY_LITERAL.test(literal.text)) continue;
      const line = syntax.getLineAndCharacterOfPosition(literal.getStart(syntax)).line + 1;
      const location = `${filename}:${line}`;
      const locations = references.get(literal.text) ?? [];
      if (!locations.includes(location)) locations.push(location);
      references.set(literal.text, locations);
    }
  };

  visitSyntax(syntax, (node) => {
    if (ts.isImportDeclaration(node) && /(?:^|\/)app-ui(?:\.ts)?$/.test(node.moduleSpecifier.text)) {
      const bindings = node.importClause?.namedBindings;
      for (const specifier of bindings && ts.isNamedImports(bindings) ? bindings.elements : []) {
        if ((specifier.propertyName?.text ?? specifier.name.text) === "message") {
          messageNames.add(specifier.name.text);
        }
      }
    }
    if (ts.isFunctionDeclaration(node) && node.name) {
      const positions = node.parameters.flatMap((parameter, index) =>
        ts.isIdentifier(parameter.name) && TRANSLATION_KEY_PROPERTY.test(parameter.name.text) ? [index] : []);
      if (positions.length) keyParameters.set(node.name.text, positions);
    }
  });

  visitSyntax(syntax, (node) => {
    if (ts.isCallExpression(node)) {
      const calleeName = ts.isIdentifier(node.expression) ? node.expression.text : null;
      const isTranslation = calleeName === "t" || messageNames.has(calleeName)
        || (ts.isPropertyAccessExpression(node.expression) && node.expression.name.text === "t");
      if (isTranslation) add(node.arguments[0]);
      for (const index of keyParameters.get(calleeName) ?? []) add(node.arguments[index], true);
    }
    if (ts.isPropertyAssignment(node) && TRANSLATION_KEY_PROPERTY.test(node.name.text ?? "")) {
      add(node.initializer, true);
    }
    if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.EqualsToken
      && ts.isIdentifier(node.left) && /(?:Key|Label)$/.test(node.left.text)) {
      add(node.right, true);
    }
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && /(?:Key|Label)$/.test(node.name.text)) {
      add(node.initializer, true);
    }
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && /MessageKeys$/.test(node.name.text)
      && node.initializer && ts.isObjectLiteralExpression(node.initializer)) {
      for (const property of node.initializer.properties) {
        if (ts.isPropertyAssignment(property)) add(property.initializer, true);
      }
    }
  });
  return references;
}

function extractMessagePlaceholders(value) {
  return Array.from(value.matchAll(/\{\s*([\w.]+)\s*\}/g), (match) => match[1]).sort();
}

module.exports = { collectTranslationReferences, extractMessagePlaceholders };
