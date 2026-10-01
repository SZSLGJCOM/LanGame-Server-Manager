import { useCallback, useEffect, useRef, useState } from "react";
import type { AssistantExecuteOperationOutput } from "../types";
import type { AssistantConfirmationDecision } from "../assistant-workflow";

export function canContinueAssistantTask(preview: AssistantExecuteOperationOutput): boolean {
  return Boolean(preview.conversationId && preview.instanceId && preview.task?.instanceId === preview.instanceId
    && ["customize_config", "apply_beginner_config", "patch_instance_text", "patch_instance_files", "repair_ports", "start_server"].includes(preview.action));
}

export function assistantConfirmationExpired(preview: AssistantExecuteOperationOutput, now = Date.now()): boolean {
  const deadline = preview.confirmationExpiresAtUnixMs;
  return deadline != null && (!Number.isFinite(deadline) || deadline <= now);
}

export function useAssistantOperationConfirmation(scopeKey: string) {
  const [preview, setPreview] = useState<AssistantExecuteOperationOutput | null>(null);
  const mounted = useRef(false);
  const pending = useRef<{
    preview: AssistantExecuteOperationOutput;
    scopeKey: string;
    resolve: (confirmed: AssistantConfirmationDecision) => void;
  } | null>(null);

  const respond = useCallback((confirmed: boolean, continueTask = false) => {
    const request = pending.current;
    if (!request) return;
    pending.current = null;
    if (mounted.current) setPreview(null);
    const allowed = confirmed && !assistantConfirmationExpired(request.preview);
    request.resolve(allowed && continueTask && canContinueAssistantTask(request.preview)
      ? { confirmed: true, continueTask: true } : allowed);
  }, []);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      respond(false);
    };
  }, [respond]);

  useEffect(() => {
    if (pending.current && pending.current.scopeKey !== scopeKey) respond(false);
  }, [scopeKey, respond]);

  const confirmPreview = useCallback((next: AssistantExecuteOperationOutput): Promise<AssistantConfirmationDecision> => {
    if (!mounted.current || pending.current) return Promise.resolve(false);
    return new Promise((resolve) => {
      pending.current = { preview: next, scopeKey, resolve };
      setPreview(next);
    });
  }, [scopeKey]);

  return { preview, confirmPreview, respond };
}
