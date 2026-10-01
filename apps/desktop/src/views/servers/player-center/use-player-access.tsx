import { ActivityNotice } from "../../../components/ActivityNotice";
import { useEffect, useMemo, useRef, useState } from "react";
import { describeError } from "../../../app-state";
import { selectLocaleText, useI18n } from "../../../i18n";
import type {
  InstanceDetails,
  InstancePlayerAccessMutationInput,
  InstancePlayerAccessMutationResult,
  ModuleDetails
} from "../../../types";
import { parseSettingsObject } from "../../settings/guided-settings";
import type { SettingsObject } from "../../settings/settings-schema";
import {
  buildRosterFields,
  isRosterRecord,
  type RosterField
} from "./player-access-roster-model";
import {
  validateObjectRosterDraft,
  validateRosterEntryInput
} from "./player-access-roster-validation";

type PlayerAccessFeedbackTone = "neutral" | "success" | "warning" | "error";

interface PlayerAccessFeedback {
  message: string;
  tone: PlayerAccessFeedbackTone;
}

interface Snapshot {
  settings: SettingsObject;
  settingsError: string | null;
  rosterFields: RosterField[];
}

interface PlayerAccessOptions {
  details: InstanceDetails;
  moduleDetails: ModuleDetails | null;
  readOnly?: boolean;
  onApplyPlayerAccessMutation?: (
    input: InstancePlayerAccessMutationInput
  ) => Promise<InstancePlayerAccessMutationResult>;
}

export function usePlayerAccess(props: PlayerAccessOptions) {
  const { locale, t } = useI18n();
  const [feedback, setFeedback] = useState<PlayerAccessFeedback | null>(null);
  const [savingRosterKey, setSavingRosterKey] = useState<string | null>(null);
  const mutationRef = useRef(false);

  const snapshot = useMemo<Snapshot>(() => {
    const settingsResult = parseSettingsObject(props.details.settings_json, t);
    const settings = settingsResult.value ?? {};
    return {
      settings,
      settingsError: settingsResult.error,
      rosterFields: buildRosterFields(props.moduleDetails, locale, t, settings, { savedValuesOnly: props.readOnly })
    };
  }, [props.details.settings_json, props.moduleDetails, props.readOnly, locale, t]);

  useEffect(() => {
    setFeedback(null);
    setSavingRosterKey(null);
  }, [props.details.summary.id]);

  function showFeedback(message: string, tone: PlayerAccessFeedbackTone = "neutral") {
    setFeedback({ message, tone });
  }

  async function applyRosterMutation(
    field: RosterField,
    operation: "add" | "remove",
    rawValue: unknown
  ): Promise<boolean> {
    const value = typeof rawValue === "string" ? rawValue.trim() : rawValue;
    if (
      props.readOnly
      || !props.onApplyPlayerAccessMutation
      || (typeof value === "string" && !value)
      || value === null
      || value === undefined
      || snapshot.settingsError
      || mutationRef.current
    ) {
      return false;
    }

    if (operation === "add") {
      const validationError = field.kind === "object-list" && isRosterRecord(value)
        ? validateObjectRosterDraft(field, value, locale)
        : typeof value === "string"
          ? validateRosterEntryInput(field, value, locale)
          : selectLocaleText(locale, "名单条目格式无效。", "The roster entry has an invalid shape.");
      if (validationError) {
        showFeedback(validationError, "error");
        return false;
      }
    }

    mutationRef.current = true;
    setSavingRosterKey(field.key);
    showFeedback(selectLocaleText(locale, "正在保存并应用玩家访问规则...", "Saving and applying player access rule..."));
    try {
      const result = await props.onApplyPlayerAccessMutation({
        instanceId: props.details.summary.id,
        fieldKey: field.key,
        operation,
        value,
        expectedValue: field.kind === "string-scalar" ? field.currentValue : undefined
      });
      const persistentText = result.persistentStatus === "updated"
        ? selectLocaleText(locale, "持久名单已更新", "Persistent roster updated")
        : selectLocaleText(locale, "持久名单已是目标状态", "Persistent roster already matched");
      if (result.liveStatus === "failed") {
        showFeedback(selectLocaleText(
          locale,
          `${persistentText}；即时同步失败：${result.liveError || "未知错误"}。下次启动仍会生效。`,
          `${persistentText}; live sync failed: ${result.liveError || "Unknown error"}. The change will still apply on the next start.`
        ), "warning");
      } else if (result.liveStatus === "restart_required" || result.liveStatus === "not_running") {
        showFeedback(selectLocaleText(
          locale,
          `${persistentText}；将在服务器下次启动时生效。`,
          `${persistentText}; applies on the next server start.`
        ), "success");
      } else if (result.verificationStatus === "failed") {
        showFeedback(selectLocaleText(
          locale,
          `${persistentText}；即时同步已发送，但状态核验失败：${result.verificationError || "未返回可确认的名单结果"}。`,
          `${persistentText}; live sync was sent, but state verification failed: ${result.verificationError || "No conclusive roster response was returned"}.`
        ), "warning");
      } else if (result.verificationStatus === "verified") {
        showFeedback(selectLocaleText(locale, `${persistentText}，已即时生效。`, `${persistentText}; applied live.`), "success");
      } else {
        showFeedback(selectLocaleText(
          locale,
          `${persistentText}，并已发送即时同步。`,
          `${persistentText}; live synchronization sent.`
        ), "warning");
      }
      return true;
    } catch (error) {
      showFeedback(describeError(error), "error");
      return false;
    } finally {
      mutationRef.current = false;
      setSavingRosterKey(null);
    }
  }

  return {
    fields: snapshot.rosterFields,
    busyFieldKey: savingRosterKey,
    disabled: Boolean(props.readOnly || !props.onApplyPlayerAccessMutation || snapshot.settingsError),
    onMutate: applyRosterMutation,
    feedback: <>
      {snapshot.settingsError ? <ActivityNotice tone="error">{snapshot.settingsError}</ActivityNotice> : null}
      {feedback ? <ActivityNotice tone={feedback.tone === "neutral" ? "info" : feedback.tone} onDismiss={() => setFeedback(null)}>
        {feedback.message}
      </ActivityNotice> : null}
    </>
  };
}

export type PlayerAccessState = ReturnType<typeof usePlayerAccess>;
