import type { SteamWorkshopBrowseKind, SteamWorkshopSearchResult } from "../../types";

export interface WorkshopBrowseRequest {
  appId: number;
  query: string;
  sort: string;
  page: number;
  locale?: string;
  browseKind?: SteamWorkshopBrowseKind;
}

export interface WorkshopBrowseState {
  key: string;
  locale?: string;
  result: SteamWorkshopSearchResult | null;
  loading: boolean;
  error: string | null;
}

export function workshopBrowseKey(request: WorkshopBrowseRequest): string {
  return JSON.stringify([request.appId, request.query.trim(), request.sort, request.page, request.locale, request.browseKind ?? "item"]);
}

/** One owned request at a time; edits replace pending work and never publish stale pages. */
export class WorkshopBrowseController {
  private pending: WorkshopBrowseRequest | null = null;
  private running = false;
  private disposed = false;
  private ready = false;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private cache = new Map<string, { time: number; result: SteamWorkshopSearchResult }>();
  private state: WorkshopBrowseState = { key: "", result: null, loading: false, error: null };

  constructor(
    private readonly search: (request: WorkshopBrowseRequest) => Promise<SteamWorkshopSearchResult>,
    private readonly publish: (state: WorkshopBrowseState) => void
  ) {}

  request(request: WorkshopBrowseRequest, delayMs = 0, refresh = false): void {
    if (this.disposed) return;
    clearTimeout(this.timer);
    const key = workshopBrowseKey(request);
    this.pending = request;
    this.ready = false;
    const cached = refresh ? undefined : this.cache.get(key);
    if (cached && Date.now() - cached.time < 60_000) {
      this.pending = null;
      this.emit({ key, locale: request.locale, result: cached.result, loading: false, error: null });
      return;
    }
    const previous = this.state.result?.app_id === request.appId
      && this.state.locale === request.locale
      && this.state.result.browse_kind === (request.browseKind ?? "item") ? this.state.result : null;
    this.emit({ key, locale: request.locale, result: previous, loading: true, error: null });
    this.timer = setTimeout(() => {
      this.ready = true;
      void this.run();
    }, delayMs);
  }

  dispose(): void {
    this.disposed = true;
    clearTimeout(this.timer);
    this.pending = null;
    this.cache.clear();
  }

  private emit(state: WorkshopBrowseState): void {
    this.state = state;
    this.publish(state);
  }

  private async run(): Promise<void> {
    if (this.disposed || this.running || !this.ready || !this.pending) return;
    const request = this.pending;
    const key = workshopBrowseKey(request);
    this.pending = null;
    this.running = true;
    try {
      const result = await this.search(request);
      if (this.disposed) return;
      // IPC responses are runtime data. An incompatible catalog is a load failure,
      // never a successful empty page; validate before publishing or caching it.
      if (!result || typeof result !== "object" || result.app_id !== request.appId
        || result.browse_kind !== (request.browseKind ?? "item") || !Array.isArray(result.items)) {
        throw new Error(JSON.stringify({
          code: "steam_workshop_browse_invalid_response",
          message: "The Workshop browse response does not match the requested catalog."
        }));
      }
      this.cache.delete(key);
      this.cache.set(key, { result, time: Date.now() });
      while (this.cache.size > 20) {
        const oldest = this.cache.keys().next().value;
        if (oldest !== undefined) this.cache.delete(oldest);
      }
      if (this.state.key === key && !this.pending) {
        this.emit({ key, locale: request.locale, result, loading: false, error: null });
      }
    } catch (error) {
      if (!this.disposed && this.state.key === key && !this.pending) {
        this.emit({ ...this.state, loading: false, error: error instanceof Error ? error.message : String(error) });
      }
    } finally {
      this.running = false;
      if (!this.disposed) void this.run();
    }
  }
}
