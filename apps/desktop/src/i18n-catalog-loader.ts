export function createCatalogLoader<Locale extends string, Catalog>(
  cache: Partial<Record<Locale, Catalog>>,
  importCatalog: (locale: Locale) => Promise<Catalog>
) {
  const pendingLoads = new Map<Locale, Promise<Catalog>>();

  function load(locale: Locale): Promise<Catalog> {
    const cached = cache[locale];
    if (cached) {
      return Promise.resolve(cached);
    }

    const pending = pendingLoads.get(locale);
    if (pending) {
      return pending;
    }

    const request = Promise.resolve()
      .then(() => importCatalog(locale))
      .then((catalog) => {
        cache[locale] = catalog;
        return catalog;
      });

    pendingLoads.set(locale, request);
    const clearPending = () => {
      if (pendingLoads.get(locale) === request) {
        pendingLoads.delete(locale);
      }
    };
    void request.then(clearPending, clearPending);
    return request;
  }

  function invalidate(locale: Locale) {
    delete cache[locale];
    pendingLoads.delete(locale);
  }

  return { invalidate, load };
}
