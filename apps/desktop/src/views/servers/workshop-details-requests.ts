import type { SteamWorkshopLookupItem } from "../../types";

const CACHE_TTL_MS = 60_000;
const MAX_CACHED_DETAILS = 32;

interface WorkshopDetailsRead {
  item: SteamWorkshopLookupItem;
  cached: boolean;
}

/** Reopening verified Mods shares recent reads; collections still refresh their members. */
export class WorkshopDetailsRequests {
  private cache = new Map<string, { item: SteamWorkshopLookupItem; time: number }>();
  private pending = new Map<string, Promise<WorkshopDetailsRead>>();

  constructor(
    private readonly fetch: (id: string, locale: string) => Promise<SteamWorkshopLookupItem>,
    private readonly now: () => number = Date.now
  ) {}

  read(id: string, locale: string): Promise<WorkshopDetailsRead> {
    const key = JSON.stringify([id, locale]);
    const cached = this.cache.get(key);
    if (cached && this.now() - cached.time < CACHE_TTL_MS) return Promise.resolve({ item: cached.item, cached: true });
    this.cache.delete(key);
    const pending = this.pending.get(key);
    if (pending) return pending;
    const request = this.fetch(id, locale).then((item) => {
      // An invalidated older request cannot repopulate the cache after a retry.
      if (this.pending.get(key) === request && item?.id === id
        && item.status === "resolved" && item.item_kind === "item" && !item.localization_warning) {
        this.cache.set(key, { item, time: this.now() });
        while (this.cache.size > MAX_CACHED_DETAILS) this.cache.delete(this.cache.keys().next().value!);
      }
      return { item, cached: false };
    }).finally(() => {
      if (this.pending.get(key) === request) this.pending.delete(key);
    });
    this.pending.set(key, request);
    return request;
  }

  invalidate(id: string, locale: string): void {
    const key = JSON.stringify([id, locale]);
    this.cache.delete(key);
    this.pending.delete(key);
  }

  clear(): void {
    this.cache.clear();
    this.pending.clear();
  }
}
