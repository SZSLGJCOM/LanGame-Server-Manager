const fs = require("fs");
const path = require("path");

function collectCatalogFilesBySuffix(root, relativeDirectory, suffix) {
  const absoluteDirectory = path.join(root, relativeDirectory);
  if (!fs.existsSync(absoluteDirectory)) {
    return [];
  }

  return fs
    .readdirSync(absoluteDirectory, { withFileTypes: true })
    .flatMap((entry) => {
      const relativePath = path.join(relativeDirectory, entry.name).replace(/\\/g, "/");
      if (entry.isDirectory()) {
        return collectCatalogFilesBySuffix(root, relativePath, suffix);
      }
      return entry.name.endsWith(suffix) ? [relativePath] : [];
    })
    .sort();
}

function collectLocaleCatalogFiles(root, relativeDirectory, locale) {
  const absoluteDirectory = path.join(root, relativeDirectory);
  if (!fs.existsSync(absoluteDirectory)) {
    return [];
  }

  const escapedLocale = locale.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = new RegExp(`\\.${escapedLocale}(?:\\.part\\d+|\\.chunk-\\d+)?\\.ts$`);
  return fs
    .readdirSync(absoluteDirectory, { withFileTypes: true })
    .flatMap((entry) => {
      const relativePath = path.join(relativeDirectory, entry.name).replace(/\\/g, "/");
      if (entry.isDirectory()) {
        return collectLocaleCatalogFiles(root, relativePath, locale);
      }
      return pattern.test(entry.name) ? [relativePath] : [];
    })
    .sort();
}

module.exports = {
  collectCatalogFilesBySuffix,
  collectLocaleCatalogFiles
};
