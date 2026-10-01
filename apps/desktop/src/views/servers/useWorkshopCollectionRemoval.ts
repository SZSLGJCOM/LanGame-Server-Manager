import { useEffect, useRef, useState } from "react";
import { readManualModInventory, readProjectZomboidWorkshopModsSnapshot } from "../../api";
import { formatDesktopError } from "../../desktop-error-message";
import { useI18n } from "../../i18n";
import type { InstanceDetails, SaveInstanceSettingsOptions } from "../../types";
import type { SettingsObject } from "../settings/settings-schema";
import { parseWorkshopIdList } from "../settings/guided-settings";
import { changedSettingKeys } from "./mod-settings-patch";
import {
  buildWorkshopCollectionRemovalPreview, buildWorkshopCollectionRemovalPlan, CollectionRemovalError
} from "./mod-workbench-collection-removal";

type Review = ReturnType<typeof buildWorkshopCollectionRemovalPreview> & { settings: SettingsObject };
interface Props {
  instanceId: string;
  moduleId: string;
  readState: () => Promise<{ details: InstanceDetails; settings: SettingsObject }>;
  runOperation: (operation: () => Promise<void>) => Promise<void>;
  save: (next: SettingsObject, base: SettingsObject, removal?: SaveInstanceSettingsOptions["collectionRemoval"]) => Promise<SettingsObject>;
  onRemoved: (id: string) => void;
}

/** A review owns one instance and one saved membership snapshot until it commits. */
export function useWorkshopCollectionRemoval(props: Props) {
  const { t } = useI18n();
  const [review, setReview] = useState<Review | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const scope = useRef(0);
  const pending = useRef(false);
  useEffect(() => {
    scope.current += 1;
    setReview(null);
    setError(null);
    setBusy(false);
    pending.current = false;
    return () => { scope.current += 1; };
  }, [props.instanceId]);

  function errorMessage(cause: unknown): string {
    return cause instanceof CollectionRemovalError
      ? t(`servers.mods.collections.removalError.${cause.code === "local-metadata-missing" && props.moduleId === "terraria" ? "terraria-metadata" : cause.code}`, undefined, cause.message)
      : formatDesktopError(t, cause);
  }

  async function open(id: string) {
    if (pending.current) return;
    pending.current = true;
    const token = scope.current;
    setBusy(true);
    setError(null);
    try {
      const current = await props.readState();
      if (scope.current !== token) return;
      setReview({ ...buildWorkshopCollectionRemovalPreview({ settings: current.settings, moduleId: props.moduleId, collectionId: id }), settings: current.settings });
    } catch (cause) {
      if (scope.current === token) setError(errorMessage(cause));
    } finally {
      if (scope.current === token) { pending.current = false; setBusy(false); }
    }
  }

  async function remove(memberIds: string[]) {
    if (!review || pending.current) return;
    const token = scope.current;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      await props.runOperation(async () => {
        const current = await props.readState();
        if (scope.current !== token) return;
        if (changedSettingKeys(review.settings, current.settings, [...Object.keys(review.settings), ...Object.keys(current.settings)]).length) {
          throw new Error(t("servers.mods.collections.removeDialog.changed", undefined, "Instance settings changed. Close this review and reopen it before removing Mods."));
        }
        const pzSnapshot = memberIds.length && props.moduleId === "projectzomboid"
          ? await readProjectZomboidWorkshopModsSnapshot(props.instanceId, [...new Set([...parseWorkshopIdList(current.settings.workshop_items), ...memberIds])]) : null;
        const manualInventory = memberIds.length && props.moduleId === "palworld"
          ? await readManualModInventory(props.instanceId) : null;
        if (scope.current !== token) return;
        const plan = buildWorkshopCollectionRemovalPlan({ settings: current.settings, moduleId: props.moduleId,
          collectionId: review.collection.id, selectedMemberIds: memberIds, pzSnapshot, manualInventory });
        await props.save(plan.nextSettings, current.settings, plan.fileRemovalIds.length
          ? { collectionId: review.collection.id, memberIds: plan.fileRemovalIds } : undefined);
        if (scope.current !== token) return;
        props.onRemoved(review.collection.id);
        setReview(null);
      });
    } catch (cause) {
      if (scope.current === token) setError(errorMessage(cause));
    } finally {
      if (scope.current === token) { pending.current = false; setBusy(false); }
    }
  }

  return { review, busy, error, open, remove,
    blockMessage: review?.memberRemovalBlock ? errorMessage(new CollectionRemovalError(review.memberRemovalBlock)) : null,
    close: () => { if (!pending.current) { setReview(null); setError(null); } }
  };
}
