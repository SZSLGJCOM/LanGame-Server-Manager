// Only successfully sanitized public descriptions are retained here. This cache
// is optional: storage denial, eviction and corruption cannot block live reads.
import { sanitizeSteamAboutHtml } from "./steam-about-html";

const DATABASE = "langame-steam-story-cache";
const STORE = "stories";
const MAX_ENTRIES = 64;
const MAX_ENTRY_BYTES = 256 * 1024;
const MAX_TOTAL_BYTES = 4 * 1024 * 1024;
const MAX_AGE_MS = 30 * 24 * 60 * 60 * 1000;
const STORAGE_BUDGET_MS = 1500;

export interface SavedLibraryStory {
  key: string;
  html: string;
  savedAt: number;
}

function validSnapshot(value: unknown, now: number): value is SavedLibraryStory {
  if (!value || typeof value !== "object") return false;
  const entry = value as Partial<SavedLibraryStory>;
  return typeof entry.key === "string" && /^story:(zh-CN|en-US):[1-9]\d*$/.test(entry.key)
    && typeof entry.html === "string" && entry.html.trim().length > 0
    && entry.html.length <= MAX_ENTRY_BYTES && new Blob([entry.html]).size <= MAX_ENTRY_BYTES
    && typeof entry.savedAt === "number" && Number.isSafeInteger(entry.savedAt)
    && entry.savedAt <= now && now - entry.savedAt <= MAX_AGE_MS;
}

function inStore<T>(mode: IDBTransactionMode, fallback: T,
  action: (store: IDBObjectStore, result: (value: T) => void) => void): Promise<T> {
  return new Promise((resolve) => {
    let settled = false;
    let database: IDBDatabase | undefined;
    let transaction: IDBTransaction | undefined;
    let result = fallback;
    const finish = (value: T) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      database?.close();
      resolve(value);
    };
    const timer = setTimeout(() => {
      finish(fallback);
      try { transaction?.abort(); } catch { /* It may have completed already. */ }
    }, STORAGE_BUDGET_MS);
    try {
      const request = indexedDB.open(DATABASE, 1);
      request.onupgradeneeded = () => request.result.createObjectStore(STORE, { keyPath: "key" });
      request.onerror = request.onblocked = () => finish(fallback);
      request.onsuccess = () => {
        database = request.result;
        if (settled) { database.close(); return; }
        database.onversionchange = () => { database?.close(); finish(fallback); };
        try {
          transaction = database.transaction(STORE, mode);
          transaction.oncomplete = () => finish(result);
          transaction.onabort = transaction.onerror = () => finish(fallback);
          action(transaction.objectStore(STORE), (value) => { result = value; });
        } catch { finish(fallback); }
      };
    } catch { finish(fallback); }
  });
}

export async function readSavedLibraryStory(key: string): Promise<SavedLibraryStory | null> {
  const entry = await inStore<unknown>("readonly", null, (store, result) => {
    const request = store.get(key);
    request.onsuccess = () => result(request.result);
  });
  if (!validSnapshot(entry, Date.now()) || entry.key !== key) return null;
  // Disk content is untrusted and sanitizer rules can change between releases.
  const html = sanitizeSteamAboutHtml(entry.html);
  return html ? { ...entry, html } : null;
}

export async function saveLibraryStory(key: string, html: string | null): Promise<void> {
  // An empty public response is not authoritative deletion. Keep the last
  // successful description and its original age for an explicit saved fallback.
  if (!html) return;
  const snapshot = { key, html: sanitizeSteamAboutHtml(html), savedAt: Date.now() };
  if (!validSnapshot(snapshot, Date.now())) return;
  await inStore("readwrite", undefined, (store) => {
    // A single transaction serializes writers across windows and tabs. Rebuild
    // this cache's bounded set; no user-authored content lives in this database.
    const request = store.getAll(undefined, MAX_ENTRIES + 1);
    request.onsuccess = () => {
      const now = Date.now();
      const entries: SavedLibraryStory[] = (request.result as unknown[])
        .filter((entry): entry is SavedLibraryStory => validSnapshot(entry, now) && entry.key !== key);
      entries.push(snapshot);
      entries.sort((a, b) => b.savedAt - a.savedAt || a.key.localeCompare(b.key));
      store.clear();
      let bytes = 0;
      let count = 0;
      for (const entry of entries) {
        const size = new Blob([entry.html]).size;
        if (count >= MAX_ENTRIES || bytes + size > MAX_TOTAL_BYTES) continue;
        store.put(entry);
        bytes += size;
        count++;
      }
    };
  });
}
