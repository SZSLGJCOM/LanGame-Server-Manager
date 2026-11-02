import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { storageMigrationApi } from "../api-storage-migration";
import { SingleFlightPoller } from "../domain/single-flight-poller";
import { storageMigrationActive, storageMigrationTerminal, type StorageMigrationApi, type StorageMigrationStatus } from "../storage-migration-types";
import type { AppPathSettingsInput } from "../types";
import { appPathSettingsEqual, createAppPathMigration, createAppPathSettingsDraft } from "../views/system/app-path-settings-draft";

type Command = "preview" | "execute" | "cancel";
export interface StorageMigrationState {
  status: StorageMigrationStatus | null;
  draft: AppPathSettingsInput | null;
  recovering: boolean;
  pending: Command | null;
  unconfirmed: boolean;
  error: string | null;
  cancelRequested: boolean;
  refreshing: boolean;
  refreshError: string | null;
  dismissError: string | null;
  hasActivity: boolean;
  canDismiss: boolean;
}
const describe = (error: unknown) => error instanceof Error ? error.message : String(error);

export type StorageMigrationDismissalStorage = Pick<Storage, "getItem" | "setItem">;
const dismissalKey = "langame.storage-migration.dismissed.v1";
const dismissalLimit = 32;
const browserDismissalStorage: StorageMigrationDismissalStorage = {
  getItem: key => window.localStorage.getItem(key),
  setItem: (key, value) => window.localStorage.setItem(key, value)
};
function readDismissals(storage: StorageMigrationDismissalStorage): string[] {
  const raw = storage.getItem(dismissalKey);
  if (raw === null) return [];
  if (raw.length > 16384) throw new Error("Saved migration notification history is too large.");
  const entries: unknown = JSON.parse(raw);
  if (!Array.isArray(entries) || entries.some(entry => typeof entry !== "string" || !entry || entry.length > 256)) {
    throw new Error("Saved migration notification history is invalid.");
  }
  return [...new Set(entries as string[])].slice(-dismissalLimit);
}

interface Options<Timer> {
  dismissalStorage: StorageMigrationDismissalStorage;
  api: StorageMigrationApi;
  onCompleted: (status: StorageMigrationStatus) => Promise<void>;
  schedule: (callback: () => void, milliseconds: number) => Timer;
  cancelTimer: (timer: Timer) => void;
}

// One application-owned observer. Native storage owns the migration and its journal.
export function createStorageMigrationController<Timer>(options: Options<Timer>) {
  const available = options.api.available();
  let dismissedIds: string[] = [];
  let dismissError: string | null = null;
  if (available) {
    try { dismissedIds = readDismissals(options.dismissalStorage); }
    catch (error) { dismissError = describe(error); }
  }
  let state: StorageMigrationState = { status: null, draft: null, recovering: available, pending: null, unconfirmed: false,
    error: null, cancelRequested: false, refreshing: false, refreshError: null, dismissError, hasActivity: available, canDismiss: false };
  const listeners = new Set<() => void>();
  let epoch = 0;
  let connected = false;
  let poller: SingleFlightPoller<StorageMigrationStatus | null, Timer> | null = null;
  let expectedId: string | null = null;
  let unknownCommand: Command | null = null;
  let refreshedId: string | null = null;
  let automatic: { input: AppPathSettingsInput; id: string | null; previousId: string | null } | null = null;
  let previewPreviousId: string | null = null;
  let revealDismissed = false;
  const sameInput = (left: AppPathSettingsInput, right: AppPathSettingsInput) =>
    left.games_root === right.games_root && left.servers_root === right.servers_root
    && left.archives_root === right.archives_root && left.steamcmd_root === right.steamcmd_root;
  const commandBlocked = () => !available || !connected || Boolean(state.pending) || state.recovering
    || state.unconfirmed || state.refreshing || state.cancelRequested || Boolean(state.status?.recovery_required);
  function publish(patch: Partial<StorageMigrationState>) {
    const next = { ...state, ...patch };
    const unsafeToDismiss = next.recovering || Boolean(next.pending) || next.unconfirmed || next.cancelRequested
      || storageMigrationActive(next.status) || Boolean(next.status?.recovery_required);
    // Native recovery state always takes precedence over a notification acknowledgement.
    const acknowledged = Boolean(next.status && dismissedIds.includes(next.status.operation_id));
    const hasActivity = available && (unsafeToDismiss || Boolean(next.status || next.error || next.refreshError)
      && (revealDismissed || !acknowledged));
    const finished = next.status ? storageMigrationTerminal(next.status)
      || next.status.phase === "ready" && (next.status.conflict_count > 0 || !automatic) : Boolean(next.error);
    state = { ...next, hasActivity, canDismiss: connected && hasActivity && !unsafeToDismiss && !next.refreshing && finished };
    for (const listener of listeners) listener();
  }
  const current = (token: number) => connected && epoch === token;

  async function refresh(reveal = true) {
    const status = state.status;
    if (!status || status.phase !== "completed" || state.refreshing) return;
    const token = epoch;
    if (reveal) revealDismissed = true;
    refreshedId = status.operation_id;
    publish({ refreshing: true, refreshError: null });
    try { await options.onCompleted(status); }
    catch (error) { if (current(token)) publish({ refreshError: describe(error) }); }
    finally { if (current(token)) publish({ refreshing: false }); }
  }

  function receive(status: StorageMigrationStatus | null, fromRead: boolean) {
    if (status && expectedId && status.operation_id !== expectedId) {
      automatic = null;
      publish({ recovering: false, unconfirmed: true, error: "The current migration identity changed. Check the existing operation before starting another." });
      return;
    }
    if (!status && state.unconfirmed && unknownCommand !== "preview") {
      publish({ recovering: false, error: "Migration outcome is unconfirmed; its progress is currently unavailable." });
      return;
    }
    if (!status && storageMigrationActive(state.status)) {
      publish({ recovering: false, unconfirmed: true, error: "Migration progress is unavailable. The operation may still be running." });
      return;
    }
    // A lost preview receipt cannot authorize execution, nor can an older
    // completed operation erase the new selection while resolving that receipt.
    if (fromRead && unknownCommand === "preview" && status?.operation_id === previewPreviousId) {
      unknownCommand = null;
      publish({ status: null, recovering: false, unconfirmed: false });
      poller?.dispose(); poller = null;
      return;
    }
    if (automatic && automatic.id === status?.operation_id && status && !sameInput(automatic.input, status.input)) {
      automatic = null;
      publish({ recovering: false, unconfirmed: true, error: "Migration targets changed unexpectedly. Check the existing operation." });
      return;
    }
    // A late cancellation receipt cannot replace a terminal native result.
    if (status && state.status?.operation_id === status.operation_id
      && storageMigrationTerminal(state.status) && (!fromRead || !storageMigrationTerminal(status))) return;
    const requestError = state.unconfirmed ? state.error : null;
    unknownCommand = null;
    if (status) expectedId = status.operation_id;
    const recoveredDraft = state.recovering && status && !dismissedIds.includes(status.operation_id)
      && !["completed", "cancelled"].includes(status.phase)
      ? status.input : state.draft;
    publish({ status, draft: status?.phase === "completed" ? null : recoveredDraft, recovering: false, unconfirmed: false,
      error: fromRead && status && storageMigrationActive(status) ? null : requestError,
      cancelRequested: status && !storageMigrationTerminal(status) ? state.cancelRequested : false });
    if (!storageMigrationActive(status) && !state.cancelRequested) { poller?.dispose(); poller = null; }
    if (status?.phase === "completed" && refreshedId !== status.operation_id) void refresh(false);
    executeSelectionWhenReady();
  }

  function observe() {
    poller?.dispose();
    const token = epoch;
    poller = new SingleFlightPoller({
      intervalMs: 1000,
      poll: options.api.read,
      schedule: options.schedule,
      cancel: options.cancelTimer,
      onValue: value => { if (current(token)) receive(value, true); },
      onError: error => { if (current(token)) publish({ error: describe(error) }); }
    });
    poller.start();
  }

  async function command(kind: Command, action: () => Promise<StorageMigrationStatus>) {
    if (commandBlocked()) return;
    const token = epoch;
    publish({ pending: kind, error: null, refreshError: null });
    try {
      const result = await action();
      if (!current(token)) return;
      if (kind === "preview" && automatic) {
        if (result.operation_id === automatic.previousId || !sameInput(result.input, automatic.input)) {
          automatic = null;
          publish({ unconfirmed: true, error: "The migration check returned another selection. Check the existing operation." });
          observe();
          return;
        }
        automatic.id = result.operation_id;
      }
      receive(result, false);
      if (kind === "cancel" && state.status && !storageMigrationTerminal(state.status)) publish({ cancelRequested: true });
      if (storageMigrationActive(state.status) || state.cancelRequested) observe();
    } catch (error) {
      if (!current(token)) return;
      if (kind !== "preview" && state.status && storageMigrationTerminal(state.status)) return;
      // A rejected transport receipt does not prove the native command failed.
      automatic = null;
      unknownCommand = kind;
      publish({ unconfirmed: true, error: describe(error), cancelRequested: kind === "cancel" });
      observe();
    } finally {
      if (current(token)) {
        publish({ pending: null });
        executeSelectionWhenReady();
      }
    }
  }

  function executeSelectionWhenReady() {
    const status = state.status;
    if (!automatic || !status) return;
    if (storageMigrationTerminal(status) || status.conflict_count > 0) { automatic = null; return; }
    if (commandBlocked() || status.phase !== "ready" || !automatic.id) return;
    if (status.operation_id !== automatic.id || !sameInput(status.input, automatic.input)) return;
    // Consume before sending. Polls, remounts and a lost execution receipt cannot
    // replay this choice; only another explicit selection/retry creates intent.
    automatic = null;
    void command("execute", () => options.api.execute(status.operation_id));
  }

  function dismiss() {
    if (commandBlocked() || !state.canDismiss) return;
    const id = state.status?.operation_id;
    if (id) {
      const next = [...dismissedIds.filter(value => value !== id), id].slice(-dismissalLimit);
      try { options.dismissalStorage.setItem(dismissalKey, JSON.stringify(next)); }
      catch (error) { publish({ dismissError: describe(error) }); return; }
      dismissedIds = next;
    }
    automatic = null;
    revealDismissed = false;
    // Preserve the native result and refreshError: committed paths still depend on them.
    publish({ draft: null, error: null, dismissError: null });
  }

  function cancel() {
    automatic = null;
    const status = state.status;
    if (!status?.can_cancel || state.cancelRequested || storageMigrationTerminal(status)) return Promise.resolve();
    return command("cancel", () => { expectedId = status.operation_id; return options.api.cancel(status.operation_id); });
  }

  return {
    available,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    getSnapshot: () => state,
    connect() {
      connected = true; ++epoch; automatic = null; revealDismissed = false;
      if (available) { expectedId = null; publish({ recovering: true, pending: null, refreshing: false }); observe(); }
    },
    disconnect() { connected = false; ++epoch; automatic = null; poller?.dispose(); poller = null; },
    migrate(input: AppPathSettingsInput) {
      if (commandBlocked() || storageMigrationActive(state.status)) return Promise.resolve();
      previewPreviousId = state.status?.operation_id ?? null;
      automatic = { input: { ...input }, id: null, previousId: previewPreviousId };
      revealDismissed = true;
      publish({ draft: { ...input } });
      return command("preview", () => { expectedId = null; return options.api.preview(input); });
    },
    discard() {
      if (commandBlocked() || storageMigrationActive(state.status)) return Promise.resolve();
      automatic = null;
      publish({ draft: null });
      dismiss();
      return Promise.resolve();
    },
    cancel,
    dismiss,
    refresh: () => refresh(),
    recheck() { if (available && connected && !state.pending) observe(); }
  };
}

export function useStorageMigration(onCompleted: (status: StorageMigrationStatus) => Promise<void>, currentSettings: AppPathSettingsInput, api = storageMigrationApi,
  dismissalStorage = browserDismissalStorage) {
  const completed = useRef(onCompleted);
  completed.current = onCompleted;
  const [controller] = useState(() => createStorageMigrationController({ api, dismissalStorage,
    onCompleted: status => completed.current(status), schedule: (callback, delay) => window.setTimeout(callback, delay),
    cancelTimer: (timer: number) => window.clearTimeout(timer) }));
  const state = useSyncExternalStore(controller.subscribe, controller.getSnapshot, controller.getSnapshot);
  useEffect(() => { controller.connect(); return controller.disconnect; }, [controller]);
  function migrate(input: AppPathSettingsInput) {
    const current = createAppPathSettingsDraft(currentSettings);
    const next = createAppPathMigration(current, input);
    // Retrying an older preview is a new selection, never a replay of its journal.
    if (appPathSettingsEqual(current, next)) return controller.discard();
    return controller.migrate(next);
  }
  return { ...state, available: controller.available, migrate, discard: controller.discard,
    cancel: controller.cancel, dismiss: controller.dismiss, refresh: controller.refresh, recheck: controller.recheck };
}

export type StorageMigrationModel = ReturnType<typeof useStorageMigration>;
