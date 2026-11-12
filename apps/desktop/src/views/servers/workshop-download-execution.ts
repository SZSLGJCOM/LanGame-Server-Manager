import {
  commitSteamWorkshopItems, discardSteamWorkshopPreparation, downloadSteamWorkshopItems, prepareSteamWorkshopItems,
  retainSteamWorkshopPreparation
} from "../../api";
import type { SteamWorkshopDownloadResult, SteamWorkshopPreparation } from "../../types";
import { requiresSteamWorkshopAccount } from "./steam-workshop-access-error";
import type { WorkshopDownloadOperation } from "./workshop-download-operations";

interface WorkshopDownloadExecution {
  instanceId: string;
  ids: string[];
  loginAccount?: string;
  onAccountRequired?: (retry: (accountName: string) => Promise<SteamWorkshopDownloadResult>) => Promise<SteamWorkshopDownloadResult>;
}

/** Preparation owns only a native ticket. The caller's entire deployment and
 * settings transaction runs in its original per-instance submission order. */
export async function executeWorkshopDownload(
  operation: WorkshopDownloadOperation,
  request: WorkshopDownloadExecution,
  apply: (download: () => Promise<SteamWorkshopDownloadResult | null>) => Promise<void>
): Promise<void> {
  let prepared: SteamWorkshopPreparation | null = null;
  let accountRequired = false;
  if (request.ids.length && request.loginAccount === undefined) {
    try {
      prepared = await prepareSteamWorkshopItems(request.instanceId, request.ids, operation.beginAttempt());
    } catch (error) {
      if (!requiresSteamWorkshopAccount(error) || !request.onAccountRequired) throw error;
      accountRequired = true;
    }
  }
  const discard = async () => {
    if (prepared) await discardSteamWorkshopPreparation(request.instanceId, prepared.ticket_id);
  };
  let retaining = false;
  const lease = prepared ? setInterval(() => {
    if (!prepared || retaining) return;
    retaining = true;
    void retainSteamWorkshopPreparation(request.instanceId, prepared.ticket_id)
      // A transport interruption does not establish whether the lease expired.
      // Retry on the next interval; commit authoritatively validates the ticket.
      .catch(() => undefined).finally(() => { retaining = false; });
  }, 60_000) : null;
  try {
    await operation.apply(() => apply(async () => {
      if (!request.ids.length) return null;
      if (prepared) {
        const result = await commitSteamWorkshopItems(request.instanceId, prepared.ticket_id);
        prepared = null;
        return result;
      }
      // Account sessions keep their existing serialized SteamCMD path. An
      // anonymous authentication failure opens that flow only at commit time.
      const download = (accountName: string | undefined) => downloadSteamWorkshopItems(
        request.instanceId, request.ids, true, accountName, operation.beginAttempt()
      );
      return accountRequired ? request.onAccountRequired!(download) : download(request.loginAccount);
    }));
  } catch (error) {
    try { await discard(); }
    catch (cleanupError) {
      const message = (value: unknown) => value instanceof Error ? value.message : String(value);
      throw new Error(`${message(error)}; Workshop preparation cleanup failed: ${message(cleanupError)}`);
    }
    throw error;
  } finally {
    if (lease !== null) clearInterval(lease);
  }
  await discard();
}
