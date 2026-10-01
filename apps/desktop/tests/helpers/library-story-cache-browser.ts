import { readSavedLibraryStory, saveLibraryStory } from "../../src/views/library/library-story-cache";

const nonce = new URLSearchParams(location.search).get("nonce");
const phaseKey = `story-persistence:${nonce}`;
const errors: string[] = [];
let checks = 0;
const scenarios: string[] = [];
const nativeNow = Date.now;
let now = nativeNow();
Date.now = () => now;
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
  checks++;
}

async function storedRows(): Promise<{ key: string; html: string; savedAt: number }[]> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open("langame-steam-story-cache", 1);
    request.onerror = () => reject(request.error);
    request.onsuccess = () => {
      const db = request.result;
      const tx = db.transaction("stories", "readonly");
      const read = tx.objectStore("stories").getAll();
      tx.oncomplete = () => { db.close(); resolve(read.result); };
      tx.onabort = () => { db.close(); reject(tx.error); };
    };
  });
}

async function run() {
  const key = "story:zh-CN:901100";
  if (!sessionStorage.getItem(phaseKey)) {
    await saveLibraryStory(key, '<p onclick="globalThis.unsafe=true">跨重载简介</p><script>globalThis.unsafe=true</script>');
    check((await readSavedLibraryStory(key))?.html.includes("跨重载简介"), "Initial save failed");
    sessionStorage.setItem(phaseKey, "saved");
    location.reload();
    return null;
  }
  sessionStorage.removeItem(phaseKey);
  const saved = await readSavedLibraryStory(key);
  check(saved?.html.includes("跨重载简介"), "Snapshot did not survive a complete page reload");
  check(!saved.html.includes("script") && !saved.html.includes("onclick"), "Unsafe saved HTML survived");
  check(!("unsafe" in globalThis), "Saved content executed code");
  check(await readSavedLibraryStory("story:en-US:901100") === null, "Cache crossed languages");
  check(await readSavedLibraryStory("story:zh-CN:901101") === null, "Cache crossed games");
  scenarios.push("reload-and-isolation");

  now = saved.savedAt + 30 * 24 * 60 * 60 * 1000 + 1;
  check(await readSavedLibraryStory(key) === null, "Expired description was returned");
  now = saved.savedAt - 1;
  check(await readSavedLibraryStory(key) === null, "Future timestamp was accepted");
  now = nativeNow();
  for (const unavailable of [null, "", "  ", "<script>globalThis.unsafe=true</script>", "x".repeat(256 * 1024 + 1)]) {
    now++;
    await saveLibraryStory(key, unavailable);
    const retained = await readSavedLibraryStory(key);
    check(retained?.html === saved.html, "An unavailable or invalid response erased the valid saved description");
    check(retained.savedAt === saved.savedAt, "An unavailable response refreshed the saved description's age");
  }
  scenarios.push("expiry-preservation-and-size");

  for (let i = 0; i < 66; i++) {
    now++;
    await saveLibraryStory(`story:en-US:${902000 + i}`, `<p>Saved ${i}</p>`);
  }
  let rows = await storedRows();
  check(rows.length === 64, "Entry count was not bounded");
  check(await readSavedLibraryStory("story:en-US:902000") === null, "Oldest entry was not evicted");
  check(Boolean(await readSavedLibraryStory("story:en-US:902065")), "Newest entry was evicted");
  for (let i = 0; i < 18; i++) {
    now++;
    await saveLibraryStory(`story:en-US:${903000 + i}`, "x".repeat(250 * 1024));
  }
  rows = await storedRows();
  const bytes = rows.reduce((sum, row) => sum + new Blob([row.html]).size, 0);
  check(bytes <= 4 * 1024 * 1024, "Snapshot payload exceeded the total byte limit");
  check(Boolean(await readSavedLibraryStory("story:en-US:903017")), "Newest large entry was lost");
  scenarios.push("bounded-retention");

  // Browser storage is an optional external boundary, including enterprise
  // policy denial. Both live-read and local-overview callers must remain usable.
  const descriptor = Object.getOwnPropertyDescriptor(window, "indexedDB");
  Object.defineProperty(window, "indexedDB", { configurable: true, get() { throw new DOMException("Denied", "SecurityError"); } });
  try {
    check(await readSavedLibraryStory(key) === null, "Denied storage did not degrade to a cache miss");
    await saveLibraryStory(key, "<p>Available live content</p>");
  } finally {
    if (descriptor) Object.defineProperty(window, "indexedDB", descriptor);
    else Reflect.deleteProperty(window, "indexedDB");
  }
  scenarios.push("storage-denied");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, scenarios, bytes, browser_errors: errors };
}

void run().catch((error: unknown) => ({ status: "failed", checks, scenarios,
  error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .finally(() => { Date.now = nativeNow; })
  .then((report) => report && fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
