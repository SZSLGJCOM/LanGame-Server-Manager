import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useInstanceSettingsSaveCoordinator } from "./InstanceSettingsSaveContext";
import type {
  InstanceDetails,
  PortBinding,
  SaveInstanceSettingsOptions,
  UpdateInstanceInput
} from "../../types";
import {
  InstanceSettingsDraftInvalidError,
  InstanceSettingsSaveQueue,
  type InstanceSettingsSaveRequest,
  type InstanceSettingsSaveStatus
} from "./instance-settings-save-queue";

interface AutoSavePayload {
  details: InstanceDetails;
  bindIp: string;
  autoBackupOnStop: boolean;
  backupRetentionCount: string;
  settingsJson: string;
  ports?: PortBinding[];
}

interface UseAutoSaveInstanceSettingsOptions extends AutoSavePayload {
  enabled?: boolean;
  delayMs?: number;
  /** False while the editor cannot validate its projection against loaded module metadata. */
  ready?: boolean;
  disabled?: boolean;
  onSave?: (input: UpdateInstanceInput, options?: SaveInstanceSettingsOptions) => Promise<InstanceDetails | undefined>;
}

export interface UseAutoSaveInstanceSettingsResult {
  status: InstanceSettingsSaveStatus;
  retry(): void;
}

function normalizeBackupRetentionCount(value: string, fallback: number): number {
  const parsed = Number.parseInt(value, 10);
  return Number.isNaN(parsed) ? Math.max(1, fallback) : Math.max(1, parsed);
}

export function buildAutoSaveInstanceSettingsInput(payload: AutoSavePayload): UpdateInstanceInput {
  return {
    id: payload.details.summary.id,
    bind_ip: payload.bindIp,
    auto_backup_on_stop: payload.autoBackupOnStop,
    backup_retention_count: normalizeBackupRetentionCount(
      payload.backupRetentionCount,
      payload.details.backup_retention_count
    ),
    settings_json: payload.settingsJson,
    ports: payload.ports ?? payload.details.ports
  };
}

export function instanceSettingsInputSignature(input: UpdateInstanceInput): string {
  return JSON.stringify({
    id: input.id,
    bind_ip: input.bind_ip,
    auto_backup_on_stop: input.auto_backup_on_stop,
    backup_retention_count: input.backup_retention_count,
    settings_json: input.settings_json,
    ports: input.ports.map((port) => ({
      name: port.name,
      port: port.port,
      protocol: port.protocol
    }))
  });
}

function buildPersistedDetailsSignature(details: InstanceDetails): string {
  return instanceSettingsInputSignature({
    id: details.summary.id,
    bind_ip: details.summary.bind_ip,
    auto_backup_on_stop: details.auto_backup_on_stop,
    backup_retention_count: Math.max(1, details.backup_retention_count),
    settings_json: details.settings_json,
    ports: details.ports
  });
}

export function useAutoSaveInstanceSettings(
  options: UseAutoSaveInstanceSettingsOptions
): UseAutoSaveInstanceSettingsResult {
  const coordinator = useInstanceSettingsSaveCoordinator();
  const input = useMemo(
    () => buildAutoSaveInstanceSettingsInput(options),
    [
      options.autoBackupOnStop,
      options.backupRetentionCount,
      options.bindIp,
      options.details,
      options.ports,
      options.settingsJson
    ]
  );
  const signature = useMemo(() => instanceSettingsInputSignature(input), [input]);
  const delayMs = options.delayMs ?? 650;
  const [status, setStatus] = useState<InstanceSettingsSaveStatus>({ state: "saved" });
  const saveRef = useRef(options.onSave);
  const saveQueueRef = useRef<InstanceSettingsSaveQueue | null>(null);
  const saveQueueInstanceIdRef = useRef<string | null>(null);
  const latestValidRequestRef = useRef(new Map<string, InstanceSettingsSaveRequest | null>());
  const latestValidRequest = options.disabled ? null : { input, signature };
  if (options.enabled !== false && options.ready !== false) {
    latestValidRequestRef.current.set(input.id, latestValidRequest);
  }
  if (options.enabled !== false && (!saveQueueRef.current || saveQueueInstanceIdRef.current !== input.id)) {
    saveQueueRef.current = new InstanceSettingsSaveQueue({
      instanceId: options.details.summary.id,
      settingsBaseline: options.details.settings_json,
      savedSignature: buildPersistedDetailsSignature(options.details),
      execute: async (nextInput, expectedSettingsJson) => {
        if (!saveRef.current) throw new Error("Settings are read only.");
        const saved = await saveRef.current(nextInput, {
          expectedSettingsJson,
          silent: true,
          throwOnError: true
        });
        if (!saved) throw new Error("Saved settings were not returned by the server.");
        return saved.settings_json;
      },
      onStatusChange: setStatus
    });
    saveQueueInstanceIdRef.current = input.id;
  }

  useLayoutEffect(() => {
    saveRef.current = options.onSave;
  }, [options.onSave]);

  useLayoutEffect(() => {
    if (options.enabled === false) return;
    const queue = saveQueueRef.current;
    const instanceId = options.details.summary.id;
    if (options.ready !== false) {
      latestValidRequestRef.current.set(instanceId, latestValidRequest);
    }
    queue?.reset(
      instanceId,
      options.details.settings_json,
      buildPersistedDetailsSignature(options.details)
    );
    const registration = coordinator.register(instanceId, async () => {
      for (;;) {
        if (!latestValidRequestRef.current.has(instanceId)) return;
        const request = latestValidRequestRef.current.get(instanceId) ?? null;
        try {
          await queue?.flushLatest(request);
        } catch (error) {
          if (latestValidRequestRef.current.get(instanceId)?.signature === request?.signature) throw error;
          continue;
        }
        if (latestValidRequestRef.current.get(instanceId)?.signature === request?.signature) return;
      }
    }, queue ?? undefined);
    return () => {
      const hasDraft = latestValidRequestRef.current.has(instanceId);
      const finalRequest = latestValidRequestRef.current.get(instanceId) ?? null;
      latestValidRequestRef.current.delete(instanceId);
      queue?.flushAndDispose(finalRequest);
      const completion = queue?.whenIdle() ?? Promise.resolve();
      const rejectInvalid = () => {
        throw new InstanceSettingsDraftInvalidError();
      };
      registration.detach(!hasDraft || finalRequest ? completion : completion.then(rejectInvalid, rejectInvalid));
    };
  }, [coordinator, options.details.summary.id, options.enabled]);

  useEffect(() => {
    // Loading a schema is not a user draft. Preserve an earlier draft if metadata reloads.
    if (options.enabled === false || options.ready === false) return;
    const queue = saveQueueRef.current;
    const shouldEnqueue = queue?.markDirty(signature) ?? false;
    if (options.disabled || !shouldEnqueue) {
      return;
    }

    const timer = window.setTimeout(() => {
      queue?.enqueue({ input, signature });
    }, delayMs);

    return () => window.clearTimeout(timer);
  }, [delayMs, input, options.disabled, options.enabled, options.ready, signature]);

  const retry = useCallback(() => {
    saveQueueRef.current?.retry();
  }, []);

  return { status, retry };
}
