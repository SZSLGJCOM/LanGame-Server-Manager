const { parseSource, visitSyntax } = require("./typescript_source_tools.cjs");
const ts = require("@typescript/typescript6");

const TRANSLATION_KEY_PROPERTY = /^(?:label|title|description|placeholder|help|summary|action|message|hint|error|warning|empty|status)Key$/;
const KEY_LITERAL = /^[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)+$/;

function stringLiterals(expression, resolve, seen = new Set()) {
  if (!expression || seen.has(expression)) return [];
  seen.add(expression);
  if (ts.isIdentifier(expression)) {
    return (resolve(expression) ?? []).flatMap(value => stringLiterals(value, resolve, seen));
  }
  if (ts.isStringLiteralLike(expression)) return [expression];
  if (ts.isConditionalExpression(expression)) {
    return [...stringLiterals(expression.whenTrue, resolve, seen), ...stringLiterals(expression.whenFalse, resolve, seen)];
  }
  if (ts.isParenthesizedExpression(expression) || ts.isAsExpression(expression)
    || ts.isTypeAssertionExpression(expression) || ts.isSatisfiesExpression(expression)) {
    return stringLiterals(expression.expression, resolve, seen);
  }
  return [];
}

function collectTranslationReferences(source, filename) {
  const syntax = parseSource(source, filename);
  const messageNames = new Set();
  const keyParameters = new Map();
  const references = new Map();
  const bindings = new Map();
  const isScope = node => ts.isSourceFile(node) || ts.isBlock(node) || ts.isFunctionLike(node)
    || ts.isCatchClause(node) || ts.isForStatement(node) || ts.isForInStatement(node) || ts.isForOfStatement(node);
  const scopeOf = node => {
    for (let scope = node.parent; scope; scope = scope.parent) if (isScope(scope)) return scope;
    return syntax;
  };
  const resolveBinding = identifier => {
    for (let scope = identifier.parent; scope; scope = scope.parent) {
      const binding = bindings.get(scope)?.get(identifier.text);
      if (binding) return binding;
    }
    return null;
  };
  const hasKeyType = type => type && ts.isIndexedAccessTypeNode(type)
    && ts.isLiteralTypeNode(type.indexType) && ts.isStringLiteralLike(type.indexType.literal)
    && TRANSLATION_KEY_PROPERTY.test(type.indexType.literal.text);
  // Resolve literal values from translation consumers, not arbitrary *Key names.
  // Keep lexical bindings separate so a storage key cannot shadow a UI key.
  visitSyntax(syntax, node => {
    if (!(ts.isVariableDeclaration(node) || ts.isParameter(node)) || !ts.isIdentifier(node.name)) return;
    const scope = scopeOf(node);
    const declarations = bindings.get(scope) ?? new Map();
    declarations.set(node.name.text, { values: node.initializer ? [node.initializer] : [], keyType: hasKeyType(node.type) });
    bindings.set(scope, declarations);
  });
  visitSyntax(syntax, node => {
    if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.EqualsToken && ts.isIdentifier(node.left)) {
      resolveBinding(node.left)?.values.push(node.right);
    }
  });
  const add = (expression, requireKeyShape = false) => {
    for (const literal of stringLiterals(expression, identifier => resolveBinding(identifier)?.values)) {
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
      && ts.isIdentifier(node.left) && resolveBinding(node.left)?.keyType) {
      add(node.right, true);
    }
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && hasKeyType(node.type)) {
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
