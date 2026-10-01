export async function prepareBrowserLocaleCatalogs(): Promise<void> {
  await Promise.all([
    import("../../src/i18n-messages"),
    import("../../src/i18n-messages-zh-cn"),
  ]);
}
